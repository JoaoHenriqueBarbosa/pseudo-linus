//! Builtins de uso geral.

use std::sync::Arc;

use sysabi::{Fd, Resource, Rlimit, RLIM_INFINITY};

use super::{Arg, opt_error, out, parse_int, parse_opts};
use crate::ast::List;
use crate::exec::TextKind;
use crate::shell::{Exec, Flow, Shell, sys, write_fd};

/// Palavras reservadas (pro `type` e `command -v`).
pub const KEYWORDS: &[&str] = &[
    "!", "[[", "]]", "case", "coproc", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in", "select", "then",
    "time", "until", "while", "{", "}",
];

pub fn echo(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut newline = true;
    let mut escapes = sh.opts.shopt("xpg_echo");
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a.len() < 2 || a[0] != b'-' || !a[1..].iter().all(|c| matches!(c, b'n' | b'e' | b'E')) {
            break;
        }
        for c in &a[1..] {
            match c {
                b'n' => newline = false,
                b'e' => escapes = true,
                _ => escapes = false,
            }
        }
        i += 1;
    }
    let mut data = Vec::new();
    for (k, a) in argv[i..].iter().enumerate() {
        if k > 0 {
            data.push(b' ');
        }
        if escapes {
            let (v, stop) = crate::quote::decode_echo(a);
            data.extend(v);
            if stop {
                return Ok(if out(sh, "echo", &data) { 0 } else { 1 });
            }
        } else {
            data.extend_from_slice(a);
        }
    }
    if newline {
        data.push(b'\n');
    }
    Ok(if out(sh, "echo", &data) { 0 } else { 1 })
}

struct ShellPrintfEnv {
    now: i64,
    start: i64,
}

impl crate::printf::PrintfEnv for ShellPrintfEnv {
    fn now(&self) -> i64 {
        self.now
    }

    fn shell_start(&self) -> i64 {
        self.start
    }

    fn localtime(&self, t: i64) -> Option<crate::printf::Tm> {
        Some(crate::timefmt::utc_tm(t))
    }
}

pub fn printf(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut i = 1;
    let mut var: Option<Vec<u8>> = None;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"--" {
            i += 1;
            break;
        }
        if a == b"-v" {
            match argv.get(i + 1) {
                Some(v) => var = Some(v.clone()),
                None => {
                    sh.builtin_error("printf", "-v: option requires an argument");
                    let _ = write_fd(Fd::STDERR, b"printf: usage: printf [-v var] format [arguments]\n");
                    return Ok(2);
                }
            }
            i += 2;
            continue;
        }
        if a.starts_with(b"-v") && a.len() > 2 {
            var = Some(a[2..].to_vec());
            i += 1;
            continue;
        }
        if a.len() > 1 && a[0] == b'-' {
            sh.builtin_error("printf", format!("{}: invalid option", String::from_utf8_lossy(&a[..2])));
            let _ = write_fd(Fd::STDERR, b"printf: usage: printf [-v var] format [arguments]\n");
            return Ok(2);
        }
        break;
    }
    let Some(fmt) = argv.get(i) else {
        let _ = write_fd(Fd::STDERR, b"printf: usage: printf [-v var] format [arguments]\n");
        return Ok(2);
    };
    let now = sys().clock_gettime(sysabi::Clock::Realtime).map(|t| t.sec).unwrap_or(0);
    let env = ShellPrintfEnv { now, start: sh.start_secs };
    let res = crate::printf::printf(fmt, &argv[i + 1..], &env);
    for e in &res.errors {
        let mut msg = b"printf: ".to_vec();
        msg.extend_from_slice(e);
        sh.error_bytes(&msg);
    }
    match var {
        Some(v) => {
            let name = String::from_utf8_lossy(&v).into_owned();
            let (base, idx) = match name.find('[') {
                Some(b) if name.ends_with(']') => (name[..b].to_string(), Some(name[b + 1..name.len() - 1].to_string())),
                _ => (name.clone(), None),
            };
            if !crate::word::is_name(base.as_bytes()) {
                sh.builtin_error("printf", format!("`{name}': not a valid identifier"));
                return Ok(2);
            }
            let ok = match idx {
                Some(i) => {
                    let w = crate::word::make_word(&i, crate::word::WordOpts::mode(crate::word::Mode::Subscript, sh.lineno))
                        .map_err(|_| Flow::Discard)?;
                    let key = sh.expand_word_string(&w)?;
                    sh.assign_element(&base, &key, res.out, false)?
                }
                None => sh.assign_scalar(&base, res.out, false)?,
            };
            Ok(if ok { res.status } else { 1 })
        }
        None => {
            if !out(sh, "printf", &res.out) {
                return Ok(1);
            }
            Ok(res.status)
        }
    }
}

/// Status do `exit`/`return`: argumento numérico (módulo 256) ou o `$?`.
fn exit_status(sh: &Shell, name: &str, argv: &[Vec<u8>]) -> Result<i32, i32> {
    match argv.get(1) {
        None => Ok(sh.status),
        Some(a) => {
            let a = if a == b"--" { match argv.get(2) { Some(x) => x, None => return Ok(sh.status) } } else { a };
            match parse_int(a) {
                Some(n) => {
                    if argv.len() > 2 && argv[1] != b"--" {
                        sh.builtin_error(name, "too many arguments");
                        return Err(1);
                    }
                    Ok((n & 0xff) as i32)
                }
                None => {
                    sh.builtin_error(name, format!("{}: numeric argument required", String::from_utf8_lossy(a)));
                    Err(2)
                }
            }
        }
    }
}

pub fn exit(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let st = match exit_status(sh, "exit", argv) {
        Ok(s) => s,
        Err(s) => s,
    };
    Err(Flow::Exit(st))
}

pub fn return_(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    if !sh.in_function() && sh.source_depth == 0 {
        sh.builtin_error("return", "can only `return' from a function or sourced script");
        return Ok(2);
    }
    match exit_status(sh, "return", argv) {
        Ok(s) => Err(Flow::Return(s)),
        Err(2) => Err(Flow::Return(2)),
        Err(s) => Ok(s),
    }
}

pub fn break_continue(sh: &mut Shell, name: &str, argv: &[Vec<u8>]) -> Exec {
    let n = match argv.get(1) {
        None => 1,
        Some(a) => match parse_int(a) {
            Some(n) if n >= 1 => n as u32,
            Some(_) => {
                sh.builtin_error(name, format!("{}: loop count out of range", String::from_utf8_lossy(a)));
                if sh.loop_depth == 0 {
                    return Ok(1);
                }
                sh.status = 1;
                return Err(Flow::Break(sh.loop_depth));
            }
            None => {
                sh.builtin_error(name, format!("{}: numeric argument required", String::from_utf8_lossy(a)));
                if sh.loop_depth == 0 {
                    return Ok(1);
                }
                return Err(Flow::Break(sh.loop_depth));
            }
        },
    };
    if sh.loop_depth == 0 {
        sh.builtin_error(name, "only meaningful in a `for', `while', or `until' loop");
        return Ok(0);
    }
    let n = n.min(sh.loop_depth);
    if name == "break" { Err(Flow::Break(n)) } else { Err(Flow::Continue(n)) }
}

