//! Classes de usuário: construção (`class`), instâncias, atributos, descritores (`staticmethod`,
//! `classmethod`, `property`), `super()`, o protocolo do `with` e o despacho dos métodos mágicos
//! (`__init__`, `__str__`, `__eq__`, `__add__`...) para o restante da VM.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::compile::Code;
use crate::object::{
    exc_is_subclass, intern, is_builtin_type, to_str, BoundMethod, ClassObj, Descriptor, Env, ExtObject, InstanceObj,
    Kw, Value, EXC_CLASSES,
};
use crate::vm::{current, exc, type_error, PyException, PyResult, Vm};

// ------------------------------------------------------------------------------------------------
// Descritores

struct StaticMethod(Value);

impl ExtObject for StaticMethod {
    fn type_name(&self) -> &'static str {
        "staticmethod"
    }
    fn descriptor(&self) -> Option<Descriptor> {
        Some(Descriptor::Static(self.0.clone()))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("'staticmethod' object has no attribute '{name}'")))
    }
}

struct ClassMethod(Value);

impl ExtObject for ClassMethod {
    fn type_name(&self) -> &'static str {
        "classmethod"
    }
    fn descriptor(&self) -> Option<Descriptor> {
        Some(Descriptor::Class(self.0.clone()))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("'classmethod' object has no attribute '{name}'")))
    }
}

struct Property {
    get: Value,
    set: Option<Value>,
    del: Option<Value>,
}

impl ExtObject for Property {
    fn type_name(&self) -> &'static str {
        "property"
    }
    fn descriptor(&self) -> Option<Descriptor> {
        Some(Descriptor::Property { get: self.get.clone(), set: self.set.clone(), del: self.del.clone() })
    }
    fn methods(&self) -> &'static [&'static str] {
        &["setter", "getter", "deleter"]
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let [f] = <[Value; 1]>::try_from(args).map_err(|a| {
            type_error(format!("{name}() takes exactly one argument ({} given)", a.len()))
        })?;
        let (mut get, mut set, mut del) = (self.get.clone(), self.set.clone(), self.del.clone());
        match name {
            "setter" => set = Some(f),
            "deleter" => del = Some(f),
            _ => get = f,
        }
        Ok(Value::Ext(Rc::new(Property { get, set, del })))
    }
}

/// `exit` de um `with` sobre arquivo: fechar o arquivo.
struct FileExit(Value);

impl ExtObject for FileExit {
    fn type_name(&self) -> &'static str {
        "method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let close = vm.getattr(&self.0, "close")?;
        vm.call_value(&close, Vec::new(), Vec::new())?;
        Ok(Value::Bool(false))
    }
}

/// Resultado de `super()`: procura o atributo nas classes depois de `cls` na ordem de herança.
struct SuperProxy {
    obj: Value,
    cls: Rc<ClassObj>,
}

impl ExtObject for SuperProxy {
    fn type_name(&self) -> &'static str {
        "super"
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let Value::Instance(inst) = &self.obj else { return None };
        let mro = inst.class.mro();
        let start = mro.iter().position(|c| Rc::ptr_eq(c, &self.cls)).map_or(0, |i| i + 1);
        for c in &mro[start..] {
            let attr = c.dict.borrow().get(name).cloned();
            if let Some(attr) = attr {
                return Some(vm.bind_class_attr(&attr, self.obj.clone(), &inst.class));
            }
        }
        // Métodos herdados das classes embutidas (`object`, `Exception`).
        Some(Ok(Value::Ext(Rc::new(BuiltinSuperMethod { obj: self.obj.clone(), name: intern(name) }))))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("'super' object has no attribute '{name}'")))
    }
}

/// Método de `object`/`BaseException` alcançado por `super().nome`.
struct BuiltinSuperMethod {
    obj: Value,
    name: &'static str,
}

