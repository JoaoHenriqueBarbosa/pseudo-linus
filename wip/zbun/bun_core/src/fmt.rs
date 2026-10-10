//! O subconjunto de `bun_core::fmt` (`src/bun_core/fmt.rs` do upstream) que os nós da AST usam.

/// `bun.fmt.FormatDouble`: o número como o JavaScript o escreve (`Number.prototype.toString()`).
pub struct FormatDouble {
    pub(crate) number: f64,
}

impl FormatDouble {
    /// No upstream é o `WTF__dtoa` do WebKit, a conversão do ECMAScript (Number::toString, 6.1.6.1.20). Aqui os
    /// dígitos vêm do formatador do Rust, que dá a menor sequência que volta ao mesmo `f64` e, no empate, a mais
    /// próxima: a mesma sequência que a especificação pede. O resto é a montagem da especificação.
    pub fn dtoa(buf: &mut [u8; 124], number: f64) -> &[u8] {
        let len = js_number_to_string(buf, number);
        &buf[..len]
    }

    pub fn dtoa_with_negative_zero(buf: &mut [u8; 124], number: f64) -> &[u8] {
        if number == 0.0 && number.is_sign_negative() {
            return b"-0";
        }
        Self::dtoa(buf, number)
    }
}

impl core::fmt::Display for FormatDouble {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut buf = [0u8; 124];
        let slice = Self::dtoa(&mut buf, self.number);
        f.write_str(core::str::from_utf8(slice).map_err(|_| core::fmt::Error)?)
    }
}

fn js_number_to_string(buf: &mut [u8; 124], number: f64) -> usize {
    let mut out = Writer { buf, len: 0 };
    if number.is_nan() {
        out.push(b"NaN");
        return out.len;
    }
    if number == 0.0 {
        out.push(b"0");
        return out.len;
    }
    if number.is_infinite() {
        out.push(if number < 0.0 { &b"-Infinity"[..] } else { &b"Infinity"[..] });
        return out.len;
    }
    if number < 0.0 {
        out.push(b"-");
    }
    // `{:e}` dá "d.ddddde-7": os dígitos significativos (k deles) e o expoente; n = expoente + 1.
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("formato {:e}");
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let k = digits.len() as i32;
    let n: i32 = exponent.parse::<i32>().expect("expoente") + 1;

    if k <= n && n <= 21 {
        out.push(&digits);
        for _ in 0..(n - k) {
            out.push(b"0");
        }
    } else if 0 < n && n <= 21 {
        out.push(&digits[..n as usize]);
        out.push(b".");
        out.push(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push(b"0.");
        for _ in 0..(-n) {
            out.push(b"0");
        }
        out.push(&digits);
    } else {
        out.push(&digits[..1]);
        if k > 1 {
            out.push(b".");
            out.push(&digits[1..]);
        }
        out.push(b"e");
        out.push(if n - 1 < 0 { &b"-"[..] } else { &b"+"[..] });
        out.push((n - 1).unsigned_abs().to_string().as_bytes());
    }
    out.len
}

struct Writer<'a> {
    buf: &'a mut [u8; 124],
    len: usize,
}

impl Writer<'_> {
    fn push(&mut self, bytes: &[u8]) {
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }
}

pub const fn hex_digit_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// [`hex_digit_value`] para um code point decodificado: fora de `0..=0xFF` (inclusive o `-1i32 as u32` do EOF) dá
/// `None`.
#[inline]
pub const fn hex_digit_value_u32(c: u32) -> Option<u8> {
    if c <= 0xFF { hex_digit_value(c as u8) } else { None }
}

/// Error from [`parse_int`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseIntError {
    InvalidCharacter,
    Overflow,
}

impl ParseIntError {
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidCharacter => "InvalidCharacter",
            Self::Overflow => "Overflow",
        }
    }
}

/// O laço de dígitos de [`parse_int`]: `digits` já sem sinal, `radix` já resolvido (2..=36). Pula `_` no meio,
/// recusa `_` no começo e no fim, acumula em `u128` com estouro verificado.
#[inline]
fn parse_with_sign(digits: &[u8], radix: u8) -> Result<u128, ParseIntError> {
    debug_assert!((2..=36).contains(&radix));
    if digits.is_empty() || digits[0] == b'_' || digits[digits.len() - 1] == b'_' {
        return Err(ParseIntError::InvalidCharacter);
    }
    let radix_u = radix as u128;
    let mut acc: u128 = 0;
    for &c in digits {
        if c == b'_' {
            continue;
        }
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u128,
            b'a'..=b'z' => (c - b'a' + 10) as u128,
            b'A'..=b'Z' => (c - b'A' + 10) as u128,
            _ => return Err(ParseIntError::InvalidCharacter),
        };
        if d >= radix_u {
            return Err(ParseIntError::InvalidCharacter);
        }
        acc = acc
            .checked_mul(radix_u)
            .and_then(|v| v.checked_add(d))
            .ok_or(ParseIntError::Overflow)?;
    }
    Ok(acc)
}

