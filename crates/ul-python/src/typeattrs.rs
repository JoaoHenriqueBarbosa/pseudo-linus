//! Atributos de classe dos tipos embutidos (`dict.fromkeys`, `str.join`, `int.from_bytes`...).
//!
//! `dict`, `str` e companhia são `NativeFn`s; quando o programa lê um atributo deles, este módulo
//! devolve o construtor alternativo (`fromkeys`) ou o método não ligado (`str.lower`).

use std::rc::Rc;

use crate::object::{code_points, Dict, ExtObject, Kw, NativeFn, Value};
use crate::vm::{type_error, PyResult, Vm};

pub(crate) const TYPES: &[&str] = &["int", "float", "str", "list", "tuple", "dict", "set", "frozenset", "bool", "bytes", "bytearray"];

/// Um valor de exemplo do tipo, só para consultar a tabela de métodos.
fn sample(tname: &str) -> Option<Value> {
    Some(match tname {
        "int" => Value::Int(0),
        "bool" => Value::Bool(false),
        "float" => Value::Float(0.0),
        "str" => Value::str(""),
        "list" => Value::list(Vec::new()),
        "tuple" => Value::tuple(Vec::new()),
        "dict" => Value::dict(Dict::default()),
        "set" => Value::set(crate::object::Set::new()),
        "frozenset" => Value::set(crate::object::Set::new().with_frozen(true)),
        "bytes" => Value::bytes(Vec::new()),
        "bytearray" => Value::bytearray(Vec::new()),
        "range" => Value::Range(crate::object::Range { start: 0, stop: 0, step: 1 }),
        "slice" => Value::Slice(Rc::new((Value::None, Value::None, Value::None))),
        "NoneType" => Value::None,
        _ => return None,
    })
}

/// O tipo embutido de `tname` que `type_attr` resolve: os de `TYPES` e os que a tabela de métodos atende
/// só pelos mágicos (`range`, `slice`, `NoneType`). `TYPES` fica sem eles porque `range[int]` não existe.
fn attr_type(tname: &str) -> Option<&'static str> {
    TYPES.iter().chain(["range", "slice", "NoneType"].iter()).copied().find(|t| *t == tname)
}

/// Método não ligado: `str.lower` chamado como `str.lower("ABC")`. `tname` é o tipo que guarda o método no
/// próprio `__dict__` (`int` para `bool.__add__`, `object` para `int.__setattr__`).
struct Unbound {
    tname: &'static str,
    name: &'static str,
}

impl Unbound {
    /// O receptor tem de ser do tipo dono do método: `int.__index__("a")` e `str.upper(5)` recusam como o
    /// CPython. Só os valores embutidos de dados são conferidos; o resto segue para a busca do atributo.
    fn check_receiver(&self, recv: &Value) -> PyResult<()> {
        let value = crate::vm::unwrap_payload(recv);
        let plain = matches!(
            value,
            Value::None
                | Value::Bool(_)
                | Value::Int(_)
                | Value::Big(_)
                | Value::Float(_)
                | Value::Str(_)
                | Value::Bytes(_)
                | Value::ByteArray(_)
                | Value::List(_)
                | Value::Tuple(_)
                | Value::Dict(_)
                | Value::Set(_)
                | Value::Range(_)
                | Value::Slice(_)
        );
        let actual = value.type_name();
        if !plain || self.tname == "object" || actual == self.tname || (self.tname == "int" && actual == "bool") {
            return Ok(());
        }
        Err(type_error(if is_slot_wrapper(self.tname, self.name) {
            format!("descriptor '{}' requires a '{}' object but received a '{actual}'", self.name, self.tname)
        } else {
            format!("descriptor '{}' for '{}' objects doesn't apply to a '{actual}' object", self.name, self.tname)
        }))
    }
}

/// As assinaturas de texto (`__text_signature__`) dos métodos dos tipos embutidos no CPython 3.13.
/// Cada linha é `tipo<TAB>nome[<TAB>assinatura]`, gerada no oráculo por `gen_builtin_method_sigs.py`
/// (os atributos chamáveis de `vars(tipo)`, só os que o tipo define); sem assinatura o método não tem (`None`).
const SIGNATURES: &str = include_str!("../data/cpython-docs/builtin-method-sigs.tsv");

type SignatureTable = std::collections::HashMap<(&'static str, &'static str), Option<&'static str>>;

fn signatures() -> &'static SignatureTable {
    static TABLE: std::sync::OnceLock<SignatureTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| crate::modules::cpydocs::parse_signature_table(SIGNATURES))
}

/// O tipo base de `tname` na cadeia de herança dos tipos embutidos (`bool` herda de `int`, cada exceção da
/// pai dela na tabela de exceções; os demais, de `object`).
fn builtin_base(tname: &str) -> Option<&'static str> {
    match tname {
        "object" => None,
        "bool" => Some("int"),
        _ => Some(
            crate::object::EXC_CLASSES.iter().find(|(n, _)| *n == tname).map(|(_, p)| *p).filter(|p| !p.is_empty()).unwrap_or("object"),
        ),
    }
}

/// O resultado de `find` para o primeiro tipo da cadeia de herança de `tname` que define o método
/// (o `__objclass__`).
fn from_defining_type<T>(tname: &str, find: impl Fn(&str) -> Option<T>) -> Option<T> {
    let mut owner = Some(tname);
    while let Some(t) = owner {
        if let Some(found) = find(t) {
            return Some(found);
        }
        owner = builtin_base(t);
    }
    None
}

/// O `__text_signature__` do método `name` do tipo embutido `tname`: o do primeiro tipo da cadeia de
/// herança que define o método (`__objclass__`).
pub(crate) fn text_signature(tname: &str, name: &str) -> Option<&'static str> {
    from_defining_type(tname, |t| signatures().get(&(t, name)).copied()).flatten()
}

/// O `__doc__` do método `name` do tipo embutido `tname` (`str.upper`, `int.__add__`): o do primeiro tipo
/// da cadeia de herança que define o método, da tabela do módulo `builtins`.
pub(crate) fn method_doc(tname: &str, name: &str) -> Option<String> {
    from_defining_type(tname, |t| crate::modules::cpydocs::builtin_doc_entry(&format!("{t}.{name}"))).flatten()
}

