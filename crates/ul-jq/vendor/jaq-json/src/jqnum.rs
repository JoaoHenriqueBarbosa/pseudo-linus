//! Porte pseudo-linus: números com a semântica do jq 1.7.1 (substitui o `num.rs` do jaq-json).
//!
//! O jq tem um tipo só de número, o double. Literais (da entrada JSON e do programa) guardam também o
//! valor decimal (decNumber) com o texto na forma canônica, que é o que sai na impressão enquanto o
//! número não passa por conta nenhuma; comparação entre dois literais é decimal (`decNumberCompare`).
//! Toda conta é em double (`jv_number_value`).
//!
//! - [`Num::Int`]: inteiro exato com `|i| < 2^53`, calculado ou literal curto. Imprime igual ao double
//!   (até 15 dígitos o `jvp_dtoa_fmt` e o decNumber escrevem os mesmos algarismos).
//! - [`Num::Float`]: double calculado.
//! - [`Num::Dec`]: literal, com o double já convertido do jeito do jq (arredonda para 17 dígitos,
//!   meio para o par, e faz `strtod`).

use super::Rc;
use alloc::string::{String, ToString};
use core::cmp::Ordering;
use core::fmt::{self, Write as _};
use core::hash::{Hash, Hasher};

/// Maior inteiro com todos os vizinhos representáveis num double.
const EXACT: f64 = 9_007_199_254_740_992.0;

/// Número do jq.
#[derive(Clone, Debug)]
pub enum Num {
    /// Inteiro exato (`|i| < 2^53`).
    Int(isize),
    /// Double calculado.
    Float(f64),
    /// Literal com valor decimal preservado.
    Dec(Rc<Literal>),
}

/// Valor decimal de um literal, como o `decNumberFromString` o entende.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decimal {
    /// `(-1)^neg * coeff * 10^exp`, `coeff` sem zeros à esquerda ("0" para zero).
    #[allow(missing_docs)]
    Finite { neg: bool, coeff: String, exp: i64 },
    /// Infinito (o decNumber aceita "Infinity"/"Inf").
    #[allow(missing_docs)]
    Inf { neg: bool },
    /// NaN (o jq imprime `null`).
    NaN,
}

/// Literal numérico: decimal exato, texto canônico e double.
#[derive(Clone, Debug)]
pub struct Literal {
    /// Valor decimal exato.
    pub dec: Decimal,
    /// `decNumberToString` do literal (o que o jq imprime).
    pub text: String,
    /// `jv_number_value` do literal.
    pub value: f64,
}

