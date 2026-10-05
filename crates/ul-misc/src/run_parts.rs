//! `run-parts` do debianutils 5.23 (Debian 13): executa os programas de um ou mais diretórios, em ordem
//! alfabética.
//!
//! Porte do `run-parts.c`: lista os nomes válidos (`^[a-zA-Z0-9_-]+$`, ou `--lsbsysinit`, ou
//! `--regex`), ordena por bytes e tira repetidos, resolve cada nome no primeiro diretório que o tem e
//! então lista (`--test`, `--list`) ou executa (`--arg`, `--verbose`, `--report`, `--stdin`,
//! `--new-session`, `--exit-on-error`, `--umask`). O código de saída é o do último programa que falhou.

use std::ffi::OsString;
use std::io::Write;

use regex_posix::{Regex, Syntax};
use sysabi::{
    AccessMode, AtFlags, Ctx, Errno, Fd, FdAction, FileType, OFlags, PollEvents, PollFd, ProcAttrs,
    SpawnSpec, WaitOptions, WaitStatus, WaitTarget, Whence, sys,
};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const O_TEST: i32 = 1001;
const O_LIST: i32 = 1002;
const O_REPORT: i32 = 1003;
const O_REVERSE: i32 = 1004;
const O_LSBSYSINIT: i32 = 1005;
const O_REGEX: i32 = 1006;
const O_STDIN: i32 = 1007;
const O_EXIT_ON_ERROR: i32 = 1008;
const O_NEW_SESSION: i32 = 1009;

/// A tabela de opções longas, na ordem do original (a ordem decide as mensagens de ambiguidade).
const LONGS: &[LongOpt] = &[
    LongOpt::new("test", HasArg::No, O_TEST),
    LongOpt::new("list", HasArg::No, O_LIST),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("debug", HasArg::No, b'd' as i32),
    LongOpt::new("report", HasArg::No, O_REPORT),
    LongOpt::new("reverse", HasArg::No, O_REVERSE),
    LongOpt::new("umask", HasArg::Required, b'u' as i32),
    LongOpt::new("arg", HasArg::Required, b'a' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("lsbsysinit", HasArg::No, O_LSBSYSINIT),
    LongOpt::new("regex", HasArg::Required, O_REGEX),
    LongOpt::new("stdin", HasArg::No, O_STDIN),
    LongOpt::new("exit-on-error", HasArg::No, O_EXIT_ON_ERROR),
    LongOpt::new("new-session", HasArg::No, O_NEW_SESSION),
];

const USAGE: &str = "Usage: run-parts [OPTION]... DIRECTORY [DIRECTORY ...]
      --test          print script names which would run, but don't run them.
      --list          print names of all valid files (can not be used with
                      --test)
  -v, --verbose       print script names before running them.
  -d, --debug         print script names while checking them.
      --report        print script names if they produce output.
      --reverse       reverse execution order of scripts.
      --exit-on-error exit as soon as a script returns with a non-zero exit
                      code.
      --stdin         multiplex stdin to scripts being run, using temporary file
      --lsbsysinit    validate filenames based on LSB sysinit specs.
      --new-session   run each script in a separate process session
      --regex=PATTERN validate filenames based on POSIX ERE pattern PATTERN.
  -u, --umask=UMASK   sets umask to UMASK (octal), default is 022.
  -a, --arg=ARGUMENT  pass ARGUMENT to scripts, use once for each argument.
  -V, --version       output version information and exit.
  -h, --help          display this help and exit.
";

const VERSION: &str = "Debian run-parts program, version 5.23.1
Copyright (C) 1994 Ian Jackson, Copyright (C) 1996 Jeff Noxon.
Copyright (C) 1996,1997,1998,1999 Guy Maor
Copyright (C) 2002-2020 Clint Adams
This is free software; see the GNU General Public License version 2
or later for copying conditions.  There is NO warranty.
";

#[derive(Copy, Clone, PartialEq, Eq)]
enum RegexMode {
    Normal,
    Ere,
    LsbSysinit,
}

/// As expressões compiladas pro modo escolhido.
struct Patterns {
    mode: RegexMode,
    custom: Option<Regex>,
    hier: Option<Regex>,
    excs: Option<Regex>,
    trad: Option<Regex>,
    classical: Option<Regex>,
}

struct Settings {
    test_mode: bool,
    list_mode: bool,
    verbose_mode: bool,
    debug_mode: bool,
    report_mode: bool,
    reverse_mode: bool,
    exit_on_error_mode: bool,
    new_session_mode: bool,
    stdin_mode: bool,
    args: Vec<Vec<u8>>,
}

/// `error()`: `run-parts: <msg>` no stderr.
fn error(msg: impl AsRef<str>) {
    io::eprint(format!("run-parts: {}\n", msg.as_ref()));
}

/// `sscanf("%o")` seguido das checagens do `set_umask`: `None` quando o valor é inválido.
fn parse_umask(s: &[u8]) -> Option<u32> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut value: u64 = 0;
    while i < s.len() && (b'0'..=b'7').contains(&s[i]) {
        value = value.wrapping_mul(8).wrapping_add(u64::from(s[i] - b'0'));
        i += 1;
    }
    if i == start {
        return None;
    }
    let v32 = if neg {
        (value as u32).wrapping_neg()
    } else {
        value as u32
    };
    if v32 > 0o7777 {
        return None;
    }
    Some(v32)
}

