//! `str` (`Objects/unicodeobject.c`): texto UTF-8 com comprimento e índice por código-ponto, o
//! `repr` do `unicode_repr` e o hash SipHash-1-3 do `Python/pyhash.c`.

use std::cell::Cell;
use std::fmt;

use crate::modules::ucd::{CaseMap, Property};

/// Valor de um `str`. O texto fica em UTF-8; `len`, indexação e fatiamento contam código-pontos,
/// como no CPython. Texto só ASCII (o caso comum) indexa em O(1) direto nos bytes.
///
/// Surrogates solitários (`'\udc80'`, que o CPython aceita e o `surrogateescape` produz) não cabem
/// em UTF-8: ficam em U+10D800..U+10DFFF (uso privado do plano 16) e voltam a ser U+D800..U+DFFF
/// em `ord`, `repr`, `encode` e afins (`surrogate_to_char` e `char_surrogate`).
///
/// Para a codificação ser injetiva, U+10FFFF é prefixo de escape: os chars reais U+10D800..U+10DFFF
/// e o próprio U+10FFFF ficam guardados como o par U+10FFFF seguido do char (`cp_to_str`), e
/// `code_points` decodifica. O par conta como um código-ponto; `has_escape` evita o custo quando
/// não há par no texto.
pub struct PyStr {
    text: String,
    char_len: usize,
    has_escape: bool,
    hash: Cell<Option<i64>>,
}

/// Prefixo de escape da codificação de código-pontos em `String`.
pub const ESCAPE: char = '\u{10FFFF}';

/// Código-pontos de `s` (U+0000..U+10FFFF, inclusive surrogates): decodifica o surrogate guardado
/// em U+10D800..U+10DFFF e o par de escape.
pub fn code_points(s: &str) -> impl Iterator<Item = u32> + '_ {
    let mut chars = s.chars();
    std::iter::from_fn(move || {
        let c = chars.next()?;
        Some(match c {
            ESCAPE => chars.next().map_or(c as u32, |next| next as u32),
            c => char_surrogate(c).unwrap_or(c as u32),
        })
    })
}

/// Os código-pontos de `s` como trechos do próprio texto: cada trecho é um char, ou o par de escape
/// inteiro. Concatenar trechos reconstrói uma codificação válida, então fatias com passo, iteração
/// e busca por posição andam por aqui em vez de `chars()`.
pub fn units(s: &str) -> impl Iterator<Item = &str> + '_ {
    let mut rest = s;
    std::iter::from_fn(move || {
        let mut chars = rest.chars();
        let first = chars.next()?;
        let mut len = first.len_utf8();
        if first == ESCAPE {
            len += chars.next().map_or(0, char::len_utf8);
        }
        let (unit, tail) = rest.split_at(len);
        rest = tail;
        Some(unit)
    })
}

/// Posições (em bytes) onde `needle` casa em `hay`, sem sobreposição, da esquerda para a direita,
/// no máximo `limit`. Só conta casamento que começa na fronteira de um código-ponto: sem par de
/// escape a busca de bytes do `str` já garante isso; com par, `"\u{10FFFF}x"` casaria no meio de
/// `"\u{10FFFF}\u{10FFFF}x"`. O `needle` vazio casa em toda fronteira, inclusive no fim.
pub fn match_offsets(hay: &str, needle: &str, limit: usize) -> Vec<usize> {
    if !hay.contains(ESCAPE) {
        return hay.match_indices(needle).take(limit).map(|(i, _)| i).collect();
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while out.len() < limit && pos <= hay.len() {
        if hay[pos..].starts_with(needle) {
            out.push(pos);
            if !needle.is_empty() {
                pos += needle.len();
                continue;
            }
        }
        match units(&hay[pos..]).next() {
            Some(unit) => pos += unit.len(),
            None => break,
        }
    }
    out
}

/// Como `match_offsets`, mas da direita para a esquerda (`rfind`, `rsplit`, `rpartition`): os
/// casamentos não se sobrepõem contando do fim.
pub fn rmatch_offsets(hay: &str, needle: &str, limit: usize) -> Vec<usize> {
    if !hay.contains(ESCAPE) {
        return hay.rmatch_indices(needle).take(limit).map(|(i, _)| i).collect();
    }
    let mut starts: Vec<usize> = Vec::new();
    let mut pos = 0;
    for unit in units(hay) {
        starts.push(pos);
        pos += unit.len();
    }
    starts.push(pos);
    let mut out = Vec::new();
    let mut end = hay.len();
    for &start in starts.iter().rev() {
        if out.len() == limit {
            break;
        }
        if start + needle.len() <= end && hay[start..end].starts_with(needle) {
            out.push(start);
            end = start;
        }
    }
    out
}

/// `hay` termina em `suffix` com o casamento começando numa fronteira de código-ponto.
pub fn ends_with_units(hay: &str, suffix: &str) -> bool {
    hay.ends_with(suffix) && (!hay.contains(ESCAPE) || rmatch_offsets(hay, suffix, 1) == [hay.len() - suffix.len()])
}

/// Ordem de dois `str` do Python (`unicode_compare` do CPython): por código-ponto. Em UTF-8 válido
/// a ordem dos bytes coincide com a dos código-pontos, e vale o `cmp` rápido. Os surrogates
/// (U+10D800..) e o escape (U+10FFFF) vivem no plano 16, cujo UTF-8 começa sempre em 0xF4; só
/// quando algum lado tem esse byte a comparação decodifica com `code_points`.
pub fn str_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    if a.as_bytes().contains(&0xF4) || b.as_bytes().contains(&0xF4) {
        code_points(a).cmp(code_points(b))
    } else {
        a.cmp(b)
    }
}