impl ExtObject for BuiltinSuperMethod {
    fn type_name(&self) -> &'static str {
        "method-wrapper"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let Value::Instance(inst) = &self.obj else {
            return Err(exc("AttributeError", format!("'super' object has no attribute '{}'", self.name)));
        };
        match self.name {
            "__init__" => {
                if inst.class.builtin_base.is_some() {
                    inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(args));
                }
                Ok(Value::None)
            }
            "__str__" => Ok(Value::str(vm.default_text(&self.obj, true))),
            "__repr__" => Ok(Value::str(vm.default_text(&self.obj, false))),
            "__eq__" => Ok(Value::Bool(args.first().is_some_and(|o| crate::object::is(&self.obj, o)))),
            "__ne__" => Ok(Value::Bool(!args.first().is_some_and(|o| crate::object::is(&self.obj, o)))),
            "__hash__" => Ok(Value::Int(crate::object::hash(&self.obj)?)),
            "__setattr__" => {
                if let [Value::Str(n), v] = args.as_slice() {
                    inst.dict.borrow_mut().insert(n.as_str().to_string(), v.clone());
                }
                Ok(Value::None)
            }
            "__getattribute__" | "__getattr__" => {
                let n = args.first().map(to_str).unwrap_or_default();
                vm.getattr(&self.obj, &n)
            }
            "__new__" => Ok(self.obj.clone()),
            _ => Err(exc("AttributeError", format!("'super' object has no attribute '{}'", self.name))),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Dunders por tabela

/// Nome do método mágico de um operador binário (`__add__`) e do refletido (`__radd__`).
pub(crate) fn binop_dunder(sym: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match sym {
        "+" => ("__add__", "__radd__", "__iadd__"),
        "-" => ("__sub__", "__rsub__", "__isub__"),
        "*" => ("__mul__", "__rmul__", "__imul__"),
        "/" => ("__truediv__", "__rtruediv__", "__itruediv__"),
        "//" => ("__floordiv__", "__rfloordiv__", "__ifloordiv__"),
        "%" => ("__mod__", "__rmod__", "__imod__"),
        "**" => ("__pow__", "__rpow__", "__ipow__"),
        "&" => ("__and__", "__rand__", "__iand__"),
        "|" => ("__or__", "__ror__", "__ior__"),
        "^" => ("__xor__", "__rxor__", "__ixor__"),
        "<<" => ("__lshift__", "__rlshift__", "__ilshift__"),
        ">>" => ("__rshift__", "__rrshift__", "__irshift__"),
        "@" => ("__matmul__", "__rmatmul__", "__imatmul__"),
        _ => return None,
    })
}

fn not_implemented() -> Value {
    Value::Builtin("NotImplemented")
}

pub(crate) fn is_not_implemented(v: &Value) -> bool {
    matches!(v, Value::Builtin("NotImplemented"))
}

impl Vm {
    /// Nome global, ou embutido, ou classe de exceção; `NameError` se não existe.
    pub(crate) fn global_or_builtin(&self, name: &str) -> PyResult<Value> {
        if let Some(v) = self.globals.borrow().get(name) {
            return Ok(v.clone());
        }
        if let Some(v) = crate::builtins::get(name) {
            return Ok(v);
        }
        if let Some(b) = crate::vm::BUILTINS.iter().find(|b| **b == name) {
            return Ok(Value::Builtin(b));
        }
        if let Some((n, _)) = EXC_CLASSES.iter().find(|(n, _)| *n == name) {
            return Ok(Value::Builtin(n));
        }
        match name {
            "object" => Ok(Value::Builtin("object")),
            "NotImplemented" => Ok(not_implemented()),
            "staticmethod" => Ok(Value::Builtin("staticmethod")),
            "classmethod" => Ok(Value::Builtin("classmethod")),
            "property" => Ok(Value::Builtin("property")),
            "super" => Ok(Value::Builtin("super")),
            "type" => Ok(Value::Builtin("type")),
            _ => Err(exc("NameError", format!("name '{name}' is not defined"))),
        }
    }

