// Porte para Rust do encoding.c do file 5.46.
//
// Copyright (c) Ian F. Darwin 1986-1995.
// Software written by Ian F. Darwin and others;
// maintained 1995-present by Christos Zoulas and others.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice immediately at the beginning of the file, without modification,
//    this list of conditions, and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
// OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
// HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
// OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
// SUCH DAMAGE.

//! Detecção da codificação de texto (`file_encoding`): ASCII, UTF-7, UTF-8 com e sem BOM,
//! UTF-32/UTF-16 com BOM, ISO-8859, "Non-ISO extended-ASCII" e EBCDIC. Quando reconhece, deixa o
//! texto convertido em pontos de código (um por caractere), que o `ascmagic` usa pra contar linhas
//! e o softmagic de texto recebe reconvertido em UTF-8.

/// Classes do `text_chars`: nunca aparece em texto, ASCII, ISO-8859, estendido (Mac, IBM PC).
const F: u8 = 0;
const T: u8 = 1;
const I: u8 = 2;
const X: u8 = 3;

#[rustfmt::skip]
const TEXT_CHARS: [u8; 256] = [
    F, F, F, F, F, F, F, T, T, T, T, T, T, T, F, F,
    F, F, F, F, F, F, F, F, F, F, F, T, F, F, F, F,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, F,
    X, X, X, X, X, T, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
];

/// O que `file_encoding` descobre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoding {
    /// Parece texto (o `looks_text`/retorno do C).
    pub text: bool,
    /// Nome pra descrição ("ASCII", "Unicode text, UTF-8"...).
    pub code: &'static str,
    /// Charset do `--mime-encoding`.
    pub code_mime: &'static str,
    /// "text" ou "binary".
    pub kind: &'static str,
    /// Texto em pontos de código.
    pub ubuf: Vec<u32>,
}

fn looks(buf: &[u8], ubuf: &mut Vec<u32>, ok: impl Fn(u8) -> bool) -> bool {
    ubuf.clear();
    for &b in buf {
        if !ok(TEXT_CHARS[usize::from(b)]) {
            return false;
        }
        ubuf.push(u32::from(b));
    }
    true
}

fn looks_ascii(buf: &[u8], ubuf: &mut Vec<u32>) -> bool {
    looks(buf, ubuf, |t| t == T)
}

fn looks_latin1(buf: &[u8], ubuf: &mut Vec<u32>) -> bool {
    looks(buf, ubuf, |t| t == T || t == I)
}

fn looks_extended(buf: &[u8], ubuf: &mut Vec<u32>) -> bool {
    looks(buf, ubuf, |t| t != F)
}

// Tabela de primeiro byte do UTF-8 (a do Go que o file copiou).
const XX: u8 = 0xF1;
const AS: u8 = 0xF0;
const S1: u8 = 0x02;
const S2: u8 = 0x13;
const S3: u8 = 0x03;
const S4: u8 = 0x23;
const S5: u8 = 0x34;
const S6: u8 = 0x04;
const S7: u8 = 0x44;

#[rustfmt::skip]
const FIRST: [u8; 256] = [
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS,
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX,
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX,
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX,
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX,
    XX, XX, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1,
    S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1,
    S2, S3, S3, S3, S3, S3, S3, S3, S3, S3, S3, S3, S3, S4, S3, S3,
    S5, S6, S6, S6, S7, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX,
];

/// `accept_ranges`: faixa válida do segundo byte; o resto do vetor de 16 é zero no C.
const ACCEPT: [(u8, u8); 16] = [
    (0x80, 0xBF),
    (0xA0, 0xBF),
    (0x80, 0x9F),
    (0x90, 0xBF),
    (0x80, 0x8F),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
];

