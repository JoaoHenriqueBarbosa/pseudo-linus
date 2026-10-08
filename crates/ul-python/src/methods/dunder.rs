//! Métodos mágicos de valores embutidos (`[].__len__`, `{}.__contains__`, `1 .__add__`, `(0).__index__`...):
//! cada um delega à mesma operação que a sintaxe usa, então `x.__len__()` e `len(x)` não podem divergir.
//!
//! Esta tabela é de todos os tipos. Quem decide se um tipo tem o método é o `dir()` dele no CPython
//! (`methods::lookup` confere a tabela do oráculo antes de chegar aqui), e a função só trata os receptores
//! que podem chegar até ela.

use crate::ast::UnaryOp;
use crate::native_util::want_int;
use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{exc, type_error, unwrap_payload, PyResult, Vm};

/// O `int` simples de um valor inteiro: `True` vira `1`, o resto segue como está.
pub(crate) fn plain_int(v: &Value) -> Value {
    match v {
        Value::Bool(b) => Value::Int(i64::from(*b)),
        other => other.clone(),
    }
}

/// O receptor e `n` argumentos, sem nomeados: o `check_num_args` dos wrappers de slot do CPython.
fn slot_args<'a>(name: &str, args: &'a [Value], kw: &Kw, n: usize) -> PyResult<&'a [Value]> {
    if !kw.is_empty() {
        return Err(type_error(format!("wrapper {name}() takes no keyword arguments")));
    }
    if args.len() != n + 1 {
        return Err(type_error(format!(
            "expected {n} argument{}, got {}",
            if n == 1 { "" } else { "s" },
            args.len().saturating_sub(1)
        )));
    }
    Ok(args)
}

/// O mesmo para os métodos comuns (`METH_NOARGS` e `METH_O`), com o texto deles.
fn method_args<'a>(qual: &str, args: &'a [Value], kw: &Kw, n: usize) -> PyResult<&'a [Value]> {
    if !kw.is_empty() {
        return Err(type_error(format!("{qual}() takes no keyword arguments")));
    }
    let given = args.len().saturating_sub(1);
    if given != n {
        return Err(type_error(if n == 0 {
            format!("{qual}() takes no arguments ({given} given)")
        } else {
            format!("{qual}() takes exactly one argument ({given} given)")
        }));
    }
    Ok(args)
}

/// `Tipo.metodo` do receptor, para o texto dos erros de aridade.
fn qualified(recv: &Value, name: &str) -> String {
    format!("{}.{name}", recv.type_name())
}

fn via_builtin(vm: &mut Vm, name: &str, args: Vec<Value>) -> PyResult<Value> {
    let f = crate::builtins::get(name).expect("builtin existe");
    vm.call(&f, args, Kw::new())
}

/// Os wrappers de slot e os métodos comuns: cada item confere a aridade e dá ao corpo o `Vm`, os
/// argumentos e o resultado da conferência (`a`, o receptor mais os `n` argumentos).
///
/// - `slot`: o texto de `check_num_args` dos wrappers de slot (`wrapper __len__() ...`);
/// - `method`: o texto de `METH_NOARGS`/`METH_O`, qualificado pelo tipo do receptor (`list.__sizeof__`);
/// - `fixed`: o mesmo, com o nome qualificado fixo (`bytes.__bytes__`).
macro_rules! checked_methods {
    (slot: $($f:ident = $name:literal, $n:literal, |$vm:ident, $args:ident, $a:ident| $body:expr;)*) => {$(
        fn $f($vm: &mut Vm, $args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let $a = slot_args($name, &$args, &kw, $n)?;
            $body
        }
    )*};
    (method: $($f:ident = $name:literal, $n:literal, |$vm:ident, $args:ident, $a:ident| $body:expr;)*) => {$(
        fn $f($vm: &mut Vm, $args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let $a = method_args(&qualified(&$args[0], $name), &$args, &kw, $n)?;
            $body
        }
    )*};
    (fixed: $($f:ident = $qual:literal, $n:literal, |$vm:ident, $args:ident, $a:ident| $body:expr;)*) => {$(
        fn $f($vm: &mut Vm, $args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let $a = method_args($qual, &$args, &kw, $n)?;
            $body
        }
    )*};
}

/// Os métodos que mudam só o lado do receptor (`__pow__`/`__rpow__`, `__divmod__`/`__rdivmod__`):
/// o nome, o lado e a função que faz a conta.
macro_rules! sided_slots {
    ($($f:ident = $imp:ident, $name:literal, $side:ident;)*) => {$(
        fn $f(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            $imp(vm, $name, Side::$side, args, kw)
        }
    )*};
}

