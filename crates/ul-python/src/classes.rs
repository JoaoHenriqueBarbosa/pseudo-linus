//! Classes de usuário: construção (`class`), instâncias, atributos, descritores (`staticmethod`,
//! `classmethod`, `property`), `super()`, o protocolo do `with` e o despacho dos métodos mágicos
//! (`__init__`, `__str__`, `__eq__`, `__add__`...) para o restante da VM.

use std::cell::RefCell;
use std::rc::Rc;

use crate::compile::Code;
use crate::object::{
    exc_is_subclass, intern, to_str, BoundMethod, ClassObj, Descriptor, Env, ExtImage, ExtObject, FuncObj,
    InstanceObj, Kw, OpaqueImage, Value, EXC_CLASSES,
};
use crate::vm::{current, exc, type_error, PyException, PyResult, Vm};

/// Uma chamada de função Python anotada para o laço de instruções executar num quadro próprio.
pub(crate) type PendingCall = Option<(Rc<FuncObj>, Vec<Value>)>;

/// O estado de `Classe(args)` depois da criação da instância.
pub(crate) enum Built {
    /// Nada mais a executar: o valor que a chamada devolve.
    Done(Value),
    /// Falta rodar o `__init__` em Python (`args` já traz a instância na frente); a chamada devolve `obj`.
    Init { obj: Value, init: Rc<FuncObj>, args: Vec<Value>, kw: Kw },
}

/// O fim de `Classe(args)`: o `__init__` tem de devolver `None`, e a chamada entrega a instância.
pub(crate) fn init_returned(obj: Value, returned: &Value) -> PyResult<Value> {
    if matches!(returned, Value::None) {
        Ok(obj)
    } else {
        Err(type_error(format!("__init__() should return None, not '{}'", returned.type_name())))
    }
}

/// Os descritores `__dict__` e `__weakref__` que o `type_new` acrescenta à classe: sem `__slots__`, os
/// dois (se nenhuma base já dá um dicionário às instâncias, as exceções só ganham `__weakref__`); com
/// `__slots__`, só os que ele nomeia. Shim de tipo em C não ganha nenhum.
fn instance_slots_added(cls: &Rc<ClassObj>) -> &'static [&'static str] {
    if let Some(slots) = cls.declared_slots() {
        let has = |n: &str| slots.iter().any(|s| s == n) && !cls.emulates_c_type();
        return match (has("__dict__"), has("__weakref__")) {
            (true, true) => &["__dict__", "__weakref__"],
            (true, false) => &["__dict__"],
            (false, true) => &["__weakref__"],
            (false, false) => &[],
        };
    }
    if cls.emulates_c_type() {
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

/// O leiaute das instâncias de uma classe (o que o `compatible_for_assignment` do CPython compara):
/// o tipo embutido de base, os nomes de `__slots__` de toda a herança e se há `__dict__`.
fn instance_layout(cls: &Rc<ClassObj>) -> (Option<&'static str>, Option<&'static str>, Vec<String>, bool) {
    let mut slots: Vec<String> = Vec::new();
    for c in cls.mro() {
        slots.extend(c.declared_slots().unwrap_or_default());
    }
    slots.retain(|s| s != "__dict__");
    (cls.data_base, cls.builtin_base, slots, cls.slots_allow("__dict__"))
}

/// A classe de `obj.__class__ = valor`: o valor precisa ser uma classe (`object_set_class`).
fn class_assignment_value(value: &Value) -> PyResult<&Rc<ClassObj>> {
    match value {
        Value::Class(c) => Ok(c),
        Value::Builtin(_) | Value::NativeFn(_) if crate::builtins::class_name(value).is_some() => {
            Err(type_error("__class__ assignment only supported for mutable types or ModuleType subclasses"))
        }
        other => Err(type_error(format!("__class__ must be set to a class, not '{}' object", other.type_name()))),
    }
}

/// As checagens do `object_set_class` para `obj.__class__ = valor` numa instância, e a troca.
fn assign_instance_class(inst: &Rc<InstanceObj>, value: &Value) -> PyResult<()> {
    let old = inst.class();
    let new = class_assignment_value(value)?;
    if instance_layout(new) != instance_layout(&old) {
        return Err(type_error(format!(
            "__class__ assignment: '{}' object layout differs from '{}'",
            new.name, old.name
        )));
    }
    inst.set_class(new.clone());
    Ok(())
}

thread_local! {
    /// A classe que `módulo.__class__ = Sub` deu a cada módulo (por nome); sem entrada, o módulo é `ModuleType`.
    static MODULE_CLASSES: RefCell<std::collections::HashMap<&'static str, Rc<ClassObj>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// A classe que `módulo.__class__ = Sub` deu ao módulo, se houver.
pub(crate) fn module_class(m: &crate::object::ModuleObj) -> Option<Rc<ClassObj>> {
    MODULE_CLASSES.with(|c| {
        let c = c.borrow();
        if c.is_empty() {
            return None;
        }
        c.get(m.name).cloned()
    })
}

/// `módulo.__class__ = valor`: só vale uma subclasse de `ModuleType` (ou o próprio `ModuleType`, que
/// desfaz a troca).
fn set_module_class(m: &crate::object::ModuleObj, value: &Value) -> PyResult<()> {
    if crate::builtins::class_name(value) == Some("module") {
        MODULE_CLASSES.with(|c| c.borrow_mut().remove(m.name));
        return Ok(());
    }
    let new = class_assignment_value(value)?;
    if new.data_base != Some("module") {
        return Err(type_error("__class__ assignment only supported for mutable types or ModuleType subclasses"));
    }
    MODULE_CLASSES.with(|c| c.borrow_mut().insert(m.name, new.clone()));
    Ok(())
}

/// Descritor de dados (`property`, ou objeto Python com `__set__`): vence o dicionário da instância.
fn is_data_descriptor(attr: &Value) -> bool {
    match attr {
        Value::Ext(e) => matches!(e.descriptor(), Some(Descriptor::Property { .. })),
        Value::Instance(d) => d.class().lookup("__set__").is_some() || is_property_instance(d),
        _ => false,
    }
}

/// `type(obj).nome` de um objeto nativo: o método sem receptor (`method_descriptor`), que chamado
/// com o objeto na frente repassa a chamada a ele.
pub(crate) struct NativeTypeMethod {
    pub(crate) owner: &'static str,
    pub(crate) name: &'static str,
}

impl ExtObject for NativeTypeMethod {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("native_type_method", (self.owner, self.name), Vec::new())
    }
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
            return Err(crate::object::no_attribute("method_descriptor", name));
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

/// `getset_descriptor` dos atributos `__dict__` e `__weakref__` que o `type` põe na classe, e o
/// `__dict__` dos tipos embutidos (`function.__dict__['__dict__']`); o dono é a classe ou o tipo.
struct GetSetDescriptor {
    name: &'static str,
    owner: Value,
}

/// O `getset_descriptor` de `name` do tipo embutido `owner` (o `__dict__` de `type`, `function`...).
pub(crate) fn builtin_getset_descriptor(name: &'static str, owner: Value) -> Value {
    Value::Ext(Rc::new(GetSetDescriptor { name, owner }))
}

impl GetSetDescriptor {
    fn owner_name(&self) -> String {
        match &self.owner {
            Value::Class(c) => c.name.clone(),
            other => crate::builtins::class_name(other).map_or_else(|| other.type_name().to_string(), str::to_string),
        }
    }

    /// `range.start`, `slice.start` e os campos de dados das exceções (`start`, `errno`, `name`...) são
    /// `member_descriptor` (o oráculo diz quais, em `builtin-type-var-kinds.tsv`); o resto, `getset_descriptor`.
    /// Uma classe de usuário nunca consulta a tabela; só o shim de um tipo de `builtins` (`memoryview`).
    fn is_member(&self) -> bool {
        let from_table = match &self.owner {
            Value::Class(c) => c.emulates_c_type() && c.module() == "builtins",
            _ => true,
        };
        matches!(self.name, "start" | "stop" | "step")
            || (from_table && crate::builtins_ext::type_var_kind(&self.owner_name(), self.name) == Some("member_descriptor"))
    }
}

impl ExtObject for GetSetDescriptor {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("getset_descriptor", self.name, vec![self.owner.clone()])
    }
    fn type_name(&self) -> &'static str {
        if self.is_member() {
            "member_descriptor"
        } else {
            "getset_descriptor"
        }
    }
    fn repr(&self) -> String {
        let kind = if self.is_member() { "member" } else { "attribute" };
        format!("<{kind} '{}' of '{}' objects>", self.name, self.owner_name())
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__get__", "__set__", "__delete__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" | "__qualname__" => Some(Ok(Value::str(if name == "__name__" {
                self.name.to_string()
            } else {
                format!("{}.{}", self.owner_name(), self.name)
            }))),
            "__objclass__" => Some(Ok(self.owner.clone())),
            "__doc__" => Some(Ok(match self.name {
                "__dict__" => Value::str("dictionary for instance variables"),
                "__weakref__" => Value::str("list of weak references to the object"),
                "real" => Value::str("the real part of a complex number"),
                "imag" => Value::str("the imaginary part of a complex number"),
                "numerator" => Value::str("the numerator of a rational number in lowest terms"),
                "denominator" => Value::str("the denominator of a rational number in lowest terms"),
                _ => Value::None,
            })),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match (name, args.as_slice()) {
            ("__get__", [Value::None, ..]) => Ok(Value::Ext(Rc::new(GetSetDescriptor {
                name: self.name,
                owner: self.owner.clone(),
            }))),
            ("__get__", [obj, ..]) if self.name != "__weakref__" => vm.load_attr(obj, self.name),
            ("__get__", [_, ..]) => Ok(Value::None),
            ("__set__", [obj, value]) if self.name == "__dict__" => {
                vm.store_attr(obj, "__dict__", value.clone())?;
                Ok(Value::None)
            }
            ("__set__" | "__delete__", _) if self.is_member() => Err(exc("AttributeError", "readonly attribute")),
            ("__set__" | "__delete__", _) => Err(exc("AttributeError", format!("attribute '{}' of '{}' objects is not writable", self.name, self.owner_name()))),
            _ => Err(type_error(format!("expected at least 1 argument, got {}", args.len()))),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Descritores

/// `member_descriptor` de um nome de `__slots__`: o `type_new` põe um na classe por nome. O valor mora
/// no espaço próprio da instância, que a gravação já confere contra os `__slots__` da herança.
struct MemberDescriptor {
    name: String,
    owner: Rc<ClassObj>,
}

impl MemberDescriptor {
    /// A instância de que o descritor lê e grava: o `member_get` recusa o que não for do dono.
    fn target<'a>(&self, obj: &'a Value) -> PyResult<&'a Rc<InstanceObj>> {
        match obj {
            Value::Instance(i) if i.class().mro().iter().any(|c| Rc::ptr_eq(c, &self.owner)) => Ok(i),
            other => Err(type_error(format!(
                "descriptor '{}' for '{}' objects doesn't apply to a '{}' object",
                self.name,
                self.owner.name,
                other.type_name()
            ))),
        }
    }
}

