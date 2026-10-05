//! Modelo de objetos (fatia 9 de `docs/python3-port.md`): `Value`, `repr()`/`str()`, igualdade e
//! hash dos tipos embutidos, com a saída do CPython 3.13.
//!
//! Escalares (`None`, `bool`, `int`, `float`) ficam inline; o resto é compartilhado por `Rc`, e os
//! mutáveis (`list`, `dict`, `set`) ficam atrás de `RefCell`. Clonar um `Value` é copiar a
//! referência, como atribuir em Python.
//!
//! Identidade: só os tipos atrás de `Rc` têm identidade de objeto (`Rc::ptr_eq`). `int` segue o
//! cache de inteiros pequenos do CPython (-5 a 256 são sempre o mesmo objeto) e `float` nunca é
//! idêntico a outro; a diferença só aparece com NaN dentro de contêineres (`[x] == [x]` com `x` NaN
//! é verdadeiro no CPython e falso aqui).

mod dict;
mod float;
mod int;
mod list;
mod set;
// O nome `str` sombrearia o tipo primitivo neste módulo.
#[path = "str.rs"]
mod pystr;

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

pub use self::dict::Dict;
pub use self::float::{float_hash, float_repr, format_float_short};
pub use self::int::{int_add, int_hash, int_mul, int_neg, int_repr, int_sub};
pub use self::set::Set;
pub use self::pystr::{bytes_hash, bytes_repr, is_printable, str_repr, PyStr};

/// Valor Python.
#[derive(Clone)]
pub enum Value {
    None,
    Bool(bool),
    /// Até a fatia 19, só a faixa de `i64` (ver `int`).
    Int(i64),
    Float(f64),
    Str(Rc<PyStr>),
    Bytes(Rc<[u8]>),
    List(Rc<RefCell<Vec<Value>>>),
    Tuple(Rc<[Value]>),
    Dict(Rc<RefCell<Dict>>),
    Set(Rc<RefCell<Set>>),
}

/// Erros do modelo de objetos. O interpretador (fatia 11) converte em exceções Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjError {
    /// `TypeError` com a mensagem do CPython.
    TypeError(String),
    /// Erro interno: resultado inteiro fora de `i64`. Some na fatia 19 (ver `int`).
    IntOverflow,
}

impl Value {
    pub fn str(text: impl Into<String>) -> Value {
        Value::Str(Rc::new(PyStr::new(text)))
    }

    pub fn bytes(data: impl Into<Vec<u8>>) -> Value {
        Value::Bytes(Rc::from(data.into()))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(RefCell::new(items)))
    }

    pub fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(Rc::from(items))
    }

    pub fn dict(d: Dict) -> Value {
        Value::Dict(Rc::new(RefCell::new(d)))
    }

    pub fn set(s: Set) -> Value {
        Value::Set(Rc::new(RefCell::new(s)))
    }

    /// `type(v).__name__`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Dict(_) => "dict",
            Value::Set(_) => "set",
        }
    }

    /// `bool(v)`.
    pub fn is_true(&self) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::List(l) => !l.borrow().is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::Dict(d) => !d.borrow().is_empty(),
            Value::Set(s) => !s.borrow().is_empty(),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&repr(self))
    }
}

/// Endereço de um objeto compartilhado, para identidade e para a pilha do `repr`.
fn addr<T: ?Sized>(rc: &Rc<T>) -> usize {
    Rc::as_ptr(rc) as *const () as usize
}

/// Pilha de contêineres em impressão (`Py_ReprEnter`/`Py_ReprLeave`), que corta a recursão de um
/// contêiner que contém a si mesmo com `[...]`, `{...}`, `(...)` ou `set(...)`.
#[derive(Default)]
pub(crate) struct ReprStack(Vec<usize>);

impl ReprStack {
    /// Falso se o objeto já está sendo impresso mais acima.
    fn enter(&mut self, id: usize) -> bool {
        if self.0.contains(&id) {
            return false;
        }
        self.0.push(id);
        true
    }

    fn leave(&mut self, id: usize) {
        if let Some(pos) = self.0.iter().rposition(|&x| x == id) {
            self.0.remove(pos);
        }
    }
}

/// `repr(v)`.
pub fn repr(v: &Value) -> String {
    let mut out = String::new();
    repr_into(v, &mut out, &mut ReprStack::default());
    out
}

/// `str(v)`: o próprio texto para `str`; para os demais tipos embutidos é igual ao `repr` (no
/// Python 3, inclusive `float` e `bytes`).
pub fn to_str(v: &Value) -> String {
    match v {
        Value::Str(s) => s.as_str().to_string(),
        _ => repr(v),
    }
}

