//! O subconjunto de `bun_core::strings` (`src/bun_core/string/immutable.rs`, `string/immutable/unicode.rs` e o
//! `strings_impl` de `lib.rs` do upstream) que os nós da AST usam. Os nomes e o comportamento são os do upstream; o
//! que lá chama o simdutf aqui é o laço escalar que o próprio upstream usa como caminho lento, que dá o mesmo
//! resultado (ver o README de `wip/zbun`).

use core::cmp::Ordering;

/// `bun.strings.CodePoint`.
pub type CodePoint = i32;

/// Largura de uma sequência WTF-8, de 0 a 4.
pub type U3Fast = u8;

pub const UNICODE_REPLACEMENT: u32 = 0xFFFD;

// ─── ASCII ──────────────────────────────────────────────────────────────────

/// Posição do primeiro byte não ASCII, ou `None` se tudo for ASCII. No upstream é o `validate_ascii_with_errors`
/// do simdutf acima de 32 bytes; a posição devolvida é a mesma.
pub fn first_non_ascii(slice: &[u8]) -> Option<u32> {
    slice.iter().position(|&b| b >= 0x80).map(|i| i as u32)
}

pub fn is_all_ascii(slice: &[u8]) -> bool {
    slice.iter().all(|&b| b < 0x80)
}

// ─── Igualdade e ordem ──────────────────────────────────────────────────────

/// Com `check_len`, igualdade; sem, `b_str` comparado ao prefixo de `a_str` do mesmo tamanho. O upstream compara
/// palavra a palavra com ponteiros; o resultado é o da comparação de fatias.
pub fn eql_long(a_str: &[u8], b_str: &[u8], check_len: bool) -> bool {
    let len = b_str.len();
    if check_len {
        if len == 0 {
            return a_str.is_empty();
        }
        if a_str.len() != len {
            return false;
        }
    } else if cfg!(debug_assertions) {
        debug_assert!(b_str.len() <= a_str.len());
    }
    a_str.get(..len) == Some(b_str)
}

pub fn eql_comptime_utf16(self_: &[u16], alt: &[u8]) -> bool {
    // Compare bytewise, widening each ASCII byte of `alt` on the fly. All call sites pass ASCII literals.
    debug_assert!(alt.iter().all(|&b| b < 0x80));
    self_.len() == alt.len() && self_.iter().zip(alt.iter()).all(|(&u, &b)| u == u16::from(b))
}

#[inline(always)]
pub fn order(a: &[u8], b: &[u8]) -> Ordering {
    a.cmp(b)
}

/// Generic lexicographic slice ordering.
#[inline]
pub fn order_t<T: Ord>(a: &[T], b: &[T]) -> Ordering {
    a.cmp(b)
}

pub fn utf16_eql_string(text: &[u16], str: &[u8]) -> bool {
    if text.len() > str.len() {
        // UTF-16 encoding can never be longer than the UTF-8 encoding.
        return false;
    }
    let mut temp = [0u8; 4];
    let n = text.len();
    let mut j: usize = 0;
    let mut i: usize = 0;
    while i < n {
        let (cp, adv) = decode_wtf16_raw(&text[i..]);
        i += adv as usize;
        let width = encode_wtf8_rune(&mut temp, cp);
        if j + width > str.len() {
            return false;
        }
        if temp[..width] != str[j..j + width] {
            return false;
        }
        j += width;
    }
    j == str.len()
}

// ─── Surrogates (macros do ICU `utf16.h`) ───────────────────────────────────

/// ICU `U16_SURROGATE_OFFSET` = `(0xD800 << 10) + 0xDC00 - 0x10000`.
pub const U16_SURROGATE_OFFSET: u32 = (0xD800u32 << 10) + 0xDC00 - 0x10000;

/// ICU `U16_IS_LEAD`: high (lead) surrogate `0xD800..=0xDBFF`.
#[inline]
pub const fn u16_is_lead(c: u16) -> bool {
    (c & 0xFC00) == 0xD800
}