/// Funções que repassam o receptor a um embutido de um argumento só (`len(x)`, `repr(x)`...).
macro_rules! builtin_slots {
    ($($f:ident = $name:literal => $builtin:literal;)*) => {$(
        fn $f(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            slot_args($name, &args, &kw, 0)?;
            via_builtin(vm, $builtin, args)
        }
    )*};
}

builtin_slots! {
    len = "__len__" => "len";
    iter = "__iter__" => "iter";
    hash = "__hash__" => "hash";
    repr = "__repr__" => "repr";
    str_ = "__str__" => "str";
    bool_ = "__bool__" => "bool";
    abs = "__abs__" => "abs";
    float_ = "__float__" => "float";
    next = "__next__" => "next";
}

/// Os operadores unários.
macro_rules! unary_slots {
    ($($f:ident = $name:literal, $op:expr;)*) => {$(
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            slot_args($name, &args, &kw, 0)?;
            crate::vm::unary($op, &args[0])
        }
    )*};
}

unary_slots! {
    neg = "__neg__", UnaryOp::USub;
    pos = "__pos__", UnaryOp::UAdd;
    invert = "__invert__", UnaryOp::Invert;
}

checked_methods! { slot:
    index = "__index__", 0, |_vm, args, _a| Ok(plain_int(&args[0]));
    int_ = "__int__", 0, |vm, args, _a| {
        if matches!(args[0], Value::Float(_)) {
            return via_builtin(vm, "int", args);
        }
        Ok(plain_int(&args[0]))
    };
    contains = "__contains__", 1, |_vm, _args, a| Ok(Value::Bool(crate::vm::contains(&a[0], &a[1])?));
    getitem = "__getitem__", 1, |_vm, _args, a| crate::vm::subscript(&a[0], &a[1]);
    setitem = "__setitem__", 2, |_vm, _args, a| {
        crate::vm::store_subscript(&a[0], &a[1], a[2].clone())?;
        Ok(Value::None)
    };
    delitem = "__delitem__", 1, |vm, _args, a| {
        vm.delete_subscript(&a[0], &a[1])?;
        Ok(Value::None)
    };
}

checked_methods! { method:
    reversed = "__reversed__", 0, |vm, args, _a| via_builtin(vm, "reversed", args);
}

fn compare(name: &str, sym: &str, args: &[Value], kw: &Kw) -> PyResult<Value> {
    let a = slot_args(name, args, kw, 1)?;
    let other = unwrap_payload(&a[1]);
    // `(1).__eq__(1.0)` é `NotImplemented`: o `int` só compara com `int`, e quem compara os dois é o `float`.
    if !crate::vm::rich_compare_accepts(&a[0], &other, !matches!(sym, "==" | "!=")) || !slot_accepts(&a[0], &other, sym, Side::Left)? {
        return Ok(crate::classes::not_implemented());
    }
    Ok(Value::Bool(crate::vm::py_compare(sym, &a[0], &other)?))
}

/// As comparações ricas: `NotImplemented` quando os tipos não se comparam.
macro_rules! compare_slots {
    ($($f:ident = $name:literal, $sym:literal;)*) => {$(
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            compare($name, $sym, &args, &kw)
        }
    )*};
}

compare_slots! {
    eq = "__eq__", "==";
    ne = "__ne__", "!=";
    lt = "__lt__", "<";
    le = "__le__", "<=";
    gt = "__gt__", ">";
    ge = "__ge__", ">=";
}

/// De que lado o receptor entra na operação: `x.__sub__(y)` é `x - y`, `x.__rsub__(y)` é `y - x` e
/// `x.__isub__(y)` é `x -= y`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Inplace,
}

