//! Funções embutidas registradas por tabela (`isinstance`, `map`, `getattr`...).
//!
//! Resolvidas por `LoadName` depois das globais do usuário e antes das classes de exceção. As
//! funções antigas (`print`, `len`, `sorted`...) ainda são despachadas por nome em `Vm::call`; ao
//! migrá-las, mova-as para esta tabela. Esta tabela tem precedência sobre as antigas.
//!
//! Onde o CPython devolve um iterador preguiçoso (`map`, `filter`, `zip`, `enumerate`, `reversed`)
//! devolvemos uma lista, porque o interpretador ainda não tem geradores. `iter()` devolve um objeto
//! iterador de verdade (`SeqIter`), que `next()` consome.

use crate::native_util::{bind, value_error, want_int};
use crate::object::{
    exc_is_subclass, repr, to_str, Dict, ExtObject, Kw, Native, NativeFn, NativeFnPtr, Set, Value,
    EXC_CLASSES,
};
use crate::vm::{exc, iterate, py_binary, py_lt, type_error, PyResult, Vm};
use std::cell::Cell;
use std::rc::Rc;

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("isinstance", b_isinstance),
    ("issubclass", b_issubclass),
    ("callable", b_callable),
    ("getattr", b_getattr),
    ("hasattr", b_hasattr),
    ("iter", b_iter),
    ("next", b_next),
    ("map", b_map),
    ("filter", b_filter),
    ("zip", b_zip),
    ("enumerate", b_enumerate),
    ("reversed", b_reversed),
    ("sorted", b_sorted),
    ("sum", b_sum),
    ("min", b_min),
    ("max", b_max),
    ("abs", b_abs),
    ("round", b_round),
    ("divmod", b_divmod),
    ("pow", b_pow),
    ("hex", b_hex),
    ("oct", b_oct),
    ("bin", b_bin),
    ("chr", b_chr),
    ("ord", b_ord),
    ("id", b_id),
    ("hash", b_hash),
    ("repr", b_repr),
    ("ascii", b_ascii),
    ("len", b_len),
    ("any", b_any),
    ("all", b_all),
    ("dict", b_dict),
    ("set", b_set),
    ("frozenset", b_frozenset),
    ("list", b_list),
    ("tuple", b_tuple),
    ("bytes", b_bytes),
    ("bytearray", b_bytearray),
    ("bool", b_bool),
    ("int", b_int),
    ("float", b_float),
    ("str", b_str),
];

/// A função embutida `name`, se existe na tabela.
pub fn get(name: &str) -> Option<Value> {
    TABLE
        .iter()
        .chain(crate::builtins_ext::TABLE.iter())
        .find(|(n, _)| *n == name)
        .map(|(n, f)| Value::NativeFn(Rc::new(NativeFn { name: n, f: *f })))
}

// ---------------------------------------------------------------------------------------------
// Utilidades de argumentos
// ---------------------------------------------------------------------------------------------

/// Recusa nomeados em funções que não os aceitam (texto do CPython para as embutidas).
fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("{fname}() takes no keyword arguments")))
    }
}

/// Exige entre `min` e `max` posicionais, com o texto "`f` expected at least 2 arguments, got 1".
fn expect(fname: &str, args: &[Value], min: usize, max: usize) -> PyResult<()> {
    let n = args.len();
    if n < min {
        let q = if min == max { "" } else { "at least " };
        return Err(type_error(format!("{fname} expected {q}{min} argument{}, got {n}", if min == 1 { "" } else { "s" })));
    }
    if n > max {
        let q = if min == max { "" } else { "at most " };
        return Err(type_error(format!("{fname} expected {q}{max} argument{}, got {n}", if max == 1 { "" } else { "s" })));
    }
    Ok(())
}

/// Um único argumento, no estilo "`f()` takes exactly one argument (N given)".
fn one(fname: &str, args: Vec<Value>, kw: &Kw) -> PyResult<Value> {
    nokw(fname, kw)?;
    let n = args.len();
    let mut it = args.into_iter();
    match (it.next(), it.next()) {
        (Some(v), None) => Ok(v),
        _ => Err(type_error(format!("{fname}() takes exactly one argument ({n} given)"))),
    }
}

/// `int` do valor (`int` ou `bool`).
fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Tipos e introspecção
// ---------------------------------------------------------------------------------------------

/// Nomes dos tipos embutidos que não são classes de exceção.
pub(crate) const TYPE_NAMES: &[&str] = &[
    "int", "str", "float", "bool", "list", "dict", "tuple", "set", "frozenset", "bytes", "bytearray", "range", "object",
    "slice",
];

/// Tipos que só existem como o resultado de `type(valor)` (`type(f)`, `type(sys)`...).
const PSEUDO_TYPES: &[&str] = &[
    "function", "module", "generator", "builtin_function_or_method", "method", "dict_keys", "dict_values", "dict_items", "coroutine",
    "async_generator", "coroutine_wrapper", "map", "filter", "zip", "enumerate", "reversed",
];