/// ICU `U16_IS_TRAIL`: low (trail) surrogate `0xDC00..=0xDFFF`.
#[inline]
pub const fn u16_is_trail(c: u16) -> bool {
    (c & 0xFC00) == 0xDC00
}

/// ICU `U16_GET_SUPPLEMENTARY`.
#[inline]
pub const fn u16_get_supplementary(lead: u16, trail: u16) -> u32 {
    ((lead as u32) << 10) + trail as u32 - U16_SURROGATE_OFFSET
}

/// ICU `U16_LEAD`: high surrogate for a supplementary code point.
#[inline]
pub const fn u16_lead(supplementary: u32) -> u16 {
    debug_assert!(supplementary >= 0x10000 && supplementary <= 0x10FFFF);
    ((supplementary >> 10) + 0xD7C0) as u16
}

/// ICU `U16_TRAIL`: low surrogate for a supplementary code point.
#[inline]
pub const fn u16_trail(supplementary: u32) -> u16 {
    debug_assert!(supplementary >= 0x10000 && supplementary <= 0x10FFFF);
    ((supplementary & 0x3FF) | 0xDC00) as u16
}

#[inline]
pub const fn encode_surrogate_pair(supplementary: u32) -> [u16; 2] {
    [u16_lead(supplementary), u16_trail(supplementary)]
}

/// Append `cp` to `buf` as 1 or 2 UTF-16 code units. Lone-surrogate code points pass through unchanged (WTF-16).
#[inline]
pub fn push_codepoint_utf16(buf: &mut Vec<u16>, cp: u32) {
    if cp <= 0xFFFF {
        buf.push(cp as u16);
    } else {
        buf.extend_from_slice(&encode_surrogate_pair(cp));
    }
}

/// Decode one code point, replacing any unpaired surrogate with U+FFFD. Returns `(code_point, units ∈ {1,2})`.
#[inline]
pub fn decode_utf16_with_fffd(input: &[u16]) -> (u32, u8) {
    let c0 = input[0];
    if u16_is_lead(c0) {
        match input.get(1) {
            Some(&c1) if u16_is_trail(c1) => (u16_get_supplementary(c0, c1), 2),
            _ => (0xFFFD, 1),
        }
    } else if u16_is_trail(c0) {
        (0xFFFD, 1)
    } else {
        (c0 as u32, 1)
    }
}

/// Decode one code point with WTF-16 pass-through: unpaired surrogates are returned verbatim.
#[inline]
pub fn decode_wtf16_raw(input: &[u16]) -> (u32, u8) {
    let c0 = input[0];
    if u16_is_lead(c0) {
        if let Some(&c1) = input.get(1) {
            if u16_is_trail(c1) {
                return (u16_get_supplementary(c0, c1), 2);
            }
        }
    }
    (c0 as u32, 1)
}

// ─── WTF-8 ──────────────────────────────────────────────────────────────────

