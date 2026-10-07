//! `sed` (GNU sed 4.9): opções, montagem do script a partir de `-e`, `-f` e do operando, e a
//! execução sobre os arquivos.
//!
//! - [`script`]: análise do script.
//! - [`exec`]: execução linha a linha.
//! - [`escape`]: sequências de escape do GNU.

pub mod escape;
pub mod exec;
pub mod script;

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};

use ul_common::getopt::{Getopt, HasArg, LongOpt};

use crate::io::{error, errno_msg, read_all};
use exec::{Exec, RunOptions};
use script::{Origin, ParseOptions, Parser};

const USAGE: &str = include_str!("usage.txt");

const VERSION: &str = "sed (GNU sed) 4.9
Packaged by Debian
Copyright (C) 2022 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Jay Fenlason, Tom Lord, Ken Pizzini,
Paolo Bonzini, Jim Meyering, and Assaf Gordon.

This sed program was built without SELinux support.

GNU sed home page: <https://www.gnu.org/software/sed/>.
General help using GNU software: <https://www.gnu.org/gethelp/>.
E-mail bug reports to: <bug-sed@gnu.org>.
";

/// Código de uso inválido (`EXIT_BAD_USAGE`).
const EXIT_BAD_USAGE: i32 = 1;
/// `panic()` do sed: arquivo de script que não abre, erro de E/S.
const EXIT_PANIC: i32 = 4;

// Opções longas sem equivalente curto.
const DEBUG_OPTION: i32 = 256;
const SANDBOX_OPTION: i32 = 257;
const POSIX_OPTION: i32 = 258;
const FOLLOW_SYMLINKS_OPTION: i32 = 259;
const HELP_OPTION: i32 = 260;
const VERSION_OPTION: i32 = 261;

const fn c(ch: u8) -> i32 {
    ch as i32
}

const LONG_OPTIONS: &[LongOpt] = &[
    LongOpt::new("binary", HasArg::No, c(b'b')),
    LongOpt::new("regexp-extended", HasArg::No, c(b'r')),
    LongOpt::new("debug", HasArg::No, DEBUG_OPTION),
    LongOpt::new("expression", HasArg::Required, c(b'e')),
    LongOpt::new("file", HasArg::Required, c(b'f')),
    LongOpt::new("in-place", HasArg::Optional, c(b'i')),
    LongOpt::new("line-length", HasArg::Required, c(b'l')),
    LongOpt::new("null-data", HasArg::No, c(b'z')),
    LongOpt::new("zero-terminated", HasArg::No, c(b'z')),
    LongOpt::new("quiet", HasArg::No, c(b'n')),
    LongOpt::new("posix", HasArg::No, POSIX_OPTION),
    LongOpt::new("silent", HasArg::No, c(b'n')),
    LongOpt::new("sandbox", HasArg::No, SANDBOX_OPTION),
    LongOpt::new("separate", HasArg::No, c(b's')),
    LongOpt::new("unbuffered", HasArg::No, c(b'u')),
    LongOpt::new("version", HasArg::No, VERSION_OPTION),
    LongOpt::new("help", HasArg::No, HELP_OPTION),
    LongOpt::new("follow-symlinks", HasArg::No, FOLLOW_SYMLINKS_OPTION),
];

const SHORT_OPTIONS: &str = "bsnrzuEe:f:l:i::";

/// Um pedaço do script, na ordem da linha de comando.
enum Piece {
    Expr(Vec<u8>),
    File(Vec<u8>),
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().skip(1).map(|a| a.as_bytes().to_vec()).collect();
    run(&argv)
}

fn usage_error() -> i32 {
    crate::io::stderr(USAGE.as_bytes());
    EXIT_BAD_USAGE
}