pub(crate) fn repr_into(v: &Value, out: &mut String, stack: &mut ReprStack) {
    match v {
        Value::None => out.push_str("None"),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Int(i) => out.push_str(&int_repr(*i)),
        Value::Float(x) => out.push_str(&float_repr(*x)),
        Value::Str(s) => out.push_str(&str_repr(s.as_str())),
        Value::Bytes(b) => out.push_str(&bytes_repr(b)),
        Value::List(l) => list::list_repr(&l.borrow(), addr(l), out, stack),
        Value::Tuple(t) => list::tuple_repr(t, addr(t), out, stack),
        Value::Dict(d) => dict::dict_repr(&d.borrow(), addr(d), out, stack),
        Value::Set(s) => set::set_repr(&s.borrow(), addr(s), out, stack),
    }
}

/// `a is b` (ver a nota de identidade no topo do módulo).
pub fn is(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y && (-5..=256).contains(x),
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y),
        (Value::Bytes(x), Value::Bytes(y)) => Rc::ptr_eq(x, y),
        (Value::List(x), Value::List(y)) => Rc::ptr_eq(x, y),
        (Value::Tuple(x), Value::Tuple(y)) => Rc::ptr_eq(x, y),
        (Value::Dict(x), Value::Dict(y)) => Rc::ptr_eq(x, y),
        (Value::Set(x), Value::Set(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Visão numérica de `bool`, `int` e `float` para comparação entre tipos.
enum Num {
    Int(i64),
    Float(f64),
}

fn as_num(v: &Value) -> Option<Num> {
    match v {
        Value::Bool(b) => Some(Num::Int(i64::from(*b))),
        Value::Int(i) => Some(Num::Int(*i)),
        Value::Float(x) => Some(Num::Float(*x)),
        _ => None,
    }
}

/// `int == float` exato, como o `float_richcompare` (sem arredondar o inteiro para `double`).
fn int_float_eq(i: i64, x: f64) -> bool {
    // 2**63 é exato em `double`; fora de [-2**63, 2**63) nenhum `i64` é igual.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    x.is_finite() && x.fract() == 0.0 && (-LIMIT..LIMIT).contains(&x) && x as i64 == i
}

/// `a == b` dos tipos embutidos.
pub fn py_eq(a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (as_num(a), as_num(b)) {
        return match (x, y) {
            (Num::Int(x), Num::Int(y)) => x == y,
            (Num::Float(x), Num::Float(y)) => x == y,
            (Num::Int(i), Num::Float(x)) | (Num::Float(x), Num::Int(i)) => int_float_eq(i, x),
        };
    }
    match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y) || x.as_str() == y.as_str(),
        (Value::Bytes(x), Value::Bytes(y)) => x[..] == y[..],
        (Value::List(x), Value::List(y)) => Rc::ptr_eq(x, y) || list::seq_eq(&x.borrow(), &y.borrow()),
        (Value::Tuple(x), Value::Tuple(y)) => Rc::ptr_eq(x, y) || list::seq_eq(x, y),
        (Value::Dict(x), Value::Dict(y)) => Rc::ptr_eq(x, y) || dict::dict_eq(&x.borrow(), &y.borrow()),
        (Value::Set(x), Value::Set(y)) => Rc::ptr_eq(x, y) || set::set_eq(&x.borrow(), &y.borrow()),
        _ => false,
    }
}

/// Hash fixo de `None` desde o 3.12 (`none_hash`), independente do endereço.
const NONE_HASH: i64 = 0xFCA8_6420;

/// `hash(v)`; contêineres mutáveis dão `TypeError: unhashable type: 'list'`.
pub fn hash(v: &Value) -> Result<i64, ObjError> {
    match v {
        Value::None => Ok(NONE_HASH),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Int(i) => Ok(int_hash(*i)),
        Value::Float(x) => Ok(float_hash(*x)),
        Value::Str(s) => Ok(s.hash()),
        Value::Bytes(b) => Ok(bytes_hash(b)),
        Value::Tuple(t) => tuple_hash(t),
        Value::List(_) | Value::Dict(_) | Value::Set(_) => {
            Err(ObjError::TypeError(format!("unhashable type: '{}'", v.type_name())))
        }
    }
}