/// Nome da classe embutida representada por `v` (`Builtin` ou `NativeFn` de tipo), se for uma.
pub(crate) fn class_name(v: &Value) -> Option<&'static str> {
    match v {
        Value::Builtin(n) => {
            if TYPE_NAMES.contains(n) || PSEUDO_TYPES.contains(n) || matches!(*n, "NoneType" | "ellipsis" | "NotImplementedType" | "property" | "staticmethod" | "classmethod" | "super") || EXC_CLASSES.iter().any(|(e, _)| e == n) || crate::object::is_native_type(n) {
                Some(*n)
            } else {
                None
            }
        }
        Value::NativeFn(f) => {
            if TYPE_NAMES.contains(&f.name) || PSEUDO_TYPES.contains(&f.name) {
                Some(f.name)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `isinstance(v, cname)` para a classe embutida `cname`.
fn instance_of(v: &Value, cname: &str) -> bool {
    if let Value::Instance(i) = v {
        return cname == "object"
            || i.class().mro().iter().any(|c| {
                c.builtin_base.is_some_and(|b| subclass_of(b, cname))
                    || c.data_base.is_some_and(|d| d == cname || (d == "bool" && cname == "int"))
            });
    }
    match cname {
        "object" => true,
        "int" => matches!(v, Value::Int(_) | Value::Big(_) | Value::Bool(_)),
        "bool" => matches!(v, Value::Bool(_)),
        "float" => matches!(v, Value::Float(_)),
        "str" => matches!(v, Value::Str(_)),
        "list" => matches!(v, Value::List(_)),
        "dict" => matches!(v, Value::Dict(_)),
        "tuple" => matches!(v, Value::Tuple(_)),
        "range" => matches!(v, Value::Range(_)),
        "slice" => matches!(v, Value::Slice(_)),
        "set" => matches!(v, Value::Set(s) if !s.borrow().is_frozen()),
        "frozenset" => matches!(v, Value::Set(s) if s.borrow().is_frozen()),
        "bytes" => matches!(v, Value::Bytes(_)),
        "bytearray" => matches!(v, Value::ByteArray(_)),
        other if PSEUDO_TYPES.contains(&other) => v.type_name() == other,
        // Tipo de objeto nativo (`weakref.ReferenceType`, ...): o `type()` dele é o nome registrado.
        other if matches!(v, Value::Ext(_)) && (other == "super" || crate::object::is_native_type(other)) => v.type_name() == other,
        other => matches!(v, Value::Exception(e) if exc_is_subclass(e.kind, other)),
    }
}

fn isinstance_check(v: &Value, cls: &Value) -> PyResult<bool> {
    if let Some(args) = crate::generic::union_args(cls) {
        for c in &args {
            let c = if matches!(c, Value::None) { Value::Builtin("NoneType") } else { c.clone() };
            if isinstance_check(v, &c)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if matches!(cls, Value::Builtin("NoneType")) {
        return Ok(matches!(v, Value::None));
    }
    if matches!(cls, Value::Builtin("type")) {
        return Ok(match v {
            Value::Class(_) => true,
            Value::Builtin(n) => crate::object::is_builtin_type(n) || *n == "object" || *n == "type",
            Value::NativeFn(f) => crate::typeattrs::TYPES.contains(&f.name) || crate::object::is_builtin_type(f.name),
            _ => false,
        });
    }
    if let (Value::Instance(i), Value::Builtin("type")) = (v, cls) {
        return Ok(i.class().is_meta);
    }
    if let Value::Class(c) = cls {
        // `__instancecheck__` da metaclasse (`collections.abc`, protocolos).
        if let Some(mut vm) = crate::vm::current() {
            if let Some(r) = vm.meta_dunder(c, "__instancecheck__", vec![v.clone()], Vec::new()) {
                return Ok(r?.is_true());
            }
        }
        // Uma classe é instância da sua metaclasse (`isinstance(Color, EnumMeta)`).
        if let Value::Class(vc) = v {
            return Ok(vc.meta.as_ref().is_some_and(|m| m.mro().iter().any(|x| Rc::ptr_eq(x, c))));
        }
        // Objeto nativo cujo tipo é uma classe em Python do módulo dono (`re.Pattern`, `_hashlib.HASH`).
        if let Value::Ext(e) = v {
            if let Some(vm) = crate::vm::current() {
                if let Some(Value::Class(native)) = vm.native_python_class(e.type_name()) {
                    return Ok(native.mro().iter().any(|x| Rc::ptr_eq(x, c)));
                }
            }
        }
        if matches!(v, Value::Instance(i) if i.class().mro().iter().any(|x| Rc::ptr_eq(x, c))) {
            return Ok(true);
        }
        // Módulo com `__class__` trocado por uma subclasse de `ModuleType`.
        if matches!(v, Value::Module(m) if crate::classes::module_class(m).is_some_and(|k| k.mro().iter().any(|x| Rc::ptr_eq(x, c)))) {
            return Ok(true);
        }
        return reported_class_is_subtype(v, cls);
    }
    // `property`, `classmethod` e `staticmethod` são descritores nativos (objetos `Ext`).
    if let Value::Builtin(n @ ("property" | "classmethod" | "staticmethod")) = cls {
        return Ok(match v {
            // Instância de subclasse de `property`, `classmethod` ou `staticmethod` (o descritor vive no payload).
            Value::Instance(i) => i.class().data_base == Some(*n),
            Value::Ext(e) => matches!(
                (e.descriptor(), *n),
                (Some(crate::object::Descriptor::Property { .. }), "property")
                    | (Some(crate::object::Descriptor::Class(_)), "classmethod")
                    | (Some(crate::object::Descriptor::Static(_)), "staticmethod")
            ),
            _ => false,
        });
    }
    if let Some(c) = class_name(cls) {
        if instance_of(v, c) {
            return Ok(true);
        }
        return reported_class_is_subtype(v, cls);
    }
    if let Value::Tuple(t) = cls {
        for c in t.iter() {
            if isinstance_check(v, c)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    // Objeto comum cujo tipo define `__instancecheck__` (os apelidos do `typing`, como `Sequence`).
    if let Value::Instance(_) = cls {
        if let Some(mut vm) = crate::vm::current() {
            if let Some(r) = vm.call_dunder(cls, "__instancecheck__", vec![v.clone()]) {
                return Ok(r?.is_true());
            }
        }
    }
    Err(type_error("isinstance() arg 2 must be a type, a tuple of types, or a union"))
}

fn b_isinstance(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("isinstance", &kw)?;
    expect("isinstance", &args, 2, 2)?;
    Ok(Value::Bool(isinstance_check(&args[0], &args[1])?))
}

/// `issubclass(a, b)` entre classes embutidas.
fn subclass_of(a: &str, b: &str) -> bool {
    a == b || b == "object" || (a == "bool" && b == "int") || exc_is_subclass(a, b)
}

/// `PyType_IsSubtype(a, cls)`: a relação crua entre tipos, sem `__subclasscheck__`, para `cls` uma
/// classe de usuário ou um tipo embutido.
fn type_is_subtype(a: &Value, cls: &Value) -> bool {
    if let Value::Class(b) = cls {
        return matches!(a, Value::Class(c) if c.mro().iter().any(|x| Rc::ptr_eq(x, b)));
    }
    class_name(cls).is_some_and(|b| match a {
        Value::Class(c) => b == "object" || c.mro().iter().any(|x| x.builtin_base.is_some_and(|n| subclass_of(n, b))),
        other => class_name(other).is_some_and(|n| subclass_of(n, b)),
    })
}

/// O recurso do `object_isinstance` quando o tipo real não casa: o `obj.__class__` (um descritor da
/// classe, como o do `unittest.mock` com `spec`) pode ser um tipo que seja subtipo de `cls`.
fn reported_class_is_subtype(v: &Value, cls: &Value) -> PyResult<bool> {
    let Value::Instance(i) = v else { return Ok(false) };
    if i.class().lookup("__class__").is_none() {
        return Ok(false);
    }
    let Some(mut vm) = crate::vm::current() else { return Ok(false) };
    let reported = match vm.load_attr(v, "__class__") {
        Ok(r) => r,
        Err(e) if e.kind == "AttributeError" => return Ok(false),
        Err(e) => return Err(e),
    };
    if matches!(&reported, Value::Class(c) if Rc::ptr_eq(c, &i.class())) {
        return Ok(false);
    }
    Ok(type_is_subtype(&reported, cls))
}

fn issubclass_check(a: &Value, cls: &Value) -> PyResult<bool> {
    // `issubclass(M, type)`: só as metaclasses (e o próprio `type`).
    if matches!(cls, Value::Builtin("type")) {
        return Ok(match a {
            Value::Class(c) => c.is_meta || c.mro().iter().any(|x| x.is_meta),
            Value::Builtin("type") => true,
            _ => false,
        });
    }
    // O próprio `type` (é o `type(Classe)`): subclasse só de `type` e de `object`.
    if matches!(a, Value::Builtin("type")) {
        return Ok(match cls {
            Value::Builtin("object") => true,
            Value::Tuple(t) => t.iter().any(|c| matches!(c, Value::Builtin("type" | "object"))),
            _ => false,
        });
    }
    if let Value::Class(b) = cls {
        if let Some(mut vm) = crate::vm::current() {
            if let Some(r) = vm.meta_dunder(b, "__subclasscheck__", vec![a.clone()], Vec::new()) {
                return Ok(r?.is_true());
            }
        }
        return Ok(type_is_subtype(a, cls));
    }
    // `property` é descritor nativo (objeto `Ext`) e não tem `instance_of` próprio; só a própria e as subclasses casam.
    if matches!(cls, Value::Builtin("property")) {
        return Ok(match a {
            Value::Class(c) => c.mro().iter().any(|x| x.data_base == Some("property")),
            other => matches!(other, Value::Builtin("property")),
        });
    }
    if class_name(cls).is_some() {
        return Ok(type_is_subtype(a, cls));
    }
    if let Value::Tuple(t) = cls {
        for c in t.iter() {
            if issubclass_check(a, c)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if let Value::Instance(_) = cls {
        if let Some(mut vm) = crate::vm::current() {
            if let Some(r) = vm.call_dunder(cls, "__subclasscheck__", vec![a.clone()]) {
                return Ok(r?.is_true());
            }
        }
    }
    Err(type_error("issubclass() arg 2 must be a class, a tuple of classes, or a union"))
}

fn b_issubclass(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("issubclass", &kw)?;
    expect("issubclass", &args, 2, 2)?;
    if !matches!(args[0], Value::Class(_) | Value::Builtin("type" | "object")) && class_name(&args[0]).is_none() {
        return Err(type_error("issubclass() arg 1 must be a class"));
    }
    Ok(Value::Bool(issubclass_check(&args[0], &args[1])?))
}

pub(crate) fn is_callable(v: &Value) -> bool {
    match v {
        Value::Function(_) | Value::Builtin(_) | Value::NativeFn(_) | Value::Bound(_) | Value::BoundFn(_) | Value::Class(_) => true,
        Value::Instance(i) => matches!(i.class().lookup("__call__"), Some(Value::Function(_))),
        Value::Ext(e) => e.methods().contains(&"__call__"),
        _ => false,
    }
}

fn b_callable(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("callable", args, &kw)?;
    Ok(Value::Bool(is_callable(&v)))
}

fn attr_name(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        other => Err(type_error(format!("attribute name must be string, not '{}'", other.type_name()))),
    }
}

fn b_getattr(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("getattr", &kw)?;
    expect("getattr", &args, 2, 3)?;
    let name = attr_name(&args[1])?;
    match vm.load_attr(&args[0], &name) {
        Err(e) if e.kind == "AttributeError" && args.len() == 3 => Ok(args[2].clone()),
        other => other,
    }
}

fn b_hasattr(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("hasattr", &kw)?;
    expect("hasattr", &args, 2, 2)?;
    let name = attr_name(&args[1])?;
    match vm.load_attr(&args[0], &name) {
        Ok(_) => Ok(Value::Bool(true)),
        Err(e) if e.kind == "AttributeError" => Ok(Value::Bool(false)),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------------------------
// Iteradores
// ---------------------------------------------------------------------------------------------

/// Iterador devolvido por `iter()`: percorre uma cópia dos itens (o interpretador não tem
/// geradores, então a lista inteira é lida de uma vez).
struct SeqIter {
    items: Vec<Value>,
    pos: Cell<usize>,
    kind: &'static str,
}

/// O iterador de sequência (`iter(lista)`) numa posição dada, para a imagem do heap refazê-lo.
pub(crate) fn seq_iter(kind: &'static str, items: Vec<Value>, pos: usize) -> Value {
    Value::Ext(Rc::new(SeqIter { items, pos: Cell::new(pos), kind }))
}

impl ExtObject for SeqIter {
    fn type_name(&self) -> &'static str {
        self.kind
    }

    fn image(&self) -> Option<crate::object::ExtImage> {
        Some(crate::object::ExtImage::Lazy(crate::lazy::LazyParts::Seq {
            kind: self.kind,
            items: self.items.clone(),
            pos: self.pos.get(),
        }))
    }

    fn repr(&self) -> String {
        crate::lazy::object_repr(self.kind, self)
    }

    fn methods(&self) -> &'static [&'static str] {
        &["__next__", "__length_hint__", "__setstate__"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "__next__" => self.iter_next()?.ok_or_else(|| exc("StopIteration", "")),
            // O que falta percorrer.
            "__length_hint__" => Ok(Value::Int(self.items.len().saturating_sub(self.pos.get()) as i64)),
            // `__setstate__(índice)`: reposiciona o iterador, preso ao fim da sequência.
            "__setstate__" => {
                let [index] = <[Value; 1]>::try_from(args).map_err(|a| {
                    type_error(format!("__setstate__() takes exactly one argument ({} given)", a.len()))
                })?;
                let index = crate::native_util::want_int(&index)?.max(0) as usize;
                self.pos.set(index.min(self.items.len()));
                Ok(Value::None)
            }
            _ => Err(crate::object::no_attribute(self.kind, name)),
        }
    }

    fn is_iterable(&self) -> bool {
        true
    }

    fn iter_next(&self) -> PyResult<Option<Value>> {
        let i = self.pos.get();
        match self.items.get(i) {
            Some(v) => {
                self.pos.set(i + 1);
                Ok(Some(v.clone()))
            }
            None => Ok(None),
        }
    }
}

pub(crate) fn make_iter(v: &Value) -> PyResult<Value> {
    let kind = match v {
        Value::List(_) => "list_iterator",
        Value::Tuple(_) => "tuple_iterator",
        Value::Str(s) if s.as_str().is_ascii() => "str_ascii_iterator",
        Value::Str(_) => "str_iterator",
        Value::Range(_) => "range_iterator",
        Value::Dict(_) => "dict_keyiterator",
        Value::Set(_) => "set_iterator",
        Value::Bytes(_) => "bytes_iterator",
        Value::ByteArray(_) => "bytearray_iterator",
        // Iteradores e arquivos são o próprio iterador.
        Value::Ext(e) if e.is_iterable() => return Ok(v.clone()),
        Value::Native(n) if matches!(&*n.borrow(), Native::File(_) | Native::CsvReader { .. }) => {
            return Ok(v.clone());
        }
        // Instância: o `__iter__` dela (preguiçoso, pode ser infinito).
        Value::Instance(_) => {
            let mut vm = crate::vm::current().ok_or_else(|| exc("SystemError", "no vm"))?;
            return match vm.call_dunder(v, "__iter__", Vec::new()) {
                Some(r) => r,
                None => Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
            };
        }
        _ => "iterator",
    };
    let items = iterate(v)?;
    Ok(Value::Ext(Rc::new(SeqIter { items, pos: Cell::new(0), kind })))
}

fn b_iter(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("iter", &kw)?;
    expect("iter", &args, 1, 2)?;
    if args.len() == 1 {
        return make_iter(&args[0]);
    }
    if !is_callable(&args[0]) {
        return Err(type_error("iter(v, w): v must be callable"));
    }
    Ok(crate::lazy::CallIter::new(args[0].clone(), args[1].clone()))
}

fn stop_or(default: Option<Value>) -> PyResult<Value> {
    match default {
        Some(v) => Ok(v),
        None => Err(exc("StopIteration", "")),
    }
}

fn b_next(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("next", &kw)?;
    expect("next", &args, 1, 2)?;
    let default = args.get(1).cloned();
    match &args[0] {
        Value::Instance(_) => match vm.call_dunder(&args[0], "__next__", Vec::new()) {
            Some(Ok(v)) => Ok(v),
            Some(Err(e)) if e.kind == "StopIteration" => stop_or(default),
            Some(Err(e)) => Err(e),
            None => Err(type_error(format!("'{}' object is not an iterator", args[0].type_name()))),
        },
        Value::Ext(e) if e.is_iterable() => match e.iter_next()? {
            Some(v) => Ok(v),
            None => stop_or(default),
        },
        Value::Native(n) if matches!(&*n.borrow(), Native::File(_)) => {
            let f = vm.load_attr(&args[0], "readline")?;
            let line = vm.call(&f, Vec::new(), Vec::new())?;
            match &line {
                Value::Str(s) if s.as_str().is_empty() => stop_or(default),
                _ => Ok(line),
            }
        }
        Value::Native(n) if matches!(&*n.borrow(), Native::CsvReader { .. }) => match crate::vm::native_next(n)? {
            Some(v) => Ok(v),
            None => stop_or(default),
        },
        other => Err(type_error(format!("'{}' object is not an iterator", other.type_name()))),
    }
}

fn b_map(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("map", &kw)?;
    if args.len() < 2 {
        return Err(type_error("map() must have at least two arguments."));
    }
    crate::lazy::MapIter::new(args[0].clone(), &args[1..])
}

fn b_filter(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("filter", &kw)?;
    expect("filter", &args, 2, 2)?;
    crate::lazy::FilterIter::new(args[0].clone(), &args[1])
}

fn b_zip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let mut strict = false;
    for (k, v) in &kw {
        if k == "strict" {
            strict = v.is_true();
        } else {
            return Err(type_error(format!("'{k}' is an invalid keyword argument for zip()")));
        }
    }
    crate::lazy::ZipIter::new(&args, strict)
}

fn b_enumerate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("enumerate", args, kw, &["iterable", "start"], 1)?;
    let start = match &s[1] {
        Some(v) => want_int(v)?,
        None => 0,
    };
    let Some(src) = &s[0] else { return Err(type_error("enumerate() missing required argument 'iterable' (pos 1)")) };
    crate::lazy::EnumerateIter::new(src, start)
}

fn b_reversed(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("reversed", args, &kw)?;
    if let Value::Instance(inst) = &v {
        if inst.class().lookup("__reversed__").is_some() {
            let f = vm.load_attr(&v, "__reversed__")?;
            return vm.call(&f, Vec::new(), Vec::new());
        }
        // Subclasse de `list`/`tuple`/`str`: inverte o conteúdo guardado.
        let payload = inst.payload.borrow().clone();
        if let Some(p @ (Value::List(_) | Value::Tuple(_) | Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_))) = payload {
            let kind = if matches!(p, Value::List(_)) { "list_reverseiterator" } else { "reversed" };
            return Ok(crate::lazy::ReversedIter::new(kind, iterate(&p)?));
        }
        // Protocolo de sequência: `__len__` e `__getitem__`.
        let class = inst.class();
        if class.lookup("__len__").is_some() && class.lookup("__getitem__").is_some() {
            let n = vm.load_attr(&v, "__len__").and_then(|f| vm.call(&f, Vec::new(), Vec::new()))?;
            let getitem = vm.load_attr(&v, "__getitem__")?;
            let mut out = Vec::new();
            if let Value::Int(n) = n {
                for i in 0..n {
                    out.push(vm.call(&getitem, vec![Value::Int(i)], Vec::new())?);
                }
            }
            return Ok(crate::lazy::ReversedIter::new("reversed", out));
        }
    }
    match &v {
        Value::List(_) | Value::Tuple(_) | Value::Str(_) | Value::Range(_) | Value::Bytes(_) | Value::ByteArray(_) | Value::Dict(_) => {
            let kind = match &v {
                Value::List(_) => "list_reverseiterator",
                Value::Range(_) => "range_iterator",
                Value::Dict(_) => "dict_reversekeyiterator",
                _ => "reversed",
            };
            Ok(crate::lazy::ReversedIter::new(kind, iterate(&v)?))
        }
        other => Err(type_error(format!("'{}' object is not reversible", other.type_name()))),
    }
}

// ---------------------------------------------------------------------------------------------
// Ordenação, soma, mínimo e máximo
// ---------------------------------------------------------------------------------------------

/// Merge sort estável sobre pares (chave, valor); compara com `<` e propaga o erro. A comparação é
/// `direita < esquerda`, como o CPython, para a mensagem de `TypeError` sair na ordem dele.
fn merge_sort(mut v: Vec<(Value, Value)>) -> PyResult<Vec<(Value, Value)>> {
    if v.len() <= 1 {
        return Ok(v);
    }
    let right = v.split_off(v.len() / 2);
    let left = merge_sort(v)?;
    let right = merge_sort(right)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut l = left.into_iter().peekable();
    let mut r = right.into_iter().peekable();
    loop {
        let take_right = match (l.peek(), r.peek()) {
            (Some(a), Some(b)) => Some(py_lt(&b.0, &a.0)?),
            (Some(_), None) => Some(false),
            (None, Some(_)) => Some(true),
            (None, None) => None,
        };
        match take_right {
            Some(true) => out.extend(r.next()),
            Some(false) => out.extend(l.next()),
            None => break,
        }
    }
    Ok(out)
}

/// Ordena `items` (estável) pela chave `key` (ou pelo próprio valor), opcionalmente invertido. As chaves são
/// chamadas na ordem dos itens, antes de qualquer comparação e antes de inverter (`list.sort` do CPython).
pub fn sort_items(vm: &mut Vm, items: Vec<Value>, key: Option<Value>, reverse: bool) -> PyResult<Vec<Value>> {
    let mut pairs = Vec::with_capacity(items.len());
    for x in items {
        let k = match &key {
            Some(f) => vm.call(f, vec![x.clone()], Vec::new())?,
            None => x.clone(),
        };
        pairs.push((k, x));
    }
    sort_keyed(pairs, reverse)
}

/// Ordena os pares (chave, item) de forma estável pela chave, opcionalmente invertido, e devolve os itens.
pub(crate) fn sort_keyed(mut pairs: Vec<(Value, Value)>, reverse: bool) -> PyResult<Vec<Value>> {
    if reverse {
        pairs.reverse();
    }
    let sorted = merge_sort(pairs)?;
    let mut out: Vec<Value> = sorted.into_iter().map(|p| p.1).collect();
    if reverse {
        out.reverse();
    }
    Ok(out)
}

fn b_sorted(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if args.len() != 1 {
        return Err(type_error(format!("sorted expected 1 argument, got {}", args.len())));
    }
    let mut key = None;
    let mut reverse = false;
    for (k, v) in kw {
        match k.as_str() {
            "key" => {
                if !matches!(v, Value::None) {
                    key = Some(v);
                }
            }
            "reverse" => reverse = v.is_true(),
            _ => return Err(type_error(format!("sort() got an unexpected keyword argument '{k}'"))),
        }
    }
    let items = iterate(&args[0])?;
    Ok(Value::list(sort_items(vm, items, key, reverse)?))
}

fn b_sum(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if args.is_empty() {
        return Err(type_error("sum() takes at least 1 positional argument (0 given)"));
    }
    if args.len() > 2 {
        return Err(type_error(format!("sum() takes at most 2 arguments ({} given)", args.len())));
    }
    let mut start: Option<Value> = args.get(1).cloned();
    for (k, v) in kw {
        if k != "start" {
            return Err(type_error(format!("sum() got an unexpected keyword argument '{k}'")));
        }
        if start.is_some() {
            return Err(type_error("argument for sum() given by name ('start') and position"));
        }
        start = Some(v);
    }
    let it = crate::vm::get_iter(&args[0])?;
    let acc = start.unwrap_or(Value::Int(0));
    match &acc {
        Value::Str(_) => return Err(type_error("sum() can't sum strings [use ''.join(seq) instead]")),
        Value::Bytes(_) => return Err(type_error("sum() can't sum bytes [use b''.join(seq) instead]")),
        Value::ByteArray(_) => return Err(type_error("sum() can't sum bytearray [use b''.join(seq) instead]")),
        _ => {}
    }
    crate::fold::run(vm, crate::fold::Fold::sum(acc), it)
}

/// `a > b`, com a mensagem de `TypeError` do operador `>`.
pub(crate) fn py_gt(a: &Value, b: &Value) -> PyResult<bool> {
    match py_lt(b, a) {
        Err(e) if e.kind == "TypeError" && e.msg.starts_with("'<' not supported between instances of") => {
            Err(type_error(format!(
                "'>' not supported between instances of '{}' and '{}'",
                a.type_name(),
                b.type_name()
            )))
        }
        r => r,
    }
}

fn minmax(vm: &mut Vm, name: &'static str, args: Vec<Value>, kw: Kw, is_max: bool) -> PyResult<Value> {
    let mut key = None;
    let mut default = None;
    for (k, v) in kw {
        match k.as_str() {
            "key" => {
                if !matches!(v, Value::None) {
                    key = Some(v);
                }
            }
            "default" => default = Some(v),
            _ => return Err(type_error(format!("'{k}' is an invalid keyword argument for {name}()"))),
        }
    }
    if args.is_empty() {
        return Err(type_error(format!("{name} expected at least 1 argument, got 0")));
    }
    let it = if args.len() == 1 {
        crate::vm::get_iter(&args[0])?
    } else {
        if default.is_some() {
            return Err(type_error(format!("Cannot specify a default for {name}() with multiple positional arguments")));
        }
        crate::vm::get_iter(&Value::tuple(args))?
    };
    crate::fold::run(vm, crate::fold::Fold::min_max(is_max, key, default), it)
}

/// `min` e `max`: a mesma busca, com o nome e o sentido como parâmetro.
macro_rules! minmax_builtins {
    ($($f:ident = $name:literal, $is_max:literal;)*) => {$(
        fn $f(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            minmax(vm, $name, args, kw, $is_max)
        }
    )*};
}

minmax_builtins! {
    b_min = "min", false;
    b_max = "max", true;
}

fn b_any(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("any", args, &kw)?;
    crate::fold::run(vm, crate::fold::Fold::Any, crate::vm::get_iter(&v)?)
}

fn b_all(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("all", args, &kw)?;
    crate::fold::run(vm, crate::fold::Fold::All, crate::vm::get_iter(&v)?)
}

// ---------------------------------------------------------------------------------------------
// Números
// ---------------------------------------------------------------------------------------------

pub(crate) fn b_abs(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("abs", args, &kw)?;
    match v {
        Value::Int(i) => Ok(i.checked_abs().map_or_else(
            || crate::bigint::norm(num_traits::Signed::abs(&num_bigint::BigInt::from(i))),
            Value::Int,
        )),
        Value::Big(n) => Ok(crate::bigint::norm(num_traits::Signed::abs(&*n))),
        Value::Bool(b) => Ok(Value::Int(i64::from(b))),
        Value::Float(x) => Ok(Value::Float(x.abs())),
        other => match vm.call_dunder(&other, "__abs__", Vec::new()) {
            Some(r) => r,
            None => Err(type_error(format!("bad operand type for abs(): '{}'", other.type_name()))),
        },
    }
}

/// `round(x, nd)` de um float com `nd` dígitos: arredonda o valor exato para o par mais próximo
/// (por isso `round(2.675, 2)` dá `2.67`).
fn round_float(x: f64, nd: i64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if nd >= 0 {
        if nd > 323 {
            return x;
        }
        let s = format!("{:.*}", nd as usize, x);
        s.parse::<f64>().unwrap_or(x)
    } else {
        if nd < -308 {
            return 0.0 * x;
        }
        let p = 10f64.powi((-nd) as i32);
        (x / p).round_ties_even() * p
    }
}

/// `round(n, nd)` de um inteiro com `nd < 0`: múltiplo de `10**-nd` mais próximo, empate no par.
fn round_int(n: i64, nd: i64) -> PyResult<Value> {
    if nd >= 0 {
        return Ok(Value::Int(n));
    }
    let k = -nd;
    if k > 30 {
        return Ok(Value::Int(0));
    }
    let p = 10i128.pow(k as u32);
    let n = i128::from(n);
    let r = n.rem_euclid(p);
    let mut base = n - r;
    if r * 2 > p || (r * 2 == p && (base / p) % 2 != 0) {
        base += p;
    }
    Ok(crate::bigint::norm(num_bigint::BigInt::from(base)))
}

/// `round(n, nd)` de `int` grande com `nd < 0`.
fn round_big(n: &num_bigint::BigInt, nd: i64) -> Value {
    use num_integer::Integer;
    if nd >= 0 {
        return crate::bigint::norm(n.clone());
    }
    let p = num_traits::pow::Pow::pow(num_bigint::BigInt::from(10), (-nd) as u32);
    let r = n.mod_floor(&p);
    let mut base = n - &r;
    let twice = &r * 2;
    if twice > p || (twice == p && (&base / &p).is_odd()) {
        base += &p;
    }
    crate::bigint::norm(base)
}

pub(crate) fn b_round(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("round", args, kw, &["number", "ndigits"], 1)?;
    let number = s[0].clone().unwrap_or(Value::None);
    let nd = match &s[1] {
        None | Some(Value::None) => None,
        Some(v) => Some(want_int(v)?),
    };
    match (&number, nd) {
        (Value::Big(n), d) => Ok(round_big(n, d.unwrap_or(0))),
        (Value::Int(_) | Value::Bool(_), None) => Ok(Value::Int(as_i64(&number).unwrap_or(0))),
        (Value::Int(_) | Value::Bool(_), Some(d)) => round_int(as_i64(&number).unwrap_or(0), d),
        (Value::Float(x), None) => {
            if x.is_nan() {
                return Err(value_error("cannot convert float NaN to integer"));
            }
            if x.is_infinite() {
                return Err(exc("OverflowError", "cannot convert float infinity to integer"));
            }
            let r = x.round_ties_even();
            if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&r) {
                return Ok(crate::bigint::norm(crate::bigint::float_to_big(r).unwrap_or_default()));
            }
            Ok(Value::Int(r as i64))
        }
        (Value::Float(x), Some(d)) => Ok(Value::Float(round_float(*x, d))),
        _ => {
            let extra: Vec<Value> = nd.map(|d| vec![Value::Int(d)]).unwrap_or_default();
            match vm.call_dunder(&number, "__round__", extra) {
                Some(r) => r,
                None => Err(type_error(format!("type {} doesn't define __round__ method", number.type_name()))),
            }
        }
    }
}

fn b_divmod(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("divmod", &kw)?;
    expect("divmod", &args, 2, 2)?;
    let (a, b) = (&args[0], &args[1]);
    if let (Some(x), Some(y)) = (as_i64(a), as_i64(b)) {
        if y == 0 {
            return Err(exc("ZeroDivisionError", "integer division or modulo by zero"));
        }
        if x == i64::MIN && y == -1 {
            let q = crate::bigint::norm(-num_bigint::BigInt::from(x));
            return Ok(Value::tuple(vec![q, Value::Int(0)]));
        }
        let mut q = x / y;
        let mut r = x % y;
        if r != 0 && ((r < 0) != (y < 0)) {
            q -= 1;
            r += y;
        }
        return Ok(Value::tuple(vec![Value::Int(q), Value::Int(r)]));
    }
    let numeric = |v: &Value| matches!(v, Value::Int(_) | Value::Big(_) | Value::Bool(_) | Value::Float(_));
    if !numeric(a) || !numeric(b) {
        if let Some(r) = vm.call_dunder(a, "__divmod__", vec![b.clone()]) {
            return r;
        }
        return Err(type_error(format!(
            "unsupported operand type(s) for divmod(): '{}' and '{}'",
            a.type_name(),
            b.type_name()
        )));
    }
    let q = py_binary("//", a, b)?;
    let r = py_binary("%", a, b)?;
    Ok(Value::tuple(vec![q, r]))
}

/// `pow(b, e, m)` em precisão arbitrária; o sinal do resultado segue `m`.
fn big_pow_mod(b: &num_bigint::BigInt, e: &num_bigint::BigInt, m: &num_bigint::BigInt) -> PyResult<num_bigint::BigInt> {
    use num_integer::Integer;
    use num_traits::{Signed, Zero};
    if m.is_zero() {
        return Err(value_error("pow() 3rd argument cannot be 0"));
    }
    let mm = m.abs();
    let mut base = b.mod_floor(&mm);
    let mut e = e.clone();
    if e.is_negative() {
        // Inverso modular por Euclides estendido.
        let (mut r0, mut r1) = (mm.clone(), base.clone());
        let (mut t0, mut t1) = (num_bigint::BigInt::zero(), num_bigint::BigInt::from(1));
        while !r1.is_zero() {
            let q = &r0 / &r1;
            let r2 = &r0 - &q * &r1;
            (r0, r1) = (r1, r2);
            let t2 = &t0 - &q * &t1;
            (t0, t1) = (t1, t2);
        }
        if r0 != num_bigint::BigInt::from(1) {
            return Err(value_error("base is not invertible for the given modulus"));
        }
        base = t0.mod_floor(&mm);
        e = -e;
    }
    let mut result = base.modpow(&e, &mm);
    if m.is_negative() && !result.is_zero() {
        result += m;
    }
    Ok(result)
}

fn b_pow(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("pow", args, kw, &["base", "exp", "mod"], 2)?;
    let base = s[0].clone().unwrap_or(Value::None);
    let exp = s[1].clone().unwrap_or(Value::None);
    match &s[2] {
        None | Some(Value::None) => py_binary("**", &base, &exp),
        Some(m) => match (crate::bigint::as_big(&base), crate::bigint::as_big(&exp), crate::bigint::as_big(m)) {
            (Some(b), Some(e), Some(m)) => Ok(crate::bigint::norm(big_pow_mod(&b, &e, &m)?)),
            _ => Err(type_error("pow() 3rd argument not allowed unless all arguments are integers")),
        },
    }
}

fn radix_str(fname: &str, args: Vec<Value>, kw: Kw, prefix: &str, radix: u32) -> PyResult<Value> {
    let v = one(fname, args, &kw)?;
    let Some(n) = crate::bigint::as_big(&crate::vm::unwrap_payload(&v)) else {
        return Err(type_error(format!("'{}' object cannot be interpreted as an integer", v.type_name())));
    };
    let digits = crate::bigint::to_radix(&num_traits::Signed::abs(&n), radix);
    Ok(Value::str(format!("{}{prefix}{digits}", if num_traits::Signed::is_negative(&n) { "-" } else { "" })))
}

fn b_hex(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    radix_str("hex", args, kw, "0x", 16)
}

fn b_oct(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    radix_str("oct", args, kw, "0o", 8)
}

fn b_bin(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    radix_str("bin", args, kw, "0b", 2)
}

/// Os embutidos de um valor só que viram outro pela conversão `$conv(&valor)`.
macro_rules! convert_builtins {
    ($($f:ident = $name:literal, $conv:ident;)*) => {$(
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            $conv(&one($name, args, &kw)?)
        }
    )*};
}

convert_builtins! {
    b_chr = "chr", chr_of;
    b_ord = "ord", ord_of;
}

/// `chr(v)`: o código-ponto `v` (surrogates inclusive) como `str` de um caractere.
pub(crate) fn chr_of(v: &Value) -> PyResult<Value> {
    // Um int grande (positivo ou negativo) está fora do intervalo: `ValueError`, não `OverflowError`.
    let n = match v {
        Value::Big(_) => return Err(value_error("chr() arg not in range(0x110000)")),
        _ => want_int(v)?,
    };
    match u32::try_from(n) {
        Ok(cp) if cp <= 0x10_FFFF => Ok(Value::str(crate::object::cp_to_str(cp))),
        _ => Err(value_error("chr() arg not in range(0x110000)")),
    }
}

/// `ord(v)`: o código-ponto de um `str` de comprimento 1 (o par de escape conta como um) ou o
/// byte de um `bytes`/`bytearray` de comprimento 1.
pub(crate) fn ord_of(v: &Value) -> PyResult<Value> {
    let (len, first) = match v {
        Value::Str(s) => (s.len(), s.cp_at(0).map_or(0, i64::from)),
        Value::Bytes(b) => (b.len(), b.first().map_or(0, |x| i64::from(*x))),
        Value::ByteArray(b) => {
            let b = b.borrow();
            (b.len(), b.first().map_or(0, |x| i64::from(*x)))
        }
        other => return Err(type_error(format!("ord() expected string of length 1, but {} found", other.type_name()))),
    };
    if len == 1 {
        Ok(Value::Int(first))
    } else {
        Err(type_error(format!("ord() expected a character, but string of length {len} found")))
    }
}

// ---------------------------------------------------------------------------------------------
// Identidade, hash, repr, tamanho
// ---------------------------------------------------------------------------------------------

fn addr<T: ?Sized>(rc: &Rc<T>) -> i64 {
    crate::object::py_addr(Rc::as_ptr(rc) as *const () as usize) as i64
}

pub(crate) fn b_id(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("id", args, &kw)?;
    Ok(Value::Int(id_of(&v)))
}

/// `id(v)`: o endereço que o programa vê do objeto.
pub(crate) fn id_of(v: &Value) -> i64 {
    match v {
        Value::None => 0x7f00_0000_1000,
        Value::Bool(b) => 0x7f00_0000_2000 + i64::from(*b) * 32,
        // Os ints pequenos (-5..=256) são objetos estáticos do binário, contíguos de 32 em 32 bytes.
        Value::Int(i) if (-5..=256).contains(i) => 0xa4_2a10 + (i + 5) * 32,
        Value::Int(i) => 0x7f00_1000_0000_i64.wrapping_add(i.wrapping_mul(32)),
        Value::Float(x) => (x.to_bits() >> 4) as i64,
        Value::Big(b) => addr(b),
        Value::Range(r) => {
            let mixed = r.start.wrapping_mul(31).wrapping_add(r.stop).wrapping_mul(31).wrapping_add(r.step);
            crate::object::py_addr((mixed as usize).wrapping_mul(0x9E37_79B9) << 3) as i64
        }
        Value::Builtin(name) => crate::object::PyStr::new(*name).hash() >> 4,
        Value::Str(s) => addr(s),
        Value::Bytes(b) => addr(b),
        Value::ByteArray(b) => addr(b),
        Value::List(l) => addr(l),
        Value::Tuple(t) => addr(t),
        Value::Dict(d) => addr(d),
        Value::Set(s) => addr(s),
        Value::Exception(e) => addr(e),
        Value::Function(f) => addr(f),
        Value::Module(m) => addr(m),
        Value::NativeFn(n) => addr(n),
        Value::Ext(e) => addr(e),
        Value::Native(n) => addr(n),
        Value::Bound(b) => addr(b),
        Value::Class(c) => crate::object::py_type_addr(Rc::as_ptr(c) as usize) as i64,
        Value::Instance(i) => addr(i),
        Value::BoundFn(b) => addr(b),
        Value::Slice(s) => addr(s),
    }
}

fn b_hash(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("hash", args, &kw)?;
    Ok(Value::Int(crate::object::hash(&v)?))
}

fn b_repr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("repr", args, &kw)?;
    Ok(Value::str(repr(&v)))
}

fn b_ascii(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("ascii", args, &kw)?;
    let mut out = String::new();
    for c in repr(&v).chars() {
        let u = u32::from(c);
        if u < 128 {
            out.push(c);
        } else if u < 256 {
            out.push_str(&format!("\\x{u:02x}"));
        } else if u < 0x1_0000 {
            out.push_str(&format!("\\u{u:04x}"));
        } else {
            out.push_str(&format!("\\U{u:08x}"));
        }
    }
    Ok(Value::str(out))
}

fn b_len(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let v = one("len", args, &kw)?;
    let n = match &v {
        Value::Str(s) => s.len() as i64,
        Value::Bytes(b) => b.len() as i64,
        Value::ByteArray(b) => b.borrow().len() as i64,
        Value::List(l) => l.borrow().len() as i64,
        Value::Tuple(t) => t.len() as i64,
        Value::Dict(d) => d.borrow().len() as i64,
        Value::Set(s) => s.borrow().len() as i64,
        Value::Range(r) => r.len(),
        Value::Ext(e) if e.len().is_some() => e.len().unwrap_or(0) as i64,
        Value::Instance(_) | Value::Class(_) => crate::vm::len(&v)?,
        other => return Err(type_error(format!("object of type '{}' has no len()", other.type_name()))),
    };
    Ok(Value::Int(n))
}

// ---------------------------------------------------------------------------------------------
// Construtores de contêineres
// ---------------------------------------------------------------------------------------------

fn fill_dict(d: &mut Dict, src: &Value) -> PyResult<()> {
    if let Some(pairs) = crate::vm::mapping_pairs(src)? {
        for (k, v) in pairs {
            d.set(k, v)?;
        }
        return Ok(());
    }
    for (i, item) in iterate(src)?.into_iter().enumerate() {
        dict_item(d, i, item)?;
    }
    Ok(())
}

/// Um item de `dict(iterável)`: um par `(chave, valor)`; `index` é a posição dele, para a mensagem de erro.
pub(crate) fn dict_item(d: &mut Dict, index: usize, item: Value) -> PyResult<()> {
    let pair = iterate(&item)
        .map_err(|_| type_error(format!("cannot convert dictionary update sequence element #{index} to a sequence")))?;
    if pair.len() != 2 {
        return Err(value_error(format!(
            "dictionary update sequence element #{index} has length {}; 2 is required",
            pair.len()
        )));
    }
    let mut it = pair.into_iter();
    if let (Some(k), Some(v)) = (it.next(), it.next()) {
        d.set(k, v)?;
    }
    Ok(())
}

fn b_dict(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if args.len() > 1 {
        return Err(type_error(format!("dict expected at most 1 argument, got {}", args.len())));
    }
    let mut d = Dict::default();
    if let Some(src) = args.first() {
        fill_dict(&mut d, src)?;
    }
    for (k, v) in kw {
        d.set(Value::str(k), v)?;
    }
    Ok(Value::dict(d))
}

fn make_set(fname: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw(fname, &kw)?;
    expect(fname, &args, 0, 1)?;
    let mut s = Set::new();
    if let Some(src) = args.first() {
        crate::methods::setm::extend(&mut s, crate::methods::setm::gather(src)?)?;
    }
    Ok(if fname == "frozenset" { Value::frozenset(s) } else { Value::set(s) })
}

fn b_set(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    make_set("set", args, kw)
}

fn b_frozenset(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    make_set("frozenset", args, kw)
}

fn b_list(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("list", &kw)?;
    expect("list", &args, 0, 1)?;
    let items = match args.first() {
        Some(v) => iterate(v)?,
        None => Vec::new(),
    };
    Ok(Value::list(items))
}

fn b_tuple(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("tuple", &kw)?;
    expect("tuple", &args, 0, 1)?;
    let items = match args.first() {
        Some(v) => iterate(v)?,
        None => Vec::new(),
    };
    Ok(Value::tuple(items))
}

fn b_bool(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("bool", &kw)?;
    expect("bool", &args, 0, 1)?;
    Ok(Value::Bool(args.first().is_some_and(Value::is_true)))
}

// ---------------------------------------------------------------------------------------------
// Codificação de texto (bytes e str)
// ---------------------------------------------------------------------------------------------

/// Argumento de texto opcional (`encoding=`, `errors=`) com valor padrão.
fn opt_text(fname: &str, name: &str, v: &Option<Value>, default: &str) -> PyResult<String> {
    match v {
        None => Ok(default.to_string()),
        Some(Value::Str(s)) => Ok(s.as_str().to_string()),
        Some(other) => Err(type_error(format!("{fname}() argument '{name}' must be str, not {}", other.type_name()))),
    }
}

/// `bytearray(...)`: os mesmos argumentos de `bytes(...)`, com o resultado mutável.
fn b_bytearray(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match b_bytes(vm, args, kw)? {
        Value::Bytes(b) => Ok(Value::bytearray(b.to_vec())),
        other => Ok(other),
    }
}

fn b_bytes(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("bytes", args, kw, &["source", "encoding", "errors"], 0)?;
    let has_text_opts = s[1].is_some() || s[2].is_some();
    let Some(src) = &s[0] else {
        if has_text_opts {
            return Err(type_error("encoding or errors without sequence argument"));
        }
        return Ok(Value::bytes(Vec::new()));
    };
    if let Value::Str(st) = src {
        if s[1].is_none() {
            return Err(type_error("string argument without an encoding"));
        }
        let enc = opt_text("bytes", "encoding", &s[1], "utf-8")?;
        let errs = opt_text("bytes", "errors", &s[2], "strict")?;
        return Ok(Value::bytes(crate::methods::strm::encode_str(st.as_str(), &enc, &errs)?));
    }
    if s[1].is_some() {
        return Err(type_error("encoding without a string argument"));
    }
    if s[2].is_some() {
        return Err(type_error("errors without a string argument"));
    }
    match src {
        Value::Instance(_) if src.bytes_like().is_some() => Ok(Value::Bytes(src.bytes_like().unwrap_or_else(|| Rc::from(&[][..])))),
        Value::Int(_) | Value::Bool(_) => {
            let n = as_i64(src).unwrap_or(0);
            if n < 0 {
                return Err(value_error("negative count"));
            }
            if n > 1 << 31 {
                return Err(exc("MemoryError", ""));
            }
            Ok(Value::bytes(vec![0u8; n as usize]))
        }
        Value::Bytes(b) => Ok(Value::Bytes(b.clone())),
        Value::ByteArray(b) => Ok(Value::bytes(b.borrow().clone())),
        Value::Float(_) => Err(type_error("cannot convert 'float' object to bytes")),
        other => {
            let mut out = Vec::new();
            for x in iterate(other)? {
                let Some(i) = as_i64(&x) else {
                    return Err(type_error(format!("'{}' object cannot be interpreted as an integer", x.type_name())));
                };
                let byte = u8::try_from(i).map_err(|_| value_error("bytes must be in range(0, 256)"))?;
                out.push(byte);
            }
            Ok(Value::bytes(out))
        }
    }
}

fn b_str(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("str", args, kw, &["object", "encoding", "errors"], 0)?;
    let Some(obj) = &s[0] else { return Ok(Value::str("")) };
    if s[1].is_none() && s[2].is_none() {
        return Ok(Value::str(to_str(obj)));
    }
    match obj {
        Value::Bytes(_) | Value::ByteArray(_) => {
            let b = obj.bytes_like().unwrap_or_else(|| Rc::from(&[][..]));
            let enc = opt_text("str", "encoding", &s[1], "utf-8")?;
            let errs = opt_text("str", "errors", &s[2], "strict")?;
            Ok(Value::str(crate::methods::bytesm::decode_bytes(&b, &enc, &errs)?))
        }
        other => Err(type_error(format!("decoding to str: need a bytes-like object, {} found", other.type_name()))),
    }
}

// ---------------------------------------------------------------------------------------------
// int() e float()
// ---------------------------------------------------------------------------------------------

enum IntParseError {
    Invalid,
}

/// Literal inteiro na `base` (2 a 36, ou 0 para deduzir do prefixo): espaços em volta, sinal,
/// prefixo `0x`/`0o`/`0b` e `_` só entre dígitos.
fn parse_int_base(text: &str, base: u32) -> Result<Value, IntParseError> {
    let t = text.trim_matches(char::is_whitespace);
    let (neg, mut rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let bytes = rest.as_bytes();
    let prefix = if bytes.len() >= 2 && bytes[0] == b'0' {
        match bytes[1] {
            b'x' | b'X' => Some(16),
            b'o' | b'O' => Some(8),
            b'b' | b'B' => Some(2),
            _ => None,
        }
    } else {
        None
    };
    let mut base = base;
    let mut has_prefix = false;
    if base == 0 {
        match prefix {
            Some(p) => {
                base = p;
                rest = &rest[2..];
                has_prefix = true;
            }
            None => {
                base = 10;
                if rest.starts_with('0') && !rest.trim_matches(['0', '_']).is_empty() {
                    return Err(IntParseError::Invalid);
                }
            }
        }
    } else if prefix == Some(base) {
        rest = &rest[2..];
        has_prefix = true;
    }
    if has_prefix && rest.starts_with('_') {
        rest = &rest[1..];
    }
    if rest.is_empty() || rest.starts_with('_') || rest.ends_with('_') || rest.contains("__") {
        return Err(IntParseError::Invalid);
    }
    let mut digits = String::with_capacity(rest.len());
    for c in rest.chars() {
        if c == '_' {
            continue;
        }
        c.to_digit(base).ok_or(IntParseError::Invalid)?;
        digits.push(c);
    }
    let value = crate::bigint::parse(&digits, base).ok_or(IntParseError::Invalid)?;
    Ok(crate::bigint::norm(if neg { -value } else { value }))
}

fn int_from_float(x: f64) -> PyResult<Value> {
    if x.is_nan() {
        return Err(value_error("cannot convert float NaN to integer"));
    }
    if x.is_infinite() {
        return Err(exc("OverflowError", "cannot convert float infinity to integer"));
    }
    let t = x.trunc();
    if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&t) {
        return Ok(crate::bigint::norm(crate::bigint::float_to_big(t).unwrap_or_default()));
    }
    Ok(Value::Int(t as i64))
}

/// Texto de um `str` ou `bytes` para `int()`/`float()`.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.as_str().to_string()),
        Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        Value::ByteArray(b) => Some(String::from_utf8_lossy(&b.borrow()).into_owned()),
        _ => None,
    }
}