pub fn shift(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    if sh.dash_style() {
        // O `shiftcmd` do dash: `number` só aceita dígitos e o excesso é `sh_error`, que encerra o
        // shell não interativo com status 2.
        let fatal = |sh: &mut Shell, msg: String| {
            sh.builtin_error("shift", msg);
            if sh.interactive { Ok(2) } else { Err(Flow::Exit(2)) }
        };
        let n = match argv.get(1) {
            None => 1,
            Some(a) => match std::str::from_utf8(a).ok().filter(|t| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit())).and_then(|t| t.parse::<usize>().ok()) {
                Some(n) => n,
                None => return fatal(sh, format!("Illegal number: {}", String::from_utf8_lossy(a))),
            },
        };
        if n > sh.params.len() {
            return fatal(sh, "can't shift that many".to_string());
        }
        sh.params.drain(..n);
        return Ok(0);
    }
    let n = match argv.get(1) {
        None => 1,
        Some(a) => match parse_int(a) {
            Some(n) => n,
            None => {
                sh.builtin_error("shift", format!("{}: numeric argument required", String::from_utf8_lossy(a)));
                return Ok(1);
            }
        },
    };
    if n < 0 {
        sh.builtin_error("shift", format!("{n}: shift count out of range"));
        return Ok(1);
    }
    let n = n as usize;
    if n > sh.params.len() {
        if sh.opts.shopt("shift_verbose") {
            sh.builtin_error("shift", format!("{n}: shift count out of range"));
        }
        return Ok(1);
    }
    sh.params.drain(..n);
    Ok(0)
}

pub fn eval(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let args: Vec<&[u8]> = argv[1..].iter().map(|a| a.as_slice()).skip_while(|a| *a == b"--").collect();
    if args.is_empty() {
        return Ok(0);
    }
    let text = String::from_utf8_lossy(&args.join(&b' ')).into_owned();
    let line = sh.lineno;
    let r = sh.run_text(&text, TextKind::Eval, Arc::from("eval"), line);
    sh.lineno = line;
    r
}

/// Acha o arquivo do `source`: com `/` usa direto; senão PATH (sourcepath) e depois o diretório
/// corrente (fora do modo POSIX).
fn find_source(sh: &Shell, name: &[u8]) -> Option<Vec<u8>> {
    if name.contains(&b'/') {
        return Some(name.to_vec());
    }
    if sh.opts.shopt("sourcepath") {
        let path = sh.var_bytes("PATH").map(|p| p.to_vec()).unwrap_or_default();
        for dir in path.split(|c| *c == b':') {
            let mut cand = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
            cand.push(b'/');
            cand.extend_from_slice(name);
            if let Ok(st) = sys().fstatat(Fd::CWD, &cand, sysabi::AtFlags::empty())
                && st.file_type() != sysabi::FileType::Directory {
                    return Some(cand);
                }
        }
    }
    if sh.posix {
        return None;
    }
    Some(name.to_vec())
}

pub fn source(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let name_s = String::from_utf8_lossy(&argv[0]).into_owned();
    let mut i = 1;
    if argv.get(1).is_some_and(|a| a == b"--") {
        i = 2;
    }
    let Some(file) = argv.get(i) else {
        sh.builtin_error(&name_s, "filename argument required");
        let _ = write_fd(Fd::STDERR, format!("{name_s}: usage: {name_s} filename [arguments]\n").as_bytes());
        return Ok(2);
    };
    let path = match find_source(sh, file) {
        Some(p) => p,
        None => {
            sh.error_bytes(&[file.as_slice(), b": file not found"].concat());
            return Ok(1);
        }
    };
    let data = match sysabi::sys::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            sh.error_bytes(&[file.as_slice(), b": ", e.message().as_bytes()].concat());
            return Ok(1);
        }
    };
    if data.iter().take(80).any(|c| *c == 0) {
        sh.error_bytes(&[file.as_slice(), b": cannot execute binary file"].concat());
        return Ok(126);
    }
    let text = String::from_utf8_lossy(&data).into_owned();
    let shown: Arc<str> = Arc::from(String::from_utf8_lossy(file).as_ref());
    let saved_params = if argv.len() > i + 1 { Some(std::mem::replace(&mut sh.params, argv[i + 1..].to_vec())) } else { None };
    sh.source_stack.push(shown.clone());
    sh.source_depth += 1;
    let saved_line = sh.lineno;
    let r = sh.run_text(&text, TextKind::Source, shown, 1);
    sh.lineno = saved_line;
    sh.source_depth -= 1;
    sh.source_stack.pop();
    if let Some(p) = saved_params {
        sh.params = p;
    }
    match r {
        Err(Flow::Return(n)) => Ok(n),
        other => other,
    }
}

pub fn exec(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "cla:", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "exec", e, "exec [-cl] [-a name] [command [argument ...]] [redirection ...]")),
    };
    let rest = &argv[opts.rest..];
    if rest.is_empty() {
        return Ok(0);
    }
    let name = rest[0].clone();
    let path = if name.contains(&b'/') { Some(name.clone()) } else { sh.search_path(&name) };
    let Some(path) = path else {
        sh.builtin_error("exec", format!("{}: not found", String::from_utf8_lossy(&name)));
        return if sh.interactive || sh.opts.shopt("execfail") { Ok(127) } else { Err(Flow::Exit(127)) };
    };
    let mut new_argv = rest.to_vec();
    if let Some(a0) = opts.value(b'a') {
        new_argv[0] = a0.to_vec();
    }
    if opts.has(b'l') {
        let mut a0 = b"-".to_vec();
        a0.extend_from_slice(&new_argv[0]);
        new_argv[0] = a0;
    }
    sh.lower_shlvl_for_exec();
    let env = if opts.has(b'c') { Vec::new() } else { sh.export_env() };
    // Fds de cópia do shell não vão pro programa novo (são CLOEXEC); traps capturadas voltam ao
    // padrão no execve.
    let e = sys().execve(&path, &new_argv, Some(&env));
    let shown = String::from_utf8_lossy(&path).into_owned();
    if e == sysabi::Errno::ENOEXEC {
        sh.builtin_error("exec", format!("{shown}: cannot execute: Exec format error"));
    } else if let Ok(st) = sys().fstatat(Fd::CWD, &path, sysabi::AtFlags::empty()) {
        if st.file_type() == sysabi::FileType::Directory {
            sh.builtin_error("exec", format!("{shown}: Is a directory"));
        } else {
            sh.builtin_error("exec", format!("{shown}: {}", e.message()));
        }
    } else {
        sh.builtin_error("exec", format!("{shown}: {}", e.message()));
    }
    let st = if e == sysabi::Errno::ENOENT { 127 } else { 126 };
    if sh.interactive || sh.opts.shopt("execfail") { Ok(st) } else { Err(Flow::Exit(st)) }
}

