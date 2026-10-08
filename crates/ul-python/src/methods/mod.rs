//! Métodos dos tipos embutidos (`str`, `list`, `dict`, `set`, `tuple`, `bytes`, `int`, `float`).
//!
//! Cada tipo tem uma tabela `TABLE` de `(nome, função)` no arquivo dele. A função tem a assinatura
//! de [`NativeFnPtr`](crate::object::NativeFnPtr) e recebe o receptor em `args[0]`:
//!
//! ```ignore
//! fn str_upper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> { ... }
//! pub const TABLE: &[(&str, NativeFnPtr)] = &[("upper", str_upper), ...];
//! ```
//!
//! `lookup` é o que `getattr` usa: `"abc".upper` vira um método preso ao receptor.

pub mod bytearraym;
pub mod bytesm;
pub mod dictm;
pub mod dunder;
pub mod listm;
pub mod numm;
pub mod rangem;
pub mod setm;
pub mod strm;
pub mod tuplem;

use crate::object::{NativeFnPtr, Value};

/// Tabela de métodos do tipo do receptor.
fn table(recv: &Value) -> Option<&'static [(&'static str, NativeFnPtr)]> {
    Some(match recv {
        Value::Str(_) => strm::TABLE,
        Value::List(_) => listm::TABLE,
        Value::Dict(_) => dictm::TABLE,
        Value::Set(s) if s.borrow().is_frozen() => setm::FROZEN_TABLE,
        Value::Set(_) => setm::TABLE,
        Value::Tuple(_) => tuplem::TABLE,
        Value::Bytes(_) => bytesm::TABLE,
        Value::Int(_) | Value::Big(_) | Value::Bool(_) | Value::Float(_) => numm::TABLE,
        Value::Range(_) => rangem::TABLE,
        // Sem método comum: só os mágicos de `dunder`.
        Value::Slice(_) | Value::None => &[],
        // Os objetos de tipo nativo (iteradores, exceções, funções) só herdam os mágicos de `object` e do tipo.
        Value::Ext(_) | Value::Exception(_) | Value::Function(_) => &[],
        _ => return None,
    })
}

/// O método `name` do tipo de `recv`, com o nome estático da tabela. O que o `dir()` do tipo no CPython
/// não lista não existe (`[].__index__`, `(1).__len__`, `(1.5).bit_length`). Os objetos de tipo nativo
/// (`Ext`, exceção, função) só têm o que a tabela do oráculo lista para o tipo deles; sem linha na tabela
/// (um tipo de módulo próprio), nada.
pub fn lookup(recv: &Value, name: &str) -> Option<(&'static str, NativeFnPtr)> {
    let listed = if matches!(recv, Value::Ext(_) | Value::Exception(_) | Value::Function(_)) {
        crate::builtins_ext::type_listed(recv.type_name(), name)
    } else {
        crate::builtins_ext::type_has_name(recv.type_name(), name)
    };
    if !listed {
        return None;
    }
    let find = |t: &'static [(&'static str, NativeFnPtr)]| t.iter().find(|(n, _)| *n == name).map(|(n, f)| (*n, *f));
    let own = if matches!(recv, Value::ByteArray(_)) {
        find(bytearraym::TABLE).or_else(|| find(bytesm::TABLE))
    } else {
        find(table(recv)?)
    };
    own.or_else(|| find(dunder::TABLE))
}

/// Atributo de um tipo que só existe como nome (`NoneType`): o interpretador o lê pelo `typeattrs`.
pub fn value_attr(obj: &Value, name: &str) -> Option<Value> {
    match obj {
        Value::Builtin(t) => crate::typeattrs::type_attr(t, name),
        _ => None,
    }
}
