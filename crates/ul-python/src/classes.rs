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

/// O `type_new` acrescenta `__dict__` e `__weakref__` à classe: ela não declara `__slots__`, não é
/// shim de tipo em C e nenhuma base já dá um dicionário às instâncias. As exceções já têm
/// dicionário, então as subclasses delas só ganham `__weakref__`.
fn instance_slots_added(cls: &Rc<ClassObj>) -> &'static [&'static str] {
    if cls.dict.borrow().contains_key("__slots__") {
        return &[];
    }
    if matches!(cls.dict.borrow().get("__module__"), Some(Value::Str(m))
        if m.as_str() == "builtins" || crate::object::BUILTIN_MODULES.contains(&m.as_str()))
    {
        return &[];
    }
    for base in cls.mro().into_iter().skip(1) {
        if !base.dict.borrow().contains_key("__slots__") {
            return &[];
        }
    }
    if cls.mro().iter().any(|c| c.builtin_base.is_some_and(|b| EXC_CLASSES.iter().any(|(n, _)| *n == b))) {
        return &["__weakref__"];
    }
    &["__dict__", "__weakref__"]
}

/// `type(obj).nome` de um objeto nativo: o método sem receptor (`method_descriptor`), que chamado
/// com o objeto na frente repassa a chamada a ele.
pub(crate) struct NativeTypeMethod {
    pub(crate) owner: &'static str,
    pub(crate) name: &'static str,
}

impl ExtObject for NativeTypeMethod {
    fn type_name(&self) -> &'static str {
        "method_descriptor"
    }
    fn repr(&self) -> String {
        format!("<method '{}' of '{}' objects>", self.name, self.owner)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" => Some(Ok(Value::str(self.name))),
            "__qualname__" => Some(Ok(Value::str(format!("{}.{}", self.owner, self.name)))),
            "__objclass__" => Some(Ok(Value::Builtin(self.owner))),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, mut args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        if name != "__call__" {
            return Err(exc("AttributeError", format!("'method_descriptor' object has no attribute '{name}'")));
        }
        if args.is_empty() {
            return Err(type_error(format!("unbound method {}.{}() needs an argument", self.owner, self.name)));
        }
        let recv = args.remove(0);
        match &recv {
            Value::Ext(e) if e.type_name() == self.owner => e.clone().call_method(vm, self.name, args, kw),
            other => Err(type_error(format!(
                "descriptor '{}' for '{}' objects doesn't apply to a '{}' object",
                self.name,
                self.owner,
                other.type_name()
            ))),
        }
    }
}

/// `getset_descriptor` dos atributos `__dict__` e `__weakref__` que o `type` põe na classe.
struct GetSetDescriptor {
    name: &'static str,
    owner: Rc<ClassObj>,
}

impl ExtObject for GetSetDescriptor {
    fn type_name(&self) -> &'static str {
        "getset_descriptor"
    }
    fn repr(&self) -> String {
        format!("<attribute '{}' of '{}' objects>", self.name, self.owner.name)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__get__", "__set__", "__delete__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" | "__qualname__" => Some(Ok(Value::str(if name == "__name__" {
                self.name.to_string()
            } else {
                format!("{}.{}", self.owner.name, self.name)
            }))),
            "__objclass__" => Some(Ok(Value::Class(self.owner.clone()))),
            "__doc__" => Some(Ok(Value::str(if self.name == "__dict__" {
                "dictionary for instance variables"
            } else {
                "list of weak references to the object"
            }))),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match (name, args.as_slice()) {
            ("__get__", [Value::None, ..]) => Ok(Value::Ext(Rc::new(GetSetDescriptor {
                name: self.name,
                owner: self.owner.clone(),
            }))),
            ("__get__", [obj, ..]) if self.name == "__dict__" => vm.getattr(obj, "__dict__"),
            ("__get__", [_, ..]) => Ok(Value::None),
            ("__set__", [obj, value]) if self.name == "__dict__" => {
                vm.store_attr(obj, "__dict__", value.clone())?;
                Ok(Value::None)
            }
            ("__set__" | "__delete__", _) => Err(exc("AttributeError", format!("attribute '{}' of '{}' objects is not writable", self.name, self.owner.name))),
            _ => Err(type_error(format!("expected at least 1 argument, got {}", args.len()))),
        }
    }
}

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
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        // Desde o 3.10 o `staticmethod` é chamável e repassa a chamada à função envolvida.
        if name == "__call__" {
            return vm.call_value(&self.0, args, kw);
        }
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
    doc: RefCell<Value>,
}

impl Property {
    fn new(get: Value, set: Option<Value>, del: Option<Value>) -> Property {
        Property { get, set, del, doc: RefCell::new(Value::None) }
    }
}

impl ExtObject for Property {
    fn type_name(&self) -> &'static str {
        "property"
    }
    fn descriptor(&self) -> Option<Descriptor> {
        Some(Descriptor::Property { get: self.get.clone(), set: self.set.clone(), del: self.del.clone() })
    }
    fn methods(&self) -> &'static [&'static str] {
        &["setter", "getter", "deleter", "__get__", "__set__", "__delete__"]
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__doc__" => {
                // Sem `doc=`, o docstring vem do getter, como no CPython.
                let own = self.doc.borrow().clone();
                if matches!(own, Value::None) {
                    return Some(Ok(vm.getattr(&self.get, "__doc__").unwrap_or(Value::None)));
                }
                Some(Ok(own))
            }
            "fget" => Some(Ok(self.get.clone())),
            "fset" => Some(Ok(self.set.clone().unwrap_or(Value::None))),
            "fdel" => Some(Ok(self.del.clone().unwrap_or(Value::None))),
            _ => None,
        }
    }
    fn setattr(&self, name: &str, value: Value) -> Option<PyResult<()>> {
        if name == "__doc__" {
            *self.doc.borrow_mut() = value;
            return Some(Ok(()));
        }
        None
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        // Chamadas diretas ao protocolo de descritor (`prop.__set__(obj, v)`), como o `ssl.py` faz.
        match name {
            "__get__" => {
                let (obj, rest) = match args.split_first() {
                    Some((o, r)) if r.len() <= 1 => (o.clone(), r.len()),
                    _ => return Err(type_error(format!("expected 1 or 2 arguments, got {}", args.len()))),
                };
                let _ = rest;
                if matches!(obj, Value::None) {
                    return Ok(Value::Ext(Rc::new(Property {
                        get: self.get.clone(),
                        set: self.set.clone(),
                        del: self.del.clone(),
                        doc: RefCell::new(self.doc.borrow().clone()),
                    })));
                }
                if matches!(self.get, Value::None) {
                    return Err(exc("AttributeError", "property has no getter"));
                }
                return vm.call_value(&self.get, vec![obj], kw);
            }
            "__set__" => {
                let [obj, value] = <[Value; 2]>::try_from(args)
                    .map_err(|a| type_error(format!("expected 2 arguments, got {}", a.len())))?;
                let Some(set) = &self.set else { return Err(exc("AttributeError", "property has no setter")) };
                vm.call_value(set, vec![obj, value], kw)?;
                return Ok(Value::None);
            }
            "__delete__" => {
                let [obj] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("expected 1 argument, got {}", a.len())))?;
                let Some(del) = &self.del else { return Err(exc("AttributeError", "property has no deleter")) };
                vm.call_value(del, vec![obj], kw)?;
                return Ok(Value::None);
            }
            _ => {}
        }
        let [f] = <[Value; 1]>::try_from(args).map_err(|a| {
            type_error(format!("{name}() takes exactly one argument ({} given)", a.len()))
        })?;
        let (mut get, mut set, mut del) = (self.get.clone(), self.set.clone(), self.del.clone());
        match name {
            "setter" => set = Some(f),
            "deleter" => del = Some(f),
            _ => get = f,
        }
        Ok(Value::Ext(Rc::new(Property::new(get, set, del))))
    }
}

/// `slice.indices(len)`: `(start, stop, step)` ajustados a um comprimento, como `PySlice_AdjustIndices`.
pub(crate) struct SliceIndices(pub Rc<(Value, Value, Value)>);