/// `command [-pVv] nome [args]`.
pub fn command(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "pvV", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "command", e, "command [-pVv] command [arg ...]")),
    };
    let rest = &argv[opts.rest..];
    if opts.has(b'v') || opts.has(b'V') {
        let verbose = opts.has(b'V');
        let mut status = 0;
        for name in rest {
            let found = describe(sh, name, if verbose { DescribeMode::Verbose } else { DescribeMode::Short }, false, false);
            if !found {
                status = 1;
                if verbose {
                    sh.builtin_error("command", format!("{}: not found", String::from_utf8_lossy(name)));
                }
            }
        }
        return Ok(status);
    }
    if rest.is_empty() {
        return Ok(0);
    }
    let name = String::from_utf8_lossy(&rest[0]).into_owned();
    if super::is_builtin(sh, &name) {
        let args: Vec<Arg> = rest.iter().map(|a| Arg::Word(a.clone())).collect();
        return super::run(sh, &name, &args);
    }
    let path = if rest[0].contains(&b'/') {
        Some(rest[0].clone())
    } else if opts.has(b'p') {
        default_path_search(&rest[0])
    } else {
        sh.find_in_path(&rest[0], true)
    };
    match path {
        Some(p) => sh.run_program(&p, rest),
        None => {
            sh.error(format!("{name}: command not found"));
            Ok(127)
        }
    }
}

fn default_path_search(name: &[u8]) -> Option<Vec<u8>> {
    for dir in [&b"/usr/local/bin"[..], b"/usr/bin", b"/bin", b"/usr/sbin", b"/sbin"] {
        let mut cand = dir.to_vec();
        cand.push(b'/');
        cand.extend_from_slice(name);
        if let Ok(st) = sys().fstatat(Fd::CWD, &cand, sysabi::AtFlags::empty())
            && st.file_type() != sysabi::FileType::Directory && st.mode & 0o111 != 0 {
                return Some(cand);
            }
    }
    None
}

pub fn builtin(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let Some(name) = argv.get(1) else { return Ok(0) };
    let n = String::from_utf8_lossy(name).into_owned();
    if !super::is_builtin(sh, &n) {
        sh.builtin_error("builtin", format!("{n}: not a shell builtin"));
        return Ok(1);
    }
    let args: Vec<Arg> = argv[1..].iter().map(|a| Arg::Word(a.clone())).collect();
    super::run(sh, &n, &args)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DescribeMode {
    /// `command -v`
    Short,
    /// `type` / `command -V`
    Verbose,
    /// `type -t`
    Kind,
    /// `type -p` / `-P`
    Path,
}

/// Descreve um nome (alias, palavra reservada, função, builtin, arquivo). Devolve se achou.
fn describe(sh: &mut Shell, name: &[u8], mode: DescribeMode, all: bool, force_path: bool) -> bool {
    let n = String::from_utf8_lossy(name).into_owned();
    let mut found = false;
    let emit = |sh: &Shell, s: String| {
        out(sh, "type", s.as_bytes());
    };
    if !force_path {
        if let Some(v) = sh.aliases.get(&n).cloned() {
            found = true;
            match mode {
                DescribeMode::Short => emit(sh, format!("alias {n}={}\n", String::from_utf8_lossy(&crate::quote::single_quote(v.as_bytes())))),
                DescribeMode::Verbose => emit(sh, format!("{n} is aliased to `{v}'\n")),
                DescribeMode::Kind => emit(sh, "alias\n".to_string()),
                DescribeMode::Path => {}
            }
            if !all {
                return true;
            }
        }
        if KEYWORDS.contains(&n.as_str()) {
            found = true;
            match mode {
                DescribeMode::Short => emit(sh, format!("{n}\n")),
                DescribeMode::Verbose => emit(sh, format!("{n} is a shell keyword\n")),
                DescribeMode::Kind => emit(sh, "keyword\n".to_string()),
                DescribeMode::Path => {}
            }
            if !all {
                return true;
            }
        }
        if let Some(f) = sh.funcs.get(&n).cloned() {
            found = true;
            match mode {
                DescribeMode::Short => emit(sh, format!("{n}\n")),
                DescribeMode::Verbose => emit(sh, format!("{n} is a function\n{}\n", crate::print::function_text(&f))),
                DescribeMode::Kind => emit(sh, "function\n".to_string()),
                DescribeMode::Path => {}
            }
            if !all {
                return true;
            }
        }
        if super::is_builtin(sh, &n) {
            found = true;
            match mode {
                DescribeMode::Short => emit(sh, format!("{n}\n")),
                DescribeMode::Verbose => emit(sh, format!("{n} is a shell builtin\n")),
                DescribeMode::Kind => emit(sh, "builtin\n".to_string()),
                DescribeMode::Path => {}
            }
            if !all {
                return true;
            }
        }
    }
    if name.contains(&b'/') {
        let ok = sys().fstatat(Fd::CWD, name, sysabi::AtFlags::empty()).is_ok_and(|st| st.mode & 0o111 != 0 && st.file_type() != sysabi::FileType::Directory);
        if ok {
            match mode {
                DescribeMode::Short | DescribeMode::Path => emit(sh, format!("{n}\n")),
                DescribeMode::Verbose => emit(sh, format!("{n} is {n}\n")),
                DescribeMode::Kind => emit(sh, "file\n".to_string()),
            }
            return true;
        }
        return found;
    }
    // Arquivo: hash primeiro (só se não for -a), depois PATH.
    if !all
        && let Some((p, _)) = sh.hash.get(&n).cloned() {
            let ps = String::from_utf8_lossy(&p).into_owned();
            match mode {
                DescribeMode::Short | DescribeMode::Path => emit(sh, format!("{ps}\n")),
                DescribeMode::Verbose => emit(sh, format!("{n} is hashed ({ps})\n")),
                DescribeMode::Kind => emit(sh, "file\n".to_string()),
            }
            return true;
        }
    let paths: Vec<Vec<u8>> = if all { all_in_path(sh, name) } else { sh.search_path(name).into_iter().collect() };
    for p in paths {
        if !is_exec(&p) && !all {
            // O bash ainda mostra o arquivo sem permissão quando não há outro.
        }
        found = true;
        let ps = String::from_utf8_lossy(&p).into_owned();
        match mode {
            DescribeMode::Short | DescribeMode::Path => emit(sh, format!("{ps}\n")),
            DescribeMode::Verbose => emit(sh, format!("{n} is {ps}\n")),
            DescribeMode::Kind => emit(sh, "file\n".to_string()),
        }
        if !all {
            break;
        }
    }
    found
}

fn is_exec(p: &[u8]) -> bool {
    sys().fstatat(Fd::CWD, p, sysabi::AtFlags::empty()).is_ok_and(|st| st.mode & 0o111 != 0)
}

fn all_in_path(sh: &Shell, name: &[u8]) -> Vec<Vec<u8>> {
    let path = sh.var_bytes("PATH").map(|p| p.to_vec()).unwrap_or_default();
    let mut out = Vec::new();
    for dir in path.split(|c| *c == b':') {
        let mut cand = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
        cand.push(b'/');
        cand.extend_from_slice(name);
        if let Ok(st) = sys().fstatat(Fd::CWD, &cand, sysabi::AtFlags::empty())
            && st.file_type() != sysabi::FileType::Directory && st.mode & 0o111 != 0 && !out.contains(&cand) {
                out.push(cand);
            }
    }
    out
}

pub fn type_(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "afptP", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "type", e, "type [-afptP] name [name ...]")),
    };
    let mode = if opts.has(b't') {
        DescribeMode::Kind
    } else if opts.has(b'p') || opts.has(b'P') {
        DescribeMode::Path
    } else {
        DescribeMode::Verbose
    };
    let all = opts.has(b'a');
    let force_path = opts.has(b'P');
    let mut status = 0;
    for name in &argv[opts.rest..] {
        let found = if force_path {
            match sh.search_path(name) {
                Some(p) => {
                    out(sh, "type", &[p.as_slice(), b"\n"].concat());
                    true
                }
                None => false,
            }
        } else {
            describe(sh, name, mode, all, false)
        };
        if !found {
            status = 1;
            if mode == DescribeMode::Verbose {
                sh.builtin_error("type", format!("{}: not found", String::from_utf8_lossy(name)));
            }
        }
    }
    Ok(status)
}

