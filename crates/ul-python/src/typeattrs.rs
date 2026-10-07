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
        "bytes" => Value::bytes(Vec::new()),
        "bytearray" => Value::bytearray(Vec::new()),
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
    fn repr(&self) -> String {
        if self.name.starts_with("__") && self.name.ends_with("__") {
            format!("<slot wrapper '{}' of '{}' objects>", self.name, self.tname)
        } else {
            format!("<method '{}' of '{}' objects>", self.name, self.tname)
        }
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
        // `dict.__getitem__(self, k)` numa subclasse que sobrescreve `__getitem__`: vale o método do tipo
        // embutido sobre o dado de dentro da instância, não a sobrescrita (senão recursa).
        let target = crate::vm::unwrap_payload(&recv);
        let bound = vm.getattr(&target, self.name)?;
        vm.call_value(&bound, args, kw)
    }
}

/// `int.__new__(cls, valor)`, `str.__new__(cls, ...)`: instância de `cls` com o valor embutido.
struct NewFn {
    tname: &'static str,
}

thread_local! {
    static NEW_FNS: std::cell::RefCell<Vec<(&'static str, Value)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// O `__new__` de um tipo embutido: o mesmo objeto a cada leitura, como o `tp_new` do CPython.
fn new_fn(tname: &'static str) -> Value {
    NEW_FNS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((_, v)) = m.iter().find(|(n, _)| *n == tname) {
            return v.clone();
        }
        let v = Value::Ext(Rc::new(NewFn { tname }));
        m.push((tname, v.clone()));
        v
    })
}

impl ExtObject for NewFn {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" => Some(Ok(Value::str("__new__"))),
            "__qualname__" => Some(Ok(Value::str(format!("{}.__new__", self.tname)))),
            "__self__" => crate::builtins::get(self.tname).map(Ok),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        // `int.__new__(int, ...)`: o próprio tipo embutido, sem subclasse, é a chamada do construtor.
        if let Some(first) = args.first() {
            if crate::builtins::class_name(first).is_some_and(|n| n == self.tname) {
                let ctor = crate::builtins::get(self.tname).unwrap_or(Value::Builtin("object"));
                return vm.call_value(&ctor, args[1..].to_vec(), kw);
            }
        }
        let Some(Value::Class(c)) = args.first() else {
            return Err(type_error(format!("{}.__new__(X): X is not a type object", self.tname)));
        };
        let rest: Vec<Value> = args[1..].iter().map(crate::vm::unwrap_payload).collect();
        let ctor = crate::builtins::get(self.tname).unwrap_or(Value::Builtin("object"));
        let payload = vm.call_value(&ctor, rest, kw)?;
        Ok(Value::Instance(Rc::new(crate::object::InstanceObj {
            class: c.clone(),
            view: Default::default(),
            dict: std::cell::RefCell::new(Default::default()),
            payload: std::cell::RefCell::new(Some(payload)),
        })))
    }
}

fn bytearray_fromhex(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match bytes_fromhex(vm, args, kw)? {
        Value::Bytes(b) => Ok(Value::bytearray(b.to_vec())),
        other => Ok(other),
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
    let mag = num_bigint::BigInt::from_bytes_be(num_bigint::Sign::Plus, &bytes);
    let bits = bytes.len() * 8;
    let n = if signed && bits > 0 && !bytes.is_empty() && bytes[0] & 0x80 != 0 {
        mag - (num_bigint::BigInt::from(1) << bits)
    } else {
        mag
    };
    Ok(crate::bigint::norm(n))
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

/// `bytes.fromhex(texto)`: pares de dígitos hexadecimais, espaços ASCII entre os bytes são ignorados.
fn bytes_fromhex(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let [Value::Str(s)] = args.as_slice() else {
        return Err(type_error("fromhex() argument must be str"));
    };
    let chars: Vec<char> = s.as_str().chars().collect();
    let bad = |at: usize| crate::vm::exc("ValueError", format!("non-hexadecimal number found in fromhex() arg at position {at}"));
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let hi = chars[i].to_digit(16).ok_or_else(|| bad(i))?;
        let lo = match chars.get(i + 1) {
            Some(c) => c.to_digit(16).ok_or_else(|| bad(i + 1))?,
            None => return Err(bad(i + 1)),
        };
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Ok(Value::bytes(out))
}

fn bytes_maketrans(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 2 {
        return Err(type_error("maketrans() takes exactly 2 arguments"));
    }
    let want = |v: &Value| v.bytes_like().ok_or_else(|| type_error("a bytes-like object is required"));
    let (from, to) = (want(&args[0])?, want(&args[1])?);
    if from.len() != to.len() {
        return Err(crate::vm::exc("ValueError", "maketrans arguments must have same length"));
    }
    let mut table: Vec<u8> = (0..=255u8).collect();
    for (f, t) in from.iter().zip(to.iter()) {
        table[*f as usize] = *t;
    }
    Ok(Value::bytes(table))
}

fn native(name: &'static str, f: crate::object::NativeFnPtr) -> Value {
    Value::NativeFn(Rc::new(NativeFn { name, f }))
}

fn object_setattr(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [Value::Instance(i), Value::Str(n), v] => {
            i.sync_from_view();
            i.dict.borrow_mut().insert(n.as_str().to_string(), v.clone());
            i.sync_to_view();
            Ok(Value::None)
        }
        [other, ..] => Err(type_error(format!(
            "can't apply this __setattr__ to {} object",
            other.type_name()
        ))),
        [] => Err(type_error("expected 3 arguments, got 0")),
    }
}

fn object_delattr(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [Value::Instance(i), Value::Str(n)] => {
            i.sync_from_view();
            let removed = i.dict.borrow_mut().shift_remove(n.as_str());
            i.sync_to_view();
            match removed {
                Some(_) => Ok(Value::None),
                None => Err(crate::vm::exc("AttributeError", n.as_str().to_string())),
            }
        }
        _ => Err(type_error("expected 2 arguments")),
    }
}

