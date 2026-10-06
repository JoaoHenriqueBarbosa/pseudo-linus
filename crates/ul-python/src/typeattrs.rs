//! Atributos de classe dos tipos embutidos (`dict.fromkeys`, `str.join`, `int.from_bytes`...).
//!
//! `dict`, `str` e companhia são `NativeFn`s; quando o programa lê um atributo deles, este módulo
//! devolve o construtor alternativo (`fromkeys`) ou o método não ligado (`str.lower`).

use std::rc::Rc;

use crate::object::{Dict, ExtObject, Kw, NativeFn, Value};
use crate::vm::{type_error, PyResult, Vm};

const TYPES: &[&str] = &["int", "float", "str", "list", "tuple", "dict", "set", "frozenset", "bool", "bytes", "bytearray"];

/// Um valor de exemplo do tipo, só para consultar a tabela de métodos.
fn sample(tname: &str) -> Option<Value> {
    Some(match tname {
        "int" | "bool" => Value::Int(0),
        "float" => Value::Float(0.0),
        "str" => Value::str(""),
        "list" => Value::list(Vec::new()),
        "tuple" => Value::tuple(Vec::new()),
        "dict" => Value::dict(Dict::new()),
        "set" | "frozenset" => Value::set(crate::object::Set::new()),
        "bytes" | "bytearray" => Value::bytes(Vec::new()),
        _ => return None,
    })
}

/// Método não ligado: `str.lower` chamado como `str.lower("ABC")`.
struct Unbound {
    tname: &'static str,
    name: &'static str,
}

impl ExtObject for Unbound {
    fn type_name(&self) -> &'static str {
        "method_descriptor"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, mut args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        if args.is_empty() {
            return Err(type_error(format!(
                "unbound method {}.{}() needs an argument",
                self.tname, self.name
            )));
        }
        let recv = args.remove(0);
        let bound = vm.getattr(&recv, self.name)?;
        vm.call_value(&bound, args, kw)
    }
}

fn fromkeys(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (keys, value) = match args.as_slice() {
        [k] => (k, Value::None),
        [k, v] => (k, v.clone()),
        _ => return Err(type_error(format!("fromkeys expected at least 1 argument, got {}", args.len()))),
    };
    let mut d = Dict::new();
    for k in crate::vm::iterate(keys)? {
        d.set(k, value.clone())?;
    }
    Ok(Value::dict(d))
}

fn int_from_bytes(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let mut byteorder = args.get(1).cloned();
    let mut signed = false;
    for (k, v) in kw {
        match k.as_str() {
            "byteorder" => byteorder = Some(v),
            "signed" => signed = v.is_true(),
            _ => return Err(type_error(format!("from_bytes() got an unexpected keyword argument '{k}'"))),
        }
    }
    let data = match args.first() {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(other) => crate::vm::iterate(other)?
            .into_iter()
            .map(|v| match v {
                Value::Int(i) if (0..256).contains(&i) => Ok(i as u8),
                _ => Err(type_error("cannot convert to bytes")),
            })
            .collect::<PyResult<Vec<u8>>>()?,
        None => return Err(type_error("from_bytes() missing required argument 'bytes' (pos 1)")),
    };
    let little = match byteorder {
        Some(Value::Str(s)) if s.as_str() == "little" => true,
        Some(Value::Str(s)) if s.as_str() == "big" => false,
        None => false,
        _ => return Err(crate::vm::exc("ValueError", "byteorder must be either 'little' or 'big'")),
    };
    let mut bytes = data;
    if little {
        bytes.reverse();
    }
    if bytes.len() > 8 {
        return Err(crate::vm::exc("OverflowError", "Python int too large to convert to C long"));
    }
    let mut v: u64 = 0;
    for b in &bytes {
        v = (v << 8) | u64::from(*b);
    }
    let bits = (bytes.len() * 8) as u32;
    let n = if signed && bits > 0 && bits < 64 && v >> (bits - 1) == 1 {
        (v as i64) - (1i64 << bits)
    } else {
        v as i64
    };
    Ok(Value::Int(n))
}

fn str_maketrans(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let mut d = Dict::new();
    match args.as_slice() {
        [Value::Dict(src)] => {
            for (k, v) in src.borrow().iter() {
                let key = match k {
                    Value::Str(s) if s.as_str().chars().count() == 1 => {
                        Value::Int(i64::from(s.as_str().chars().next().map_or(0, |c| c as u32)))
                    }
                    Value::Str(_) => return Err(type_error("string keys in translate table must be of length 1")),
                    other => other.clone(),
                };
                d.set(key, v.clone())?;
            }
        }
        [Value::Str(a), Value::Str(b)] | [Value::Str(a), Value::Str(b), _] => {
            let (a, b): (Vec<char>, Vec<char>) = (a.as_str().chars().collect(), b.as_str().chars().collect());
            if a.len() != b.len() {
                return Err(crate::vm::exc("ValueError", "the first two maketrans arguments must have equal length"));
            }
            for (x, y) in a.iter().zip(b.iter()) {
                d.set(Value::Int(i64::from(*x as u32)), Value::Int(i64::from(*y as u32)))?;
            }
            if let [_, _, Value::Str(del)] = args.as_slice() {
                for c in del.as_str().chars() {
                    d.set(Value::Int(i64::from(c as u32)), Value::None)?;
                }
            }
        }
        _ => return Err(type_error("maketrans() argument error")),
    }
    Ok(Value::dict(d))
}

fn native(name: &'static str, f: crate::object::NativeFnPtr) -> Value {
    Value::NativeFn(Rc::new(NativeFn { name, f }))
}

/// O atributo `name` do tipo embutido `tname`, se existir.
pub fn type_attr(tname: &str, name: &str) -> Option<Value> {
    let tname: &'static str = TYPES.iter().copied().find(|t| *t == tname)?;
    match (tname, name) {
        (_, "__name__" | "__qualname__") => return Some(Value::str(tname)),
        (_, "__module__") => return Some(Value::str("builtins")),
        ("dict", "fromkeys") => return Some(native("fromkeys", fromkeys)),
        ("int", "from_bytes") => return Some(native("from_bytes", int_from_bytes)),
        ("str", "maketrans") => return Some(native("maketrans", str_maketrans)),
        _ => {}
    }
    let (method, _) = crate::methods::lookup(&sample(tname)?, name)?;
    Some(Value::Ext(Rc::new(Unbound { tname, name: method })))
}