/// Compila as expressões do modo; `Err` com a mensagem do `regerror`.
fn compile_patterns(mode: RegexMode, custom_ere: Option<&[u8]>) -> Result<Patterns, String> {
    let ere = |p: &[u8]| Regex::new(p, Syntax::POSIX_EXTENDED).map_err(|e| e.message().to_string());
    let mut pt = Patterns {
        mode,
        custom: None,
        hier: None,
        excs: None,
        trad: None,
        classical: None,
    };
    match mode {
        RegexMode::Ere => pt.custom = Some(ere(custom_ere.unwrap_or(b""))?),
        RegexMode::LsbSysinit => {
            pt.hier = Some(ere(b"^_?([a-z0-9_.]+-)+[a-z0-9]+$")?);
            pt.excs = Some(ere(b"^[a-z0-9-].*\\.dpkg-(old|dist|new|tmp)$")?);
            pt.trad = Some(
                Regex::new(b"^[a-z0-9][a-z0-9_-]*$", Syntax::POSIX_BASIC)
                    .map_err(|e| e.message().to_string())?,
            );
        }
        RegexMode::Normal => pt.classical = Some(ere(b"^[a-zA-Z0-9_-]+$")?),
    }
    Ok(pt)
}

impl Patterns {
    /// `valid_name`.
    fn valid_name(&self, name: &[u8], debug: bool) -> bool {
        let m = |r: &Option<Regex>| r.as_ref().is_some_and(|re| re.is_match(name));
        let label = |ok: bool| if ok { "pass" } else { "fail" };
        let shown = io::lossy(name);
        match self.mode {
            RegexMode::Ere => {
                let ok = m(&self.custom);
                if debug {
                    io::eprint(format!("\"{shown}\": customre {}\n", label(ok)));
                }
                ok
            }
            RegexMode::LsbSysinit => {
                if m(&self.hier) {
                    let ok = !m(&self.excs);
                    if debug {
                        io::eprint(format!("\"{shown}\": hierre pass, excsre {}\n", label(ok)));
                    }
                    ok
                } else {
                    let ok = m(&self.trad);
                    if debug {
                        io::eprint(format!("\"{shown}\": tradre {}\n", label(ok)));
                    }
                    ok
                }
            }
            RegexMode::Normal => {
                let ok = m(&self.classical);
                if debug {
                    io::eprint(format!("\"{shown}\": classicalre {}\n", label(ok)));
                }
                ok
            }
        }
    }
}

