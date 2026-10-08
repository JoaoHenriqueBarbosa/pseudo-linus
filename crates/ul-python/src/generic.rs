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
        Value::NativeFn(f) => crate::typeattrs::TYPES.contains(&f.name),
        Value::Ext(e) => matches!(e.type_name(), "GenericAlias" | "UnionType"),
        // `type X = ...`: o `|` do CPython aceita o `TypeAliasType` dos dois lados.
        v => is_type_alias(v),
    }
}

/// Um `TypeAliasType` (do `_typing` em Python).
fn is_type_alias(v: &Value) -> bool {
    matches!(v, Value::Instance(i) if i.class().name == "TypeAliasType")
}

fn type_repr(v: &Value) -> String {
    match v {
        Value::None => "None".to_string(),
        Value::Builtin("Ellipsis") => "...".to_string(),
        Value::Builtin(n) => (*n).to_string(),
        Value::NativeFn(f) => f.name.to_string(),
        Value::Class(c) => match c.lookup("__module__") {
            Some(Value::Str(m)) if m.as_str() != "builtins" => format!("{}.{}", m.as_str(), c.qualname),
            Some(Value::Str(_)) => c.qualname.to_string(),
            _ => format!("__main__.{}", c.qualname),
        },
        Value::List(items) => {
            let parts: Vec<String> = items.borrow().iter().map(type_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        // `~T`, `+T`, `-T`, `*Ts` e o nome de um `type X = ...`: o `__repr__` do `typing`.
        v if is_type_param(v) || is_type_alias(v) => match crate::vm::current() {
            Some(mut vm) => vm.repr_of(v).unwrap_or_else(|_| crate::object::repr(v)),
            None => crate::object::repr(v),
        },
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

    /// `__parameters__`: as variáveis de tipo livres dos argumentos, na ordem, sem repetição.
    fn parameters(&self) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for a in &self.args {
            let found = if is_type_param(a) { vec![a.clone()] } else { alias_parameters(a) };
            for p in found {
                if !out.iter().any(|x| same_object(x, &p)) {
                    out.push(p);
                }
            }
        }
        out
    }
}

/// `TypeVar`, `ParamSpec` ou `TypeVarTuple` (do `typing` em Python).
fn is_type_param(v: &Value) -> bool {
    matches!(v, Value::Instance(i) if matches!(i.class().name.as_str(), "TypeVar" | "ParamSpec" | "TypeVarTuple"))
}

fn same_object(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Instance(x), Value::Instance(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Os parâmetros de um alias aninhado (`list[_T]` dentro de `dict[str, list[_T]]`).
fn alias_parameters(v: &Value) -> Vec<Value> {
    match v {
        Value::Ext(e) => e.as_any().and_then(|a| a.downcast_ref::<GenericAlias>()).map(|g| g.parameters()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Troca cada parâmetro de `params` pelo argumento correspondente, descendo em aliases aninhados.
fn substitute(v: &Value, params: &[Value], subs: &[Value]) -> Value {
    if is_type_param(v) {
        if let Some(i) = params.iter().position(|p| same_object(p, v)) {
            return subs[i].clone();
        }
        return v.clone();
    }
    if let Value::Ext(e) = v {
        if let Some(g) = e.as_any().and_then(|a| a.downcast_ref::<GenericAlias>()) {
            let args = g.args.iter().map(|a| substitute(a, params, subs)).collect();
            return Value::Ext(Rc::new(GenericAlias { origin: g.origin.clone(), args }));
        }
    }
    v.clone()
}

impl ExtObject for GenericAlias {
    fn type_name(&self) -> &'static str {
        "GenericAlias"
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        let refs = std::iter::once(self.origin.clone()).chain(self.args.iter().cloned()).collect();
        crate::object::OpaqueImage::image("generic_alias", (), refs)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn repr(&self) -> String {
        let parts: Vec<String> = self.args.iter().map(type_repr).collect();
        format!("{}[{}]", type_repr(&self.origin), parts.join(", "))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__origin__" => Some(Ok(self.origin.clone())),
            "__args__" => Some(Ok(Value::tuple(self.args.clone()))),
            "__parameters__" => Some(Ok(Value::tuple(self.parameters()))),
            _ => None,
        }
    }
    /// `dict[_T, None][int]`: substitui os parâmetros livres, como o `_Py_subs_parameters`.
    fn getitem(&self, key: &Value) -> Option<PyResult<Value>> {
        let params = self.parameters();
        if params.is_empty() {
            return Some(Err(type_error(format!("{} is not a generic class", self.repr()))));
        }
        let subs = match key {
            Value::Tuple(t) => t.to_vec(),
            other => vec![other.clone()],
        };
        if subs.len() != params.len() {
            let (which, least) = if subs.len() > params.len() { ("many", "") } else { ("few", "at least ") };
            return Some(Err(type_error(format!(
                "Too {which} arguments for {}; actual {}, expected {least}{}",
                self.repr(),
                subs.len(),
                params.len()
            ))));
        }
        let args = self.args.iter().map(|a| substitute(a, &params, &subs)).collect();
        Some(Ok(Value::Ext(Rc::new(GenericAlias { origin: self.origin.clone(), args }))))
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        vm.call(&self.origin, args, kw)
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

/// Refaz um `GenericAlias` (origem e depois os argumentos) ou uma `UnionType` (só os argumentos) a
/// partir da imagem do heap.
pub(crate) fn restore_image(tag: &str, _state: &(dyn std::any::Any + Send + Sync), refs: Vec<Value>) -> Option<Value> {
    match tag {
        "generic_alias" => {
            let mut refs = refs.into_iter();
            let origin = refs.next()?;
            Some(Value::Ext(Rc::new(GenericAlias { origin, args: refs.collect() })))
        }
        "union_type" => Some(Value::Ext(Rc::new(UnionType { args: refs }))),
        _ => None,
    }
}

impl ExtObject for UnionType {
    fn type_name(&self) -> &'static str {
        "UnionType"
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        crate::object::OpaqueImage::image("union_type", (), self.args.clone())
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
                    Some(crate::object::Descriptor::Class(Value::Builtin("GenericAlias"))) => {
                        Some(Ok(GenericAlias::make(container.clone(), key)))
                    }
                    _ => None,
                },
                // Herdado de uma base embutida que o define (`class Counter(dict)`).
                None if matches!(c.data_base, Some("dict" | "list" | "tuple" | "set" | "frozenset")) => {
                    Some(Ok(GenericAlias::make(container.clone(), key)))
                }
                _ => Some(Err(type_error(format!("type '{}' is not subscriptable", c.name)))),
            }
        }
        Value::NativeFn(f) if crate::typeattrs::TYPES.contains(&f.name) || f.name == "ref" => {
            Some(builtin_class_getitem(f.name, container, key))
        }
        Value::Builtin(n) if crate::object::is_builtin_type(n) => Some(builtin_class_getitem(n, container, key)),
        _ => None,
    }
}

/// Os tipos embutidos do 3.13 que definem `__class_getitem__` (`Py_GenericAlias`), mais o `weakref.ref`
/// (nativo aqui); os demais (`int`, `str`, `range`, as exceções...) recusam como o CPython.
fn builtin_class_getitem(name: &str, container: &Value, key: &Value) -> PyResult<Value> {
    if matches!(
        name,
        "type" | "list" | "dict" | "tuple" | "set" | "frozenset" | "enumerate" | "generator" | "coroutine" | "async_generator" | "ref" | "ReferenceType"
    ) {
        Ok(GenericAlias::make(container.clone(), key))
    } else {
        Err(type_error(format!("type '{name}' is not subscriptable")))
    }
}
