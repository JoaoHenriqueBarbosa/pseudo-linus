//! Métodos mágicos de valores embutidos (`[].__len__`, `{}.__contains__`, `1 .__add__`...): cada um
//! delega à mesma operação que a sintaxe usa, então `x.__len__()` e `len(x)` não podem divergir.

use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{type_error, PyResult, Vm};

fn recv_and<'a>(args: &'a [Value], name: &str, n: usize) -> PyResult<&'a [Value]> {
    if args.len() != n + 1 {
        return Err(type_error(format!("expected {n} argument{}, got {}", if n == 1 { "" } else { "s" }, args.len() - 1)));
    }
    let _ = name;
    Ok(args)
}

fn via_builtin(vm: &mut Vm, name: &str, args: Vec<Value>) -> PyResult<Value> {
    let f = crate::builtins::get(name).expect("builtin existe");
    vm.call(&f, args, Kw::new())
}

fn contains(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let a = recv_and(&args, "__contains__", 1)?;
    Ok(Value::Bool(crate::vm::py_contains(&a[0], &a[1])?))
}

fn len(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__len__", 0)?;
    via_builtin(vm, "len", args)
}

fn getitem(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let a = recv_and(&args, "__getitem__", 1)?;
    crate::vm::py_subscript(&a[0], &a[1])
}

fn setitem(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let a = recv_and(&args, "__setitem__", 2)?;
    crate::vm::store_subscript(&a[0], &a[1], a[2].clone())?;
    Ok(Value::None)
}

fn delitem(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let a = recv_and(&args, "__delitem__", 1)?;
    vm.delete_subscript(&a[0], &a[1])?;
    Ok(Value::None)
}

fn iter(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__iter__", 0)?;
    via_builtin(vm, "iter", args)
}

fn hash(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__hash__", 0)?;
    via_builtin(vm, "hash", args)
}

fn repr(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__repr__", 0)?;
    via_builtin(vm, "repr", args)
}

fn str_(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__str__", 0)?;
    via_builtin(vm, "str", args)
}

fn bool_(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__bool__", 0)?;
    via_builtin(vm, "bool", args)
}

fn compare(sym: &str, args: &[Value]) -> PyResult<Value> {
    let a = recv_and(args, sym, 1)?;
    Ok(Value::Bool(crate::vm::py_compare(sym, &a[0], &a[1])?))
}

fn eq(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    compare("==", &args)
}
fn ne(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    compare("!=", &args)
}
fn lt(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    compare("<", &args)
}
fn le(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    compare("<=", &args)
}
fn gt(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    compare(">", &args)
}
fn ge(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    compare(">=", &args)
}

fn binary(sym: &str, args: &[Value]) -> PyResult<Value> {
    let a = recv_and(args, sym, 1)?;
    crate::vm::py_binary(sym, &a[0], &a[1])
}

fn add(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("+", &args)
}
fn sub(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("-", &args)
}
fn mul(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("*", &args)
}
fn truediv(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("/", &args)
}
fn floordiv(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("//", &args)
}
fn mod_(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("%", &args)
}
fn pow(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    binary("**", &args)
}

fn format(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    recv_and(&args, "__format__", 1)?;
    via_builtin(vm, "format", args)
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("__contains__", contains),
    ("__len__", len),
    ("__getitem__", getitem),
    ("__setitem__", setitem),
    ("__delitem__", delitem),
    ("__iter__", iter),
    ("__hash__", hash),
    ("__repr__", repr),
    ("__str__", str_),
    ("__bool__", bool_),
    ("__eq__", eq),
    ("__ne__", ne),
    ("__lt__", lt),
    ("__le__", le),
    ("__gt__", gt),
    ("__ge__", ge),
    ("__add__", add),
    ("__sub__", sub),
    ("__mul__", mul),
    ("__truediv__", truediv),
    ("__floordiv__", floordiv),
    ("__mod__", mod_),
    ("__pow__", pow),
    ("__format__", format),
];
