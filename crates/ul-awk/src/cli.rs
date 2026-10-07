//! Linha de comando do gawk 5.2.1: opções, fontes do programa, `--version` e o texto de uso.

use std::sync::Arc;

use sysabi::{Fd, OFlags, Syscalls};

use crate::ast::Source;
use crate::interp::{Config, Interp, is_assignment, process_escapes};

fn usage(prog: &str) -> String {
    format!(
        "Usage: {prog} [POSIX or GNU style options] -f progfile [--] file ...\n\
Usage: {prog} [POSIX or GNU style options] [--] 'program' file ...\n\
POSIX options:\t\tGNU long options: (standard)\n\
\t-f progfile\t\t--file=progfile\n\
\t-F fs\t\t\t--field-separator=fs\n\
\t-v var=val\t\t--assign=var=val\n\
Short options:\t\tGNU long options: (extensions)\n\
\t-b\t\t\t--characters-as-bytes\n\
\t-c\t\t\t--traditional\n\
\t-C\t\t\t--copyright\n\
\t-d[file]\t\t--dump-variables[=file]\n\
\t-D[file]\t\t--debug[=file]\n\
\t-e 'program-text'\t--source='program-text'\n\
\t-E file\t\t\t--exec=file\n\
\t-g\t\t\t--gen-pot\n\
\t-h\t\t\t--help\n\
\t-i includefile\t\t--include=includefile\n\
\t-I\t\t\t--trace\n\
\t-l library\t\t--load=library\n\
\t-L[fatal|invalid|no-ext]\t--lint[=fatal|invalid|no-ext]\n\
\t-M\t\t\t--bignum\n\
\t-N\t\t\t--use-lc-numeric\n\
\t-n\t\t\t--non-decimal-data\n\
\t-o[file]\t\t--pretty-print[=file]\n\
\t-O\t\t\t--optimize\n\
\t-p[file]\t\t--profile[=file]\n\
\t-P\t\t\t--posix\n\
\t-r\t\t\t--re-interval\n\
\t-s\t\t\t--no-optimize\n\
\t-S\t\t\t--sandbox\n\
\t-t\t\t\t--lint-old\n\
\t-V\t\t\t--version\n\
\n\
To report bugs, use the `gawkbug' program.\n\
For full instructions, see the node `Bugs' in `gawk.info'\n\
which is section `Reporting Problems and Bugs' in the\n\
printed version.  This same information may be found at\n\
https://www.gnu.org/software/gawk/manual/html_node/Bugs.html.\n\
PLEASE do NOT try to report bugs by posting in comp.lang.awk,\n\
or by using a web forum such as Stack Overflow.\n\
\n\
gawk is a pattern scanning and processing language.\n\
By default it reads standard input and writes standard output.\n\
\n\
Examples:\n\
\t{prog} '{{ sum += $1 }}; END {{ print sum }}' file\n\
\t{prog} -F: '{{ print $1 }}' /etc/passwd\n"
    )
}

const VERSION_TEXT: &str = "GNU Awk 5.2.1, API 3.2, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)
Copyright (C) 1989, 1991-2022 Free Software Foundation.

This program is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see http://www.gnu.org/licenses/.
";

const COPYRIGHT_TEXT: &str = "Copyright (C) 1989, 1991-2022 Free Software Foundation.

This program is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see http://www.gnu.org/licenses/.
";

fn write_fd(sys: &Arc<dyn Syscalls>, fd: Fd, s: &[u8]) {
    let _ = crate::io::write_all(sys, fd, s);
}

/// Lê um arquivo inteiro pelo sysabi.
fn read_file(sys: &Arc<dyn Syscalls>, path: &[u8]) -> Result<Vec<u8>, sysabi::Errno> {
    let fd = sys.openat(Fd::CWD, path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
    if let Ok(st) = sys.fstat(fd)
        && st.file_type() == sysabi::FileType::Directory {
            let _ = sys.close(fd);
            return Err(sysabi::Errno::EISDIR);
        }
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    loop {
        match sys.read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if out.try_reserve(n).is_err() {
                    let _ = sys.close(fd);
                    return Err(sysabi::Errno::ENOMEM);
                }
                out.extend_from_slice(&buf[..n]);
            }
            Err(sysabi::Errno::EINTR) => {}
            Err(e) => {
                let _ = sys.close(fd);
                return Err(e);
            }
        }
    }
    let _ = sys.close(fd);
    Ok(out)
}