/// Dunders que o CPython define como método comum (`method_descriptor`) e não como slot do tipo.
const PLAIN_DUNDERS: &[&str] = &[
    "__format__", "__getnewargs__", "__sizeof__", "__reduce__", "__reduce_ex__", "__dir__", "__reversed__", "__round__",
    "__trunc__", "__floor__", "__ceil__", "__setstate__", "__getstate__", "__length_hint__", "__subclasshook__",
    "__init_subclass__", "__class_getitem__", "__missing__", "__bytes__", "__complex__", "__alloc__", "__getformat__",
    "__copy__", "__deepcopy__",
];

/// Slots que estes tipos expõem como método comum (`METH_COEXIST`): `dict.__getitem__` é `method_descriptor`.
const PLAIN_SLOTS: &[(&str, &str)] =
    &[("dict", "__getitem__"), ("dict", "__contains__"), ("list", "__getitem__"), ("set", "__contains__"), ("frozenset", "__contains__")];

/// O primeiro tipo depois de `tname` na cadeia de herança embutida que guarda `name` no próprio `__dict__`: é
/// onde `super(tname, obj).name` encontra o atributo.
pub(crate) fn inherited_owner(tname: &str, name: &str) -> Option<&'static str> {
    let mut owner = builtin_base(tname);
    while let Some(t) = owner {
        if crate::builtins_ext::own_type_keys(t).is_some_and(|keys| keys.contains(&name)) {
            return Some(t);
        }
        owner = builtin_base(t);
    }
    None
}

/// O tipo do descritor que o primeiro tipo da cadeia de herança de `tname` que define `name` guarda no
/// `__dict__` (`wrapper_descriptor`, `method_descriptor`, `classmethod_descriptor`...), da tabela do oráculo.
/// `None` quando o tipo não tem linha na tabela ou nenhum tipo da cadeia define o nome.
fn descriptor_kind(tname: &str, name: &str) -> Option<&'static str> {
    if !crate::builtins_ext::has_var_kinds(tname) {
        return None;
    }
    from_defining_type(tname, |t| crate::builtins_ext::type_var_kind(t, name))
}

/// O método `name` do tipo embutido `tname` é o wrapper de um slot (`int.__add__`, `str.__repr__`): vira
/// `wrapper_descriptor` no tipo e `method-wrapper` ligado a um objeto. Os tipos que a tabela do oráculo
/// cobre respondem por ela; os demais, pela lista de mágicos que são método comum.
pub(crate) fn is_slot_wrapper(tname: &str, name: &str) -> bool {
    if let Some(kind) = descriptor_kind(tname, name) {
        return kind == "wrapper_descriptor";
    }
    (attr_type(tname).is_some() || tname == "object" || is_exception_type(tname))
        && name.len() > 4
        && name.starts_with("__")
        && name.ends_with("__")
        && !PLAIN_DUNDERS.contains(&name)
        && !PLAIN_SLOTS.contains(&(tname, name))
}

/// O método `name` do tipo `tname` é de classe (`float.__getformat__`, `list.__class_getitem__`,
/// `object.__init_subclass__`): ligado a um objeto, o `__self__` dele é o tipo.
pub(crate) fn is_class_method(tname: &str, name: &str) -> bool {
    descriptor_kind(tname, name) == Some("classmethod_descriptor")
}

/// Refaz um destes objetos a partir da imagem do heap: o estado é `(tipo, nome)` e `type_attr` devolve o
/// mesmo objeto que o pai tinha (o do cache do filho, quando o tipo guarda um).
pub(crate) fn restore_image(tag: &str, state: &(dyn std::any::Any + Send + Sync), _refs: Vec<Value>) -> Option<Value> {
    let (tname, name) = *state.downcast_ref::<(&'static str, &'static str)>()?;
    if tag == "unbound" {
        return Some(unbound(tname, name));
    }
    type_attr(tname, name)
}

/// O objeto de tipo embutido `(tname, name)` como imagem do heap: refeito por [`type_attr`].
fn type_attr_image(tag: &'static str, tname: &'static str, name: &'static str) -> Option<crate::object::ExtImage> {
    crate::object::OpaqueImage::image(tag, (tname, name), Vec::new())
}