fn int_from_text(v: &Value, text: &str, base: u32) -> PyResult<Value> {
    let folded = crate::modules::unicodedata::fold_decimal_digits(text);
    let text = folded.as_deref().unwrap_or(text);
    match parse_int_base(text, base) {
        Ok(v) => Ok(v),
        Err(IntParseError::Invalid) => {
            Err(value_error(format!("invalid literal for int() with base {base}: {}", repr(v))))
        }
    }
}

pub(crate) fn b_int(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("int", args, kw, &["x", "base"], 0)?;
    let Some(x) = &s[0] else {
        if s[1].is_some() {
            return Err(type_error("int() missing string argument"));
        }
        return Ok(Value::Int(0));
    };
    if let Some(b) = &s[1] {
        let base = want_int(b)?;
        if base != 0 && !(2..=36).contains(&base) {
            return Err(value_error("int() base must be >= 2 and <= 36, or 0"));
        }
        let Some(text) = text_of(x) else {
            return Err(type_error("int() can't convert non-string with explicit base"));
        };
        return int_from_text(x, &text, base as u32);
    }
    match x {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Big(_) => Ok(x.clone()),
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Float(f) => int_from_float(*f),
        Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_) => {
            let text = text_of(x).unwrap_or_default();
            int_from_text(x, &text, 10)
        }
        other => {
            for name in ["__int__", "__index__", "__trunc__"] {
                match vm.call_dunder(other, name, Vec::new()) {
                    Some(Ok(r @ (Value::Int(_) | Value::Big(_)))) => return Ok(r),
                    Some(Ok(r @ Value::Bool(_))) => return b_int(vm, vec![r], Vec::new()),
                    Some(Ok(r)) if name == "__trunc__" => return b_int(vm, vec![r], Vec::new()),
                    Some(Ok(r)) => {
                        return Err(type_error(format!("{}.{name} returned non-int (type {})", other.type_name(), r.type_name())))
                    }
                    Some(Err(e)) => return Err(e),
                    None => {}
                }
            }
            // Subclasse de `int`/`float`/`str` (`IntEnum`...): vale o valor da base.
            if let Value::Instance(i) = other {
                let payload = i.payload.borrow().clone();
                if let Some(p @ (Value::Int(_) | Value::Big(_) | Value::Bool(_) | Value::Float(_) | Value::Str(_))) = payload {
                    return b_int(vm, vec![p], Vec::new());
                }
            }
            Err(type_error(format!(
                "int() argument must be a string, a bytes-like object or a real number, not '{}'",
                other.type_name()
            )))
        }
    }
}

