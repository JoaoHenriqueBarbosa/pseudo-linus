//! Tradução entre UTF-8, UTF-16, UTF-16BE e UTF-16LE (utf.c do SQLite 3.46.1).
//!
//! Só entram as funções que operam em bytes puros. As que dependem de `Mem`/`sqlite3`
//! (`sqlite3VdbeMemTranslate`, `sqlite3VdbeMemHandleBom`, `sqlite3Utf16to8`) ficam para o
//! módulo da Mem; o corpo de conversão de `sqlite3VdbeMemTranslate` está aqui em
//! [`translate_bytes`], para o chamador só cuidar de flags, terminadores e alocação.
//!
//! Convenção de leitura: o C assume que a cadeia UTF-8 é terminada em zero. Aqui, o byte
//! logo depois do fim da fatia lê como `0x00`.

use crate::consts::{SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF16NATIVE, SQLITE_UTF8};

/// Tabela que ajuda a decodificar o primeiro byte de um caractere UTF-8 multibyte.
const UTF8_TRANS1: [u8; 64] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x00, 0x01, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00,
];

/// Lê o byte `i` de `z`; depois do fim, devolve zero (terminador implícito do C).
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Macro `WRITE_UTF8`: acrescenta o caractere `c` codificado em UTF-8.
pub fn write_utf8(out: &mut Vec<u8>, c: u32) {
    if c < 0x00080 {
        out.push((c & 0xFF) as u8);
    } else if c < 0x00800 {
        out.push(0xC0u8.wrapping_add(((c >> 6) & 0x1F) as u8));
        out.push(0x80u8.wrapping_add((c & 0x3F) as u8));
    } else if c < 0x10000 {
        out.push(0xE0u8.wrapping_add(((c >> 12) & 0x0F) as u8));
        out.push(0x80u8.wrapping_add(((c >> 6) & 0x3F) as u8));
        out.push(0x80u8.wrapping_add((c & 0x3F) as u8));
    } else {
        out.push(0xF0u8.wrapping_add(((c >> 18) & 0x07) as u8));
        out.push(0x80u8.wrapping_add(((c >> 12) & 0x3F) as u8));
        out.push(0x80u8.wrapping_add(((c >> 6) & 0x3F) as u8));
        out.push(0x80u8.wrapping_add((c & 0x3F) as u8));
    }
}

/// Macro `WRITE_UTF16LE`: acrescenta o caractere `c` em UTF-16 little-endian.
pub fn write_utf16le(out: &mut Vec<u8>, c: u32) {
    if c <= 0xFFFF {
        out.push((c & 0x00FF) as u8);
        out.push(((c >> 8) & 0x00FF) as u8);
    } else {
        let d = c.wrapping_sub(0x10000);
        out.push((((c >> 10) & 0x003F).wrapping_add((d >> 10) & 0x00C0)) as u8);
        out.push((0x00D8u32.wrapping_add((d >> 18) & 0x03)) as u8);
        out.push((c & 0x00FF) as u8);
        out.push((0x00DCu32.wrapping_add((c >> 8) & 0x03)) as u8);
    }
}

/// Macro `WRITE_UTF16BE`: acrescenta o caractere `c` em UTF-16 big-endian.
pub fn write_utf16be(out: &mut Vec<u8>, c: u32) {
    if c <= 0xFFFF {
        out.push(((c >> 8) & 0x00FF) as u8);
        out.push((c & 0x00FF) as u8);
    } else {
        let d = c.wrapping_sub(0x10000);
        out.push((0x00D8u32.wrapping_add((d >> 18) & 0x03)) as u8);
        out.push((((c >> 10) & 0x003F).wrapping_add((d >> 10) & 0x00C0)) as u8);
        out.push((0x00DCu32.wrapping_add((c >> 8) & 0x03)) as u8);
        out.push((c & 0x00FF) as u8);
    }
}

/// `sqlite3Utf8Read` e macro `READ_UTF8`: decodifica um caractere UTF-8 a partir de `z[*pos]`
/// e avança `*pos` para o próximo byte não lido. O fim da fatia faz o papel de `zTerm`
/// (e do zero terminador).
///
/// Sobre UTF-8 inválido:
/// - um valor 0x00 a 0x7f codificado em multibyte vira 0xfffd;
/// - um substituto UTF-16 (0xd800 a 0xdfff) vira 0xfffd;
/// - bytes 0x80 a 0xbf como primeiro byte são lidos como eles mesmos;
/// - codificações supérfluas de valores 0x80 ou mais são aceitas.
pub fn utf8_read(z: &[u8], pos: &mut usize) -> u32 {
    let mut c = at(z, *pos) as u32;
    *pos += 1;
    if c >= 0xc0 {
        c = UTF8_TRANS1[(c - 0xc0) as usize] as u32;
        while (at(z, *pos) & 0xc0) == 0x80 {
            c = (c << 6).wrapping_add(0x3f & at(z, *pos) as u32);
            *pos += 1;
        }
        if c < 0x80 || (c & 0xFFFF_F800) == 0xD800 || (c & 0xFFFF_FFFE) == 0xFFFE {
            c = 0xFFFD;
        }
    }
    c
}

/// `sqlite3Utf8ReadLimited`: lê um caractere de `z` usando no máximo `n` bytes (e nunca mais
/// de 4). Devolve `(valor, bytes usados)`; os bytes usados ficam sempre entre 1 e 4. Não
/// detecta UTF-8 inválido.
pub fn utf8_read_limited(z: &[u8], n: i32) -> (u32, i32) {
    let mut i: usize = 1;
    debug_assert!(n > 0);
    let mut c = at(z, 0) as u32;
    if c >= 0xc0 {
        c = UTF8_TRANS1[(c - 0xc0) as usize] as u32;
        let n = n.min(4) as usize;
        // O C lê z[i] sem checar o fim do buffer; aqui o fim da fatia lê como zero.
        while i < n && (at(z, i) & 0xc0) == 0x80 {
            c = (c << 6).wrapping_add(0x3f & at(z, i) as u32);
            i += 1;
        }
    }
    (c, i as i32)
}

