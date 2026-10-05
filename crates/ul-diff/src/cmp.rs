//! `cmp` (GNU diffutils 3.10): compara dois arquivos byte a byte.
//!
//! Formatos observados no oráculo: "A B differ: byte N, line M" (e com `-b` " is 144 d 170 x"); `-l`
//! com a coluna do byte na largura do maior deslocamento comparável e o caractere no estilo do
//! `cat -v`; "EOF on X after byte N, line M" (ou ", in line M" quando o fim cai no meio da linha, ou
//! "which is empty"); `-s` cala inclusive os erros de arquivo; saída padrão em `/dev/null` desliga a
//! contagem de linhas (otimização do GNU que aparece na mensagem de EOF). Números de `-i`, `-n` e dos
//! SKIP aceitam base do C (`0x`, `0`) e sufixos `kB`, `K`, `KiB`, `M`... até onde cabe em 63 bits.

use std::ffi::OsString;

use sysabi::{AtFlags, Ctx, Errno, Fd, FileType, Whence};

use crate::getopt::{Getopt, HasArg, Item, LongOpt};
use crate::sysutil::{self, Output};

const HELP: &str = r#"Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]
Compare two files byte by byte.

The optional SKIP1 and SKIP2 specify the number of bytes to skip
at the beginning of each file (zero by default).

Mandatory arguments to long options are mandatory for short options too.
  -b, --print-bytes          print differing bytes
  -i, --ignore-initial=SKIP         skip first SKIP bytes of both inputs
  -i, --ignore-initial=SKIP1:SKIP2  skip first SKIP1 bytes of FILE1 and
                                      first SKIP2 bytes of FILE2
  -l, --verbose              output byte numbers and differing byte values
  -n, --bytes=LIMIT          compare at most LIMIT bytes
  -s, --quiet, --silent      suppress all normal output
      --help                 display this help and exit
  -v, --version              output version information and exit

SKIP values may be followed by the following multiplicative suffixes:
kB 1000, K 1024, MB 1,000,000, M 1,048,576,
GB 1,000,000,000, G 1,073,741,824, and so on for T, P, E, Z, Y.

If a FILE is '-' or missing, read standard input.
Exit status is 0 if inputs are the same, 1 if different, 2 if trouble.

Report bugs to: bug-diffutils@gnu.org
GNU diffutils home page: <https://www.gnu.org/software/diffutils/>
General help using GNU software: <https://www.gnu.org/gethelp/>
"#;

const VERSION: &str = "cmp (GNU diffutils) 3.10
Copyright (C) 2023 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Torbjörn Granlund and David MacKenzie.
";

const HELP_ID: u32 = 1000;

const LONGS: &[LongOpt] = &[
    LongOpt::new("bytes", HasArg::Required, b'n' as u32),
    LongOpt::new("help", HasArg::No, HELP_ID),
    LongOpt::new("ignore-initial", HasArg::Required, b'i' as u32),
    LongOpt::new("print-bytes", HasArg::No, b'b' as u32),
    LongOpt::new("print-chars", HasArg::No, b'c' as u32),
    LongOpt::new("quiet", HasArg::No, b's' as u32),
    LongOpt::new("silent", HasArg::No, b's' as u32),
    LongOpt::new("verbose", HasArg::No, b'l' as u32),
    LongOpt::new("version", HasArg::No, b'v' as u32),
];

/// Número com base do C e sufixos multiplicativos (`kB` 1000, `K`/`KiB` 1024, `MB`, `M`...). `None`
/// se inválido ou se não cabe em 63 bits.
pub fn parse_number(s: &[u8]) -> Option<u64> {
    if s.is_empty() || s[0] == b'-' {
        return None;
    }
    let (radix, digits_start) = if s.len() > 1 && s[0] == b'0' && (s[1] == b'x' || s[1] == b'X') {
        (16u32, 2usize)
    } else if s[0] == b'0' {
        (8, 0)
    } else {
        (10, 0)
    };
    let mut i = digits_start;
    let mut v: u128 = 0;
    let start = i;
    while i < s.len() && (s[i] as char).is_digit(radix) {
        v = v * radix as u128 + (s[i] as char).to_digit(radix)? as u128;
        if v > i64::MAX as u128 {
            return None;
        }
        i += 1;
    }
    if i == start {
        return None;
    }
    let rest = &s[i..];
    if !rest.is_empty() {
        let power = match rest[0] {
            b'k' | b'K' => 1,
            b'M' => 2,
            b'G' => 3,
            b'T' => 4,
            b'P' => 5,
            b'E' => 6,
            b'Z' => 7,
            b'Y' => 8,
            b'R' => 9,
            b'Q' => 10,
            _ => return None,
        };
        let base: u128 = match &rest[1..] {
            b"" | b"iB" => 1024,
            b"B" => 1000,
            _ => return None,
        };
        for _ in 0..power {
            v = v.checked_mul(base)?;
            if v > i64::MAX as u128 {
                return None;
            }
        }
    }
    Some(v as u64)
}