/// Literal de ponto flutuante do `float(str)`: espaços em volta, `inf`, `nan`, expoente e `_` só
/// entre dígitos.
fn parse_float_text(text: &str) -> Option<f64> {
    let t = text.trim_matches(char::is_whitespace);
    if t.is_empty() {
        return None;
    }
    let bytes = t.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let prev = i > 0 && bytes[i - 1].is_ascii_digit();
            let next = i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
            if !(prev && next) {
                return None;
            }
        }
    }
    t.replace('_', "").parse::<f64>().ok()
}

fn b_float(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("float", &kw)?;
    expect("float", &args, 0, 1)?;
    match args.first() {
        None => Ok(Value::Float(0.0)),
        Some(Value::Int(i)) => Ok(Value::Float(*i as f64)),
        Some(Value::Big(n)) => Ok(Value::Float(crate::bigint::to_f64(n)?)),
        Some(Value::Bool(b)) => Ok(Value::Float(f64::from(u8::from(*b)))),
        Some(Value::Float(x)) => Ok(Value::Float(*x)),
        Some(v @ (Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_))) => {
            let text = text_of(v).unwrap_or_default();
            let text = crate::modules::unicodedata::fold_decimal_digits(&text).unwrap_or(text);
            parse_float_text(&text)
                .map(Value::Float)
                .ok_or_else(|| value_error(format!("could not convert string to float: {}", repr(v))))
        }
        Some(v) => {
            for name in ["__float__", "__index__"] {
                match vm.call_dunder(v, name, Vec::new()) {
                    Some(Ok(Value::Float(x))) if name == "__float__" => return Ok(Value::Float(x)),
                    Some(Ok(r @ (Value::Int(_) | Value::Big(_)))) if name == "__index__" => return b_float(vm, vec![r], Vec::new()),
                    Some(Ok(r)) => {
                        return Err(type_error(format!("{}.{name} returned non-{} (type {})", v.type_name(), if name == "__float__" { "float" } else { "int" }, r.type_name())))
                    }
                    Some(Err(e)) => return Err(e),
                    None => {}
                }
            }
            Err(type_error(format!(
            "float() argument must be a string or a real number, not '{}'",
            v.type_name()
        )))
        }
    }
}

