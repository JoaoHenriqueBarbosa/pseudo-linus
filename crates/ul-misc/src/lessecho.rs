//! `lessecho` do less 668 (pacote less do Debian 13): imprime os argumentos de arquivo na saída
//! padrão, protegendo com aspas os que contêm metacaracteres, ou escapando cada metacaractere.
//!
//! As opções são de uma só letra com o valor colado (`-ox`, `-cx`, `-pn`, `-dn`, `-mx`, `-nn`,
//! `-ex`, `-fn`, `-a`) e não passam por getopt: o `--` encerra as opções, `--version` imprime a
//! revisão do arquivo (1.15) e `--help` e `-?` o uso. Os números aceitam a base automática do
//! `lstrtol` do original (`0x` hexadecimal, `0` octal) e são truncados para um byte, como o
//! `(char)` do C.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;

const USAGE: &str =
    "usage: lessecho [-ox] [-cx] [-pn] [-dn] [-mx] [-nn] [-ex] [-fn] [-a] file ...\n";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Byte na posição `i`, ou NUL além do fim (o terminador da string em C).
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `lstrtol` do original com base 0. Devolve o valor e o índice onde parou (depois de pular os
/// espaços finais).
fn lstrtol(s: &[u8]) -> (i64, usize) {
    let mut i = 0;
    while matches!(at(s, i), b' ' | b'\t') {
        i += 1;
    }
    let mut neg = false;
    if at(s, i) == b'-' {
        neg = true;
        i += 1;
    } else if at(s, i) == b'+' {
        i += 1;
    }
    let mut radix: i64 = 10;
    if at(s, i) == b'0' {
        i += 1;
        if at(s, i) == b'x' {
            radix = 16;
            i += 1;
        } else {
            radix = 8;
        }
    }
    let mut n: i64 = 0;
    loop {
        let c = at(s, i);
        let v = match c {
            b'0'..=b'9' => (c - b'0') as i64,
            b'a'..=b'f' => (c - b'a') as i64 + 10,
            b'A'..=b'F' => (c - b'A') as i64 + 10,
            _ => break,
        };
        if v >= radix {
            break;
        }
        n = n.wrapping_mul(radix).wrapping_add(v);
        i += 1;
    }
    while matches!(at(s, i), b' ' | b'\t') {
        i += 1;
    }
    (if neg { n.wrapping_neg() } else { n }, i)
}

fn fail(msg: &str) -> i32 {
    io::eprint(format!("{msg}\n"));
    1
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut quote_all = false;
    let mut openquote: u8 = b'"';
    let mut closequote: u8 = b'"';
    let mut meta_escape: Vec<u8> = b"\\".to_vec();
    // Como `metachars` é uma string em C, um NUL adicionado corta o que vem depois.
    let mut metachars: Vec<u8> = Vec::new();
    let mut no_more_options = false;

    let mut idx = 1;
    while idx < argv.len() {
        let arg = &argv[idx];
        if at(arg, 0) != b'-' || no_more_options {
            break;
        }
        idx += 1;
        let rest = &arg[1..];
        match at(rest, 0) {
            b'a' => quote_all = true,
            b'c' => closequote = at(rest, 1),
            b'd' => {
                let (v, end) = lstrtol(&rest[1.min(rest.len())..]);
                closequote = v as u8;
                if end == 0 {
                    return fail("Missing number after -d");
                }
            }
            b'e' => {
                let v = &rest[1..];
                meta_escape = if v == b"-" { Vec::new() } else { v.to_vec() };
            }
            b'f' => {
                let (v, end) = lstrtol(&rest[1..]);
                let b = v as u8;
                meta_escape = if b == 0 { Vec::new() } else { vec![b] };
                if end == 0 {
                    return fail("Missing number after -f");
                }
            }
            b'o' => openquote = at(rest, 1),
            b'p' => {
                let (v, end) = lstrtol(&rest[1..]);
                openquote = v as u8;
                if end == 0 {
                    return fail("Missing number after -p");
                }
            }
            b'm' => metachars.push(at(rest, 1)),
            b'n' => {
                let (v, end) = lstrtol(&rest[1..]);
                metachars.push(v as u8);
                if end == 0 {
                    return fail("Missing number after -n");
                }
            }
            b'?' => {
                io::eprint(USAGE.to_string());
                return 0;
            }
            b'-' => {
                let long = &rest[1..];
                if long.is_empty() {
                    no_more_options = true;
                } else if long == b"version" {
                    let mut out = io::stdout();
                    let _ = out.write_all(b"1.15\n");
                    return if out.flush().is_err() { 1 } else { 0 };
                } else if long == b"help" {
                    io::eprint(USAGE.to_string());
                    return 0;
                } else {
                    return fail("Invalid option after --");
                }
            }
            _ => return fail("Invalid option letter"),
        }
    }

    let visible: Vec<u8> = metachars.iter().copied().take_while(|&b| b != 0).collect();
    let is_meta = |b: u8| visible.contains(&b);

    let mut buf: Vec<u8> = Vec::new();
    let files = &argv[idx..];
    for (k, arg) in files.iter().enumerate() {
        let has_meta = arg.iter().any(|&b| is_meta(b));
        if quote_all || (has_meta && meta_escape.is_empty()) {
            buf.push(openquote);
            buf.extend_from_slice(arg);
            buf.push(closequote);
        } else {
            for &b in arg {
                if is_meta(b) {
                    buf.extend_from_slice(&meta_escape);
                }
                buf.push(b);
            }
        }
        buf.push(if k + 1 < files.len() { b' ' } else { b'\n' });
    }
    let mut out = io::stdout();
    let _ = out.write_all(&buf);
    if out.flush().is_err() {
        return 1;
    }
    0
}
