//! Os acessos a palavras de um `double` que o libm do glibc usa em todo arquivo (`math_private.h`:
//! `GET_HIGH_WORD`, `GET_LOW_WORD`, `SET_HIGH_WORD`) e o `fma`, num lugar só para `glibc_math`,
//! `glibc_hyper` e `glibc_trig` não terem cópia própria.

/// `GET_HIGH_WORD`.
#[inline]
pub(super) fn high_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// `GET_LOW_WORD`.
#[inline]
pub(super) fn low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `SET_HIGH_WORD`.
#[inline]
pub(super) fn set_high_word(x: f64, high: u32) -> f64 {
    f64::from_bits(((high as u64) << 32) | (x.to_bits() & 0xffff_ffff))
}

/// `__builtin_fma`: o nome do C nas fórmulas portadas (a conta é `mul_add`, com um único arredondamento).
#[inline]
pub(super) fn fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}