/// `file_looks_utf8()`: -1 UTF-8 inválido, 0 controle estranho, 1 ASCII de 7 bits, 2 UTF-8 com
/// bytes altos válidos.
pub fn looks_utf8(buf: &[u8], mut ubuf: Option<&mut Vec<u32>>) -> i32 {
    if let Some(u) = ubuf.as_deref_mut() {
        u.clear();
    }
    let mut gotone = false;
    let mut ctrl = false;
    let mut i = 0usize;
    let n = buf.len();
    while i < n {
        let b = buf[i];
        if b & 0x80 == 0 {
            if TEXT_CHARS[usize::from(b)] != T {
                ctrl = true;
            }
            if let Some(u) = ubuf.as_deref_mut() {
                u.push(u32::from(b));
            }
        } else if b & 0x40 == 0 {
            return -1;
        } else {
            let x = FIRST[usize::from(b)];
            let ar = ACCEPT[usize::from(x >> 4)];
            if x == XX {
                return -1;
            }
            let (mut c, following) = if b & 0x20 == 0 {
                (u32::from(b & 0x1f), 1)
            } else if b & 0x10 == 0 {
                (u32::from(b & 0x0f), 2)
            } else if b & 0x08 == 0 {
                (u32::from(b & 0x07), 3)
            } else if b & 0x04 == 0 {
                (u32::from(b & 0x03), 4)
            } else if b & 0x02 == 0 {
                (u32::from(b & 0x01), 5)
            } else {
                return -1;
            };
            for k in 0..following {
                i += 1;
                if i >= n {
                    return if ctrl {
                        0
                    } else if gotone {
                        2
                    } else {
                        1
                    };
                }
                let nb = buf[i];
                if k == 0 && (nb < ar.0 || nb > ar.1) {
                    return -1;
                }
                if nb & 0x80 == 0 || nb & 0x40 != 0 {
                    return -1;
                }
                c = (c << 6) + u32::from(nb & 0x3f);
            }
            if let Some(u) = ubuf.as_deref_mut() {
                u.push(c);
            }
            gotone = true;
        }
        i += 1;
    }
    if ctrl {
        0
    } else if gotone {
        2
    } else {
        1
    }
}

fn looks_utf8_with_bom(buf: &[u8], ubuf: &mut Vec<u32>) -> i32 {
    if buf.len() > 3 && buf[0] == 0xef && buf[1] == 0xbb && buf[2] == 0xbf {
        looks_utf8(&buf[3..], Some(ubuf))
    } else {
        -1
    }
}

fn looks_utf7(buf: &[u8], ubuf: &mut Vec<u32>) -> i32 {
    if buf.len() > 4 && buf[0] == b'+' && buf[1] == b'/' && buf[2] == b'v' {
        match buf[3] {
            b'8' | b'9' | b'+' | b'/' => {
                ubuf.clear();
                1
            }
            _ => -1,
        }
    } else {
        -1
    }
}

fn looks_ucs16(bf: &[u8], ubf: &mut Vec<u32>) -> i32 {
    if bf.len() < 2 {
        return 0;
    }
    let bigend = if bf[0] == 0xff && bf[1] == 0xfe {
        false
    } else if bf[0] == 0xfe && bf[1] == 0xff {
        true
    } else {
        return 0;
    };
    ubf.clear();
    let mut hi: u32 = 0;
    let mut i = 2usize;
    while i + 1 < bf.len() {
        let mut uc = if bigend {
            u32::from(bf[i + 1]) | (u32::from(bf[i]) << 8)
        } else {
            u32::from(bf[i]) | (u32::from(bf[i + 1]) << 8)
        };
        uc &= 0xffff;
        if uc == 0xfffe || uc == 0xffff || (0xfdd0..=0xfdef).contains(&uc) {
            return 0;
        }
        if hi != 0 {
            if !(0xdc00..=0xdfff).contains(&uc) {
                return 0;
            }
            uc = 0x10000 + 0x400 * (hi - 1) + (uc - 0xdc00);
            hi = 0;
        }
        if uc < 128 && TEXT_CHARS[uc as usize] != T {
            return 0;
        }
        ubf.push(uc);
        if (0xd800..=0xdbff).contains(&uc) {
            hi = uc - 0xd800 + 1;
        }
        if (0xdc00..=0xdfff).contains(&uc) {
            return 0;
        }
        i += 2;
    }
    1 + i32::from(bigend)
}