impl ExtObject for Unbound {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        if is_slot_wrapper(self.tname, self.name) {
            "wrapper_descriptor"
        } else {
            "method_descriptor"
        }
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        type_attr_image("unbound", self.tname, self.name)
    }
    fn repr(&self) -> String {
        if is_slot_wrapper(self.tname, self.name) {
            format!("<slot wrapper '{}' of '{}' objects>", self.name, self.tname)
        } else {
            format!("<method '{}' of '{}' objects>", self.name, self.tname)
        }
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__", "__get__"]
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" => Some(Ok(Value::str(self.name))),
            "__qualname__" => Some(Ok(Value::str(format!("{}.{}", self.tname, self.name)))),
            "__objclass__" => Some(Ok(objclass(vm, self.tname))),
            "__text_signature__" => Some(Ok(text_signature(self.tname, self.name).map_or(Value::None, Value::str))),
            "__doc__" => Some(Ok(method_doc(self.tname, self.name).map_or(Value::None, Value::str))),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, mut args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        // `str.upper.__get__(obj, tipo)`: sem objeto, o próprio descritor; com objeto, o método ligado a ele.
        if name == "__get__" {
            let obj = match args.first() {
                Some(o) => o.clone(),
                None => return Err(type_error("expected at least 1 argument, got 0")),
            };
            if matches!(obj, Value::None) {
                return Ok(unbound(self.tname, self.name));
            }
            self.check_receiver(&obj)?;
            // `object.__setattr__.__get__(o)`: o slot do próprio `object`, sem passar pela sobrescrita da
            // classe do receptor (o `attrs` guarda `object.__setattr__` para escrever em classe congelada).
            if self.tname == "object" {
                if let Some(bound) = crate::classes::plain_object_method(&obj, self.name) {
                    return Ok(bound);
                }
            }
            return vm.load_attr(&crate::vm::unwrap_payload(&obj), self.name);
        }
        if args.is_empty() {
            return Err(type_error(if is_slot_wrapper(self.tname, self.name) {
                format!("descriptor '{}' of '{}' object needs an argument", self.name, self.tname)
            } else {
                format!("unbound method {}.{}() needs an argument", self.tname, self.name)
            }));
        }
        let recv = args.remove(0);
        self.check_receiver(&recv)?;
        // `BaseException.__str__(e)` e `__repr__(e)`: o texto padrão da exceção, sem passar pela sobrescrita.
        if is_exception_type(self.tname) && matches!(self.name, "__repr__" | "__str__") {
            let is_str = self.name == "__str__";
            return match &recv {
                Value::Instance(_) => Ok(Value::str(vm.default_text(&recv, is_str))),
                Value::Exception(e) if is_str => Ok(Value::str(crate::object::exc_str(e))),
                Value::Exception(e) => Ok(Value::str(crate::object::exc_repr(e))),
                other => Err(type_error(format!(
                    "descriptor '{}' requires a '{}' object but received a '{}'",
                    self.name,
                    self.tname,
                    other.type_name()
                ))),
            };
        }
        // Os ganchos de atributo (`tuple.__getattribute__(self, nome)`) agem sobre a instância inteira: o
        // `__dict__` dela mora na instância, não no dado de dentro, que o `unwrap_payload` expõe. Os slots de
        // `object` são a implementação de `object` em si: `object.__init__(self)` dentro de um `__init__`
        // sobrescrito não pode voltar ao atributo do receptor.
        if self.tname == "object" || matches!(self.name, "__getattribute__" | "__setattr__" | "__delattr__") {
            if let Some(native) = object_attr(self.name) {
                args.insert(0, recv);
                return vm.call(&native, args, kw);
            }
        }
        // `type.__call__(Cls, *args)`: o slot `tp_call` de `type` instancia direto, sem passar pelo `__call__`
        // da metaclasse de `Cls` (que é quem costuma chamá-lo, via `super().__call__`).
        if let ("type", "__call__") = (self.tname, self.name) {
            return match &recv {
                Value::Class(c) => vm.instantiate_default(c, args, kw),
                other => vm.call(other, args, kw),
            };
        }
        if let ("type", "__instancecheck__" | "__subclasscheck__") = (self.tname, self.name) {
            let [arg] = <[Value; 1]>::try_from(args).map_err(|a| {
                type_error(format!("{}() takes exactly one argument ({} given)", self.name, a.len()))
            })?;
            return crate::builtins::real_type_check(vm, &recv, &arg, self.name == "__instancecheck__").map(Value::Bool);
        }
        // Os slots dos tipos de descritor (`wrapper_descriptor.__get__(d, obj, tipo)`, que o `inspect` chama pelo
        // tipo): o objeto nativo os atende.
        if let Value::Ext(e) = &recv {
            if e.type_name() == self.tname
                && matches!(
                    self.tname,
                    "getset_descriptor" | "member_descriptor" | "method_descriptor" | "wrapper_descriptor"
                        | "classmethod_descriptor" | "method-wrapper"
                )
            {
                return e.clone().call_method(vm, self.name, args, kw);
            }
        }
        // `dict.__getitem__(self, k)` numa subclasse que sobrescreve `__getitem__`: vale o método do tipo
        // embutido sobre o dado de dentro da instância, não a sobrescrita (senão recursa).
        let target = crate::vm::unwrap_payload(&recv);
        let bound = vm.load_attr(&target, self.name)?;
        vm.call(&bound, args, kw)
    }
}

/// O objeto de tipo `tname` como o módulo `builtins` o guarda (o shim em Python de `memoryview` e `complex`, ou
/// o tipo nativo): o `__objclass__` dos descritores.
fn objclass(vm: &mut Vm, tname: &'static str) -> Value {
    crate::modules::import(vm, "builtins")
        .and_then(|m| m.attrs.borrow().get(tname).cloned())
        .unwrap_or_else(|| type_object(tname))
}

/// Atributos de um método que o tipo já devolve preso a ele (`dict.fromkeys`, `int.__new__`): um
/// `builtin_function_or_method` cujo `__self__` é o tipo.
fn type_method_attr(tname: &'static str, name: &str, attr: &str) -> Option<PyResult<Value>> {
    match attr {
        "__name__" => Some(Ok(Value::str(name))),
        "__qualname__" => Some(Ok(Value::str(format!("{tname}.{name}")))),
        // `maketrans` é `staticmethod` no CPython: a função embutida não tem a quem se ligar.
        "__self__" if name == "maketrans" => Some(Ok(Value::None)),
        "__self__" => crate::builtins::get(tname).or_else(|| is_exception_type(tname).then_some(Value::Builtin(tname))).map(Ok),
        "__module__" => Some(Ok(Value::None)),
        "__text_signature__" => Some(Ok(text_signature(tname, name).map_or(Value::None, Value::str))),
        "__doc__" => Some(Ok(method_doc(tname, name).map_or(Value::None, Value::str))),
        _ => None,
    }
}

/// O `repr` de um método embutido do tipo que mora no endereço `at`: `<built-in method nome of type object at ...>`.
fn type_method_repr(at: i64, name: &str) -> String {
    format!("<built-in method {name} of type object at {at:#x}>")
}

/// O endereço do objeto de tipo embutido `tname` (0 enquanto o interpretador não o tem).
fn type_address(tname: &str) -> i64 {
    crate::builtins::get(tname).map_or(0, |t| crate::builtins::id_of(&t))
}

/// Construtor alternativo de um tipo embutido (`dict.fromkeys`, `int.from_bytes`, `str.maketrans`).
struct TypeClassMethod {
    tname: &'static str,
    name: &'static str,
    f: crate::object::NativeFnPtr,
}

impl ExtObject for TypeClassMethod {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn repr(&self) -> String {
        type_method_repr(type_address(self.tname), self.name)
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        type_attr_image("type_class_method", self.tname, self.name)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        type_method_attr(self.tname, self.name, name)
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        (self.f)(vm, args, kw)
    }
}

thread_local! {
    static CLASS_METHODS: std::cell::RefCell<Vec<(&'static str, &'static str, Value)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// O construtor alternativo `tname.name`: o mesmo objeto a cada leitura.
fn class_method(tname: &'static str, name: &'static str, f: crate::object::NativeFnPtr) -> Value {
    CLASS_METHODS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((_, _, v)) = m.iter().find(|(t, n, _)| *t == tname && *n == name) {
            return v.clone();
        }
        let v = Value::Ext(Rc::new(TypeClassMethod { tname, name, f }));
        m.push((tname, name, v.clone()));
        v
    })
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
    fn repr(&self) -> String {
        type_method_repr(type_address(self.tname), "__new__")
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        type_attr_image("new_fn", self.tname, "__new__")
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        type_method_attr(self.tname, "__new__", name)
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        if self.tname == "NoneType" {
            return match (args.len(), kw.is_empty()) {
                (1, true) => Ok(Value::None),
                (0, _) => Err(type_error("NoneType.__new__(): not enough arguments")),
                _ => Err(type_error("NoneType takes no arguments")),
            };
        }
        // `int.__new__(int, ...)`: o próprio tipo embutido, sem subclasse, é a chamada do construtor.
        if let Some(first) = args.first() {
            if crate::builtins::class_name(first).is_some_and(|n| n == self.tname) {
                let ctor = crate::builtins::get(self.tname).unwrap_or(Value::Builtin("object"));
                return vm.call(&ctor, args[1..].to_vec(), kw);
            }
        }
        let Some(Value::Class(c)) = args.first() else {
            return Err(type_error(format!("{}.__new__(X): X is not a type object", self.tname)));
        };
        let rest: Vec<Value> = args[1..].iter().map(crate::vm::unwrap_payload).collect();
        let ctor = crate::builtins::get(self.tname).unwrap_or(Value::Builtin("object"));
        let payload = vm.call(&ctor, rest, kw)?;
        Ok(Value::Instance(crate::object::InstanceObj::new_rc(c, Some(payload))))
    }
}

