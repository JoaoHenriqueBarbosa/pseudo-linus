//! `rev` do util-linux 2.41: inverte cada linha caractere a caractere.
//!
//! Comportamento medido no oráculo (Debian 13, C.UTF-8):
//!
//! - A linha é invertida por caractere largo (UTF-8); o separador fica no fim. Linha final sem
//!   separador sai sem separador.
//! - `-0`/`--zero`: o separador é o NUL.
//! - Sequência UTF-8 inválida: `rev: fgetwc() failed: Invalid or incomplete multibyte or wide
//!   character`, o arquivo é abandonado (a linha em curso não sai) e o código final é 1.
//! - Arquivo que não abre: `rev: cannot open X: <erro>`; diretório: `rev: fgetwc() failed: Is a
//!   directory`. Os outros arquivos seguem; o código final é 1.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};

use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;

const USAGE: &str = "
Usage:
 rev [options] [<file> ...]

Reverse lines characterwise.

Options:
 -0, --zero     use the NUL byte as line separator
 -h, --help     display this help
 -V, --version  display version

For more details see rev(1).
";

const LONGS: &[LongOpt] = &[
    LongOpt::new("zero", HasArg::No, b'0' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut g = Getopt::from_env(&argv[1..], "0hV", LONGS);
    let mut sep = b'\n';
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => match o.short() {
                Some('0') => sep = 0,
                Some('h') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(&USAGE.as_bytes()[1..]);
                    return 0;
                }
                Some('V') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(b"rev from util-linux 2.41.5\n");
                    return 0;
                }
                _ => return 1,
            },
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry 'rev --help' for more information.\n",
                    e.message("rev")
                ));
                return 1;
            }
        }
    }
    let files = g.operands();
    let mut out = io::stdout();
    let mut status = 0;
    if files.is_empty() {
        if reverse_fd(Fd::STDIN, sep, &mut out).is_err() {
            status = 1;
        }
    } else {
        for f in &files {
            let fd = match sys::open(f, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    let _ = out.flush();
                    io::eprint(format!(
                        "rev: cannot open {}: {}\n",
                        io::lossy(f),
                        e.message()
                    ));
                    status = 1;
                    continue;
                }
            };
            if reverse_fd(fd, sep, &mut out).is_err() {
                status = 1;
            }
            let _ = sys::close(fd);
        }
    }
    if out.flush().is_err() {
        return 1;
    }
    status
}

/// Inverte o conteúdo de um fd, linha a linha, em streaming. `Err` quando a leitura falhou (a
/// mensagem já saiu).
fn reverse_fd(fd: Fd, sep: u8, out: &mut impl Write) -> Result<(), ()> {
    let mut line: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match sys::read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(Errno::EINTR) => continue,
            Err(e) => {
                let _ = out.flush();
                io::eprint(format!("rev: fgetwc() failed: {}\n", e.message()));
                return Err(());
            }
        };
        for &b in &buf[..n] {
            if b == sep {
                emit(&line, out)?;
                let _ = out.write_all(&[sep]);
                line.clear();
            } else {
                line.push(b);
            }
        }
        sys::checkpoint();
    }
    if !line.is_empty() {
        emit(&line, out)?;
    }
    Ok(())
}

/// Escreve a linha invertida por caractere; sequência inválida é erro, como o `fgetwc` da glibc.
fn emit(line: &[u8], out: &mut impl Write) -> Result<(), ()> {
    let Ok(text) = std::str::from_utf8(line) else {
        let _ = out.flush();
        io::eprint("rev: fgetwc() failed: Invalid or incomplete multibyte or wide character\n");
        return Err(());
    };
    let reversed: String = text.chars().rev().collect();
    let _ = out.write_all(reversed.as_bytes());
    Ok(())
}