/// Imprime o que um programa escreveu em `--report`: o nome do programa antes do primeiro byte.
fn report_chunk(progname: &[u8], to_stderr: bool, data: &[u8], printflag: &mut bool) {
    let fd = if to_stderr { Fd::STDERR } else { Fd::STDOUT };
    if !*printflag {
        let _ = io::flush_stdout();
        let mut header = progname.to_vec();
        header.extend_from_slice(b":\n");
        let _ = sys::write_all(fd, &header);
        *printflag = true;
    }
    let _ = io::flush_stdout();
    let _ = sys::write_all(fd, data);
}

/// `run_part`: executa `progname`; devolve o novo `exitstatus` (se o programa falhou) ou `None`.
fn run_part(st: &Settings, progname: &[u8], stdin_fd: Option<Fd>) -> Option<i32> {
    let s = sys::current();
    let mut actions: Vec<FdAction> = Vec::new();
    let mut pipes: Option<((Fd, Fd), (Fd, Fd))> = None;

    if st.report_mode {
        let pout = s.pipe2(OFlags::CLOEXEC);
        let perr = s.pipe2(OFlags::CLOEXEC);
        match (pout, perr) {
            (Ok(po), Ok(pe)) => pipes = Some((po, pe)),
            (Err(e), _) | (_, Err(e)) => {
                error(format!("pipe: {}", e.message()));
                sys::exit(1);
            }
        }
    }
    if st.stdin_mode
        && let Some(fd) = stdin_fd
    {
        // O filho compartilha o deslocamento do arquivo: rebobina antes de cada programa.
        let _ = s.lseek(fd, 0, Whence::Set);
        actions.push(FdAction::Dup2 {
            from: fd,
            to: Fd::STDIN,
        });
    }
    if let Some(((po_r, po_w), (pe_r, pe_w))) = pipes {
        actions.push(FdAction::Dup2 {
            from: po_w,
            to: Fd::STDOUT,
        });
        actions.push(FdAction::Dup2 {
            from: pe_w,
            to: Fd::STDERR,
        });
        actions.push(FdAction::Close(po_r));
        actions.push(FdAction::Close(pe_r));
        actions.push(FdAction::Close(po_w));
        actions.push(FdAction::Close(pe_w));
    }

    let mut argv: Vec<Vec<u8>> = vec![progname.to_vec()];
    argv.extend(st.args.iter().cloned());
    let attrs = ProcAttrs {
        fd_actions: actions,
        new_session: st.new_session_mode,
        ..ProcAttrs::default()
    };
    let spawned = s.spawn(SpawnSpec {
        path: progname.to_vec(),
        argv,
        attrs,
    });

    let mut printflag = false;
    let status: WaitStatus;
    match spawned {
        Err(e) => {
            // O filho do original escreve a mensagem e sai com 1 (no stderr dele, ou seja, no pipe).
            let msg = format!(
                "run-parts: failed to exec {}: {}\n",
                io::lossy(progname),
                e.message()
            );
            if pipes.is_some() {
                report_chunk(progname, true, msg.as_bytes(), &mut printflag);
            } else {
                io::eprint(msg);
            }
            status = WaitStatus::Exited(1);
            if let Some(((po_r, po_w), (pe_r, pe_w))) = pipes {
                for fd in [po_r, po_w, pe_r, pe_w] {
                    let _ = s.close(fd);
                }
            }
        }
        Ok(pid) => {
            let mut result: Option<WaitStatus> = None;
            if let Some(((po_r, po_w), (pe_r, pe_w))) = pipes {
                let _ = s.close(po_w);
                let _ = s.close(pe_w);
                let mut open: [Option<Fd>; 2] = [Some(po_r), Some(pe_r)];
                let mut buf = [0u8; 4096];
                while open.iter().any(Option::is_some) {
                    if result.is_none() {
                        match s.wait4(WaitTarget::Pid(pid), WaitOptions::NOHANG) {
                            Ok(Some((_, w))) => {
                                if matches!(w, WaitStatus::Exited(_) | WaitStatus::Signaled { .. })
                                {
                                    // Programa morto: só lê o que sobrou, sem esperar (pode haver
                                    // netos segurando o pipe).
                                    result = Some(w);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                error(format!("waitpid: {}", e.message()));
                                sys::exit(1);
                            }
                        }
                    }
                    let mut pfds: Vec<PollFd> = Vec::new();
                    let mut which: Vec<usize> = Vec::new();
                    for (i, f) in open.iter().enumerate() {
                        if let Some(fd) = f {
                            pfds.push(PollFd {
                                fd: *fd,
                                events: PollEvents::IN,
                                revents: PollEvents::empty(),
                            });
                            which.push(i);
                        }
                    }
                    let timeout = if result.is_some() {
                        Some(std::time::Duration::ZERO)
                    } else {
                        None
                    };
                    let n = match s.poll(&mut pfds, timeout) {
                        Ok(n) => n,
                        Err(Errno::EINTR) => continue,
                        Err(e) => {
                            error(format!("select: {}", e.message()));
                            sys::exit(1);
                        }
                    };
                    if n == 0 {
                        if result.is_some() {
                            // Zero timeout, no data left.
                            for f in open.iter_mut() {
                                if let Some(fd) = f.take() {
                                    let _ = s.close(fd);
                                }
                            }
                        }
                        continue;
                    }
                    for (k, idx) in which.iter().enumerate() {
                        if pfds[k].revents.is_empty() {
                            continue;
                        }
                        let Some(fd) = open[*idx] else { continue };
                        match s.read(fd, &mut buf) {
                            Ok(c) if c > 0 => {
                                report_chunk(progname, *idx == 1, &buf[..c], &mut printflag)
                            }
                            Ok(_) => {
                                let _ = s.close(fd);
                                open[*idx] = None;
                            }
                            Err(e) => {
                                let _ = s.close(fd);
                                open[*idx] = None;
                                let which_pipe = if *idx == 0 {
                                    "stdout pipe"
                                } else {
                                    "error pipe"
                                };
                                error(format!("failed to read from {which_pipe}: {}", e.message()));
                            }
                        }
                    }
                }
            }
            status = match result {
                Some(w) => w,
                None => match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
                    Ok(Some((_, w))) => w,
                    Ok(None) => WaitStatus::Exited(0),
                    Err(e) => {
                        error(format!("waitpid: {}", e.message()));
                        sys::exit(1);
                    }
                },
            };
        }
    }

    match status {
        WaitStatus::Exited(code) if code != 0 => {
            error(format!(
                "{} exited with return code {}",
                io::lossy(progname),
                code
            ));
            Some(code)
        }
        WaitStatus::Signaled { signal, .. } => {
            error(format!(
                "{} exited because of uncaught signal {}",
                io::lossy(progname),
                signal.0
            ));
            Some(1)
        }
        _ => None,
    }
}