/// Encode a code point as WTF-8 (UTF-8 that permits unpaired surrogates). Returns bytes written (1..=4).
#[inline]
pub fn encode_wtf8_rune(out: &mut [u8; 4], cp: u32) -> usize {
    if cp < 0x80 {
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        out[0] = 0xC0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else if cp < 0x10000 {
        out[0] = 0xE0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[2] = 0x80 | (cp & 0x3F) as u8;
        3
    } else {
        out[0] = 0xF0 | (cp >> 18) as u8;
        out[1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
        out[2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[3] = 0x80 | (cp & 0x3F) as u8;
        4
    }
}

/// Returns **1** for an invalid lead byte, so callers can always advance ≥1 (replacement-char semantics).
#[inline]
pub const fn wtf8_byte_sequence_length(first_byte: u8) -> u8 {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[inline]
pub const fn wtf8_byte_sequence_length_with_invalid(first_byte: u8) -> u8 {
    wtf8_byte_sequence_length(first_byte)
}

/// Integer types usable as the codepoint result of [`decode_wtf8_rune_t`] (`i32` aka `CodePoint`, and `u32`).
pub trait CodePointZero:
    Copy
    + Eq
    + PartialOrd
    + From<u8>
    + core::ops::Shl<u32, Output = Self>
    + core::ops::BitOr<Output = Self>
    + core::ops::BitAnd<Output = Self>
{
    fn from_u32(v: u32) -> Self;
    #[inline]
    fn from_u32_const(v: u32) -> Self {
        Self::from_u32(v)
    }
}

impl CodePointZero for CodePoint {
    #[inline]
    fn from_u32(v: u32) -> Self {
        v as i32
    }
}

impl CodePointZero for u32 {
    #[inline]
    fn from_u32(v: u32) -> Self {
        v
    }
}

/// Convert potentially ill-formed UTF-8 or UTF-16 bytes to a Unicode Codepoint.
/// - Invalid codepoints are replaced with `zero` parameter
/// - Null bytes return 0
pub fn decode_wtf8_rune_t<T: CodePointZero>(p: [u8; 4], len: U3Fast, zero: T) -> T {
    if len == 0 {
        return zero;
    }
    if len == 1 {
        return T::from(p[0]);
    }
    decode_wtf8_rune_t_multibyte::<T>(p, len, zero)
}

/// Clone of esbuild's decodeWTF8Rune (golang's "utf8.DecodeRune" modified to decode WTF-8). Asserts a multi-byte
/// codepoint.
#[inline]
pub fn decode_wtf8_rune_t_multibyte<T: CodePointZero>(p: [u8; 4], len: U3Fast, zero: T) -> T {
    debug_assert!(len > 1);

    let s1 = p[1];
    if (s1 & 0xC0) != 0x80 {
        return zero;
    }

    if len == 2 {
        let cp = (T::from(p[0] & 0x1F) << 6) | T::from(s1 & 0x3F);
        if cp < T::from(0x80u8) {
            return zero;
        }
        return cp;
    }

    let s2 = p[2];

    if (s2 & 0xC0) != 0x80 {
        return zero;
    }

    if len == 3 {
        let cp = (T::from(p[0] & 0x0F) << 12) | (T::from(s1 & 0x3F) << 6) | T::from(s2 & 0x3F);
        if cp < T::from_u32_const(0x800) {
            return zero;
        }
        return cp;
    }

    let s3 = p[3];

    if (s3 & 0xC0) != 0x80 {
        return zero;
    }

    let cp = (T::from(p[0] & 0x07) << 18) | (T::from(s1 & 0x3F) << 12) | (T::from(s2 & 0x3F) << 6) | T::from(s3 & 0x3F);
    if cp < T::from_u32_const(0x10000) || cp > T::from_u32_const(0x10FFFF) {
        return zero;
    }
    cp
}

// ─── CodepointIterator ──────────────────────────────────────────────────────

/// `CodepointIterator`: yields WTF-8 codepoints with byte-width.
pub struct NewCodePointIterator<'a> {
    pub bytes: &'a [u8],
}
pub type CodepointIterator<'a> = NewCodePointIterator<'a>;
pub type UnsignedCodepointIterator<'a> = NewCodePointIterator<'a>;

#[derive(Default, Clone, Copy)]
pub struct Cursor {
    pub i: u32,
    pub width: u8,
    pub c: CodePoint,
}

impl<'a> NewCodePointIterator<'a> {
    pub const ZERO_VALUE: CodePoint = -1;

    pub fn init(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// True iff any byte in `slice` begins a multi-byte WTF-8 sequence.
    pub fn needs_utf8_decoding(slice: &[u8]) -> bool {
        let mut i = 0usize;
        while i < slice.len() {
            let cp_len = wtf8_byte_sequence_length(slice[i]);
            match cp_len {
                0 => return false,
                1 => i += 1,
                _ => return true,
            }
        }
        false
    }

    /// Cursor advance. Returns `false` at end.
    #[inline(always)]
    pub fn next(&self, cursor: &mut Cursor) -> bool {
        let bytes = self.bytes;
        let pos = cursor.i as usize + cursor.width as usize;
        if pos >= bytes.len() {
            return false;
        }
        let tail = &bytes[pos..];
        let first = tail[0];
        cursor.i = pos as u32;
        // ASCII fast path: the overwhelmingly common case for JS source.
        if first < 0x80 {
            cursor.c = first as CodePoint;
            cursor.width = 1;
            return true;
        }
        let len = wtf8_byte_sequence_length(first);
        // `take ∈ 1..=4` clamped to the remaining length.
        let take = (len as usize).min(tail.len());
        let mut buf = [0u8; 4];
        buf[..take].copy_from_slice(&tail[..take]);
        let cp = decode_wtf8_rune_t::<CodePoint>(buf, len, -1);
        if cp == -1 {
            cursor.c = UNICODE_REPLACEMENT as CodePoint;
            cursor.width = 1;
        } else {
            cursor.c = cp;
            cursor.width = len;
        }
        true
    }
}

// ─── UTF-8 → UTF-16 ─────────────────────────────────────────────────────────

/// `strings.UTF16Replacement`: decoded codepoint with surrogate metadata.
#[derive(Clone, Copy)]
pub struct UTF16Replacement {
    pub code_point: u32,
    pub len: U3Fast,

    /// Explicit fail boolean to distinguish between a Unicode Replacement Codepoint that was already in there and
    /// a genuine error.
    pub fail: bool,

    pub can_buffer: bool,
    pub is_lead: bool,
}

impl Default for UTF16Replacement {
    fn default() -> Self {
        Self {
            code_point: UNICODE_REPLACEMENT,
            len: 0,
            fail: false,
            can_buffer: true,
            is_lead: false,
        }
    }
}

impl UTF16Replacement {
    #[inline]
    pub fn utf8_width(self) -> U3Fast {
        match self.code_point {
            0..=0x7F => 1,
            0x80..=0x7FF => 2,
            0x800..=0xFFFF => 3,
            _ => 4,
        }
    }
}

// https://github.com/WebKit/WebKit/blob/443e796d1538654c34f2690e39600c70c8052b63/Source/WebCore/PAL/pal/text/TextCodecUTF8.cpp#L69
fn non_ascii_sequence_length(first_byte: u8) -> U3Fast {
    match first_byte {
        0..=193 => 0,
        194..=223 => 2,
        224..=239 => 3,
        240..=244 => 4,
        245..=255 => 0,
    }
}

/// The early return for a continuation byte out of range: `len` bytes consumed, `need` bytes to buffer.
fn invalid_continuation(len: U3Fast, need: usize, remaining_len: usize) -> UTF16Replacement {
    UTF16Replacement {
        len,
        fail: true,
        can_buffer: remaining_len < need,
        ..Default::default()
    }
}

fn convert_utf8_bytes_into_utf16_with_length(sequence: [u8; 4], len: U3Fast, remaining_len: usize) -> UTF16Replacement {
    debug_assert!(sequence[0] > 127);
    let in_range = |b: u8, lo: u8, hi: u8| b >= lo && b <= hi;
    match len {
        2 => {
            debug_assert!(sequence[0] >= 0xC0);
            debug_assert!(sequence[0] <= 0xDF);
            if !in_range(sequence[1], 0x80, 0xBF) {
                return invalid_continuation(1, 2, remaining_len);
            }
            UTF16Replacement {
                len,
                code_point: (u32::from(sequence[0]) << 6) + u32::from(sequence[1]) - 0x00003080,
                ..Default::default()
            }
        }
        3 => {
            debug_assert!(sequence[0] >= 0xE0);
            debug_assert!(sequence[0] <= 0xEF);
            let (lo, hi) = match sequence[0] {
                0xE0 => (0xA0, 0xBF),
                0xED => (0x80, 0x9F),
                _ => (0x80, 0xBF),
            };
            if !in_range(sequence[1], lo, hi) {
                return invalid_continuation(1, 2, remaining_len);
            }
            if !in_range(sequence[2], 0x80, 0xBF) {
                return invalid_continuation(2, 3, remaining_len);
            }
            UTF16Replacement {
                len,
                code_point: ((u32::from(sequence[0]) << 12) + (u32::from(sequence[1]) << 6) + u32::from(sequence[2]))
                    - 0x000E2080,
                ..Default::default()
            }
        }
        4 => {
            let (lo, hi) = match sequence[0] {
                0xF0 => (0x90, 0xBF),
                0xF4 => (0x80, 0x8F),
                // invalid code point; this used to be an assertion
                0..=0xEF | 0xF5..=u8::MAX => {
                    return UTF16Replacement {
                        len: 1,
                        fail: true,
                        can_buffer: false,
                        ..Default::default()
                    };
                }
                _ => (0x80, 0xBF),
            };
            if !in_range(sequence[1], lo, hi) {
                return invalid_continuation(1, 2, remaining_len);
            }
            if !in_range(sequence[2], 0x80, 0xBF) {
                return invalid_continuation(2, 3, remaining_len);
            }
            if !in_range(sequence[3], 0x80, 0xBF) {
                return invalid_continuation(3, 4, remaining_len);
            }
            UTF16Replacement {
                len,
                code_point: ((u32::from(sequence[0]) << 18)
                    + (u32::from(sequence[1]) << 12)
                    + (u32::from(sequence[2]) << 6)
                    + u32::from(sequence[3]))
                    - 0x03C82080,
                ..Default::default()
            }
        }
        // invalid unicode sequence; 1 or 0 are both invalid here
        _ => UTF16Replacement {
            len: 1,
            fail: true,
            ..Default::default()
        },
    }
}

/// This variation matches WebKit behavior.
pub fn convert_utf8_bytes_into_utf16(bytes: &[u8]) -> UTF16Replacement {
    let sequence: [u8; 4] = match bytes.len() {
        0 => unreachable!(),
        1 => [bytes[0], 0, 0, 0],
        2 => [bytes[0], bytes[1], 0, 0],
        3 => [bytes[0], bytes[1], bytes[2], 0],
        _ => [bytes[0], bytes[1], bytes[2], bytes[3]],
    };
    debug_assert!(sequence[0] > 127);
    let sequence_length = non_ascii_sequence_length(sequence[0]);
    convert_utf8_bytes_into_utf16_with_length(sequence, sequence_length, bytes.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
pub enum ToUTF16Error {
    OutOfMemory,
    InvalidByteSequence,
}

/// UTF-8 → UTF-16 iff `bytes` has a non-ASCII byte; pure ASCII returns `None`. The upstream tries the simdutf fast
/// path first and falls back to this loop on any invalid byte; for valid UTF-8 both give the same units, so the
/// port always runs the loop.
pub fn to_utf16_alloc(bytes: &[u8], fail_if_invalid: bool, sentinel: bool) -> Result<Option<Vec<u16>>, ToUTF16Error> {
    if first_non_ascii(bytes).is_none() {
        return Ok(None);
    }
    let mut out: Vec<u16> = Vec::with_capacity(bytes.len() + usize::from(sentinel));
    let mut remaining = bytes;
    while let Some(i) = first_non_ascii(remaining) {
        let i = i as usize;
        // Copy ASCII prefix as-is (one u16 per byte).
        out.extend(remaining[..i].iter().map(|&b| u16::from(b)));
        remaining = &remaining[i..];
        let replacement = convert_utf8_bytes_into_utf16(remaining);
        if replacement.fail && fail_if_invalid {
            return Err(ToUTF16Error::InvalidByteSequence);
        }
        remaining = &remaining[(replacement.len as usize).max(1)..];
        push_codepoint_utf16(&mut out, replacement.code_point);
    }
    out.extend(remaining.iter().map(|&b| u16::from(b)));
    if sentinel {
        out.push(0);
    }
    Ok(Some(out))
}

/// Like [`to_utf16_alloc`] but **always** returns a `Vec<u16>` (pure-ASCII inputs are widened 1:1).
pub fn to_utf16_alloc_for_real(bytes: &[u8], fail_if_invalid: bool, sentinel: bool) -> Result<Vec<u16>, ToUTF16Error> {
    if let Some(v) = to_utf16_alloc(bytes, fail_if_invalid, sentinel)? {
        return Ok(v);
    }
    let mut out: Vec<u16> = Vec::with_capacity(bytes.len() + usize::from(sentinel));
    out.extend(bytes.iter().map(|&b| u16::from(b)));
    if sentinel {
        out.push(0);
    }
    Ok(out)
}

// ─── UTF-16 → UTF-8 ─────────────────────────────────────────────────────────

/// `convertUTF16ToUTF8Append`: unpaired surrogates become U+FFFD (the simdutf fast path refuses them and the
/// upstream falls back to this same replacement loop).
pub fn convert_utf16_to_utf8_append(list: &mut Vec<u8>, utf16: &[u16]) {
    let mut i = 0usize;
    let mut buf = [0u8; 4];
    while i < utf16.len() {
        let (cp, adv) = decode_utf16_with_fffd(&utf16[i..]);
        i += adv as usize;
        let n = encode_wtf8_rune(&mut buf, cp);
        list.extend_from_slice(&buf[..n]);
    }
}

#[inline]
pub fn to_utf8_alloc(utf16: &[u16]) -> Vec<u8> {
    let mut list = Vec::with_capacity(utf16.len());
    convert_utf16_to_utf8_append(&mut list, utf16);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trip_and_replacement() {
        let s = "aé€😀".as_bytes();
        let u = to_utf16_alloc_for_real(s, false, false).unwrap();
        assert_eq!(u, "aé€😀".encode_utf16().collect::<Vec<_>>());
        assert_eq!(to_utf8_alloc(&u), s);
        // byte inválido vira U+FFFD; surrogate solto também, na volta
        assert_eq!(to_utf16_alloc_for_real(b"a\xffb", false, false).unwrap(), [0x61, 0xFFFD, 0x62]);
        assert_eq!(to_utf16_alloc(b"\xff", true, false), Err(ToUTF16Error::InvalidByteSequence));
        assert_eq!(to_utf8_alloc(&[0xD800, 0x61]), "\u{FFFD}a".as_bytes());
        assert!(utf16_eql_string(&[0xD83D, 0xDE00], "😀".as_bytes()));
    }

    #[test]
    fn codepoint_iterator_widths() {
        let it = CodepointIterator::init("aé\u{FFFF}\x00".as_bytes());
        let mut c = Cursor::default();
        let mut seen = Vec::new();
        while it.next(&mut c) {
            seen.push((c.c, c.width));
        }
        assert_eq!(seen, [(0x61, 1), (0xE9, 2), (0xFFFF, 3), (0, 1)]);
        let bad = CodepointIterator::init(b"\xC3");
        let mut c = Cursor::default();
        assert!(bad.next(&mut c));
        assert_eq!((c.c, c.width), (0xFFFD, 1));
    }

    #[test]
    fn eql_long_prefix() {
        assert!(eql_long(b"abc", b"abc", true));
        assert!(!eql_long(b"abcd", b"abc", true));
        assert!(eql_long(b"abcd", b"abc", false));
    }
}