/// Com `radix == 0`, tira o prefixo `0x`/`0o`/`0b` e escolhe a base (10 sem prefixo).
#[inline]
fn auto_radix(digits: &[u8], radix: u8) -> (&[u8], u8) {
    if radix != 0 {
        return (digits, radix);
    }
    if digits.len() >= 2 && digits[0] == b'0' {
        match digits[1] {
            b'x' | b'X' => return (&digits[2..], 16),
            b'o' | b'O' => return (&digits[2..], 8),
            b'b' | b'B' => return (&digits[2..], 2),
            _ => {}
        }
    }
    (digits, 10)
}

/// O `std.fmt.parseInt` do Zig: sinal opcional, `_` entre dígitos, `radix` de 2 a 36 ou 0 para detectar pelo
/// prefixo.
pub fn parse_int<T>(buf: &[u8], radix: u8) -> Result<T, ParseIntError>
where
    T: TryFrom<i128> + TryFrom<u128>,
{
    if buf.is_empty() {
        return Err(ParseIntError::InvalidCharacter);
    }
    let (neg, rest) = match buf[0] {
        b'+' => (false, &buf[1..]),
        b'-' => (true, &buf[1..]),
        _ => (false, buf),
    };
    let (digits, radix) = auto_radix(rest, radix);
    let acc = parse_with_sign(digits, radix)?;
    if neg {
        let signed: i128 = if acc == (i128::MAX as u128) + 1 {
            i128::MIN
        } else if acc > i128::MAX as u128 {
            return Err(ParseIntError::Overflow);
        } else {
            -(acc as i128)
        };
        T::try_from(signed).map_err(|_| ParseIntError::Overflow)
    } else {
        T::try_from(acc).map_err(|_| ParseIntError::Overflow)
    }
}

/// Error from [`parse_double`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct InvalidCharacter;

impl core::fmt::Display for InvalidCharacter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("InvalidCharacter")
    }
}
impl core::error::Error for InvalidCharacter {}

/// `WTF::parseDouble`: aceita o maior prefixo numérico (`b"1.5x"` dá 1.5) e recusa a entrada sem prefixo. No
/// upstream é o `fast_float::from_chars` do WebKit com `chars_format::general`: `-` opcional (o `+` não), dígitos
/// com ponto opcional (ao menos um dígito de um dos lados) e expoente só quando há dígito depois do `e`; sem
/// `inf` nem `nan`. O prefixo reconhecido aqui é o mesmo, e a conversão do Rust arredonda corretamente como o
/// fast_float, então o `f64` é o mesmo.
pub fn parse_double(buf: &[u8]) -> Result<f64, InvalidCharacter> {
    let digits_from = |mut i: usize| {
        while i < buf.len() && buf[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut i = usize::from(buf.first() == Some(&b'-'));
    let int_end = digits_from(i);
    let mut n_digits = int_end - i;
    i = int_end;
    if i < buf.len() && buf[i] == b'.' {
        let frac_end = digits_from(i + 1);
        n_digits += frac_end - (i + 1);
        i = frac_end;
    }
    if n_digits == 0 {
        return Err(InvalidCharacter);
    }
    if i < buf.len() && (buf[i] == b'e' || buf[i] == b'E') {
        let mut j = i + 1;
        if j < buf.len() && (buf[j] == b'+' || buf[j] == b'-') {
            j += 1;
        }
        let exp_end = digits_from(j);
        if exp_end > j {
            i = exp_end;
        }
    }
    // O prefixo é ASCII por construção.
    let text = core::str::from_utf8(&buf[..i]).map_err(|_| InvalidCharacter)?;
    text.parse::<f64>().map_err(|_| InvalidCharacter)
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parse_int_and_double() {
        assert_eq!(parse_int::<i32>(b"ff", 16), Ok(255));
        assert_eq!(parse_int::<i32>(b"0x1_0", 0), Ok(16));
        assert_eq!(parse_int::<u8>(b"256", 10), Err(ParseIntError::Overflow));
        assert_eq!(parse_double(b"1.5x"), Ok(1.5));
        assert_eq!(parse_double(b".5"), Ok(0.5));
        assert_eq!(parse_double(b"5."), Ok(5.0));
        assert_eq!(parse_double(b"1e"), Ok(1.0));
        assert_eq!(parse_double(b"1e-2"), Ok(0.01));
        assert_eq!(parse_double(b"-0"), Ok(-0.0));
        assert_eq!(parse_double(b"."), Err(InvalidCharacter));
        assert_eq!(parse_double(b"+1"), Err(InvalidCharacter));
        assert_eq!(parse_double(b"Infinity"), Err(InvalidCharacter));
    }
}

/// Format a string to an ECMAScript identifier. Always allocates/copies.
pub fn fmt_identifier(name: &[u8]) -> FormatValidIdentifier<'_> {
    FormatValidIdentifier { name }
}

/// esbuild has an 8 character truncation of a base32 encoded bytes. this
/// is not exactly that, but it will appear as such. the character list
/// chosen omits similar characters in the unlikely case someone is
/// trying to memorize a hash.
pub struct TruncatedHash32(pub u64);

pub fn truncated_hash32(int: u64) -> TruncatedHash32 {
    TruncatedHash32(int)
}

impl core::fmt::Display for TruncatedHash32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_bytes(f, &truncated_hash32_bytes(self.0))
    }
}