/// Procura um arquivo de programa no AWKPATH (e com `.awk` no fim), como o gawk.
fn find_source(sys: &Arc<dyn Syscalls>, name: &str) -> Result<(String, Vec<u8>), sysabi::Errno> {
    let first = read_file(sys, name.as_bytes());
    if first.is_ok() || name.contains('/') {
        return first.map(|d| (name.to_string(), d));
    }
    let path = sys.getenv(b"AWKPATH").map(|p| String::from_utf8_lossy(&p).into_owned()).unwrap_or_else(|| ".:/usr/share/awk".to_string());
    for candidate in [name.to_string(), format!("{name}.awk")] {
        for dir in path.split(':') {
            let dir = if dir.is_empty() { "." } else { dir };
            let full = format!("{dir}/{candidate}");
            if let Ok(d) = read_file(sys, full.as_bytes()) {
                return Ok((full, d));
            }
        }
    }
    if !name.ends_with(".awk")
        && let Ok(d) = read_file(sys, format!("{name}.awk").as_bytes()) {
            return Ok((format!("{name}.awk"), d));
        }
    first.map(|d| (name.to_string(), d))
}

enum SrcSpec {
    Text(Vec<u8>),
    File(String),
    Include(String),
}

/// Entrada do programa: devolve o código de saída.
pub fn run(sys: Arc<dyn Syscalls>, prog: &str, argv: Vec<Vec<u8>>) -> i32 {
    let mut cfg = Config { prog_name: prog.to_string(), full_argv: argv.clone(), mawk: prog == "mawk", ..Config::default() };
    let mut sources: Vec<SrcSpec> = Vec::new();
    let mut i = 1;
    let mut stop_at_exec = false;
    // Opções (estilo getopt "+": param no primeiro operando).
    while i < argv.len() {
        let a = argv[i].clone();
        if a == b"--" {
            i += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        if a.starts_with(b"--") {
            let body = String::from_utf8_lossy(&a[2..]).into_owned();
            let (name, inline): (&str, Option<String>) = match body.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (body.as_str(), None),
            };
            const LONG: &[(&str, bool)] = &[
                ("assign", true), ("bignum", false), ("characters-as-bytes", false), ("copyright", false),
                ("debug", false), ("dump-variables", false), ("exec", true), ("field-separator", true), ("file", true),
                ("gen-pot", false), ("help", false), ("include", true), ("lint", false), ("lint-old", false),
                ("load", true), ("no-optimize", false), ("non-decimal-data", false), ("optimize", false),
                ("posix", false), ("pretty-print", false), ("profile", false), ("re-interval", false),
                ("sandbox", false), ("source", true), ("trace", false), ("traditional", false),
                ("use-lc-numeric", false), ("version", false), ("persist", false),
            ];
            // Prefixo único também vale, como no getopt_long.
            let matches: Vec<&(&str, bool)> = LONG.iter().filter(|(n, _)| *n == name || n.starts_with(name)).collect();
            let exact = LONG.iter().find(|(n, _)| *n == name);
            let Some(&(lname, takes)) = exact.or(if matches.len() == 1 { Some(matches[0]) } else { None }) else {
                write_fd(&sys, Fd::STDERR, usage(prog).as_bytes());
                return 1;
            };
            let value = if takes {
                match inline {
                    Some(v) => Some(v.into_bytes()),
                    None => {
                        i += 1;
                        match argv.get(i) {
                            Some(v) => Some(v.clone()),
                            None => {
                                write_fd(&sys, Fd::STDERR, usage(prog).as_bytes());
                                return 1;
                            }
                        }
                    }
                }
            } else {
                None
            };
            i += 1;
            match lname {
                "assign" => {
                    let v = value.unwrap_or_default();
                    if let Some(code) = check_assign(&sys, prog, &v) {
                        return code;
                    }
                    cfg.assigns.push(v);
                }
                "field-separator" => cfg.fs = Some(process_escapes(&value.unwrap_or_default())),
                "file" => sources.push(SrcSpec::File(String::from_utf8_lossy(&value.unwrap_or_default()).into_owned())),
                "exec" => {
                    sources.push(SrcSpec::File(String::from_utf8_lossy(&value.unwrap_or_default()).into_owned()));
                    stop_at_exec = true;
                }
                "source" => sources.push(SrcSpec::Text(value.unwrap_or_default())),
                "include" => sources.push(SrcSpec::Include(String::from_utf8_lossy(&value.unwrap_or_default()).into_owned())),
                "help" => {
                    write_fd(&sys, Fd::STDOUT, usage(prog).as_bytes());
                    return 0;
                }
                "version" => {
                    write_fd(&sys, Fd::STDOUT, VERSION_TEXT.as_bytes());
                    return 0;
                }
                "copyright" => {
                    write_fd(&sys, Fd::STDOUT, COPYRIGHT_TEXT.as_bytes());
                    return 0;
                }
                "posix" => cfg.posix = true,
                "traditional" => cfg.traditional = true,
                "non-decimal-data" => cfg.non_decimal = true,
                "sandbox" => cfg.sandbox = true,
                "load" => {
                    let lib = String::from_utf8_lossy(&value.unwrap_or_default()).into_owned();
                    let msg = format!("{prog}: fatal: load_ext: cannot open library `{lib}'\n");
                    write_fd(&sys, Fd::STDERR, msg.as_bytes());
                    return 2;
                }
                _ => {}
            }
            if stop_at_exec {
                break;
            }
            continue;
        }
        // Opções curtas agrupadas.
        let mut j = 1;
        let mut consumed_next = false;
        while j < a.len() {
            let c = a[j];
            let rest = &a[j + 1..];
            let takes_arg = matches!(c, b'F' | b'v' | b'f' | b'e' | b'E' | b'i' | b'l');
            let optional_arg = matches!(c, b'd' | b'D' | b'L' | b'o' | b'p');
            if takes_arg {
                let val = if !rest.is_empty() {
                    rest.to_vec()
                } else {
                    match argv.get(i + 1) {
                        Some(v) => {
                            consumed_next = true;
                            v.clone()
                        }
                        None => {
                            write_fd(&sys, Fd::STDERR, usage(prog).as_bytes());
                            return 1;
                        }
                    }
                };
                match c {
                    b'F' => cfg.fs = Some(process_escapes(&val)),
                    b'v' => {
                        if let Some(code) = check_assign(&sys, prog, &val) {
                            return code;
                        }
                        cfg.assigns.push(val);
                    }
                    b'f' => sources.push(SrcSpec::File(String::from_utf8_lossy(&val).into_owned())),
                    b'E' => {
                        sources.push(SrcSpec::File(String::from_utf8_lossy(&val).into_owned()));
                        stop_at_exec = true;
                    }
                    b'e' => sources.push(SrcSpec::Text(val)),
                    b'i' => sources.push(SrcSpec::Include(String::from_utf8_lossy(&val).into_owned())),
                    _ => {
                        let lib = String::from_utf8_lossy(&val).into_owned();
                        let msg = format!("{prog}: fatal: load_ext: cannot open library `{lib}'\n");
                        write_fd(&sys, Fd::STDERR, msg.as_bytes());
                        return 2;
                    }
                }
                break;
            }
            if optional_arg {
                // `-d[arq]`: o resto do argumento é o arquivo (ignorado aqui).
                break;
            }
            match c {
                b'b' | b'g' | b'I' | b'M' | b'N' | b'O' | b'r' | b's' | b't' => {}
                b'c' => cfg.traditional = true,
                b'P' => cfg.posix = true,
                b'n' => cfg.non_decimal = true,
                b'S' => cfg.sandbox = true,
                b'h' => {
                    write_fd(&sys, Fd::STDOUT, usage(prog).as_bytes());
                    return 0;
                }
                b'V' => {
                    write_fd(&sys, Fd::STDOUT, VERSION_TEXT.as_bytes());
                    return 0;
                }
                b'C' => {
                    write_fd(&sys, Fd::STDOUT, COPYRIGHT_TEXT.as_bytes());
                    return 0;
                }
                _ => {
                    write_fd(&sys, Fd::STDERR, usage(prog).as_bytes());
                    return 1;
                }
            }
            j += 1;
        }
        i += 1;
        if consumed_next {
            i += 1;
        }
        if stop_at_exec {
            break;
        }
    }
    // Programa na linha de comando se não houve -f/-e.
    let has_program_source = sources.iter().any(|s| matches!(s, SrcSpec::Text(_) | SrcSpec::File(_)));
    if !has_program_source {
        let Some(p) = argv.get(i) else {
            write_fd(&sys, Fd::STDERR, usage(prog).as_bytes());
            return 1;
        };
        sources.push(SrcSpec::Text(p.clone()));
        i += 1;
    }
    // Fontes.
    let mut srcs = Vec::new();
    for s in sources {
        match s {
            SrcSpec::Text(t) => srcs.push(Source { name: "cmd. line".to_string(), text: t }),
            SrcSpec::File(name) => {
                let r = if name == "-" || name == "/dev/stdin" { read_file(&sys, b"/dev/stdin").map(|d| (name.clone(), d)) } else { find_source(&sys, &name) };
                match r {
                    Ok((_found, text)) => srcs.push(Source { name, text }),
                    Err(e) => {
                        let msg = format!("{prog}: fatal: cannot open source file `{name}' for reading: {}\n", e.message());
                        write_fd(&sys, Fd::STDERR, msg.as_bytes());
                        return 2;
                    }
                }
            }
            SrcSpec::Include(name) => {
                let mut t = b"@include \"".to_vec();
                t.extend_from_slice(name.as_bytes());
                t.extend_from_slice(b"\"\n");
                srcs.push(Source { name: "cmd. line".to_string(), text: t });
            }
        }
    }
    // ARGV: nome do programa e os operandos.
    let mut args = vec![prog.as_bytes().to_vec()];
    args.extend(argv[i.min(argv.len())..].iter().cloned());
    cfg.argv = args;

    let sys2 = sys.clone();
    let mut loader = move |name: &str| -> Result<Source, String> {
        match find_source(&sys2, name) {
            Ok((found, text)) => Ok(Source { name: found, text }),
            Err(e) => Err(e.message()),
        }
    };
    let parsed = match crate::parser::parse(prog, srcs, &mut loader) {
        Ok(p) => p,
        Err(f) => {
            if cfg.mawk {
                let (text, code) = mawk_syntax_error(&f.stderr);
                write_fd(&sys, Fd::STDERR, text.as_bytes());
                return code;
            }
            write_fd(&sys, Fd::STDERR, f.stderr.as_bytes());
            return f.code;
        }
    };
    if !parsed.warnings.is_empty() {
        write_fd(&sys, Fd::STDERR, parsed.warnings.as_bytes());
    }
    // Erros de compilação das regexes constantes aparecem no fim do parse, como no gawk.
    let mut re_errors = String::new();
    let mut re_warnings = String::new();
    let mut warned = std::collections::HashSet::new();
    for (i, re) in parsed.program.regexes.iter().enumerate() {
        let (src, line) = parsed.program.regex_locs.get(i).copied().unwrap_or((0, 1));
        let loc = crate::parser::location(&parsed.program.sources, src, line);
        match crate::regex::Regex::new(re, false) {
            Ok((_, warns)) => {
                // O gawk avisa cada escape uma vez só por execução.
                for w in warns {
                    if warned.insert(w.clone()) {
                        re_warnings.push_str(&format!("{prog}: {loc}: warning: {w}\n"));
                    }
                }
            }
            Err(e) => re_errors.push_str(&format!("{prog}: {loc}: error: {}\n", e.static_message())),
        }
    }
    if !re_warnings.is_empty() {
        write_fd(&sys, Fd::STDERR, re_warnings.as_bytes());
    }
    if !re_errors.is_empty() {
        write_fd(&sys, Fd::STDERR, re_errors.as_bytes());
        return 1;
    }
    let mut it = Interp::new(&parsed.program, cfg, sys);
    it.regex_warned = warned;
    it.run()
}

