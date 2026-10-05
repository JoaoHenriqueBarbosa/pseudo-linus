//! `sysctl` do procps-ng 4.0.4: leitura e escrita dos parâmetros de `/proc/sys`.
//!
//! Leitura de chaves (`kernel.ostype` ou `kernel/ostype`), `-a` (`-A`, `-X`) com a árvore inteira
//! em ordem de nome, `-n`, `-N`, `-b`, `-e`, `-q`, escrita com `-w` ou `chave=valor`, `--dry-run`,
//! `-p [arquivo]` (padrão `/etc/sysctl.conf`, `-` é a entrada padrão) e `--system`. As mensagens são
//! as do original, inclusive a diferença de aspas entre `permission denied on key 'x'` (leitura) e
//! `permission denied on key "x"` (escrita). O `-r` (expressão regular) não está portado e cai no
//! erro de opção inválida.

use std::ffi::OsString;

use sysabi::{Errno, FileType, OFlags, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::out;

const PROC_PATH: &str = "/proc/sys/";
const DEFAULT_PRELOAD: &str = "/etc/sysctl.conf";

const OPT_DEPRECATED: i32 = 256;
const OPT_SYSTEM: i32 = 257;
const OPT_DRYRUN: i32 = 258;

const USAGE: &str = "\nUsage:\n sysctl [options] [variable[=value] ...]\n\nOptions:\n  -a, --all            display all variables\n  -A                   alias of -a\n  -X                   alias of -a\n      --deprecated     include deprecated parameters to listing\n      --dry-run        Print the key and values but do not write\n  -b, --binary         print value without new line\n  -e, --ignore         ignore unknown variables errors\n  -N, --names          print variable names without values\n  -n, --values         print only values of the given variable(s)\n  -p, --load[=<file>]  read values from file\n  -f                   alias of -p\n      --system         read values from all system directories\n  -r, --pattern <expression>\n                       select setting that match expression\n  -q, --quiet          do not echo variable set\n  -w, --write          enable writing a value to variable\n  -o                   does nothing\n  -x                   does nothing\n  -d                   alias of -h\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see sysctl(8).\n";

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, 'a' as i32),
    LongOpt::new("deprecated", HasArg::No, OPT_DEPRECATED),
    LongOpt::new("dry-run", HasArg::No, OPT_DRYRUN),
    LongOpt::new("binary", HasArg::No, 'b' as i32),
    LongOpt::new("ignore", HasArg::No, 'e' as i32),
    LongOpt::new("names", HasArg::No, 'N' as i32),
    LongOpt::new("values", HasArg::No, 'n' as i32),
    LongOpt::new("load", HasArg::Optional, 'p' as i32),
    LongOpt::new("quiet", HasArg::No, 'q' as i32),
    LongOpt::new("write", HasArg::No, 'w' as i32),
    LongOpt::new("system", HasArg::No, OPT_SYSTEM),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

/// Diretórios do `--system`, na ordem de prioridade do original (o primeiro com um nome ganha).
const SYSTEM_DIRS: &[&str] = &["/etc/sysctl.d/", "/run/sysctl.d/", "/usr/local/lib/sysctl.d/", "/usr/lib/sysctl.d/", "/lib/sysctl.d/"];

/// Chaves que o `-a` esconde sem `--deprecated`.
const DEPRECATED: &[&str] = &["base_reachable_time", "retrans_time"];

fn warn(msg: &str) {
    crate::common::warn("sysctl", msg);
}

fn warn_errno(msg: &str, e: Errno) {
    crate::common::warn("sysctl", &format!("{msg}: {}", e.message()));
}

/// Estado das opções (as variáveis globais do original).
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    pub print_name: bool,
    pub print_newline: bool,
    pub ignore_error: bool,
    pub name_only: bool,
    pub quiet: bool,
    pub dry_run: bool,
    pub ignore_deprecated: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            print_name: true,
            print_newline: true,
            ignore_error: false,
            name_only: false,
            quiet: false,
            dry_run: false,
            ignore_deprecated: true,
        }
    }
}

/// `slashdot`: troca `old` por `new` (e `new` por `old`) a partir do primeiro separador, a não ser
/// que o primeiro separador já seja `new`. Separador repetido gera um aviso, uma vez.
pub fn slashdot(s: &str, old: char, new: char) -> String {
    let b: Vec<char> = s.chars().collect();
    let Some(first) = b.iter().position(|c| *c == '/' || *c == '.') else { return s.to_string() };
    if b[first] == new {
        return s.to_string();
    }
    let mut out = b.clone();
    let mut warned = false;
    for i in first..b.len() {
        let c = b[i];
        if c != '/' && c != '.' {
            continue;
        }
        if !warned && matches!(b.get(i + 1), Some('/') | Some('.')) {
            let rest: String = out[i..].iter().collect();
            warn(&format!("separators should not be repeated: {rest}"));
            warned = true;
        }
        if c == old {
            out[i] = new;
        } else if c == new {
            out[i] = old;
        }
    }
    out.into_iter().collect()
}

