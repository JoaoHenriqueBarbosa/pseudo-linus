//! Métodos de `bytearray` (`Objects/bytearrayobject.c`).
//!
//! Os métodos que não mudam o conteúdo vêm de [`bytesm`](super::bytesm); aqui ficam os que mudam
//! (`append`, `extend`, `pop`...) e os envoltórios que devolvem `bytearray` em vez de `bytes`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::native_util::want_int;
use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{exc, iterate, type_error, PyResult, Vm};

use super::bytesm;

type Buf = Rc<RefCell<Vec<u8>>>;

fn this(args: &[Value]) -> PyResult<Buf> {
    match args.first() {
        Some(Value::ByteArray(b)) => Ok(b.clone()),
        _ => Err(type_error("descriptor requires a 'bytearray' object")),
    }
}

fn arity(name: &str, args: &[Value], min: usize, max: usize) -> PyResult<()> {
    let n = args.len() - 1;
    if n < min || n > max {
        return Err(type_error(if min == max {
            format!("bytearray.{name}() takes exactly {min} argument{} ({n} given)", if min == 1 { "" } else { "s" })
        } else if n < min {
            format!("bytearray.{name}() takes at least {min} argument ({n} given)")
        } else {
            format!("bytearray.{name}() takes at most {max} argument ({n} given)")
        }));
    }
    Ok(())
}

/// Um byte: inteiro de 0 a 255.
pub fn want_byte(v: &Value) -> PyResult<u8> {
    let i = match v {
        Value::Int(_) | Value::Bool(_) => want_int(v)?,
        other => {
            return Err(type_error(format!("'{}' object cannot be interpreted as an integer", other.type_name())))
        }
    };
    u8::try_from(i).map_err(|_| exc("ValueError", "byte must be in range(0, 256)"))
}

/// Os bytes que `extend` e a atribuição de fatia aceitam: bytes-like ou iterável de inteiros.
pub fn bytes_of_iterable(v: &Value) -> PyResult<Vec<u8>> {
    if let Some(b) = v.bytes_like() {
        return Ok(b.to_vec());
    }
    if matches!(v, Value::Str(_)) {
        return Err(type_error("expected iterable of integers; got: 'str'"));
    }
    let mut out = Vec::new();
    for x in iterate(v).map_err(|_| type_error(format!("can't extend bytearray with {}", v.type_name())))? {
        out.push(want_byte(&x)?);
    }
    Ok(out)
}

fn append(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("append", &args, 1, 1)?;
    let b = this(&args)?;
    let byte = want_byte(&args[1])?;
    b.borrow_mut().push(byte);
    Ok(Value::None)
}

fn extend(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("extend", &args, 1, 1)?;
    let b = this(&args)?;
    let extra = bytes_of_iterable(&args[1])?;
    b.borrow_mut().extend(extra);
    Ok(Value::None)
}

fn insert(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("insert", &args, 2, 2)?;
    let b = this(&args)?;
    let at = want_int(&args[1])?;
    let byte = want_byte(&args[2])?;
    let mut v = b.borrow_mut();
    let len = v.len() as i64;
    let at = if at < 0 { (at + len).max(0) } else { at.min(len) };
    v.insert(at as usize, byte);
    Ok(Value::None)
}

fn pop(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("pop", &args, 0, 1)?;
    let b = this(&args)?;
    let mut v = b.borrow_mut();
    if v.is_empty() {
        return Err(exc("IndexError", "pop from empty bytearray"));
    }
    let len = v.len() as i64;
    let i = match args.get(1) {
        Some(i) => want_int(i)?,
        None => -1,
    };
    let i = if i < 0 { i + len } else { i };
    if !(0..len).contains(&i) {
        return Err(exc("IndexError", "pop index out of range"));
    }
    Ok(Value::Int(i64::from(v.remove(i as usize))))
}

fn remove(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("remove", &args, 1, 1)?;
    let b = this(&args)?;
    let byte = want_byte(&args[1])?;
    let mut v = b.borrow_mut();
    match v.iter().position(|x| *x == byte) {
        Some(i) => {
            v.remove(i);
            Ok(Value::None)
        }
        None => Err(exc("ValueError", "value not found in bytearray")),
    }
}

fn clear(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("clear", &args, 0, 0)?;
    this(&args)?.borrow_mut().clear();
    Ok(Value::None)
}

fn reverse(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("reverse", &args, 0, 0)?;
    this(&args)?.borrow_mut().reverse();
    Ok(Value::None)
}

fn copy(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    arity("copy", &args, 0, 0)?;
    Ok(Value::bytearray(this(&args)?.borrow().clone()))
}

/// `bytes` vira `bytearray`; listas e tuplas de `bytes` (de `split`, `partition`) também.
fn to_bytearray(v: Value) -> Value {
    match v {
        Value::Bytes(b) => Value::bytearray(b.to_vec()),
        Value::List(l) => Value::list(l.borrow().iter().cloned().map(to_bytearray).collect()),
        Value::Tuple(t) => Value::tuple(t.iter().cloned().map(to_bytearray).collect()),
        other => other,
    }
}

macro_rules! rewrap {
    ($($name:ident),* $(,)?) => {
        $(
            fn $name(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
                bytesm::$name(vm, args, kw).map(to_bytearray)
            }
        )*
    };
}

rewrap!(
    split, rsplit, splitlines, partition, rpartition, strip, lstrip, rstrip, replace, join, upper, lower, capitalize,
    swapcase, title, center, ljust, rjust, zfill, expandtabs, translate, removeprefix, removesuffix,
);

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("append", append),
    ("extend", extend),
    ("insert", insert),
    ("pop", pop),
    ("remove", remove),
    ("clear", clear),
    ("reverse", reverse),
    ("copy", copy),
    ("split", split),
    ("rsplit", rsplit),
    ("splitlines", splitlines),
    ("partition", partition),
    ("rpartition", rpartition),
    ("strip", strip),
    ("lstrip", lstrip),
    ("rstrip", rstrip),
    ("replace", replace),
    ("join", join),
    ("upper", upper),
    ("lower", lower),
    ("capitalize", capitalize),
    ("swapcase", swapcase),
    ("title", title),
    ("center", center),
    ("ljust", ljust),
    ("rjust", rjust),
    ("zfill", zfill),
    ("expandtabs", expandtabs),
    ("translate", translate),
    ("removeprefix", removeprefix),
    ("removesuffix", removesuffix),
];