pub fn hash(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "rdltp:", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "hash", e, "hash [-lr] [-p pathname] [-dt] [name ...]")),
    };
    let names = &argv[opts.rest..];
    if opts.has(b'r') {
        sh.hash.clear();
    }
    if let Some(p) = opts.value(b'p') {
        for n in names {
            sh.hash.insert(String::from_utf8_lossy(n).into_owned(), (p.to_vec(), 0));
        }
        return Ok(0);
    }
    if opts.has(b'd') {
        let mut st = 0;
        for n in names {
            if sh.hash.remove(String::from_utf8_lossy(n).as_ref()).is_none() {
                sh.builtin_error("hash", format!("{}: not found", String::from_utf8_lossy(n)));
                st = 1;
            }
        }
        return Ok(st);
    }
    if opts.has(b't') {
        let mut st = 0;
        let many = names.len() > 1;
        for n in names {
            match sh.hash.get(String::from_utf8_lossy(n).as_ref()) {
                Some((p, _)) => {
                    let line = if many {
                        format!("{}\t{}\n", String::from_utf8_lossy(n), String::from_utf8_lossy(p))
                    } else {
                        format!("{}\n", String::from_utf8_lossy(p))
                    };
                    out(sh, "hash", line.as_bytes());
                }
                None => {
                    sh.builtin_error("hash", format!("{}: not found", String::from_utf8_lossy(n)));
                    st = 1;
                }
            }
        }
        return Ok(st);
    }
    if names.is_empty() {
        if opts.has(b'r') {
            return Ok(0);
        }
        if sh.hash.is_empty() {
            out(sh, "hash", b"hash: hash table empty\n");
            return Ok(0);
        }
        let mut text = String::new();
        if opts.has(b'l') {
            for (k, (p, _)) in &sh.hash {
                text.push_str(&format!("builtin hash -p {} {k}\n", String::from_utf8_lossy(p)));
            }
        } else {
            text.push_str("hits\tcommand\n");
            for (p, hits) in sh.hash.values() {
                text.push_str(&format!("{hits:4}\t{}\n", String::from_utf8_lossy(p)));
            }
        }
        out(sh, "hash", text.as_bytes());
        return Ok(0);
    }
    let mut st = 0;
    for n in names {
        let key = String::from_utf8_lossy(n).into_owned();
        if n.contains(&b'/') || super::is_builtin(sh, &key) && !n.contains(&b'/') && false {
            continue;
        }
        if super::is_builtin(sh, &key) {
            continue;
        }
        match sh.search_path(n) {
            Some(p) => {
                sh.hash.insert(key, (p, 0));
            }
            None => {
                sh.builtin_error("hash", format!("{key}: not found"));
                st = 1;
            }
        }
    }
    Ok(st)
}

pub fn enable(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "adnpsf:", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "enable", e, "enable [-a] [-dnps] [-f filename] [name ...]")),
    };
    let names = &argv[opts.rest..];
    if names.is_empty() {
        let mut text = String::new();
        for b in super::BUILTINS {
            let disabled = sh.disabled_builtins.contains(*b);
            let show = if opts.has(b'a') { true } else if opts.has(b'n') { disabled } else { !disabled };
            if show {
                text.push_str(&format!("enable {}{b}\n", if disabled { "-n " } else { "" }));
            }
        }
        out(sh, "enable", text.as_bytes());
        return Ok(0);
    }
    let mut st = 0;
    for n in names {
        let key = String::from_utf8_lossy(n).into_owned();
        if !super::BUILTINS.contains(&key.as_str()) {
            sh.builtin_error("enable", format!("{key}: not a shell builtin"));
            st = 1;
            continue;
        }
        if opts.has(b'n') {
            sh.disabled_builtins.insert(key);
        } else {
            sh.disabled_builtins.remove(&key);
        }
    }
    Ok(st)
}