/// O slot numérico do receptor aceita `other`? É o `CHECK_BINOP` do CPython: `(1).__add__(1.5)` devolve
/// `NotImplemented` (o `float` é quem sabe somar), enquanto os de sequência levantam o `TypeError` da operação.
fn slot_accepts(recv: &Value, other: &Value, sym: &str, side: Side) -> PyResult<bool> {
    Ok(match (recv, sym) {
        (Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_) | Value::List(_) | Value::Tuple(_), "*") => {
            want_int(other)?;
            true
        }
        // `texto % x` formata qualquer `x`; `x % texto` só quando `x` é texto do mesmo tipo.
        (Value::Str(_), "%") if side == Side::Right => matches!(other, Value::Str(_)),
        (Value::Bytes(_), "%") if side == Side::Right => matches!(other, Value::Bytes(_)),
        (Value::ByteArray(_), "%") if side == Side::Right => matches!(other, Value::ByteArray(_)),
        (Value::Int(_) | Value::Big(_) | Value::Bool(_), _) => {
            matches!(other, Value::Int(_) | Value::Big(_) | Value::Bool(_))
        }
        (Value::Float(_), _) => matches!(other, Value::Float(_) | Value::Int(_) | Value::Big(_) | Value::Bool(_)),
        (Value::Set(_), _) => matches!(other, Value::Set(_)),
        (Value::Dict(_), _) => side == Side::Inplace || matches!(other, Value::Dict(_)),
        _ => true,
    })
}

fn binary_slot(name: &str, sym: &str, side: Side, args: Vec<Value>, kw: &Kw) -> PyResult<Value> {
    let a = slot_args(name, &args, kw, 1)?;
    let other = unwrap_payload(&a[1]);
    if !slot_accepts(&a[0], &other, sym, side)? {
        return Ok(crate::classes::not_implemented());
    }
    match side {
        Side::Left => crate::vm::py_binary(sym, &a[0], &other),
        Side::Right => crate::vm::py_binary(sym, &other, &a[0]),
        Side::Inplace => crate::vm::py_binary_in(sym, &a[0], &other, true),
    }
}

/// Os operadores binários: o nome do método, o símbolo e o lado do receptor.
macro_rules! binary_slots {
    ($($f:ident = $name:literal, $sym:literal, $side:ident;)*) => {$(
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            binary_slot($name, $sym, Side::$side, args, &kw)
        }
    )*};
}

binary_slots! {
    add = "__add__", "+", Left;
    radd = "__radd__", "+", Right;
    iadd = "__iadd__", "+", Inplace;
    sub = "__sub__", "-", Left;
    rsub = "__rsub__", "-", Right;
    isub = "__isub__", "-", Inplace;
    mul = "__mul__", "*", Left;
    rmul = "__rmul__", "*", Right;
    imul = "__imul__", "*", Inplace;
    truediv = "__truediv__", "/", Left;
    rtruediv = "__rtruediv__", "/", Right;
    floordiv = "__floordiv__", "//", Left;
    rfloordiv = "__rfloordiv__", "//", Right;
    mod_ = "__mod__", "%", Left;
    rmod = "__rmod__", "%", Right;
    and_ = "__and__", "&", Left;
    rand = "__rand__", "&", Right;
    iand = "__iand__", "&", Inplace;
    or_ = "__or__", "|", Left;
    ror = "__ror__", "|", Right;
    ior = "__ior__", "|", Inplace;
    xor = "__xor__", "^", Left;
    rxor = "__rxor__", "^", Right;
    ixor = "__ixor__", "^", Inplace;
    lshift = "__lshift__", "<<", Left;
    rlshift = "__rlshift__", "<<", Right;
    rshift = "__rshift__", ">>", Left;
    rrshift = "__rrshift__", ">>", Right;
}

/// `__pow__` e `__rpow__`: `pow(base, expoente[, módulo])`, o módulo opcional.
fn pow_slot(vm: &mut Vm, name: &str, side: Side, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if !kw.is_empty() {
        return Err(type_error(format!("wrapper {name}() takes no keyword arguments")));
    }
    let given = args.len().saturating_sub(1);
    if !(1..=2).contains(&given) {
        return Err(type_error(if given == 0 {
            " expected at least 1 argument, got 0".to_string()
        } else {
            format!(" expected at most 2 arguments, got {given}")
        }));
    }
    let other = unwrap_payload(&args[1]);
    if !slot_accepts(&args[0], &other, "**", side)? {
        return Ok(crate::classes::not_implemented());
    }
    let (base, exponent) = if side == Side::Left { (args[0].clone(), other) } else { (other, args[0].clone()) };
    match args.get(2) {
        Some(modulus) if !matches!(modulus, Value::None) => via_builtin(vm, "pow", vec![base, exponent, modulus.clone()]),
        _ => crate::vm::py_binary("**", &base, &exponent),
    }
}

sided_slots! {
    pow = pow_slot, "__pow__", Left;
    rpow = pow_slot, "__rpow__", Right;
}