/// Normalização léxica do caminho (o `realpath` do `is_proc_path`, sem os links simbólicos, que
/// `/proc/sys` não tem).
fn under_proc_sys(path: &str) -> bool {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.len() >= 2 && parts[0] == "proc" && parts[1] == "sys"
}

fn is_dir(st: &sysabi::Stat) -> bool {
    FileType::from_mode(st.mode) == FileType::Directory
}

/// Lê uma chave (`ReadSetting`); devolve o código a somar no de saída.
fn read_setting(name: &str, o: &Opts) -> i32 {
    if name.is_empty() {
        warn(&format!("\"{name}\" is an unknown key"));
        return -1;
    }
    let tmpname = format!("{PROC_PATH}{}", slashdot(name, '.', '/'));
    let outname = slashdot(name, '/', '.');
    let st = match sys::stat(tmpname.as_bytes()) {
        Ok(st) => st,
        Err(e) => {
            if o.ignore_error {
                return 0;
            }
            warn_errno(&format!("cannot stat {tmpname}"), e);
            return 1;
        }
    };
    if !under_proc_sys(&tmpname) {
        warn(&format!("Path is not under {PROC_PATH}: {tmpname}"));
        return -1;
    }
    if st.mode & 0o400 == 0 {
        return 0;
    }
    if is_dir(&st) {
        return display_all(&format!("{tmpname}/"), o);
    }
    if o.name_only {
        out(format!("{outname}\n"));
        return 0;
    }
    match sys::read_file(tmpname.as_bytes()) {
        Ok(data) => {
            if data.is_empty() {
                return 0;
            }
            let mut s = Vec::new();
            if o.print_name {
                s.extend_from_slice(format!("{outname} = ").as_bytes());
                s.extend_from_slice(&data);
                if data.last() != Some(&b'\n') {
                    s.push(b'\n');
                }
            } else {
                // Sem o nome, o original imprime só a primeira linha (o `fgets` único), e sem o
                // `\n` no `-b`.
                let first = match data.iter().position(|b| *b == b'\n') {
                    Some(p) => &data[..=p],
                    None => &data[..],
                };
                if o.print_newline {
                    s.extend_from_slice(first);
                } else {
                    s.extend_from_slice(first.strip_suffix(b"\n").unwrap_or(first));
                }
            }
            out(s);
            0
        }
        Err(Errno::ENOENT) => {
            if o.ignore_error {
                return 0;
            }
            warn(&format!("\"{outname}\" is an unknown key"));
            1
        }
        Err(Errno::EACCES) => {
            warn(&format!("permission denied on key '{outname}'"));
            1
        }
        Err(Errno::EIO) => 1,
        Err(Errno::EISDIR) => display_all(&format!("{tmpname}/"), o),
        Err(e) => {
            warn_errno(&format!("reading key \"{outname}\""), e);
            1
        }
    }
}

/// `DisplayAll`: a árvore a partir de `path` (que termina em `/`), entradas em ordem de nome.
fn display_all(path: &str, o: &Opts) -> i32 {
    let mut entries = match sys::read_dir(path.as_bytes()) {
        Ok(v) => v,
        Err(_) => {
            warn(&format!("unable to open directory \"{path}\""));
            return 1;
        }
    };
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut rc = 0;
    for (n, e) in entries.iter().enumerate() {
        if n % 64 == 0 {
            sys::checkpoint();
        }
        let name = String::from_utf8_lossy(&e.name).into_owned();
        if o.ignore_deprecated && DEPRECATED.contains(&name.as_str()) {
            continue;
        }
        let full = format!("{path}{name}");
        match sys::stat(full.as_bytes()) {
            Err(err) => warn_errno(&format!("cannot stat {full}"), err),
            Ok(st) if is_dir(&st) => {
                display_all(&format!("{full}/"), o);
            }
            Ok(_) => rc |= read_setting(&full[PROC_PATH.len()..], o),
        }
    }
    rc
}

/// Uma linha `chave = valor` já separada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    /// Chave com `.` (como sai na tela).
    pub key: String,
    /// Caminho em `/proc/sys`.
    pub path: String,
    pub value: String,
    /// Linha começando com `-`: falha de escrita não conta.
    pub ignore_failure: bool,
}