/// Descrições curtas do `help -d` (as do bash 5.2).
const HELP: &[(&str, &str, &str)] = &[
    (":", ": [arguments]", "Null command."),
    (".", ". filename [arguments]", "Execute commands from a file in the current shell."),
    ("[", "[ arg... ]", "Evaluate conditional expression."),
    ("alias", "alias [-p] [name[=value] ... ]", "Define or display aliases."),
    ("bg", "bg [job_spec ...]", "Move jobs to the background."),
    ("break", "break [n]", "Exit for, while, or until loops."),
    ("builtin", "builtin [shell-builtin [arg ...]]", "Execute shell builtins."),
    ("caller", "caller [expr]", "Return the context of the current subroutine call."),
    ("cd", "cd [-L|[-P [-e]] [-@]] [dir]", "Change the shell working directory."),
    ("command", "command [-pVv] command [arg ...]", "Execute a simple command or display information about commands."),
    ("compgen", "compgen [-abcdefgjksuv] [-o option] [-A action] [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat] [-P prefix] [-S suffix] [word]", "Display possible completions depending on the options."),
    ("continue", "continue [n]", "Resume for, while, or until loops."),
    ("declare", "declare [-aAfFgiIlnrtux] [name[=value] ...] or declare -p [-aAfFilnrtux] [name ...]", "Set variable values and attributes."),
    ("dirs", "dirs [-clpv] [+N] [-N]", "Display directory stack."),
    ("disown", "disown [-h] [-ar] [jobspec ... | pid ...]", "Remove jobs from current shell."),
    ("echo", "echo [-neE] [arg ...]", "Write arguments to the standard output."),
    ("enable", "enable [-a] [-dnps] [-f filename] [name ...]", "Enable and disable shell builtins."),
    ("eval", "eval [arg ...]", "Execute arguments as a shell command."),
    ("exec", "exec [-cl] [-a name] [command [argument ...]] [redirection ...]", "Replace the shell with the given command."),
    ("exit", "exit [n]", "Exit the shell."),
    ("export", "export [-fn] [name[=value] ...] or export -p", "Set export attribute for shell variables."),
    ("false", "false", "Return an unsuccessful result."),
    ("fg", "fg [job_spec]", "Move job to the foreground."),
    ("getopts", "getopts optstring name [arg ...]", "Parse option arguments."),
    ("hash", "hash [-lr] [-p pathname] [-dt] [name ...]", "Remember or display program locations."),
    ("help", "help [-dms] [pattern ...]", "Display information about builtin commands."),
    ("jobs", "jobs [-lnprs] [jobspec ...] or jobs -x command [args]", "Display status of jobs."),
    ("kill", "kill [-s sigspec | -n signum | -sigspec] pid | jobspec ... or kill -l [sigspec]", "Send a signal to a job."),
    ("let", "let arg [arg ...]", "Evaluate arithmetic expressions."),
    ("local", "local [option] name[=value] ...", "Define local variables."),
    ("logout", "logout [n]", "Exit a login shell."),
    ("mapfile", "mapfile [-d delim] [-n count] [-O origin] [-s count] [-t] [-u fd] [-C callback] [-c quantum] [array]", "Read lines from the standard input into an indexed array variable."),
    ("popd", "popd [-n] [+N | -N]", "Remove directories from stack."),
    ("printf", "printf [-v var] format [arguments]", "Formats and prints ARGUMENTS under control of the FORMAT."),
    ("pushd", "pushd [-n] [+N | -N | dir]", "Add directories to stack."),
    ("pwd", "pwd [-LP]", "Print the name of the current working directory."),
    ("read", "read [-ers] [-a array] [-d delim] [-i text] [-n nchars] [-N nchars] [-p prompt] [-t timeout] [-u fd] [name ...]", "Read a line from the standard input and split it into fields."),
    ("readarray", "readarray [-d delim] [-n count] [-O origin] [-s count] [-t] [-u fd] [-C callback] [-c quantum] [array]", "Read lines from a file into an array variable."),
    ("readonly", "readonly [-aAf] [name[=value] ...] or readonly -p", "Mark shell variables as unchangeable."),
    ("return", "return [n]", "Return from a shell function."),
    ("set", "set [-abefhkmnptuvxBCEHPT] [-o option-name] [--] [-] [arg ...]", "Set or unset values of shell options and positional parameters."),
    ("shift", "shift [n]", "Shift positional parameters."),
    ("shopt", "shopt [-pqsu] [-o] [optname ...]", "Set and unset shell options."),
    ("source", "source filename [arguments]", "Execute commands from a file in the current shell."),
    ("suspend", "suspend [-f]", "Suspend shell execution."),
    ("test", "test [expr]", "Evaluate conditional expression."),
    ("times", "times", "Display process times."),
    ("trap", "trap [-lp] [[arg] signal_spec ...]", "Trap signals and other events."),
    ("true", "true", "Return a successful result."),
    ("type", "type [-afptP] name [name ...]", "Display information about command type."),
    ("typeset", "typeset [-aAfFgiIlnrtux] name[=value] ... or typeset -p [-aAfFilnrtux] [name ...]", "Set variable values and attributes."),
    ("ulimit", "ulimit [-SHabcdefiklmnpqrstuvxPRT] [limit]", "Modify shell resource limits."),
    ("umask", "umask [-p] [-S] [mode]", "Display or set file mode mask."),
    ("unalias", "unalias [-a] name [name ...]", "Remove each NAME from the list of defined aliases."),
    ("unset", "unset [-f] [-v] [-n] [name ...]", "Unset values and attributes of shell variables and functions."),
    ("wait", "wait [-fn] [-p var] [id ...]", "Wait for job completion and return exit status."),
];

pub fn help(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "dms", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "help", e, "help [-dms] [pattern ...]")),
    };
    let pats = &argv[opts.rest..];
    let mut text = String::new();
    if pats.is_empty() {
        text.push_str("GNU bash, version 5.2.37(1)-release (x86_64-pc-linux-gnu)\n");
        text.push_str("These shell commands are defined internally.  Type `help' to see this list.\n");
        text.push_str("Type `help name' to find out more about the function `name'.\n\n");
        for (_, usage, _) in HELP {
            text.push_str(&format!(" {usage}\n"));
        }
        out(sh, "help", text.as_bytes());
        return Ok(0);
    }
    let mut status = 0;
    for p in pats {
        let pat = crate::pattern::Pattern::new(p, sh.match_opts(false));
        let mut any = false;
        for (name, usage, desc) in HELP {
            if pat.matches(name.as_bytes()) {
                any = true;
                if opts.has(b'd') {
                    text.push_str(&format!("{name} - {desc}\n"));
                } else if opts.has(b's') {
                    text.push_str(&format!("{name}: {usage}\n"));
                } else {
                    text.push_str(&format!("{name}: {usage}\n    {desc}\n"));
                }
            }
        }
        if !any {
            sh.builtin_error("help", format!("no help topics match `{}'.  Try `help help' or `man -k {}' or `info {}'.", String::from_utf8_lossy(p), String::from_utf8_lossy(p), String::from_utf8_lossy(p)));
            status = 1;
        }
    }
    out(sh, "help", text.as_bytes());
    Ok(status)
}