fn divmod_slot(vm: &mut Vm, name: &str, side: Side, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = slot_args(name, &args, &kw, 1)?;
    let other = unwrap_payload(&a[1]);
    if !slot_accepts(&a[0], &other, "divmod", side)? {
        return Ok(crate::classes::not_implemented());
    }
    let pair = if side == Side::Left { vec![a[0].clone(), other] } else { vec![other, a[0].clone()] };
    via_builtin(vm, "divmod", pair)
}

sided_slots! {
    divmod = divmod_slot, "__divmod__", Left;
    rdivmod = divmod_slot, "__rdivmod__", Right;
}

checked_methods! { method:
    format = "__format__", 1, |vm, args, a| {
        if !matches!(a[1], Value::Str(_)) {
            return Err(type_error(format!("__format__() argument must be str, not {}", a[1].type_name())));
        }
        via_builtin(vm, "format", args)
    };
    // `__getnewargs__`: os argumentos que `__new__` precisa para refazer o valor, `(valor,)`.
    getnewargs = "__getnewargs__", 0, |_vm, args, _a| Ok(Value::tuple(vec![plain_int(&args[0])]));
}

/// `float.__getformat__(tipo)`: o formato binário que o `float` do hospedeiro usa.
pub(crate) fn getformat(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = method_args("float.__getformat__", &args, &kw, 1)?;
    match &a[1] {
        Value::Str(s) if matches!(s.as_str(), "double" | "float") => Ok(Value::str("IEEE, little-endian")),
        Value::Str(_) => Err(exc("ValueError", "__getformat__() argument 1 must be 'double' or 'float'")),
        other => Err(type_error(format!("__getformat__() argument must be str, not {}", other.type_name()))),
    }
}

checked_methods! { fixed:
    bytes_ = "bytes.__bytes__", 0, |_vm, args, _a| Ok(args[0].clone());
    // `bytearray.__alloc__()`: os bytes reservados, o conteúdo mais o terminador.
    alloc = "bytearray.__alloc__", 0, |_vm, args, _a| {
        let len = match &args[0] {
            Value::ByteArray(b) => b.borrow().len(),
            _ => 0,
        };
        Ok(Value::Int(i64::try_from(len + 1).unwrap_or(i64::MAX)))
    };
}

checked_methods! { slot:
    // `__buffer__(flags)`: a visão do conteúdo, um `memoryview`.
    buffer = "__buffer__", 1, |vm, _args, a| {
        want_int(&a[1])?;
        // `memoryview` é uma classe em Python (`modules/py/_memoryview.py`), não está na tabela de embutidos.
        let cls = crate::modules::import(vm, "_memoryview")
            .and_then(|m| m.attrs.borrow().get("memoryview").cloned())
            .ok_or_else(|| exc("NameError", "name 'memoryview' is not defined"))?;
        vm.call(&cls, vec![a[0].clone()], Kw::new())
    };
    release_buffer = "__release_buffer__", 1, |_vm, _args, _a| Ok(Value::None);
}

// `__del__` de um gerador, de uma corrente e dos objetos que fecham ao ser finalizados: roda o `close()`.
checked_methods! { slot:
    finalize = "__del__", 0, |vm, args, _a| {
        if let Ok(close) = vm.load_attr(&args[0], "close") {
            vm.call(&close, Vec::new(), Kw::new())?;
        }
        Ok(Value::None)
    };
}

// `__setstate__` dos iteradores `zip` e `reversed`.
checked_methods! { method:
    setstate = "__setstate__", 1, |_vm, args, a| {
        let recv = &args[0];
        match recv {
            Value::Ext(e) => match crate::lazy::set_state(&**e, &a[1]) {
                Some(done) => done.map(|()| Value::None),
                None => Err(crate::object::no_attribute(recv.type_name(), "__setstate__")),
            },
            _ => Err(crate::object::no_attribute(recv.type_name(), "__setstate__")),
        }
    };
}

/// `list[int]`, `dict[str, int]`: o `__class_getitem__` do tipo, lido pela instância.
fn class_getitem(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let tname = args[0].type_name();
    let Some(f) = crate::typeattrs::type_attr(tname, "__class_getitem__") else {
        return Err(crate::object::no_attribute(tname, "__class_getitem__"));
    };
    vm.call(&f, args[1..].to_vec(), kw)
}

/// Quantos bytes o CPython 3.13 reserva para os dígitos de 30 bits de um `int`.
fn int_digits(v: &Value) -> i64 {
    crate::bigint::as_big(v).map_or(0, |n| i64::try_from(n.bits().div_ceil(30)).unwrap_or(i64::MAX))
}

