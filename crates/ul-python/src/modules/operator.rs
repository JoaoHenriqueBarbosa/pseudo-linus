//! Módulo `operator` do CPython 3.13, só as funções sem estado: aritmética, comparação, lógica,
//! bits sobre inteiros, `getitem` e `contains`.
//!
//! Ficam de fora: `itemgetter`, `attrgetter` e `methodcaller` (são classes que guardam estado),
//! `setitem`/`delitem`/`countOf`/`indexOf`/`length_hint`, os operadores `i*` (in-place), `matmul`,
//! `concat` de outros tipos além dos que o `+` já cobre, e `getitem` com fatias.

use std::rc::Rc;

use crate::ast::UnaryOp;
use crate::modules::ModuleBuilder;
use crate::native_util::{exactly, no_kwargs, want_int};
use crate::object::{is, py_eq, repr, Kw, ModuleObj, Value};
use crate::vm::{exc, py_binary, py_lt, type_error, PyException, PyResult, Vm};

fn pair<'a>(fname: &str, args: &'a [Value], kw: &Kw) -> PyResult<(&'a Value, &'a Value)> {
    no_kwargs(fname, kw)?;
    exactly(fname, args, 2)?;
    Ok((&args[0], &args[1]))
}

fn single<'a>(fname: &str, args: &'a [Value], kw: &Kw) -> PyResult<&'a Value> {
    no_kwargs(fname, kw)?;
    exactly(fname, args, 1)?;
    Ok(&args[0])
}

macro_rules! arith {
    ($f:ident, $py:literal, $sym:literal) => {
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let (a, b) = pair($py, &args, &kw)?;
            py_binary($sym, a, b)
        }
    };
}

arith!(op_add, "add", "+");
arith!(op_sub, "sub", "-");
arith!(op_mul, "mul", "*");
arith!(op_truediv, "truediv", "/");
arith!(op_floordiv, "floordiv", "//");
arith!(op_mod, "mod", "%");
arith!(op_pow, "pow", "**");

// As unárias são as do interpretador (`PyNumber_Negative` e afins): inteiro grande, métodos
// especiais de classe do usuário e as mesmas mensagens de erro.
fn op_neg(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::vm::unary(UnaryOp::USub, single("neg", &args, &kw)?)
}

fn op_pos(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::vm::unary(UnaryOp::UAdd, single("pos", &args, &kw)?)
}

fn op_abs(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = single("abs", &args, &kw)?.clone();
    crate::builtins::b_abs(vm, vec![v], Vec::new())
}

fn op_invert(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::vm::unary(UnaryOp::Invert, single("invert", &args, &kw)?)
}

fn op_not(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    Ok(Value::Bool(!single("not_", &args, &kw)?.is_true()))
}

fn op_truth(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    Ok(Value::Bool(single("truth", &args, &kw)?.is_true()))
}

fn op_index(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = single("index", &args, &kw)?;
    if matches!(v, Value::Big(_)) {
        return Ok(v.clone());
    }
    Ok(Value::Int(want_int(v)?))
}

fn op_is(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("is_", &args, &kw)?;
    Ok(Value::Bool(is(a, b)))
}

fn op_is_not(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("is_not", &args, &kw)?;
    Ok(Value::Bool(!is(a, b)))
}

fn op_eq(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("eq", &args, &kw)?;
    Ok(Value::Bool(py_eq(a, b)))
}

fn op_ne(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("ne", &args, &kw)?;
    Ok(Value::Bool(!py_eq(a, b)))
}

