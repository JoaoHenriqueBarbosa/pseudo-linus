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
        &["setter", "getter", "deleter"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__doc__" => Some(Ok(self.doc.borrow().clone())),
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
        Ok(Value::Ext(Rc::new(Property::new(get, set, del))))
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
        if let Value::Class(recv) = &self.obj {
            // Receptor é uma classe (`__init_subclass__`, `__new__` de metaclasse, classmethods).
            let mro = recv.mro();
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
                            Some(t) => Some(vm.call(&data_ctor(t), rest.iter().map(crate::vm::unwrap_payload).collect(), kw)?),
                            None => None,
                        };
                        Ok(Value::Instance(Rc::new(InstanceObj {
                            class: c.clone(),
                            view: Default::default(),
                            dict: RefCell::new(indexmap::IndexMap::new()),
                            payload: RefCell::new(payload),
                        })))
                    }
                    _ => Err(type_error("type.__new__() takes exactly 3 arguments")),
                },
                "__init__" => Ok(Value::None),
                // `super().__call__(...)` de uma metaclasse: instancia a classe normalmente.
                "__call__" => match args.as_slice() {
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
            "__debug__" => Ok(Value::Bool(true)),
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
        for b in &bases {
            if let Value::Instance(i) = b {
                if let Some(Value::Function(f)) = i.class.lookup("__mro_entries__") {
                    match self.call_function(&f, vec![b.clone(), Value::tuple(bases.clone())], Vec::new())? {
                        Value::Tuple(t) => expanded.extend(t.iter().cloned()),
                        _ => return Err(type_error("__mro_entries__ must return a tuple")),
                    }
                    continue;
                }
            }
            expanded.push(b.clone());
        }
        let bases = expanded;
        let class_env = Env::new(env.capture(), true, false);
        self.exec(body, &class_env)?;
        let mut ns = namespace_of(&class_env);
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
        let cls = Rc::new(ClassObj {
            name,
            bases: info.classes,
            builtin_base: info.builtin_base,
            data_base: info.data_base,
            meta,
            is_meta: info.derives_type,
            dict: RefCell::new(ns.into_iter().collect()),
        });
        let owner = Value::Class(cls.clone());
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
        let fresh = Rc::new(InstanceObj {
            class: cls.clone(),
            view: Default::default(),
            dict: RefCell::new(indexmap::IndexMap::new()),
            payload: RefCell::new(None),
        });
        if let Some(t) = cls.data_base {
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
            "__dict__" => return Ok(inst.live_dict()),
            _ => {}
        }
        inst.sync_from_view();
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
            return self.bind_class_attr(&attr, obj.clone(), &inst.class);
        }
        if hook {
            if let Some(Value::Function(f)) = inst.class.lookup("__getattr__") {
                return self.call_function(&f, vec![obj.clone(), Value::str(name)], Vec::new());
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
        if name == "with_traceback" && inst.class.builtin_base.is_some() {
            return Ok(Value::Ext(Rc::new(ExcWithTraceback { obj: obj.clone() })));
        }
        Err(exc("AttributeError", format!("'{}' object has no attribute '{name}'", inst.class.name)))
    }

    pub(crate) fn class_getattr(&mut self, cls: &Rc<ClassObj>, name: &str) -> PyResult<Value> {
        match name {
            "__name__" | "__qualname__" => return Ok(Value::str(cls.name.clone())),
            "__module__" => return Ok(Value::str(cls.module())),
            "__bases__" => {
                return Ok(Value::tuple(cls.bases.iter().map(|b| Value::Class(b.clone())).collect()));
            }
            "__mro__" => {
                return Ok(Value::tuple(cls.mro().into_iter().map(Value::Class).collect()));
            }
            "__dict__" => {
                let mut d = crate::object::Dict::new();
                for (k, v) in cls.dict.borrow().iter() {
                    d.set(Value::str(k.clone()), v.clone())?;
                }
                return Ok(Value::dict(d));
            }
            "__class__" => return Ok(Value::Builtin("type")),
            // O docstring não é herdado: sem o próprio, `__doc__` é `None`.
            "__doc__" => return Ok(cls.dict.borrow().get("__doc__").cloned().unwrap_or(Value::None)),
            _ => {}
        }
        if let Some(attr) = cls.lookup(name) {
            // `__init_subclass__` e `__class_getitem__` são métodos de classe implícitos.
            if let (Value::Function(f), "__init_subclass__" | "__class_getitem__") = (&attr, name) {
                return Ok(Value::BoundFn(Rc::new((Value::Class(cls.clone()), f.clone()))));
            }
            return self.bind_class_attr(&attr, Value::Class(cls.clone()), cls);
        }
        // Atributos da metaclasse (`Color.__members__`, métodos de `EnumMeta`).
        if let Some(meta) = &cls.meta {
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
        // Os métodos de `object` (`__init__`, `__eq__`, `__setattr__`...) valem para toda classe comum.
        if cls.data_base.is_none() && cls.builtin_base.is_none() && name != "__name__" {
            if let Some(v) = crate::typeattrs::object_attr(name) {
                return Ok(v);
            }
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
                if let Some(Value::Instance(d)) = inst.class.lookup(name) {
                    if let Some(Value::Function(f)) = d.class.lookup("__set__") {
                        self.call_function(&f, vec![Value::Instance(d.clone()), obj.clone(), value], Vec::new())?;
                        return Ok(());
                    }
                }
                inst.sync_from_view();
                inst.dict.borrow_mut().insert(name.to_string(), value);
                inst.sync_to_view();
                Ok(())
            }
            Value::Class(c) => {
                c.dict.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Module(m) => {
                if let Some(g) = self.module_globals.borrow().get(m.name) {
                    g.borrow_mut().insert(name.to_string(), value.clone());
                }
                m.attrs.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Function(f) => {
                f.attrs.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Ext(e) if e.setattr(name, value.clone()).is_some() => e.setattr(name, value).unwrap_or(Ok(())),
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
            Value::Class(_) => Value::Builtin("type"),
            other => {
                // Os tipos de dados são os mesmos valores que os nomes globais `int`, `dict`...
                let n = other.type_name();
                crate::builtins::get(n).unwrap_or_else(|| Value::Builtin(intern(n)))
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
        format!("<{}.{} object at {:#x}>", i.class.module(), i.class.name, Rc::as_ptr(i) as usize)
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
                // `super()` sem argumentos vira `super(self, "Classe")` no compilador.
                [obj, Value::Str(cname)] => {
                    let mro = match obj {
                        Value::Instance(inst) => inst.class.mro(),
                        Value::Class(c) => c.mro(),
                        _ => return Err(type_error("super(): __self__ is not an instance")),
                    };
                    let cls = mro
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
                [Value::Str(n), Value::Tuple(bases), ns] => {
                    let ns = dict_to_ns(ns)?;
                    Ok(Value::Class(self.create_class(n.as_str().to_string(), bases, ns, None, kw)?))
                }
                _ => Err(type_error("type() takes 1 or 3 arguments")),
            },
            "object" => {
                if !args.is_empty() {
                    return Err(type_error("object() takes no arguments"));
                }
                let base = Rc::new(ClassObj {
                    name: "object".to_string(),
                    bases: Vec::new(),
                    builtin_base: None,
                    data_base: None,
                    meta: None,
                    is_meta: false,
                    dict: RefCell::new(indexmap::IndexMap::new()),
                });
                Ok(Value::Instance(Rc::new(InstanceObj {
                    class: base,
                    view: Default::default(),
                    dict: RefCell::new(indexmap::IndexMap::new()),
                    payload: RefCell::new(None),
                })))
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
    ["int", "float", "str", "list", "tuple", "dict", "set", "bool"].into_iter().find(|n| *n == name)
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
    env.order.borrow().iter().filter_map(|k| vars.get(k).map(|v| (k.clone(), v.clone()))).collect()
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