/// `list.__class_getitem__(item)`: o `Py_GenericAlias` dos contêineres embutidos genéricos.
struct ClassGetitem {
    tname: &'static str,
}

impl ExtObject for ClassGetitem {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn repr(&self) -> String {
        type_method_repr(type_address(self.tname), "__class_getitem__")
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        type_attr_image("class_getitem", self.tname, "__class_getitem__")
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        type_method_attr(self.tname, "__class_getitem__", name)
    }
    fn call_method(&self, _vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        crate::native_util::no_kwargs("__class_getitem__", &kw)?;
        match args.as_slice() {
            [key] => Ok(crate::generic::GenericAlias::make(crate::builtins::get(self.tname).unwrap_or(Value::Builtin("object")), key)),
            _ => Err(type_error(format!("__class_getitem__() takes exactly one argument ({} given)", args.len()))),
        }
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
    let mut d = Dict::default();
    for k in crate::vm::iterate(keys)? {
        d.set(k, value.clone())?;
    }
    Ok(Value::dict(d))
}

/// Os atributos de dados (`getset_descriptor` e `member_descriptor`) dos tipos numéricos e de `range`/`slice`.
const DATA_ATTRS: &[&str] = &["real", "imag", "numerator", "denominator", "start", "stop", "step"];

/// `bool.from_bytes(...)`: o `int.from_bytes` da base, com o resultado passado por `bool`.
fn bool_from_bytes(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    Ok(Value::Bool(int_from_bytes(vm, args, kw)?.is_true()))
}

/// `float.__getformat__(tipo)` lido pela classe: o receptor é o tipo, que a função de instância
/// (`methods::dunder::getformat`) espera em `args[0]`.
fn float_getformat(vm: &mut Vm, mut args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    args.insert(0, Value::Float(0.0));
    crate::methods::dunder::getformat(vm, args, kw)
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

/// `x * 2**k` sem estourar no meio do caminho: `x` já é exato e o resultado também.
fn ldexp(mut x: f64, mut k: i64) -> f64 {
    while k > 1000 {
        x *= 2f64.powi(1000);
        k -= 1000;
    }
    while k < -1000 {
        x *= 2f64.powi(-1000);
        k += 1000;
    }
    x * 2f64.powi(k as i32)
}

/// Os dígitos, o expoente binário e o sinal de um número hexadecimal (`0x1.8p3`), sem o `inf`/`nan`.
/// A mantissa guarda até 100 bits, com os dígitos que sobram resumidos num bit de arredondamento.
fn parse_hex_float(text: &str) -> Option<(bool, u128, i64)> {
    let invalid = || None;
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let rest = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")).unwrap_or(rest);
    let bytes = rest.as_bytes();
    let (mut mantissa, mut sticky, mut exp2, mut digits, mut at) = (0u128, false, 0i64, 0usize, 0usize);
    let mut fraction = false;
    while at < bytes.len() {
        match bytes[at] {
            b'.' if !fraction => fraction = true,
            b => {
                let Some(d) = (b as char).to_digit(16) else { break };
                digits += 1;
                if mantissa < 1 << 96 {
                    mantissa = mantissa * 16 + u128::from(d);
                    if fraction {
                        exp2 -= 4;
                    }
                } else {
                    sticky |= d != 0;
                    if !fraction {
                        exp2 += 4;
                    }
                }
            }
        }
        at += 1;
    }
    if digits == 0 {
        return invalid();
    }
    if matches!(bytes.get(at), Some(b'p' | b'P')) {
        at += 1;
        let (exp_negative, start) = match bytes.get(at) {
            Some(b'-') => (true, at + 1),
            Some(b'+') => (false, at + 1),
            _ => (false, at),
        };
        let exp_digits = &rest[start..];
        if exp_digits.is_empty() || !exp_digits.bytes().all(|b| b.is_ascii_digit()) {
            return invalid();
        }
        let magnitude = exp_digits.bytes().fold(0i64, |acc, b| (acc * 10 + i64::from(b - b'0')).min(1_000_000_000));
        exp2 += if exp_negative { -magnitude } else { magnitude };
        at = bytes.len();
    }
    if at != bytes.len() {
        return invalid();
    }
    Some((negative, mantissa | u128::from(sticky), exp2))
}

/// `float.fromhex(texto)` (`float_fromhex`): o arredondamento é o do IEEE (metade para o par), com
/// subnormais, e `OverflowError` quando o valor não cabe.
fn float_fromhex(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let Some(Value::Str(text)) = args.first() else {
        return Err(type_error(format!("must be str, not {}", args.first().map_or("None", Value::type_name))));
    };
    let text = text.as_str().trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'));
    let lowered = text.to_ascii_lowercase();
    let unsigned = lowered.trim_start_matches(['+', '-']);
    let negative = lowered.starts_with('-');
    if lowered.len() - unsigned.len() <= 1 && matches!(unsigned, "inf" | "infinity" | "nan") {
        let special = if unsigned == "nan" { f64::NAN } else { f64::INFINITY };
        return Ok(Value::Float(if negative { -special } else { special }));
    }
    let invalid = || crate::vm::exc("ValueError", "invalid hexadecimal floating-point string");
    let (negative, mantissa, exp2) = parse_hex_float(text).ok_or_else(invalid)?;
    let sign = if negative { -1.0 } else { 1.0 };
    if mantissa == 0 {
        return Ok(Value::Float(sign * 0.0));
    }
    let bits = i64::from(128 - mantissa.leading_zeros());
    let top = exp2 + bits - 1;
    let overflow = || crate::vm::exc("OverflowError", "hexadecimal value too large to represent as a float");
    if top > 1023 {
        return Err(overflow());
    }
    // O `f64` guarda 53 bits; abaixo de 2**-1022 o peso do último bit fica fixo em 2**-1074.
    let keep = if top >= -1022 { 53 } else { top + 1075 };
    if keep < 0 {
        return Ok(Value::Float(sign * 0.0));
    }
    let drop = bits - keep;
    let (quotient, lsb) = if drop <= 0 {
        (mantissa << (-drop), exp2 + drop)
    } else {
        let (quotient, rest, half) = (mantissa >> drop, mantissa & ((1u128 << drop) - 1), 1u128 << (drop - 1));
        let up = rest > half || (rest == half && quotient & 1 == 1);
        (quotient + u128::from(up), exp2 + drop)
    };
    let value = ldexp(quotient as f64, lsb);
    if value.is_infinite() {
        return Err(overflow());
    }
    Ok(Value::Float(sign * value))
}

fn str_maketrans(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let mut d = Dict::default();
    match args.as_slice() {
        [Value::Dict(src)] => {
            for (k, v) in src.borrow().iter() {
                let key = match k {
                    Value::Str(s) if s.len() == 1 => Value::Int(i64::from(s.cp_at(0).unwrap_or(0))),
                    Value::Str(_) => return Err(type_error("string keys in translate table must be of length 1")),
                    other => other.clone(),
                };
                d.set(key, v.clone())?;
            }
        }
        [Value::Str(a), Value::Str(b)] | [Value::Str(a), Value::Str(b), _] => {
            let (a, b): (Vec<u32>, Vec<u32>) = (code_points(a.as_str()).collect(), code_points(b.as_str()).collect());
            if a.len() != b.len() {
                return Err(crate::vm::exc("ValueError", "the first two maketrans arguments must have equal length"));
            }
            for (x, y) in a.iter().zip(b.iter()) {
                d.set(Value::Int(i64::from(*x)), Value::Int(i64::from(*y)))?;
            }
            if let [_, _, Value::Str(del)] = args.as_slice() {
                for c in code_points(del.as_str()) {
                    d.set(Value::Int(i64::from(c)), Value::None)?;
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
            i.set_own(n.as_str(), v.clone());
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
        [Value::Instance(i), Value::Str(n)] => match i.remove_own(n.as_str()) {
            Some(_) => Ok(Value::None),
            None => Err(crate::vm::exc("AttributeError", n.as_str().to_string())),
        },
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
        [other, Value::Str(n)] => vm.load_attr(other, n.as_str()),
        _ => Err(type_error("expected 2 arguments")),
    }
}

fn object_new(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match args.first() {
        Some(Value::Class(c)) => Ok(Value::Instance(crate::object::InstanceObj::new_rc(c, None))),
        // `object.__new__(object)`: o próprio `object` é o tipo, e a chamada é a do construtor.
        Some(Value::Builtin("object")) => vm.call(&args[0], args[1..].to_vec(), kw),
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
    let same = crate::object::is(&a, &b);
    match vm.call_dunder(&a, "__eq__", vec![b]) {
        Some(Ok(r)) if !crate::classes::is_not_implemented(&r) && r.is_true() => Ok(Value::Bool(false)),
        Some(Ok(Value::Bool(false))) => Ok(Value::Bool(true)),
        // Sem `__eq__` de usuário vale o `object.__eq__`: idêntico é igual, o resto não sabe responder.
        None if same => Ok(Value::Bool(false)),
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

/// `object.__lt__`, `__le__`, `__gt__` e `__ge__`: sempre `NotImplemented`.
fn object_order(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.len() {
        2 => Ok(crate::classes::not_implemented()),
        n => Err(type_error(format!("expected 1 argument, got {}", n.saturating_sub(1)))),
    }
}

/// `object.__format__(self, spec)`: `str(self)`, e só com especificação vazia.
fn object_format(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [a, Value::Str(spec)] if spec.as_str().is_empty() => Ok(Value::str(vm.str_of(a)?)),
        [a, Value::Str(_)] => {
            let ty = vm.type_of(a);
            let name = vm.load_attr(&ty, "__name__").ok().and_then(|n| match n {
                Value::Str(s) => Some(s.as_str().to_string()),
                _ => None,
            });
            Err(type_error(format!(
                "unsupported format string passed to {}.__format__",
                name.unwrap_or_else(|| a.type_name().to_string())
            )))
        }
        [_, other] => Err(type_error(format!("__format__() argument must be str, not {}", other.type_name()))),
        _ => Err(type_error(format!("__format__() takes exactly one argument ({} given)", args.len().saturating_sub(1)))),
    }
}

fn object_sizeof(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.len() {
        1 => Ok(Value::Int(16)),
        n => Err(type_error(format!("object.__sizeof__() takes no arguments ({} given)", n.saturating_sub(1)))),
    }
}

fn object_dir(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.as_slice() {
        [a] => Ok(crate::builtins_ext::merge_dir(vm, a)),
        _ => Err(type_error(format!("object.__dir__() takes no arguments ({} given)", args.len().saturating_sub(1)))),
    }
}

/// `object.__subclasshook__(cls)`: `NotImplemented`, a decisão fica com o mecanismo normal.
fn object_subclasshook(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match args.len() {
        1 => Ok(crate::classes::not_implemented()),
        n => Err(type_error(format!("object.__subclasshook__() takes exactly one argument ({n} given)"))),
    }
}

fn object_init_subclass(_vm: &mut Vm, _args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match kw.first() {
        Some((k, _)) => Err(type_error(format!("object.__init_subclass__() takes no keyword arguments ({k})"))),
        None => Ok(Value::None),
    }
}

/// `object.__reduce_ex__`, `__reduce__` e `__getstate__`: as funções de `copyreg`.
fn copyreg_call(vm: &mut Vm, fname: &str, args: Vec<Value>) -> PyResult<Value> {
    let f = crate::modules::pysrc::copyreg_helper(vm, fname)?;
    vm.call(&f, args, Vec::new())
}

fn object_reduce_ex(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    // O `__reduce__` de um shim em Python de um tipo de `builtins` (`memoryview`): o `__dict__` da classe mostra
    // ali o `method_descriptor` do oráculo, que o `copyreg` toma pelo `__reduce__` embutido e ignora; a função
    // do shim é a que representa o método de C, então vale como `__reduce__` próprio.
    if let Some(Value::Instance(inst)) = args.first() {
        if let Some(Value::Function(f)) = inst.class().lookup("__reduce__") {
            if f.is_builtin_type_method() {
                return vm.call(&Value::Function(f), vec![args[0].clone()], Vec::new());
            }
        }
    }
    copyreg_call(vm, "_object_reduce_ex", args)
}

fn object_reduce(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    copyreg_call(vm, "_object_reduce", args)
}

fn object_getstate(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    copyreg_call(vm, "_object_getstate", args)
}

/// Atributos de `object`: `object.__setattr__(self, nome, valor)` e companhia.
pub fn object_attr(name: &str) -> Option<Value> {
    Some(match name {
        "__lt__" => native("__lt__", object_order),
        "__le__" => native("__le__", object_order),
        "__gt__" => native("__gt__", object_order),
        "__ge__" => native("__ge__", object_order),
        "__format__" => native("__format__", object_format),
        "__sizeof__" => native("__sizeof__", object_sizeof),
        "__dir__" => native("__dir__", object_dir),
        "__subclasshook__" => crate::classes::object_class_method(Value::Builtin("object"), "__subclasshook__"),
        "__init_subclass__" => crate::classes::object_class_method(Value::Builtin("object"), "__init_subclass__"),
        "__reduce_ex__" => native("__reduce_ex__", object_reduce_ex),
        "__reduce__" => native("__reduce__", object_reduce),
        "__getstate__" => native("__getstate__", object_getstate),
        "__setattr__" => native("__setattr__", object_setattr),
        "__delattr__" => native("__delattr__", object_delattr),
        "__getattribute__" => native("__getattribute__", object_getattribute),
        "__new__" => crate::classes::object_class_method(Value::Builtin("object"), "__new__"),
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

/// O nativo de um dos métodos de classe de `object` (`__new__`, `__init_subclass__`, `__subclasshook__`).
pub(crate) fn object_class_native(name: &str) -> Option<crate::object::NativeFnPtr> {
    Some(match name {
        "__new__" => object_new,
        "__init_subclass__" => object_init_subclass,
        "__subclasshook__" => object_subclasshook,
        _ => return None,
    })
}

/// O tipo da cadeia de herança de `tname` que guarda `name` no próprio `__dict__`: `int` para
/// `bool.__add__`, `object` para `int.__setattr__` (é o `__objclass__` do descritor).
fn owner_of(tname: &'static str, name: &'static str) -> &'static str {
    thread_local! {
        static OWNERS: std::cell::RefCell<std::collections::HashMap<(&'static str, &'static str), &'static str>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    if let Some(owner) = OWNERS.with(|c| c.borrow().get(&(tname, name)).copied()) {
        return owner;
    }
    let mut owner = tname;
    loop {
        let own = crate::builtins_ext::own_type_keys(owner).is_some_and(|keys| keys.iter().any(|k| *k == name));
        match builtin_base(owner) {
            Some(base) if !own => owner = base,
            _ => break,
        }
    }
    OWNERS.with(|c| c.borrow_mut().insert((tname, name), owner));
    owner
}

fn is_exception_type(tname: &str) -> bool {
    crate::object::EXC_CLASSES.iter().any(|(n, _)| *n == tname)
}

/// O atributo `name` da classe de exceção embutida `tname`: o descritor que o `BaseException` (ou o
/// `OSError`, o `ImportError`, o `KeyError`...) guarda no próprio `__dict__`. Os métodos são
/// `method_descriptor` (`__repr__` e `__str__`, `wrapper_descriptor`) e os campos, `getset_descriptor`;
/// o `__objclass__` é o primeiro tipo da cadeia de herança que o define (`ValueError.__reduce__` é o de
/// `BaseException`).
pub(crate) fn exception_attr(tname: &str, name: &str) -> Option<Value> {
    const METHODS: &[&str] = &["__reduce__", "__setstate__", "__repr__", "__str__", "add_note", "with_traceback"];
    const FIELDS: &[&str] = &["args", "__cause__", "__context__", "__suppress_context__", "__traceback__"];
    let mut owner: &'static str = crate::object::EXC_CLASSES.iter().map(|(n, _)| *n).find(|n| *n == tname)?;
    loop {
        let own = crate::builtins_ext::own_type_keys(owner).is_some_and(|keys| keys.contains(&name));
        let parent = crate::object::EXC_CLASSES.iter().find(|(n, _)| *n == owner).map_or("", |(_, p)| *p);
        if own || parent.is_empty() {
            break;
        }
        owner = parent;
    }
    if let Some(method) = METHODS.iter().copied().find(|m| *m == name) {
        return Some(unbound(owner, method));
    }
    let field = FIELDS.iter().copied().find(|f| *f == name)?;
    Some(crate::classes::builtin_getset_descriptor(field, crate::builtins::get(owner).unwrap_or(Value::Builtin(owner))))
}

/// `vars(T)[key]` no tipo de descritor que o oráculo registra (`builtin-type-var-kinds.tsv`), para o que
/// `getattr(T, key)` não entrega: o descritor cru do `__dict__` do tipo. `ty` é o próprio tipo (o embutido ou
/// o shim em Python que o emula). `None` deixa o chamador usar o atributo comum (a docstring, o `__hash__`
/// que vale `None`). Os métodos e wrappers de slot são `method_descriptor` e `wrapper_descriptor`, os campos
/// `member_descriptor` e `getset_descriptor`, e o `__new__` um método embutido do tipo, para todo tipo da
/// tabela; os construtores alternativos são `classmethod_descriptor` e `staticmethod`.
pub(crate) fn descriptor_for_kind(tname: &'static str, key: &'static str, ty: &Value) -> Option<Value> {
    let kind = crate::builtins_ext::type_var_kind(tname, key)?;
    Some(match kind {
        "classmethod_descriptor" => Value::Ext(Rc::new(ClassMethodDescriptor { tname, name: key, ty: ty.clone() })),
        "staticmethod" => Value::Ext(Rc::new(StaticMethodHolder { tname, name: key, ty: ty.clone() })),
        "wrapper_descriptor" | "method_descriptor" => unbound(tname, key),
        "member_descriptor" | "getset_descriptor" => crate::classes::builtin_getset_descriptor(key, ty.clone()),
        // Os tipos que `type_attr` atende (`int`, `str`...) têm o `__new__` em cache próprio.
        "builtin_function_or_method" if key == "__new__" && attr_type(tname).is_none() => {
            Value::Ext(Rc::new(TypeNew { tname, ty: ty.clone() }))
        }
        _ => return None,
    })
}

/// `vars(T)['fromhex']`, `vars(T)['__class_getitem__']`: o `classmethod_descriptor` do tipo embutido. Lido
/// por `__get__` (ou chamado com o tipo na frente), entrega o método ligado ao tipo que `T.nome` dá.
struct ClassMethodDescriptor {
    tname: &'static str,
    name: &'static str,
    ty: Value,
}

impl ExtObject for ClassMethodDescriptor {
    fn type_name(&self) -> &'static str {
        "classmethod_descriptor"
    }
    fn repr(&self) -> String {
        format!("<method '{}' of '{}' objects>", self.name, self.tname)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__", "__get__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "__name__" => Some(Ok(Value::str(self.name))),
            "__qualname__" => Some(Ok(Value::str(format!("{}.{}", self.tname, self.name)))),
            "__objclass__" => Some(Ok(self.ty.clone())),
            "__text_signature__" => Some(Ok(text_signature(self.tname, self.name).map_or(Value::None, Value::str))),
            "__doc__" => Some(Ok(method_doc(self.tname, self.name).map_or(Value::None, Value::str))),
            _ => None,
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let bound = vm.load_attr(&self.ty, self.name)?;
        if name == "__get__" {
            return Ok(bound);
        }
        if args.is_empty() {
            return Err(type_error(format!("descriptor '{}' of '{}' object needs an argument", self.name, self.tname)));
        }
        vm.call(&bound, args[1..].to_vec(), kw)
    }
}

/// `vars(str)['maketrans']`: o `staticmethod` que embrulha a função do tipo.
struct StaticMethodHolder {
    tname: &'static str,
    name: &'static str,
    ty: Value,
}

impl ExtObject for StaticMethodHolder {
    fn type_name(&self) -> &'static str {
        "staticmethod"
    }
    fn repr(&self) -> String {
        format!("<staticmethod({})>", type_method_repr(type_address(self.tname), self.name))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__", "__get__"]
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        (name == "__wrapped__").then(|| vm.load_attr(&self.ty, self.name))
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let inner = vm.load_attr(&self.ty, self.name)?;
        if name == "__get__" {
            return Ok(inner);
        }
        vm.call(&inner, args, kw)
    }
}

/// `vars(ArithmeticError)['__new__']`, `vars(enumerate)['__new__']`: o `tp_new` do tipo, como método embutido
/// dele (`<built-in method __new__ of type object at ...>`). `ty` é o objeto de tipo: o embutido, ou o shim
/// em Python (que tem o `__new__` no próprio dicionário). A chamada vai para o `__new__` que a VM já resolve.
struct TypeNew {
    tname: &'static str,
    ty: Value,
}

impl ExtObject for TypeNew {
    fn type_name(&self) -> &'static str {
        "builtin_function_or_method"
    }
    fn repr(&self) -> String {
        type_method_repr(crate::builtins::id_of(&self.ty), "__new__")
    }
    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        type_method_attr(self.tname, "__new__", name)
    }
    fn call_method(&self, vm: &mut Vm, _name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let new = match &self.ty {
            // Ler `__new__` pela classe do shim devolveria este mesmo objeto: vale a função do próprio shim.
            Value::Class(c) => c.lookup("__new__").ok_or_else(|| type_error("object has no __new__"))?,
            ty => vm.load_attr(ty, "__new__")?,
        };
        vm.call(&new, args, kw)
    }
}

/// O atributo `name` do tipo embutido `tname`, se existir.
pub fn type_attr(tname: &str, name: &str) -> Option<Value> {
    let tname: &'static str = attr_type(tname)?;
    match (tname, name) {
        (_, "__name__" | "__qualname__") => return Some(Value::str(tname)),
        (_, "__module__") => return Some(Value::str("builtins")),
        (_, "__new__") => return Some(new_fn(tname)),
        ("int" | "bool", "real" | "imag" | "numerator" | "denominator") | ("float", "real" | "imag") | ("range" | "slice", "start" | "stop" | "step") => {
            let name = DATA_ATTRS.iter().copied().find(|n| *n == name)?;
            let owner = crate::builtins::get(tname).unwrap_or(Value::Builtin(tname));
            return Some(crate::classes::builtin_getset_descriptor(name, owner));
        }
        ("bool", "from_bytes") => return Some(class_method(tname, "from_bytes", bool_from_bytes)),
        // Os tipos mutáveis não têm hash: `list.__hash__` é `None`.
        ("list" | "dict" | "set" | "bytearray", "__hash__") => return Some(Value::None),
        ("dict", "fromkeys") => return Some(class_method(tname, "fromkeys", fromkeys)),
        ("list" | "dict" | "tuple" | "set" | "frozenset", "__class_getitem__") => {
            return Some(Value::Ext(Rc::new(ClassGetitem { tname })));
        }
        ("float", "__getformat__") => return Some(class_method(tname, "__getformat__", float_getformat)),
        ("float", "fromhex") => return Some(class_method(tname, "fromhex", float_fromhex)),
        ("slice", "indices") => return Some(unbound("slice", "indices")),
        ("int", "from_bytes") => return Some(class_method(tname, "from_bytes", int_from_bytes)),
        ("bytes", "fromhex") => return Some(class_method(tname, "fromhex", bytes_fromhex)),
        ("bytearray", "fromhex") => return Some(class_method(tname, "fromhex", bytearray_fromhex)),
        ("bytes" | "bytearray", "maketrans") => return Some(class_method(tname, "maketrans", bytes_maketrans)),
        ("str", "maketrans") => return Some(class_method(tname, "maketrans", str_maketrans)),
        _ => {}
    }
    // Os métodos de classe de `object` (`int.__init_subclass__`, `dict.__subclasshook__`) já saem ligados ao tipo:
    // chamados sem argumento (`super().__init_subclass__(**kw)`), não podem pedir o receptor como um descritor.
    if matches!(name, "__init_subclass__" | "__subclasshook__") {
        return Some(crate::classes::object_class_method(type_object(tname), crate::object::intern(name)));
    }
    if let Some((method, _)) = sample(tname).and_then(|s| crate::methods::lookup(&s, name)) {
        return Some(unbound(owner_of(tname, method), method));
    }
    // Os slots e métodos que a tabela do CPython lista para tipos sem amostra (`function.__get__`,
    // `type.__subclasscheck__`): o descritor despacha pelo receptor.
    crate::builtins_ext::type_var_kind(tname, name)
        .filter(|k| matches!(*k, "wrapper_descriptor" | "method_descriptor"))
        .map(|_| unbound(tname, crate::object::intern(name)))
}

/// O nome do slot ou método de `object` que `v` é (`object.__init__`, herdado por `A.__init__`), se for.
pub(crate) fn object_descriptor_name(v: &Value) -> Option<&'static str> {
    let Value::Ext(e) = v else { return None };
    let u = e.as_any()?.downcast_ref::<Unbound>()?;
    (u.tname == "object").then_some(u.name)
}

/// O atributo `name` de `object` lido pelo tipo (`object.__le__`, e o mesmo herdado por `V.__le__`): os slots e
/// métodos são descritores (`wrapper_descriptor`, `method_descriptor`), como no CPython, e um só objeto por
/// nome (o `functools.total_ordering` compara `getattr(cls, op) is getattr(object, op)`); o resto, o nativo.
pub(crate) fn object_type_attr(name: &str) -> Option<Value> {
    let native = object_attr(name)?;
    let is_descriptor = crate::builtins_ext::type_var_kind("object", name)
        .is_some_and(|k| matches!(k, "wrapper_descriptor" | "method_descriptor"));
    Some(if is_descriptor { unbound("object", crate::object::intern(name)) } else { native })
}

/// O descritor `tname.name` (`tname` é o tipo dono): um objeto só por par, para `dict.__repr__ is
/// dict.__repr__` e `bool.__add__ is int.__add__` valerem e o descritor servir de chave de dict (o `pprint`
/// despacha por `type(obj).__repr__`).
pub(crate) fn unbound(tname: &'static str, name: &'static str) -> Value {
    thread_local! {
        static UNBOUND: std::cell::RefCell<std::collections::HashMap<(&'static str, &'static str), Value>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    UNBOUND.with(|c| c.borrow_mut().entry((tname, name)).or_insert_with(|| Value::Ext(Rc::new(Unbound { tname, name }))).clone())
}

/// O objeto de tipo embutido `name`. Um tipo nativo sem objeto próprio (`list_iterator`) é registrado, para o
/// `repr` e o `isinstance` o tratarem como classe.
pub(crate) fn type_object(name: &'static str) -> Value {
    crate::builtins::get(name).unwrap_or_else(|| {
        if !crate::object::is_builtin_type(name) {
            crate::object::register_native_type(name);
        }
        Value::Builtin(name)
    })
}

/// O `__text_signature__` do próprio tipo embutido `tname` (`float.__text_signature__` é `(x=0, /)`): a
/// linha `__text_signature__` do tipo na tabela de assinaturas, sem subir pela herança.
pub(crate) fn type_text_signature(tname: &str) -> Option<&'static str> {
    signatures().get(&(tname, "__text_signature__")).copied().flatten()
}

/// O `repr` de um método embutido ligado a `recv`, como o `meth_repr` e o `wrapper_repr` do CPython:
/// `<method-wrapper '__eq__' of int object at ...>` para o slot, `<built-in method upper of str object at ...>`
/// para o método, e `of type object` quando o método é de classe (o `__self__` é o tipo).
pub(crate) fn bound_method_repr(recv: &Value, name: &str) -> String {
    let tname = recv.type_name();
    if is_slot_wrapper(tname, name) {
        format!("<method-wrapper '{name}' of {tname} object at {:#x}>", crate::builtins::id_of(recv))
    } else if is_class_method(tname, name) {
        format!("<built-in method {name} of type object at {:#x}>", crate::builtins::id_of(&type_object(tname)))
    } else {
        // O `tp_name` do tipo do receptor: `re.Pattern` e `_hashlib.HASH` levam o módulo C que os define.
        let shown = crate::object::native_type_owner(tname).map_or_else(|| tname.to_string(), |owner| format!("{owner}.{tname}"));
        format!("<built-in method {name} of {shown} object at {:#x}>", crate::builtins::id_of(recv))
    }
}

/// `__self__`, `__name__`, `__qualname__`, `__objclass__`, `__module__` e `__text_signature__` do método embutido
/// `name` ligado a `recv`. O wrapper de slot guarda o tipo que o define (`int.__eq__` de um `bool`, `object.__eq__`
/// de um iterador) e tem `__objclass__`; a função embutida usa o tipo do receptor e não tem `__objclass__`.
pub(crate) fn bound_method_attr(recv: &Value, name: &'static str, attr: &str) -> Option<Value> {
    let tname = recv.type_name();
    let wrapper = is_slot_wrapper(tname, name);
    Some(match attr {
        "__self__" if is_class_method(tname, name) => type_object(tname),
        "__self__" => recv.clone(),
        "__name__" => Value::str(name),
        "__qualname__" => Value::str(match recv {
            Value::Module(_) => name.to_string(),
            Value::Builtin(c) => format!("{c}.{name}"),
            Value::NativeFn(c) => format!("{}.{name}", c.name),
            Value::Class(c) => format!("{}.{name}", c.qualname()),
            _ if wrapper => format!("{}.{name}", owner_of(tname, name)),
            _ => format!("{tname}.{name}"),
        }),
        "__objclass__" if wrapper => {
            let owner = owner_of(tname, name);
            match recv {
                Value::Instance(i) if i.class().name == owner => Value::Class(i.class()),
                _ => type_object(owner),
            }
        }
        "__module__" => Value::None,
        "__text_signature__" => {
            if let Some(sig) = crate::modules::re::pattern_method_signature(tname, name) {
                return Some(Value::str(sig));
            }
            // O método de classe de um shim de tipo embutido (`memoryview._from_flags`): a assinatura é a do
            // tipo que o shim emula.
            let owner = match recv {
                Value::Class(c) if c.emulates_c_type() => c.name.as_str(),
                _ => tname,
            };
            text_signature(owner, name).map_or(Value::None, Value::str)
        }
        _ => return None,
    })
}

/// `__doc__` e `__new__` que um objeto de tipo nativo (iterador, exceção, função) herda e cujo `dir()` lista no
/// oráculo, sem que o objeto os implemente. `__new__` é o do tipo que o define (`OSError` para
/// `FileNotFoundError`), ou o de `object`.
pub(crate) fn inherited_type_attr(obj: &Value, name: &str) -> Option<Value> {
    let tname = obj.type_name();
    if !crate::builtins_ext::type_listed(tname, name) {
        return None;
    }
    match name {
        "__doc__" => Some(crate::modules::cpydocs::builtin_doc(tname).map_or(Value::None, Value::str)),
        "__new__" => {
            let owner = owner_of(tname, "__new__");
            Some(if owner == "object" {
                crate::classes::object_class_method(Value::Builtin("object"), "__new__")
            } else {
                Value::Ext(Rc::new(TypeNew { tname: owner, ty: Value::Builtin(owner) }))
            })
        }
        _ => None,
    }
}