impl ExtObject for MemberDescriptor {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("member_descriptor", self.name.clone(), vec![Value::Class(self.owner.clone())])
    }
    fn type_name(&self) -> &'static str {
        "member_descriptor"
    }
    fn repr(&self) -> String {
        format!("<member '{}' of '{}' objects>", self.name, self.owner.name)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__get__", "__set__", "__delete__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" => Some(Ok(Value::str(self.name.clone()))),
            "__qualname__" => Some(Ok(Value::str(format!("{}.{}", self.owner.qualname(), self.name)))),
            "__objclass__" => Some(Ok(Value::Class(self.owner.clone()))),
            "__doc__" => Some(Ok(Value::None)),
            _ => None,
        }
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match (name, args.as_slice()) {
            ("__get__", [Value::None, ..]) => Ok(member_descriptor(self.name.clone(), self.owner.clone())),
            ("__get__", [obj, ..]) => {
                let inst = self.target(obj)?;
                let found = inst.dict.borrow().get(&self.name).cloned();
                found.ok_or_else(|| crate::object::no_attribute(&inst.class().name, &self.name))
            }
            ("__set__", [obj, value]) => {
                self.target(obj)?.set_own(&self.name, value.clone());
                Ok(Value::None)
            }
            ("__delete__", [obj]) => {
                let inst = self.target(obj)?;
                inst.remove_own(&self.name)
                    .map(|_| Value::None)
                    .ok_or_else(|| slot_delete_error(&inst, &self.name))
            }
            _ => Err(type_error(format!("expected at least 1 argument, got {}", args.len()))),
        }
    }
}

/// O `member_descriptor` de `name` na classe `owner`.
fn member_descriptor(name: String, owner: Rc<ClassObj>) -> Value {
    Value::Ext(Rc::new(MemberDescriptor { name, owner }))
}

/// `cls.lookup(name)` como o programa a enxerga: o `__slots__` que um shim de tipo em C usa para
/// esconder o próprio estado não existe no tipo real, então a busca não o devolve.
fn lookup_public(cls: &Rc<ClassObj>, name: &str) -> Option<Value> {
    // `__qualname__` é um getset de `type`: a instância não o enxerga, mesmo que o `plain` de um módulo embutido o
    // tenha gravado no dict da classe.
    if name == "__qualname__" {
        return None;
    }
    if name == "__slots__" {
        let owner = cls.mro().into_iter().find(|c| c.dict.borrow().contains_key("__slots__"));
        if owner.is_some_and(|c| c.emulates_c_type()) {
            return None;
        }
    }
    cls.lookup(name)
}

/// `true` se `name` é um campo de `__slots__` de alguma classe da herança (não de um shim de tipo em C).
fn is_slot_member(cls: &Rc<ClassObj>, name: &str) -> bool {
    cls.mro().iter().any(|c| {
        !c.emulates_c_type() && c.declared_slots().is_some_and(|slots| slots.iter().any(|s| s == name))
    })
}

/// O `AttributeError` de `setattr`/`delattr` sobre um tipo sem `__dict__` (`PyObject_GenericSetAttr`).
fn no_dict_error(type_name: &str, name: &str) -> PyException {
    exc("AttributeError", format!("'{type_name}' object has no attribute '{name}' and no __dict__ for setting new attributes"))
}

/// O erro de apagar o campo de um slot sem valor: o `member_delete` só diz o nome.
fn slot_delete_error(inst: &InstanceObj, name: &str) -> PyException {
    if is_slot_member(&inst.class(), name) {
        exc("AttributeError", name)
    } else {
        crate::object::no_attribute(&inst.class().name, name)
    }
}

/// Os descritores que o `type_new` cria para os nomes do `__slots__` da classe, ordenados como ele
/// os ordena. Shim de tipo em C não tem nenhum: o estado dele não existe para o programa.
fn slot_members(cls: &Rc<ClassObj>) -> Vec<(String, Value)> {
    if cls.emulates_c_type() {
        return Vec::new();
    }
    let mut names = cls.declared_slots().unwrap_or_default();
    names.retain(|n| n != "__dict__" && n != "__weakref__");
    names.sort();
    names.dedup();
    names.into_iter().map(|n| (n.clone(), member_descriptor(n, cls.clone()))).collect()
}

struct StaticMethod(Value);

/// Os atributos que o `classmethod` e o `staticmethod` do 3.13 expõem: `__func__` e `__wrapped__` são a função
/// envolvida, `__isabstractmethod__` vem dela, e `__module__`, `__name__`, `__qualname__` e `__doc__` são copiados
/// dela na criação (o `functools_wraps` do `funcobject.c`).
fn wrapped_function_attr(vm: &mut Vm, inner: &Value, name: &str) -> Option<PyResult<Value>> {
    match name {
        "__func__" | "__wrapped__" => Some(Ok(inner.clone())),
        "__isabstractmethod__" => {
            Some(Ok(Value::Bool(vm.load_attr(inner, "__isabstractmethod__").is_ok_and(|v| v.is_true()))))
        }
        "__module__" | "__name__" | "__qualname__" | "__doc__" => vm.load_attr(inner, name).ok().map(Ok),
        // O `__dict__` guarda só o que a criação copiou da função.
        "__dict__" => Some((|vm: &mut Vm| -> PyResult<Value> {
            let mut copied = crate::object::Dict::default();
            for attr in ["__module__", "__name__", "__qualname__", "__doc__"] {
                if let Ok(v) = vm.load_attr(inner, attr) {
                    copied.set(Value::str(attr), v)?;
                }
            }
            Ok(Value::dict(copied))
        })(vm)),
        _ => None,
    }
}

/// `__get__` do `classmethod` e do `staticmethod` chamado como método (`cm.__get__(obj, owner)`): a função
/// envolvida, ligada à classe (`owner`, ou o tipo de `obj`) no caso do `classmethod`.
fn wrapped_function_get(vm: &mut Vm, inner: &Value, bind_class: bool, args: &[Value]) -> PyResult<Value> {
    let owner = match (args, bind_class) {
        ([], _) | ([_, _, _, ..], _) => {
            return Err(type_error(format!("expected 1 or 2 arguments, got {}", args.len())));
        }
        (_, false) => return Ok(inner.clone()),
        ([_, owner], true) if !matches!(owner, Value::None) => owner.clone(),
        ([obj, ..], true) => vm.type_of(obj),
    };
    Ok(match (inner, owner) {
        (Value::Function(f), owner @ Value::Class(_)) => Value::BoundFn(Rc::new((owner, f.clone()))),
        _ => inner.clone(),
    })
}

impl ExtObject for StaticMethod {
    fn type_name(&self) -> &'static str {
        "staticmethod"
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        wrapped_function_attr(vm, &self.0, name)
    }
    fn descriptor(&self) -> Option<Descriptor> {
        Some(Descriptor::Static(self.0.clone()))
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::StaticMethod(self.0.clone()))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__", "__get__"]
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        // Desde o 3.10 o `staticmethod` é chamável e repassa a chamada à função envolvida.
        match name {
            "__call__" => vm.call(&self.0, args, kw),
            "__get__" => wrapped_function_get(vm, &self.0, false, &args),
            _ => Err(crate::object::no_attribute("staticmethod", name)),
        }
    }
}

struct ClassMethod(Value);

impl ExtObject for ClassMethod {
    fn type_name(&self) -> &'static str {
        "classmethod"
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        wrapped_function_attr(vm, &self.0, name)
    }
    fn descriptor(&self) -> Option<Descriptor> {
        Some(Descriptor::Class(self.0.clone()))
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::ClassMethod(self.0.clone()))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__get__"]
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__get__" => wrapped_function_get(vm, &self.0, true, &args),
            _ => Err(crate::object::no_attribute("classmethod", name)),
        }
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
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::Property {
            get: self.get.clone(),
            set: self.set.clone(),
            del: self.del.clone(),
            doc: self.doc.borrow().clone(),
        })
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["setter", "getter", "deleter", "__get__", "__set__", "__delete__", "__set_name__"]
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__isabstractmethod__" => {
                let abstract_fn = |vm: &mut Vm, f: &Value| {
                    vm.load_attr(f, "__isabstractmethod__").is_ok_and(|v| v.is_true())
                };
                let any = abstract_fn(vm, &self.get)
                    || self.set.as_ref().is_some_and(|f| abstract_fn(vm, f))
                    || self.del.as_ref().is_some_and(|f| abstract_fn(vm, f));
                Some(Ok(Value::Bool(any)))
            }
            "__doc__" => {
                // Sem `doc=`, o docstring vem do getter, como no CPython.
                let own = self.doc.borrow().clone();
                if matches!(own, Value::None) {
                    // Sem getter (`property()`), não há de onde tirar o docstring: o `None.__doc__` é o do
                    // tipo `NoneType`, que não pode vazar como docstring da propriedade.
                    if matches!(self.get, Value::None) {
                        return Some(Ok(Value::None));
                    }
                    return Some(Ok(vm.load_attr(&self.get, "__doc__").unwrap_or(Value::None)));
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
                return vm.call(&self.get, vec![obj], kw);
            }
            "__set__" => {
                let [obj, value] = <[Value; 2]>::try_from(args)
                    .map_err(|a| type_error(format!("expected 2 arguments, got {}", a.len())))?;
                let Some(set) = &self.set else { return Err(exc("AttributeError", "property has no setter")) };
                vm.call(set, vec![obj, value], kw)?;
                return Ok(Value::None);
            }
            // O `type_new` chama `__set_name__(dono, nome)` em todo descritor do corpo; a propriedade só
            // usa o nome nas mensagens de erro, que aqui o procuram na classe.
            "__set_name__" => {
                if args.len() != 2 {
                    return Err(type_error(format!("expected 2 arguments, got {}", args.len())));
                }
                return Ok(Value::None);
            }
            "__delete__" => {
                let [obj] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("expected 1 argument, got {}", a.len())))?;
                let Some(del) = &self.del else { return Err(exc("AttributeError", "property has no deleter")) };
                vm.call(del, vec![obj], kw)?;
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