/// `parse_setting_line`: espaços em volta da chave e do valor somem; linha vazia ou comentário
/// (`#`, `;`) não é configuração. `Err` traz a mensagem de erro.
pub fn parse_setting_line(source: &str, lineno: usize, line: &str) -> Result<Option<Setting>, String> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
        return Ok(None);
    }
    let (ignore_failure, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let Some((key, value)) = t.split_once('=') else {
        return Err(format!("{source}({lineno}): invalid syntax, continuing..."));
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("malformed setting \"{}\"", line.trim_end_matches('\n')));
    }
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    Ok(Some(Setting {
        key: slashdot(key, '/', '.'),
        path: format!("{PROC_PATH}{}", slashdot(key, '.', '/')),
        value: value.to_string(),
        ignore_failure,
    }))
}

/// Escreve uma chave (`WriteSetting`).
fn write_setting(s: &Setting, o: &Opts) -> i32 {
    let key = &s.key;
    let st = match sys::stat(s.path.as_bytes()) {
        Ok(st) => st,
        Err(e) => {
            if o.ignore_error || s.ignore_failure {
                return 0;
            }
            warn_errno(&format!("cannot stat {}", s.path), e);
            return 1;
        }
    };
    if !under_proc_sys(&s.path) {
        warn(&format!("Path is not under {PROC_PATH}: {}", s.path));
        return 1;
    }
    if st.mode & 0o200 == 0 || is_dir(&st) {
        // Root de contêiner: o oráculo recebe EPERM do kernel e o procps 4.0.4 sai com 0.
        let (e, rc) = if is_dir(&st) { (Errno::EISDIR, 1) } else { (Errno::EPERM, 0) };
        warn_errno(&format!("setting key \"{key}\""), e);
        return rc;
    }
    let mut rc = 0;
    if !o.dry_run {
        match sys::open(s.path.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o666) {
            Err(e) => {
                rc = if s.ignore_failure { 0 } else { 1 };
                match e {
                    Errno::ENOENT => {
                        if o.ignore_error {
                            rc = 0;
                        } else {
                            warn(&format!("\"{key}\" is an unknown key"));
                        }
                    }
                    Errno::EPERM | Errno::EROFS | Errno::EACCES => warn(&format!("permission denied on key \"{key}\"")),
                    other => warn_errno(&format!("setting key \"{key}\""), other),
                }
            }
            Ok(fd) => {
                let line = format!("{}\n", s.value);
                let w = sys::write_all(fd, line.as_bytes());
                let c = sys::close(fd);
                if let Err(e) = w.and(c) {
                    warn_errno(&format!("setting key \"{key}\""), e);
                    rc = if s.ignore_failure { 0 } else { 1 };
                }
            }
        }
    }
    if (rc == 0 && !o.quiet) || o.dry_run {
        if o.name_only {
            out(format!("{key}\n"));
        } else if o.print_name {
            out(format!("{key} = {}\n", s.value));
        } else if o.print_newline {
            out(format!("{}\n", s.value));
        } else {
            out(s.value.as_bytes());
        }
    }
    rc
}

/// `Preload`: aplica as linhas de um arquivo (`-` ou vazio é a entrada padrão).
fn preload(filename: &str, o: &Opts) -> i32 {
    let data = if filename.is_empty() || filename == "-" {
        sys::read_to_end(sysabi::Fd::STDIN)
    } else {
        sys::read_file(filename.as_bytes())
    };
    let data = match data {
        Ok(d) => d,
        Err(e) => {
            warn_errno(&format!("cannot open \"{filename}\""), e);
            return 1;
        }
    };
    let mut rc = 0;
    for (i, line) in String::from_utf8_lossy(&data).lines().enumerate() {
        match parse_setting_line(filename, i + 1, line) {
            Ok(None) => {}
            Ok(Some(s)) => rc |= write_setting(&s, o),
            Err(msg) => {
                warn(&msg);
                rc |= 1;
            }
        }
    }
    rc
}