impl ExtObject for SliceIndices {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, _vm: &mut Vm, _name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let [len] = <[Value; 1]>::try_from(args)
            .map_err(|a| type_error(format!("indices() takes exactly one argument ({} given)", a.len())))?;
        let length = crate::native_util::want_int(&len)?;
        if length < 0 {
            return Err(exc("ValueError", "length should not be negative"));
        }
        let part = |v: &Value| -> PyResult<Option<i64>> {
            match v {
                Value::None => Ok(None),
                other => Ok(Some(crate::native_util::want_int(other)?)),
            }
        };
        let step = part(&self.0 .2)?.unwrap_or(1);
        if step == 0 {
            return Err(exc("ValueError", "slice step cannot be zero"));
        }
        let (lower, upper) = if step > 0 { (0, length) } else { (-1, length - 1) };
        let clamp = |v: Option<i64>, default: i64| match v {
            None => default,
            Some(mut x) => {
                if x < 0 {
                    x += length;
                    if x < lower {
                        x = lower;
                    }
                } else if x > upper {
                    x = upper;
                }
                x
            }
        };
        let start = clamp(part(&self.0 .0)?, if step < 0 { upper } else { lower });
        let stop = clamp(part(&self.0 .1)?, if step < 0 { lower } else { upper });
        Ok(Value::tuple(vec![Value::Int(start), Value::Int(stop), Value::Int(step)]))
    }
}

/// `Classe.__subclasses__`: chamável que devolve a lista das subclasses diretas vivas.
struct SubclassesCall(Vec<Value>);

impl ExtObject for SubclassesCall {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, _vm: &mut Vm, _name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        if !args.is_empty() {
            return Err(type_error(format!("__subclasses__() takes no arguments ({} given)", args.len())));
        }
        Ok(Value::list(self.0.clone()))
    }
}

/// `D.fromkeys(...)` numa subclasse de tipo embutido: o construtor do tipo base, convertido para
/// a subclasse (`cls(resultado)`), como o CPython faz.
struct AltCtor {
    cls: Value,
    inner: Value,
}

impl ExtObject for AltCtor {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let base = vm.call_value(&self.inner, args, kw)?;
        vm.call_value(&self.cls, vec![base], Vec::new())
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

/// `object()`: a instância do tipo `object` em si, sem `__dict__` e com igualdade e hash por identidade.
struct PlainObject;

impl ExtObject for PlainObject {
    fn type_name(&self) -> &'static str {
        "object"
    }
    fn repr(&self) -> String {
        format!("<object object at 0x{:x}>", crate::object::py_addr(self as *const PlainObject as usize))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__eq__", "__ne__", "__hash__", "__repr__", "__str__", "__format__", "__sizeof__", "__getstate__", "__init__"]
    }
    fn setattr(&self, name: &str, _value: Value) -> Option<PyResult<()>> {
        Some(Err(exc(
            "AttributeError",
            format!("'object' object has no attribute '{name}' and no __dict__ for setting new attributes"),
        )))
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let me = self as *const PlainObject as usize;
        let same = |v: &Value| matches!(v, Value::Ext(e) if Rc::as_ptr(e) as *const () as usize == me);
        match (name, args.as_slice()) {
            ("__eq__", [other]) => Ok(if same(other) { Value::Bool(true) } else { Value::Builtin("NotImplemented") }),
            ("__ne__", [other]) => Ok(if same(other) { Value::Bool(false) } else { Value::Builtin("NotImplemented") }),
            ("__hash__", []) => Ok(Value::Int(crate::object::py_addr_hash(me))),
            ("__repr__" | "__str__", []) => Ok(Value::str(self.repr())),
            ("__format__", [Value::Str(spec)]) if spec.as_str().is_empty() => Ok(Value::str(self.repr())),
            ("__format__", [_]) => Err(type_error("unsupported format string passed to object.__format__")),
            ("__sizeof__", []) => Ok(Value::Int(16)),
            ("__init__", []) => Ok(Value::None),
            ("__getstate__", []) => Ok(Value::None),
            _ => Err(exc("AttributeError", format!("'object' object has no attribute '{name}'"))),
        }
    }
    fn hash_value(&self) -> Option<i64> {
        Some(crate::object::py_addr_hash(self as *const PlainObject as usize))
    }
}

/// A célula `__class__` que o corpo de uma classe passa em `__classcell__`: o escopo que os métodos
/// capturam, onde `type.__new__` grava a classe criada.
struct ClassCell(Rc<Env>);

impl ExtObject for ClassCell {
    fn type_name(&self) -> &'static str {
        "cell"
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn repr(&self) -> String {
        match self.0.vars.borrow().get("__class__") {
            Some(Value::Class(c)) => format!(
                "<cell at 0x{:x}: type object at 0x{:x}>",
                crate::object::py_addr(Rc::as_ptr(&self.0) as usize),
                crate::object::py_type_addr(Rc::as_ptr(c) as usize)
            ),
            _ => format!("<cell at 0x{:x}: empty>", crate::object::py_addr(Rc::as_ptr(&self.0) as usize)),
        }
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("'cell' object has no attribute '{name}'")))
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
    fn repr(&self) -> String {
        // Como o `super_repr` do CPython: a classe e o nome do tipo do receptor.
        let recv = match &self.obj {
            Value::Instance(i) => i.class.name.clone(),
            Value::Class(c) => c.meta.as_ref().map_or_else(|| "type".to_string(), |m| m.name.clone()),
            other => other.type_name().to_string(),
        };
        format!("<super: <class '{}'>, <{recv} object>>", self.cls.name)
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        if let Value::Class(recv) = &self.obj {
            // Receptor é uma classe (`__init_subclass__`, `__new__` de metaclasse, classmethods).
            let mut mro = recv.mro();
            // Método de metaclasse chamando `super()`: o MRO que vale é o da metaclasse.
            if !mro.iter().any(|c| Rc::ptr_eq(c, &self.cls)) {
                if let Some(m) = &recv.meta {
                    if m.mro().iter().any(|c| Rc::ptr_eq(c, &self.cls)) {
                        mro = m.mro();
                    }
                }
            }
            let start = mro.iter().position(|c| Rc::ptr_eq(c, &self.cls)).map_or(0, |i| i + 1);
            for c in &mro[start..] {
                let attr = c.dict.borrow().get(name).cloned();
                if let Some(attr) = attr {
                    return Some(match (&attr, name) {
                        (Value::Function(f), "__init_subclass__") => {
                            Ok(Value::BoundFn(Rc::new((self.obj.clone(), f.clone()))))
                        }
                        _ => vm.bind_class_attr(&attr, self.obj.clone(), recv),
                    });
                }
            }
            return Some(Ok(Value::Ext(Rc::new(BuiltinSuperMethod { obj: self.obj.clone(), name: intern(name) }))));
        }
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

/// `exc.with_traceback(tb)`: grava `__traceback__` e devolve a própria exceção.
pub(crate) struct ExcWithTraceback {
    pub(crate) obj: Value,
}

impl ExtObject for ExcWithTraceback {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, _vm: &mut Vm, _name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let [tb] = <[Value; 1]>::try_from(args)
            .map_err(|a| type_error(format!("with_traceback() takes exactly one argument ({} given)", a.len())))?;
        match &self.obj {
            Value::Exception(e) => *e.traceback.borrow_mut() = Some(tb),
            Value::Instance(i) => {
                i.dict.borrow_mut().insert("__traceback__".to_string(), tb);
            }
            _ => {}
        }
        Ok(self.obj.clone())
    }
}

/// `exc.add_note(texto)`: anexa a `exc.__notes__`, criando a lista na primeira nota.
pub(crate) struct ExcAddNote {
    pub(crate) obj: Value,
}

impl ExtObject for ExcAddNote {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let [note] = <[Value; 1]>::try_from(args)
            .map_err(|a| type_error(format!("BaseException.add_note() takes exactly one argument ({} given)", a.len())))?;
        if !matches!(note, Value::Str(_)) {
            return Err(type_error(format!("note must be a str, not '{}'", note.type_name())));
        }
        let notes = match vm.getattr(&self.obj, "__notes__") {
            Ok(Value::List(l)) => l,
            Ok(_) => return Err(type_error("Cannot add note: __notes__ is not a list")),
            Err(_) => {
                let l = Value::list(Vec::new());
                vm.store_attr(&self.obj, "__notes__", l.clone())?;
                match l {
                    Value::List(l) => l,
                    _ => unreachable!(),
                }
            }
        };
        notes.borrow_mut().push(note);
        Ok(Value::None)
    }
}

/// Método mágico de uma instância (`d.__getitem__`) que passa pelo despacho da classe.
struct InstanceDunder {
    obj: Value,
    name: &'static str,
}