    /// Executa o corpo de `class` e monta a classe.
    pub(crate) fn build_class(&mut self, body: &Rc<Code>, bases: Vec<Value>, env: &Rc<Env>) -> PyResult<Value> {
        let mut class_bases: Vec<Rc<ClassObj>> = Vec::new();
        let mut builtin_base: Option<&'static str> = None;
        for b in &bases {
            match b {
                Value::Class(c) => {
                    if builtin_base.is_none() {
                        builtin_base = c.builtin_base;
                    }
                    class_bases.push(c.clone());
                }
                Value::Builtin(n) if EXC_CLASSES.iter().any(|(e, _)| e == n) => {
                    if builtin_base.is_none() {
                        builtin_base = EXC_CLASSES.iter().find(|(e, _)| e == n).map(|(e, _)| *e);
                    }
                }
                Value::Builtin("object") => {}
                other => {
                    return Err(type_error(format!(
                        "cannot create a class from base '{}' (builtin base classes are not supported yet)",
                        to_str(other)
                    )))
                }
            }
        }
        let class_env = Env::new(env.capture(), true, false);
        self.exec(body, &class_env)?;
        let dict: BTreeMap<String, Value> =
            class_env.vars.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(Value::Class(Rc::new(ClassObj {
            name: body.name.clone(),
            bases: class_bases,
            builtin_base,
            dict: RefCell::new(dict),
        })))
    }