/// `sys.getsizeof` sem o cabeçalho do coletor: o `__sizeof__` do valor.
fn sizeof_of(v: &Value) -> i64 {
    let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    match v {
        Value::None => 16,
        Value::Bool(_) => 28,
        Value::Int(_) | Value::Big(_) => 24 + 4 * int_digits(v).max(1),
        Value::Float(_) => 24,
        Value::Str(s) => {
            let text = s.as_str();
            let chars = count(text.chars().count());
            if text.is_ascii() {
                41 + chars
            } else {
                let widest = text.chars().map(u32::from).max().unwrap_or(0);
                let width = if widest <= 0xff { 1 } else if widest <= 0xffff { 2 } else { 4 };
                56 + (chars + 1) * width
            }
        }
        Value::Bytes(b) => 33 + count(b.len()),
        Value::ByteArray(b) => {
            let len = b.borrow().len();
            56 + if len == 0 { 0 } else { count(len + 1) }
        }
        Value::List(l) => 40 + 8 * count(l.borrow().len()),
        Value::Tuple(t) => 24 + 8 * count(t.len()),
        // A tabela de hash do `dict` e do `set` cresce em potências de dois: até cinco itens cabem na
        // tabela inicial de oito posições.
        Value::Dict(d) => {
            let len = d.borrow().len();
            if len == 0 {
                48
            } else {
                let slots = (len * 3).div_ceil(2).next_power_of_two().max(8);
                let entries = slots * 2 / 3;
                let entry = if d.borrow().iter().all(|(k, _)| matches!(k, Value::Str(_))) { 16 } else { 24 };
                48 + 32 + count(slots) + count(entries) * entry
            }
        }
        Value::Set(s) => {
            let len = s.borrow().len();
            if len <= 5 {
                200
            } else {
                72 + 16 * count((len * 5).div_ceil(3).next_power_of_two().max(8))
            }
        }
        Value::Range(_) => 48,
        Value::Slice(_) => 40,
        _ => 16,
    }
}

checked_methods! { method:
    sizeof = "__sizeof__", 0, |_vm, args, _a| Ok(Value::Int(sizeof_of(&args[0])));
}

/// `__init__` do tipo: `list`, `set` e `bytearray` refazem o conteúdo no próprio objeto
/// (`l.__init__([1])`), o `dict` acrescenta ao que tem; nos outros é o `object.__init__`, que não faz nada.
fn init(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let recv = args[0].clone();
    let mutable = match &recv {
        Value::List(_) | Value::Dict(_) | Value::ByteArray(_) => true,
        Value::Set(s) => !s.borrow().is_frozen(),
        _ => false,
    };
    let Some(ctor) = crate::builtins::get(recv.type_name()).filter(|_| mutable) else {
        return Ok(Value::None);
    };
    let fresh = vm.call(&ctor, args[1..].to_vec(), kw)?;
    match (&recv, &fresh) {
        (Value::List(d), Value::List(f)) => *d.borrow_mut() = f.borrow().clone(),
        // `dict.__init__` acrescenta ao que o dicionário já tem, como `update`.
        (Value::Dict(d), Value::Dict(f)) => {
            for (k, v) in f.borrow().iter() {
                d.borrow_mut().set(k.clone(), v.clone())?;
            }
        }
        (Value::Set(d), Value::Set(f)) => *d.borrow_mut() = f.borrow().clone(),
        (Value::ByteArray(d), Value::ByteArray(f)) => *d.borrow_mut() = f.borrow().clone(),
        _ => {}
    }
    Ok(Value::None)
}

/// O nome de atributo de `getattr`/`setattr`: tem de ser texto.
fn attr_name(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        other => Err(type_error(format!("attribute name must be string, not '{}'", other.type_name()))),
    }
}

checked_methods! { slot:
    setattr = "__setattr__", 2, |vm, _args, a| {
        vm.store_attr(&a[0], &attr_name(&a[1])?, a[2].clone())?;
        Ok(Value::None)
    };
    delattr = "__delattr__", 1, |vm, _args, a| {
        vm.delete_attr(&a[0], &attr_name(&a[1])?)?;
        Ok(Value::None)
    };
    getattribute = "__getattribute__", 1, |vm, _args, a| vm.load_attr(&a[0], &attr_name(&a[1])?);
}

/// Os métodos de `object` que o valor embutido herda sem mudar: a conta é a mesma do `object`.
macro_rules! object_methods {
    ($($f:ident = $name:literal, $n:literal;)*) => {$(
        fn $f(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let recv = args.first().map_or("object", Value::type_name);
            method_args(&format!("{recv}.{}", $name), &args, &kw, $n)?;
            let method = crate::typeattrs::object_attr($name).expect("object define o método");
            vm.call(&method, args, kw)
        }
    )*};
}

