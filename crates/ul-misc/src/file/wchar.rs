//! Caracteres largos no C.UTF-8 da glibc 2.41, do jeito que o `file` usa: `mbrtowc` pra
//! decodificar, `iswprint` pra decidir o escape e `wcwidth` pro alinhamento dos nomes. As classes
//! acima do ASCII vêm de uma sonda no oráculo ([`super::wctype_table`]).

use super::wctype_table::{PRINTABLE, WIDE};

fn in_ranges(c: u32, table: &[(u32, u32)]) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `iswprint()` no C.UTF-8.
pub fn iswprint(c: u32) -> bool {
    if c < 0x80 {
        return (0x20..0x7f).contains(&c);
    }
    in_ranges(c, PRINTABLE)
}

/// `wcwidth()` limitado por baixo a 1 (o `w > 0 ? w : 1` do `file_mbswidth`).
pub fn width_at_least_one(c: u32) -> usize {
    if c >= 0x80 && in_ranges(c, WIDE) {
        2
    } else {
        1
    }
}

/// Sequência multibyte inválida ou incompleta (o `(size_t)-1` e o `(size_t)-2` do `mbrtowc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadSequence;

/// Um passo do `mbrtowc`: `Ok((ponto de código, bytes))` ou `Err(BadSequence)` pra sequência
/// inválida ou incompleta.
pub fn mbrtowc(s: &[u8]) -> Result<(u32, usize), BadSequence> {
    let b0 = *s.first().ok_or(BadSequence)?;
    if b0 < 0x80 {
        return Ok((u32::from(b0), 1));
    }
    let n = match b0 {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Err(BadSequence),
    };
    if s.len() < n {
        return Err(BadSequence);
    }
    match std::str::from_utf8(&s[..n]) {
        Ok(t) => Ok((t.chars().next().map(u32::from).unwrap_or(0), n)),
        Err(_) => Err(BadSequence),
    }
}

/// `\ooo` de um byte (o `file_octal` e o `OCTALIFY`).
pub fn push_octal(out: &mut Vec<u8>, c: u8) {
    out.push(b'\\');
    out.push(b'0' + ((c >> 6) & 7));
    out.push(b'0' + ((c >> 3) & 7));
    out.push(b'0' + (c & 7));
}

/// `fname_print()`: o nome com os não imprimíveis em octal. Um caractere largo não imprimível
/// sai como o octal só do byte de baixo do ponto de código (o "XXX" do file.c).
pub fn fname_print(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    let mut i = 0;
    while i < name.len() {
        match mbrtowc(&name[i..]) {
            Err(BadSequence) => {
                push_octal(&mut out, name[i]);
                i += 1;
            }
            Ok((c, n)) => {
                if iswprint(c) {
                    out.extend_from_slice(&name[i..i + n]);
                } else {
                    push_octal(&mut out, c as u8);
                }
                i += n;
            }
        }
    }
    out
}

/// `file_mbswidth()`: colunas que o nome ocupa na saída (4 por escape).
pub fn mbswidth(name: &[u8], raw: bool) -> usize {
    let mut w = 0;
    let mut i = 0;
    while i < name.len() {
        match mbrtowc(&name[i..]) {
            Err(BadSequence) => {
                w += 4;
                i += 1;
            }
            Ok((c, n)) => {
                w += if raw || iswprint(c) {
                    width_at_least_one(c)
                } else {
                    4
                };
                i += n;
            }
        }
    }
    w
}

/// O escape do `file_getbuffer()` sobre a descrição inteira: se tudo decodifica, caracteres
/// imprimíveis passam e os outros viram octal byte a byte; se houver qualquer sequência inválida,
/// o texto inteiro é escapado byte a byte com o `isprint` (só o ASCII passa), até o primeiro NUL.
pub fn escape_output(buf: &[u8]) -> Vec<u8> {
    let buf = super::cutil::cstr(buf);
    let mut out = Vec::with_capacity(buf.len());
    let mut i = 0;
    let mut ok = true;
    while i < buf.len() {
        match mbrtowc(&buf[i..]) {
            Err(BadSequence) => {
                ok = false;
                break;
            }
            Ok((c, n)) => {
                if iswprint(c) {
                    out.extend_from_slice(&buf[i..i + n]);
                } else {
                    for &b in &buf[i..i + n] {
                        push_octal(&mut out, b);
                    }
                }
                i += n;
            }
        }
    }
    if ok {
        return out;
    }
    out.clear();
    for &b in buf {
        if super::cutil::is_print(b) {
            out.push(b);
        } else {
            push_octal(&mut out, b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_from_oracle() {
        assert!(iswprint(u32::from('é')));
        assert!(iswprint(0xa0));
        assert!(!iswprint(0x80));
        assert!(!iswprint(0x2028));
        assert!(!iswprint(0x0a));
        assert_eq!(width_at_least_one(u32::from('日')), 2);
        assert_eq!(width_at_least_one(u32::from('a')), 1);
    }

    #[test]
    fn names_and_widths() {
        assert_eq!(fname_print("café".as_bytes()), "café".as_bytes());
        assert_eq!(fname_print(b"a\x80b"), b"a\\200b");
        assert_eq!(fname_print("a\u{80}b".as_bytes()), b"a\\200b");
        // U+2028 sai só com o byte de baixo (0x28).
        assert_eq!(fname_print("x\u{2028}".as_bytes()), b"x\\050");
        assert_eq!(fname_print(b"tab\there"), b"tab\\011here");
        assert_eq!(mbswidth("日本".as_bytes(), false), 4);
        assert_eq!(mbswidth(b"a\xffb", false), 6);
        assert_eq!(mbswidth("a\u{80}".as_bytes(), true), 2);
        assert_eq!(mbswidth("a\u{80}".as_bytes(), false), 5);
    }

    #[test]
    fn output_escape() {
        assert_eq!(escape_output("ação\n".as_bytes()), "ação\\012".as_bytes());
        assert_eq!(escape_output(b"caf\xe9"), b"caf\\351");
    }
}