#[cfg(test)]
mod tests {
    fn out(src: &str) -> String {
        let o = crate::run_source(src);
        assert_eq!(o.status, 0, "stderr: {}", o.stderr);
        String::from_utf8(o.stdout).unwrap()
    }

    #[test]
    fn isinstance_and_issubclass() {
        assert_eq!(
            out("print(isinstance(1, int), isinstance(True, int), isinstance(1, bool), isinstance('a', (int, str)), isinstance(1.0, int))"),
            "True True False True False\n"
        );
        assert_eq!(out("print(issubclass(bool, int), issubclass(int, bool), issubclass(KeyError, LookupError))"), "True False True\n");
        assert_eq!(out("print(isinstance(ValueError('x'), Exception), isinstance([], list), isinstance({}, dict))"), "True True True\n");
    }

    #[test]
    fn sorted_is_stable_with_key_and_reverse() {
        assert_eq!(out("print(sorted([3, 1, 2]))"), "[1, 2, 3]\n");
        assert_eq!(out("print(sorted([3, 1, 2], reverse=True))"), "[3, 2, 1]\n");
        assert_eq!(out("print(sorted(['bb', 'a', 'cc', 'd'], key=len))"), "['a', 'd', 'bb', 'cc']\n");
        assert_eq!(out("print(sorted(['bb', 'a', 'cc', 'd'], key=len, reverse=True))"), "['bb', 'cc', 'a', 'd']\n");
        assert_eq!(
            out("try:\n    sorted([1, 'a'])\nexcept TypeError as e:\n    print(e)"),
            "'<' not supported between instances of 'str' and 'int'\n"
        );
    }

