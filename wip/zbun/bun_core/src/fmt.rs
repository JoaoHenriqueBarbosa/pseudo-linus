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

#[cfg(test)]
mod tests {
    use super::FormatDouble;

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
