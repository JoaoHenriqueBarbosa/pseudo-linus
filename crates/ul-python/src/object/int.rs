//! `int` (`Objects/longobject.c`), por enquanto limitado a `i64`.
//!
//! Ponto de troca da fatia 19: toda aritmética inteira passa pelas funções `int_*` deste módulo. Hoje
//! elas fazem a conta com `checked_*` e, fora da faixa de `i64`, devolvem `ObjError::IntOverflow`, um
//! erro interno do interpretador (o CPython nunca transborda: promove para inteiro arbitrário). A
//! fatia 19 troca `Value::Int(i64)` por um inteiro pequeno inline mais um `BigInt` próprio, e estas
//! funções passam a promover em vez de falhar; nenhum chamador precisa mudar.

use super::ObjError;

/// Módulo do hash numérico (`_PyHASH_MODULUS`, o primo de Mersenne 2**61 - 1).
pub const HASH_MODULUS: u64 = (1 << HASH_BITS) - 1;
pub const HASH_BITS: u32 = 61;

/// `repr()` e `str()` de `int`.
pub fn int_repr(v: i64) -> String {
    v.to_string()
}

/// `long_hash`: valor absoluto módulo 2**61 - 1 com o sinal do número; -1 vira -2, porque -1 é o
/// código de erro do `tp_hash`.
pub fn int_hash(v: i64) -> i64 {
    let a = (v.unsigned_abs() % HASH_MODULUS) as i64;
    let h = if v < 0 { -a } else { a };
    if h == -1 { -2 } else { h }
}

pub fn int_add(a: i64, b: i64) -> Result<i64, ObjError> {
    a.checked_add(b).ok_or(ObjError::IntOverflow)
}

pub fn int_sub(a: i64, b: i64) -> Result<i64, ObjError> {
    a.checked_sub(b).ok_or(ObjError::IntOverflow)
}

pub fn int_mul(a: i64, b: i64) -> Result<i64, ObjError> {
    a.checked_mul(b).ok_or(ObjError::IntOverflow)
}

/// `-a`: só `-(-2**63)` transborda.
pub fn int_neg(a: i64) -> Result<i64, ObjError> {
    a.checked_neg().ok_or(ObjError::IntOverflow)
}