fn looks_ucs32(bf: &[u8], ubf: &mut Vec<u32>) -> i32 {
    if bf.len() < 4 {
        return 0;
    }
    let bigend = if bf[0] == 0xff && bf[1] == 0xfe && bf[2] == 0 && bf[3] == 0 {
        false
    } else if bf[0] == 0 && bf[1] == 0 && bf[2] == 0xfe && bf[3] == 0xff {
        true
    } else {
        return 0;
    };
    ubf.clear();
    let mut i = 4usize;
    while i + 3 < bf.len() {
        let v = if bigend {
            u32::from_be_bytes([bf[i], bf[i + 1], bf[i + 2], bf[i + 3]])
        } else {
            u32::from_le_bytes([bf[i], bf[i + 1], bf[i + 2], bf[i + 3]])
        };
        ubf.push(v);
        if v == 0xfffe {
            return 0;
        }
        if v < 128 && TEXT_CHARS[v as usize] != T {
            return 0;
        }
        i += 4;
    }
    1 + i32::from(bigend)
}

/// Tabela EBCDIC para ASCII estendido (a do `dd` do POSIX).
#[rustfmt::skip]
const EBCDIC_TO_ASCII: [u8; 256] = [
      0,   1,   2,   3, 156,   9, 134, 127, 151, 141, 142,  11,  12,  13,  14,  15,
     16,  17,  18,  19, 157, 133,   8, 135,  24,  25, 146, 143,  28,  29,  30,  31,
    128, 129, 130, 131, 132,  10,  23,  27, 136, 137, 138, 139, 140,   5,   6,   7,
    144, 145,  22, 147, 148, 149, 150,   4, 152, 153, 154, 155,  20,  21, 158,  26,
    b' ', 160, 161, 162, 163, 164, 165, 166, 167, 168, 213, b'.', b'<', b'(', b'+', b'|',
    b'&', 169, 170, 171, 172, 173, 174, 175, 176, 177, b'!', b'$', b'*', b')', b';', b'~',
    b'-', b'/', 178, 179, 180, 181, 182, 183, 184, 185, 203, b',', b'%', b'_', b'>', b'?',
    186, 187, 188, 189, 190, 191, 192, 193, 194, b'`', b':', b'#', b'@', b'\'', b'=', b'"',
    195, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', 196, 197, 198, 199, 200, 201,
    202, b'j', b'k', b'l', b'm', b'n', b'o', b'p', b'q', b'r', b'^', 204, 205, 206, 207, 208,
    209, 229, b's', b't', b'u', b'v', b'w', b'x', b'y', b'z', 210, 211, 212, b'[', 214, 215,
    216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, b']', 230, 231,
    b'{', b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', 232, 233, 234, 235, 236, 237,
    b'}', b'J', b'K', b'L', b'M', b'N', b'O', b'P', b'Q', b'R', 238, 239, 240, 241, 242, 243,
    b'\\', 159, b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z', 244, 245, 246, 247, 248, 249,
    b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', 250, 251, 252, 253, 254, 255,
];

