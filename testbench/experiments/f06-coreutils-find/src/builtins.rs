//! Programas mínimos escritos à mão pra tabela da bancada, só os que `xargs` e `find -exec` precisam
//! e que não fazem parte do porte medido: `echo` (o comando padrão do xargs). Seguem o `echo` do
//! coreutils (não o builtin do bash): `-n`, `-e`, `-E` só quando o argumento inteiro é feito dessas
//! letras.

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

pub fn echo(args: Vec<OsString>) -> i32 {
    let mut newline = true;
    let mut escapes = false;
    let mut words: &[OsString] = args.get(1..).unwrap_or(&[]);
    while let Some(first) = words.first() {
        let b = first.as_bytes();
        if b.len() < 2 || b[0] != b'-' || !b[1..].iter().all(|c| matches!(c, b'n' | b'e' | b'E')) {
            break;
        }
        for c in &b[1..] {
            match c {
                b'n' => newline = false,
                b'e' => escapes = true,
                _ => escapes = false,
            }
        }
        words = &words[1..];
    }
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        if escapes {
            if !unescape(w.as_bytes(), &mut out) {
                newline = false;
                break;
            }
        } else {
            out.extend_from_slice(w.as_bytes());
        }
    }
    if newline {
        out.push(b'\n');
    }
    let _ = sysio::io::stdout().write_all(&out);
    0
}

/// Devolve false quando encontra `\c` (para tudo, inclusive a quebra de linha final).
fn unescape(s: &[u8], out: &mut Vec<u8>) -> bool {
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' || i + 1 == s.len() {
            out.push(s[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = s[i];
        i += 1;
        match c {
            b'\\' => out.push(b'\\'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'c' => return false,
            b'e' => out.push(27),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'0' => {
                let mut v: u32 = 0;
                let mut n = 0;
                while n < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                    v = v * 8 + u32::from(s[i] - b'0');
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            b'x' if i < s.len() && s[i].is_ascii_hexdigit() => {
                let mut v: u32 = 0;
                let mut n = 0;
                while n < 2 && i < s.len() && s[i].is_ascii_hexdigit() {
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap_or(0);
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    true
}