/// Reescreve o `TypeError` de `<` para o símbolo da comparação pedida.
fn fix_order_error(e: PyException, sym: &str, a: &Value, b: &Value) -> PyException {
    if e.kind == "TypeError" && e.msg.starts_with("'<' not supported between instances of") {
        return type_error(format!(
            "'{sym}' not supported between instances of '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ));
    }
    e
}

fn op_lt(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("lt", &args, &kw)?;
    Ok(Value::Bool(py_lt(a, b)?))
}

fn op_le(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("le", &args, &kw)?;
    let lt = py_lt(a, b).map_err(|e| fix_order_error(e, "<=", a, b))?;
    Ok(Value::Bool(lt || py_eq(a, b)))
}

fn op_gt(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("gt", &args, &kw)?;
    Ok(Value::Bool(py_lt(b, a).map_err(|e| fix_order_error(e, ">", a, b))?))
}

fn op_ge(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("ge", &args, &kw)?;
    let gt = py_lt(b, a).map_err(|e| fix_order_error(e, ">=", a, b))?;
    Ok(Value::Bool(gt || py_eq(a, b)))
}

// ---------------------------------------------------------------------------
// bits
// ---------------------------------------------------------------------------

fn op_and(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("and_", &args, &kw)?;
    py_binary("&", a, b)
}

fn op_or(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("or_", &args, &kw)?;
    py_binary("|", a, b)
}

fn op_xor(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("xor", &args, &kw)?;
    py_binary("^", a, b)
}

fn op_lshift(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("lshift", &args, &kw)?;
    py_binary("<<", a, b)
}

fn op_rshift(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("rshift", &args, &kw)?;
    py_binary(">>", a, b)
}

// ---------------------------------------------------------------------------
// getitem / contains
// ---------------------------------------------------------------------------

fn as_index(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

fn normalize(i: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let i = if i < 0 { i + len } else { i };
    (0..len).contains(&i).then_some(i as usize)
}

fn getitem_value(container: &Value, index: &Value) -> PyResult<Value> {
    let seq_index = |what: &str| -> PyResult<i64> {
        as_index(index)
            .ok_or_else(|| type_error(format!("{what} indices must be integers or slices, not {}", index.type_name())))
    };
    match container {
        Value::Ext(e) => match e.getitem(index) {
            Some(r) => r,
            None => Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
        },
        Value::List(l) => {
            let i = seq_index("list")?;
            let items = l.borrow();
            normalize(i, items.len())
                .map(|k| items[k].clone())
                .ok_or_else(|| exc("IndexError", "list index out of range"))
        }
        Value::Tuple(t) => {
            let i = seq_index("tuple")?;
            normalize(i, t.len()).map(|k| t[k].clone()).ok_or_else(|| exc("IndexError", "tuple index out of range"))
        }
        Value::Str(s) => {
            let i = as_index(index)
                .ok_or_else(|| type_error(format!("string indices must be integers, not '{}'", index.type_name())))?;
            normalize(i, s.len())
                .and_then(|k| s.unit_at(k))
                .map(Value::str)
                .ok_or_else(|| exc("IndexError", "string index out of range"))
        }
        Value::Bytes(b) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("byte indices must be integers or slices, not {}", index.type_name()))
            })?;
            normalize(i, b.len()).map(|k| Value::Int(i64::from(b[k]))).ok_or_else(|| exc("IndexError", "index out of range"))
        }
        Value::Dict(d) => match d.borrow().get(index)? {
            Some(v) => Ok(v),
            None => Err(PyException {
                kind: "KeyError",
                msg: repr(index),
                value: Some(Value::Exception(Rc::new(crate::object::ExcObj::new("KeyError", vec![index.clone()])))),
                tb: Vec::new(),
            }),
        },
        _ => Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
    }
}

fn op_getitem(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("getitem", &args, &kw)?;
    getitem_value(a, b)
}

fn contains_value(container: &Value, item: &Value) -> PyResult<bool> {
    let member = |items: &[Value]| items.iter().any(|x| is(x, item) || py_eq(x, item));
    match container {
        Value::List(l) => Ok(member(&l.borrow()[..])),
        Value::Tuple(t) => Ok(member(&t[..])),
        Value::Str(s) => match item {
            Value::Str(sub) => Ok(s.as_str().contains(sub.as_str())),
            _ => Err(type_error(format!("'in <string>' requires string as left operand, not {}", item.type_name()))),
        },
        Value::Dict(d) => Ok(d.borrow().contains(item)?),
        Value::Set(s) => Ok(s.borrow().contains(item)?),
        Value::Range(r) => match as_index(item) {
            Some(i) => Ok(r.contains_int(i)),
            None => Ok(false),
        },
        Value::Bytes(b) => match as_index(item) {
            Some(i) if (0..256).contains(&i) => Ok(b.contains(&(i as u8))),
            Some(_) => Err(exc("ValueError", "byte must be in range(0, 256)")),
            None => match item {
                Value::Bytes(sub) => Ok(sub.is_empty() || b.windows(sub.len()).any(|w| w == &sub[..])),
                _ => Err(type_error(format!("a bytes-like object is required, not '{}'", item.type_name()))),
            },
        },
        _ => Err(type_error(format!("argument of type '{}' is not iterable", container.type_name()))),
    }
}