/// `fget`, `fset` e `fdel` de um `property` ou de uma instância de subclasse dele (o valor embutido
/// da subclasse é um `Property` guardado no payload). `None` para qualquer outro valor.
pub(crate) fn property_parts(v: &Value) -> Option<(Value, Option<Value>, Option<Value>)> {
    let inner = match v {
        Value::Instance(i) if i.class().data_base == Some("property") => i.payload.borrow().clone()?,
        Value::Instance(_) => return None,
        other => other.clone(),
    };
    let Value::Ext(e) = inner else { return None };
    match e.descriptor() {
        Some(Descriptor::Property { get, set, del }) => Some((get, set, del)),
        _ => None,
    }
}

/// Instância de uma subclasse de `property` (o descritor vive no payload).
pub(crate) fn is_property_instance(i: &InstanceObj) -> bool {
    i.class().data_base == Some("property")
}

/// O nome com que o atributo aparece na classe (ou numa das bases), para as mensagens do `property`.
fn attr_name_in(cls: &Rc<ClassObj>, attr: &Value) -> Option<String> {
    cls.mro().iter().find_map(|c| {
        c.dict.borrow().iter().find(|(_, v)| crate::object::is(v, attr)).map(|(k, _)| k.clone())
    })
}

/// `property 'x' of 'A' object has no setter` (o 3.13 omite o nome quando não o conhece).
fn property_missing(cls: &Rc<ClassObj>, attr: &Value, what: &str) -> PyException {
    let owner = &cls.name;
    match attr_name_in(cls, attr) {
        Some(n) => exc("AttributeError", format!("property '{n}' of '{owner}' object has no {what}")),
        None => exc("AttributeError", format!("property of '{owner}' object has no {what}")),
    }
}

/// `getter`/`setter`/`deleter` de uma subclasse de `property`: o `property_copy` do CPython chama
/// `type(self)(fget, fset, fdel, doc)`, então o resultado é da subclasse.
struct PropertyCopy {
    obj: Value,
    which: &'static str,
}

impl ExtObject for PropertyCopy {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::PropertyCopy { obj: self.obj.clone(), which: self.which })
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let [f] = <[Value; 1]>::try_from(args)
            .map_err(|a| type_error(format!("{}() takes exactly one argument ({} given)", self.which, a.len())))?;
        if !kw.is_empty() {
            return Err(type_error(format!("{}() takes no keyword arguments", self.which)));
        }
        let Value::Instance(inst) = &self.obj else { return Err(type_error("not a property")) };
        let (mut get, mut set, mut del) = property_parts(&self.obj).unwrap_or((Value::None, None, None));
        match self.which {
            "setter" => set = Some(f),
            "deleter" => del = Some(f),
            _ => get = f,
        }
        let doc = match inst.payload.borrow().as_ref() {
            Some(Value::Ext(e)) => {
                e.as_any().and_then(|a| a.downcast_ref::<Property>()).map_or(Value::None, |p| p.doc.borrow().clone())
            }
            _ => Value::None,
        };
        let args = vec![get, set.unwrap_or(Value::None), del.unwrap_or(Value::None), doc];
        vm.call(&Value::Class(inst.class()), args, Vec::new())
    }
}

/// `slice.indices(len)`: `(start, stop, step)` ajustados a um comprimento, como `PySlice_AdjustIndices`.
pub(crate) struct SliceIndices(pub Rc<(Value, Value, Value)>);

impl ExtObject for SliceIndices {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("slice_indices", (), vec![Value::Slice(self.0.clone())])
    }
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn repr(&self) -> String {
        crate::typeattrs::bound_method_repr(&Value::Slice(self.0.clone()), "indices")
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        crate::typeattrs::bound_method_attr(&Value::Slice(self.0.clone()), "indices", name).map(Ok)
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
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("subclasses_call", (), self.0.clone())
    }
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
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("alt_ctor", (), vec![self.cls.clone(), self.inner.clone()])
    }
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let base = vm.call(&self.inner, args, kw)?;
        vm.call(&self.cls, vec![base], Vec::new())
    }
}

/// `exit` de um `with` sobre arquivo: fechar o arquivo.
struct FileExit(Value);

impl ExtObject for FileExit {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("file_exit", (), vec![self.0.clone()])
    }
    fn type_name(&self) -> &'static str {
        "method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let close = vm.load_attr(&self.0, "close")?;
        vm.call(&close, Vec::new(), Vec::new())?;
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
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::PlainObject)
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
    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
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
            _ => Err(crate::object::no_attribute("object", name)),
        }
    }
    fn hash_value(&self) -> Option<i64> {
        Some(crate::object::py_addr_hash(self as *const PlainObject as usize))
    }
}

/// A célula `__class__` que o corpo de uma classe passa em `__classcell__`: o escopo que os métodos
/// capturam, onde `type.__new__` grava a classe criada.
struct ClassCell(Rc<Env>);

/// O valor `cell` que guarda o escopo `env`.
pub(crate) fn cell_value(env: Rc<Env>) -> Value {
    Value::Ext(Rc::new(ClassCell(env)))
}

/// O escopo que o valor `cell` guarda, se `v` for uma.
pub(crate) fn cell_env(v: &Value) -> Option<Rc<Env>> {
    match v {
        Value::Ext(e) => e.as_any().and_then(|a| a.downcast_ref::<ClassCell>()).map(|c| c.0.clone()),
        _ => None,
    }
}

impl ExtObject for ClassCell {
    fn type_name(&self) -> &'static str {
        "cell"
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::ClassCell(self.0.clone()))
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
}

/// Refaz um objeto nativo a partir da imagem do heap (o inverso de [`ExtObject::image`]).
/// `None` para as imagens que outro módulo refaz (iteradores, geradores, `weakref`, `Opaque`: ver
/// `heapimage`), porque elas precisam do contexto da reconstrução.
pub(crate) fn ext_from_image(image: ExtImage) -> Option<Value> {
    Some(match image {
        ExtImage::StaticMethod(f) => Value::Ext(Rc::new(StaticMethod(f))),
        ExtImage::ClassMethod(f) => Value::Ext(Rc::new(ClassMethod(f))),
        ExtImage::Property { get, set, del, doc } => {
            Value::Ext(Rc::new(Property { get, set, del, doc: RefCell::new(doc) }))
        }
        ExtImage::PropertyCopy { obj, which } => Value::Ext(Rc::new(PropertyCopy { obj, which })),
        ExtImage::PlainObject => Value::Ext(Rc::new(PlainObject)),
        ExtImage::ClassCell(env) => Value::Ext(Rc::new(ClassCell(env))),
        ExtImage::Lazy(_)
        | ExtImage::Generator { .. }
        | ExtImage::AsyncGenWrapped(_)
        | ExtImage::WeakRef { .. }
        | ExtImage::Opaque(_)
        | ExtImage::CodeObject { .. }
        | ExtImage::CodeSource { .. }
        | ExtImage::Frame(_) => return None,
    })
}

/// Refaz os objetos de método e de descritor desta unidade a partir da imagem do heap (o inverso de
/// `ExtObject::image` de cada um); o `tag` diz qual deles é.
pub(crate) fn restore_image(tag: &str, state: &(dyn std::any::Any + Send + Sync), refs: Vec<Value>) -> Option<Value> {
    let name_pair = || state.downcast_ref::<(&'static str, &'static str)>().copied();
    let name = || state.downcast_ref::<&'static str>().copied();
    let mut refs = refs.into_iter();
    let ext: Rc<dyn ExtObject> = match tag {
        "native_type_method" => {
            let (owner, name) = name_pair()?;
            Rc::new(NativeTypeMethod { owner, name })
        }
        "getset_descriptor" => match refs.next()? {
            owner @ (Value::Class(_) | Value::Builtin(_)) => Rc::new(GetSetDescriptor { name: name()?, owner }),
            _ => return None,
        },
        "member_descriptor" => match refs.next()? {
            Value::Class(owner) => Rc::new(MemberDescriptor { name: state.downcast_ref::<String>()?.clone(), owner }),
            _ => return None,
        },
        "slice_indices" => match refs.next()? {
            Value::Slice(s) => Rc::new(SliceIndices(s)),
            _ => return None,
        },
        "subclasses_call" => Rc::new(SubclassesCall(refs.collect())),
        "alt_ctor" => Rc::new(AltCtor { cls: refs.next()?, inner: refs.next()? }),
        "file_exit" => Rc::new(FileExit(refs.next()?)),
        "exc_with_traceback" => Rc::new(ExcWithTraceback { obj: refs.next()? }),
        "exc_add_note" => Rc::new(ExcAddNote { obj: refs.next()? }),
        "shim_method" => Rc::new(ShimMethod { recv: refs.next()?, func: refs.next()?, name: name()? }),
        "instance_dunder" => Rc::new(InstanceDunder { obj: refs.next()?, name: name()? }),
        "builtin_super_method" => Rc::new(BuiltinSuperMethod { obj: refs.next()?, name: name()? }),
        "plain_object_method" => Rc::new(PlainObjectMethod { obj: refs.next()?, name: name()? }),
        "object_class_method" => return Some(object_class_method(refs.next()?, name()?)),
        "super_proxy" => match (refs.next()?, refs.next()?) {
            (obj, Value::Class(cls)) => Rc::new(SuperProxy { obj, cls }),
            _ => return None,
        },
        _ => return None,
    };
    Some(Value::Ext(ext))
}

/// Resultado de `super()`: procura o atributo nas classes depois de `cls` na ordem de herança.
struct SuperProxy {
    obj: Value,
    cls: Rc<ClassObj>,
}

impl ExtObject for SuperProxy {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("super_proxy", (), vec![self.obj.clone(), Value::Class(self.cls.clone())])
    }
    fn type_name(&self) -> &'static str {
        "super"
    }
    fn repr(&self) -> String {
        // Como o `super_repr` do CPython: a classe e o nome do tipo do receptor.
        format!("<super: <class '{}'>, <{} object>>", self.cls.name, receiver_type_name(&self.obj))
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
        let mro = inst.class().mro();
        let start = mro.iter().position(|c| Rc::ptr_eq(c, &self.cls)).map_or(0, |i| i + 1);
        for c in &mro[start..] {
            let attr = c.dict.borrow().get(name).cloned();
            if let Some(attr) = attr {
                return Some(vm.bind_class_attr(&attr, self.obj.clone(), &inst.class()));
            }
        }
        // Métodos herdados das classes embutidas (`object`, `Exception`).
        Some(Ok(Value::Ext(Rc::new(BuiltinSuperMethod { obj: self.obj.clone(), name: intern(name) }))))
    }
}