fn fmt_tm(d: std::time::Duration) -> String {
    let ms = d.as_millis() as u64;
    format!("{}m{}.{:03}s", ms / 60_000, (ms / 1000) % 60, ms % 1000)
}

pub fn times(sh: &mut Shell, _argv: &[Vec<u8>]) -> Exec {
    let s = sys();
    let me = s.getrusage(sysabi::RusageWho::SelfProcess).unwrap_or_default();
    let ch = s.getrusage(sysabi::RusageWho::Children).unwrap_or_default();
    let text = format!("{} {}\n{} {}\n", fmt_tm(me.utime), fmt_tm(me.stime), fmt_tm(ch.utime), fmt_tm(ch.stime));
    Ok(if out(sh, "times", text.as_bytes()) { 0 } else { 1 })
}

pub fn caller(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let n = match argv.get(1) {
        None => None,
        Some(a) => match parse_int(a) {
            Some(n) if n >= 0 => Some(n as usize),
            _ => {
                sh.builtin_error("caller", format!("{}: invalid number", String::from_utf8_lossy(a)));
                return Ok(2);
            }
        },
    };
    if sh.frames.is_empty() {
        return Ok(1);
    }
    let k = n.unwrap_or(0);
    if k >= sh.frames.len() {
        return Ok(1);
    }
    let idx = sh.frames.len() - 1 - k;
    let line = sh.frames[idx].call_line;
    let caller_name = if idx == 0 { "main".to_string() } else { sh.frames[idx - 1].name.clone() };
    let file = if idx == 0 {
        sh.source_stack.last().map(|s| s.to_string()).unwrap_or_else(|| "NULL".to_string())
    } else {
        sh.frames[idx - 1].source.to_string()
    };
    let file = if file.is_empty() { "NULL".to_string() } else { file };
    let text = if n.is_none() { format!("{line} {file}\n") } else { format!("{line} {caller_name} {file}\n") };
    out(sh, "caller", text.as_bytes());
    Ok(0)
}

pub fn let_(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    if argv.len() < 2 {
        sh.builtin_error("let", "expression expected");
        return Ok(1);
    }
    let mut last = 0;
    for a in &argv[1..] {
        match sh.arith_eval_prefixed(a, "let") {
            Ok(v) => last = v,
            Err(r) => return r,
        }
    }
    Ok(if last != 0 { 0 } else { 1 })
}

pub fn alias(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "p", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "alias", e, "alias [-p] [name[=value] ... ]")),
    };
    let args = &argv[opts.rest..];
    let show = |name: &str, value: &str| format!("alias {name}={}\n", String::from_utf8_lossy(&crate::quote::single_quote(value.as_bytes())));
    if args.is_empty() || opts.has(b'p') {
        let mut names: Vec<&String> = sh.aliases.keys().collect();
        names.sort();
        let text: String = names.iter().map(|n| show(n, &sh.aliases[*n])).collect();
        out(sh, "alias", text.as_bytes());
        if args.is_empty() {
            return Ok(0);
        }
    }
    let mut status = 0;
    for a in args {
        let s = String::from_utf8_lossy(a).into_owned();
        match s.find('=') {
            Some(eq) => {
                let name = &s[..eq];
                if name.is_empty() || name.contains(['/', '$', '`', '=', ' ', '\t', '\'', '"', '\\']) {
                    sh.builtin_error("alias", format!("`{name}': invalid alias name"));
                    status = 1;
                    continue;
                }
                Arc::make_mut(&mut sh.aliases).insert(name.to_string(), s[eq + 1..].to_string());
                sh.parse_generation += 1;
            }
            None => match sh.aliases.get(&s) {
                Some(v) => {
                    let line = show(&s, v);
                    out(sh, "alias", line.as_bytes());
                }
                None => {
                    sh.builtin_error("alias", format!("{s}: not found"));
                    status = 1;
                }
            },
        }
    }
    Ok(status)
}

pub fn unalias(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "a", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "unalias", e, "unalias [-a] name [name ...]")),
    };
    if opts.has(b'a') {
        Arc::make_mut(&mut sh.aliases).clear();
        sh.parse_generation += 1;
        return Ok(0);
    }
    let args = &argv[opts.rest..];
    if args.is_empty() {
        let _ = write_fd(Fd::STDERR, b"unalias: usage: unalias [-a] name [name ...]\n");
        return Ok(2);
    }
    let mut status = 0;
    for a in args {
        let s = String::from_utf8_lossy(a).into_owned();
        if Arc::make_mut(&mut sh.aliases).remove(&s).is_none() {
            sh.builtin_error("unalias", format!("{s}: not found"));
            status = 1;
        } else {
            sh.parse_generation += 1;
        }
    }
    Ok(status)
}

fn symbolic_umask(mask: u32) -> String {
    let perm = !mask & 0o777;
    let part = |shift: u32| {
        let b = (perm >> shift) & 7;
        let mut s = String::new();
        if b & 4 != 0 {
            s.push('r');
        }
        if b & 2 != 0 {
            s.push('w');
        }
        if b & 1 != 0 {
            s.push('x');
        }
        s
    };
    format!("u={},g={},o={}", part(6), part(3), part(0))
}

/// Aplica um modo simbólico (`u=rwx,g+w`) a uma máscara.
fn apply_symbolic(mask: u32, spec: &str) -> Option<u32> {
    let mut perm = !mask & 0o777;
    for clause in spec.split(',') {
        let b = clause.as_bytes();
        let mut i = 0;
        let mut who = 0u32;
        while i < b.len() && matches!(b[i], b'u' | b'g' | b'o' | b'a') {
            who |= match b[i] {
                b'u' => 0o700,
                b'g' => 0o070,
                b'o' => 0o007,
                _ => 0o777,
            };
            i += 1;
        }
        if who == 0 {
            who = 0o777;
        }
        if i >= b.len() {
            return None;
        }
        while i < b.len() {
            let op = b[i];
            if !matches!(op, b'+' | b'-' | b'=') {
                return None;
            }
            i += 1;
            let mut bits = 0u32;
            while i < b.len() && matches!(b[i], b'r' | b'w' | b'x') {
                bits |= match b[i] {
                    b'r' => 0o444,
                    b'w' => 0o222,
                    _ => 0o111,
                };
                i += 1;
            }
            let bits = bits & who;
            match op {
                b'+' => perm |= bits,
                b'-' => perm &= !bits,
                _ => perm = (perm & !who) | bits,
            }
        }
    }
    Some(!perm & 0o777)
}