impl Decimal {
    /// `decNumberFromString`: sinal opcional, dígitos com ponto opcional, expoente opcional; ou
    /// Inf/Infinity/NaN/sNaN sem diferenciar maiúsculas.
    pub fn parse(s: &str) -> Option<Decimal> {
        let b = s.as_bytes();
        let mut i = 0;
        let mut neg = false;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            neg = b[i] == b'-';
            i += 1;
        }
        let rest = &s[i..];
        if rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity") {
            return Some(Decimal::Inf { neg });
        }
        let lower = rest.to_ascii_lowercase();
        let nan_body = lower.strip_prefix("snan").or_else(|| lower.strip_prefix("nan"));
        if let Some(payload) = nan_body {
            return payload.bytes().all(|c| c.is_ascii_digit()).then_some(Decimal::NaN);
        }
        let mut digits = String::new();
        let mut frac_digits: i64 = 0;
        let mut seen_dot = false;
        let mut any_digit = false;
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_digit() {
                digits.push(c as char);
                any_digit = true;
                if seen_dot {
                    frac_digits += 1;
                }
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
            } else {
                break;
            }
            i += 1;
        }
        if !any_digit {
            return None;
        }
        let mut exp: i64 = 0;
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            i += 1;
            let mut eneg = false;
            if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                eneg = b[i] == b'-';
                i += 1;
            }
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if start == i {
                return None;
            }
            // Expoentes absurdos saturam (o decNumber daria overflow para infinito).
            let e: i64 = s[start..i].parse().unwrap_or(i64::MAX / 4);
            exp = if eneg { -e } else { e };
        }
        if i != b.len() {
            return None;
        }
        let trimmed = digits.trim_start_matches('0');
        let coeff = if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() };
        Some(Decimal::Finite { neg, coeff, exp: exp.saturating_sub(frac_digits) })
    }

    /// `decNumberToString` (to-scientific-string).
    pub fn to_text(&self) -> String {
        match self {
            Decimal::NaN => "NaN".to_string(),
            Decimal::Inf { neg } => if *neg { "-Infinity" } else { "Infinity" }.to_string(),
            Decimal::Finite { neg, coeff, exp } => {
                let mut out = String::new();
                if *neg {
                    out.push('-');
                }
                let n = coeff.len() as i64;
                let exp = *exp;
                let adjusted = exp + n - 1;
                if exp <= 0 && adjusted >= -6 {
                    if exp == 0 {
                        out.push_str(coeff);
                    } else {
                        let point = n + exp;
                        if point > 0 {
                            out.push_str(&coeff[..point as usize]);
                            out.push('.');
                            out.push_str(&coeff[point as usize..]);
                        } else {
                            out.push_str("0.");
                            for _ in 0..(-point) {
                                out.push('0');
                            }
                            out.push_str(coeff);
                        }
                    }
                } else {
                    out.push_str(&coeff[..1]);
                    if n > 1 {
                        out.push('.');
                        out.push_str(&coeff[1..]);
                    }
                    out.push('E');
                    if adjusted >= 0 {
                        out.push('+');
                    }
                    let _ = write!(out, "{adjusted}");
                }
                out
            }
        }
    }

    /// `jvp_literal_number_to_double`: `decNumberReduce` num contexto decimal64 com 17 dígitos (meio
    /// para o par, expoente entre -383 e 384) e `strtod` do resultado.
    pub fn to_f64(&self) -> f64 {
        match self {
            Decimal::NaN => f64::NAN,
            Decimal::Inf { neg } => {
                if *neg {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            }
            Decimal::Finite { neg, coeff, exp } => {
                let (coeff, exp) = round_half_even(coeff, *exp, 17);
                let sign = if *neg { -1.0 } else { 1.0 };
                if coeff == "0" {
                    return sign * 0.0;
                }
                let adjusted = exp + coeff.len() as i64 - 1;
                if adjusted > 384 {
                    return sign * f64::INFINITY;
                }
                let text = alloc::format!("{coeff}e{exp}");
                sign * text.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
    }

    fn is_zero(&self) -> bool {
        matches!(self, Decimal::Finite { coeff, .. } if coeff == "0")
    }

    /// `decNumberCompare` entre dois decimais sem NaN.
    fn cmp_dec(&self, other: &Decimal) -> Ordering {
        use Decimal::*;
        let sign = |d: &Decimal| -> i8 {
            match d {
                Inf { neg } | Finite { neg, .. } if !d.is_zero() => {
                    if *neg {
                        -1
                    } else {
                        1
                    }
                }
                _ => 0,
            }
        };
        let (sa, sb) = (sign(self), sign(other));
        if sa != sb {
            return sa.cmp(&sb);
        }
        if sa == 0 {
            return Ordering::Equal;
        }
        // Mesmo sinal: compara magnitudes e inverte se negativo.
        let mag = match (self, other) {
            (Inf { .. }, Inf { .. }) => Ordering::Equal,
            (Inf { .. }, _) => Ordering::Greater,
            (_, Inf { .. }) => Ordering::Less,
            (Finite { coeff: ca, exp: ea, .. }, Finite { coeff: cb, exp: eb, .. }) => {
                let adj_a = *ea + ca.len() as i64;
                let adj_b = *eb + cb.len() as i64;
                if adj_a != adj_b {
                    adj_a.cmp(&adj_b)
                } else {
                    // Mesma ordem de grandeza: compara os dígitos alinhados à esquerda.
                    let n = ca.len().max(cb.len());
                    let pa = ca.bytes().chain(core::iter::repeat(b'0')).take(n);
                    let pb = cb.bytes().chain(core::iter::repeat(b'0')).take(n);
                    pa.cmp(pb)
                }
            }
            _ => Ordering::Equal,
        };
        if sa < 0 {
            mag.reverse()
        } else {
            mag
        }
    }
}

/// Arredonda o coeficiente para no máximo `digits` dígitos (meio para o par), ajustando o expoente.
fn round_half_even(coeff: &str, exp: i64, digits: usize) -> (String, i64) {
    if coeff.len() <= digits {
        return (coeff.to_string(), exp);
    }
    let drop = coeff.len() - digits;
    let (keep, rest) = coeff.split_at(digits);
    let first = rest.as_bytes()[0];
    let tail_nonzero = rest[1..].bytes().any(|c| c != b'0');
    let last_odd = (keep.as_bytes()[digits - 1] - b'0') % 2 == 1;
    let up = first > b'5' || (first == b'5' && (tail_nonzero || last_odd));
    let mut kept: alloc::vec::Vec<u8> = keep.bytes().collect();
    let mut exp = exp + drop as i64;
    if up {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                kept.pop();
                exp += 1;
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let s = String::from_utf8(kept).unwrap_or_default();
    (s, exp)
}

impl Literal {
    /// Literal a partir do decimal (texto canônico e double calculados aqui).
    pub fn new(dec: Decimal) -> Literal {
        let text = dec.to_text();
        let value = dec.to_f64();
        Literal { dec, text, value }
    }

    /// O que o jq imprime para este literal: o texto do decNumber, `null` para NaN, e o double
    /// saturado para infinito (`jv_dump_term`).
    pub fn dump(&self) -> String {
        match &self.dec {
            Decimal::NaN => "null".to_string(),
            Decimal::Inf { neg } => dtoa_fmt(if *neg { -f64::MAX } else { f64::MAX }),
            Decimal::Finite { .. } => self.text.clone(),
        }
    }
}

impl Num {
    /// Número a partir do texto de um literal (programa ou entrada JSON). `None` se o decNumber não
    /// aceitar o texto.
    pub fn from_literal(text: &str) -> Option<Num> {
        let dec = Decimal::parse(text)?;
        if let Decimal::Finite { neg, coeff, exp: 0 } = &dec {
            // Inteiro curto: a forma canônica é a mesma que o double imprime.
            if coeff.len() <= 15 && !(*neg && coeff == "0") {
                let i: isize = coeff.parse().ok()?;
                return Some(Num::Int(if *neg { -i } else { i }));
            }
        }
        Some(Num::Dec(Rc::new(Literal::new(dec))))
    }

    /// Resultado de uma conta em double.
    pub fn from_f64(x: f64) -> Num {
        if x.fract() == 0.0 && x.abs() < EXACT && !(x == 0.0 && x.is_sign_negative()) {
            Num::Int(x as isize)
        } else {
            Num::Float(x)
        }
    }

    /// Inteiro de máquina (resultado exato), usado por `length`, índices etc.
    pub fn from_integral(i: usize) -> Num {
        if (i as f64) < EXACT {
            Num::Int(i as isize)
        } else {
            Num::Float(i as f64)
        }
    }

    /// `jv_number_value`.
    pub fn as_f64(&self) -> f64 {
        match self {
            Num::Int(i) => *i as f64,
            Num::Float(f) => *f,
            Num::Dec(l) => l.value,
        }
    }

    /// Valor inteiro exato, se for `Int`.
    pub fn as_isize(&self) -> Option<isize> {
        match self {
            Num::Int(i) => Some(*i),
            _ => {
                let f = self.as_f64();
                (f.fract() == 0.0 && f.abs() < EXACT).then_some(f as isize)
            }
        }
    }

    /// `jvp_number_is_nan`.
    pub fn is_nan(&self) -> bool {
        match self {
            Num::Dec(l) => matches!(l.dec, Decimal::NaN),
            _ => self.as_f64().is_nan(),
        }
    }

    /// `jv_is_integer`: parte fracionária menor que `DBL_EPSILON`.
    pub fn is_integer(&self) -> bool {
        match self {
            Num::Int(_) => true,
            _ => {
                let x = self.as_f64();
                let fpart = x - x.trunc();
                fpart.abs() < f64::EPSILON
            }
        }
    }

    /// Texto do número na saída do jq (`jv_dump_term`).
    pub fn dump(&self) -> String {
        match self {
            Num::Int(i) => i.to_string(),
            Num::Float(f) => {
                if f.is_nan() {
                    "null".to_string()
                } else {
                    dtoa_fmt(f.clamp(-f64::MAX, f64::MAX))
                }
            }
            Num::Dec(l) => l.dump(),
        }
    }

    /// `jvp_number_cmp`: decimal entre dois literais, double no resto (sem tratar NaN).
    pub fn cmp_values(&self, other: &Num) -> Ordering {
        if let (Num::Dec(a), Num::Dec(b)) = (self, other) {
            if !matches!(a.dec, Decimal::NaN) && !matches!(b.dec, Decimal::NaN) {
                return a.dec.cmp_dec(&b.dec);
            }
        }
        let (a, b) = (self.as_f64(), other.as_f64());
        if a < b {
            Ordering::Less
        } else if a == b {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }

    /// `jv_cmp` para números: NaN conta como menor que qualquer número (inclusive outro NaN).
    pub fn jv_cmp(&self, other: &Num) -> Ordering {
        if self.is_nan() {
            Ordering::Less
        } else if other.is_nan() {
            Ordering::Greater
        } else {
            self.cmp_values(other)
        }
    }

    /// `jvp_number_equal`: NaN nunca é igual a nada.
    pub fn jv_equal(&self, other: &Num) -> bool {
        if self.is_nan() || other.is_nan() {
            return false;
        }
        self.cmp_values(other) == Ordering::Equal
    }

    /// `jv_identical` para números: mesmo literal (ponteiro) ou mesmo double nativo.
    pub fn identical(&self, other: &Num) -> bool {
        match (self, other) {
            (Num::Dec(a), Num::Dec(b)) => Rc::ptr_eq(a, b),
            (Num::Dec(_), _) | (_, Num::Dec(_)) => false,
            (a, b) => a.as_f64().to_bits() == b.as_f64().to_bits(),
        }
    }
}

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        self.jv_equal(other)
    }
}

impl Eq for Num {}

impl PartialOrd for Num {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Num {
    fn cmp(&self, other: &Self) -> Ordering {
        self.jv_cmp(other)
    }
}

impl Hash for Num {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(0);
        let f = self.as_f64();
        if f.is_finite() {
            // 0.0 e -0.0 são iguais.
            let f = if f == 0.0 { 0.0 } else { f };
            f.to_bits().hash(state);
        }
    }
}

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.dump())
    }
}

