//! `str` (`Objects/unicodeobject.c`): texto UTF-8 com comprimento e índice por código-ponto, o
//! `repr` do `unicode_repr` e o hash SipHash-1-3 do `Python/pyhash.c`.

use std::cell::Cell;
use std::fmt;

/// Valor de um `str`. O texto fica em UTF-8; `len`, indexação e fatiamento contam código-pontos,
/// como no CPython. Texto só ASCII (o caso comum) indexa em O(1) direto nos bytes.
///
/// Surrogates solitários (`'\udc80'`, que o CPython aceita e o `surrogateescape` produz) não cabem
/// em UTF-8: ficam em U+10D800..U+10DFFF (uso privado do plano 16) e voltam a ser U+D800..U+DFFF
/// em `ord`, `repr`, `encode` e afins (`surrogate_to_char` e `char_surrogate`).
pub struct PyStr {
    text: String,
    char_len: usize,
    hash: Cell<Option<i64>>,
}

/// Deslocamento que leva um surrogate (U+D800..U+DFFF) ao uso privado do plano 16.
pub const SURROGATE_OFFSET: u32 = 0x10_0000;

/// O `char` que guarda o surrogate `cp` (U+D800..U+DFFF) dentro de um `str`.
pub fn surrogate_to_char(cp: u32) -> char {
    char::from_u32(SURROGATE_OFFSET + cp).unwrap_or('\u{fffd}')
}

/// O código-ponto do surrogate que `c` guarda, se for um.
pub fn char_surrogate(c: char) -> Option<u32> {
    let v = c as u32;
    (0x10_D800..=0x10_DFFF).contains(&v).then(|| v - SURROGATE_OFFSET)
}

impl PyStr {
    pub fn new(text: impl Into<String>) -> PyStr {
        let text = text.into();
        let char_len = text.chars().count();
        PyStr { text, char_len, hash: Cell::new(None) }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// `len()`, em código-pontos.
    pub fn len(&self) -> usize {
        self.char_len
    }

    pub fn is_empty(&self) -> bool {
        self.char_len == 0
    }

    pub fn is_ascii(&self) -> bool {
        self.text.len() == self.char_len
    }

    /// Código-ponto na posição `index` (já normalizada, sem índice negativo).
    pub fn char_at(&self, index: usize) -> Option<char> {
        if self.is_ascii() {
            self.text.as_bytes().get(index).map(|&b| char::from(b))
        } else {
            self.text.chars().nth(index)
        }
    }

    /// Deslocamento em bytes do código-ponto `index`; além do fim, o tamanho do texto.
    fn byte_offset(&self, index: usize) -> usize {
        if self.is_ascii() {
            index.min(self.text.len())
        } else {
            self.text.char_indices().nth(index).map_or(self.text.len(), |(offset, _)| offset)
        }
    }

    /// Trecho `[start:end]` em código-pontos, com os limites recortados ao tamanho como no CPython.
    pub fn slice(&self, start: usize, end: usize) -> &str {
        let end = end.min(self.char_len);
        let start = start.min(end);
        &self.text[self.byte_offset(start)..self.byte_offset(end)]
    }

    /// `hash()`, calculado uma vez e guardado como o `ob_hash` do CPython.
    pub fn hash(&self) -> i64 {
        if let Some(h) = self.hash.get() {
            return h;
        }
        let h = str_hash(&self.text);
        self.hash.set(Some(h));
        h
    }
}

impl PartialEq for PyStr {
    fn eq(&self, other: &PyStr) -> bool {
        self.text == other.text
    }
}

impl fmt::Debug for PyStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&str_repr(&self.text))
    }
}