pub fn umask(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "pS", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "umask", e, "umask [-p] [-S] [mode]")),
    };
    let s = sys();
    let current = s.umask(0);
    s.umask(current);
    let args = &argv[opts.rest..];
    if args.is_empty() {
        let text = if opts.has(b'S') {
            format!("{}{}\n", if opts.has(b'p') { "umask -S " } else { "" }, symbolic_umask(current))
        } else {
            format!("{}{:04o}\n", if opts.has(b'p') { "umask " } else { "" }, current)
        };
        out(sh, "umask", text.as_bytes());
        return Ok(0);
    }
    let spec = String::from_utf8_lossy(&args[0]).into_owned();
    let new = if spec.bytes().all(|c| c.is_ascii_digit()) {
        match u32::from_str_radix(&spec, 8) {
            Ok(v) if v <= 0o777 && spec.bytes().all(|c| c < b'8') => v,
            _ => {
                sh.builtin_error("umask", format!("{spec}: octal number out of range"));
                return Ok(1);
            }
        }
    } else {
        match apply_symbolic(current, &spec) {
            Some(v) => v,
            None => {
                sh.builtin_error("umask", format!("`{}': invalid symbolic mode operator", spec.chars().find(|c| !"ugoarwx".contains(*c)).unwrap_or(' ')));
                return Ok(1);
            }
        }
    };
    s.umask(new);
    if opts.has(b'S') {
        out(sh, "umask", format!("{}\n", symbolic_umask(new)).as_bytes());
    }
    Ok(0)
}

/// Tabela do `ulimit`: (letra, recurso, descrição, unidade em bytes ou 1).
const ULIMITS: &[(u8, Resource, &str, u64)] = &[
    (b'c', Resource::Core, "core file size              (blocks, -c)", 512),
    (b'd', Resource::Data, "data seg size               (kbytes, -d)", 1024),
    (b'e', Resource::Nice, "scheduling priority                 (-e)", 1),
    (b'f', Resource::Fsize, "file size                   (blocks, -f)", 512),
    (b'i', Resource::Sigpending, "pending signals                     (-i)", 1),
    (b'l', Resource::Memlock, "max locked memory           (kbytes, -l)", 1024),
    (b'm', Resource::Rss, "max memory size             (kbytes, -m)", 1024),
    (b'n', Resource::Nofile, "open files                          (-n)", 1),
    (b'p', Resource::Nofile, "pipe size                (512 bytes, -p)", 0),
    (b'q', Resource::Msgqueue, "POSIX message queues         (bytes, -q)", 1),
    (b'r', Resource::Rtprio, "real-time priority                  (-r)", 1),
    (b's', Resource::Stack, "stack size                  (kbytes, -s)", 1024),
    (b't', Resource::Cpu, "cpu time                   (seconds, -t)", 1),
    (b'u', Resource::Nproc, "max user processes                  (-u)", 1),
    (b'v', Resource::As, "virtual memory              (kbytes, -v)", 1024),
    (b'x', Resource::Locks, "file locks                          (-x)", 1),
];

pub fn ulimit(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let s = sys();
    let mut hard = false;
    let mut soft = false;
    let mut all = false;
    let mut which: Vec<u8> = Vec::new();
    let mut value: Option<Vec<u8>> = None;
    for a in &argv[1..] {
        if a.len() > 1 && a[0] == b'-' {
            for c in &a[1..] {
                match c {
                    b'H' => hard = true,
                    b'S' => soft = true,
                    b'a' => all = true,
                    c if ULIMITS.iter().any(|u| u.0 == *c) => which.push(*c),
                    _ => {
                        sh.builtin_error("ulimit", format!("-{}: invalid option", *c as char));
                        let _ = write_fd(Fd::STDERR, b"ulimit: usage: ulimit [-SHabcdefiklmnpqrstuvxPRT] [limit]\n");
                        return Ok(2);
                    }
                }
            }
        } else {
            value = Some(a.clone());
        }
    }
    let show_hard = hard && !soft;
    let fmt = |l: Rlimit, unit: u64| -> String {
        let v = if show_hard { l.max } else { l.cur };
        if unit == 0 {
            return "8".to_string();
        }
        if v == RLIM_INFINITY { "unlimited".to_string() } else { (v / unit).to_string() }
    };
    if all {
        let mut text = String::new();
        for (_, res, desc, unit) in ULIMITS {
            let l = s.getrlimit(*res).unwrap_or(Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY });
            text.push_str(&format!("{desc} {}\n", fmt(l, *unit)));
        }
        out(sh, "ulimit", text.as_bytes());
        return Ok(0);
    }
    if which.is_empty() {
        which.push(b'f');
    }
    match value {
        None => {
            let mut text = String::new();
            for c in &which {
                let (_, res, desc, unit) = ULIMITS.iter().find(|u| u.0 == *c).copied().unwrap_or(ULIMITS[3]);
                let l = s.getrlimit(res).unwrap_or(Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY });
                if which.len() > 1 {
                    text.push_str(&format!("{desc} {}\n", fmt(l, unit)));
                } else {
                    text.push_str(&format!("{}\n", fmt(l, unit)));
                }
            }
            out(sh, "ulimit", text.as_bytes());
            Ok(0)
        }
        Some(v) => {
            let c = which[0];
            let (_, res, _, unit) = ULIMITS.iter().find(|u| u.0 == c).copied().unwrap_or(ULIMITS[3]);
            let cur = s.getrlimit(res).unwrap_or(Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY });
            let n = if v == b"unlimited" {
                RLIM_INFINITY
            } else if v == b"hard" {
                cur.max
            } else if v == b"soft" {
                cur.cur
            } else {
                match parse_int(&v) {
                    Some(n) if n >= 0 => (n as u64).saturating_mul(unit.max(1)),
                    _ => {
                        sh.builtin_error("ulimit", format!("{}: invalid number", String::from_utf8_lossy(&v)));
                        return Ok(1);
                    }
                }
            };
            let mut new = cur;
            if hard || !soft {
                new.max = n;
            }
            if soft || !hard {
                new.cur = n;
            }
            if let Err(e) = s.setrlimit(res, new) {
                let (_, _, desc, _) = ULIMITS.iter().find(|u| u.0 == c).copied().unwrap_or(ULIMITS[3]);
                let short = desc.split('(').next().unwrap_or("").trim_end();
                sh.builtin_error("ulimit", format!("{short}: cannot modify limit: {}", e.message()));
                return Ok(1);
            }
            Ok(0)
        }
    }
}

