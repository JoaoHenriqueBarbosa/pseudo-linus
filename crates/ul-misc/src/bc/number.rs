//
// Copyright (c) 2024-2026 Hemi Labs, Inc.
//
// This file is part of the posixutils-rs project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT
//
// Modificado no pseudo-linus (2026, MIT): o `Number` sobre `BigDecimal` do upstream virou `Num`
// sobre `BigUint` com sinal e escala explícitos, porque o GNU bc 1.07.1 guarda o sinal à parte da
// magnitude e isso aparece na saída (o "-0" de uma potência truncada, a comparação que põe -0 abaixo
// de 0). Do upstream ficaram a conversão exata de constantes em base qualquer, a contagem de dígitos
// fracionários em `obase` diferente de 10 e a largura dos grupos de dígitos acima da base 16.

//! Números do bc com a semântica do GNU bc 1.07.1, medida em caixa preta no oráculo:
//!
//! - soma e subtração exatas, escala do resultado = maior escala; sinal decidido como no GNU (dois
//!   negativos somam num negativo mesmo quando a magnitude é zero; magnitudes iguais com sinais
//!   opostos dão zero positivo);
//! - multiplicação truncada em `min(sa+sb, max(scale, sa, sb))`, divisão truncada em `scale`, resto
//!   `a - (a/b)*b` com o quociente em `scale` e o resultado em `max(sa, sb+scale)`; as duas
//!   normalizam o zero pra positivo;
//! - potência exata e truncada só no fim, em `min(sa*e, max(scale, sa))` (expoente negativo: `1/x^e`
//!   em `scale`), o que preserva o sinal de um resultado que truncou pra zero;
//! - raiz quadrada exatamente truncada em `max(scale, sa)`, com 0 e 1 devolvidos com escala 0;
//! - comparação que olha o sinal antes da magnitude.

use std::cmp::Ordering;

use num_bigint::BigUint;
use num_traits::{One, ToPrimitive, Zero};

/// `BC_SCALE_MAX` do `limits`.
pub const SCALE_MAX: u64 = 2_147_483_647;
/// `BC_BASE_MAX` do `limits` (maior `obase`).
pub const BASE_MAX: u64 = 2_147_483_647;
/// `BC_DIM_MAX` do `limits` (maior índice de array).
pub const DIM_MAX: u64 = 16_777_215;
/// Maior `ibase` (dígitos 0-9 e A-Z).
pub const IBASE_MAX: u64 = 36;

/// Teto de dígitos decimais de um valor intermediário. O GNU aloca o que a conta pedir até o
/// `malloc` falhar ("Fatal error: Out of memory for malloc."); aqui o teto faz o papel do limite de
/// memória do sandbox, com a mesma mensagem.
pub const MAX_DIGITS: u64 = 20_000_000;

/// A conta precisaria de mais memória do que o teto permite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfMemory;

pub type NumResult<T> = Result<T, OutOfMemory>;

/// Erros da potência, com o texto do GNU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaiseError {
    /// `exponent too large in raise`
    TooLarge,
    /// `divide by zero` (zero elevado a expoente negativo)
    DivideByZero,
    OutOfMemory,
}

impl From<OutOfMemory> for RaiseError {
    fn from(_: OutOfMemory) -> Self {
        RaiseError::OutOfMemory
    }
}

fn check_digits(n: u64) -> NumResult<()> {
    if n > MAX_DIGITS {
        Err(OutOfMemory)
    } else {
        Ok(())
    }
}

/// `10^n`.
pub fn pow10(n: u64) -> NumResult<BigUint> {
    check_digits(n)?;
    Ok(BigUint::from(10u32).pow(n as u32))
}

/// Número de dígitos decimais de uma magnitude (zero tem 1).
fn decimal_digits(m: &BigUint) -> u64 {
    if m.is_zero() {
        return 1;
    }
    // Estimativa pelos bits, corrigida com uma comparação exata.
    let bits = m.bits();
    let est = ((bits - 1) as f64 * std::f64::consts::LOG10_2).floor() as u64 + 1;
    // est é o número de dígitos de 2^(bits-1); m pode ter um a mais.
    match pow10(est) {
        Ok(p) if *m >= p => est + 1,
        _ => est,
    }
}