impl ExtObject for InstanceDunder {
    fn type_name(&self) -> &'static str {
        "method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match vm.call_dunder(&self.obj, self.name, args) {
            Some(r) => r,
            None => Err(exc("AttributeError", format!("object has no attribute '{}'", self.name))),
        }
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
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        if let Value::Class(_) = &self.obj {
            return match self.name {
                "__init_subclass__" => {
                    if kw.is_empty() {
                        Ok(Value::None)
                    } else {
                        Err(type_error("__init_subclass__() takes no keyword arguments"))
                    }
                }
                // `super().__new__(mcs, nome, bases, ns)` de uma metaclasse: é o `type.__new__`.
                "__new__" => match args.as_slice() {
                    [Value::Class(meta), Value::Str(n), Value::Tuple(bases), ns] if meta.is_meta => {
                        let ns = dict_to_ns(ns)?;
                        let c = vm.create_class(n.as_str().to_string(), bases, ns, Some(meta.clone()), kw)?;
                        Ok(Value::Class(c))
                    }
                    [Value::Class(c), rest @ ..] => {
                        let payload = match c.data_base {
                            Some("module") => None,
                            Some(t) => Some(vm.call(&data_ctor(t), rest.iter().map(crate::vm::unwrap_payload).collect(), kw)?),
                            None => None,
                        };
                        Ok(Value::Instance(Rc::new(InstanceObj {
                            class: c.clone(),
                            view: Default::default(),
                            dict: RefCell::new(Default::default()),
                            payload: RefCell::new(payload),
                        })))
                    }
                    _ => Err(type_error("type.__new__() takes exactly 3 arguments")),
                },
                "__init__" => Ok(Value::None),
                // `super().__call__(...)` de uma metaclasse: instancia a classe normalmente.
                "__call__" => match &self.obj {
                    Value::Class(c) => vm.instantiate_default(c, args, kw),
                    _ => Err(type_error("type.__call__() needs a class")),
                },
                "__call__x" => match args.as_slice() {
                    [Value::Class(c), rest @ ..] => vm.instantiate_default(c, rest.to_vec(), kw),
                    _ => Err(type_error("type.__call__() needs a class")),
                },
                _ => Err(exc("AttributeError", format!("'super' object has no attribute '{}'", self.name))),
            };
        }
        let Value::Instance(inst) = &self.obj else {
            return Err(exc("AttributeError", format!("'super' object has no attribute '{}'", self.name)));
        };
        match self.name {
            "__init__" => {
                if inst.class.builtin_base.is_some() {
                    inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(args));
                    return Ok(Value::None);
                }
                if inst.class.data_base == Some("module") {
                    init_module_fields(inst, &args, &kw)?;
                    return Ok(Value::None);
                }
                // `super().__init__(...)` de subclasse de `dict`/`list`/`set`: preenche o valor embutido.
                let payload = inst.payload.borrow().clone();
                if let Some(p @ (Value::Dict(_) | Value::List(_) | Value::Set(_))) = payload {
                    let fresh = vm.call(&data_ctor(inst.class.data_base.unwrap_or("dict")), args, kw)?;
                    match (&p, &fresh) {
                        (Value::Dict(d), Value::Dict(f)) => *d.borrow_mut() = f.borrow().clone(),
                        (Value::List(l), Value::List(f)) => *l.borrow_mut() = f.borrow().clone(),
                        (Value::Set(s), Value::Set(f)) => *s.borrow_mut() = f.borrow().clone(),
                        _ => {}
                    }
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
            "__getattribute__" => {
                // `object.__getattribute__` não passa pelo `__getattr__` da classe.
                let n = args.first().map(to_str).unwrap_or_default();
                let own = inst.dict.borrow().get(n.as_str()).cloned();
                match own {
                    Some(v) => Ok(v),
                    None => vm.instance_getattr_plain(&self.obj, inst, &n),
                }
            }
            // `object` não define `__getattr__`: quem chama via `super()` recebe `AttributeError`.
            "__getattr__" => Err(exc("AttributeError", "'super' object has no attribute '__getattr__'")),
            "__new__" => Ok(self.obj.clone()),
            other => {
                // Método herdado de `dict`/`list`/`str`...: age sobre o valor embutido.
                let payload = inst.payload.borrow().clone();
                if let Some(p) = payload {
                    if let Some(r) = crate::vm::payload_dunder(&p, other, args.clone()) {
                        return r;
                    }
                    let method = vm.getattr(&p, other)?;
                    return vm.call(&method, args, kw);
                }
                Err(exc("AttributeError", format!("'super' object has no attribute '{}'", self.name)))
            }
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

pub(crate) fn not_implemented() -> Value {
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
            "IOError" | "EnvironmentError" => Ok(Value::Builtin("OSError")),
            "object" => Ok(Value::Builtin("object")),
            "NotImplemented" => Ok(not_implemented()),
            "Ellipsis" => Ok(Value::Builtin("Ellipsis")),
            "__debug__" => Ok(Value::Bool(crate::OPTIMIZE.load(std::sync::atomic::Ordering::Relaxed) == 0)),
            "staticmethod" => Ok(Value::Builtin("staticmethod")),
            "classmethod" => Ok(Value::Builtin("classmethod")),
            "property" => Ok(Value::Builtin("property")),
            "super" => Ok(Value::Builtin("super")),
            "type" => Ok(Value::Builtin("type")),
            // `complex` é uma classe em Python (`modules/py/_complex.py`), carregada na primeira vez.
            "complex" => {
                let mut vm = self.clone();
                crate::modules::import(&mut vm, "_complex")
                    .and_then(|m| m.attrs.borrow().get("complex").cloned())
                    .ok_or_else(|| exc("NameError", "name 'complex' is not defined"))
            }
            // `memoryview` é uma classe em Python (`modules/py/_memoryview.py`).
            "memoryview" => {
                let mut vm = self.clone();
                crate::modules::import(&mut vm, "_memoryview")
                    .and_then(|m| m.attrs.borrow().get("memoryview").cloned())
                    .ok_or_else(|| exc("NameError", "name 'memoryview' is not defined"))
            }
            // Grupos de exceções e o auxiliar de `except*` (`modules/py/_excgroup.py`).
            "ExceptionGroup" | "BaseExceptionGroup" | "_eg_split" => {
                let mut vm = self.clone();
                crate::modules::import(&mut vm, "_excgroup")
                    .and_then(|m| m.attrs.borrow().get(name).cloned())
                    .ok_or_else(|| exc("NameError", format!("name '{name}' is not defined")))
            }
            // `__builtins__`: o módulo `builtins` (que também aceita `__builtins__['nome']`).
            "__builtins__" => {
                let mut vm = self.clone();
                crate::modules::import(&mut vm, "builtins")
                    .map(Value::Module)
                    .ok_or_else(|| exc("NameError", format!("name '{name}' is not defined")))
            }
            // Auxiliares da instrução `match` (`modules/py/_match.py`).
            n if n.starts_with("_match_") => {
                let mut vm = self.clone();
                crate::modules::import(&mut vm, "_match")
                    .and_then(|m| m.attrs.borrow().get(n).cloned())
                    .ok_or_else(|| exc("NameError", format!("name '{name}' is not defined")))
            }
            _ => Err(exc("NameError", format!("name '{name}' is not defined"))),
        }
    }

    /// Executa o corpo de `class` e monta a classe (com metaclasse, se houver).
    pub(crate) fn build_class(
        &mut self,
        body: &Rc<Code>,
        bases: Vec<Value>,
        mut kw: Kw,
        env: &Rc<Env>,
    ) -> PyResult<Value> {
        let explicit_meta = kw.iter().position(|(k, _)| k == "metaclass").map(|i| kw.remove(i).1);
        // `__mro_entries__` (`class Box(Generic[T])`): o objeto-base escolhe as bases reais.
        let mut expanded: Vec<Value> = Vec::with_capacity(bases.len());
        let mut any_entries = false;
        for b in &bases {
            if let Value::Instance(i) = b {
                if let Some(Value::Function(f)) = i.class.lookup("__mro_entries__") {
                    any_entries = true;
                    match self.call_function(&f, vec![b.clone(), Value::tuple(bases.clone())], Vec::new())? {
                        Value::Tuple(t) => expanded.extend(t.iter().cloned()),
                        _ => return Err(type_error("__mro_entries__ must return a tuple")),
                    }
                    continue;
                }
            }
            // `class X(list[Any])`: o `types.GenericAlias` entra na MRO como a sua origem.
            if let Value::Ext(e) = b {
                if let Some(g) = e.as_any().and_then(|a| a.downcast_ref::<crate::generic::GenericAlias>()) {
                    any_entries = true;
                    expanded.push(g.origin.clone());
                    continue;
                }
            }
            expanded.push(b.clone());
        }
        let orig_bases = (expanded.len() != bases.len() || any_entries).then(|| Value::tuple(bases.clone()));
        let bases = expanded;
        // Como no CPython, os métodos enxergam a classe pela variável livre `__class__` (é dela que o
        // `super()` sem argumentos tira a classe): um escopo só com ela fica entre o corpo e o de fora,
        // e `type.__new__` a preenche ao receber `__classcell__`.
        let cell_env = Env::new(env.capture(), false, false);
        let class_env = Env::new(Some(cell_env.clone()), true, false);
        self.exec(body, &class_env)?;
        let mut ns = namespace_of(&class_env);
        if body.uses_class_cell {
            ns.push(("__classcell__".to_string(), Value::Ext(Rc::new(ClassCell(cell_env)))));
        }
        if let Some(orig) = orig_bases {
            ns.push(("__orig_bases__".to_string(), orig));
        }
        if !ns.iter().any(|(k, _)| k == "__module__") {
            let module = self.globals.borrow().get("__name__").cloned().unwrap_or_else(|| Value::str("__main__"));
            ns.insert(0, ("__module__".to_string(), module));
        }
        let explicit = match explicit_meta {
            Some(Value::Class(m)) => Some(m),
            Some(Value::Builtin("type")) | None => None,
            Some(other) => {
                return Err(type_error(format!("metaclass must be a class, not '{}'", other.type_name())))
            }
        };
        let meta = explicit.or_else(|| {
            bases.iter().find_map(|b| match b {
                Value::Class(c) => c.meta.clone(),
                _ => None,
            })
        });
        let name = body.name.clone();
        if let Some(m) = &meta {
            let ns_dict = ns_to_dict(&ns)?;
            let cls = match m.lookup("__new__") {
                Some(Value::Function(new)) => {
                    let args = vec![Value::Class(m.clone()), Value::str(name.clone()), Value::tuple(bases.clone()), ns_dict.clone()];
                    self.call_function(&new, args, kw.clone())?
                }
                _ => Value::Class(self.create_class(name.clone(), &bases, ns, Some(m.clone()), kw.clone())?),
            };
            if let (Value::Class(c), Some(Value::Function(init))) = (&cls, m.lookup("__init__")) {
                if c.meta.as_ref().is_some_and(|cm| Rc::ptr_eq(cm, m)) {
                    let args = vec![cls.clone(), Value::str(name), Value::tuple(bases), ns_dict];
                    self.call_function(&init, args, kw)?;
                }
            }
            return Ok(cls);
        }
        Ok(Value::Class(self.create_class(name, &bases, ns, None, kw)?))
    }

    /// `type.__new__`: monta a classe a partir do nome, das bases e do espaço de nomes, depois roda
    /// `__set_name__` dos atributos e o `__init_subclass__` da base.
    pub(crate) fn create_class(
        &mut self,
        name: String,
        bases: &[Value],
        ns: Vec<(String, Value)>,
        meta: Option<Rc<ClassObj>>,
        kw: Kw,
    ) -> PyResult<Rc<ClassObj>> {
        let info = resolve_bases(bases)?;
        let meta = meta.or_else(|| info.classes.iter().find_map(|c| c.meta.clone()));
        let named: Vec<(String, Value)> =
            ns.iter().filter(|(_, v)| matches!(v, Value::Instance(_))).cloned().collect();
        let mut ns = ns;
        // `__classcell__` não vira atributo: a célula recebe a classe criada.
        let cell = ns.iter().position(|(k, _)| k == "__classcell__").map(|i| ns.remove(i).1);
        // Como `type.__new__`: o `__qualname__` do corpo sai do espaço de nomes e vira atributo do tipo.
        let qualname = match ns.iter().position(|(k, _)| k == "__qualname__") {
            Some(i) => match ns.remove(i).1 {
                Value::Str(s) => s.as_str().to_string(),
                _ => name.clone(),
            },
            None => name.clone(),
        };
        let cls = Rc::new(ClassObj {
            qualname,
            name,
            bases: info.classes,
            builtin_base: info.builtin_base,
            data_base: info.data_base,
            meta,
            is_meta: info.derives_type,
            dict: RefCell::new(ns.into_iter().collect()),
            subclasses: RefCell::new(Vec::new()),
        });
        for base in &cls.bases {
            base.subclasses.borrow_mut().push(Rc::downgrade(&cls));
        }
        let owner = Value::Class(cls.clone());
        if let Some(cell) = cell {
            let target = match &cell {
                Value::Ext(e) => e.as_any().and_then(|a| a.downcast_ref::<ClassCell>()).map(|c| c.0.clone()),
                _ => None,
            };
            match target {
                Some(env) => env.set("__class__", owner.clone()),
                None => {
                    let ty = crate::object::repr(&self.type_of(&cell));
                    return Err(type_error(format!("__classcell__ must be a nonlocal cell, not {ty}")));
                }
            }
        }
        for (k, v) in named {
            if let Value::Instance(i) = &v {
                if i.class.lookup("__set_name__").is_some() {
                    if let Some(r) = self.call_dunder(&v, "__set_name__", vec![owner.clone(), Value::str(k)]) {
                        r?;
                    }
                }
            }
        }
        let mro = cls.mro();
        let hook = mro[1..].iter().find_map(|c| c.dict.borrow().get("__init_subclass__").cloned());
        match hook {
            Some(Value::Function(f)) => {
                self.call_function(&f, vec![owner], kw)?;
            }
            Some(Value::Ext(e)) => {
                if let Some(Descriptor::Class(Value::Function(f))) = e.descriptor() {
                    self.call_function(&f, vec![owner], kw)?;
                }
            }
            _ => {
                if let Some((k, _)) = kw.first() {
                    return Err(type_error(format!(
                        "{}.__init_subclass__() takes no keyword arguments",
                        cls.name
                    )));
                    #[allow(unreachable_code)]
                    {
                        let _ = k;
                    }
                }
            }
        }
        Ok(cls)
    }

    /// Recusa instanciar uma classe derivada de `ABC` que ainda tem métodos abstratos.
    fn check_abstract(&mut self, cls: &Rc<ClassObj>) -> PyResult<()> {
        let mro = cls.mro();
        if !mro.iter().any(|c| c.dict.borrow().contains_key("__abstract_base__")) {
            return Ok(());
        }
        let is_abstract = |v: &Value| match v {
            Value::Function(f) => f.attrs.borrow().contains_key("__isabstractmethod__"),
            Value::Ext(e) => match e.descriptor() {
                Some(Descriptor::Static(Value::Function(f))) | Some(Descriptor::Class(Value::Function(f))) => {
                    f.attrs.borrow().contains_key("__isabstractmethod__")
                }
                Some(Descriptor::Property { get: Value::Function(f), .. }) => {
                    f.attrs.borrow().contains_key("__isabstractmethod__")
                }
                _ => false,
            },
            _ => false,
        };
        let mut names: Vec<String> = Vec::new();
        for c in mro.iter().rev() {
            for (k, v) in c.dict.borrow().iter() {
                if is_abstract(v) && !names.contains(k) {
                    names.push(k.clone());
                }
            }
        }
        let missing: Vec<String> = names
            .into_iter()
            .filter(|n| cls.lookup(n).is_some_and(|v| is_abstract(&v)))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let quoted: Vec<String> = missing.iter().map(|n| format!("'{n}'")).collect();
        let (noun, list) = if missing.len() == 1 { ("method", quoted[0].clone()) } else { ("methods", quoted.join(", ")) };
        Err(type_error(format!(
            "Can't instantiate abstract class {} without an implementation for abstract {noun} {list}",
            cls.name
        )))
    }

    /// Método mágico definido na metaclasse de `cls` (`__call__`, `__iter__`, `__getitem__`...).
    pub(crate) fn meta_dunder(&mut self, cls: &Rc<ClassObj>, name: &str, args: Vec<Value>, kw: Kw) -> Option<PyResult<Value>> {
        let meta = cls.meta.as_ref()?;
        let Some(Value::Function(f)) = meta.lookup(name) else { return None };
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(Value::Class(cls.clone()));
        full.extend(args);
        Some(self.call_function(&f, full, kw))
    }

    /// `Classe(args)`: respeita o `__call__` da metaclasse, senão cria a instância.
    pub(crate) fn instantiate(&mut self, cls: &Rc<ClassObj>, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        if let Some(r) = self.meta_dunder(cls, "__call__", args.clone(), kw.clone()) {
            return r;
        }
        self.instantiate_default(cls, args, kw)
    }

    /// `Classe(args)`: cria a instância e roda `__init__`.
    pub(crate) fn instantiate_default(&mut self, cls: &Rc<ClassObj>, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        self.check_abstract(cls)?;
        let user_new = match cls.lookup("__new__") {
            Some(Value::Function(f)) => Some(f),
            Some(Value::Ext(e)) => match e.descriptor() {
                Some(Descriptor::Static(Value::Function(f))) => Some(f),
                _ => None,
            },
            _ => None,
        };
        let user_new_defined = user_new.is_some();
        // Metaclasse sem `__new__` próprio: `Meta(nome, bases, ns)` é o `type.__new__` com a
        // metaclasse, seguido do `__init__` dela, se houver.
        if cls.is_meta && !user_new_defined && args.len() == 3 {
            let mut full = vec![Value::Class(cls.clone())];
            full.extend(args.iter().cloned());
            let made = type_new(self, full, kw.clone())?;
            if let Some(Value::Function(f)) = cls.lookup("__init__") {
                let mut init = vec![made.clone()];
                init.extend(args);
                self.call_function(&f, init, kw)?;
            }
            return Ok(made);
        }
        let fresh = Rc::new(InstanceObj {
            class: cls.clone(),
            view: Default::default(),
            dict: RefCell::new(Default::default()),
            payload: RefCell::new(None),
        });
        if cls.data_base == Some("module") {
            init_module_fields(&fresh, &args, &kw)?;
        } else if let Some(t) = cls.data_base {
            // Sem `__init__`/`__new__` de usuário os argumentos vão direto para o tipo embutido.
            let own = user_new.is_some() || matches!(cls.lookup("__init__"), Some(Value::Function(_)));
            let (a, k) = if own { (Vec::new(), Vec::new()) } else { (args.clone(), kw.clone()) };
            let payload = self.call(&data_ctor(t), a, k)?;
            *fresh.payload.borrow_mut() = Some(payload);
        }
        let (inst, obj) = match user_new {
            Some(f) => {
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(Value::Class(cls.clone()));
                full.extend(args.iter().cloned());
                let made = self.call_function(&f, full, kw.clone())?;
                match &made {
                    Value::Instance(i) if i.class.mro().iter().any(|c| Rc::ptr_eq(c, cls)) => (i.clone(), made),
                    _ => return Ok(made),
                }
            }
            None => (fresh.clone(), Value::Instance(fresh)),
        };
        if cls.builtin_base.is_some() {
            inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(args.clone()));
            // `OSError(errno, strerror[, filename])` expõe os campos como atributos.
            if cls.builtin_base.is_some_and(|b| crate::object::exc_is_subclass(b, "OSError")) {
                if let [errno @ Value::Int(_), msg, rest @ ..] = args.as_slice() {
                    let mut d = inst.dict.borrow_mut();
                    d.insert("errno".to_string(), errno.clone());
                    d.insert("strerror".to_string(), msg.clone());
                    d.insert("filename".to_string(), rest.first().cloned().unwrap_or(Value::None));
                    d.insert("filename2".to_string(), rest.get(2).cloned().unwrap_or(Value::None));
                }
            }
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
                // `object.__init__` só reclama de argumentos sobrando quando `__new__` não foi sobrescrito.
                if !user_new_defined
                    && cls.builtin_base.is_none()
                    && cls.data_base.is_none()
                    && (!args.is_empty() || !kw.is_empty())
                {
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
            // Função embutida no CPython (escrita em Python aqui): `Classe.attr = time.time` não liga `self`.
            Value::Function(f) if f.attrs.borrow().contains_key("__no_bind__") => Ok(attr.clone()),
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
            // Descritor escrito em Python: `__get__(self, instância ou None, classe)`.
            Value::Instance(d) => match d.class.lookup("__get__") {
                Some(Value::Function(f)) => {
                    let instance = match recv {
                        Value::Class(_) => Value::None,
                        other => other,
                    };
                    self.call_function(&f, vec![attr.clone(), instance, Value::Class(cls.clone())], Vec::new())
                }
                _ => Ok(attr.clone()),
            },
            other => Ok(other.clone()),
        }
    }

    pub(crate) fn instance_getattr(&mut self, obj: &Value, inst: &Rc<InstanceObj>, name: &str) -> PyResult<Value> {
        self.instance_getattr_with(obj, inst, name, true)
    }

    /// `object.__getattribute__`: a busca normal sem o gancho `__getattr__` da classe.
    pub(crate) fn instance_getattr_plain(&mut self, obj: &Value, inst: &Rc<InstanceObj>, name: &str) -> PyResult<Value> {
        self.instance_getattr_with(obj, inst, name, false)
    }

    fn instance_getattr_with(&mut self, obj: &Value, inst: &Rc<InstanceObj>, name: &str, hook: bool) -> PyResult<Value> {
        match name {
            "__class__" => return Ok(Value::Class(inst.class.clone())),
            // O shim de um tipo embutido (`memoryview`) não tem `__dict__`, como o tipo em C.
            "__dict__" if matches!(inst.class.dict.borrow().get("__module__"), Some(Value::Str(m)) if m.as_str() == "builtins") => {
                return Err(exc("AttributeError", format!("'{}' object has no attribute '__dict__'", inst.class.name)));
            }
            "__dict__" if inst.class.slots_allow("__dict__") => return Ok(inst.live_dict()),
            _ => {}
        }
        inst.sync_from_view();
        // `__getattribute__` de usuário intercepta toda busca; `AttributeError` dele cai no `__getattr__`.
        if hook {
            if let Some(Value::Function(f)) = inst.class.lookup("__getattribute__") {
                return match self.call_function(&f, vec![obj.clone(), Value::str(name)], Vec::new()) {
                    Err(e) if e.kind == "AttributeError" => match inst.class.lookup("__getattr__") {
                        Some(Value::Function(g)) => self.call_function(&g, vec![obj.clone(), Value::str(name)], Vec::new()),
                        _ => Err(e),
                    },
                    other => other,
                };
            }
        }
        // Propriedades têm precedência sobre o dicionário da instância.
        let class_attr = inst.class.lookup(name);
        if let Some(Value::Ext(e)) = &class_attr {
            if matches!(e.descriptor(), Some(Descriptor::Property { .. })) {
                return self.bind_class_attr(class_attr.as_ref().unwrap_or(&Value::None), obj.clone(), &inst.class);
            }
        }
        // Descritor de dados escrito em Python (tem `__set__`) também vence o dicionário da instância.
        if let Some(Value::Instance(d)) = &class_attr {
            if d.class.lookup("__set__").is_some() {
                return self.bind_class_attr(class_attr.as_ref().unwrap_or(&Value::None), obj.clone(), &inst.class);
            }
        }
        let own = inst.dict.borrow().get(name).cloned();
        if let Some(v) = own {
            return Ok(v);
        }
        if let Some(attr) = class_attr {
            // `__new__` é estático implicitamente: pela instância vem a função sem ligar.
            if name == "__new__" {
                if let Value::Function(_) = &attr {
                    return Ok(attr);
                }
            }
            return self.bind_class_attr(&attr, obj.clone(), &inst.class);
        }
        if hook {
            if let Some(Value::Function(f)) = inst.class.lookup("__getattr__") {
                return self.call_function(&f, vec![obj.clone(), Value::str(name)], Vec::new());
            }
        }
        // `object.__reduce_ex__`, `__reduce__` e `__getstate__` (pickle, copy) vivem em `copyreg`.
        if let Some(fname) = match name {
            "__reduce_ex__" => Some("_object_reduce_ex"),
            "__reduce__" => Some("_object_reduce"),
            "__getstate__" => Some("_object_getstate"),
            _ => None,
        } {
            let m = crate::modules::import_checked(self, "copyreg")?;
            if let Value::Function(f) = self.getattr(&Value::Module(m), fname)? {
                return Ok(Value::BoundFn(Rc::new((obj.clone(), f))));
            }
        }
        let payload = inst.payload.borrow().clone();
        if let Some(p) = payload {
            // `d.__getitem__` de subclasse de `dict` com `__missing__`: a chave ausente chama o gancho.
            if name == "__getitem__" && matches!(p, Value::Dict(_)) && inst.class.lookup("__missing__").is_some() {
                return Ok(Value::Ext(Rc::new(InstanceDunder { obj: obj.clone(), name: "__getitem__" })));
            }
            return self.getattr(&p, name);
        }
        if name == "__doc__" {
            return Ok(inst.class.lookup("__doc__").unwrap_or(Value::None));
        }
        // `object.__new__` é um método estático: pela instância vale o mesmo objeto que pela classe.
        if name == "__new__" {
            let cls = inst.class.clone();
            return self.class_getattr(&cls, "__new__");
        }
        // `object.__init__` herdado: ligado à instância, como qualquer método.
        if name == "__init__" {
            let cls = inst.class.clone();
            let attr = self.class_getattr(&cls, "__init__")?;
            return self.bind_class_attr(&attr, obj.clone(), &cls);
        }
        if inst.class.builtin_base.is_some() && matches!(name, "__cause__" | "__context__" | "__suppress_context__") {
            return Ok(if name == "__suppress_context__" { Value::Bool(false) } else { Value::None });
        }
        if name == "with_traceback" && inst.class.builtin_base.is_some() {
            return Ok(Value::Ext(Rc::new(ExcWithTraceback { obj: obj.clone() })));
        }
        if name == "add_note" && inst.class.builtin_base.is_some() {
            return Ok(Value::Ext(Rc::new(ExcAddNote { obj: obj.clone() })));
        }
        Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", inst.class.name)))
    }

    pub(crate) fn class_getattr(&mut self, cls: &Rc<ClassObj>, name: &str) -> PyResult<Value> {
        match name {
            "__name__" => return Ok(Value::str(cls.name.clone())),
            "__qualname__" => return Ok(Value::str(cls.qualname())),
            "__module__" => return Ok(Value::str(cls.module())),
            "__bases__" => {
                if cls.bases.is_empty() {
                    let base = cls.builtin_base.map_or(Value::Builtin("object"), Value::Builtin);
                    return Ok(Value::tuple(vec![base]));
                }
                return Ok(Value::tuple(cls.bases.iter().map(|b| Value::Class(b.clone())).collect()));
            }
            "__mro__" => {
                let mut mro: Vec<Value> = cls.mro().into_iter().map(Value::Class).collect();
                mro.push(Value::Builtin("object"));
                return Ok(Value::tuple(mro));
            }
            "__dict__" => {
                let mut d = crate::object::Dict::new();
                for (k, v) in cls.dict.borrow().iter() {
                    d.set(Value::str(k.clone()), v.clone())?;
                }
                // O `type` sempre grava `__doc__` e, se nenhuma base já dá um `__dict__` às
                // instâncias, acrescenta os descritores `__dict__` e `__weakref__`.
                if !cls.dict.borrow().contains_key("__doc__") {
                    d.set(Value::str("__doc__"), Value::None)?;
                }
                for slot in instance_slots_added(cls) {
                    let desc = GetSetDescriptor { name: slot, owner: cls.clone() };
                    d.set(Value::str(*slot), Value::Ext(Rc::new(desc)))?;
                }
                return Ok(Value::dict(d));
            }
            "__class__" => return Ok(Value::Builtin("type")),
            "__subclasses__" => {
                let alive: Vec<Value> =
                    cls.subclasses.borrow().iter().filter_map(|w| w.upgrade()).map(Value::Class).collect();
                return Ok(Value::Ext(Rc::new(SubclassesCall(alive))));
            }
            // O docstring não é herdado: sem o próprio, `__doc__` é `None`.
            "__doc__" => return Ok(cls.dict.borrow().get("__doc__").cloned().unwrap_or(Value::None)),
            _ => {}
        }
        if name == "mro" && cls.lookup("mro").is_none() {
            let m = crate::modules::import_checked(self, "copyreg")?;
            if let Value::Function(f) = self.getattr(&Value::Module(m), "_type_mro")? {
                return Ok(Value::BoundFn(Rc::new((Value::Class(cls.clone()), f))));
            }
        }
        if let Some(attr) = cls.lookup(name) {
            // `__init_subclass__` e `__class_getitem__` são métodos de classe implícitos.
            if let (Value::Function(f), "__init_subclass__" | "__class_getitem__") = (&attr, name) {
                return Ok(Value::BoundFn(Rc::new((Value::Class(cls.clone()), f.clone()))));
            }
            return self.bind_class_attr(&attr, Value::Class(cls.clone()), cls);
        }
        // `Classe.__weakref__`: o descritor que o `type` pôs na primeira classe do MRO que o tem.
        if name == "__weakref__" {
            if let Some(owner) = cls.mro().into_iter().find(|c| instance_slots_added(c).contains(&"__weakref__")) {
                return Ok(Value::Ext(Rc::new(GetSetDescriptor { name: "__weakref__", owner })));
            }
        }
        // Atributos da metaclasse (`Color.__members__`, métodos de `EnumMeta`).
        // `__new__` e `__init__` existem em `object`, que vem antes da metaclasse na busca de atributos da classe.
        if let Some(meta) = cls.meta.as_ref().filter(|_| !matches!(name, "__new__" | "__init__")) {
            if let Some(attr) = meta.lookup(name) {
                let me = Value::Class(cls.clone());
                return match &attr {
                    Value::Function(f) => Ok(Value::BoundFn(Rc::new((me, f.clone())))),
                    Value::Ext(e) => match e.descriptor() {
                        Some(Descriptor::Property { get, .. }) => self.call_value(&get, vec![me], Vec::new()),
                        _ => Ok(attr.clone()),
                    },
                    other => Ok(other.clone()),
                };
            }
        }
        if name == "__new__" && cls.data_base.is_none() && cls.builtin_base.is_none() {
            return Ok(crate::typeattrs::object_new_value());
        }
        // Subclasse de `dict`/`list`/...: o `__new__` é o do tipo embutido (`dict.__new__(cls)`).
        if name == "__new__" {
            if let Some(v) = cls.data_base.and_then(|t| crate::typeattrs::type_attr(t, "__new__")) {
                return Ok(v);
            }
        }
        // Construtores alternativos do tipo base (`fromkeys`, `from_bytes`, `fromhex`, `maketrans`).
        if let Some(t) = cls.data_base {
            if matches!(name, "fromkeys" | "from_bytes" | "fromhex") {
                if let Some(inner) = crate::typeattrs::type_attr(t, name) {
                    return Ok(Value::Ext(Rc::new(AltCtor { cls: Value::Class(cls.clone()), inner })));
                }
            }
            if name == "maketrans" {
                if let Some(v) = crate::typeattrs::type_attr(t, name) {
                    return Ok(v);
                }
            }
        }
        // Os métodos de `object` (`__init__`, `__eq__`, `__setattr__`...) valem para toda classe comum.
        if cls.data_base.is_none() && cls.builtin_base.is_none() && name != "__name__" {
            if let Some(v) = crate::typeattrs::object_attr(name) {
                return Ok(v);
            }
        }
        // Subclasse de exceção embutida: `H.__init__(self, msg)` é o `BaseException.__init__`.
        if let Some(base) = cls.builtin_base {
            if matches!(
                name,
                "__new__" | "__init__" | "__str__" | "__repr__" | "__reduce__" | "__setstate__" | "with_traceback" | "add_note"
            ) {
                return self.load_attr(&Value::Builtin(base), name);
            }
        }
        Err(exc("AttributeError", format!("type object '{}' has no attribute '{name}'", cls.name)))
    }

    pub(crate) fn store_attr(&mut self, obj: &Value, name: &str, value: Value) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                // Uma busca só serve aos dois casos de descritor (property e `__set__` em Python).
                let class_attr = inst.class.lookup(name);
                if let Some(Value::Ext(e)) = &class_attr {
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
                if let Some(Value::Instance(d)) = &class_attr {
                    if let Some(Value::Function(f)) = d.class.lookup("__set__") {
                        self.call_function(&f, vec![Value::Instance(d.clone()), obj.clone(), value], Vec::new())?;
                        return Ok(());
                    }
                }
                if name == "__dict__" && inst.class.slots_allow("__dict__") {
                    // `obj.__dict__ = d`: o próprio `d` passa a ser o espaço de atributos (vivo).
                    let Value::Dict(d) = value else {
                        return Err(type_error(format!(
                            "__dict__ must be set to a dictionary, not a '{}'",
                            value.type_name()
                        )));
                    };
                    *inst.view.borrow_mut() = Some(d);
                    inst.sync_from_view();
                    return Ok(());
                }
                if !inst.class.slots_allow(name) {
                    return Err(exc(
                        "AttributeError",
                        format!(
                            "'{}' object has no attribute '{name}' and no __dict__ for setting new attributes",
                            inst.class.name
                        ),
                    ));
                }
                inst.sync_from_view();
                {
                    let mut d = inst.dict.borrow_mut();
                    match d.get_mut(name) {
                        Some(slot) => *slot = value,
                        None => {
                            d.insert(name.to_string(), value);
                        }
                    }
                }
                inst.sync_to_view();
                Ok(())
            }
            Value::Class(c) => {
                c.dict.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Module(m) => {
                if let Some(g) = self.module_globals.borrow().get(m.name) {
                    g.borrow_mut().insert(name.into(), value.clone());
                }
                m.attrs.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Function(f) => {
                f.attrs.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Ext(e) if e.setattr(name, value.clone()).is_some() => e.setattr(name, value).unwrap_or(Ok(())),
            // `exc.__traceback__ = tb` (o `contextlib` devolve o traceback original ao propagar).
            Value::Exception(x) if name == "__traceback__" => {
                *x.traceback.borrow_mut() = if matches!(value, Value::None) { None } else { Some(value) };
                Ok(())
            }
            Value::Exception(x) if name == "__notes__" => {
                let mut extra = x.extra.borrow_mut();
                extra.retain(|(k, _)| *k != "__notes__");
                extra.push(("__notes__", value));
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
                if let Some(Value::Function(f)) = inst.class.lookup("__delattr__") {
                    self.call_function(&f, vec![obj.clone(), Value::str(name)], Vec::new())?;
                    return Ok(());
                }
                if let Some(Value::Instance(d)) = inst.class.lookup(name) {
                    if let Some(Value::Function(f)) = d.class.lookup("__delete__") {
                        self.call_function(&f, vec![Value::Instance(d.clone()), obj.clone()], Vec::new())?;
                        return Ok(());
                    }
                }
                inst.sync_from_view();
                if inst.dict.borrow_mut().shift_remove(name).is_none() {
                    return Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", inst.class.name)));
                }
                inst.sync_to_view();
                Ok(())
            }
            Value::Class(c) => {
                if c.dict.borrow_mut().shift_remove(name).is_none() {
                    return Err(exc("AttributeError", format!("type object '{}' has no attribute '{name}'", c.name)));
                }
                Ok(())
            }
            Value::Module(m) => {
                let from_globals = self
                    .module_globals
                    .borrow()
                    .get(m.name)
                    .is_some_and(|g| g.borrow_mut().remove(name).is_some());
                let from_attrs = m.attrs.borrow_mut().remove(name).is_some();
                if !from_globals && !from_attrs {
                    return Err(exc("AttributeError", format!("'module' object has no attribute '{name}'")));
                }
                Ok(())
            }
            _ => Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", obj.type_name()))),
        }
    }

    /// `del container[index]`.
    pub(crate) fn delete_subscript(&mut self, container: &Value, index: &Value) -> PyResult<()> {
        match container {
            Value::ByteArray(b) => {
                let len = b.borrow().len() as i64;
                match index {
                    Value::Slice(s) => {
                        let mut doomed = crate::vm::slice_indices(len as usize, s)?;
                        doomed.sort_unstable();
                        let mut v = b.borrow_mut();
                        for i in doomed.into_iter().rev() {
                            v.remove(i);
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
                            return Err(exc("IndexError", "bytearray index out of range"));
                        }
                        b.borrow_mut().remove(j as usize);
                        Ok(())
                    }
                    other => Err(type_error(format!(
                        "bytearray indices must be integers or slices, not {}",
                        other.type_name()
                    ))),
                }
            }
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
                    e.value = Some(Value::Exception(Rc::new(crate::object::ExcObj::new("KeyError", vec![index.clone()]))));
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
            Value::Builtin("Ellipsis") => Value::Builtin("ellipsis"),
            Value::Builtin("NotImplemented") => Value::Builtin("NotImplementedType"),
            // `int`, `object`, `ValueError`...: as classes embutidas são instâncias de `type`.
            Value::Builtin(_) | Value::NativeFn(_)
                if crate::builtins::class_name(v).is_some()
                    && !matches!(v, Value::Builtin("Ellipsis" | "NotImplemented")) =>
            {
                Value::Builtin("type")
            }
            Value::Class(c) => match &c.meta {
                Some(m) => Value::Class(m.clone()),
                None => Value::Builtin("type"),
            },
            other => {
                // Os tipos de dados são os mesmos valores que os nomes globais `int`, `dict`...
                let n = other.type_name();
                crate::builtins::get(n).unwrap_or_else(|| {
                    let n = intern(n);
                    match other {
                        Value::Ext(e) => crate::object::register_native_type_methods(n, e.methods()),
                        _ => crate::object::register_native_type(n),
                    }
                    Value::Builtin(n)
                })
            }
        }
    }

    /// Texto padrão de uma instância sem `__str__`/`__repr__` de usuário.
    pub(crate) fn default_text(&mut self, v: &Value, is_str: bool) -> String {
        let Value::Instance(i) = v else { return to_str(v) };
        // Subclasse de `str`/`int`/`list`...: o texto padrão é o do valor guardado.
        let payload = i.payload.borrow().clone();
        if let Some(p) = payload {
            return if is_str { to_str(&p) } else { crate::object::repr(&p) };
        }
        if i.class.builtin_base.is_some() {
            let args = match i.dict.borrow().get("args") {
                Some(Value::Tuple(t)) => t.to_vec(),
                _ => Vec::new(),
            };
            if is_str {
                if let (Some(base), [Value::Int(_), _, ..]) = (i.class.builtin_base, args.as_slice())
                    && crate::object::exc_is_subclass(base, "OSError")
                {
                    return crate::object::exc_str(&crate::object::ExcObj::new(base, args));
                }
                return match args.as_slice() {
                    [] => String::new(),
                    [one] if i.class.builtin_base.is_some_and(|b| crate::object::exc_is_subclass(b, "KeyError")) => {
                        crate::object::repr(one)
                    }
                    [one] => to_str(one),
                    _ => to_str(&Value::tuple(args)),
                };
            }
            let inner: Vec<String> = args.iter().map(crate::object::repr).collect();
            return format!("{}({})", i.class.name, inner.join(", "));
        }
        format!("<{}.{} object at {:#x}>", i.class.module(), i.class.name, crate::object::py_addr(Rc::as_ptr(i) as usize))
    }

    /// `str(v)`, chamando `__str__` (ou `__repr__`) de usuário.
    pub(crate) fn str_of(&mut self, v: &Value) -> PyResult<String> {
        if let Value::Class(c) = v {
            for name in ["__str__", "__repr__"] {
                if let Some(Value::Function(f)) = c.meta.as_ref().and_then(|m| m.lookup(name)) {
                    return match self.call_function(&f, vec![v.clone()], Vec::new())? {
                        Value::Str(s) => Ok(s.as_str().to_string()),
                        other => Err(type_error(format!("{name} returned non-string (type {})", other.type_name()))),
                    };
                }
            }
        }
        if let Value::Instance(i) = v {
            for name in ["__str__", "__repr__"] {
                // `str.__str__` devolve o próprio texto: o `__repr__` de usuário não entra em `str(x)`.
                if name == "__repr__" && matches!(&*i.payload.borrow(), Some(Value::Str(_))) {
                    continue;
                }
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
        if let Value::Class(c) = v {
            if let Some(Value::Function(f)) = c.meta.as_ref().and_then(|m| m.lookup("__repr__")) {
                return match self.call_function(&f, vec![v.clone()], Vec::new())? {
                    Value::Str(s) => Ok(s.as_str().to_string()),
                    other => Err(type_error(format!("__repr__ returned non-string (type {})", other.type_name()))),
                };
            }
        }
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
        let user = match i.class.lookup(name) {
            Some(Value::Function(f)) => Some(f),
            // Atributo de classe que já é um chamável preso (`__next__ = gerador.__next__`): chama direto.
            Some(bound @ (Value::Bound(_) | Value::BoundFn(_) | Value::Ext(_) | Value::NativeFn(_))) => {
                return Some(self.call_value(&bound, args, Vec::new()));
            }
            // Descritor de usuário (`__get__` em Python, como o `MagicProxy` do mock): resolve e chama.
            Some(attr @ Value::Instance(_)) if matches!(&attr, Value::Instance(d) if d.class.lookup("__get__").is_some()) => {
                let bound = match self.bind_class_attr(&attr, obj.clone(), &i.class) {
                    Ok(b) => b,
                    Err(e) => return Some(Err(e)),
                };
                return Some(self.call_value(&bound, args, Vec::new()));
            }
            // Objeto chamável sem `__get__` na classe (um `MagicMock` posto como `__len__`): o CPython o
            // chama só com os argumentos, sem o `self`.
            Some(attr @ Value::Instance(_)) if matches!(&attr, Value::Instance(d) if d.class.lookup("__call__").is_some()) => {
                return Some(self.call_value(&attr, args, Vec::new()));
            }
            _ => None,
        };
        let Some(f) = user else {
            // Subclasse de tipo embutido: o que a classe não redefine vai para o valor embutido.
            let payload = i.payload.borrow().clone()?;
            let r = crate::vm::payload_dunder(&payload, name, args.clone())?;
            if let (Err(e), "__getitem__") = (&r, name) {
                if e.kind == "KeyError" {
                    if let Some(Value::Function(m)) = i.class.lookup("__missing__") {
                        let key = args.first().cloned().unwrap_or(Value::None);
                        return Some(self.call_function(&m, vec![obj.clone(), key], Vec::new()));
                    }
                }
            }
            // `x += y` sobre lista/dict/set muta o valor embutido e continua sendo a mesma instância.
            return Some(match r {
                Ok(_)
                    if matches!(name, "__iadd__" | "__isub__" | "__imul__" | "__iand__" | "__ior__" | "__ixor__")
                        && matches!(payload, Value::List(_) | Value::Dict(_) | Value::Set(_)) =>
                {
                    Ok(obj.clone())
                }
                other => other,
            });
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
                let _doc = it.next();
                let mut get = get;
                let (mut set, mut del) = (set, del);
                for (k, v) in kw {
                    match k.as_str() {
                        "fget" => get = v,
                        "fset" => set = Some(v),
                        "fdel" => del = Some(v),
                        "doc" => {}
                        _ => return Err(type_error(format!("property() got an unexpected keyword argument '{k}'"))),
                    }
                }
                Ok(Value::Ext(Rc::new(Property::new(get, set, del))))
            }
            "super" => match args.as_slice() {
                // `super()` sem argumentos vira `super(__class__, primeiro_parâmetro)` no compilador.
                [Value::Class(cls), obj] => Ok(Value::Ext(Rc::new(SuperProxy { obj: obj.clone(), cls: cls.clone() }))),
                _ => Err(exc("RuntimeError", "super(): no arguments")),
            },
            "type" => match args.as_slice() {
                [v] => Ok(self.type_of(v)),
                [Value::Str(n), Value::Tuple(bases), ns] => {
                    let ns = dict_to_ns(ns)?;
                    Ok(Value::Class(self.create_class(n.as_str().to_string(), bases, ns, None, kw)?))
                }
                _ => Err(type_error("type() takes 1 or 3 arguments")),
            },
            "object" => {
                if !args.is_empty() || !kw.is_empty() {
                    return Err(type_error("object() takes no arguments"));
                }
                Ok(Value::Ext(Rc::new(PlainObject)))
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
    if let Value::Class(c) = v {
        // classe cuja metaclasse define `__repr__`/`__str__` (EnumType: `<enum 'Color'>`)
        let meta = c.meta.as_ref()?;
        let mut vm = current()?;
        let names: &[&str] = if is_str { &["__str__", "__repr__"] } else { &["__repr__"] };
        for name in names {
            if let Some(Value::Function(f)) = meta.lookup(name) {
                return match vm.call_function(&f, vec![v.clone()], Vec::new()) {
                    Ok(Value::Str(s)) => Some(s.as_str().to_string()),
                    _ => None,
                };
            }
        }
        return None;
    }
    let Value::Instance(i) = v else { return None };
    let mut vm = current()?;
    let names: &[&str] = if is_str { &["__str__", "__repr__"] } else { &["__repr__"] };
    for name in names {
        if *name == "__repr__" && is_str && matches!(&*i.payload.borrow(), Some(Value::Str(_))) {
            continue;
        }
        if let Some(Value::Function(f)) = i.class.lookup(name) {
            return match vm.call_function(&f, vec![v.clone()], Vec::new()) {
                Ok(Value::Str(s)) => Some(s.as_str().to_string()),
                Ok(other) => {
                    crate::vm::note_text_error(
                        type_error(format!("{name} returned non-string (type {})", other.type_name())),
                        vm.depth_now(),
                    );
                    None
                }
                Err(e) => {
                    crate::vm::note_text_error(e, vm.depth_now());
                    None
                }
            };
        }
    }
    if i.class.builtin_base.is_some() {
        return Some(vm.default_text(v, is_str));
    }
    let payload = i.payload.borrow().clone()?;
    Some(if is_str { crate::object::to_str(&payload) } else { crate::object::repr(&payload) })
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
    let has_payload = |v: &Value| matches!(v, Value::Instance(i) if i.payload.borrow().is_some());
    if has_payload(a) || has_payload(b) {
        return Some(crate::object::py_eq(&crate::vm::unwrap_payload(a), &crate::vm::unwrap_payload(b)));
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
    let payload = i.payload.borrow().clone()?;
    crate::object::hash(&payload).ok()
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
    if let Value::Instance(i) = v {
        if let Some(p) = i.payload.borrow().as_ref() {
            return p.is_true();
        }
    }
    true
}

// ------------------------------------------------------------------------------------------------
// Construção de classes: bases, espaço de nomes

struct BaseInfo {
    classes: Vec<Rc<ClassObj>>,
    builtin_base: Option<&'static str>,
    data_base: Option<&'static str>,
    derives_type: bool,
}

/// Tipos de dados embutidos que uma classe de usuário pode estender (a instância guarda o valor).
fn data_type(name: &str) -> Option<&'static str> {
    ["int", "float", "str", "list", "tuple", "dict", "set", "bool", "module"].into_iter().find(|n| *n == name)
}

/// `types.ModuleType(name, doc=None)`: a subclasse de módulo não tem valor embutido, só os
/// atributos `__name__` e `__doc__` no próprio `__dict__`.
fn init_module_fields(inst: &InstanceObj, args: &[Value], kw: &[(String, Value)]) -> PyResult<()> {
    let arg = |i: usize, key: &str| args.get(i).cloned().or_else(|| kw.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()));
    let Some(name) = arg(0, "name") else {
        return Err(type_error("module.__init__() missing required argument 'name' (pos 1)"));
    };
    if !matches!(name, Value::Str(_)) {
        return Err(type_error(format!("module.__init__() argument 'name' must be str, not {}", name.type_name())));
    }
    let mut d = inst.dict.borrow_mut();
    d.insert("__name__".to_string(), name);
    d.insert("__doc__".to_string(), arg(1, "doc").unwrap_or(Value::None));
    d.insert("__package__".to_string(), Value::None);
    d.insert("__loader__".to_string(), Value::None);
    d.insert("__spec__".to_string(), Value::None);
    Ok(())
}

/// O construtor embutido (`dict`, `list`...) de um tipo de dados.
fn data_ctor(name: &str) -> Value {
    crate::builtins::get(name).unwrap_or(Value::Builtin("object"))
}

fn resolve_bases(bases: &[Value]) -> PyResult<BaseInfo> {
    let mut info = BaseInfo { classes: Vec::new(), builtin_base: None, data_base: None, derives_type: false };
    for b in bases {
        match b {
            Value::Class(c) => {
                if info.builtin_base.is_none() {
                    info.builtin_base = c.builtin_base;
                }
                if info.data_base.is_none() {
                    info.data_base = c.data_base;
                }
                info.derives_type |= c.is_meta;
                info.classes.push(c.clone());
            }
            Value::Builtin(n) if EXC_CLASSES.iter().any(|(e, _)| e == n) => {
                // Entre várias bases de exceção vale a mais específica (`A(BaseException)` + `Exception`).
                let more_specific = match info.builtin_base {
                    None => true,
                    Some(cur) => cur != *n && crate::object::exc_is_subclass(n, cur),
                };
                if more_specific {
                    info.builtin_base = EXC_CLASSES.iter().find(|(e, _)| e == n).map(|(e, _)| *e);
                }
            }
            Value::Builtin("type") => info.derives_type = true,
            Value::Builtin("object") => {}
            Value::Builtin(n) if data_type(n).is_some() => {
                if info.data_base.is_none() {
                    info.data_base = data_type(n);
                }
            }
            Value::NativeFn(f) if data_type(f.name).is_some() => {
                if info.data_base.is_none() {
                    info.data_base = data_type(f.name);
                }
            }
            other => {
                return Err(type_error(format!(
                    "cannot create a class from base '{}' (this builtin base class is not supported yet)",
                    to_str(other)
                )))
            }
        }
    }
    Ok(info)
}

/// Os nomes definidos no corpo da classe, na ordem em que nasceram.
fn namespace_of(env: &Rc<Env>) -> Vec<(String, Value)> {
    let vars = env.vars.borrow();
    env.order.borrow().iter().filter_map(|k| vars.get(k.as_str()).map(|v| (k.clone(), v.clone()))).collect()
}

/// `type.__new__(mcs, nome, bases, ns)` lido direto do tipo (`return type.__new__(mcs, ...)` numa
/// metaclasse), igual ao `super().__new__` de metaclasse.
pub(crate) fn type_new(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [meta, Value::Str(n), Value::Tuple(bases), ns] => {
            let meta = match meta {
                Value::Class(m) if m.is_meta => Some(m.clone()),
                Value::Builtin("type") => None,
                other => {
                    return Err(type_error(format!("type.__new__(X): X is not a type object ({})", other.type_name())));
                }
            };
            let ns = dict_to_ns(ns)?;
            Ok(Value::Class(vm.create_class(n.as_str().to_string(), bases, ns, meta, kw)?))
        }
        [_, obj] => Ok(vm.type_of(obj)),
        [] => Err(type_error("type.__new__(): not enough arguments")),
        _ => Err(type_error("type.__new__() takes exactly 3 arguments")),
    }
}

fn dict_to_ns(ns: &Value) -> PyResult<Vec<(String, Value)>> {
    let Value::Dict(d) = ns else {
        return Err(type_error("type.__new__() argument 3 must be dict"));
    };
    Ok(d.borrow().iter().map(|(k, v)| (to_str(k), v.clone())).collect())
}

fn ns_to_dict(ns: &[(String, Value)]) -> PyResult<Value> {
    let mut d = crate::object::Dict::new();
    for (k, v) in ns {
        d.set(Value::str(k.clone()), v.clone()).map_err(|_| type_error("unhashable type"))?;
    }
    Ok(Value::dict(d))
}