/// Corpo de `sqlite3VdbeMemTranslate`: converte `input` (texto em `enc`) para `desired_enc`
/// e devolve os bytes convertidos, SEM terminador (a Mem acrescenta os zeros e cuida de
/// flags, `enc` e alocação). `enc` e `desired_enc` valem `SQLITE_UTF8`, `SQLITE_UTF16LE` ou
/// `SQLITE_UTF16BE` e são diferentes entre si.
///
/// Entre as duas ordens de bytes do UTF-16 só se trocam os pares de bytes; um byte final
/// sobrando (comprimento ímpar) fica como está. Ao converter de UTF-16 para UTF-8 o
/// comprimento é arredondado para baixo para par (`pMem->n &= ~1`).
pub fn translate_bytes(input: &[u8], enc: u8, desired_enc: u8) -> Vec<u8> {
    let enc = enc as i32;
    let desired_enc = desired_enc as i32;
    debug_assert!(enc != desired_enc);
    debug_assert!(enc != 0);

    // Troca de ordem de bytes entre UTF-16LE e UTF-16BE.
    if enc != SQLITE_UTF8 && desired_enc != SQLITE_UTF8 {
        let mut out = input.to_vec();
        let n = out.len() & !1;
        let mut i = 0;
        while i < n {
            out.swap(i, i + 1);
            i += 2;
        }
        return out;
    }

    let mut z_in: usize = 0;
    let z_term: usize;
    let mut out: Vec<u8>;
    if desired_enc == SQLITE_UTF8 {
        // Crescimento máximo: caractere de 2 bytes virando 4 bytes de UTF-8.
        z_term = input.len() & !1;
        out = Vec::with_capacity(2 * z_term);
    } else {
        // Crescimento máximo: caractere de 1 byte virando 2 bytes de UTF-16.
        z_term = input.len();
        out = Vec::with_capacity(2 * z_term);
    }
    let input = &input[..z_term];

    if enc == SQLITE_UTF8 {
        if desired_enc == SQLITE_UTF16LE {
            // UTF-8 -> UTF-16 little-endian
            while z_in < z_term {
                let c = utf8_read(input, &mut z_in);
                write_utf16le(&mut out, c);
            }
        } else {
            debug_assert!(desired_enc == SQLITE_UTF16BE);
            // UTF-8 -> UTF-16 big-endian
            while z_in < z_term {
                let c = utf8_read(input, &mut z_in);
                write_utf16be(&mut out, c);
            }
        }
    } else {
        debug_assert!(desired_enc == SQLITE_UTF8);
        let little = enc == SQLITE_UTF16LE;
        // Lê um par de bytes na ordem de `enc`.
        let pair = |i: usize| -> u32 {
            if little {
                input[i] as u32 + ((input[i + 1] as u32) << 8)
            } else {
                ((input[i] as u32) << 8) + input[i + 1] as u32
            }
        };
        while z_in < z_term {
            let mut c = pair(z_in);
            z_in += 2;
            if (0xd800..0xe000).contains(&c) && z_in < z_term {
                let c2 = pair(z_in);
                z_in += 2;
                c = (c2 & 0x03FF)
                    .wrapping_add((c & 0x003F) << 10)
                    .wrapping_add(((c & 0x03C0).wrapping_add(0x0040)) << 10);
            }
            write_utf8(&mut out, c);
        }
    }
    out
}

/// `SQLITE_SKIP_UTF8`: avança `*pos` para o primeiro byte do próximo caractere UTF-8.
pub fn skip_utf8(z: &[u8], pos: &mut usize) {
    let b = at(z, *pos);
    *pos += 1;
    if b >= 0xc0 {
        while (at(z, *pos) & 0xc0) == 0x80 {
            *pos += 1;
        }
    }
}

/// `sqlite3Utf8CharLen`: número de caracteres UTF-8 em `z`. Se `n_byte` for negativo, conta
/// até o primeiro byte 0x00 (ou o fim da fatia); senão, conta nos primeiros `n_byte` bytes
/// (ou até o primeiro 0x00, o que vier antes).
pub fn utf8_char_len(z: &[u8], n_byte: i32) -> i32 {
    let mut r = 0;
    let mut i: usize = 0;
    let z_term = if n_byte >= 0 { n_byte as usize } else { usize::MAX };
    while at(z, i) != 0 && i < z_term {
        skip_utf8(z, &mut i);
        r += 1;
    }
    r
}

/// `sqlite3Utf16ByteLen`: número de bytes dos primeiros `n_char` caracteres da cadeia UTF-16
/// nativa `z` (que tem pelo menos `n_char` caracteres). `n_char` não é negativo.
pub fn utf16_byte_len(z: &[u8], n_char: i32) -> i32 {
    let native_le = SQLITE_UTF16NATIVE == SQLITE_UTF16LE;
    let mut i: usize = if native_le { 1 } else { 0 };
    let mut n = 0;
    while n < n_char {
        let c = at(z, i);
        i += 2;
        if c >= 0xd8 && c < 0xdc && at(z, i) >= 0xdc && at(z, i) < 0xe0 {
            i += 2;
        }
        n += 1;
    }
    i as i32 - if native_le { 1 } else { 0 }
}