/// Um número do bc: `(-1)^neg * mag / 10^scale`.
#[derive(Clone, Debug, Default)]
pub struct Num {
    neg: bool,
    mag: BigUint,
    scale: u32,
}

impl Num {
    pub fn zero() -> Num {
        Num::default()
    }

    pub fn one() -> Num {
        Num {
            neg: false,
            mag: BigUint::one(),
            scale: 0,
        }
    }

    pub fn from_u64(v: u64) -> Num {
        Num {
            neg: false,
            mag: BigUint::from(v),
            scale: 0,
        }
    }

    pub fn from_i64(v: i64) -> Num {
        Num {
            neg: v < 0,
            mag: BigUint::from(v.unsigned_abs()),
            scale: 0,
        }
    }

    /// Monta a partir das partes (pra testes e pra biblioteca matemática).
    pub fn from_parts(neg: bool, mag: BigUint, scale: u32) -> Num {
        Num { neg, mag, scale }
    }

    pub fn mag(&self) -> &BigUint {
        &self.mag
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// O bit de sinal (verdadeiro também no "-0").
    pub fn is_neg(&self) -> bool {
        self.neg
    }

    /// Zero pela magnitude (o `bc_is_zero` do GNU, que ignora o sinal).
    pub fn is_zero(&self) -> bool {
        self.mag.is_zero()
    }

    /// A magnitude reescalada pra `s` dígitos fracionários (truncando se `s` for menor).
    fn mag_at(&self, s: u32) -> NumResult<BigUint> {
        let cur = self.scale;
        if s >= cur {
            Ok(&self.mag * pow10(u64::from(s - cur))?)
        } else {
            Ok(&self.mag / pow10(u64::from(cur - s))?)
        }
    }

    /// O mesmo valor com outra escala (truncando ao reduzir), sinal preservado.
    pub fn with_scale(&self, s: u32) -> NumResult<Num> {
        Ok(Num {
            neg: self.neg,
            mag: self.mag_at(s)?,
            scale: s,
        })
    }

    /// Parte inteira (truncada em direção a zero), com o sinal do original.
    pub fn trunc(&self) -> Num {
        let mag = if self.scale == 0 {
            self.mag.clone()
        } else {
            // 10^scale cabe: o próprio número já tem esses dígitos.
            &self.mag / BigUint::from(10u32).pow(self.scale)
        };
        Num {
            neg: self.neg,
            mag,
            scale: 0,
        }
    }

    /// Estimativa barata (por cima) do total de dígitos decimais da magnitude.
    fn digits_est(&self) -> u64 {
        (self.mag.bits() as f64 * std::f64::consts::LOG10_2) as u64 + 1
    }

    /// Estimativa por cima dos dígitos da parte inteira.
    fn int_digits_est(&self) -> u64 {
        self.digits_est().saturating_sub(u64::from(self.scale)) + 1
    }

    /// Magnitude da parte inteira.
    pub fn int_mag(&self) -> BigUint {
        self.trunc().mag
    }

    /// Dígitos decimais da parte inteira (o `n_len` do GNU: 1 quando a parte inteira é zero).
    pub fn int_len(&self) -> u64 {
        decimal_digits(&self.int_mag())
    }

    /// `length()`: dígitos da parte inteira (se não for zero) mais a escala, no mínimo 1.
    pub fn length(&self) -> u64 {
        let int = self.int_mag();
        let n = if int.is_zero() {
            0
        } else {
            decimal_digits(&int)
        } + u64::from(self.scale);
        n.max(1)
    }

    /// Compara magnitudes.
    fn cmp_mag(&self, other: &Num) -> Ordering {
        if self.scale == other.scale {
            return self.mag.cmp(&other.mag);
        }
        let s = self.scale.max(other.scale);
        // Escalar pra cima nunca passa do tamanho que os próprios números já têm somado.
        let a = &self.mag * BigUint::from(10u32).pow(s - self.scale);
        let b = &other.mag * BigUint::from(10u32).pow(s - other.scale);
        a.cmp(&b)
    }

    /// Comparação do GNU: sinal primeiro (então -0 < 0), depois magnitude.
    pub fn compare(&self, other: &Num) -> Ordering {
        if self.neg != other.neg {
            return if self.neg {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let m = self.cmp_mag(other);
        if self.neg { m.reverse() } else { m }
    }

    fn mag_sum(a: &Num, b: &Num, s: u32) -> NumResult<BigUint> {
        Ok(a.mag_at(s)? + b.mag_at(s)?)
    }

    fn mag_diff(big: &Num, small: &Num, s: u32) -> NumResult<BigUint> {
        Ok(big.mag_at(s)? - small.mag_at(s)?)
    }

    /// `bc_add` com `scale_min`.
    pub fn add(&self, other: &Num, scale_min: u32) -> NumResult<Num> {
        let s = scale_min.max(self.scale).max(other.scale);
        check_digits(u64::from(s) + self.int_digits_est().max(other.int_digits_est()) + 1)?;
        if self.neg == other.neg {
            return Ok(Num {
                neg: self.neg,
                mag: Num::mag_sum(self, other, s)?,
                scale: s,
            });
        }
        Ok(match self.cmp_mag(other) {
            Ordering::Less => Num {
                neg: other.neg,
                mag: Num::mag_diff(other, self, s)?,
                scale: s,
            },
            Ordering::Equal => Num {
                neg: false,
                mag: BigUint::zero(),
                scale: s,
            },
            Ordering::Greater => Num {
                neg: self.neg,
                mag: Num::mag_diff(self, other, s)?,
                scale: s,
            },
        })
    }

    /// `bc_sub` com `scale_min`.
    pub fn sub(&self, other: &Num, scale_min: u32) -> NumResult<Num> {
        let s = scale_min.max(self.scale).max(other.scale);
        check_digits(u64::from(s) + self.int_digits_est().max(other.int_digits_est()) + 1)?;
        if self.neg != other.neg {
            return Ok(Num {
                neg: self.neg,
                mag: Num::mag_sum(self, other, s)?,
                scale: s,
            });
        }
        Ok(match self.cmp_mag(other) {
            Ordering::Less => Num {
                neg: !other.neg,
                mag: Num::mag_diff(other, self, s)?,
                scale: s,
            },
            Ordering::Equal => Num {
                neg: false,
                mag: BigUint::zero(),
                scale: s,
            },
            Ordering::Greater => Num {
                neg: self.neg,
                mag: Num::mag_diff(self, other, s)?,
                scale: s,
            },
        })
    }

    /// Negação: o GNU faz `0 - x`, então -0 vira 0 e 0 continua 0.
    pub fn negate(&self) -> Num {
        if self.mag.is_zero() {
            return Num {
                neg: false,
                mag: BigUint::zero(),
                scale: self.scale,
            };
        }
        Num {
            neg: !self.neg,
            mag: self.mag.clone(),
            scale: self.scale,
        }
    }

    /// `bc_multiply`.
    pub fn mul(&self, other: &Num, scale: u32) -> NumResult<Num> {
        let full = u64::from(self.scale) + u64::from(other.scale);
        let want = u64::from(scale.max(self.scale).max(other.scale));
        let prod_scale = full.min(want);
        check_digits(self.digits_est() + other.digits_est())?;
        let prod = &self.mag * &other.mag;
        let mag = if prod_scale < full {
            prod / pow10(full - prod_scale)?
        } else {
            prod
        };
        let neg = self.neg != other.neg && !mag.is_zero();
        Ok(Num {
            neg,
            mag,
            scale: prod_scale as u32,
        })
    }

    /// `bc_divide`: quociente truncado com exatamente `scale` dígitos. `None` se o divisor é zero.
    pub fn div(&self, other: &Num, scale: u32) -> NumResult<Option<Num>> {
        if other.mag.is_zero() {
            return Ok(None);
        }
        check_digits(u64::from(scale) + self.int_digits_est() + u64::from(other.scale) + 1)?;
        // floor(|a|/|b| * 10^scale) = floor(am * 10^(scale + sb - sa) / bm)
        let shift = i64::from(scale) + i64::from(other.scale) - i64::from(self.scale);
        let mag = if shift >= 0 {
            (&self.mag * pow10(shift as u64)?) / &other.mag
        } else {
            &self.mag / (&other.mag * pow10(shift.unsigned_abs())?)
        };
        let neg = self.neg != other.neg && !mag.is_zero();
        Ok(Some(Num { neg, mag, scale }))
    }

    /// `bc_modulo`: `a - (a/b)*b`. `None` se o divisor é zero.
    pub fn modulo(&self, other: &Num, scale: u32) -> NumResult<Option<Num>> {
        let Some(q) = self.div(other, scale)? else {
            return Ok(None);
        };
        let rscale = u64::from(self.scale).max(u64::from(other.scale) + u64::from(scale));
        let rscale = u32::try_from(rscale).map_err(|_| OutOfMemory)?;
        let t = q.mul(other, rscale)?;
        Ok(Some(self.sub(&t, rscale)?))
    }

    /// O `bc_num2long` do GNU 1.07.1 como ele se comporta: lê os dígitos da parte inteira enquanto o
    /// acumulado não passa de 214748364 e devolve 0 se sobrar dígito (é o que faz `1^2147483649`
    /// funcionar e `1^2147483650` dar "exponent too large").
    pub fn num2long(&self) -> i64 {
        let int = self.int_mag();
        let digits = int.to_str_radix(10);
        let mut val: i64 = 0;
        let mut used = 0;
        for d in digits.bytes() {
            if val > 214_748_364 {
                break;
            }
            val = val * 10 + i64::from(d - b'0');
            used += 1;
        }
        if used < digits.len() {
            return 0;
        }
        if self.neg { -val } else { val }
    }

    /// `bc_raise`. Devolve o resultado e se houve o aviso "non-zero scale in exponent".
    pub fn raise(&self, expo: &Num, scale: u32) -> Result<(Num, bool), RaiseError> {
        let warn = expo.scale != 0;
        let e = expo.num2long();
        if e == 0 {
            if !expo.int_mag().is_zero() {
                return Err(RaiseError::TooLarge);
            }
            return Ok((Num::one(), warn));
        }
        let negative = e < 0;
        let e = e.unsigned_abs();
        let sa = u64::from(self.scale);
        let rscale = if negative {
            u64::from(scale)
        } else {
            sa.saturating_mul(e).min(u64::from(scale).max(sa))
        };
        let full_scale = sa.saturating_mul(e);
        let odd = e % 2 == 1;
        // Base 0 ou 1 (inteira): o resultado não cresce, qualquer que seja o expoente.
        if !negative && self.scale == 0 && (self.mag.is_zero() || self.mag.is_one()) {
            let neg = if self.mag.is_zero() {
                e == 1 && self.neg
            } else {
                self.neg && odd
            };
            return Ok((
                Num {
                    neg,
                    mag: self.mag.clone(),
                    scale: 0,
                },
                warn,
            ));
        }
        // Dígitos do resultado exato antes de truncar.
        check_digits(self.digits_est().saturating_mul(e))?;
        let mag = self.mag.pow(e as u32);
        let neg = if mag.is_zero() {
            e == 1 && self.neg
        } else {
            self.neg && odd
        };
        let power = Num {
            neg,
            mag,
            scale: full_scale as u32,
        };
        if negative {
            let one = Num::one();
            return match one.div(&power, rscale as u32)? {
                Some(q) => Ok((q, warn)),
                None => Err(RaiseError::DivideByZero),
            };
        }
        // Truncar reduzindo a escala, como o GNU (o sinal fica, mesmo que a magnitude zere).
        Ok((power.with_scale(rscale as u32)?, warn))
    }

    /// `bc_sqrt`: `None` pra negativo (inclusive -0).
    pub fn sqrt(&self, scale: u32) -> NumResult<Option<Num>> {
        match self.compare(&Num::zero()) {
            Ordering::Less => return Ok(None),
            Ordering::Equal => return Ok(Some(Num::zero())),
            Ordering::Greater => {}
        }
        if self.compare(&Num::one()) == Ordering::Equal {
            return Ok(Some(Num::one()));
        }
        let rscale = scale.max(self.scale);
        check_digits(2 * u64::from(rscale) + self.int_digits_est())?;
        // floor(sqrt(m / 10^s) * 10^r) = floor(sqrt(m * 10^(2r - s)))
        let shift = 2 * i64::from(rscale) - i64::from(self.scale);
        let n = if shift >= 0 {
            &self.mag * pow10(shift as u64)?
        } else {
            &self.mag / pow10(shift.unsigned_abs())?
        };
        Ok(Some(Num {
            neg: false,
            mag: n.sqrt(),
            scale: rscale,
        }))
    }

    /// Valor da parte inteira de |x| como `u64`, se couber.
    pub fn int_u64(&self) -> Option<u64> {
        self.int_mag().to_u64()
    }

    /// Converte uma constante do programa (dígitos 0-9 e A-Z, ponto opcional, já sem zeros à
    /// esquerda) com a base `ibase`, como o GNU faz a cada execução: parte inteira de um dígito só
    /// vale o próprio dígito; com mais dígitos, cada dígito maior ou igual à base vale `base-1`; a
    /// fração é truncada em tantos dígitos decimais quantos foram escritos.
    pub fn parse_constant(text: &[u8], ibase: u32) -> NumResult<Num> {
        let digit = |c: u8| -> u32 {
            match c {
                b'0'..=b'9' => u32::from(c - b'0'),
                b'A'..=b'Z' => u32::from(c - b'A') + 10,
                _ => 0,
            }
        };
        let (int_part, frac_part) = match text.iter().position(|&c| c == b'.') {
            Some(p) => (&text[..p], &text[p + 1..]),
            None => (text, &text[text.len()..]),
        };
        check_digits((int_part.len() + frac_part.len()) as u64 * 2 + 2)?;
        let base = BigUint::from(ibase);
        let top = ibase - 1;
        let int = if ibase == 10 && int_part.iter().all(u8::is_ascii_digit) && !int_part.is_empty()
        {
            BigUint::parse_bytes(int_part, 10).unwrap_or_default()
        } else if int_part.len() == 1 {
            BigUint::from(digit(int_part[0]))
        } else {
            let mut v = BigUint::zero();
            for &c in int_part {
                v = v * &base + BigUint::from(digit(c).min(top));
            }
            v
        };
        let n = frac_part.len() as u32;
        if n == 0 {
            return Ok(Num {
                neg: false,
                mag: int,
                scale: 0,
            });
        }
        let ten_n = pow10(u64::from(n))?;
        let frac = if ibase == 10 && frac_part.iter().all(u8::is_ascii_digit) {
            BigUint::parse_bytes(frac_part, 10).unwrap_or_default()
        } else {
            let mut num = BigUint::zero();
            for &c in frac_part {
                num = num * &base + BigUint::from(digit(c).min(top));
            }
            num * &ten_n / base.pow(n)
        };
        Ok(Num {
            neg: false,
            mag: int * ten_n + frac,
            scale: n,
        })
    }

    /// Escreve o número em `obase` (o `out_num` do GNU), um byte por vez em `out`.
    pub fn write(&self, obase: u64, out: &mut dyn FnMut(u8)) {
        if self.neg {
            out(b'-');
        }
        if self.mag.is_zero() {
            out(b'0');
            return;
        }
        let scale = self.scale;
        let ten_s = BigUint::from(10u32).pow(scale);
        let int = &self.mag / &ten_s;
        if obase == 10 {
            let digits = self.mag.to_str_radix(10);
            let digits = digits.as_bytes();
            let sc = scale as usize;
            let (ip, fp): (Vec<u8>, Vec<u8>) = if digits.len() > sc {
                (
                    digits[..digits.len() - sc].to_vec(),
                    digits[digits.len() - sc..].to_vec(),
                )
            } else {
                let mut f = vec![b'0'; sc - digits.len()];
                f.extend_from_slice(digits);
                (Vec::new(), f)
            };
            for c in ip {
                out(c);
            }
            if scale > 0 {
                out(b'.');
                for c in fp {
                    out(c);
                }
            }
            return;
        }
        let frac = &self.mag - &int * &ten_s;
        let width = digit_width(obase);
        if !int.is_zero() {
            if obase <= 16 {
                for c in int.to_str_radix(obase as u32).to_uppercase().bytes() {
                    out(c);
                }
            } else {
                let base = BigUint::from(obase);
                let mut rest = int;
                let mut stack = Vec::new();
                while !rest.is_zero() {
                    stack.push((&rest % &base).to_u64().unwrap_or(0));
                    rest /= &base;
                    sysabi::sys::checkpoint();
                }
                for d in stack.iter().rev() {
                    out(b' ');
                    write_padded(*d, width, out);
                }
            }
        }
        if scale > 0 {
            out(b'.');
            let count = fractional_digits_for(obase, u64::from(scale));
            let base = BigUint::from(obase);
            let mut f = frac;
            for i in 0..count {
                f *= &base;
                let d = (&f / &ten_s).to_u64().unwrap_or(0);
                f -= BigUint::from(d) * &ten_s;
                if obase <= 16 {
                    out(b"0123456789ABCDEF"[d as usize]);
                } else {
                    if i > 0 {
                        out(b' ');
                    }
                    write_padded(d, width, out);
                }
                if i % 1024 == 1023 {
                    sysabi::sys::checkpoint();
                }
            }
        }
    }

    /// Texto em base 10 (pra testes e mensagens).
    pub fn to_text(&self, obase: u64) -> String {
        let mut s = Vec::new();
        self.write(obase, &mut |c| s.push(c));
        String::from_utf8_lossy(&s).into_owned()
    }
}

/// Largura de um dígito acima da base 16: os dígitos decimais de `base - 1` (o `max_o_digit`).
fn digit_width(base: u64) -> usize {
    (base - 1).max(1).ilog10() as usize + 1
}

fn write_padded(d: u64, width: usize, out: &mut dyn FnMut(u8)) {
    for c in format!("{d:0width$}").bytes() {
        out(c);
    }
}

/// Quantos dígitos fracionários em `base` pra um valor com `scale` dígitos decimais: o menor `k`
/// com `base^k >= 10^scale` (o laço do `out_num` com `t_num`).
fn fractional_digits_for(base: u64, scale: u64) -> u64 {
    if scale == 0 {
        return 0;
    }
    if base == 10 {
        return scale;
    }
    let target = BigUint::from(10u32).pow(scale as u32);
    let base_big = BigUint::from(base);
    let mut k = ((scale as f64) / (base as f64).log10()).ceil().max(1.0) as u64;
    while base_big.pow(k as u32) < target {
        k += 1;
    }
    while k > 1 && base_big.pow(k as u32 - 1) >= target {
        k -= 1;
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Num {
        let (neg, body) = match s.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, s),
        };
        let x = Num::parse_constant(body.as_bytes(), 10).unwrap();
        if neg { Num { neg: true, ..x } } else { x }
    }

    fn t(x: &Num) -> String {
        x.to_text(10)
    }

    #[test]
    fn output_base10() {
        assert_eq!(t(&n("0.5")), ".5");
        assert_eq!(t(&n("-0.5")), "-.5");
        assert_eq!(t(&n("123.4500")), "123.4500");
        assert_eq!(t(&n("0.000")), "0");
        assert_eq!(t(&n("1.0")), "1.0");
    }

    #[test]
    fn output_other_bases() {
        assert_eq!(n("255").to_text(16), "FF");
        assert_eq!(n("-31").to_text(16), "-1F");
        assert_eq!(n("16.5").to_text(17), " 16.08");
        assert_eq!(n("-17").to_text(17), "- 01 00");
        assert_eq!(n("-1234567.891").to_text(1000), "- 001 234 567.891");
        assert_eq!(n(".1").to_text(16), ".1");
        assert_eq!(n("0.001").to_text(17), ".00 00 04");
        let third = n("1").div(&n("3"), 20).unwrap().unwrap();
        assert_eq!(third.to_text(16), ".55555555555555554");
        assert_eq!(n(".1").to_text(2), ".0001");
    }

    #[test]
    fn constants_in_other_bases() {
        let c = |s: &str, b: u32| t(&Num::parse_constant(s.as_bytes(), b).unwrap());
        assert_eq!(c("12", 2), "3");
        assert_eq!(c("9", 2), "9");
        assert_eq!(c("A.0", 2), "10.0");
        assert_eq!(c("AB.0", 2), "3.0");
        assert_eq!(c(".A", 2), ".5");
        assert_eq!(c(".1", 16), "0");
        assert_eq!(c(".FF", 16), ".99");
        assert_eq!(c("FF.FF", 16), "255.99");
        assert_eq!(c(".2222", 3), ".9876");
        assert_eq!(c("1A", 10), "19");
        assert_eq!(c("ZZ", 10), "99");
        assert_eq!(c(".123456789ABCDEF", 16), ".071111111111111");
    }

    #[test]
    fn scales_of_operations() {
        assert_eq!(t(&n("7.5").modulo(&n("2"), 2).unwrap().unwrap()), "0");
        assert_eq!(t(&n("-7").modulo(&n("3"), 2).unwrap().unwrap()), "-.01");
        assert_eq!(t(&n("7.5").modulo(&n("2"), 0).unwrap().unwrap()), "1.5");
        assert_eq!(t(&n("10").modulo(&n("3.3"), 3).unwrap().unwrap()), ".0010");
        assert_eq!(t(&n("1").div(&n("3"), 2).unwrap().unwrap()), ".33");
        assert!(n("1").div(&n("0"), 2).unwrap().is_none());
        assert_eq!(t(&n("1.5").mul(&n("1.5"), 0).unwrap()), "2.2");
        assert_eq!(t(&n("1.5").mul(&n("1.5"), 5).unwrap()), "2.25");
    }

    #[test]
    fn raise_rules() {
        let r = |a: &str, b: &str, s: u32| t(&n(a).raise(&n(b), s).unwrap().0);
        assert_eq!(r("2", "10", 0), "1024");
        assert_eq!(r("2", "-1", 0), "0");
        assert_eq!(r("2", "-3", 5), ".12500");
        assert_eq!(r("1.5", "3", 3), "3.375");
        assert_eq!(r("2.5", "0", 3), "1");
        assert_eq!(r("-0.0395", "19", 0), "-0");
        assert_eq!(r("-2", "-1", 10), "-.5000000000");
        assert_eq!(
            n("1").raise(&n("2147483649"), 0).unwrap().0.to_text(10),
            "1"
        );
        assert_eq!(
            n("1").raise(&n("2147483650"), 0).unwrap_err(),
            RaiseError::TooLarge
        );
        assert_eq!(
            n("0").raise(&n("-1"), 0).unwrap_err(),
            RaiseError::DivideByZero
        );
        assert!(n("2").raise(&n("1.5"), 0).unwrap().1);
    }

    #[test]
    fn negative_zero_rules() {
        let z = n("-0.01").raise(&n("3"), 0).unwrap().0;
        assert_eq!(t(&z), "-0");
        assert_eq!(t(&z.add(&n("0"), 0).unwrap()), "0");
        assert_eq!(t(&z.sub(&n("0"), 0).unwrap()), "-0");
        assert_eq!(t(&z.add(&z, 0).unwrap()), "-0");
        assert_eq!(t(&z.sub(&z, 0).unwrap()), "0");
        assert_eq!(t(&z.negate()), "0");
        assert_eq!(t(&z.mul(&n("1"), 0).unwrap()), "0");
        assert_eq!(t(&z.modulo(&n("1"), 0).unwrap().unwrap()), "-0");
        assert_eq!(z.compare(&n("0")), Ordering::Less);
        assert!(z.sqrt(0).unwrap().is_none());
    }

    #[test]
    fn sqrt_and_length() {
        assert_eq!(
            t(&n("2").sqrt(30).unwrap().unwrap()),
            "1.414213562373095048801688724209"
        );
        assert_eq!(t(&n("1.00").sqrt(5).unwrap().unwrap()), "1");
        assert_eq!(n("0.000").sqrt(5).unwrap().unwrap().scale(), 0);
        assert_eq!(t(&n("2.25").sqrt(0).unwrap().unwrap()), "1.50");
        assert_eq!(n("0").length(), 1);
        assert_eq!(n("0.000").length(), 3);
        assert_eq!(n(".001").length(), 3);
        assert_eq!(n("-12.30").length(), 4);
        assert_eq!(n("10.0").length(), 3);
    }
}