    /// `Classe(args)`: cria a instância e roda `__init__`.
    pub(crate) fn instantiate(&mut self, cls: &Rc<ClassObj>, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let inst = Rc::new(InstanceObj { class: cls.clone(), dict: RefCell::new(BTreeMap::new()) });
        let obj = Value::Instance(inst.clone());
        if cls.builtin_base.is_some() {
            inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(args.clone()));
        }
        match cls.lookup("__init__") {
            Some(Value::Function(f)) => {
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(obj.clone());
                full.extend(args);
                let r = self.call_function(&f, full, kw)?;
                if !matches!(r, Value::None) {
                    return Err(type_error(format!("__init__() should return None, not '{}'", r.type_name())));
                }
            }
            Some(_) => {}
            None => {
                if cls.builtin_base.is_none() && (!args.is_empty() || !kw.is_empty()) {
                    return Err(type_error(format!("{}() takes no arguments", cls.name)));
                }
            }
        }
        Ok(obj)
    }

    /// Atributo de classe visto de uma instância ou da própria classe: funções viram métodos
    /// presos, descritores são desembrulhados.
    pub(crate) fn bind_class_attr(&mut self, attr: &Value, recv: Value, cls: &Rc<ClassObj>) -> PyResult<Value> {
        match attr {
            Value::Function(f) => match recv {
                Value::Class(_) => Ok(attr.clone()),
                _ => Ok(Value::BoundFn(Rc::new((recv, f.clone())))),
            },
            Value::Ext(e) => match e.descriptor() {
                Some(Descriptor::Static(f)) => Ok(f),
                Some(Descriptor::Class(Value::Function(f))) => {
                    Ok(Value::BoundFn(Rc::new((Value::Class(cls.clone()), f))))
                }
                Some(Descriptor::Class(other)) => Ok(other),
                Some(Descriptor::Property { get, .. }) => match recv {
                    Value::Class(_) => Ok(attr.clone()),
                    _ => self.call_value(&get, vec![recv], Vec::new()),
                },
                None => Ok(attr.clone()),
            },
            other => Ok(other.clone()),
        }
    }

    pub(crate) fn instance_getattr(&mut self, obj: &Value, inst: &Rc<InstanceObj>, name: &str) -> PyResult<Value> {
        match name {
            "__class__" => return Ok(Value::Class(inst.class.clone())),
            "__dict__" => {
                let mut d = crate::object::Dict::new();
                for (k, v) in inst.dict.borrow().iter() {
                    d.set(Value::str(k.clone()), v.clone())?;
                }
                return Ok(Value::dict(d));
            }
            _ => {}
        }
        // Propriedades têm precedência sobre o dicionário da instância.
        let class_attr = inst.class.lookup(name);
        if let Some(Value::Ext(e)) = &class_attr {
            if matches!(e.descriptor(), Some(Descriptor::Property { .. })) {
                return self.bind_class_attr(class_attr.as_ref().unwrap_or(&Value::None), obj.clone(), &inst.class);
            }
        }
        let own = inst.dict.borrow().get(name).cloned();
        if let Some(v) = own {
            return Ok(v);
        }
        if let Some(attr) = class_attr {
            return self.bind_class_attr(&attr, obj.clone(), &inst.class);
        }
        if let Some(Value::Function(f)) = inst.class.lookup("__getattr__") {
            return self.call_function(&f, vec![obj.clone(), Value::str(name)], Vec::new());
        }
        Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", inst.class.name)))
    }

    pub(crate) fn class_getattr(&mut self, cls: &Rc<ClassObj>, name: &str) -> PyResult<Value> {
        match name {
            "__name__" | "__qualname__" => return Ok(Value::str(cls.name.clone())),
            "__module__" => return Ok(Value::str("__main__")),
            "__bases__" => {
                return Ok(Value::tuple(cls.bases.iter().map(|b| Value::Class(b.clone())).collect()));
            }
            "__mro__" => {
                return Ok(Value::tuple(cls.mro().into_iter().map(Value::Class).collect()));
            }
            _ => {}
        }
        if let Some(attr) = cls.lookup(name) {
            return self.bind_class_attr(&attr, Value::Class(cls.clone()), cls);
        }
        Err(exc("AttributeError", format!("type object '{}' has no attribute '{name}'", cls.name)))
    }

    pub(crate) fn store_attr(&mut self, obj: &Value, name: &str, value: Value) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                if let Some(Value::Ext(e)) = inst.class.lookup(name) {
                    if let Some(Descriptor::Property { set, .. }) = e.descriptor() {
                        return match set {
                            Some(f) => self.call_value(&f, vec![obj.clone(), value], Vec::new()).map(|_| ()),
                            None => Err(exc(
                                "AttributeError",
                                format!("property '{name}' of '{}' object has no setter", inst.class.name),
                            )),
                        };
                    }
                }
                if let Some(Value::Function(f)) = inst.class.lookup("__setattr__") {
                    self.call_function(&f, vec![obj.clone(), Value::str(name), value], Vec::new())?;
                    return Ok(());
                }
                inst.dict.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Class(c) => {
                c.dict.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Module(m) => {
                m.attrs.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            _ => Err(exc(
                "AttributeError",
                format!("'{}' object has no attribute '{name}' and no __dict__ for setting new attributes", obj.type_name()),
            )),
        }
    }

    pub(crate) fn delete_attr(&mut self, obj: &Value, name: &str) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                if inst.dict.borrow_mut().remove(name).is_none() {
                    return Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", inst.class.name)));
                }
                Ok(())
            }
            Value::Class(c) => {
                if c.dict.borrow_mut().remove(name).is_none() {
                    return Err(exc("AttributeError", format!("type object '{}' has no attribute '{name}'", c.name)));
                }
                Ok(())
            }
            _ => Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", obj.type_name()))),
        }
    }

    /// `del container[index]`.
    pub(crate) fn delete_subscript(&mut self, container: &Value, index: &Value) -> PyResult<()> {
        match container {
            Value::List(l) => {
                let len = l.borrow().len() as i64;
                match index {
                    Value::Slice(s) => {
                        let (start, stop, step) = crate::vm::slice_bounds(len, s)?;
                        let mut doomed: Vec<usize> = Vec::new();
                        let mut i = start;
                        while (step > 0 && i < stop) || (step < 0 && i > stop) {
                            doomed.push(i as usize);
                            i += step;
                        }
                        doomed.sort_unstable();
                        for i in doomed.into_iter().rev() {
                            l.borrow_mut().remove(i);
                        }
                        Ok(())
                    }
                    Value::Int(_) | Value::Bool(_) => {
                        let i = match index {
                            Value::Int(i) => *i,
                            _ => i64::from(index.is_true()),
                        };
                        let j = if i < 0 { i + len } else { i };
                        if j < 0 || j >= len {
                            return Err(exc("IndexError", "list assignment index out of range"));
                        }
                        l.borrow_mut().remove(j as usize);
                        Ok(())
                    }
                    other => Err(type_error(format!(
                        "list indices must be integers or slices, not {}",
                        other.type_name()
                    ))),
                }
            }
            Value::Dict(d) => {
                if d.borrow_mut().remove(index)?.is_none() {
                    let mut e = exc("KeyError", crate::object::repr(index));
                    e.value = Some(Value::Exception(Rc::new(crate::object::ExcObj {
                        kind: "KeyError",
                        args: vec![index.clone()],
                    })));
                    return Err(e);
                }
                Ok(())
            }
            Value::Instance(inst) => match inst.class.lookup("__delitem__") {
                Some(Value::Function(f)) => {
                    self.call_function(&f, vec![container.clone(), index.clone()], Vec::new())?;
                    Ok(())
                }
                _ => Err(type_error(format!("'{}' object doesn't support item deletion", inst.class.name))),
            },
            other => Err(type_error(format!("'{}' object doesn't support item deletion", other.type_name()))),
        }
    }

    /// Abre um `with`: devolve `(exit, valor do __enter__)`.
    pub(crate) fn with_enter(&mut self, mgr: &Value) -> PyResult<(Value, Value)> {
        let unsupported = || type_error(format!("'{}' object does not support the context manager protocol", mgr.type_name()));
        match mgr {
            Value::Instance(i) => {
                let (Some(enter), Some(exit)) = (i.class.lookup("__enter__"), i.class.lookup("__exit__")) else {
                    return Err(unsupported());
                };
                let entered = match &enter {
                    Value::Function(f) => self.call_function(f, vec![mgr.clone()], Vec::new())?,
                    other => self.call_value(other, vec![mgr.clone()], Vec::new())?,
                };
                let exit = self.bind_class_attr(&exit, mgr.clone(), &i.class)?;
                Ok((exit, entered))
            }
            Value::Native(_) => Ok((Value::Ext(Rc::new(FileExit(mgr.clone()))), mgr.clone())),
            Value::Ext(e) => {
                let has = |n: &str| e.methods().iter().any(|m| *m == n);
                if !(has("__enter__") && has("__exit__")) {
                    return Err(unsupported());
                }
                let exit_name = e.methods().iter().find(|m| **m == "__exit__").copied().unwrap_or("__exit__");
                let entered = e.clone().call_method(self, "__enter__", Vec::new(), Vec::new())?;
                Ok((Value::Bound(Rc::new(BoundMethod { recv: mgr.clone(), name: exit_name })), entered))
            }
            _ => Err(unsupported()),
        }
    }

    /// `type(valor)`.
    pub(crate) fn type_of(&self, v: &Value) -> Value {
        match v {
            Value::Instance(i) => Value::Class(i.class.clone()),
            Value::Exception(e) => Value::Builtin(e.kind),
            Value::Class(_) => Value::Builtin("type"),
            other => {
                let n = other.type_name();
                if is_builtin_type(n) || n == "type" {
                    Value::Builtin(intern(n))
                } else {
                    Value::Builtin(intern(n))
                }
            }
        }
    }

    /// Texto padrão de uma instância sem `__str__`/`__repr__` de usuário.
    pub(crate) fn default_text(&mut self, v: &Value, is_str: bool) -> String {
        let Value::Instance(i) = v else { return to_str(v) };
        if i.class.builtin_base.is_some() {
            let args = match i.dict.borrow().get("args") {
                Some(Value::Tuple(t)) => t.to_vec(),
                _ => Vec::new(),
            };
            if is_str {
                return match args.as_slice() {
                    [] => String::new(),
                    [one] => to_str(one),
                    _ => to_str(&Value::tuple(args)),
                };
            }
            let inner: Vec<String> = args.iter().map(crate::object::repr).collect();
            return format!("{}({})", i.class.name, inner.join(", "));
        }
        format!("<__main__.{} object at {:#x}>", i.class.name, Rc::as_ptr(i) as usize)
    }

    /// `str(v)`, chamando `__str__` (ou `__repr__`) de usuário.
    pub(crate) fn str_of(&mut self, v: &Value) -> PyResult<String> {
        if let Value::Instance(i) = v {
            for name in ["__str__", "__repr__"] {
                if let Some(Value::Function(f)) = i.class.lookup(name) {
                    let r = self.call_function(&f, vec![v.clone()], Vec::new())?;
                    return match r {
                        Value::Str(s) => Ok(s.as_str().to_string()),
                        other => Err(type_error(format!("{name} returned non-string (type {})", other.type_name()))),
                    };
                }
            }
            return Ok(self.default_text(v, true));
        }
        Ok(to_str(v))
    }

    /// `repr(v)`, chamando `__repr__` de usuário.
    pub(crate) fn repr_of(&mut self, v: &Value) -> PyResult<String> {
        if let Value::Instance(i) = v {
            if let Some(Value::Function(f)) = i.class.lookup("__repr__") {
                let r = self.call_function(&f, vec![v.clone()], Vec::new())?;
                return match r {
                    Value::Str(s) => Ok(s.as_str().to_string()),
                    other => Err(type_error(format!("__repr__ returned non-string (type {})", other.type_name()))),
                };
            }
            return Ok(self.default_text(v, false));
        }
        Ok(crate::object::repr(v))
    }

    /// `format(v, spec)`.
    pub(crate) fn format_value(&mut self, v: &Value, spec: &str) -> PyResult<String> {
        if let Value::Instance(i) = v {
            if let Some(Value::Function(f)) = i.class.lookup("__format__") {
                let r = self.call_function(&f, vec![v.clone(), Value::str(spec)], Vec::new())?;
                return Ok(to_str(&r));
            }
            if spec.is_empty() {
                return self.str_of(v);
            }
            return Err(type_error(format!("unsupported format string passed to {}.__format__", i.class.name)));
        }
        crate::format::format_value(v, spec)
    }

    /// `except cls` com a instância do topo: classes de usuário, embutidas ou tupla delas.
    pub(crate) fn exc_matches_value(&mut self, exc_value: &Value, cls: &Value) -> PyResult<bool> {
        match cls {
            Value::Tuple(items) => {
                for it in items.iter() {
                    if self.exc_matches_value(exc_value, it)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::Class(c) => Ok(match exc_value {
                Value::Instance(i) => i.class.mro().iter().any(|k| Rc::ptr_eq(k, c)),
                _ => false,
            }),
            Value::Builtin(name) if EXC_CLASSES.iter().any(|(n, _)| n == name) => {
                let kind = match exc_value {
                    Value::Exception(e) => e.kind,
                    Value::Instance(i) => i.class.builtin_base.unwrap_or(""),
                    _ => "",
                };
                Ok(!kind.is_empty() && exc_is_subclass(kind, name))
            }
            _ => Err(type_error("catching classes that do not inherit from BaseException is not allowed")),
        }
    }

    /// Valor do `raise X`: classe vira instância; instância de classe de usuário é aceita.
    pub(crate) fn raise_any(&mut self, v: Value) -> PyResult<PyException> {
        match &v {
            Value::Class(c) => {
                if c.builtin_base.is_none() {
                    return Err(type_error("exceptions must derive from BaseException"));
                }
                let inst = self.instantiate(c, Vec::new(), Vec::new())?;
                Ok(PyException::from_value(&inst))
            }
            Value::Instance(i) => {
                if i.class.builtin_base.is_none() {
                    return Err(type_error("exceptions must derive from BaseException"));
                }
                Ok(PyException::from_value(&v))
            }
            _ => crate::vm::raise_value(v),
        }
    }

    /// Chama o método mágico `name` de uma instância, se a classe o define.
    pub(crate) fn call_dunder(&mut self, obj: &Value, name: &str, args: Vec<Value>) -> Option<PyResult<Value>> {
        let Value::Instance(i) = obj else { return None };
        let attr = i.class.lookup(name)?;
        let f = match attr {
            Value::Function(f) => f,
            _ => return None,
        };
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(obj.clone());
        full.extend(args);
        Some(self.call_function(&f, full, Vec::new()))
    }

    /// Builtins ligados a classes: `staticmethod`, `classmethod`, `property`, `super`, `type`, `object`.
    pub(crate) fn call_class_builtin(&mut self, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        match name {
            "staticmethod" | "classmethod" => {
                let [f] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("{name} expected 1 argument, got {}", a.len())))?;
                Ok(Value::Ext(if name == "staticmethod" { Rc::new(StaticMethod(f)) } else { Rc::new(ClassMethod(f)) }))
            }
            "property" => {
                let mut it = args.into_iter();
                let get = it.next().unwrap_or(Value::None);
                let set = it.next().filter(|v| !matches!(v, Value::None));
                let del = it.next().filter(|v| !matches!(v, Value::None));
                let mut get = get;
                let (mut set, mut del) = (set, del);
                for (k, v) in kw {
                    match k.as_str() {
                        "fget" => get = v,
                        "fset" => set = Some(v),
                        "fdel" => del = Some(v),
                        _ => return Err(type_error(format!("property() got an unexpected keyword argument '{k}'"))),
                    }
                }
                Ok(Value::Ext(Rc::new(Property { get, set, del })))
            }
            "super" => match args.as_slice() {
                // `super()` sem argumentos vira `super(self, "Classe")` no compilador.
                [obj, Value::Str(cname)] => {
                    let Value::Instance(inst) = obj else {
                        return Err(type_error("super(): __self__ is not an instance"));
                    };
                    let cls = inst
                        .class
                        .mro()
                        .into_iter()
                        .find(|c| c.name == cname.as_str())
                        .ok_or_else(|| exc("RuntimeError", "super(): __class__ cell not found"))?;
                    Ok(Value::Ext(Rc::new(SuperProxy { obj: obj.clone(), cls })))
                }
                [Value::Class(cls), obj] => Ok(Value::Ext(Rc::new(SuperProxy { obj: obj.clone(), cls: cls.clone() }))),
                _ => Err(exc("RuntimeError", "super(): no arguments")),
            },
            "type" => match args.as_slice() {
                [v] => Ok(self.type_of(v)),
                _ => Err(type_error("type() takes 1 argument (class creation with type() is not supported)")),
            },
            "object" => {
                if !args.is_empty() {
                    return Err(type_error("object() takes no arguments"));
                }
                let base = Rc::new(ClassObj {
                    name: "object".to_string(),
                    bases: Vec::new(),
                    builtin_base: None,
                    dict: RefCell::new(BTreeMap::new()),
                });
                Ok(Value::Instance(Rc::new(InstanceObj { class: base, dict: RefCell::new(BTreeMap::new()) })))
            }
            _ => Err(type_error(format!("'{name}' object is not callable"))),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Funções livres chamadas pelo modelo de objetos (`repr`, `==`, `hash`)

/// Texto de uma instância com `__str__`/`__repr__` de usuário, ou de exceção (`args`); `None` se a
/// classe não define nada (o chamador usa o texto padrão).
pub fn instance_text(v: &Value, is_str: bool) -> Option<String> {
    let Value::Instance(i) = v else { return None };
    let mut vm = current()?;
    let names: &[&str] = if is_str { &["__str__", "__repr__"] } else { &["__repr__"] };
    for name in names {
        if let Some(Value::Function(f)) = i.class.lookup(name) {
            return match vm.call_function(&f, vec![v.clone()], Vec::new()) {
                Ok(Value::Str(s)) => Some(s.as_str().to_string()),
                _ => None,
            };
        }
    }
    if i.class.builtin_base.is_some() {
        return Some(vm.default_text(v, is_str));
    }
    None
}

/// `a == b` quando um dos lados é instância com `__eq__`.
pub fn instance_eq(a: &Value, b: &Value) -> Option<bool> {
    let mut vm = current()?;
    for (x, y) in [(a, b), (b, a)] {
        if let Value::Instance(i) = x {
            if let Some(Value::Function(f)) = i.class.lookup("__eq__") {
                if let Ok(r) = vm.call_function(&f, vec![x.clone(), y.clone()], Vec::new()) {
                    if !is_not_implemented(&r) {
                        return Some(r.is_true());
                    }
                }
            }
        }
    }
    None
}

/// `hash(v)` com `__hash__` de usuário.
pub fn instance_hash(v: &Value) -> Option<i64> {
    let Value::Instance(i) = v else { return None };
    let mut vm = current()?;
    if let Some(Value::Function(f)) = i.class.lookup("__hash__") {
        if let Ok(Value::Int(h)) = vm.call_function(&f, vec![v.clone()], Vec::new()) {
            return Some(h);
        }
    }
    None
}

/// `bool(v)` de uma instância: `__bool__`, depois `__len__`, senão verdadeiro.
pub fn instance_truth(v: &Value) -> bool {
    let Some(mut vm) = current() else { return true };
    if let Some(Ok(r)) = vm.call_dunder(v, "__bool__", Vec::new()) {
        return r.is_true();
    }
    if let Some(Ok(Value::Int(n))) = vm.call_dunder(v, "__len__", Vec::new()) {
        return n != 0;
    }
    true
}