/// `exc.with_traceback(tb)`: grava `__traceback__` e devolve a própria exceção.
pub(crate) struct ExcWithTraceback {
    pub(crate) obj: Value,
}

impl ExtObject for ExcWithTraceback {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("exc_with_traceback", (), vec![self.obj.clone()])
    }
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn repr(&self) -> String {
        crate::typeattrs::bound_method_repr(&self.obj, "with_traceback")
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        crate::typeattrs::bound_method_attr(&self.obj, "with_traceback", name).map(Ok)
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

/// Método de um tipo de `builtins` escrito em Python aqui (`memoryview.cast`, `complex.__eq__`) ligado ao objeto:
/// no CPython é um método embutido, ou o `method-wrapper` de um slot, e não o `method` de uma função. A regra é
/// geral (a função vem de um shim cujo módulo se chama `builtins`, ver `Function::is_builtin_type_method`);
/// a chamada vai para a função de verdade.
struct ShimMethod {
    recv: Value,
    func: Value,
    name: &'static str,
}

impl ExtObject for ShimMethod {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("shim_method", self.name, vec![self.recv.clone(), self.func.clone()])
    }
    fn type_name(&self) -> &'static str {
        if crate::typeattrs::is_slot_wrapper(self.recv.type_name(), self.name) {
            "method-wrapper"
        } else {
            "builtin_function_or_method"
        }
    }
    fn repr(&self) -> String {
        crate::typeattrs::bound_method_repr(&self.recv, self.name)
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        crate::typeattrs::bound_method_attr(&self.recv, self.name, name).map(Ok)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(self.recv.clone());
        full.extend(args);
        vm.call(&self.func, full, kw)
    }
}

/// `exc.add_note(texto)`: anexa a `exc.__notes__`, criando a lista na primeira nota.
pub(crate) struct ExcAddNote {
    pub(crate) obj: Value,
}

impl ExtObject for ExcAddNote {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("exc_add_note", (), vec![self.obj.clone()])
    }
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn repr(&self) -> String {
        crate::typeattrs::bound_method_repr(&self.obj, "add_note")
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        crate::typeattrs::bound_method_attr(&self.obj, "add_note", name).map(Ok)
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
        let notes = match vm.load_attr(&self.obj, "__notes__") {
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
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("instance_dunder", self.name, vec![self.obj.clone()])
    }
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

/// `x.__iadd__(y)` de subclasse de tipo embutido: o método do valor embutido, devolvendo a própria
/// instância quando ele devolve o valor embutido (`list.__iadd__` devolve `self`).
struct InplaceMethod {
    obj: Value,
    payload: Value,
    inner: Value,
}

impl ExtObject for InplaceMethod {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("inplace_method", (), vec![self.obj.clone(), self.payload.clone(), self.inner.clone()])
    }
    fn type_name(&self) -> &'static str {
        "method-wrapper"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let result = vm.call(&self.inner, args, kw)?;
        Ok(if crate::object::is(&result, &self.payload) { self.obj.clone() } else { result })
    }
}

/// Método de `object()` simples (`object().__delattr__`): o nativo de
/// `typeattrs::object_attr` com o receptor na frente.
struct PlainObjectMethod {
    obj: Value,
    name: &'static str,
}

impl ExtObject for PlainObjectMethod {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("plain_object_method", self.name, vec![self.obj.clone()])
    }
    fn type_name(&self) -> &'static str {
        object_method_type_name(self.name)
    }
    fn repr(&self) -> String {
        bound_object_method_repr(&self.obj, self.name)
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        bound_object_method_attr(&self.obj, self.name, name)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let copyreg_method = match self.name {
            "__setstate__" => Some("_exception_setstate"),
            "__getstate__" => Some("_exception_getstate"),
            _ => None,
        };
        let native = if let Some(fname) = copyreg_method {
            // Os métodos de exceção que não são o `__reduce__` de `object`: a lógica vive em `copyreg`.
            let copyreg = crate::modules::import_checked(vm, "copyreg")?;
            Some(vm.load_attr(&Value::Module(copyreg), fname)?)
        } else {
            crate::typeattrs::object_attr(self.name)
        };
        let Some(native) = native else {
            return Err(crate::object::no_attribute("object", self.name));
        };
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(self.obj.clone());
        full.extend(args);
        vm.call(&native, full, kw)
    }
}

/// O método de `object` que um `object()` simples herda (o que `PlainObject` não atende por conta própria).
pub(crate) fn plain_object_method(obj: &Value, name: &str) -> Option<Value> {
    const NAMES: &[&str] = &[
        "__setattr__", "__delattr__", "__getattribute__", "__reduce__", "__reduce_ex__", "__dir__", "__lt__",
        "__le__", "__gt__", "__ge__",
    ];
    let name = NAMES.iter().find(|n| **n == name)?;
    Some(Value::Ext(Rc::new(PlainObjectMethod { obj: obj.clone(), name })))
}

/// `__reduce__`, `__reduce_ex__` e `__setstate__` de uma exceção, como método embutido ligado a ela
/// (o `__setstate__` é de `BaseException`; os outros dois são os de `object`, que despacham para o
/// `__reduce__` do tipo).
pub(crate) fn exception_method(obj: &Value, name: &str) -> Option<Value> {
    let name = ["__reduce__", "__reduce_ex__", "__setstate__", "__getstate__"].into_iter().find(|n| *n == name)?;
    Some(Value::Ext(Rc::new(PlainObjectMethod { obj: obj.clone(), name })))
}

/// O nome do tipo do receptor, o `tp_name` que o `repr` dos métodos ligados do CPython imprime.
fn receiver_type_name(recv: &Value) -> String {
    match recv {
        Value::Instance(i) => i.class().name.clone(),
        Value::Class(c) => c.meta.as_ref().map_or_else(|| "type".to_string(), |m| m.name.clone()),
        Value::Builtin(_) | Value::NativeFn(_) if crate::builtins::class_name(recv).is_some() => "type".to_string(),
        other => other.type_name().to_string(),
    }
}

/// O `__qualname__` do tipo que `meth_get__qualname__` usa: o próprio receptor quando ele é um tipo.
fn receiver_qualname(recv: &Value) -> String {
    match recv {
        Value::Instance(i) => i.class().qualname(),
        Value::Class(c) => c.qualname(),
        Value::Builtin(_) | Value::NativeFn(_) => match crate::builtins::class_name(recv) {
            Some(n) => n.to_string(),
            // Um tipo nativo registrado (`list_iterator`): o nome dele, não o `type`.
            None => match recv {
                Value::Builtin(n) if crate::object::is_builtin_type(n) => (*n).to_string(),
                _ => recv.type_name().to_string(),
            },
        },
        other => other.type_name().to_string(),
    }
}

/// Os métodos de `object` que o CPython guarda como função embutida ligada (`builtin_function_or_method`):
/// os de classe, o `__new__` e os `method_descriptor`; os slots viram `method-wrapper`.
fn object_method_type_name(name: &str) -> &'static str {
    if matches!(name, "__init_subclass__" | "__subclasshook__" | "__new__") || !crate::typeattrs::is_slot_wrapper("object", name) {
        "builtin_function_or_method"
    } else {
        "method-wrapper"
    }
}

/// O `repr` de um método de `object` ligado a `recv`, como o `meth_repr` e o `wrapper_repr` do CPython.
fn bound_object_method_repr(recv: &Value, name: &str) -> String {
    let (at, ty) = (crate::builtins::id_of(recv), receiver_type_name(recv));
    if object_method_type_name(name) == "method-wrapper" {
        format!("<method-wrapper '{name}' of {ty} object at {at:#x}>")
    } else {
        format!("<built-in method {name} of {ty} object at {at:#x}>")
    }
}

/// `__self__`, `__name__`, `__qualname__`, `__text_signature__`, `__doc__` (e `__objclass__`, nos wrappers)
/// de um método de `object` ligado a `recv`.
fn bound_object_method_attr(recv: &Value, name: &str, attr: &str) -> Option<PyResult<Value>> {
    let wrapper = object_method_type_name(name) == "method-wrapper";
    // As exceções herdam também os métodos de `BaseException` (`__setstate__`): a cadeia parte do tipo delas.
    let owner = if let Value::Exception(e) = recv { e.kind } else { "object" };
    match attr {
        "__self__" => Some(Ok(recv.clone())),
        "__name__" => Some(Ok(Value::str(name))),
        // O wrapper guarda o tipo dono (`object`); a função embutida usa o tipo do receptor.
        "__qualname__" if wrapper => Some(Ok(Value::str(format!("object.{name}")))),
        "__qualname__" => Some(Ok(Value::str(format!("{}.{name}", receiver_qualname(recv))))),
        "__objclass__" if wrapper => Some(Ok(Value::Builtin("object"))),
        "__module__" if !wrapper => Some(Ok(Value::None)),
        "__text_signature__" => {
            Some(Ok(crate::typeattrs::text_signature(owner, name).map_or(Value::None, Value::str)))
        }
        "__doc__" => Some(Ok(crate::typeattrs::method_doc(owner, name).map_or(Value::None, Value::str))),
        _ => None,
    }
}

/// `object.__init_subclass__`, `__subclasshook__` e `__new__` ligados ao tipo `owner`: função embutida cujo
/// `__self__` é o tipo (`object` para o `__new__` e para a leitura direta em `object`).
struct ObjectClassMethod {
    owner: Value,
    name: &'static str,
    f: crate::object::NativeFnPtr,
}

impl ExtObject for ObjectClassMethod {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("object_class_method", self.name, vec![self.owner.clone()])
    }
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn repr(&self) -> String {
        bound_object_method_repr(&self.owner, self.name)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        bound_object_method_attr(&self.owner, self.name, name)
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        (self.f)(vm, args, kw)
    }
}

thread_local! {
    static OBJECT_CLASS_METHODS: RefCell<Vec<(&'static str, Value)>> = const { RefCell::new(Vec::new()) };
}

/// O método de classe (ou o `__new__`) de `object` ligado a `owner`. Ligado ao próprio `object` é um objeto
/// só por nome, como o `tp_new_wrapper` do CPython (`cls.__new__ is object.__new__`).
pub(crate) fn object_class_method(owner: Value, name: &'static str) -> Value {
    let Some(f) = crate::typeattrs::object_class_native(name) else { return Value::None };
    let make = |owner: Value| Value::Ext(Rc::new(ObjectClassMethod { owner, name, f }));
    if !matches!(&owner, Value::Builtin("object")) {
        return make(owner);
    }
    OBJECT_CLASS_METHODS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((_, v)) = m.iter().find(|(n, _)| *n == name) {
            return v.clone();
        }
        let v = make(owner);
        m.push((name, v.clone()));
        v
    })
}