fn op_contains(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (a, b) = pair("contains", &args, &kw)?;
    Ok(Value::Bool(contains_value(a, b)?))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_operator")
        .func("add", op_add)
        .func("concat", op_add)
        .func("sub", op_sub)
        .func("mul", op_mul)
        .func("truediv", op_truediv)
        .func("floordiv", op_floordiv)
        .func("mod", op_mod)
        .func("pow", op_pow)
        .func("neg", op_neg)
        .func("pos", op_pos)
        .func("abs", op_abs)
        .func("invert", op_invert)
        .func("inv", op_invert)
        .func("not_", op_not)
        .func("truth", op_truth)
        .func("index", op_index)
        .func("is_", op_is)
        .func("is_not", op_is_not)
        .func("eq", op_eq)
        .func("ne", op_ne)
        .func("lt", op_lt)
        .func("le", op_le)
        .func("gt", op_gt)
        .func("ge", op_ge)
        .func("and_", op_and)
        .func("or_", op_or)
        .func("xor", op_xor)
        .func("lshift", op_lshift)
        .func("rshift", op_rshift)
        .func("getitem", op_getitem)
        .func("contains", op_contains)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Dict, NativeFnPtr};

    fn call(f: NativeFnPtr, args: Vec<Value>) -> PyResult<Value> {
        let mut vm = Vm::new();
        f(&mut vm, args, Vec::new())
    }

    fn r(f: NativeFnPtr, args: Vec<Value>) -> String {
        repr(&call(f, args).unwrap())
    }

    fn err(f: NativeFnPtr, args: Vec<Value>) -> String {
        let e = call(f, args).unwrap_err();
        format!("{}: {}", e.kind, e.msg)
    }

    fn i(n: i64) -> Value {
        Value::Int(n)
    }

    #[test]
    fn arithmetic() {
        assert_eq!(r(op_add, vec![i(2), i(3)]), "5");
        assert_eq!(r(op_add, vec![Value::str("a"), Value::str("b")]), "'ab'");
        assert_eq!(r(op_sub, vec![i(5), i(2)]), "3");
        assert_eq!(r(op_mul, vec![i(6), i(7)]), "42");
        assert_eq!(r(op_truediv, vec![i(7), i(2)]), "3.5");
        assert_eq!(r(op_floordiv, vec![i(7), i(2)]), "3");
        assert_eq!(r(op_mod, vec![i(7), i(3)]), "1");
        assert_eq!(r(op_pow, vec![i(2), i(10)]), "1024");
        assert_eq!(r(op_neg, vec![i(5)]), "-5");
        assert_eq!(r(op_neg, vec![Value::Float(0.0)]), "-0.0");
        assert_eq!(r(op_pos, vec![i(5)]), "5");
        assert_eq!(r(op_abs, vec![i(-5)]), "5");
        assert_eq!(r(op_invert, vec![i(5)]), "-6");
        assert_eq!(err(op_neg, vec![Value::str("x")]), "TypeError: bad operand type for unary -: 'str'");
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(r(op_eq, vec![i(1), Value::Float(1.0)]), "True");
        assert_eq!(r(op_ne, vec![i(1), i(2)]), "True");
        assert_eq!(r(op_lt, vec![i(1), i(2)]), "True");
        assert_eq!(r(op_le, vec![i(2), i(2)]), "True");
        assert_eq!(r(op_gt, vec![i(3), i(2)]), "True");
        assert_eq!(r(op_ge, vec![i(2), i(3)]), "False");
        assert_eq!(r(op_not, vec![i(0)]), "True");
        assert_eq!(r(op_truth, vec![Value::str("")]), "False");
        assert_eq!(r(op_is, vec![Value::None, Value::None]), "True");
        assert_eq!(r(op_is_not, vec![Value::None, i(1)]), "True");
        assert_eq!(err(op_le, vec![i(1), Value::str("a")]), "TypeError: '<=' not supported between instances of 'int' and 'str'");
        assert_eq!(err(op_gt, vec![i(1), Value::str("a")]), "TypeError: '>' not supported between instances of 'int' and 'str'");
        assert_eq!(err(op_ge, vec![i(1), Value::str("a")]), "TypeError: '>=' not supported between instances of 'int' and 'str'");
    }

    #[test]
    fn bit_operations() {
        assert_eq!(r(op_and, vec![i(12), i(10)]), "8");
        assert_eq!(r(op_or, vec![i(12), i(10)]), "14");
        assert_eq!(r(op_xor, vec![i(12), i(10)]), "6");
        assert_eq!(r(op_and, vec![Value::Bool(true), Value::Bool(false)]), "False");
        assert_eq!(r(op_lshift, vec![i(1), i(4)]), "16");
        assert_eq!(r(op_rshift, vec![i(-16), i(2)]), "-4");
        assert_eq!(err(op_lshift, vec![i(1), i(-1)]), "ValueError: negative shift count");
    }

    #[test]
    fn getitem_and_contains() {
        let l = Value::list(vec![i(1), i(2), i(3)]);
        assert_eq!(r(op_getitem, vec![l.clone(), i(-1)]), "3");
        assert_eq!(err(op_getitem, vec![l.clone(), i(3)]), "IndexError: list index out of range");
        assert_eq!(r(op_getitem, vec![Value::str("abc"), i(1)]), "'b'");
        assert_eq!(r(op_getitem, vec![Value::tuple(vec![i(9)]), i(0)]), "9");
        let mut d = Dict::default();
        d.set(Value::str("a"), i(1)).unwrap();
        let d = Value::dict(d);
        assert_eq!(r(op_getitem, vec![d.clone(), Value::str("a")]), "1");
        let e = call(op_getitem, vec![d.clone(), Value::str("zz")]).unwrap_err();
        assert_eq!(e.kind, "KeyError");
        assert_eq!(r(op_contains, vec![l, i(2)]), "True");
        assert_eq!(r(op_contains, vec![Value::str("hello"), Value::str("ell")]), "True");
        assert_eq!(r(op_contains, vec![d, Value::str("a")]), "True");
    }
}
