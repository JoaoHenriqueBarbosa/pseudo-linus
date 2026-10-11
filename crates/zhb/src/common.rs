//! `hb-common.cc`: `hb_feature_from_string`, `hb_tag_from_string` e
//! `hb_language_from_string`, com o mesmo analisador permissivo do C.

use crate::shape::Feature;

/// `ISSPACE` do `hb-algs.hh`.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\x0c' | b'\n' | b'\r' | b'\t' | b'\x0b')
}

struct Parser<'s> {
    s: &'s [u8],
    p: usize,
}

impl Parser<'_> {
    fn at_end(&self) -> bool {
        self.p == self.s.len()
    }

    fn peek(&self) -> u8 {
        self.s[self.p]
    }

    fn space(&mut self) -> bool {
        while !self.at_end() && is_space(self.peek()) {
            self.p += 1;
        }
        true
    }

    fn char(&mut self, c: u8) -> bool {
        self.space();
        if self.at_end() || self.peek() != c {
            return false;
        }
        self.p += 1;
        true
    }

    /// `hb_parse_int`: `strtol` sobre no máximo 31 bytes copiados com `strncpy`, e o `long`
    /// truncado para `int`.
    fn int(&mut self) -> Option<i32> {
        let len = (self.s.len() - self.p).min(31);
        let buf = &self.s[self.p..self.p + len];
        let buf = &buf[..buf.iter().position(|&b| b == 0).unwrap_or(buf.len())];
        let mut i = 0;
        while i < buf.len() && is_space(buf[i]) {
            i += 1;
        }
        let mut neg = false;
        if i < buf.len() && (buf[i] == b'+' || buf[i] == b'-') {
            neg = buf[i] == b'-';
            i += 1;
        }
        let digits_start = i;
        // No máximo 31 dígitos: cabe folgado num i128.
        let mut v: i128 = 0;
        while i < buf.len() && buf[i].is_ascii_digit() {
            v = v * 10 + i128::from(buf[i] - b'0');
            i += 1;
        }
        if i == digits_start {
            return None;
        }
        let v = if neg { -v } else { v };
        // `ERANGE` quando o `long` estoura.
        if v > i128::from(i64::MAX) || v < i128::from(i64::MIN) {
            return None;
        }
        self.p += i;
        Some(v as i64 as i32)
    }

    fn uint(&mut self) -> Option<u32> {
        self.int().map(|v| v as u32)
    }

    fn bool(&mut self) -> Option<u32> {
        self.space();
        let start = self.p;
        while !self.at_end() && self.peek().is_ascii_alphabetic() {
            self.p += 1;
        }
        let w = self.s[start..self.p].to_ascii_lowercase();
        match w.as_slice() {
            b"on" => Some(1),
            b"off" => Some(0),
            _ => None,
        }
    }

    fn tag(&mut self) -> Option<u32> {
        self.space();
        let mut quote = 0u8;
        if !self.at_end() && (self.peek() == b'\'' || self.peek() == b'"') {
            quote = self.peek();
            self.p += 1;
        }
        let start = self.p;
        while !self.at_end() && !matches!(self.peek(), b' ' | b'=' | b'[') && self.peek() != quote {
            self.p += 1;
        }
        let len = self.p - start;
        if len == 0 || len > 4 {
            return None;
        }
        let t = tag_from_string(&self.s[start..self.p]);
        if quote != 0 {
            if len != 4 || self.at_end() || self.peek() != quote {
                return None;
            }
            self.p += 1;
        }
        Some(t)
    }
}

/// `hb_tag_from_string`.
pub fn tag_from_string(s: &[u8]) -> u32 {
    if s.is_empty() || s[0] == 0 {
        return 0;
    }
    let mut t = [b' '; 4];
    for (d, &b) in t.iter_mut().zip(s.iter().take_while(|&&b| b != 0)) {
        *d = b;
    }
    u32::from_be_bytes(t)
}

/// `hb_feature_from_string`.
pub fn feature_from_string(s: &[u8]) -> Option<Feature> {
    let mut p = Parser { s, p: 0 };
    let mut f = Feature { tag: 0, value: 1, start: Feature::GLOBAL_START, end: Feature::GLOBAL_END };
    // `parse_feature_value_prefix`.
    if p.char(b'-') {
        f.value = 0;
    } else {
        p.char(b'+');
        f.value = 1;
    }
    f.tag = p.tag()?;
    // `parse_feature_indices`.
    p.space();
    if p.char(b'[') {
        let start = p.uint();
        if let Some(s) = start {
            f.start = s;
        }
        if p.char(b':') || p.char(b';') {
            if let Some(e) = p.uint() {
                f.end = e;
            }
        } else if start.is_some() {
            f.end = f.start.wrapping_add(1);
        }
        if !p.char(b']') {
            return None;
        }
    }
    // `parse_feature_value_postfix`.
    let had_equal = p.char(b'=');
    let value = p.uint().or_else(|| p.bool());
    if let Some(v) = value {
        f.value = v;
    }
    if had_equal && value.is_none() {
        return None;
    }
    p.space();
    p.at_end().then_some(f)
}

/// `canon_map`.
fn canon(b: u8) -> u8 {
    match b {
        b'-' | b'_' => b'-',
        b'0'..=b'9' | b'a'..=b'z' => b,
        b'A'..=b'Z' => b.to_ascii_lowercase(),
        _ => 0,
    }
}

/// `hb_language_from_string`: a forma canônica, cortada no primeiro caractere que o
/// `canon_map` zera; `None` é `HB_LANGUAGE_INVALID`.
pub fn language_from_string(s: &[u8]) -> Option<String> {
    let s = &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())];
    if s.is_empty() {
        return None;
    }
    let s = &s[..s.len().min(63)];
    let canon: Vec<u8> = s.iter().map(|&b| canon(b)).take_while(|&b| b != 0).collect();
    Some(String::from_utf8(canon).unwrap_or_default())
}

/// `_raqm_u8_to_u32`: decodifica pelo byte inicial, sem validar, até `len` codepoints ou o
/// primeiro NUL; byte solto é lido como `char` com sinal.
pub fn raqm_utf8_to_u32(text: &[u8]) -> Vec<u32> {
    let at = |i: usize| -> u32 { u32::from(text.get(i).copied().unwrap_or(0)) };
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() && text[i] != 0 && out.len() < text.len() {
        let c = at(i);
        if c & 0xf8 == 0xf0 {
            out.push(((c & 0x07) << 18) | ((at(i + 1) & 0x3f) << 12) | ((at(i + 2) & 0x3f) << 6) | (at(i + 3) & 0x3f));
            i += 4;
        } else if c & 0xf0 == 0xe0 {
            out.push(((c & 0x0f) << 12) | ((at(i + 1) & 0x3f) << 6) | (at(i + 2) & 0x3f));
            i += 3;
        } else if c & 0xe0 == 0xc0 {
            out.push(((c & 0x1f) << 6) | (at(i + 1) & 0x3f));
            i += 2;
        } else {
            out.push(i32::from(text[i] as i8) as u32);
            i += 1;
        }
    }
    out
}