/// `copy_stdin`: o stdin inteiro num arquivo temporário já removido, com o descritor aberto.
fn copy_stdin() -> Option<Fd> {
    let tmpdir = sys::getenv("TMPDIR").unwrap_or_else(|| b"/tmp".to_vec());
    let s = sys::current();
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut fd = None;
    for _ in 0..100 {
        let mut rnd = [0u8; 6];
        let _ = s.getrandom(&mut rnd);
        let mut path = tmpdir.clone();
        path.extend_from_slice(b"/run-parts.stdin.");
        path.extend(
            rnd.iter()
                .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()]),
        );
        match s.openat(
            Fd::CWD,
            &path,
            OFlags::RDWR | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC,
            0o600,
        ) {
            Ok(f) => {
                let _ = s.unlinkat(Fd::CWD, &path, AtFlags::empty());
                fd = Some(f);
                break;
            }
            Err(Errno::EEXIST) => continue,
            Err(_) => break,
        }
    }
    let fd = fd?;
    let mut buf = [0u8; 4096];
    loop {
        match s.read(Fd::STDIN, &mut buf) {
            Ok(0) => return Some(fd),
            Ok(n) => {
                if sys::write_all(fd, &buf[..n]).is_err() {
                    error("run-parts: failed to write to temporary file\n");
                    let _ = s.close(fd);
                    return None;
                }
            }
            Err(Errno::EINTR) => {}
            Err(_) => {
                error("run-parts: failed to read from stdin\n");
                let _ = s.close(fd);
                return None;
            }
        }
    }
}