    #[test]
    fn min_max_and_sum() {
        assert_eq!(out("print(min(3, 1, 2), max([4, 9, 2]), min([], default=7), max('abc'))"), "1 9 7 c\n");
        assert_eq!(out("print(max(['aa', 'b'], key=len))"), "aa\n");
        assert_eq!(
            out("try:\n    min([])\nexcept ValueError as e:\n    print(e)"),
            "min() iterable argument is empty\n"
        );
        assert_eq!(out("print(sum([1, 2, 3]), sum([1, 2], 10), sum([1, 2.5]))"), "6 13 3.5\n");
        assert_eq!(out("print(sum([0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]))"), "1.0\n");
    }

    #[test]
    fn rounding_and_numbers() {
        assert_eq!(out("print(round(2.5), round(3.5), round(2.675, 2), round(3.14159, 3))"), "2 4 2.67 3.142\n");
        assert_eq!(out("print(round(1234, -2), round(15, -1), round(25, -1))"), "1200 20 20\n");
        assert_eq!(out("print(pow(2, 10), pow(2, 10, 1000), pow(3, -1, 7))"), "1024 24 5\n");
        assert_eq!(out("print(divmod(7, 2), divmod(-7, 2), abs(-3))"), "(3, 1) (-4, 1) 3\n");
        assert_eq!(out("print(hex(255), hex(-255), oct(8), bin(5), chr(97), ord('a'))"), "0xff -0xff 0o10 0b101 a 97\n");
    }

