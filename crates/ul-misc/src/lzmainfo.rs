//! `lzmainfo` do xz-utils 5.8.1: mostra o que o cabeçalho de um `.lzma` guarda.
//!
//! Comportamento medido no oráculo (Debian 13):
//!
//! - Sem operandos lê a entrada padrão e não imprime nome nem linhas em branco. Com operandos, uma
//!   linha em branco abre a saída; cada arquivo (exceto `-`) imprime o nome na primeira linha e uma
//!   linha em branco depois, também quando o cabeçalho é recusado.
//! - Menos de 13 bytes: `File is too small to be a .lzma file`; propriedades `lc/lp/pb` inválidas
//!   (byte maior ou igual a 225): `Not a .lzma file`. Os dois dão código 1.
//! - Tamanho `UINT64_MAX` sai como `Unknown`; os MB arredondam com meio MiB de folga; o dicionário
//!   mostra `2^n` com n igual ao piso do log2 (0 pra 0).
//! - Arquivo que não abre: `lzmainfo: X: <erro>` e segue; sem linha em branco depois.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};

use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;

const USAGE: &str = "Usage: lzmainfo [--help] [--version] [FILE]...
Show information stored in the .lzma file header.
With no FILE, or when FILE is -, read standard input.

Report bugs to <xz@tukaani.org> (in English or Finnish).
XZ Utils home page: <https://tukaani.org/xz/>
";

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut g = Getopt::from_env(&argv[1..], "", LONGS);
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => match o.short() {
                Some('h') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(USAGE.as_bytes());
                    let _ = out.flush();
                    return 0;
                }
                Some('V') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(b"lzmainfo (XZ Utils) 5.8.1\n");
                    let _ = out.flush();
                    return 0;
                }
                _ => return 1,
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message("lzmainfo")));
                return 1;
            }
        }
    }
    let files = g.operands();
    let mut out = io::stdout();
    let mut status = 0;
    if files.is_empty() {
        if !info(Fd::STDIN, "(stdin)", false, &mut out) {
            status = 1;
        }
    } else {
        let _ = out.write_all(b"\n");
        for f in &files {
            if &f[..] == b"-" {
                if !info(Fd::STDIN, "(stdin)", false, &mut out) {
                    status = 1;
                }
                continue;
            }
            let fd = match sys::open(f, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    io::eprint(format!("lzmainfo: {}: {}\n", io::lossy(f), e.message()));
                    status = 1;
                    continue;
                }
            };
            if !info(fd, &io::lossy(f), true, &mut out) {
                status = 1;
            }
            let _ = out.write_all(b"\n");
            let _ = sys::close(fd);
        }
    }
    if out.flush().is_err() {
        return 1;
    }
    status
}

/// Lê o cabeçalho de 13 bytes e imprime a tabela. `false` quando o arquivo foi recusado.
fn info(fd: Fd, name: &str, show_name: bool, out: &mut impl Write) -> bool {
    let mut buf = [0u8; 13];
    let mut got = 0;
    while got < buf.len() {
        match sys::read(fd, &mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(Errno::EINTR) => continue,
            Err(e) => {
                io::eprint(format!("lzmainfo: {}: {}\n", name, e.message()));
                return false;
            }
        }
    }
    if got != buf.len() {
        io::eprint(format!("lzmainfo: {name}: File is too small to be a .lzma file\n"));
        return false;
    }
    let props = buf[0] as u32;
    if props >= 9 * 5 * 5 {
        io::eprint(format!("lzmainfo: {name}: Not a .lzma file\n"));
        return false;
    }
    let lc = props % 9;
    let rest = props / 9;
    let lp = rest % 5;
    let pb = rest / 5;
    let dict = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let size = u64::from_le_bytes([buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12]]);

    let mut text = String::new();
    if show_name {
        text.push_str(name);
        text.push('\n');
    }
    text.push_str("Uncompressed size:             ");
    if size == u64::MAX {
        text.push_str("Unknown\n");
    } else {
        let mb = (size as u128 + 512 * 1024) / (1024 * 1024);
        text.push_str(&format!("{mb} MB ({size} bytes)\n"));
    }
    let mb = (dict as u64 + 512 * 1024) / (1024 * 1024);
    let log2 = if dict == 0 { 0 } else { 31 - dict.leading_zeros() };
    text.push_str(&format!("Dictionary size:               {mb} MB (2^{log2} bytes)\n"));
    text.push_str(&format!("Literal context bits (lc):     {lc}\n"));
    text.push_str(&format!("Literal pos bits (lp):         {lp}\n"));
    text.push_str(&format!("Number of pos bits (pb):       {pb}\n"));
    let _ = out.write_all(text.as_bytes());
    true
}
