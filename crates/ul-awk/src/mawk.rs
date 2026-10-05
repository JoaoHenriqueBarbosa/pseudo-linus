//! Front-end `mawk` 1.3.4 (Debian 13): opções `-W`, `--version`, `-f`, `-F`, `-v` e as mensagens
//! de uso do mawk. O programa em si roda no interpretador comum ([`crate::cli::run`]); este módulo
//! só traduz a linha de comando do mawk para a do interpretador.

use std::sync::Arc;

use sysabi::{Fd, Syscalls};

const VERSION_TEXT: &str = "mawk 1.3.4 20240123
Copyright 2008-2023,2024, Thomas E. Dickey
Copyright 1991-1996,2014, Michael D. Brennan

random-funcs:       srandom/random
regex-funcs:        internal
compiled limits:
sprintf buffer      8192
maximum-integer     2147483647
";

fn usage_text() -> String {
    "Usage: mawk [Options] [Program] [file ...]

Program:
    The -f option value is the name of a file containing program text.
    If no -f option is given, a \"--\" ends option processing; the following
    parameters are the program text.

Options:
    -f program-file  Program  text is read from file instead of from the
                     command-line.  Multiple -f options are accepted.
    -F value         sets the field separator, FS, to value.
    -v var=value     assigns value to program variable var.
    --               unambiguous end of options.

    Implementation-specific options are prefixed with \"-W\".  They can be
    abbreviated:

    -W version       show version information and exit.
    -W dump          show assembler-like listing of program and exit.
    -W help          show this message and exit.
    -W interactive   set unbuffered output, line-buffered input.
    -W exec file     use file as program as well as last option.
    -W random=number set initial random seed.
    -W sprintf=number adjust size of sprintf buffer.
    -W posix_space   do not consider \"\\n\" a space.
    -W usage         show this message and exit.
"
    .to_string()
}

fn put(sys: &Arc<dyn Syscalls>, fd: Fd, s: &str) {
    let _ = crate::io::write_all(sys, fd, s.as_bytes());
}

/// Resultado de interpretar uma opção `-W`.
enum Wopt {
    Version,
    Help,
    Exec(Option<Vec<u8>>),
    Ignored,
    Dump,
    Unknown,
}

/// Resolve `-W palavra[=valor]` por prefixo, como o mawk (`-Wv`, `-Wversion`, `-W vers`...).
fn classify_w(word: &[u8]) -> Wopt {
    let name_end = word.iter().position(|b| *b == b'=').unwrap_or(word.len());
    let name = &word[..name_end];
    if name.is_empty() {
        return Wopt::Unknown;
    }
    let table: [(&[u8], Wopt); 9] = [
        (b"version", Wopt::Version),
        (b"help", Wopt::Help),
        (b"usage", Wopt::Help),
        (b"dump", Wopt::Dump),
        (b"exec", Wopt::Exec(None)),
        (b"interactive", Wopt::Ignored),
        (b"random", Wopt::Ignored),
        (b"sprintf", Wopt::Ignored),
        (b"posix_space", Wopt::Ignored),
    ];
    for (full, w) in table {
        if full.starts_with(name) {
            return w;
        }
    }
    Wopt::Unknown
}

/// Entrada do `mawk`: devolve o código de saída.
pub fn run(sys: Arc<dyn Syscalls>, argv: Vec<Vec<u8>>) -> i32 {
    let prog = "mawk";
    let mut out: Vec<Vec<u8>> = vec![prog.as_bytes().to_vec()];
    let mut has_file = false;
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].clone();
        if a == b"--" {
            i += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        // `--version`, `--help` etc. equivalem a `-W` com a mesma palavra.
        let (c, rest): (u8, Vec<u8>) = if a.starts_with(b"--") { (b'W', a[2..].to_vec()) } else { (a[1], a[2..].to_vec()) };
        let take_value = |i: &mut usize| -> Option<Vec<u8>> {
            if !rest.is_empty() {
                Some(rest.clone())
            } else {
                *i += 1;
                argv.get(*i).cloned()
            }
        };
        match c {
            b'f' => {
                let Some(v) = take_value(&mut i) else {
                    put(&sys, Fd::STDERR, "mawk: option requires an argument -- f\n");
                    put(&sys, Fd::STDERR, &usage_text());
                    return 2;
                };
                has_file = true;
                out.push(b"-f".to_vec());
                out.push(v);
            }
            b'F' => {
                let Some(v) = take_value(&mut i) else {
                    put(&sys, Fd::STDERR, "mawk: option requires an argument -- F\n");
                    put(&sys, Fd::STDERR, &usage_text());
                    return 2;
                };
                out.push(b"-F".to_vec());
                out.push(v);
            }
            b'v' => {
                let Some(v) = take_value(&mut i) else {
                    put(&sys, Fd::STDERR, "mawk: option requires an argument -- v\n");
                    put(&sys, Fd::STDERR, &usage_text());
                    return 2;
                };
                out.push(b"-v".to_vec());
                out.push(v);
            }
            b'W' => {
                // `-W exec arq` leva o valor no argumento seguinte; as demais palavras vêm coladas.
                let word = if rest.is_empty() {
                    i += 1;
                    match argv.get(i) {
                        Some(w) => w.clone(),
                        None => {
                            put(&sys, Fd::STDERR, "mawk: option requires an argument -- W\n");
                            put(&sys, Fd::STDERR, &usage_text());
                            return 2;
                        }
                    }
                } else {
                    rest.clone()
                };
                match classify_w(&word) {
                    Wopt::Version => {
                        put(&sys, Fd::STDOUT, VERSION_TEXT);
                        return 0;
                    }
                    Wopt::Help => {
                        put(&sys, Fd::STDOUT, &usage_text());
                        return 0;
                    }
                    Wopt::Dump => {
                        // O listador de código do mawk não existe aqui: aceita e sai sem rodar.
                        return 0;
                    }
                    Wopt::Exec(_) => {
                        i += 1;
                        let Some(f) = argv.get(i).cloned() else {
                            put(&sys, Fd::STDERR, "mawk: option requires an argument -- W exec\n");
                            return 2;
                        };
                        has_file = true;
                        out.push(b"-f".to_vec());
                        out.push(f);
                        i += 1;
                        break;
                    }
                    Wopt::Ignored => {}
                    Wopt::Unknown => {
                        let w = String::from_utf8_lossy(&word).into_owned();
                        put(&sys, Fd::STDERR, &format!("mawk: vacuous option: -W {w}\n"));
                    }
                }
            }
            _ => {
                let o = String::from_utf8_lossy(&a).into_owned();
                put(&sys, Fd::STDERR, &format!("mawk: not an option: {o}\n"));
                return 2;
            }
        }
        i += 1;
    }
    if !has_file && i >= argv.len() {
        put(&sys, Fd::STDERR, &usage_text());
        return 2;
    }
    out.push(b"--".to_vec());
    out.extend(argv[i.min(argv.len())..].iter().cloned());
    crate::cli::run(sys, prog, out)
}
