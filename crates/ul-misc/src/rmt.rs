//! `rmt` do GNU tar 1.35 (instalado no Debian 13 como `/usr/sbin/rmt-tar`, com `rmt` por alternativa).
//!
//! Servidor do protocolo de fita remota: lê comandos de uma linha do stdin e responde `A<n>` ou
//! `E<errno>` seguido da mensagem. Cobre `O` (abrir), `C` (fechar), `R` (ler), `W` (escrever) e as
//! respostas de erro de `L`, `I` e `S`, que o original só atende em dispositivo de fita (aqui o
//! arquivo aberto não é uma fita, então `ioctl`/`lseek` falham como num arquivo comum).
//! Fim de entrada encerra com 0.

use std::ffi::OsString;
use std::io::{Read, Write};

use sysabi::{Errno, OFlags};

use crate::util::io::{self, File};

const HELP: &str = "Usage: rmt [OPTION...]
Manipulate a tape drive, accepting commands from a remote process

  -d, --debug=NUMBER         set debug level
  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

Report bugs to <bug-tar@gnu.org>.
";

const USAGE: &str = "Usage: rmt [-?V] [-d NUMBER] [--debug=NUMBER] [--help] [--usage] [--version]\n";

const VERSION: &str = "rmt (GNU tar) 1.35
Copyright (C) 2023 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Sergey Poznyakoff.
";

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn try_help(prog: &str) {
    io::eprint(format!("Try `{prog} --help' or `{prog} --usage' for more information.\n"));
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_slice();
        i += 1;
        match a {
            b"--help" | b"-?" => {
                let _ = io::stdout().write_all(HELP.as_bytes());
                return 0;
            }
            b"--usage" => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            b"--version" | b"-V" => {
                let _ = io::stdout().write_all(VERSION.as_bytes());
                return 0;
            }
            b"-d" | b"--debug" => {
                if i >= argv.len() {
                    if a == b"-d" {
                        io::eprint("rmt: option requires an argument -- 'd'\n");
                    } else {
                        io::eprint("rmt: option '--debug' requires an argument\n");
                    }
                    try_help("rmt");
                    return 64;
                }
                i += 1;
            }
            _ if a.starts_with(b"--debug=") || (a.starts_with(b"-d") && a.len() > 2) => {}
            _ if a.starts_with(b"--") => {
                io::eprint(format!("rmt: unrecognized option '{}'\n", io::lossy(a)));
                try_help("rmt");
                return 64;
            }
            _ if a.len() > 1 && a[0] == b'-' => {
                io::eprint(format!("rmt: invalid option -- '{}'\n", a[1] as char));
                try_help("rmt");
                return 64;
            }
            _ => {}
        }
    }
    serve()
}

struct Input {
    data: Vec<u8>,
    pos: usize,
}

impl Input {
    /// Próxima linha sem o `\n`; `None` no fim da entrada.
    fn line(&mut self) -> Option<Vec<u8>> {
        if self.pos >= self.data.len() {
            return None;
        }
        let rest = &self.data[self.pos..];
        let (line, used) = match rest.iter().position(|b| *b == b'\n') {
            Some(p) => (rest[..p].to_vec(), p + 1),
            None => (rest.to_vec(), rest.len()),
        };
        self.pos += used;
        Some(line)
    }

    fn take(&mut self, n: usize) -> Vec<u8> {
        let end = (self.pos + n).min(self.data.len());
        let v = self.data[self.pos..end].to_vec();
        self.pos = end;
        v
    }
}

fn atoi(s: &[u8]) -> i64 {
    let t = io::lossy(s);
    let t = t.trim_start();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let n: i64 = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |a, b| a.saturating_mul(10).saturating_add(i64::from(b - b'0')));
    if neg { -n } else { n }
}

fn reply(msg: &[u8]) {
    let _ = io::stdout().write_all(msg);
}

fn reply_error(e: Errno) {
    reply(format!("E{}\n{}\n", e.0, e.message()).as_bytes());
}

fn serve() -> i32 {
    let mut inp = Input {
        data: io::read_stdin().unwrap_or_default(),
        pos: 0,
    };
    let mut dev: Option<File> = None;
    loop {
        let Some(cmd) = inp.line() else {
            return 0;
        };
        let Some(&c) = cmd.first() else {
            continue;
        };
        match c {
            b'O' => {
                let Some(path) = inp.line() else { return 0 };
                let Some(mode) = inp.line() else { return 0 };
                dev = None;
                let m = atoi(&mode);
                let mut flags = match m & 3 {
                    0 => OFlags::RDONLY,
                    1 => OFlags::WRONLY,
                    _ => OFlags::RDWR,
                };
                if m & 0o100 != 0 {
                    flags = flags | OFlags::CREAT;
                }
                if m & 0o1000 != 0 {
                    flags = flags | OFlags::TRUNC;
                }
                match File::open_with(&path, flags, 0o666) {
                    Ok(f) => {
                        dev = Some(f);
                        reply(b"A0\n");
                    }
                    Err(e) => reply_error(e),
                }
            }
            b'C' => {
                let _ = inp.line();
                if dev.take().is_some() {
                    reply(b"A0\n");
                } else {
                    reply_error(Errno::EBADF);
                }
            }
            b'W' => {
                let Some(count) = inp.line() else { return 0 };
                let n = atoi(&count).max(0) as usize;
                let buf = inp.take(n);
                match dev.as_mut() {
                    None => reply_error(Errno::EBADF),
                    Some(f) => match f.write_all(&buf) {
                        Ok(()) => reply(format!("A{}\n", buf.len()).as_bytes()),
                        Err(e) => reply_error(io::io_errno(&e)),
                    },
                }
            }
            b'R' => {
                let Some(count) = inp.line() else { return 0 };
                let n = atoi(&count).max(0) as usize;
                match dev.as_mut() {
                    None => reply_error(Errno::EBADF),
                    Some(f) => {
                        let mut buf = vec![0u8; n];
                        match f.read(&mut buf) {
                            Ok(got) => {
                                let mut out = format!("A{got}\n").into_bytes();
                                out.extend_from_slice(&buf[..got]);
                                reply(&out);
                            }
                            Err(e) => reply_error(io::io_errno(&e)),
                        }
                    }
                }
            }
            b'L' => {
                let _ = inp.line();
                let _ = inp.line();
                reply_error(Errno::ESPIPE);
            }
            b'I' => {
                let _ = inp.line();
                let _ = inp.line();
                reply_error(Errno::ENOTTY);
            }
            b'S' => {
                reply_error(Errno::ENOTTY);
            }
            _ => {
                reply(b"E22\nGarbage command\n");
                return 1;
            }
        }
    }
}
