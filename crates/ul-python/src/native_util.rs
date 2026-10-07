//! Utilidades para escrever funções nativas (módulos, métodos de tipos embutidos, builtins).
//!
//! Toda função nativa tem a assinatura [`NativeFnPtr`](crate::object::NativeFnPtr):
//! `fn(&mut Vm, Vec<Value>, Kw) -> PyResult<Value>`. Nos métodos de tipo o receptor vem em
//! `args[0]`. Estes helpers padronizam a ligação de argumentos e as mensagens de `TypeError`, que
//! precisam sair iguais às do CPython.

use crate::object::{Kw, Value};
use crate::vm::{exc, type_error, PyResult};

/// Liga posicionais e nomeados aos `names` (posicional-ou-nomeado, na ordem). Os `required`
/// primeiros são obrigatórios. Devolve um slot por nome (`None` = não passado).
pub fn bind(fname: &str, args: Vec<Value>, kw: Kw, names: &[&str], required: usize) -> PyResult<Vec<Option<Value>>> {
    if args.len() > names.len() {
        let n = names.len();
        return Err(type_error(format!(
            "{fname}() takes at most {n} argument{} ({} given)",
            if n == 1 { "" } else { "s" },
            args.len()
        )));
    }
    let mut out: Vec<Option<Value>> = vec![None; names.len()];
    for (i, a) in args.into_iter().enumerate() {
        out[i] = Some(a);
    }
    for (k, v) in kw {
        match names.iter().position(|n| *n == k) {
            Some(i) if out[i].is_none() => out[i] = Some(v),
            Some(_) => return Err(type_error(format!("argument for {fname}() given by name ('{k}') and position"))),
            None => return Err(type_error(format!("{fname}() got an unexpected keyword argument '{k}'"))),
        }
    }
    for (i, slot) in out.iter().enumerate().take(required) {
        if slot.is_none() {
            return Err(type_error(format!("{fname}() missing required argument '{}' (pos {})", names[i], i + 1)));
        }
    }
    Ok(out)
}

/// Recusa nomeados em funções que não os aceitam.
pub fn no_kwargs(fname: &str, kw: &Kw) -> PyResult<()> {
    match kw.first() {
        Some((k, _)) => Err(type_error(format!("{fname}() takes no keyword arguments ('{k}' given)"))),
        None => Ok(()),
    }
}

/// Exige exatamente `n` posicionais.
pub fn exactly(fname: &str, args: &[Value], n: usize) -> PyResult<()> {
    if args.len() == n {
        return Ok(());
    }
    Err(type_error(format!(
        "{fname}() takes exactly {n} argument{} ({} given)",
        if n == 1 { "" } else { "s" },
        args.len()
    )))
}

/// `str` do argumento ou `TypeError: <fname>() argument must be str, not <tipo>`.
pub fn want_str<'a>(fname: &str, v: &'a Value) -> PyResult<&'a str> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        other => Err(type_error(format!("{fname}() argument must be str, not {}", other.type_name()))),
    }
}

/// Argumento inteiro opcional: `default` quando ausente.
pub fn int_or(v: Option<&Value>, default: i64) -> PyResult<i64> {
    v.map(want_int).transpose().map(|i| i.unwrap_or(default))
}

/// `int` do argumento (aceita `bool`) ou `TypeError` com o texto do CPython.
pub fn want_int(v: &Value) -> PyResult<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Big(_) => Err(exc("OverflowError", "Python int too large to convert to C ssize_t")),
        // Instância de subclasse de `int` (`IntEnum`, `IntFlag`...): vale o inteiro que ela carrega.
        Value::Instance(i) if matches!(&*i.payload.borrow(), Some(Value::Int(_) | Value::Bool(_) | Value::Big(_))) => {
            let inner = i.payload.borrow().clone().unwrap_or(Value::None);
            want_int(&inner)
        }
        other => Err(type_error(format!("'{}' object cannot be interpreted as an integer", other.type_name()))),
    }
}

/// `ValueError` com a mensagem.
pub fn value_error(msg: impl Into<String>) -> crate::vm::PyException {
    exc("ValueError", msg)
}
