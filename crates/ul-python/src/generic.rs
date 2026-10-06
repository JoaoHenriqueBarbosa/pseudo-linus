//! `list[int]` (generic alias) e `int | None` (união de tipos): o suficiente para anotações e
//! `isinstance(x, int | str)` funcionarem como no CPython.

use std::rc::Rc;

use crate::object::{ExtObject, Kw, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

/// O valor é um tipo (classe embutida, de usuário, `None` ou um alias já montado)?
pub fn is_type_like(v: &Value) -> bool {
    match v {
        Value::None | Value::Class(_) => true,
        Value::Builtin(n) => crate::object::is_builtin_type(n) || *n == "object" || *n == "type",
        Value::NativeFn(f) => crate::typeattrs::is_type_name(f.name),
        Value::Ext(e) => matches!(e.type_name(), "GenericAlias" | "UnionType"),
        _ => false,
    }
}

fn type_repr(v: &Value) -> String {
    match v {
        Value::None => "None".to_string(),
        Value::Builtin("Ellipsis") => "...".to_string(),
        Value::Builtin(n) => (*n).to_string(),
        Value::NativeFn(f) => f.name.to_string(),
        Value::Class(c) => match c.lookup("__module__") {
            Some(Value::Str(m)) if m.as_str() != "builtins" => format!("{}.{}", m.as_str(), c.name),
            Some(Value::Str(_)) => c.name.to_string(),
            _ => format!("__main__.{}", c.name),
        },
        Value::List(items) => {
            let parts: Vec<String> = items.borrow().iter().map(type_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        other => crate::object::repr(other),
    }
}

/// `origem[args]`.
pub struct GenericAlias {
    pub(crate) origin: Value,
    args: Vec<Value>,
}

impl GenericAlias {
    pub fn make(origin: Value, key: &Value) -> Value {
        let args = match key {
            Value::Tuple(t) => t.to_vec(),
            other => vec![other.clone()],
        };
        Value::Ext(Rc::new(GenericAlias { origin, args }))
    }
}

impl ExtObject for GenericAlias {
    fn type_name(&self) -> &'static str {
        "GenericAlias"
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn repr(&self) -> String {
        let parts: Vec<String> = self.args.iter().map(type_repr).collect();
        format!("{}[{}]", type_repr(&self.origin).trim_start_matches("__main__."), parts.join(", "))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__origin__" => Some(Ok(self.origin.clone())),
            "__args__" => Some(Ok(Value::tuple(self.args.clone()))),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        vm.call_value(&self.origin, args, kw)
    }
    fn binop(&self, op: &str, other: &Value, reflected: bool) -> Option<PyResult<Value>> {
        if op == "|" && is_type_like(other) {
            let me = Value::Ext(Rc::new(GenericAlias { origin: self.origin.clone(), args: self.args.clone() }));
            return Some(Ok(union(if reflected { other } else { &me }, if reflected { &me } else { other })));
        }
        None
    }
    fn richcmp(&self, op: &str, other: &Value) -> Option<PyResult<bool>> {
        let Value::Ext(o) = other else { return None };
        if o.type_name() != "GenericAlias" || !matches!(op, "==" | "!=") {
            return None;
        }
        let same = o.repr() == self.repr();
        Some(Ok(if op == "==" { same } else { !same }))
    }
}

/// `a | b` entre tipos.
pub struct UnionType {
    args: Vec<Value>,
}

/// A união achatada e sem repetições de `a` e `b`.
pub fn union(a: &Value, b: &Value) -> Value {
    let mut args: Vec<Value> = Vec::new();
    for side in [a, b] {
        let parts = match side {
            Value::Ext(e) if e.type_name() == "UnionType" => union_args(side).unwrap_or_default(),
            other => vec![other.clone()],
        };
        for p in parts {
            if !args.iter().any(|x| type_repr(x) == type_repr(&p)) {
                args.push(p);
            }
        }
    }
    Value::Ext(Rc::new(UnionType { args }))
}

/// Os tipos de uma união (`None` se `v` não é uma).
pub fn union_args(v: &Value) -> Option<Vec<Value>> {
    let Value::Ext(e) = v else { return None };
    if e.type_name() != "UnionType" {
        return None;
    }
    let mut vm = crate::vm::current()?;
    match e.getattr(&mut vm, "__args__") {
        Some(Ok(Value::Tuple(t))) => Some(t.to_vec()),
        _ => None,
    }
}

impl ExtObject for UnionType {
    fn type_name(&self) -> &'static str {
        "UnionType"
    }
    fn repr(&self) -> String {
        let parts: Vec<String> = self.args.iter().map(type_repr).collect();
        parts.join(" | ")
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__args__" => Some(Ok(Value::tuple(self.args.clone()))),
            _ => None,
        }
    }
    fn call_method(&self, _vm: &mut Vm, _name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("TypeError", "'types.UnionType' object is not callable"))
    }
    fn binop(&self, op: &str, other: &Value, reflected: bool) -> Option<PyResult<Value>> {
        if op == "|" && is_type_like(other) {
            let me = Value::Ext(Rc::new(UnionType { args: self.args.clone() }));
            return Some(Ok(union(if reflected { other } else { &me }, if reflected { &me } else { other })));
        }
        None
    }
}

/// `tipo[chave]` para um tipo embutido ou de usuário (com `__class_getitem__`).
pub fn class_getitem(vm: &mut Vm, container: &Value, key: &Value) -> Option<PyResult<Value>> {
    match container {
        Value::Class(c) => {
            if let Some(r) = vm.meta_dunder(c, "__getitem__", vec![key.clone()], Vec::new()) {
                return Some(r);
            }
            match c.lookup("__class_getitem__") {
                Some(Value::Function(f)) => Some(vm.call_function(&f, vec![container.clone(), key.clone()], Vec::new())),
                Some(Value::Ext(e)) => match e.descriptor() {
                    Some(crate::object::Descriptor::Class(Value::Function(f)))
                    | Some(crate::object::Descriptor::Static(Value::Function(f))) => {
                        Some(vm.call_function(&f, vec![container.clone(), key.clone()], Vec::new()))
                    }
                    // `__class_getitem__ = classmethod(GenericAlias)`, o idioma da stdlib.
                    Some(crate::object::Descriptor::Class(Value::Class(g))) if g.name == "GenericAlias" => {
                        Some(Ok(GenericAlias::make(container.clone(), key)))
                    }
                    _ => None,
                },
                _ => Some(Err(type_error(format!("type '{}' is not subscriptable", c.name)))),
            }
        }
        Value::NativeFn(f) if crate::typeattrs::is_type_name(f.name) => Some(Ok(GenericAlias::make(container.clone(), key))),
        Value::Builtin(n) if crate::object::is_builtin_type(n) => Some(Ok(GenericAlias::make(container.clone(), key))),
        _ => None,
    }
}