/// Guarda o código-ponto `cp` num `String`: o char comum, o char do surrogate, ou o par de escape
/// para os chars reais que colidem com a faixa dos surrogates e para U+10FFFF.
pub fn cp_to_str(cp: u32) -> String {
    let mut out = String::new();
    match char::from_u32(cp) {
        Some(c) if (0x10_D800..=0x10_DFFF).contains(&cp) || c == ESCAPE => {
            out.push(ESCAPE);
            out.push(c);
        }
        Some(c) => out.push(c),
        None if (0xD800..=0xDFFF).contains(&cp) => out.push(surrogate_to_char(cp)),
        None => out.push('\u{fffd}'),
    }
    out
}

/// Acrescenta o código-ponto `cp` a `out` na codificação dos `str` da VM. O plano básico sem
/// surrogates (o caso comum) vai direto, sem alocar.
pub fn push_cp(out: &mut String, cp: u32) {
    match char::from_u32(cp) {
        Some(c) if cp < 0x1_0000 => out.push(c),
        _ => out.push_str(&cp_to_str(cp)),
    }
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
        let has_escape = text.contains(ESCAPE);
        let char_len = if has_escape { code_points(&text).count() } else { text.chars().count() };
        PyStr { text, char_len, has_escape, hash: Cell::new(None) }
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

    /// Trecho guardado para o código-ponto na posição `index` (já normalizada, sem índice negativo):
    /// um char, ou o par de escape inteiro.
    pub fn unit_at(&self, index: usize) -> Option<&str> {
        if self.is_ascii() {
            self.text.get(index..index + 1)
        } else {
            units(&self.text[self.byte_offset(index)..]).next()
        }
    }

    /// Código-ponto na posição `index`, decodificado (surrogates e pares de escape).
    pub fn cp_at(&self, index: usize) -> Option<u32> {
        if self.is_ascii() {
            self.text.as_bytes().get(index).map(|&b| u32::from(b))
        } else if self.has_escape {
            code_points(&self.text[self.byte_offset(index)..]).next()
        } else {
            self.text.chars().nth(index).map(|c| char_surrogate(c).unwrap_or(c as u32))
        }
    }

    /// Deslocamento em bytes do código-ponto `index`; além do fim, o tamanho do texto.
    fn byte_offset(&self, index: usize) -> usize {
        if self.is_ascii() {
            index.min(self.text.len())
        } else if self.has_escape {
            let mut chars = self.text.char_indices();
            let mut n = 0;
            while let Some((offset, c)) = chars.next() {
                if n == index {
                    return offset;
                }
                if c == ESCAPE {
                    chars.next();
                }
                n += 1;
            }
            self.text.len()
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
    for cp in code_points(s) {
        let Some(c) = char::from_u32(cp) else {
            push_escaped(&mut out, cp);
            continue;
        };
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
            c if is_printable(cp) => out.push(c),
            c => push_escaped(&mut out, c as u32),
        }
    }
    out.push(quote);
    out
}

/// Escape `\x`, `\u` ou `\U` do `repr` para um código-ponto não imprimível.
fn push_escaped(out: &mut String, v: u32) {
    if v <= 0xff {
        out.push_str(&format!("\\x{v:02x}"));
    } else if v <= 0xffff {
        out.push_str(&format!("\\u{v:04x}"));
    } else {
        out.push_str(&format!("\\U{v:08x}"));
    }
}

/// `Py_UNICODE_ISPRINTABLE`: falso nas categorias Cc, Cf, Cs, Co, Cn, Zl, Zp e Zs, exceto o espaço.
/// Opera por código-ponto, então os surrogates (categoria Cs) nunca são imprimíveis.
pub fn is_printable(cp: u32) -> bool {
    cp == 0x20
        || !matches!(
            crate::modules::ucd::current().category(cp),
            "Cc" | "Cf" | "Cs" | "Co" | "Cn" | "Zl" | "Zp" | "Zs"
        )
}

/// Categoria geral de `cp` no Unicode 15.1.0 do `unicodedata` (a base de todos os predicados abaixo).
fn category(cp: u32) -> &'static str {
    crate::modules::ucd::current().category(cp)
}