/// Métodos de `object` que uma instância sem valor embutido herda e que o `BuiltinSuperMethod` atende
/// repassando ao atributo de mesmo nome de `object` (`typeattrs::object_attr`).
const OBJECT_INSTANCE_METHODS: &[&str] = &[
    "__dir__", "__eq__", "__format__", "__ge__", "__gt__", "__hash__", "__le__", "__lt__", "__ne__", "__repr__",
    "__sizeof__", "__str__",
];

/// Método de `object`/`BaseException` alcançado por `super().nome`.
struct BuiltinSuperMethod {
    obj: Value,
    name: &'static str,
}

impl ExtObject for BuiltinSuperMethod {
    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("builtin_super_method", self.name, vec![self.obj.clone()])
    }
    fn type_name(&self) -> &'static str {
        object_method_type_name(self.name)
    }
    fn repr(&self) -> String {
        bound_object_method_repr(&self.obj, self.name)
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        bound_object_method_attr(&self.obj, self.name, name)
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
                        Ok(Value::Instance(InstanceObj::new_rc(c, payload)))
                    }
                    _ => Err(type_error("type.__new__() takes exactly 3 arguments")),
                },
                "__init__" => Ok(Value::None),
                "__subclasshook__" => match crate::typeattrs::object_attr("__subclasshook__") {
                    Some(native) => {
                        let mut full = vec![self.obj.clone()];
                        full.extend(args);
                        vm.call(&native, full, kw)
                    }
                    None => Err(exc("AttributeError", "'super' object has no attribute '__subclasshook__'")),
                },
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
                if inst.class().builtin_base.is_some() {
                    inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(args));
                    return Ok(Value::None);
                }
                if inst.class().data_base == Some("module") {
                    init_module_fields(inst, &args, &kw)?;
                    return Ok(Value::None);
                }
                // `super().__init__(fget, doc=doc)` de subclasse de `property`: refaz o descritor
                // guardado e, como o `property_init`, fixa o `__doc__` na instância.
                if is_property_instance(inst) {
                    let fresh = vm.call(&Value::Builtin("property"), args, kw)?;
                    let doc = vm.load_attr(&fresh, "__doc__")?;
                    *inst.payload.borrow_mut() = Some(fresh);
                    inst.dict.borrow_mut().insert("__doc__".to_string(), doc);
                    return Ok(Value::None);
                }
                // `super().__init__(...)` de subclasse de `dict`/`list`/`set`: preenche o valor embutido.
                let payload = inst.payload.borrow().clone();
                if let Some(p @ (Value::Dict(_) | Value::List(_) | Value::Set(_))) = payload {
                    let fresh = vm.call(&data_ctor(inst.class().data_base.unwrap_or("dict")), args, kw)?;
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
            "__eq__" | "__ne__" | "__format__" | "__lt__" | "__le__" | "__gt__" | "__ge__" | "__sizeof__" | "__dir__"
                if inst.payload.borrow().is_none() =>
            {
                let Some(native) = crate::typeattrs::object_attr(self.name) else {
                    return Err(exc("AttributeError", format!("'super' object has no attribute '{}'", self.name)));
                };
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(self.obj.clone());
                full.extend(args);
                vm.call(&native, full, kw)
            }
            "__hash__" => Ok(Value::Int(crate::object::hash(&self.obj)?)),
            "__setattr__" => {
                if let [Value::Str(n), v] = args.as_slice() {
                    if n.as_str() == "__class__" {
                        assign_instance_class(inst, v)?;
                    } else {
                        inst.dict.borrow_mut().insert(n.as_str().to_string(), v.clone());
                    }
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
            // `property.__get__(None, tipo)` devolve o próprio descritor, que aqui é a instância da subclasse.
            "__get__" if is_property_instance(inst) && matches!(args.first(), Some(Value::None)) => Ok(self.obj.clone()),
            "__get__" if is_property_instance(inst) => {
                let (get, ..) = property_parts(&self.obj).unwrap_or((Value::None, None, None));
                let Some(target) = args.into_iter().next() else {
                    return Err(type_error("expected 1 or 2 arguments, got 0"));
                };
                if matches!(get, Value::None) {
                    return Err(property_missing(&inst.class(), &self.obj, "getter"));
                }
                vm.call(&get, vec![target], kw)
            }
            "__set__" if is_property_instance(inst) => {
                let [target, value] = <[Value; 2]>::try_from(args)
                    .map_err(|a| type_error(format!("expected 2 arguments, got {}", a.len())))?;
                let (_, set, _) = property_parts(&self.obj).unwrap_or((Value::None, None, None));
                let Some(f) = set else { return Err(property_missing(&inst.class(), &self.obj, "setter")) };
                vm.call(&f, vec![target, value], kw)?;
                Ok(Value::None)
            }
            "__delete__" if is_property_instance(inst) => {
                let [target] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("expected 1 argument, got {}", a.len())))?;
                let (_, _, del) = property_parts(&self.obj).unwrap_or((Value::None, None, None));
                let Some(f) = del else { return Err(property_missing(&inst.class(), &self.obj, "deleter")) };
                vm.call(&f, vec![target], kw)?;
                Ok(Value::None)
            }
            other => {
                // Método herdado de `dict`/`list`/`str`...: age sobre o valor embutido.
                let payload = inst.payload.borrow().clone();
                if let Some(p) = payload {
                    if let Some(r) = crate::vm::payload_dunder(&p, other, args.clone()) {
                        return r;
                    }
                    let method = vm.load_attr(&p, other)?;
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
                if let Some(Value::Function(f)) = i.class().lookup("__mro_entries__") {
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
        // Como o `type_new_set_module`: `type(nome, bases, ns)` sem `__module__` o toma do `__name__` das globais de quem chama.
        if !ns.iter().any(|(k, _)| k == "__module__") {
            if let Some(module @ Value::Str(_)) = self.globals.borrow().get("__name__").cloned() {
                ns.push(("__module__".to_string(), module));
            }
        }
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
            let target = cell_env(&cell);
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
                if i.class().lookup("__set_name__").is_some() {
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
                if !kw.is_empty() {
                    return Err(type_error(format!(
                        "{}.__init_subclass__() takes no keyword arguments",
                        cls.name
                    )));
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
        match self.begin_instance(cls, args, kw)? {
            Built::Done(v) => Ok(v),
            Built::Init { obj, init, args, kw } => {
                let r = self.call_function(&init, args, kw)?;
                init_returned(obj, &r)
            }
        }
    }

    /// A parte de `Classe(args)` que vem antes do `__init__` de usuário: cria a instância (com o
    /// `__new__`, se houver) e, quando falta rodar um `__init__` em Python, devolve-o com os argumentos
    /// para quem chamou decidir como executá-lo (o laço da VM empilha o quadro em vez de recursar).
    pub(crate) fn begin_instance(&mut self, cls: &Rc<ClassObj>, args: Vec<Value>, kw: Kw) -> PyResult<Built> {
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
            return Ok(Built::Done(made));
        }
        let fresh = InstanceObj::new_rc(cls, None);
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
                    Value::Instance(i) if i.class().mro().iter().any(|c| Rc::ptr_eq(c, cls)) => (i.clone(), made),
                    _ => return Ok(Built::Done(made)),
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
                return Ok(Built::Init { obj, init: f, args: full, kw });
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
        Ok(Built::Done(obj))
    }

    /// Atributo de classe visto de uma instância ou da própria classe: funções viram métodos
    /// presos, descritores são desembrulhados.
    pub(crate) fn bind_class_attr(&mut self, attr: &Value, recv: Value, cls: &Rc<ClassObj>) -> PyResult<Value> {
        self.bind_class_attr_defer(attr, recv, cls, None)
    }

    /// Chama `f` com `args`; com `defer`, uma função Python não roda aqui: a chamada fica anotada em
    /// `defer` (e o valor devolvido é um marcador sem uso) para o laço de instruções abrir o quadro.
    fn call_or_defer(&mut self, f: &Value, args: Vec<Value>, defer: Option<&mut PendingCall>) -> PyResult<Value> {
        match (f, defer) {
            (Value::Function(func), Some(slot)) => {
                *slot = Some((func.clone(), args));
                Ok(Value::None)
            }
            _ => self.call(f, args, Vec::new()),
        }
    }

    /// `bind_class_attr` que, dado `defer`, anota em vez de executar a função Python que produziria o
    /// valor (o `fget` de uma `property`, o `__get__` de um descritor).
    fn bind_class_attr_defer(
        &mut self,
        attr: &Value,
        recv: Value,
        cls: &Rc<ClassObj>,
        defer: Option<&mut PendingCall>,
    ) -> PyResult<Value> {
        match attr {
            // Função embutida no CPython (escrita em Python aqui): `Classe.attr = time.time` não liga `self`.
            Value::Function(f) if f.attrs.borrow().contains_key("__no_bind__") => Ok(attr.clone()),
            Value::Function(f) => match recv {
                Value::Class(_) => Ok(attr.clone()),
                // O método de um shim de tipo de `builtins` ligado a um objeto é método embutido, como no CPython.
                _ if f.is_builtin_type_method() => {
                    Ok(Value::Ext(Rc::new(ShimMethod { recv, func: attr.clone(), name: intern(&f.code.name) })))
                }
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
                    _ => self.call_or_defer(&get, vec![recv], defer),
                },
                None => Ok(attr.clone()),
            },
            // Descritor escrito em Python: `__get__(self, instância ou None, classe)`.
            Value::Instance(d) => match d.class().lookup("__get__") {
                Some(Value::Function(f)) => {
                    let instance = match recv {
                        Value::Class(_) => Value::None,
                        other => other,
                    };
                    let args = vec![attr.clone(), instance, Value::Class(cls.clone())];
                    self.call_or_defer(&Value::Function(f), args, defer)
                }
                // Subclasse de `property` sem `__get__` próprio: o `property.__get__` herdado.
                _ => match (property_parts(attr), recv) {
                    (Some((get, ..)), recv) if !matches!(recv, Value::Class(_)) => {
                        if matches!(get, Value::None) {
                            return Err(property_missing(cls, attr, "getter"));
                        }
                        self.call_or_defer(&get, vec![recv], defer)
                    }
                    _ => Ok(attr.clone()),
                },
            },
            other => Ok(other.clone()),
        }
    }

    pub(crate) fn instance_getattr(&mut self, obj: &Value, inst: &Rc<InstanceObj>, name: &str) -> PyResult<Value> {
        self.instance_getattr_with(obj, inst, name, true, None)
    }

    /// `módulo.nome` quando `module.__class__ = Sub` deu uma classe ao módulo: o descritor de dados da
    /// classe (`property`) vence as globais, como na busca de atributo do `module_getattro`.
    pub(crate) fn module_class_data_attr(&mut self, m: &Rc<crate::object::ModuleObj>, name: &str) -> Option<PyResult<Value>> {
        let cls = module_class(m)?;
        let attr = cls.lookup(name)?;
        is_data_descriptor(&attr).then(|| self.bind_class_attr(&attr, Value::Module(m.clone()), &cls))
    }

    /// O que sobra para `módulo.nome` ausente nas globais de um módulo com classe trocada: o atributo
    /// (método, valor) da classe, ligado ao módulo.
    pub(crate) fn module_class_attr(&mut self, m: &Rc<crate::object::ModuleObj>, name: &str) -> Option<PyResult<Value>> {
        let cls = module_class(m)?;
        let attr = cls.lookup(name)?;
        Some(self.bind_class_attr(&attr, Value::Module(m.clone()), &cls))
    }

    /// O `__getattr__` da classe de um módulo com classe trocada, última tentativa antes do `AttributeError`.
    pub(crate) fn module_class_getattr(&mut self, m: &Rc<crate::object::ModuleObj>, name: &str) -> Option<PyResult<Value>> {
        let Some(f @ Value::Function(_)) = module_class(m)?.lookup("__getattr__") else { return None };
        Some(self.call(&f, vec![Value::Module(m.clone()), Value::str(name)], Vec::new()))
    }

    /// `instance_getattr` para o laço de instruções: quando o valor do atributo sai da execução de uma
    /// função Python (o `fget` de uma `property`, o `__get__` de um descritor, o `__getattr__`), devolve
    /// `Err` com essa chamada, sem executá-la, para o laço empilhar o quadro.
    pub(crate) fn instance_getattr_call(
        &mut self,
        obj: &Value,
        inst: &Rc<InstanceObj>,
        name: &str,
    ) -> PyResult<Result<Value, (Rc<FuncObj>, Vec<Value>)>> {
        let mut call: PendingCall = None;
        let value = self.instance_getattr_with(obj, inst, name, true, Some(&mut call))?;
        Ok(call.map_or(Ok(value), Err))
    }

    /// `object.__getattribute__`: a busca normal sem o gancho `__getattr__` da classe.
    pub(crate) fn instance_getattr_plain(&mut self, obj: &Value, inst: &Rc<InstanceObj>, name: &str) -> PyResult<Value> {
        self.instance_getattr_with(obj, inst, name, false, None)
    }

    fn instance_getattr_with(
        &mut self,
        obj: &Value,
        inst: &Rc<InstanceObj>,
        name: &str,
        hook: bool,
        defer: Option<&mut PendingCall>,
    ) -> PyResult<Value> {
        match name {
            // `__class__` de `object` é um descritor de dados na MRO: uma classe que define o seu
            // (`__class__ = property(...)`, o `spec` do `unittest.mock`) passa pela busca normal.
            "__class__" if inst.class().lookup("__class__").is_none() => return Ok(Value::Class(inst.class())),
            // O shim de um tipo embutido (`memoryview`) não tem `__dict__`, como o tipo em C.
            "__dict__" if matches!(inst.class().dict.borrow().get("__module__"), Some(Value::Str(m)) if m.as_str() == "builtins") => {
                return Err(exc("AttributeError", format!("'{}' object has no attribute '__dict__'", inst.class().name)));
            }
            "__dict__" if inst.class().slots_allow("__dict__") => return Ok(inst.live_dict()),
            _ => {}
        }
        inst.sync_from_view();
        // `__getattribute__` de usuário intercepta toda busca; `AttributeError` dele cai no `__getattr__`.
        if hook {
            if let Some(Value::Function(f)) = inst.class().lookup("__getattribute__") {
                return match self.call_function(&f, vec![obj.clone(), Value::str(name)], Vec::new()) {
                    Err(e) if e.kind == "AttributeError" => match inst.class().lookup("__getattr__") {
                        Some(Value::Function(g)) => self.call_function(&g, vec![obj.clone(), Value::str(name)], Vec::new()),
                        _ => Err(e),
                    },
                    other => other,
                };
            }
        }
        // Propriedades e descritores de dados escritos em Python (têm `__set__`) têm precedência sobre o
        // dicionário da instância.
        let class_attr = lookup_public(&inst.class(), name);
        if class_attr.as_ref().is_some_and(is_data_descriptor) {
            return self.bind_class_attr_defer(class_attr.as_ref().unwrap_or(&Value::None), obj.clone(), &inst.class(), defer);
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
            return self.bind_class_attr_defer(&attr, obj.clone(), &inst.class(), defer);
        }
        // Os ganchos de atributo de `object` estão na MRO antes do `__getattr__`: um wrapper que chama
        // `self.__getattribute__(...)` dentro do próprio `__getattr__` não recursa.
        if matches!(name, "__getattribute__" | "__setattr__" | "__delattr__") {
            return Ok(Value::Ext(Rc::new(BuiltinSuperMethod { obj: obj.clone(), name: intern(name) })));
        }
        // Os demais métodos de `object` também estão na MRO, antes do `__getattr__` de usuário. Instância de
        // tipo embutido (valor guardado, exceção) tem os seus, resolvidos mais abaixo.
        if inst.payload.borrow().is_none() && inst.class().builtin_base.is_none() {
            if matches!(name, "__init_subclass__" | "__subclasshook__") {
                // Métodos de classe: ligados à classe da instância.
                let owner = Value::Class(inst.class());
                return Ok(Value::Ext(Rc::new(BuiltinSuperMethod { obj: owner, name: intern(name) })));
            }
            if OBJECT_INSTANCE_METHODS.contains(&name) {
                return Ok(Value::Ext(Rc::new(BuiltinSuperMethod { obj: obj.clone(), name: intern(name) })));
            }
        }
        if hook {
            if let Some(Value::Function(f)) = inst.class().lookup("__getattr__") {
                return self.call_or_defer(&Value::Function(f), vec![obj.clone(), Value::str(name)], defer);
            }
        }
        // `object.__reduce_ex__`, `__reduce__` e `__getstate__` (pickle, copy): no CPython são métodos embutidos de
        // `object` ligados ao objeto; a lógica vive em `copyreg`, chamada por dentro do nativo.
        if matches!(name, "__reduce_ex__" | "__reduce__" | "__getstate__") {
            if let Some(method) = exception_method(obj, name) {
                return Ok(method);
            }
        }
        let payload = inst.payload.borrow().clone();
        if let Some(p) = payload {
            if is_property_instance(inst) && matches!(name, "getter" | "setter" | "deleter") {
                let which = match name {
                    "getter" => "getter",
                    "setter" => "setter",
                    _ => "deleter",
                };
                return Ok(Value::Ext(Rc::new(PropertyCopy { obj: obj.clone(), which })));
            }
            // `d.__getitem__` de subclasse de `dict` com `__missing__`: a chave ausente chama o gancho.
            if name == "__getitem__" && matches!(p, Value::Dict(_)) && inst.class().lookup("__missing__").is_some() {
                return Ok(Value::Ext(Rc::new(InstanceDunder { obj: obj.clone(), name: "__getitem__" })));
            }
            let inner = self.load_attr(&p, name)?;
            if matches!(name, "__iadd__" | "__imul__" | "__ior__" | "__iand__" | "__isub__" | "__ixor__") {
                return Ok(Value::Ext(Rc::new(InplaceMethod { obj: obj.clone(), payload: p, inner })));
            }
            return Ok(inner);
        }
        if name == "__doc__" {
            return Ok(inst.class().lookup("__doc__").unwrap_or(Value::None));
        }
        // `object.__new__` é um método estático: pela instância vale o mesmo objeto que pela classe.
        if name == "__new__" {
            let cls = inst.class();
            return self.class_getattr(&cls, "__new__");
        }
        // `object.__init__` herdado: ligado à instância, como qualquer método.
        if name == "__init__" {
            let cls = inst.class();
            let attr = self.class_getattr(&cls, "__init__")?;
            return self.bind_class_attr(&attr, obj.clone(), &cls);
        }
        if inst.class().builtin_base.is_some() && matches!(name, "__cause__" | "__context__" | "__suppress_context__") {
            return Ok(if name == "__suppress_context__" { Value::Bool(false) } else { Value::None });
        }
        if name == "with_traceback" && inst.class().builtin_base.is_some() {
            return Ok(Value::Ext(Rc::new(ExcWithTraceback { obj: obj.clone() })));
        }
        // `BaseException.__setstate__` (o `pickle` o chama ao reconstruir a exceção) vive em `copyreg`.
        if name == "__setstate__" && inst.class().builtin_base.is_some() {
            if let Some(method) = exception_method(obj, name) {
                return Ok(method);
            }
        }
        if name == "add_note" && inst.class().builtin_base.is_some() {
            return Ok(Value::Ext(Rc::new(ExcAddNote { obj: obj.clone() })));
        }
        Err(crate::object::no_attribute(&inst.class().tp_name(), name))
    }

    pub(crate) fn class_getattr(&mut self, cls: &Rc<ClassObj>, name: &str) -> PyResult<Value> {
        // Tipo de módulo em C do CPython escrito em Python aqui (`itertools.chain`, `_io.BytesIO`): o
        // `__text_signature__` que o CPython tira do `tp_doc`, da tabela gerada no oráculo.
        if name == "__text_signature__" {
            if let Some(sig) = crate::modules::cpydocs::class_signature(&cls.module(), &cls.name) {
                return Ok(Value::str(sig));
            }
        }
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
                // A cadeia embutida da base (`OSError`, `Exception`, `BaseException`, `object`) fecha o MRO.
                match cls.builtin_base.or(cls.data_base) {
                    Some(base) => {
                        let base = crate::builtins::get(base).unwrap_or(Value::Builtin(base));
                        match self.load_attr(&base, "__mro__") {
                            Ok(Value::Tuple(chain)) => mro.extend(chain.iter().cloned()),
                            _ => mro.push(Value::Builtin("object")),
                        }
                    }
                    None => mro.push(Value::Builtin("object")),
                }
                return Ok(Value::tuple(mro));
            }
            "__dict__" => {
                if let Some(view) = crate::builtins_ext::emulated_type_dict(self, cls)? {
                    return Ok(view);
                }
                let mut d = crate::object::Dict::default();
                for (k, v) in cls.dict.borrow().iter() {
                    // O `__slots__` de um shim de tipo em C é maquinaria nossa: o tipo real não o tem.
                    if k == "__slots__" && cls.emulates_c_type() {
                        continue;
                    }
                    d.set(Value::str(k.clone()), v.clone())?;
                }
                for (name, member) in slot_members(cls) {
                    d.set(Value::str(name), member)?;
                }
                // O `type` sempre grava `__doc__` e, se nenhuma base já dá um `__dict__` às
                // instâncias, acrescenta os descritores `__dict__` e `__weakref__`.
                if !cls.dict.borrow().contains_key("__doc__") {
                    d.set(Value::str("__doc__"), Value::None)?;
                }
                for slot in instance_slots_added(cls) {
                    let desc = GetSetDescriptor { name: slot, owner: Value::Class(cls.clone()) };
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
            if let Value::Function(f) = self.load_attr(&Value::Module(m), "_type_mro")? {
                return Ok(Value::BoundFn(Rc::new((Value::Class(cls.clone()), f))));
            }
        }
        if let Some(attr) = lookup_public(cls, name) {
            // `__init_subclass__` e `__class_getitem__` são métodos de classe implícitos.
            if let (Value::Function(f), "__init_subclass__" | "__class_getitem__") = (&attr, name) {
                return Ok(Value::BoundFn(Rc::new((Value::Class(cls.clone()), f.clone()))));
            }
            return self.bind_class_attr(&attr, Value::Class(cls.clone()), cls);
        }
        // `Classe.nome` de um nome do `__slots__`: o `member_descriptor` que o `type` pôs na classe.
        if let Some(member) = cls.mro().iter().find_map(|c| slot_members(c).into_iter().find(|(n, _)| n == name)) {
            return Ok(member.1);
        }
        // `Classe.__weakref__`: o descritor que o `type` pôs na primeira classe do MRO que o tem.
        if name == "__weakref__" {
            if let Some(owner) = cls.mro().into_iter().find(|c| instance_slots_added(c).contains(&"__weakref__")) {
                return Ok(Value::Ext(Rc::new(GetSetDescriptor { name: "__weakref__", owner: Value::Class(owner) })));
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
                        Some(Descriptor::Property { get, .. }) => self.call(&get, vec![me], Vec::new()),
                        _ => Ok(attr.clone()),
                    },
                    other => Ok(other.clone()),
                };
            }
        }
        if name == "__new__" && cls.data_base.is_none() && cls.builtin_base.is_none() {
            return Ok(object_class_method(Value::Builtin("object"), "__new__"));
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
                // Os métodos de classe ficam ligados à própria classe (`A.__init_subclass__.__self__ is A`).
                if matches!(name, "__init_subclass__" | "__subclasshook__") {
                    return Ok(object_class_method(Value::Class(cls.clone()), intern(name)));
                }
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
        // O que a subclasse de um tipo embutido herda dele (`I.__index__`, `L.__len__`): o descritor do tipo.
        if let Some(v) = cls.data_base.and_then(|t| crate::typeattrs::type_attr(t, name)) {
            return Ok(v);
        }
        Err(exc("AttributeError", format!("type object '{}' has no attribute '{name}'", cls.name)))
    }

    pub(crate) fn store_attr(&mut self, obj: &Value, name: &str, value: Value) -> PyResult<()> {
        self.store_attr_with(obj, name, value, None)
    }

    /// `store_attr` para o laço de instruções: quando a gravação sai da execução de uma função Python
    /// (o `fset` de uma `property`, o `__set__` de um descritor, o `__setattr__`), devolve essa chamada
    /// sem executá-la, para o laço empilhar o quadro.
    pub(crate) fn store_attr_call(&mut self, obj: &Value, name: &str, value: Value) -> PyResult<PendingCall> {
        let mut call: PendingCall = None;
        self.store_attr_with(obj, name, value, Some(&mut call))?;
        Ok(call)
    }

    fn store_attr_with(&mut self, obj: &Value, name: &str, value: Value, defer: Option<&mut PendingCall>) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                // Uma busca só serve aos dois casos de descritor (property e `__set__` em Python).
                let class_attr = inst.class().lookup(name);
                if let Some(Value::Ext(e)) = &class_attr {
                    if let Some(Descriptor::Property { set, .. }) = e.descriptor() {
                        return match set {
                            Some(f) => self.call_or_defer(&f, vec![obj.clone(), value], defer).map(|_| ()),
                            None => Err(exc(
                                "AttributeError",
                                format!("property '{name}' of '{}' object has no setter", inst.class().name),
                            )),
                        };
                    }
                }
                if let Some(f @ Value::Function(_)) = inst.class().lookup("__setattr__") {
                    return self.call_or_defer(&f, vec![obj.clone(), Value::str(name), value], defer).map(|_| ());
                }
                if let Some(Value::Instance(d)) = &class_attr {
                    if let Some(f @ Value::Function(_)) = d.class().lookup("__set__") {
                        return self.call_or_defer(&f, vec![Value::Instance(d.clone()), obj.clone(), value], defer).map(|_| ());
                    }
                    // Subclasse de `property` sem `__set__` próprio: o `property.__set__` herdado.
                    if let Some((_, set, _)) = property_parts(&Value::Instance(d.clone())) {
                        let Some(f) = set else {
                            return Err(property_missing(&inst.class(), &Value::Instance(d.clone()), "setter"));
                        };
                        return self.call_or_defer(&f, vec![obj.clone(), value], defer).map(|_| ());
                    }
                }
                if name == "__class__" {
                    assign_instance_class(inst, &value)?;
                    return Ok(());
                }
                if name == "__dict__" && inst.class().slots_allow("__dict__") {
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
                if !inst.class().slots_allow(name) {
                    return Err(exc(
                        "AttributeError",
                        format!(
                            "'{}' object has no attribute '{name}' and no __dict__ for setting new attributes",
                            inst.class().tp_name()
                        ),
                    ));
                }
                inst.set_own(name, value);
                Ok(())
            }
            Value::Class(c) => {
                c.dict.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            Value::Module(m) => {
                if name == "__class__" {
                    return set_module_class(m, &value);
                }
                // `module.__class__ = Sub`: o `property` (ou descritor com `__set__`) da subclasse recebe a escrita.
                if let Some(cls) = module_class(m) {
                    match cls.lookup(name) {
                        Some(Value::Ext(e)) => {
                            if let Some(Descriptor::Property { set, .. }) = e.descriptor() {
                                return match set {
                                    Some(f) => self.call_or_defer(&f, vec![obj.clone(), value], defer).map(|_| ()),
                                    None => Err(exc(
                                        "AttributeError",
                                        format!("property '{name}' of '{}' object has no setter", cls.name),
                                    )),
                                };
                            }
                        }
                        Some(Value::Instance(d)) => {
                            if let Some(f @ Value::Function(_)) = d.class().lookup("__set__") {
                                return self
                                    .call_or_defer(&f, vec![Value::Instance(d.clone()), obj.clone(), value], defer)
                                    .map(|_| ());
                            }
                        }
                        _ => {}
                    }
                }
                let live = self.module_globals.borrow().get(m.name).cloned();
                if let Some(g) = live {
                    g.borrow_mut().insert(name.into(), value.clone());
                    if crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed) {
                        crate::globalsview::push(&g, name, Some(&value));
                    }
                }
                crate::modules::pysrc::mirror_private(m.name, name, Some(&value));
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
            // `exc.__cause__ = c` (que também suprime o contexto), `exc.__context__ = c` e
            // `exc.__suppress_context__ = flag`.
            Value::Exception(x) if matches!(name, "__cause__" | "__context__" | "__suppress_context__") => {
                let link = |value: Value| match value {
                    Value::None => Ok(None),
                    Value::Exception(_) | Value::Instance(_) => Ok(Some(value)),
                    _ => Err(type_error(format!(
                        "exception {} must be None or derive from BaseException",
                        if name == "__cause__" { "cause" } else { "context" }
                    ))),
                };
                let mut chain = x.chain.borrow_mut();
                match name {
                    "__cause__" => {
                        chain.cause = link(value)?;
                        chain.suppress = true;
                    }
                    "__context__" => chain.context = link(value)?,
                    _ => chain.suppress = value.is_true(),
                }
                Ok(())
            }
            Value::Exception(x) if name == "__notes__" => {
                let mut extra = x.extra.borrow_mut();
                extra.retain(|(k, _)| *k != "__notes__");
                extra.push(("__notes__", value));
                Ok(())
            }
            // `e.x = 5` numa exceção embutida: entra no `__dict__` dela (`name`, `obj` e `path` são os campos do tipo).
            Value::Exception(x) if !x.is_computed_attr(name) => {
                x.extra_set(crate::object::intern(name), value);
                Ok(())
            }
            _ => Err(no_dict_error(obj.type_name(), name)),
        }
    }

    pub(crate) fn delete_attr(&mut self, obj: &Value, name: &str) -> PyResult<()> {
        self.delete_attr_with(obj, name, None)
    }

    /// `delete_attr` para o laço de instruções: devolve, sem executar, a chamada do `__delattr__`, do
    /// `__delete__` de um descritor ou do `fdel` de uma `property` quando é função Python.
    pub(crate) fn delete_attr_call(&mut self, obj: &Value, name: &str) -> PyResult<PendingCall> {
        let mut call: PendingCall = None;
        self.delete_attr_with(obj, name, Some(&mut call))?;
        Ok(call)
    }

    fn delete_attr_with(&mut self, obj: &Value, name: &str, defer: Option<&mut PendingCall>) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                if let Some(f @ Value::Function(_)) = inst.class().lookup("__delattr__") {
                    return self.call_or_defer(&f, vec![obj.clone(), Value::str(name)], defer).map(|_| ());
                }
                if name == "__class__" && inst.class().lookup("__class__").is_none() {
                    return Err(type_error("can't delete __class__ attribute"));
                }
                if let Some(Value::Instance(d)) = inst.class().lookup(name) {
                    if let Some(f @ Value::Function(_)) = d.class().lookup("__delete__") {
                        return self.call_or_defer(&f, vec![Value::Instance(d.clone()), obj.clone()], defer).map(|_| ());
                    }
                }
                // `del obj.x` sobre `property` (ou subclasse sem `__delete__` próprio): chama o `fdel`.
                if let Some(attr @ (Value::Ext(_) | Value::Instance(_))) = inst.class().lookup(name) {
                    if let Some((_, _, del)) = property_parts(&attr) {
                        let Some(f) = del else {
                            return Err(property_missing(&inst.class(), &attr, "deleter"));
                        };
                        return self.call_or_defer(&f, vec![obj.clone()], defer).map(|_| ());
                    }
                }
                if inst.remove_own(name).is_none() {
                    return Err(slot_delete_error(inst, name));
                }
                Ok(())
            }
            Value::Class(c) => {
                if c.dict.borrow_mut().shift_remove(name).is_none() {
                    return Err(exc("AttributeError", format!("type object '{}' has no attribute '{name}'", c.name)));
                }
                Ok(())
            }
            Value::Module(m) => {
                if name == "__class__" {
                    return Err(type_error("can't delete __class__ attribute"));
                }
                let live = self.module_globals.borrow().get(m.name).cloned();
                let from_globals = live.as_ref().is_some_and(|g| g.borrow_mut().shift_remove(name).is_some());
                if from_globals && crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Some(g) = &live {
                        crate::globalsview::push(g, name, None);
                    }
                }
                let from_attrs = m.attrs.borrow_mut().remove(name).is_some();
                if !from_globals && !from_attrs {
                    return Err(crate::object::no_attribute("module", name));
                }
                crate::modules::pysrc::mirror_private(m.name, name, None);
                Ok(())
            }
            // Atributo de usuário de função (`f.__click_params__`, `f.cache`): vive no `__dict__` dela.
            Value::Function(f) => {
                if f.attrs.borrow_mut().remove(name).is_none() {
                    return Err(crate::object::no_attribute("function", name));
                }
                Ok(())
            }
            // Objetos nativos que aceitam apagar atributos (`del frame.f_trace`).
            Value::Ext(e) => match e.delattr(name) {
                Some(result) => result,
                None => Err(crate::object::no_attribute(obj.type_name(), name)),
            },
            // `del e.x` numa exceção embutida: tira a entrada do `__dict__` dela.
            Value::Exception(x) if x.dict_get(name).is_some() => {
                x.extra.borrow_mut().retain(|(k, _)| *k != name);
                Ok(())
            }
            _ => Err(no_dict_error(obj.type_name(), name)),
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
            Value::Instance(inst) => match inst.class().lookup("__delitem__") {
                Some(Value::Function(f)) => {
                    self.call_function(&f, vec![container.clone(), index.clone()], Vec::new())?;
                    Ok(())
                }
                _ => Err(type_error(format!("'{}' object doesn't support item deletion", inst.class().name))),
            },
            other => Err(type_error(format!("'{}' object doesn't support item deletion", other.type_name()))),
        }
    }

    /// Abre um `with`: devolve `(exit, valor do __enter__)`.
    pub(crate) fn with_enter(&mut self, mgr: &Value) -> PyResult<(Value, Value)> {
        let unsupported = || type_error(format!("'{}' object does not support the context manager protocol", mgr.type_name()));
        match mgr {
            Value::Instance(i) => {
                let (Some(enter), Some(exit)) = (i.class().lookup("__enter__"), i.class().lookup("__exit__")) else {
                    return Err(unsupported());
                };
                let entered = match &enter {
                    Value::Function(f) => self.call_function(f, vec![mgr.clone()], Vec::new())?,
                    other => self.call(other, vec![mgr.clone()], Vec::new())?,
                };
                let exit = self.bind_class_attr(&exit, mgr.clone(), &i.class())?;
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
        if let Value::Module(m) = v {
            if let Some(cls) = module_class(m) {
                return Value::Class(cls);
            }
        }
        match v {
            Value::Instance(i) => Value::Class(i.class()),
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
        if let Some(p) = payload.filter(|_| !is_property_instance(i)) {
            return if is_str { to_str(&p) } else { crate::object::repr(&p) };
        }
        if i.class().builtin_base.is_some() {
            let args = match i.dict.borrow().get("args") {
                Some(Value::Tuple(t)) => t.to_vec(),
                _ => Vec::new(),
            };
            if is_str {
                if let (Some(base), [Value::Int(_), _, ..]) = (i.class().builtin_base, args.as_slice())
                    && crate::object::exc_is_subclass(base, "OSError")
                {
                    return crate::object::exc_str(&crate::object::ExcObj::new(base, args));
                }
                return match args.as_slice() {
                    [] => String::new(),
                    [one] if i.class().builtin_base.is_some_and(|b| crate::object::exc_is_subclass(b, "KeyError")) => {
                        crate::object::repr(one)
                    }
                    [one] => to_str(one),
                    _ => to_str(&Value::tuple(args)),
                };
            }
            let inner: Vec<String> = args.iter().map(crate::object::repr).collect();
            return format!("{}({})", i.class().name, inner.join(", "));
        }
        format!("<{}.{} object at {:#x}>", i.class().module(), i.class().name, crate::object::py_addr(Rc::as_ptr(i) as usize))
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
                if let Some(Value::Function(f)) = i.class().lookup(name) {
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
            if let Some(Value::Function(f)) = i.class().lookup("__repr__") {
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
            if let Some(Value::Function(f)) = i.class().lookup("__format__") {
                let r = self.call_function(&f, vec![v.clone(), Value::str(spec)], Vec::new())?;
                return Ok(to_str(&r));
            }
            if spec.is_empty() {
                return self.str_of(v);
            }
            return Err(type_error(format!("unsupported format string passed to {}.__format__", i.class().name)));
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
                Value::Instance(i) => i.class().mro().iter().any(|k| Rc::ptr_eq(k, c)),
                _ => false,
            }),
            Value::Builtin(name) if EXC_CLASSES.iter().any(|(n, _)| n == name) => {
                let kind = match exc_value {
                    Value::Exception(e) => e.kind,
                    Value::Instance(i) => i.class().builtin_base.unwrap_or(""),
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
                if i.class().builtin_base.is_none() {
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
        let user = match i.class().lookup(name) {
            Some(Value::Function(f)) => Some(f),
            // Atributo de classe que já é um chamável preso (`__next__ = gerador.__next__`): chama direto.
            Some(bound @ (Value::Bound(_) | Value::BoundFn(_) | Value::Ext(_) | Value::NativeFn(_))) => {
                return Some(self.call(&bound, args, Vec::new()));
            }
            // Descritor de usuário (`__get__` em Python, como o `MagicProxy` do mock): resolve e chama.
            Some(attr @ Value::Instance(_)) if matches!(&attr, Value::Instance(d) if d.class().lookup("__get__").is_some()) => {
                let bound = match self.bind_class_attr(&attr, obj.clone(), &i.class()) {
                    Ok(b) => b,
                    Err(e) => return Some(Err(e)),
                };
                return Some(self.call(&bound, args, Vec::new()));
            }
            // Objeto chamável sem `__get__` na classe (um `MagicMock` posto como `__len__`): o CPython o
            // chama só com os argumentos, sem o `self`.
            Some(attr @ Value::Instance(_)) if matches!(&attr, Value::Instance(d) if d.class().lookup("__call__").is_some()) => {
                return Some(self.call(&attr, args, Vec::new()));
            }
            _ => None,
        };
        let Some(f) = user else {
            // Subclasse de tipo embutido: o que a classe não redefine vai para o valor embutido.
            let payload = i.payload.borrow().clone()?;
            let r = crate::vm::payload_dunder(&payload, name, args.clone())?;
            if let (Err(e), "__getitem__") = (&r, name) {
                if e.kind == "KeyError" {
                    if let Some(Value::Function(m)) = i.class().lookup("__missing__") {
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
                let mut doc = it.next().unwrap_or(Value::None);
                let mut get = get;
                let (mut set, mut del) = (set, del);
                for (k, v) in kw {
                    match k.as_str() {
                        "fget" => get = v,
                        "fset" => set = Some(v),
                        "fdel" => del = Some(v),
                        "doc" => doc = v,
                        _ => return Err(type_error(format!("property() got an unexpected keyword argument '{k}'"))),
                    }
                }
                let p = Property::new(get, set, del);
                *p.doc.borrow_mut() = doc;
                Ok(Value::Ext(Rc::new(p)))
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
        if let Some(Value::Function(f)) = i.class().lookup(name) {
            return match vm.call_function(&f, vec![v.clone()], Vec::new()) {
                Ok(Value::Str(s)) => Some(s.as_str().to_string()),
                Ok(other) => {
                    crate::vm::note_text_error(
                        type_error(format!("{name} returned non-string (type {})", other.type_name())),
                        vm.depth.get(),
                    );
                    None
                }
                Err(e) => {
                    crate::vm::note_text_error(e, vm.depth.get());
                    None
                }
            };
        }
    }
    if i.class().builtin_base.is_some() || is_property_instance(i) {
        return Some(vm.default_text(v, is_str));
    }
    let payload = i.payload.borrow().clone()?;
    Some(if is_str { crate::object::to_str(&payload) } else { crate::object::repr(&payload) })
}

/// O operando direito é instância de uma subclasse estrita da classe do esquerdo: o `richcompare` e
/// o refletido dele vão primeiro (`do_richcompare` e `slot_nb_*` do CPython).
pub(crate) fn right_is_subclass(a: &Value, b: &Value) -> bool {
    matches!((a, b), (Value::Instance(l), Value::Instance(r)) if r.class().is_strict_subtype_of(&l.class()))
}

/// `a == b` quando um dos lados é instância com `__eq__`.
pub fn instance_eq(a: &Value, b: &Value) -> Option<bool> {
    let mut vm = current()?;
    let sides = if right_is_subclass(a, b) { [(b, a), (a, b)] } else { [(a, b), (b, a)] };
    for (x, y) in sides {
        if let Value::Instance(i) = x {
            if let Some(Value::Function(f)) = i.class().lookup("__eq__") {
                if let Ok(r) = vm.call_function(&f, vec![x.clone(), y.clone()], Vec::new()) {
                    if !is_not_implemented(&r) {
                        return Some(r.is_true());
                    }
                }
            }
        }
    }
    payload_eq(a, b)
}

/// `a == b` quando um dos lados é instância de subclasse de tipo embutido: compara os valores embutidos.
pub(crate) fn payload_eq(a: &Value, b: &Value) -> Option<bool> {
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
    if let Some(Value::Function(f)) = i.class().lookup("__hash__") {
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
    ["int", "float", "str", "bytes", "list", "tuple", "dict", "set", "bool", "module", "property"].into_iter().find(|n| *n == name)
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
    if name == "property" {
        return Value::Builtin("property");
    }
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
    let mut d = crate::object::Dict::default();
    for (k, v) in ns {
        d.set(Value::str(k.clone()), v.clone()).map_err(|_| type_error("unhashable type"))?;
    }
    Ok(Value::dict(d))
}