fn object_getattribute(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [Value::Instance(i), Value::Str(n)] => {
            i.sync_from_view();
            let own = i.dict.borrow().get(n.as_str()).cloned();
            match own {
                Some(v) => Ok(v),
                None => {
                    let obj = Value::Instance(i.clone());
                    vm.instance_getattr_plain(&obj, i, n.as_str())
                }
            }
        }
        [other, Value::Str(n)] => vm.getattr(other, n.as_str()),
        _ => Err(type_error("expected 2 arguments")),
    }
}

fn object_new(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.first() {
        Some(Value::Class(c)) => Ok(Value::Instance(Rc::new(crate::object::InstanceObj {
            class: c.clone(),
            view: Default::default(),
            dict: std::cell::RefCell::new(Default::default()),
            payload: std::cell::RefCell::new(None),
        }))),
        _ => Err(type_error("object.__new__(X): X is not a type object")),
    }
}

/// `object.__str__`: o `repr` do objeto.
fn object_str(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [a] => Ok(Value::str(vm.repr_of(a)?)),
        _ => Err(type_error("expected 0 arguments")),
    }
}

/// `object.__repr__`: `<módulo.Classe object at 0x...>`.
fn object_repr(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [a] => Ok(Value::str(vm.default_text(a, false))),
        _ => Err(type_error("expected 0 arguments")),
    }
}

/// `object.__eq__`: identidade; devolve `NotImplemented` para outro objeto.
fn object_eq(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [a, b] if crate::object::is(a, b) => Ok(Value::Bool(true)),
        [_, _] => Ok(crate::classes::not_implemented()),
        _ => Err(type_error("expected 1 argument")),
    }
}

/// `object.__ne__`: o inverso de `__eq__`, se ele souber responder.
fn object_ne(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let [a, b] = <[Value; 2]>::try_from(args).map_err(|_| type_error("expected 1 argument"))?;
    match vm.call_dunder(&a, "__eq__", vec![b]) {
        Some(Ok(r)) if r.is_true() => Ok(Value::Bool(false)),
        Some(Ok(Value::Bool(false))) => Ok(Value::Bool(true)),
        Some(Ok(_)) | None => Ok(crate::classes::not_implemented()),
        Some(Err(e)) => Err(e),
    }
}

fn object_hash(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.first() {
        Some(v) => Ok(Value::Int(crate::object::hash(v)?)),
        None => Err(type_error("expected 0 arguments")),
    }
}

fn object_init(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::None)
}

/// Atributos de `object`: `object.__setattr__(self, nome, valor)` e companhia.
pub fn object_attr(name: &str) -> Option<Value> {
    Some(match name {
        "__setattr__" => native("__setattr__", object_setattr),
        "__delattr__" => native("__delattr__", object_delattr),
        "__getattribute__" => native("__getattribute__", object_getattribute),
        "__new__" => native("__new__", object_new),
        "__init__" => native("__init__", object_init),
        "__eq__" => native("__eq__", object_eq),
        "__ne__" => native("__ne__", object_ne),
        "__hash__" => native("__hash__", object_hash),
        "__str__" => native("__str__", object_str),
        "__repr__" => native("__repr__", object_repr),
        "__name__" => Value::str("object"),
        _ => return None,
    })
}

/// O nome é o de um tipo embutido de dados (`int`, `dict`...)?
pub fn is_type_name(name: &str) -> bool {
    TYPES.contains(&name)
}

/// `object.__new__`, para classes de usuário que não definem `__new__`.
pub fn object_new_value() -> Value {
    native("__new__", object_new)
}

/// O atributo `name` do tipo embutido `tname`, se existir.
pub fn type_attr(tname: &str, name: &str) -> Option<Value> {
    let tname: &'static str = TYPES.iter().copied().find(|t| *t == tname)?;
    match (tname, name) {
        (_, "__name__" | "__qualname__") => return Some(Value::str(tname)),
        (_, "__module__") => return Some(Value::str("builtins")),
        (t, "__new__") if t != "bool" => return Some(new_fn(tname)),
        ("dict", "fromkeys") => return Some(native("fromkeys", fromkeys)),
        ("int", "from_bytes") => return Some(native("from_bytes", int_from_bytes)),
        ("bytes", "fromhex") => return Some(native("fromhex", bytes_fromhex)),
        ("bytearray", "fromhex") => return Some(native("fromhex", bytearray_fromhex)),
        ("bytes" | "bytearray", "maketrans") => return Some(native("maketrans", bytes_maketrans)),
        ("str", "maketrans") => return Some(native("maketrans", str_maketrans)),
        _ => {}
    }
    let (method, _) = crate::methods::lookup(&sample(tname)?, name)?;
    // Um objeto só por (tipo, método): `dict.__repr__ is dict.__repr__`, e vale de chave de dict
    // (o `pprint` despacha por `type(obj).__repr__`).
    thread_local! {
        static UNBOUND: std::cell::RefCell<std::collections::HashMap<(&'static str, &'static str), Value>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    Some(UNBOUND.with(|c| {
        c.borrow_mut().entry((tname, method)).or_insert_with(|| Value::Ext(Rc::new(Unbound { tname, name: method }))).clone()
    }))
}