/// `jvp_dtoa_fmt`: os dígitos mais curtos que fazem ida e volta, em notação fixa ou científica como o
/// `freedtoa` do jq (científica quando o expoente decimal é menor que -4 ou maior que dígitos + 15),
/// com expoente de pelo menos dois dígitos e sinal.
pub fn dtoa_fmt(x: f64) -> String {
    if x.is_nan() {
        return "null".into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = alloc::format!("{:e}", x.abs());
    let (mant, e) = sci.split_once('e').unwrap_or((&sci, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let ndig = digits.len() as i32;
    let decpt = e + 1;
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if decpt <= -4 || decpt > ndig + 15 {
        out.push_str(&digits[..1]);
        if ndig > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let ex = decpt - 1;
        out.push(if ex < 0 { '-' } else { '+' });
        let _ = write!(out, "{:02}", ex.abs());
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..(-decpt) {
            out.push('0');
        }
        out.push_str(&digits);
    } else if decpt >= ndig {
        out.push_str(&digits);
        for _ in 0..(decpt - ndig) {
            out.push('0');
        }
    } else {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(s: &str) -> String {
        Decimal::parse(s).unwrap().to_text()
    }

    #[test]
    fn decnumber_canonical_forms_match_jq() {
        assert_eq!(canon("1.0"), "1.0");
        assert_eq!(canon("1.50"), "1.50");
        assert_eq!(canon("100"), "100");
        assert_eq!(canon("1e2"), "1E+2");
        assert_eq!(canon("1E-2"), "0.01");
        assert_eq!(canon("0.10e1"), "1.0");
        assert_eq!(canon("3.000"), "3.000");
        assert_eq!(canon("1e1000"), "1E+1000");
        assert_eq!(canon("1e-7"), "1E-7");
        assert_eq!(canon("1e-5"), "0.00001");
        assert_eq!(canon("1.5e300"), "1.5E+300");
        assert_eq!(canon("-0"), "-0");
    }

    #[test]
    fn dtoa_matches_jq() {
        assert_eq!(dtoa_fmt(0.30000000000000004), "0.30000000000000004");
        assert_eq!(dtoa_fmt(1e20), "1e+20");
        assert_eq!(dtoa_fmt(1e16), "1e+16");
        assert_eq!(dtoa_fmt(12345678901234567890123.0), "12345678901234568000000");
        assert_eq!(dtoa_fmt(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(dtoa_fmt(3.0), "3");
        assert_eq!(dtoa_fmt(0.5), "0.5");
        assert_eq!(dtoa_fmt(-1.5), "-1.5");
        assert_eq!(dtoa_fmt(1e-5), "1e-05");
    }

    #[test]
    fn literals_and_comparisons() {
        let big = Num::from_literal("10000000000000000000000000000001").unwrap();
        let lit = Num::from_literal("10000000000000000000000000000000").unwrap();
        assert_eq!(big.cmp_values(&lit), Ordering::Greater);
        let computed = Num::from_f64(big.as_f64() + 1.0);
        assert_eq!(computed.cmp_values(&lit), Ordering::Equal);
        assert_eq!(Num::from_literal("13911860366432393").unwrap().as_f64(), 13911860366432392.0);
        assert_eq!(Num::from_literal("10000000000000000").unwrap().dump(), "10000000000000000");
        assert_eq!(Num::from_f64(1e16).dump(), "1e+16");
        assert_eq!(Num::from_literal("1E1000").unwrap().dump(), "1.7976931348623157e+308");
        assert!(Num::from_literal("nan").unwrap().is_nan());
        assert_eq!(Num::from_literal("1.0").unwrap(), Num::Int(1));
    }
}