/// `compgen` mínimo: -c, -b, -k, -v, -f, -d, -a, -A function, -W lista; filtra pelo prefixo.
pub fn compgen(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "abcdefgjksuvA:W:G:P:S:X:o:F:C:", false) {
        Ok(o) => o,
        Err(e) => return Ok(opt_error(sh, "compgen", e, "compgen [-abcdefgjksuv] [-o option] [-A action] [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat] [-P prefix] [-S suffix] [word]")),
    };
    let prefix = argv.get(opts.rest).cloned().unwrap_or_default();
    let mut cands: Vec<Vec<u8>> = Vec::new();
    let mut actions: Vec<String> = Vec::new();
    for (c, v) in &opts.flags {
        match c {
            b'a' => actions.push("alias".into()),
            b'b' => actions.push("builtin".into()),
            b'c' => actions.push("command".into()),
            b'd' => actions.push("directory".into()),
            b'e' => actions.push("export".into()),
            b'f' => actions.push("file".into()),
            b'k' => actions.push("keyword".into()),
            b'v' => actions.push("variable".into()),
            b'A' => actions.push(String::from_utf8_lossy(v.as_deref().unwrap_or_default()).into_owned()),
            _ => {}
        }
    }
    for a in &actions {
        match a.as_str() {
            "alias" => cands.extend(sh.aliases.keys().map(|k| k.clone().into_bytes())),
            "builtin" => cands.extend(super::BUILTINS.iter().map(|b| b.as_bytes().to_vec())),
            "keyword" => cands.extend(KEYWORDS.iter().map(|k| k.as_bytes().to_vec())),
            "function" => cands.extend(sh.funcs.keys().map(|k| k.clone().into_bytes())),
            "variable" => cands.extend(sh.all_var_names().into_iter().map(String::into_bytes)),
            "export" => cands.extend(sh.export_env().into_iter().map(|e| e.split(|c| *c == b'=').next().unwrap_or(&[]).to_vec())),
            "command" => {
                cands.extend(sh.aliases.keys().map(|k| k.clone().into_bytes()));
                cands.extend(super::BUILTINS.iter().map(|b| b.as_bytes().to_vec()));
                cands.extend(sh.funcs.keys().map(|k| k.clone().into_bytes()));
                cands.extend(KEYWORDS.iter().map(|k| k.as_bytes().to_vec()));
                let path = sh.var_bytes("PATH").map(|p| p.to_vec()).unwrap_or_default();
                for dir in path.split(|c| *c == b':') {
                    if let Ok(entries) = sysabi::sys::read_dir(if dir.is_empty() { b"." } else { dir }) {
                        cands.extend(entries.into_iter().map(|e| e.name));
                    }
                }
            }
            "file" | "directory" => {
                let (dir, base) = match prefix.iter().rposition(|c| *c == b'/') {
                    Some(p) => (prefix[..=p].to_vec(), prefix[p + 1..].to_vec()),
                    None => (Vec::new(), prefix.clone()),
                };
                let _ = base;
                if let Ok(entries) = sysabi::sys::read_dir(if dir.is_empty() { b"." } else { &dir }) {
                    for e in entries {
                        if a == "directory" && e.kind != sysabi::FileType::Directory {
                            continue;
                        }
                        let mut full = dir.clone();
                        full.extend(e.name);
                        cands.push(full);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(w) = opts.value(b'W') {
        let ifs = sh.ifs();
        cands.extend(crate::expand::ifs_split(w, &ifs, 0));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut text = Vec::new();
    let pre = opts.value(b'P').unwrap_or_default().to_vec();
    let suf = opts.value(b'S').unwrap_or_default().to_vec();
    let mut any = false;
    for c in cands {
        if !c.starts_with(&prefix) || !seen.insert(c.clone()) {
            continue;
        }
        any = true;
        text.extend_from_slice(&pre);
        text.extend(c);
        text.extend_from_slice(&suf);
        text.push(b'\n');
    }
    out(sh, "compgen", &text);
    Ok(if any { 0 } else { 1 })
}

/// `select nome in palavras; do ...; done`: menu no stderr, resposta do stdin.
pub fn select_loop(sh: &mut Shell, var: &str, items: Vec<Vec<u8>>, body: &List) -> Exec {
    if items.is_empty() {
        return Ok(0);
    }
    let mut status = 0;
    sh.loop_depth += 1;
    let r = (|| loop {
        let mut menu = Vec::new();
        let width = items.len().to_string().len();
        for (i, it) in items.iter().enumerate() {
            menu.extend_from_slice(format!("{:>width$}) ", i + 1).as_bytes());
            menu.extend_from_slice(it);
            menu.push(b'\n');
        }
        let ps3 = sh.var_bytes("PS3").map(|v| v.to_vec()).unwrap_or_else(|| b"#? ".to_vec());
        menu.extend(ps3);
        let _ = write_fd(Fd::STDERR, &menu);
        let line = match super::read::read_line(sh, Fd::STDIN, b'\n') {
            Some(l) => l,
            None => {
                let _ = write_fd(Fd::STDERR, b"\n");
                return Ok(1);
            }
        };
        let reply = line.clone();
        sh.assign_scalar("REPLY", reply.clone(), false)?;
        if reply.is_empty() {
            continue;
        }
        let choice = parse_int(&reply).filter(|n| *n >= 1 && (*n as usize) <= items.len());
        let value = choice.map(|n| items[n as usize - 1].clone()).unwrap_or_default();
        sh.assign_scalar(var, value, false)?;
        match sh.exec_list(body) {
            Ok(st) => status = st,
            Err(Flow::Break(n)) => {
                if n > 1 {
                    return Err(Flow::Break(n - 1));
                }
                return Ok(status);
            }
            Err(Flow::Continue(n)) => {
                if n > 1 {
                    return Err(Flow::Continue(n - 1));
                }
            }
            Err(f) => return Err(f),
        }
    })();
    sh.loop_depth -= 1;
    r
}

impl Shell {
    /// Roda um programa externo (usado pelo `command`).
    pub fn run_program(&mut self, path: &[u8], argv: &[Vec<u8>]) -> Exec {
        let env = self.export_env();
        let spec = sysabi::SpawnSpec {
            path: path.to_vec(),
            argv: argv.to_vec(),
            attrs: sysabi::ProcAttrs { env: Some(env), reset_signals: self.trapped_signals(), ..sysabi::ProcAttrs::default() },
        };
        match sys().spawn(spec) {
            Ok(pid) => Ok(self.wait_pid(pid)),
            Err(e) => {
                let shown = String::from_utf8_lossy(path).into_owned();
                if e == sysabi::Errno::ENOENT {
                    self.error(format!("{shown}: No such file or directory"));
                    Ok(127)
                } else {
                    self.error(format!("{shown}: {}", e.message()));
                    Ok(126)
                }
            }
        }
    }
}
