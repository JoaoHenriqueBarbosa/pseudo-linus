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
        Value::Set(_) => setm::TABLE,
        Value::Tuple(_) => tuplem::TABLE,
        Value::Bytes(_) => bytesm::TABLE,
        Value::Int(_) | Value::Big(_) | Value::Bool(_) | Value::Float(_) => numm::TABLE,
        _ => return None,
    })
}

/// O método `name` do tipo de `recv`, com o nome estático da tabela.
pub fn lookup(recv: &Value, name: &str) -> Option<(&'static str, NativeFnPtr)> {
    let find = |t: &'static [(&'static str, NativeFnPtr)]| t.iter().find(|(n, _)| *n == name).map(|(n, f)| (*n, *f));
    if matches!(recv, Value::ByteArray(_)) {
        return find(bytearraym::TABLE).or_else(|| find(bytesm::TABLE)).or_else(|| find(dunder::TABLE));
    }
    find(table(recv)?).or_else(|| find(dunder::TABLE))
}