/// Caractere no estilo `cat -v`: `^A`, `^?`, `M-^@`, `M-a`.
fn printable(c: u8) -> String {
    let mut s = String::new();
    let mut c = c;
    if c >= 0x80 {
        s.push_str("M-");
        c -= 0x80;
    }
    if c < 0x20 {
        s.push('^');
        s.push((c + 0x40) as char);
    } else if c == 0x7f {
        s.push_str("^?");
    } else {
        s.push(c as char);
    }
    s
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    FirstDiff,
    AllDiffs,
    Status,
}

struct Input {
    name: Vec<u8>,
    fd: Fd,
    owned: bool,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    eof: bool,
}

impl Input {
    /// Próximo byte disponível, lendo mais quando o buffer acaba.
    fn fill(&mut self) -> Result<(), Errno> {
        if self.pos < self.len || self.eof {
            return Ok(());
        }
        loop {
            match sysabi::sys::read(self.fd, &mut self.buf) {
                Ok(0) => {
                    self.eof = true;
                    self.pos = 0;
                    self.len = 0;
                    return Ok(());
                }
                Ok(n) => {
                    self.pos = 0;
                    self.len = n;
                    return Ok(());
                }
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e),
            }
        }
    }

    fn available(&self) -> &[u8] {
        &self.buf[self.pos..self.len]
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    let argv0 = sysutil::argv0(&argv);
    let try_help = |msg: &str| -> i32 {
        sysutil::eprint(format!("{argv0}: {msg}\n{argv0}: Try '{argv0} --help' for more information.\n"));
        2
    };
    let mut print_bytes = false;
    let mut verbose = false;
    let mut silent = false;
    let mut limit: Option<u64> = None;
    let mut skip: [Option<u64>; 2] = [None, None];
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in Getopt::from_env(&argv, "bci:ln:sv", LONGS) {
        let opt = match item {
            Ok(Item::Operand(v)) => {
                operands.push(v);
                continue;
            }
            Ok(Item::Opt(o)) => o,
            Err(e) => {
                sysutil::eprint(e.message_bytes(&argv0));
                sysutil::eprint(format!("{argv0}: Try '{argv0} --help' for more information.\n"));
                return 2;
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            x if x == b'b' as u32 || x == b'c' as u32 => print_bytes = true,
            x if x == b'i' as u32 => {
                let bad = || format!("invalid --ignore-initial value '{}'", String::from_utf8_lossy(&arg));
                let (s1, s2) = match arg.iter().position(|&b| b == b':') {
                    Some(p) => (parse_number(&arg[..p]), Some(parse_number(&arg[p + 1..]))),
                    None => (parse_number(&arg), None),
                };
                match (s1, s2) {
                    (Some(a), None) => skip = [Some(a), Some(a)],
                    (Some(a), Some(Some(b))) => skip = [Some(a), Some(b)],
                    _ => return try_help(&bad()),
                }
            }
            x if x == b'l' as u32 => verbose = true,
            x if x == b'n' as u32 => match parse_number(&arg) {
                Some(n) => limit = Some(n),
                None => return try_help(&format!("invalid --bytes value '{}'", String::from_utf8_lossy(&arg))),
            },
            x if x == b's' as u32 => silent = true,
            x if x == b'v' as u32 => {
                let mut out = Output::stdout();
                out.write_str(VERSION);
                return if out.finish().is_ok() { 0 } else { 2 };
            }
            HELP_ID => {
                let mut out = Output::stdout();
                out.write_str(HELP);
                return if out.finish().is_ok() { 0 } else { 2 };
            }
            _ => {}
        }
    }
    if verbose && silent {
        return try_help("options -l and -s are incompatible");
    }
    if operands.is_empty() {
        return try_help(&format!("missing operand after '{argv0}'"));
    }
    if operands.len() > 4 {
        return try_help(&format!("extra operand '{}'", String::from_utf8_lossy(&operands[4])));
    }
    for (k, slot) in skip.iter_mut().enumerate() {
        if let Some(s) = operands.get(2 + k) {
            match parse_number(s) {
                Some(n) => {
                    if slot.is_none() {
                        *slot = Some(n);
                    }
                }
                None => {
                    return try_help(&format!("invalid --ignore-initial value '{}'", String::from_utf8_lossy(s)));
                }
            }
        }
    }
    let kind = if silent {
        Kind::Status
    } else if verbose {
        Kind::AllDiffs
    } else {
        Kind::FirstDiff
    };
    let names = [operands[0].clone(), operands.get(1).cloned().unwrap_or_else(|| b"-".to_vec())];
    let skip = [skip[0].unwrap_or(0), skip[1].unwrap_or(0)];
    run(&argv0, kind, print_bytes, limit, &names, skip)
}

fn stdout_is_dev_null() -> bool {
    let Some(sys) = sysabi::sys::try_current() else { return false };
    let Ok(out) = sys.fstat(Fd::STDOUT) else { return false };
    let Ok(null) = sys.fstatat(Fd::CWD, b"/dev/null", AtFlags::empty()) else { return false };
    out.file_type() == FileType::CharDevice && out.dev == null.dev && out.ino == null.ino
}

fn run(argv0: &str, kind: Kind, print_bytes: bool, limit: Option<u64>, names: &[Vec<u8>; 2], skip: [u64; 2]) -> i32 {
    let sys = sysabi::sys::current();
    let quiet_errors = kind == Kind::Status;
    let fail = |name: &[u8], e: Errno| -> i32 {
        if !quiet_errors {
            sysutil::error_path(argv0, name, e);
        }
        2
    };
    let mut inputs: Vec<Input> = Vec::with_capacity(2);
    let mut stats = Vec::with_capacity(2);
    for name in names {
        let (fd, owned) = if name.as_slice() == b"-" {
            (Fd::STDIN, false)
        } else {
            match sysutil::open_read(name) {
                Ok(fd) => (fd, true),
                Err(e) => return fail(name, e),
            }
        };
        let st = match sys.fstat(fd) {
            Ok(s) => s,
            Err(e) => return fail(name, e),
        };
        stats.push(st);
        inputs.push(Input { name: name.clone(), fd, owned, buf: vec![0u8; 64 * 1024], pos: 0, len: 0, eof: false });
    }
    let close_all = |inputs: &[Input]| {
        for i in inputs {
            if i.owned {
                let _ = sysabi::sys::close(i.fd);
            }
        }
    };
    // O mesmo arquivo com o mesmo deslocamento é igual sem ler.
    if stats[0].dev == stats[1].dev
        && stats[0].ino == stats[1].ino
        && skip[0] == skip[1]
        && (names[0] != b"-" || names[1] == b"-")
    {
        close_all(&inputs);
        return 0;
    }
    // Pula o começo: lseek em arquivo regular, leitura no resto.
    for k in 0..2 {
        let mut left = skip[k];
        if left == 0 {
            continue;
        }
        if stats[k].file_type() == FileType::Regular
            && sys.lseek(inputs[k].fd, left.min(i64::MAX as u64) as i64, Whence::Cur).is_ok()
        {
            continue;
        }
        while left > 0 {
            if let Err(e) = inputs[k].fill() {
                close_all(&inputs);
                return fail(&names[k], e);
            }
            if inputs[k].eof {
                break;
            }
            let n = (inputs[k].len - inputs[k].pos).min(left.min(usize::MAX as u64) as usize);
            inputs[k].pos += n;
            left -= n as u64;
        }
    }
    // Com -s e dois arquivos regulares de tamanhos diferentes (descontados os pulos), já se sabe.
    if kind == Kind::Status && stats.iter().all(|s| s.file_type() == FileType::Regular) {
        let s0 = stats[0].size.saturating_sub(skip[0]);
        let s1 = stats[1].size.saturating_sub(skip[1]);
        let (s0, s1) = match limit {
            Some(l) => (s0.min(l), s1.min(l)),
            None => (s0, s1),
        };
        if s0 != s1 {
            close_all(&inputs);
            return 1;
        }
    }
    let count_lines = kind == Kind::FirstDiff && !stdout_is_dev_null();
    // Largura do número do byte no -l: dígitos do maior deslocamento comparável.
    let mut max = limit.unwrap_or(i64::MAX as u64);
    for k in 0..2 {
        if stats[k].file_type() == FileType::Regular {
            max = max.min(stats[k].size.saturating_sub(skip[k]));
        }
    }
    let width = max.to_string().len();
    let mut out = Output::stdout();
    let mut offset: u64 = 0;
    let mut line: u64 = 1;
    let mut last_newline = true;
    let mut status = 0;
    let mut remaining = limit;
    loop {
        if remaining == Some(0) {
            break;
        }
        for k in 0..2 {
            if let Err(e) = inputs[k].fill() {
                out.flush();
                close_all(&inputs);
                return fail(&names[k], e);
            }
        }
        let (e0, e1) = (inputs[0].eof, inputs[1].eof);
        if e0 || e1 {
            if e0 && e1 {
                break;
            }
            // Um acabou antes: EOF no menor.
            let short = if e0 { 0 } else { 1 };
            status = 1;
            if kind != Kind::Status {
                out.flush();
                let name = String::from_utf8_lossy(&inputs[short].name).into_owned();
                let msg = if offset == 0 {
                    format!("{argv0}: EOF on {name} which is empty\n")
                } else if kind == Kind::FirstDiff && count_lines {
                    if last_newline {
                        format!("{argv0}: EOF on {name} after byte {offset}, line {}\n", line - 1)
                    } else {
                        format!("{argv0}: EOF on {name} after byte {offset}, in line {line}\n")
                    }
                } else {
                    format!("{argv0}: EOF on {name} after byte {offset}\n")
                };
                sysutil::eprint(msg);
            }
            break;
        }
        let a = inputs[0].available();
        let b = inputs[1].available();
        let mut n = a.len().min(b.len());
        if let Some(r) = remaining {
            n = n.min(r.min(usize::MAX as u64) as usize);
        }
        let (a, b) = (&a[..n], &b[..n]);
        let mut first_diff = None;
        match kind {
            Kind::AllDiffs => {
                for i in 0..n {
                    if a[i] != b[i] {
                        let pos = offset + i as u64 + 1;
                        let line_text = if print_bytes {
                            format!(
                                "{pos:>width$} {:>3o} {:<4} {:>3o} {}\n",
                                a[i],
                                printable(a[i]),
                                b[i],
                                printable(b[i])
                            )
                        } else {
                            format!("{pos:>width$} {:>3o} {:>3o}\n", a[i], b[i])
                        };
                        out.write_str(&line_text);
                        status = 1;
                    }
                }
            }
            _ => {
                first_diff = a.iter().zip(b).position(|(x, y)| x != y);
            }
        }
        if let Some(i) = first_diff {
            status = 1;
            if kind == Kind::FirstDiff {
                let lines_before = a[..i].iter().filter(|&&c| c == b'\n').count() as u64;
                let pos = offset + i as u64 + 1;
                let ln = line + lines_before;
                let mut msg = format!(
                    "{} {} differ: byte {pos}, line {ln}",
                    String::from_utf8_lossy(&names[0]),
                    String::from_utf8_lossy(&names[1])
                );
                if print_bytes {
                    msg.push_str(&format!(" is {:>3o} {} {:>3o} {}", a[i], printable(a[i]), b[i], printable(b[i])));
                }
                msg.push('\n');
                out.write_str(&msg);
            }
            break;
        }
        if count_lines {
            line += a.iter().filter(|&&c| c == b'\n').count() as u64;
        }
        if let Some(&c) = a.last() {
            last_newline = c == b'\n';
        }
        offset += n as u64;
        inputs[0].pos += n;
        inputs[1].pos += n;
        if let Some(r) = remaining.as_mut() {
            *r -= n as u64;
        }
        sysabi::sys::checkpoint();
    }
    close_all(&inputs);
    match out.finish() {
        Ok(()) => status,
        Err(e) => {
            sysutil::error(argv0, format!("write error: {}", e.message()));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_gnu() {
        assert_eq!(parse_number(b"1k"), Some(1024));
        assert_eq!(parse_number(b"1kB"), Some(1000));
        assert_eq!(parse_number(b"1KiB"), Some(1024));
        assert_eq!(parse_number(b"1MB"), Some(1_000_000));
        assert_eq!(parse_number(b"0x10"), Some(16));
        assert_eq!(parse_number(b"010"), Some(8));
        assert_eq!(parse_number(b"1m"), None);
        assert_eq!(parse_number(b"1Z"), None);
        assert_eq!(parse_number(b"1E"), Some(1 << 60));
        assert_eq!(parse_number(b"-1"), None);
        assert_eq!(parse_number(b"0x"), None);
    }

    #[test]
    fn cat_v_style() {
        assert_eq!(printable(1), "^A");
        assert_eq!(printable(0x7f), "^?");
        assert_eq!(printable(0x80), "M-^@");
        assert_eq!(printable(0xff), "M-^?");
        assert_eq!(printable(b'x'), "x");
    }
}