/// `file_encoding()`. `encoding_max` é o parâmetro `-P encoding` (65536 por padrão).
pub fn file_encoding(buf: &[u8], encoding_max: usize) -> Encoding {
    let buf = &buf[..buf.len().min(encoding_max)];
    let mut ubuf: Vec<u32> = Vec::new();
    if ubuf.try_reserve(buf.len() + 1).is_err() {
        return Encoding {
            text: false,
            code: "unknown",
            code_mime: "binary",
            kind: "text",
            ubuf,
        };
    }
    let enc = |code: &'static str, mime: &'static str, ubuf: Vec<u32>| Encoding {
        text: true,
        code,
        code_mime: mime,
        kind: "text",
        ubuf,
    };
    if looks_ascii(buf, &mut ubuf) {
        if looks_utf7(buf, &mut ubuf) > 0 {
            return enc("Unicode text, UTF-7", "utf-7", ubuf);
        }
        return enc("ASCII", "us-ascii", ubuf);
    }
    if looks_utf8_with_bom(buf, &mut ubuf) > 0 {
        return enc("Unicode text, UTF-8 (with BOM)", "utf-8", ubuf);
    }
    if looks_utf8(buf, Some(&mut ubuf)) > 1 {
        return enc("Unicode text, UTF-8", "utf-8", ubuf);
    }
    match looks_ucs32(buf, &mut ubuf) {
        1 => return enc("Unicode text, UTF-32, little-endian", "utf-32le", ubuf),
        2 => return enc("Unicode text, UTF-32, big-endian", "utf-32be", ubuf),
        _ => {}
    }
    match looks_ucs16(buf, &mut ubuf) {
        1 => return enc("Unicode text, UTF-16, little-endian", "utf-16le", ubuf),
        2 => return enc("Unicode text, UTF-16, big-endian", "utf-16be", ubuf),
        _ => {}
    }
    if looks_latin1(buf, &mut ubuf) {
        return enc("ISO-8859", "iso-8859-1", ubuf);
    }
    if looks_extended(buf, &mut ubuf) {
        return enc("Non-ISO extended-ASCII", "unknown-8bit", ubuf);
    }
    let nbuf: Vec<u8> = buf
        .iter()
        .map(|&b| EBCDIC_TO_ASCII[usize::from(b)])
        .collect();
    if looks_ascii(&nbuf, &mut ubuf) {
        return enc("EBCDIC", "ebcdic", ubuf);
    }
    if looks_latin1(&nbuf, &mut ubuf) {
        return enc("International EBCDIC", "ebcdic", ubuf);
    }
    // O C deixa no ubuf o que a última tentativa (latin1 no EBCDIC) chegou a converter.
    Encoding {
        text: false,
        code: "unknown",
        code_mime: "binary",
        kind: "binary",
        ubuf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(file_encoding(b"hello\n", 65536).code, "ASCII");
        assert_eq!(
            file_encoding("olá\n".as_bytes(), 65536).code,
            "Unicode text, UTF-8"
        );
        assert_eq!(
            file_encoding(b"\xef\xbb\xbfhi\n", 65536).code,
            "Unicode text, UTF-8 (with BOM)"
        );
        assert_eq!(
            file_encoding(b"\xff\xfeh\x00i\x00", 65536).code_mime,
            "utf-16le"
        );
        assert_eq!(
            file_encoding(b"\xfe\xff\x00h\x00i", 65536).code_mime,
            "utf-16be"
        );
        assert_eq!(
            file_encoding(b"\xff\xfe\x00\x00h\x00\x00\x00", 65536).code_mime,
            "utf-32le"
        );
        assert_eq!(file_encoding(b"caf\xe9\n", 65536).code, "ISO-8859");
        assert_eq!(
            file_encoding(b"caf\x85\x90\n", 65536).code,
            "Non-ISO extended-ASCII"
        );
        assert_eq!(file_encoding(b"+/v8 abc", 65536).code_mime, "utf-7");
        // "hello" + NL em EBCDIC; o 0x15 não é texto em nenhuma tabela ASCII, então cai no EBCDIC.
        assert_eq!(
            file_encoding(b"\x88\x85\x93\x93\x96\x15", 65536).code,
            "EBCDIC"
        );
        // Sem byte de controle, o mesmo texto ainda passa por "estendido".
        assert_eq!(
            file_encoding(b"\x88\x85\x93\x93\x96\x25", 65536).code,
            "Non-ISO extended-ASCII"
        );
        let bin = file_encoding(b"\x00\x01\x02\xff", 65536);
        assert!(!bin.text);
        assert_eq!(bin.kind, "binary");
        assert_eq!(bin.code_mime, "binary");
    }

    #[test]
    fn utf8_validation_like_go_table() {
        assert_eq!(looks_utf8(b"abc", None), 1);
        assert_eq!(looks_utf8("ação".as_bytes(), None), 2);
        assert_eq!(looks_utf8(b"\xc0\x80", None), -1);
        assert_eq!(looks_utf8(b"\xed\xa0\x80", None), -1);
        assert_eq!(looks_utf8(b"a\x01", None), 0);
        // Sequência cortada no fim do buffer conta como válida.
        assert_eq!(looks_utf8(b"ab\xc3", None), 1);
        let mut u = Vec::new();
        assert_eq!(looks_utf8("é".as_bytes(), Some(&mut u)), 2);
        assert_eq!(u, vec![0xe9]);
    }
}