/// `tuplehash` do 3.13 (variante do xxHash de 64 bits).
fn tuple_hash(items: &[Value]) -> Result<i64, ObjError> {
    const PRIME_1: u64 = 11_400_714_785_074_694_791;
    const PRIME_2: u64 = 14_029_467_366_897_019_727;
    const PRIME_5: u64 = 2_870_177_450_012_600_261;
    let mut acc = PRIME_5;
    for item in items {
        let lane = hash(item)? as u64;
        acc = acc.wrapping_add(lane.wrapping_mul(PRIME_2));
        acc = acc.rotate_left(31);
        acc = acc.wrapping_mul(PRIME_1);
    }
    acc = acc.wrapping_add((items.len() as u64) ^ (PRIME_5 ^ 3_527_539));
    if acc == u64::MAX {
        return Ok(1_546_275_796);
    }
    Ok(acc as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value {
        Value::str(text)
    }

    fn dict_of(pairs: Vec<(Value, Value)>) -> Value {
        let mut d = Dict::new();
        for (k, v) in pairs {
            d.set(k, v).unwrap();
        }
        Value::dict(d)
    }

    fn set_of(items: Vec<Value>) -> Value {
        let mut st = Set::new();
        for item in items {
            st.add(item).unwrap();
        }
        Value::set(st)
    }

    #[test]
    fn scalar_reprs() {
        assert_eq!(repr(&Value::None), "None");
        assert_eq!(repr(&Value::Bool(true)), "True");
        assert_eq!(repr(&Value::Int(-42)), "-42");
        assert_eq!(repr(&Value::Float(0.1)), "0.1");
        assert_eq!(repr(&Value::Float(1.0)), "1.0");
        assert_eq!(repr(&Value::Float(1e16)), "1e+16");
        assert_eq!(repr(&Value::Float(1e-5)), "1e-05");
        assert_eq!(repr(&Value::Float(-0.0)), "-0.0");
        assert_eq!(repr(&Value::Float(f64::NAN)), "nan");
        assert_eq!(repr(&s("it's")), "\"it's\"");
        assert_eq!(repr(&s("a'\"b")), "'a\\'\"b'");
        assert_eq!(repr(&s("\u{feff}é\t")), "'\\ufeffé\\t'");
        assert_eq!(repr(&Value::bytes(b"a'\x00\xff".to_vec())), "b\"a'\\x00\\xff\"");
    }

    #[test]
    fn str_versus_repr() {
        assert_eq!(to_str(&s("a\nb")), "a\nb");
        assert_eq!(to_str(&Value::Float(2.5)), "2.5");
        assert_eq!(to_str(&Value::list(vec![s("x")])), "['x']");
    }

    #[test]
    fn container_reprs() {
        assert_eq!(repr(&Value::list(vec![])), "[]");
        assert_eq!(repr(&Value::tuple(vec![])), "()");
        assert_eq!(repr(&Value::tuple(vec![Value::Int(1)])), "(1,)");
        assert_eq!(repr(&Value::tuple(vec![Value::Int(1), s("a")])), "(1, 'a')");
        assert_eq!(repr(&dict_of(vec![])), "{}");
        assert_eq!(repr(&Value::set(Set::new())), "set()");
        let nested = Value::list(vec![
            Value::Int(1),
            Value::tuple(vec![Value::None, Value::list(vec![])]),
            dict_of(vec![(s("k"), Value::list(vec![Value::Float(0.5)]))]),
        ]);
        assert_eq!(repr(&nested), "[1, (None, []), {'k': [0.5]}]");
    }

    #[test]
    fn recursive_reprs() {
        let l = Value::list(vec![Value::Int(1)]);
        if let Value::List(inner) = &l {
            inner.borrow_mut().push(l.clone());
        }
        assert_eq!(repr(&l), "[1, [...]]");

        let d = dict_of(vec![]);
        if let Value::Dict(inner) = &d {
            inner.borrow_mut().set(s("self"), d.clone()).unwrap();
        }
        assert_eq!(repr(&d), "{'self': {...}}");

        // Tupla que contém uma lista que contém a tupla.
        let l = Value::list(vec![]);
        let t = Value::tuple(vec![l.clone()]);
        if let Value::List(inner) = &l {
            inner.borrow_mut().push(t.clone());
        }
        assert_eq!(repr(&t), "([(...)],)");

        // O mesmo objeto repetido sem recursão não é cortado.
        let shared = Value::list(vec![Value::Int(0)]);
        assert_eq!(repr(&Value::list(vec![shared.clone(), shared])), "[[0], [0]]");
    }

    #[test]
    fn dict_keeps_insertion_order_and_first_key() {
        let mut d = Dict::new();
        d.set(s("b"), Value::Int(1)).unwrap();
        d.set(s("a"), Value::Int(2)).unwrap();
        d.set(Value::Int(1), s("x")).unwrap();
        d.set(Value::Bool(true), s("y")).unwrap();
        d.set(Value::Float(1.0), s("z")).unwrap();
        assert_eq!(repr(&Value::dict(d.clone())), "{'b': 1, 'a': 2, 1: 'z'}");
        assert_eq!(d.remove(&s("b")).unwrap().map(|v| repr(&v)), Some("1".to_string()));
        d.set(s("b"), Value::Int(3)).unwrap();
        assert_eq!(repr(&Value::dict(d.clone())), "{'a': 2, 1: 'z', 'b': 3}");
        assert_eq!(
            d.set(Value::list(vec![]), Value::None),
            Err(ObjError::TypeError("unhashable type: 'list'".to_string()))
        );
    }

    #[test]
    fn set_order_follows_cpython_table() {
        // `s = set()` seguido de `s.add` com 100, 1 e 8: posições 100 & 7 = 4, 1 e 8 & 7 = 0.
        let st = set_of(vec![Value::Int(100), Value::Int(1), Value::Int(8)]);
        assert_eq!(repr(&st), "{8, 1, 100}");
        // `s.add` com 50, 40, 30, 20, 10 e 0: o 10 colide com o 50 e vai para a posição 3; o quinto
        // elemento enche a tabela de 8 (fill * 5 >= mask * 3) e ela passa a 32 posições.
        let st = set_of((0..6).rev().map(|i| Value::Int(i * 10)).collect());
        assert_eq!(repr(&st), "{0, 40, 10, 50, 20, 30}");
        let st = set_of(vec![Value::Int(1), Value::Bool(true), Value::Float(1.0)]);
        assert_eq!(repr(&st), "{1}");
    }

    #[test]
    fn equality() {
        assert!(py_eq(&Value::Int(1), &Value::Float(1.0)));
        assert!(py_eq(&Value::Bool(true), &Value::Int(1)));
        assert!(!py_eq(&Value::Int(1), &s("1")));
        assert!(!py_eq(&Value::Float(f64::NAN), &Value::Float(f64::NAN)));
        assert!(!py_eq(&Value::Int(i64::MAX), &Value::Float(9_223_372_036_854_775_808.0)));
        assert!(py_eq(
            &Value::list(vec![Value::Int(1), s("a")]),
            &Value::list(vec![Value::Float(1.0), s("a")])
        ));
        assert!(!py_eq(&Value::list(vec![]), &Value::tuple(vec![])));
        let a = dict_of(vec![(s("x"), Value::Int(1)), (s("y"), Value::Int(2))]);
        let b = dict_of(vec![(s("y"), Value::Int(2)), (s("x"), Value::Int(1))]);
        assert!(py_eq(&a, &b));
    }

    #[test]
    fn hashes_match_cpython() {
        assert_eq!(hash(&Value::Int(-1)), Ok(-2));
        assert_eq!(hash(&Value::Int(-2)), Ok(-2));
        assert_eq!(hash(&Value::Int((1 << 61) - 1)), Ok(0));
        assert_eq!(hash(&Value::Int(i64::MAX)), Ok(3)); // 2**63 - 1 == 4 * (2**61 - 1) + 3
        assert_eq!(hash(&Value::Float(1.5)), Ok(1_152_921_504_606_846_977));
        assert_eq!(hash(&Value::Float(2.0)), Ok(2));
        assert_eq!(hash(&Value::Float(-1.0)), Ok(-2));
        assert_eq!(hash(&Value::Float(f64::INFINITY)), Ok(314_159));
        assert_eq!(hash(&Value::None), Ok(0xFCA8_6420));
        assert_eq!(hash(&s("")), Ok(0));
        assert_eq!(hash(&s("a")), hash(&Value::bytes(b"a".to_vec())));
        assert_eq!(hash(&Value::tuple(vec![])), Ok(5_740_354_900_026_072_187));
        assert_eq!(
            hash(&Value::dict(Dict::new())),
            Err(ObjError::TypeError("unhashable type: 'dict'".to_string()))
        );
    }

    #[test]
    fn str_indexes_by_code_point() {
        let st = PyStr::new("aé😀b");
        assert_eq!(st.len(), 4);
        assert_eq!(st.char_at(1), Some('é'));
        assert_eq!(st.char_at(2), Some('😀'));
        assert_eq!(st.char_at(4), None);
        assert_eq!(st.slice(1, 3), "é😀");
        assert_eq!(st.slice(3, 99), "b");
        assert_eq!(PyStr::new("abc").slice(2, 1), "");
    }

    #[test]
    fn int_overflow_is_internal_error() {
        assert_eq!(int_add(i64::MAX, 1), Err(ObjError::IntOverflow));
        assert_eq!(int_neg(i64::MIN), Err(ObjError::IntOverflow));
        assert_eq!(int_mul(-3, 7), Ok(-21));
        assert_eq!(int_sub(0, 5), Ok(-5));
    }
}
