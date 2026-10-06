//! Métodos de `str` (`Objects/unicodeobject.c`).

use crate::native_util::{exactly, no_kwargs, want_str};
use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{PyResult, Vm};

fn recv(args: &[Value]) -> &str {
    match &args[0] {
        Value::Str(s) => s.as_str(),
        _ => "",
    }
}

fn upper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("upper", &kw)?;
    exactly("upper", &args[1..], 0)?;
    Ok(Value::str(recv(&args).to_uppercase()))
}

fn lower(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("lower", &kw)?;
    exactly("lower", &args[1..], 0)?;
    Ok(Value::str(recv(&args).to_lowercase()))
}

fn startswith(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("startswith", &kw)?;
    exactly("startswith", &args[1..], 1)?;
    Ok(Value::Bool(recv(&args).starts_with(want_str("startswith", &args[1])?)))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[("upper", upper), ("lower", lower), ("startswith", startswith)];