fn run(args: &[Vec<u8>]) -> i32 {
    let prog: &[u8] = b"sed";
    let posixly_correct = sys::getenv("POSIXLY_CORRECT").is_some();
    let mut g = Getopt::new(args, SHORT_OPTIONS, LONG_OPTIONS, posixly_correct);

    let mut parse = ParseOptions { posixly_correct, ..ParseOptions::default() };
    let mut run = RunOptions { line_len: 70, ..RunOptions::default() };
    // `COLS` muda a largura do `l` (o sed usa COLS-1).
    if let Some(cols) = sys::getenv("COLS")
        && let Ok(n) = String::from_utf8_lossy(&cols).trim().parse::<usize>()
        && n > 1
    {
        run.line_len = n - 1;
    }
    let mut pieces: Vec<Piece> = Vec::new();

    while let Some(opt) = g.next_opt() {
        let opt = match opt {
            Ok(o) => o,
            Err(e) => {
                error(prog, &e.detail());
                return usage_error();
            }
        };
        let arg = opt.arg.unwrap_or_default();
        match opt.id {
            x if x == c(b'b') => {}
            x if x == c(b'n') => run.quiet = true,
            x if x == c(b'e') => pieces.push(Piece::Expr(arg)),
            x if x == c(b'f') => pieces.push(Piece::File(arg)),
            x if x == c(b'i') => {
                run.separate = true;
                run.in_place = Some(arg);
            }
            x if x == c(b'l') => run.line_len = atoi(&arg),
            x if x == c(b'E') || x == c(b'r') => parse.extended = true,
            x if x == c(b's') => run.separate = true,
            x if x == c(b'u') => run.unbuffered = true,
            x if x == c(b'z') => {
                parse.null_data = true;
                run.null_data = true;
            }
            DEBUG_OPTION => run.debug = true,
            SANDBOX_OPTION => parse.sandbox = true,
            POSIX_OPTION => parse.posix = true,
            FOLLOW_SYMLINKS_OPTION => run.follow_symlinks = true,
            HELP_OPTION => {
                let _ = sys::write_all(Fd::STDOUT, USAGE.as_bytes());
                return 0;
            }
            VERSION_OPTION => {
                let _ = sys::write_all(Fd::STDOUT, VERSION.as_bytes());
                return 0;
            }
            _ => return usage_error(),
        }
    }
    let mut operands = g.operands();
    if pieces.is_empty() {
        if operands.is_empty() {
            return usage_error();
        }
        pieces.push(Piece::Expr(operands.remove(0)));
    }
    run.posix_n = parse.posix || posixly_correct;

    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(sys::checkpoint);
    let mut parser = Parser::new(parse, hook);
    for piece in pieces {
        let result = match piece {
            Piece::Expr(text) => {
                let origin = parser.next_expr();
                parser.chunk(origin, &text)
            }
            Piece::File(name) => {
                let data = match read_script(&name) {
                    Ok(d) => d,
                    Err(e) => {
                        let mut m = b"couldn't open file ".to_vec();
                        m.extend_from_slice(&errno_msg(&name, e));
                        error(prog, &m);
                        return EXIT_PANIC;
                    }
                };
                // Como o `ck_fopen` + `read_text`: o newline final do arquivo não conta.
                parser.chunk(Origin::File(name), &data)
            }
        };
        if let Err(e) = result {
            error(prog, &e.msg);
            return e.status;
        }
    }
    let program = match parser.finish() {
        Ok(p) => p,
        Err(e) => {
            error(prog, &e.msg);
            return e.status;
        }
    };
    if program.quiet {
        run.quiet = true;
    }
    if operands.is_empty() {
        if run.in_place.is_some() {
            error(prog, b"no input files");
            return EXIT_PANIC;
        }
        operands.push(b"-".to_vec());
    }
    let mut exec = Exec::new(&program, &run, prog);
    exec.run(&operands)
}

/// Conteúdo de um `-f` (`-` é a entrada padrão).
fn read_script(name: &[u8]) -> Result<Vec<u8>, Errno> {
    if name == b"-" {
        return read_all(Fd::STDIN);
    }
    let fd = sys::open(name, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
    let data = read_all(fd);
    let _ = sys::close(fd);
    data
}

/// `atoi` da libc: espaços, sinal e dígitos; o resto é ignorado.
fn atoi(s: &[u8]) -> usize {
    let t = String::from_utf8_lossy(s);
    let t = t.trim_start();
    let digits: String = t.trim_start_matches('+').chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}