/// `repr()` de `str` (`unicode_repr` do `Objects/unicodeobject.c`).
pub fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            c if is_printable(c) => out.push(c),
            c => {
                let v = char_surrogate(c).unwrap_or(c as u32);
                if v <= 0xff {
                    out.push_str(&format!("\\x{v:02x}"));
                } else if v <= 0xffff {
                    out.push_str(&format!("\\u{v:04x}"));
                } else {
                    out.push_str(&format!("\\U{v:08x}"));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// `str.isprintable` para código-pontos fora do ASCII imprimível: falso nas categorias Cc, Cf, Co,
/// Zl, Zp e Zs (exceto o espaço, já tratado), nos não-caracteres e nos planos sem atribuição.
/// Código-pontos Cn espalhados dentro de blocos atribuídos dependem da tabela do `unicodedata`.
pub fn is_printable(c: char) -> bool {
    let v = c as u32;
    const NOT_PRINTABLE: &[(u32, u32)] = &[
        // Cc
        (0x00, 0x1f),
        (0x7f, 0x9f),
        // Zs
        (0xa0, 0xa0),
        (0x1680, 0x1680),
        (0x2000, 0x200a),
        (0x202f, 0x202f),
        (0x205f, 0x205f),
        (0x3000, 0x3000),
        // Zl, Zp
        (0x2028, 0x2029),
        // Cf
        (0xad, 0xad),
        (0x600, 0x605),
        (0x61c, 0x61c),
        (0x6dd, 0x6dd),
        (0x70f, 0x70f),
        (0x890, 0x891),
        (0x8e2, 0x8e2),
        (0x180e, 0x180e),
        (0x200b, 0x200f),
        (0x202a, 0x202e),
        (0x2060, 0x2064),
        (0x2066, 0x206f),
        (0xfeff, 0xfeff),
        (0xfff9, 0xfffb),
        (0x110bd, 0x110bd),
        (0x110cd, 0x110cd),
        (0x13430, 0x1343f),
        (0x1bca0, 0x1bca3),
        (0x1d173, 0x1d17a),
        (0xe0001, 0xe0001),
        (0xe0020, 0xe007f),
        // Co
        (0xe000, 0xf8ff),
        (0xf0000, 0x10ffff),
        // Cn: não-caracteres do BMP e planos sem atribuição
        (0xfdd0, 0xfdef),
        (0x40000, 0xdffff),
        (0xe0080, 0xeffff),
    ];
    if v & 0xfffe == 0xfffe {
        return false;
    }
    !NOT_PRINTABLE.iter().any(|&(lo, hi)| v >= lo && v <= hi)
}

/// `repr()` de `bytes` (`bytes_repr` do `Objects/bytesobject.c`).
pub fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') { b'"' } else { b'\'' };
    let mut out = String::with_capacity(b.len() + 3);
    out.push('b');
    out.push(char::from(quote));
    for &c in b {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(char::from(c));
            }
            0x20..=0x7e => out.push(char::from(c)),
            c => out.push_str(&format!("\\x{c:02x}")),
        }
    }
    out.push(char::from(quote));
    out
}

/// Hash de `str`: o CPython aplica o SipHash sobre a representação interna compacta (1, 2 ou 4
/// bytes por código-ponto, conforme o maior deles), não sobre o UTF-8. Por isso `hash('a')` é igual
/// a `hash(b'a')`.
fn str_hash(s: &str) -> i64 {
    let max = s.chars().map(|c| c as u32).max().unwrap_or(0);
    let width = if max <= 0xff {
        1
    } else if max <= 0xffff {
        2
    } else {
        4
    };
    let mut buf = Vec::with_capacity(s.len() * width);
    for c in s.chars() {
        let v = c as u32;
        match width {
            1 => buf.push(v as u8),
            2 => buf.extend_from_slice(&(v as u16).to_le_bytes()),
            _ => buf.extend_from_slice(&v.to_le_bytes()),
        }
    }
    bytes_hash(&buf)
}

/// `_Py_HashBytes` com o algoritmo padrão de 64 bits (SipHash-1-3). A chave é a do
/// `PYTHONHASHSEED=0` (segredo zerado): sem semente, o CPython sorteia a chave a cada execução, e
/// nenhuma saída observável pode depender disso; com semente zero, os valores batem com o oráculo.
pub fn bytes_hash(data: &[u8]) -> i64 {
    if data.is_empty() {
        return 0;
    }
    let h = siphash13(0, 0, data) as i64;
    if h == -1 { -2 } else { h }
}

/// `siphash13` do `Python/pyhash.c`.
fn siphash13(k0: u64, k1: u64, data: &[u8]) -> u64 {
    let mut v0 = k0 ^ 0x736f_6d65_7073_6575;
    let mut v1 = k1 ^ 0x646f_7261_6e64_6f6d;
    let mut v2 = k0 ^ 0x6c79_6765_6e65_7261;
    let mut v3 = k1 ^ 0x7465_6462_7974_6573;

    fn half_round(a: &mut u64, b: &mut u64, c: &mut u64, d: &mut u64, s: u32, t: u32) {
        *a = a.wrapping_add(*b);
        *c = c.wrapping_add(*d);
        *b = b.rotate_left(s) ^ *a;
        *d = d.rotate_left(t) ^ *c;
        *a = a.rotate_left(32);
    }
    fn single_round(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
        half_round(v0, v1, v2, v3, 13, 16);
        half_round(v2, v1, v0, v3, 17, 21);
    }

    let mut b = (data.len() as u64) << 56;
    let (words, rest) = data.as_chunks::<8>();
    for word in words {
        let mi = u64::from_le_bytes(*word);
        v3 ^= mi;
        single_round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= mi;
    }
    let mut tail = [0u8; 8];
    tail[..rest.len()].copy_from_slice(rest);
    b |= u64::from_le_bytes(tail);

    v3 ^= b;
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= b;
    v2 ^= 0xff;
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    (v0 ^ v1) ^ (v2 ^ v3)
}