/// Const-fn core of [`truncated_hash32`]: the 8-byte base32-ish encoding (native-endian byte reinterpretation).
pub const fn truncated_hash32_bytes(int: u64) -> [u8; 8] {
    const CHARS: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let b = int.to_ne_bytes();
    let mut out = [0u8; 8];
    let mut i = 0;
    while i < 8 {
        out[i] = CHARS[(b[i] & 31) as usize];
        i += 1;
    }
    out
}

/// Format a string to an ECMAScript identifier.
pub struct FormatValidIdentifier<'a> {
    pub name: &'a [u8],
}

/// O `write_bytes` do upstream é um `from_utf8_unchecked`. Os trechos que chegam aqui são feitos só de
/// codepoints de identificador, que nunca são surrogates soltos, então a conversão checada não falha.
fn write_bytes(f: &mut core::fmt::Formatter<'_>, bytes: &[u8]) -> core::fmt::Result {
    f.write_str(core::str::from_utf8(bytes).map_err(|_| core::fmt::Error)?)
}

impl core::fmt::Display for FormatValidIdentifier<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::identifier::{is_identifier_part as is_identifier_continue, is_identifier_start};
        use crate::strings::{CodepointIterator, Cursor};

        let mut iterator = CodepointIterator::init(self.name);
        let mut cursor = Cursor::default();

        let mut needs_gap;
        let mut start_i: usize = 0;

        if !iterator.next(&mut cursor) {
            return f.write_str("_");
        }

        // Common case: no gap necessary. No allocation necessary.
        needs_gap = !is_identifier_start(cursor.c);
        if !needs_gap {
            // Are there any non-alphanumeric chars at all?
            while iterator.next(&mut cursor) {
                if !is_identifier_continue(cursor.c) {
                    needs_gap = true;
                    start_i = cursor.i as usize;
                    break;
                }
            }
        }

        if needs_gap {
            needs_gap = false;
            if start_i > 0 {
                write_bytes(f, &self.name[..start_i])?;
            } else {
                // the first letter can be a non-identifier start
                // https://github.com/oven-sh/bun/issues/2946
                f.write_str("_")?;
            }
            let slice = &self.name[start_i..];
            iterator = CodepointIterator::init(slice);
            cursor = Cursor::default();

            while iterator.next(&mut cursor) {
                if is_identifier_continue(cursor.c) {
                    if needs_gap {
                        f.write_str("_")?;
                        needs_gap = false;
                    }
                    let i = cursor.i as usize;
                    write_bytes(f, &slice[i..i + cursor.width as usize])?;
                } else if !needs_gap {
                    needs_gap = true;
                    // skip the code point, replace it with a single _
                }
            }

            // If it ends with an emoji
            if needs_gap {
                f.write_str("_")?;
            }
            return Ok(());
        }

        write_bytes(f, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::{FormatDouble, fmt_identifier};

    #[test]
    fn identifier_formatting() {
        assert_eq!(fmt_identifier(b"").to_string(), "_");
        assert_eq!(fmt_identifier(b"react").to_string(), "react");
        assert_eq!(fmt_identifier(b"react-dom").to_string(), "react_dom");
        assert_eq!(fmt_identifier(b"1abc").to_string(), "_1abc");
        assert_eq!(fmt_identifier(b"a--b!").to_string(), "a_b_");
    }

    fn js(n: f64) -> String {
        let mut buf = [0u8; 124];
        String::from_utf8(FormatDouble::dtoa(&mut buf, n).to_vec()).unwrap()
    }

    /// Os valores esperados são os de `String(n)` no JavaScript.
    #[test]
    fn matches_number_to_string() {
        assert_eq!(js(1.5), "1.5");
        assert_eq!(js(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(js(1e21), "1e+21");
        assert_eq!(js(1e20), "100000000000000000000");
        assert_eq!(js(123456789012345680000.0), "123456789012345680000");
        assert_eq!(js(0.000001), "0.000001");
        assert_eq!(js(1e-7), "1e-7");
        assert_eq!(js(-2.5e-10), "-2.5e-10");
        assert_eq!(js(5e-324), "5e-324");
        assert_eq!(js(1.7976931348623157e308), "1.7976931348623157e+308");
        assert_eq!(js(-0.0), "0");
        assert_eq!(js(f64::NEG_INFINITY), "-Infinity");
    }
}