/// Texto do token que começa em `b`, como o mawk o cita depois de `near`.
fn mawk_token(b: &[u8]) -> String {
    let Some(&c) = b.first() else { return "end of line".to_string() };
    let n = if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
        b.iter().take_while(|x| x.is_ascii_alphanumeric() || **x == b'_' || **x == b'.').count()
    } else if c == b'"' {
        let mut i = 1;
        while i < b.len() && b[i] != b'"' {
            i += if b[i] == b'\\' { 2 } else { 1 };
        }
        (i + 1).min(b.len())
    } else if b.len() >= 2
        && matches!(&b[..2], b"==" | b"!=" | b"<=" | b">=" | b"&&" | b"||" | b"++" | b"--" | b"+=" | b"-=" | b"*=" | b"/=" | b"%=" | b"^=" | b">>" | b"!~")
    {
        2
    } else {
        1
    };
    String::from_utf8_lossy(&b[..n]).into_owned()
}

/// Reescreve o erro de sintaxe de duas linhas do gawk (`prog: fonte:N: texto` e a linha do circunflexo)
/// no formato do mawk: `mawk: line N: syntax error at or near X`, ou `missing ) near X` quando há
/// parêntese aberto antes do ponto do erro. O mawk sai com 2.
fn mawk_syntax_error(gawk: &str) -> (String, i32) {
    let fallback = || (gawk.to_string(), 2);
    let mut lines = gawk.lines();
    let (Some(l1), Some(l2)) = (lines.next(), lines.next()) else { return fallback() };
    let Some(idx) = l2.find('^') else { return fallback() };
    let Some(colon) = l2[..idx].rfind(':') else { return fallback() };
    let prefix_len = colon + 2;
    if prefix_len > idx || l1.len() < prefix_len || !l1.is_char_boundary(prefix_len) {
        return fallback();
    }
    let Some(head) = l2[..colon].strip_prefix("mawk: ") else { return fallback() };
    let Some((name, lineno)) = head.rsplit_once(':') else { return fallback() };
    if lineno.parse::<u32>().is_err() {
        return fallback();
    }
    let msg = l2.get(idx + 1..).unwrap_or("").trim_start();
    let text = l1[prefix_len..].as_bytes();
    let col = idx - prefix_len;
    let origin = if name == "cmd. line" { String::new() } else { format!("{name}: ") };
    if msg.starts_with("source files") {
        return (format!("mawk: {origin}line {lineno}: syntax error at or near end of file\n"), 2);
    }
    let near = if col >= text.len() { "end of line".to_string() } else { mawk_token(&text[col..]) };
    // Parênteses abertos antes do erro (ignorando o que está em strings).
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0;
    while i < col.min(text.len()) {
        match text[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'(' if !in_str => depth += 1,
            b')' if !in_str => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    let what = if depth > 0 { format!("missing ) near {near}") } else { format!("syntax error at or near {near}") };
    (format!("mawk: {origin}line {lineno}: {what}\n"), 2)
}


/// Confere `-v nome=valor`; devolve o código de saída em erro.
fn check_assign(sys: &Arc<dyn Syscalls>, prog: &str, v: &[u8]) -> Option<i32> {
    let Some(eq) = v.iter().position(|b| *b == b'=') else {
        let s = String::from_utf8_lossy(v).into_owned();
        let msg = format!("{prog}: `{s}' argument to `-v' not in `var=value' form\n\n{}", usage(prog));
        write_fd(sys, Fd::STDERR, msg.as_bytes());
        return Some(1);
    };
    if !is_assignment(v) {
        let name = String::from_utf8_lossy(&v[..eq]).into_owned();
        let msg = format!("{prog}: fatal: `{name}' is not a legal variable name\n");
        write_fd(sys, Fd::STDERR, msg.as_bytes());
        return Some(2);
    }
    None
}
