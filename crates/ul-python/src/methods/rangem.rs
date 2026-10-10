//! Métodos de `range` (`Objects/rangeobject.c`): `count` e `index`.

use crate::object::{py_eq, repr, Kw, NativeFnPtr, Range, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

/// O `range` do receptor e o único argumento de um método `METH_O`.
fn receiver_and_item<'a>(fname: &str, args: &'a [Value], kw: &Kw) -> PyResult<(Range, &'a Value)> {
    if !kw.is_empty() {
        return Err(type_error(format!("range.{fname}() takes no keyword arguments")));
    }
    let (Some(Value::Range(r)), [item]) = (args.first(), &args[1..]) else {
        return Err(type_error(format!("range.{fname}() takes exactly one argument ({} given)", args.len().saturating_sub(1))));
    };
    Ok((*r, item))
}

/// A posição de `item` no `range`: pela conta, se é inteiro; senão, pela igualdade item a item.
fn position(r: Range, item: &Value) -> PyResult<Option<i64>> {
    Ok(match item {
        Value::Int(_) | Value::Bool(_) => {
            let x = match item {
                Value::Int(x) => *x,
                _ => i64::from(item.is_true()),
            };
            r.contains_int(x).then(|| (x - r.start) / r.step)
        }
        Value::Big(_) => None,
        other => {
            for i in 0..r.len() {
                if py_eq(&Value::Int(r.item(i)), other)? {
                    return Ok(Some(i));
                }
            }
            None
        }
    })
}

fn count(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (r, item) = receiver_and_item("count", &args, &kw)?;
    let found = match item {
        Value::Int(_) | Value::Bool(_) | Value::Big(_) => i64::from(position(r, item)?.is_some()),
        other => {
            let mut n = 0;
            for i in 0..r.len() {
                n += i64::from(py_eq(&Value::Int(r.item(i)), other)?);
            }
            n
        }
    };
    Ok(Value::Int(found))
}

fn index(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (r, item) = receiver_and_item("index", &args, &kw)?;
    match position(r, item)? {
        Some(i) => Ok(Value::Int(i)),
        None => Err(exc("ValueError", format!("{} is not in range", repr(item)))),
    }
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[("count", count), ("index", index)];