/// `--system`: os `*.conf` dos diretórios do sistema (o primeiro diretório com um nome ganha), em
/// ordem de nome, e por fim `/etc/sysctl.conf`.
fn preload_system(o: &Opts) -> i32 {
    let mut files: Vec<(Vec<u8>, String)> = Vec::new();
    for dir in SYSTEM_DIRS {
        let Ok(entries) = sys::read_dir(dir.as_bytes()) else { continue };
        for e in entries {
            if !e.name.ends_with(b".conf") || files.iter().any(|(n, _)| *n == e.name) {
                continue;
            }
            files.push((e.name.clone(), format!("{dir}{}", String::from_utf8_lossy(&e.name))));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut rc = 0;
    for (_, path) in &files {
        if !o.quiet {
            out(format!("* Applying {path} ...\n"));
        }
        rc |= preload(path, o);
    }
    if sys::stat(DEFAULT_PRELOAD.as_bytes()).is_ok() {
        if !o.quiet {
            out(format!("* Applying {DEFAULT_PRELOAD} ...\n"));
        }
        rc |= preload(DEFAULT_PRELOAD, o);
    }
    rc
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut o = Opts::default();
    let mut write_mode = false;
    let mut display_all_opt = false;
    let mut preload_opt = false;
    let mut system_opt = false;
    let mut preload_files: Vec<String> = Vec::new();
    let mut g = Getopt::from_env(&argv[1..], "bneNwfp::qoxaAXVdh", LONGS);
    while let Some(r) = g.next_opt() {
        match r {
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
            Ok(opt) => match opt.id {
                OPT_DEPRECATED => o.ignore_deprecated = false,
                OPT_DRYRUN => o.dry_run = true,
                OPT_SYSTEM => system_opt = true,
                id => match u8::try_from(id).map(char::from).unwrap_or('\0') {
                    'b' => {
                        o.print_name = false;
                        o.print_newline = false;
                    }
                    'e' => o.ignore_error = true,
                    'N' => o.name_only = true,
                    'n' => o.print_name = false,
                    'w' => write_mode = true,
                    'f' | 'p' => {
                        preload_opt = true;
                        if let Some(a) = &opt.arg {
                            preload_files.push(String::from_utf8_lossy(a).into_owned());
                        }
                    }
                    'q' => o.quiet = true,
                    'o' | 'x' => {}
                    'a' | 'A' | 'X' => display_all_opt = true,
                    'V' => {
                        out("sysctl from procps-ng 4.0.4\n");
                        return 0;
                    }
                    'd' | 'h' => {
                        out(USAGE);
                        return 0;
                    }
                    _ => unreachable!("tabela de opções do sysctl"),
                },
            },
        }
    }
    let operands: Vec<String> = g.operands().iter().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    if display_all_opt {
        return display_all(PROC_PATH, &o);
    }
    if system_opt {
        return preload_system(&o);
    }
    if preload_opt {
        // Com `-p`, os operandos também são arquivos; sem nenhum, o padrão.
        preload_files.extend(operands);
        if preload_files.is_empty() {
            preload_files.push(DEFAULT_PRELOAD.to_string());
        }
        let mut rc = 0;
        for f in &preload_files {
            rc |= preload(f, &o);
        }
        return rc;
    }
    // O `xerrx` do original usa o `program_invocation_short_name`, não o argv[0].
    if operands.is_empty() {
        io::eprint(USAGE);
        return 1;
    }
    if o.name_only && o.quiet {
        warn("options -N and -q cannot coexist\nTry `sysctl --help' for more information.");
        return 1;
    }
    let mut rc: i32 = 0;
    for a in &operands {
        if write_mode || a.contains('=') {
            match parse_setting_line("command line", 0, a) {
                Ok(Some(s)) => rc |= write_setting(&s, &o),
                Ok(None) => {}
                Err(msg) => {
                    warn(&msg);
                    rc |= 1;
                }
            }
        } else {
            rc = rc.wrapping_add(read_setting(a, &o));
        }
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slashdot_conversion() {
        assert_eq!(slashdot("kernel.ostype", '.', '/'), "kernel/ostype");
        assert_eq!(slashdot("kernel/ostype", '.', '/'), "kernel/ostype");
        assert_eq!(slashdot("net.ipv4.conf.eth0/1.rp_filter", '.', '/'), "net/ipv4/conf/eth0.1/rp_filter");
        assert_eq!(slashdot("net/ipv4/conf/eth0.1/rp_filter", '/', '.'), "net.ipv4.conf.eth0/1.rp_filter");
        assert_eq!(slashdot("x", '.', '/'), "x");
    }

    #[test]
    fn setting_lines() {
        let s = parse_setting_line("f", 1, "  kernel.pid_max =  4096  ").unwrap().unwrap();
        assert_eq!(s.key, "kernel.pid_max");
        assert_eq!(s.path, "/proc/sys/kernel/pid_max");
        assert_eq!(s.value, "4096");
        assert!(!s.ignore_failure);
        assert!(parse_setting_line("f", 2, "# x").unwrap().is_none());
        assert!(parse_setting_line("f", 3, "-a.b=1").unwrap().unwrap().ignore_failure);
        assert_eq!(parse_setting_line("f", 4, "abc").unwrap_err(), "f(4): invalid syntax, continuing...");
    }

    #[test]
    fn proc_path_check() {
        assert!(under_proc_sys("/proc/sys/kernel/ostype"));
        assert!(!under_proc_sys("/proc/sys/../../etc/passwd"));
    }
}