/// `run_parts`: devolve o `exitstatus`.
fn run_parts(st: &Settings, pt: &Patterns, dirnames: &[Vec<u8>], exitstatus_in: i32) -> i32 {
    let mut exitstatus = exitstatus_in;

    // 1st step: gather a list of all files in the given directories
    let mut basenames: Vec<Vec<u8>> = Vec::new();
    for dir in dirnames {
        let Ok(entries) = sys::read_dir(dir) else {
            continue;
        };
        for e in entries {
            if !pt.valid_name(&e.name, st.debug_mode) {
                continue;
            }
            basenames.push(e.name);
        }
    }
    if basenames.is_empty() {
        // nothing to do
        return exitstatus;
    }

    // 2nd step: sort that list; 3rd step: unique elements
    basenames.sort();
    basenames.dedup();
    if st.debug_mode {
        io::eprint("list of unique basenames:\n");
        for b in &basenames {
            io::eprint(format!(" - {}\n", io::lossy(b)));
        }
    }

    // 4th step: the first directory in which each basename exists
    let mut full_paths: Vec<Vec<u8>> = Vec::new();
    for b in &basenames {
        for d in dirnames {
            let mut p = d.clone();
            p.push(b'/');
            p.extend_from_slice(b);
            if st.debug_mode {
                io::eprint(format!("checking {}... ", io::lossy(&p)));
            }
            if sys::stat(&p).is_err() {
                if st.debug_mode {
                    io::eprint("not found\n");
                }
                continue;
            }
            if st.debug_mode {
                io::eprint("found\n");
            }
            full_paths.push(p);
            break;
        }
    }

    let mut stdin_fd: Option<Fd> = None;
    if st.stdin_mode {
        stdin_fd = copy_stdin();
        if stdin_fd.is_none() {
            error("run-parts: failed to copy content of stdin\n");
            sys::exit(1);
        }
    }

    // 5th step: process the list of full paths
    let order: Vec<usize> = if st.reverse_mode {
        (0..full_paths.len()).rev().collect()
    } else {
        (0..full_paths.len()).collect()
    };
    let mut out = io::stdout();
    let s = sys::current();
    for i in order {
        let filename = &full_paths[i];
        if st.debug_mode {
            io::eprint(format!("processing: {}\n", io::lossy(filename)));
        }
        let stat = match sys::stat(filename) {
            Ok(x) => x,
            Err(e) => {
                error(format!(
                    "failed to stat component {}: {}",
                    io::lossy(filename),
                    e.message()
                ));
                if st.exit_on_error_mode {
                    sys::exit(1);
                }
                continue;
            }
        };
        match stat.file_type() {
            FileType::Regular => {
                let can_exec = s
                    .faccessat(Fd::CWD, filename, AccessMode::X_OK, AtFlags::empty())
                    .is_ok();
                let can_read = s
                    .faccessat(Fd::CWD, filename, AccessMode::R_OK, AtFlags::empty())
                    .is_ok();
                if can_exec {
                    if st.test_mode {
                        let _ = out.write_all(filename);
                        let _ = out.write_all(b"\n");
                    } else if st.list_mode {
                        if can_read {
                            let _ = out.write_all(filename);
                            let _ = out.write_all(b"\n");
                        }
                    } else {
                        if st.verbose_mode {
                            let mut line = format!("run-parts: executing {}", io::lossy(filename));
                            for a in &st.args {
                                line.push(' ');
                                line.push_str(&io::lossy(a));
                            }
                            line.push('\n');
                            io::eprint(line);
                        }
                        let _ = out.flush();
                        if let Some(code) = run_part(st, filename, stdin_fd) {
                            exitstatus = code;
                        }
                        if exitstatus != 0 && st.exit_on_error_mode {
                            return exitstatus;
                        }
                    }
                } else if can_read && st.list_mode {
                    let _ = out.write_all(filename);
                    let _ = out.write_all(b"\n");
                }
            }
            FileType::Directory => {}
            _ => {
                if !st.list_mode {
                    error(format!(
                        "run-parts: component {} is not an executable plain file\n",
                        io::lossy(filename)
                    ));
                    exitstatus = 1;
                }
            }
        }
        sys::checkpoint();
    }
    exitstatus
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);

    sys::current().umask(0o022);

    let mut st = Settings {
        test_mode: false,
        list_mode: false,
        verbose_mode: false,
        debug_mode: false,
        report_mode: false,
        reverse_mode: false,
        exit_on_error_mode: false,
        new_session_mode: false,
        stdin_mode: false,
        args: Vec::new(),
    };
    let mut regex_mode = RegexMode::Normal;
    let mut custom_ere: Option<Vec<u8>> = None;

    let mut g = Getopt::from_env(&argv[1..], "u:ha:vV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry `run-parts --help' for more information.\n",
                    e.message(&argv0)
                ));
                return 1;
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        match o.id {
            O_TEST => st.test_mode = true,
            O_LIST => st.list_mode = true,
            O_REPORT => st.report_mode = true,
            O_REVERSE => st.reverse_mode = true,
            O_LSBSYSINIT => regex_mode = RegexMode::LsbSysinit,
            O_REGEX => {
                regex_mode = RegexMode::Ere;
                custom_ere = Some(arg);
            }
            O_STDIN => st.stdin_mode = true,
            O_EXIT_ON_ERROR => st.exit_on_error_mode = true,
            O_NEW_SESSION => st.new_session_mode = true,
            id if id == i32::from(b'u') => match parse_umask(&arg) {
                Some(m) => {
                    sys::current().umask(m);
                }
                None => {
                    error("bad umask value");
                    return 1;
                }
            },
            id if id == i32::from(b'a') => st.args.push(arg),
            id if id == i32::from(b'h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            id if id == i32::from(b'v') => st.verbose_mode = true,
            id if id == i32::from(b'd') => st.debug_mode = true,
            id if id == i32::from(b'V') => {
                let mut out = io::stdout();
                let _ = out.write_all(VERSION.as_bytes());
                return 0;
            }
            _ => {
                io::eprint("Try `run-parts --help' for more information.\n");
                return 1;
            }
        }
    }

    let dirs = g.operands();
    // We require exactly one argument: the directory name
    if dirs.is_empty() {
        error("missing operand");
        io::eprint("Try `run-parts --help' for more information.\n");
        return 1;
    }
    if st.list_mode && st.test_mode {
        error("--list and --test can not be used together");
        io::eprint("Try `run-parts --help' for more information.\n");
        return 1;
    }

    let patterns = match compile_patterns(regex_mode, custom_ere.as_deref()) {
        Ok(p) => p,
        Err(m) => {
            io::eprint(format!("Unable to build regexp: {m}\n"));
            return 1;
        }
    };
    let code = run_parts(&st, &patterns, &dirs, 0);
    let _ = io::flush_stdout();
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umask_parsing_follows_sscanf_o() {
        assert_eq!(parse_umask(b"022"), Some(0o22));
        assert_eq!(parse_umask(b"0x7"), Some(0));
        assert_eq!(parse_umask(b"12z"), Some(0o12));
        assert_eq!(parse_umask(b"8"), None);
        assert_eq!(parse_umask(b""), None);
        assert_eq!(parse_umask(b"-1"), None);
        assert_eq!(parse_umask(b"10000"), None);
    }

    #[test]
    fn name_filters() {
        let classical = compile_patterns(RegexMode::Normal, None).expect("classical");
        assert!(classical.valid_name(b"10-a_b", false));
        assert!(!classical.valid_name(b"a.b", false));
        let lsb = compile_patterns(RegexMode::LsbSysinit, None).expect("lsb");
        assert!(lsb.valid_name(b"foo-bar", false));
        assert!(lsb.valid_name(b"plain", false));
        assert!(!lsb.valid_name(b"foo-bar.dpkg-old", false));
        assert!(!lsb.valid_name(b"-x", false));
    }
}
