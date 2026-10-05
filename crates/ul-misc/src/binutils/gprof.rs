//! `gprof` do GNU binutils 2.44 (Debian 13).
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Cobre `--help`, `--version`, validação de opções, os erros de imagem e de
//! arquivo `gmon.out` inexistente ou malformado e os códigos de saída.
//!
//! Limitação documentada: não há análise de perfil. Com imagem e `gmon.out` legíveis o programa
//! responde `profile analysis is not supported by this implementation` e sai com 1 (TODO).

use std::ffi::OsString;
use std::io::Write;

use crate::util::io::{self, File};

const VERSION_TEXT: &str = "GNU gprof (GNU Binutils for Debian) 2.44\n\
Based on BSD gprof, copyright 1983 Regents of the University of California.\n\
This program is free software.  This program has absolutely no warranty.\n";

const USAGE: &str = "\t[--[no-]time=name] [--all-lines] [--brief] [--debug[=level]]
\t[--function-ordering] [--file-ordering] [--inline-file-names]
\t[--directory-path=dirs] [--display-unused-functions]
\t[--file-format=name] [--file-info] [--help] [--line] [--min-count=n]
\t[--no-static] [--print-path] [--separate-files]
\t[--static-call-graph] [--sum] [--table-length=len] [--traditional]
\t[--version] [--width=n] [--ignore-non-functions]
\t[--demangle[=STYLE]] [--no-demangle] [--external-symbol-table=name] [@FILE]
\t[image-file] [profile-file...]
Report bugs to <https://sourceware.org/bugzilla/>
";

const USAGE_FIRST: &str = "Usage: gprof [-[abcDhilLrsTvwxyz]] [-[ACeEfFJnNOpPqQRStZ][name]] [-I dirs]
\t[-d[num]] [-k from/to] [-m min-count] [-t table-length]
\t[--[no-]annotated-source[=name]] [--[no-]exec-counts[=name]]
\t[--[no-]flat-profile[=name]] [--[no-]graph[=name]]
";

/// Opções curtas com argumento obrigatório.
const SHORT_ARG: &[u8] = b"eEfFIkmnNORSt";
/// Opções curtas com argumento opcional (só anexado).
const SHORT_OPT: &[u8] = b"ACdJpPqQZ";
/// Opções curtas sem argumento.
const SHORT_FLAG: &[u8] = b"abcDhilLrsTvwxyz";

const LONG_FLAG: &[&str] = &[
    "all-lines", "brief", "file-info", "help", "line", "no-static", "print-path",
    "separate-files", "static-call-graph", "sum", "traditional", "version",
    "ignore-non-functions", "function-ordering", "inline-file-names", "display-unused-functions",
    "no-demangle", "no-annotated-source", "no-exec-counts", "no-flat-profile", "no-graph",
    "no-time",
];
const LONG_OPT: &[&str] = &[
    "annotated-source", "exec-counts", "flat-profile", "graph", "debug", "demangle",
];
const LONG_ARG: &[&str] = &[
    "time", "file-format", "min-count", "table-length", "width", "directory-path",
    "external-symbol-table", "file-ordering",
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(to_stdout: bool) -> i32 {
    let text = format!("{USAGE_FIRST}{USAGE}");
    if to_stdout {
        let mut o = io::stdout();
        let _ = o.write_all(text.as_bytes());
        0
    } else {
        io::eprint(text);
        1
    }
}

fn run(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let argv = io::args_bytes(args);
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        i += 1;
        if a == b"--" {
            operands.extend(argv[i..].iter().cloned());
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            operands.push(a.clone());
            continue;
        }
        let text = String::from_utf8_lossy(a).into_owned();
        if let Some(long) = text.strip_prefix("--") {
            let (name, has_val) = match long.split_once('=') {
                Some((n, _)) => (n, true),
                None => (long, false),
            };
            if name == "help" {
                return usage(true);
            }
            if name == "version" {
                let mut o = io::stdout();
                let _ = o.write_all(VERSION_TEXT.as_bytes());
                return 0;
            }
            if LONG_FLAG.contains(&name) {
                if has_val {
                    io::eprint(format!(
                        "{prog}: option '--{name}' doesn't allow an argument\n"
                    ));
                    return usage(false);
                }
            } else if LONG_OPT.contains(&name) {
            } else if LONG_ARG.contains(&name) {
                if !has_val {
                    if i >= argv.len() {
                        io::eprint(format!(
                            "{prog}: option '--{name}' requires an argument\n"
                        ));
                        return usage(false);
                    }
                    i += 1;
                }
            } else {
                io::eprint(format!("{prog}: unrecognized option '--{name}'\n"));
                return usage(false);
            }
            continue;
        }
        // Cluster de opções curtas.
        let bytes = &a[1..];
        let mut k = 0;
        while k < bytes.len() {
            let c = bytes[k];
            k += 1;
            if c == b'h' {
                return usage(true);
            }
            if c == b'v' {
                let mut o = io::stdout();
                let _ = o.write_all(VERSION_TEXT.as_bytes());
                return 0;
            }
            if SHORT_FLAG.contains(&c) {
                continue;
            }
            if SHORT_OPT.contains(&c) {
                // O resto do cluster é o argumento.
                break;
            }
            if SHORT_ARG.contains(&c) {
                if k >= bytes.len() {
                    if i >= argv.len() {
                        io::eprint(format!(
                            "{prog}: option requires an argument -- '{}'\n",
                            c as char
                        ));
                        return usage(false);
                    }
                    i += 1;
                }
                break;
            }
            io::eprint(format!("{prog}: invalid option -- '{}'\n", c as char));
            return usage(false);
        }
    }
    let image = operands
        .first()
        .cloned()
        .unwrap_or_else(|| b"a.out".to_vec());
    let gmons: Vec<Vec<u8>> = if operands.len() > 1 {
        operands[1..].to_vec()
    } else {
        vec![b"gmon.out".to_vec()]
    };
    let mut image_file = match File::open(&image) {
        Ok(f) => f,
        Err(e) => {
            io::eprint(format!(
                "{prog}: {}: {}\n",
                io::lossy(&image),
                e.message()
            ));
            return 1;
        }
    };
    let mut head = [0u8; 4];
    let n = image_file.read_full(&mut head).unwrap_or(0);
    if n < 4 || &head != b"\x7fELF" {
        io::eprint(format!(
            "{prog}: {}: file format not recognized\n",
            io::lossy(&image)
        ));
        return 1;
    }
    for g in &gmons {
        let mut f = match File::open(g) {
            Ok(f) => f,
            Err(e) => {
                io::eprint(format!("{prog}: {}: {}\n", io::lossy(g), e.message()));
                return 1;
            }
        };
        let mut magic = [0u8; 4];
        let n = f.read_full(&mut magic).unwrap_or(0);
        if n < 4 {
            io::eprint(format!(
                "{prog}: {}: file too short to be a gmon file\n",
                io::lossy(g)
            ));
            return 1;
        }
        if &magic != b"gmon" {
            io::eprint(format!(
                "{prog}: file `{}' has bad magic cookie\n",
                io::lossy(g)
            ));
            return 1;
        }
    }
    // TODO: sem análise de perfil (tabela plana, grafo de chamadas).
    io::eprint(format!(
        "{prog}: profile analysis is not supported by this implementation\n"
    ));
    1
}