/// `Py_UNICODE_ISALPHA`: categorias Lm, Lt, Lu, Ll e Lo.
pub fn is_alpha(cp: u32) -> bool {
    matches!(category(cp), "Lm" | "Lt" | "Lu" | "Ll" | "Lo")
}

/// Gera os predicados sobre um código-ponto que consultam o banco Unicode: `$test` vê o banco em `$db` e o
/// código-ponto em `$cp`.
macro_rules! ucd_predicates {
    ($($(#[$meta:meta])* $name:ident => |$db:ident, $cp:ident| $test:expr;)*) => {
        $($(#[$meta])*
        pub fn $name($cp: u32) -> bool {
            let $db = crate::modules::ucd::current();
            $test
        })*
    };
}

ucd_predicates! {
    /// `Py_UNICODE_ISDECIMAL`: tem valor decimal no `UnicodeData.txt`.
    is_decimal => |db, cp| db.decimal(cp).is_some();
    /// `Py_UNICODE_ISDIGIT`: tem valor de dígito no `UnicodeData.txt`.
    is_digit => |db, cp| db.digit(cp).is_some();
    /// `Py_UNICODE_ISNUMERIC`: tem valor numérico (inclui os do Unihan).
    is_numeric => |db, cp| db.numeric(cp).is_some();
    /// `Py_UNICODE_ISLOWER`: propriedade `Lowercase` do `DerivedCoreProperties.txt` do 15.1.0.
    is_lower => |db, cp| db.has(Property::Lowercase, cp);
    /// `Py_UNICODE_ISUPPER`: propriedade `Uppercase` do 15.1.0.
    is_upper => |db, cp| db.has(Property::Uppercase, cp);
    /// `_PyUnicode_IsCased`: propriedade `Cased` do 15.1.0.
    is_cased => |db, cp| db.has(Property::Cased, cp);
    /// `_PyUnicode_IsCaseIgnorable`: propriedade `Case_Ignorable` do 15.1.0.
    is_case_ignorable => |db, cp| db.has(Property::CaseIgnorable, cp);
    /// `XID_Start` do 15.1.0, direto do `DerivedCoreProperties.txt`, como o `makeunicodedata.py`.
    is_xid_start => |db, cp| db.has(Property::XidStart, cp);
    /// `XID_Continue` do 15.1.0.
    is_xid_continue => |db, cp| db.has(Property::XidContinue, cp);
}

/// `str.isalnum`: letra, ou decimal, dígito ou numérico.
pub fn is_alnum(cp: u32) -> bool {
    is_alpha(cp) || is_numeric(cp)
}

/// `Py_UNICODE_ISSPACE`: categoria Zs ou bidirecional WS, B ou S.
pub fn is_space(cp: u32) -> bool {
    let db = crate::modules::ucd::current();
    db.category(cp) == "Zs" || matches!(db.bidirectional(cp), "WS" | "B" | "S")
}

/// `Lt`: letra de título.
pub fn is_title(cp: u32) -> bool {
    category(cp) == "Lt"
}

/// Aplica o mapeamento completo de caixa `which` a `cp`, acrescentando o resultado a `out`.
fn push_mapped(out: &mut String, which: CaseMap, cp: u32) {
    match crate::modules::ucd::current().case_map(which, cp) {
        Some(mapped) => mapped.iter().for_each(|&m| push_cp(out, m)),
        None => push_cp(out, cp),
    }
}

/// `lower_ucs4`: minúscula completa de `cps[i]`, com o sigma final (`handle_capital_sigma`):
/// `Σ` vira `ς` quando vem depois de caractere `Cased` (ignorando os `Case_Ignorable`) e não
/// antes de outro.
fn push_lower(out: &mut String, cps: &[u32], i: usize) {
    if cps[i] != 0x3A3 {
        return push_mapped(out, CaseMap::Lower, cps[i]);
    }
    let significant = |c: &&u32| !is_case_ignorable(**c);
    let before = cps[..i].iter().rev().find(significant).is_some_and(|&c| is_cased(c));
    let after = cps[i + 1..].iter().find(significant).is_some_and(|&c| is_cased(c));
    push_cp(out, if before && !after { 0x3C2 } else { 0x3C3 });
}

/// O texto de `s` convertido caractere a caractere: `convert` acrescenta a `out` o resultado de `cps[i]` e vê
/// o texto todo (o sigma final e o título dependem dos vizinhos).
fn convert_code_points(s: &str, mut convert: impl FnMut(&mut String, &[u32], usize)) -> String {
    let cps: Vec<u32> = code_points(s).collect();
    let mut out = String::with_capacity(s.len());
    (0..cps.len()).for_each(|i| convert(&mut out, &cps, i));
    out
}

/// Gera as conversões de caixa que só aplicam o mapeamento completo `$map` a cada código-ponto.
macro_rules! full_case_mappings {
    ($($(#[$meta:meta])* $name:ident => $map:ident;)*) => {
        $($(#[$meta])*
        pub fn $name(s: &str) -> String {
            convert_code_points(s, |out, cps, i| push_mapped(out, CaseMap::$map, cps[i]))
        })*
    };
}

full_case_mappings! {
    /// `str.upper`.
    upper_str => Upper;
    /// `str.casefold`: o dobramento completo do `CaseFolding.txt` (status C e F).
    casefold_str => Fold;
}

/// `str.lower`.
pub fn lower_str(s: &str) -> String {
    convert_code_points(s, push_lower)
}

/// `str.swapcase`.
pub fn swapcase_str(s: &str) -> String {
    convert_code_points(s, |out, cps, i| {
        let c = cps[i];
        if is_upper(c) {
            push_lower(out, cps, i);
        } else if is_lower(c) {
            push_mapped(out, CaseMap::Upper, c);
        } else {
            push_cp(out, c);
        }
    })
}

/// `str.title`.
pub fn title_str(s: &str) -> String {
    let mut previous_is_cased = false;
    convert_code_points(s, |out, cps, i| {
        if previous_is_cased {
            push_lower(out, cps, i);
        } else {
            push_mapped(out, CaseMap::Title, cps[i]);
        }
        previous_is_cased = is_cased(cps[i]);
    })
}

/// `str.capitalize`: título no primeiro caractere, minúscula nos demais.
pub fn capitalize_str(s: &str) -> String {
    convert_code_points(s, |out, cps, i| {
        if i == 0 {
            push_mapped(out, CaseMap::Title, cps[0]);
        } else {
            push_lower(out, cps, i);
        }
    })
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
    let max = code_points(s).max().unwrap_or(0);
    let width = if max <= 0xff {
        1
    } else if max <= 0xffff {
        2
    } else {
        4
    };
    let mut buf = Vec::with_capacity(s.len() * width);
    for v in code_points(s) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_point_round_trip_is_injective() {
        for cp in 0..=0x10_FFFFu32 {
            let stored = cp_to_str(cp);
            assert_eq!(code_points(&stored).collect::<Vec<_>>(), [cp], "cp {cp:#x}");
            let s = PyStr::new(format!("a{stored}b"));
            assert_eq!(s.len(), 3, "cp {cp:#x}");
            assert_eq!(s.cp_at(1), Some(cp), "cp {cp:#x}");
            assert_eq!(s.slice(1, 2), stored, "cp {cp:#x}");
        }
    }

    #[test]
    fn escape_pairs_count_as_one() {
        let text = format!("{}x{}{}", cp_to_str(0x10_D800), cp_to_str(0x10_FFFF), cp_to_str(0xD800));
        let s = PyStr::new(text);
        assert_eq!(s.len(), 4);
        assert_eq!(s.cp_at(3), Some(0xD800));
        assert_eq!(s.slice(2, 4), format!("{}{}", cp_to_str(0x10_FFFF), cp_to_str(0xD800)));
    }
}