    #[test]
    fn int_and_float_parsing() {
        assert_eq!(out("print(int('ff', 16), int('0x1f', 0), int(' 42 '), int('1_000'), int(3.9), int(-3.9))"), "255 31 42 1000 3 -3\n");
        assert_eq!(out("print(float('inf'), float(' 1e3 '), float('nan'), float('1_0'))"), "inf 1000.0 nan 10.0\n");
        assert_eq!(
            out("try:\n    int('x')\nexcept ValueError as e:\n    print(e)"),
            "invalid literal for int() with base 10: 'x'\n"
        );
        assert_eq!(
            out("try:\n    float('x')\nexcept ValueError as e:\n    print(e)"),
            "could not convert string to float: 'x'\n"
        );
        assert_eq!(
            out("try:\n    int('z', 16)\nexcept ValueError as e:\n    print(e)"),
            "invalid literal for int() with base 16: 'z'\n"
        );
    }

    #[test]
    fn containers() {
        assert_eq!(out("print(dict([(1, 2), (3, 4)]), dict(a=1, b=2))"), "{1: 2, 3: 4} {'a': 1, 'b': 2}\n");
        assert_eq!(out("print(sorted(set([3, 1, 3])), tuple([1, 2]), list('ab'))"), "[1, 3] (1, 2) ['a', 'b']\n");
        assert_eq!(out("print(bytes('h\u{e9}', 'utf-8'),bytes(3), bytes([104, 105]))"), "b'h\\xc3\\xa9' b'\\x00\\x00\\x00' b'hi'\n");
        assert_eq!(out("print(str(b'hi', 'utf-8'), bool([]), bool('a'))"), "hi False True\n");
    }