object_methods! {
    dir = "__dir__", 0;
    getstate = "__getstate__", 0;
}

/// `__reduce__` e `__reduce_ex__`: os valores embutidos que não têm classe em Python (`range`, `slice`,
/// `bytearray`) saem pelo `copyreg._builtin_reduce*`; os demais usam o método de `object`.
fn reduce_with(vm: &mut Vm, name: &'static str, n: usize, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let recv = args.first().map_or("object", Value::type_name);
    method_args(&format!("{recv}.{name}"), &args, &kw, n)?;
    let method = if matches!(args.first(), Some(Value::Range(_) | Value::ByteArray(_) | Value::Slice(_))) {
        let reducer = if name == "__reduce__" { "_builtin_reduce" } else { "_builtin_reduce_ex" };
        crate::modules::pysrc::copyreg_helper(vm, reducer)?
    } else {
        crate::typeattrs::object_attr(name).expect("object define o método")
    };
    vm.call(&method, args, kw)
}

fn reduce(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    reduce_with(vm, "__reduce__", 0, args, kw)
}

fn reduce_ex(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    reduce_with(vm, "__reduce_ex__", 1, args, kw)
}

/// Os métodos de classe de `object` (`__init_subclass__`, `__subclasshook__`) lidos por uma instância:
/// o receptor não entra na chamada.
macro_rules! object_classmethods {
    ($($f:ident = $name:literal;)*) => {$(
        fn $f(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let method = crate::typeattrs::object_attr($name).expect("object define o método");
            vm.call(&method, args[1..].to_vec(), kw)
        }
    )*};
}

object_classmethods! {
    init_subclass = "__init_subclass__";
    subclasshook = "__subclasshook__";
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("__contains__", contains),
    ("__len__", len),
    ("__getitem__", getitem),
    ("__setitem__", setitem),
    ("__delitem__", delitem),
    ("__iter__", iter),
    ("__next__", next),
    ("__reversed__", reversed),
    ("__hash__", hash),
    ("__repr__", repr),
    ("__str__", str_),
    ("__bool__", bool_),
    ("__eq__", eq),
    ("__ne__", ne),
    ("__lt__", lt),
    ("__le__", le),
    ("__gt__", gt),
    ("__ge__", ge),
    ("__add__", add),
    ("__radd__", radd),
    ("__iadd__", iadd),
    ("__sub__", sub),
    ("__rsub__", rsub),
    ("__isub__", isub),
    ("__mul__", mul),
    ("__rmul__", rmul),
    ("__imul__", imul),
    ("__truediv__", truediv),
    ("__rtruediv__", rtruediv),
    ("__floordiv__", floordiv),
    ("__rfloordiv__", rfloordiv),
    ("__mod__", mod_),
    ("__rmod__", rmod),
    ("__and__", and_),
    ("__rand__", rand),
    ("__iand__", iand),
    ("__or__", or_),
    ("__ror__", ror),
    ("__ior__", ior),
    ("__xor__", xor),
    ("__rxor__", rxor),
    ("__ixor__", ixor),
    ("__lshift__", lshift),
    ("__rlshift__", rlshift),
    ("__rshift__", rshift),
    ("__rrshift__", rrshift),
    ("__pow__", pow),
    ("__rpow__", rpow),
    ("__divmod__", divmod),
    ("__rdivmod__", rdivmod),
    ("__neg__", neg),
    ("__pos__", pos),
    ("__invert__", invert),
    ("__abs__", abs),
    ("__index__", index),
    ("__int__", int_),
    ("__float__", float_),
    ("__format__", format),
    ("__getnewargs__", getnewargs),
    ("__getformat__", getformat),
    ("__bytes__", bytes_),
    ("__buffer__", buffer),
    ("__release_buffer__", release_buffer),
    ("__alloc__", alloc),
    ("__class_getitem__", class_getitem),
    ("__sizeof__", sizeof),
    ("__del__", finalize),
    ("__setstate__", setstate),
    ("__init__", init),
    ("__setattr__", setattr),
    ("__delattr__", delattr),
    ("__getattribute__", getattribute),
    ("__dir__", dir),
    ("__getstate__", getstate),
    ("__reduce__", reduce),
    ("__reduce_ex__", reduce_ex),
    ("__init_subclass__", init_subclass),
    ("__subclasshook__", subclasshook),
];
