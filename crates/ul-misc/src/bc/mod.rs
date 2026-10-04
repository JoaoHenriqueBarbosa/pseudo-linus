//! `bc` do GNU bc 1.07.1 (Debian 13), escrito a partir do manual e do comportamento medido no
//! oráculo: léxico, gramática LALR com a recuperação de erro do bison, bytecode com os endereços do
//! GNU nas mensagens de erro, escopo dinâmico de variáveis e a biblioteca matemática do `-l`.
//!
//! Opções: `-h`, `-i`, `-l`, `-q`, `-s`, `-w`, `-v` (e as longas); `BC_ENV_ARGS` entra antes dos
//! argumentos, `POSIXLY_CORRECT` liga o `-s`, `BC_LINE_LENGTH` dá a largura da saída. Os arquivos são
//! lidos em ordem e depois o stdin.

pub mod compile;
pub mod exec;
pub mod grammar;
pub mod lalr;
pub mod lexer;
pub mod number;
pub mod output;

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Fd, sys};

use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;

const USAGE: &str = "usage: bc [options] [file ...]
  -h  --help         print this usage and exit
  -i  --interactive  force interactive mode
  -l  --mathlib      use the predefined math routines
  -q  --quiet        don't print initial banner
  -s  --standard     non-standard bc constructs are errors
  -w  --warn         warn about non-standard bc constructs
  -v  --version      print version information and exit
";

const VERSION: &str = "bc 1.07.1
Copyright 1991-1994, 1997, 1998, 2000, 2004, 2006, 2008, 2012-2017 Free Software Foundation, Inc.
";

const BANNER_TAIL: &str = "This is free software with ABSOLUTELY NO WARRANTY.
For details type `warranty'.
";

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("interactive", HasArg::No, b'i' as i32),
    LongOpt::new("mathlib", HasArg::No, b'l' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("standard", HasArg::No, b's' as i32),
    LongOpt::new("warn", HasArg::No, b'w' as i32),
    LongOpt::new("version", HasArg::No, b'v' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let mut argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    // BC_ENV_ARGS: palavras separadas por espaço, antes dos argumentos da linha de comando.
    if let Some(env) = sys::getenv("BC_ENV_ARGS") {
        let words: Vec<Vec<u8>> =
            env.split(|c| matches!(c, b' ' | b'\t' | b'\n')).filter(|w| !w.is_empty()).map(<[u8]>::to_vec).collect();
        let at = argv.len().min(1);
        argv.splice(at..at, words);
    }
    let rest = if argv.is_empty() { &argv[..] } else { &argv[1..] };
    let mut g = Getopt::from_env(rest, "chilqswv", LONGS);
    let (mut interactive, mut mathlib, mut quiet, mut std_only, mut warn) = (false, false, false, false, false);
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => match o.short() {
                Some('h') => {
                    let _ = io::stdout().write_all(USAGE.as_bytes());
                    return 0;
                }
                Some('i') => interactive = true,
                Some('l') => mathlib = true,
                Some('q') => quiet = true,
                Some('s') => std_only = true,
                Some('w') => warn = true,
                Some('v') => {
                    let _ = io::stdout().write_all(VERSION.as_bytes());
                    return 0;
                }
                _ => {}
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 1;
            }
        }
    }
    if sys::getenv("POSIXLY_CORRECT").is_some() {
        std_only = true;
    }
    let stdin_tty = sys::try_current().is_some_and(|s| s.isatty(Fd::STDIN));
    let interactive = interactive || stdin_tty;
    let line_size = output::line_length_from_env(sys::getenv("BC_LINE_LENGTH").as_deref());
    let mut vm = exec::Vm::new(line_size);
    if interactive && !quiet {
        vm.out.raw(VERSION.as_bytes());
        vm.out.raw(BANNER_TAIL.as_bytes());
    }
    if mathlib {
        vm.install_mathlib();
    }
    // O readline ecoa a linha quando a entrada não é um terminal.
    let lex = lexer::Lexer::new(g.operands(), std_only, interactive && !stdin_tty);
    let mut c = compile::Compiler::new(lex, vm, std_only, warn);
    let bc = grammar::get();
    lalr::parse(&bc.tables, &mut c, bc.eof, bc.error);
    let code = c.exit.unwrap_or(0);
    c.vm.out.flush();
    code
}