    #[test]
    fn eager_iteration_helpers() {
        assert_eq!(out("print(list(zip([1, 2], [3, 4])))"), "[(1, 3), (2, 4)]\n");
        assert_eq!(out("print(list(enumerate(['a', 'b'], start=1)))"), "[(1, 'a'), (2, 'b')]\n");
        assert_eq!(out("print(list(map(abs, [-1, 2])), list(filter(None, [0, 1, 2])), list(reversed([1, 2, 3])))"), "[1, 2] [1, 2] [3, 2, 1]\n");
    }

    #[test]
    fn iter_and_next() {
        assert_eq!(out("it = iter([1, 2])\nprint(next(it), next(it), next(it, 'end'))"), "1 2 end\n");
        assert_eq!(
            out("it = iter([1])\nnext(it)\ntry:\n    next(it)\nexcept StopIteration:\n    print('stop')"),
            "stop\n"
        );
        assert_eq!(out("total = 0\nfor x in iter([1, 2, 3]):\n    total += x\nprint(total)"), "6\n");
    }

    #[test]
    fn introspection_and_misc() {
        assert_eq!(out("print(hasattr('a', 'upper'), hasattr('a', 'nope'), getattr('a', 'nope', 5))"), "True False 5\n");
        assert_eq!(out("print(callable(len), callable(1))"), "True False\n");
        assert_eq!(out("print(any([0, 0, 1]), all([1, 0]), len('h\u{e9}llo'), repr('x'), ascii('\u{e9}'))"), "True False 5 'x' '\\xe9'\n");
        assert_eq!(out("a = [1]\nprint(id(a) == id(a), hash(1), hash((1, 2)) == hash((1, 2)))"), "True 1 True\n");
    }
}
