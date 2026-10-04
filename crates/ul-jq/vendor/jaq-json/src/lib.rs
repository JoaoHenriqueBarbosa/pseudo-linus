//! JSON values for jaq (jaq-json 2.0.3, MIT, Michael Färber).
//!
//! Porte pseudo-linus: reescrita completa deste arquivo sobre o mesmo tipo `Val` do jaq, para a
//! semântica do jq 1.7.1 (`src/jv.c`, `src/jv_aux.c` e os operadores de `src/builtin.c`):
//!
//! - números do jq (double, com literal decimal preservado; ver [`Num`]);
//! - indexação, fatias, `getpath`/`setpath`/`delpaths` como `jv_get`/`jv_set`/`jv_*path*`;
//! - operadores com as mensagens do jq (`number (1) and number (0) cannot be divided because the
//!   divisor is zero`, truncadas em 11 bytes como o `jv_dump_string_trunc`);
//! - ordem (`jv_cmp`), igualdade (`jv_equal`) e identidade (`jv_identical`) do jq;
//! - chave de objeto só string, e ordem de inserção das chaves preservada inclusive ao apagar;
//! - impressão e leitura nos módulos [`jqfmt`] e [`jqparse`].
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

mod jqfuns;
pub mod jqfmt;
mod jqnum;
pub mod jqparse;
mod jqpath;

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::{boxed::Box, string::String, vec::Vec};
use bytes::Bytes;
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use jaq_core::box_iter::box_once;
use jaq_core::val;

pub use jqfuns::{funs, object};
pub use jqnum::{dtoa_fmt, Decimal, Literal, Num};

pub use alloc::rc::Rc;

/// Valor JSON do jq.
#[derive(Clone, Debug, Default)]
pub enum Val {
    #[default]
    /// Null
    Null,
    /// Boolean
    Bool(bool),
    /// Number
    Num(Num),
    /// Mantido só por compatibilidade de tipo com o código do jaq; o jq não tem strings de bytes e
    /// nada aqui constrói esta variante (ela é tratada como string de texto onde aparece).
    BStr(Box<Bytes>),
    /// Texto UTF-8 (sempre válido: entradas inválidas viram U+FFFD na leitura, como no jq).
    TStr(Box<Bytes>),
    /// Array
    Arr(Rc<Vec<Val>>),
    /// Object
    Obj(Rc<Map<Val, Val>>),
}

#[cfg(target_arch = "x86_64")]
const _: () = {
    assert!(core::mem::size_of::<Val>() == 16);
};

/// Order-preserving map
pub type Map<K = Val, V = K> = indexmap::IndexMap<K, V, foldhash::fast::RandomState>;

/// Error that can occur during filter execution.
pub type Error = jaq_core::Error<Val>;
/// A value or an eRror.
pub type ValR = jaq_core::ValR<Val>;
/// A value or an eXception.
pub type ValX<'a> = jaq_core::ValX<'a, Val>;

/// `INT_MAX` do C, limite de vários tamanhos no jq.
pub(crate) const INT_MAX: i64 = i32::MAX as i64;

fn rc_unwrap_or_clone<T: Clone>(a: Rc<T>) -> T {
    Rc::try_unwrap(a).unwrap_or_else(|a| (*a).clone())
}

/// Erro com mensagem de texto.
pub fn err(msg: impl Into<String>) -> Error {
    Error::new(Val::from(msg.into()))
}

/// `type_error` do `builtin.c`: "<tipo> (<valor truncado em 11 bytes>) <msg>".
pub fn type_error(v: &Val, msg: &str) -> Error {
    err(format!("{} ({}) {msg}", jqfmt::kind_name(v), jqfmt::dump_trunc(v, 15)))
}

/// `type_error2` do `builtin.c`.
pub fn type_error2(a: &Val, b: &Val, msg: &str) -> Error {
    err(format!(
        "{} ({}) and {} ({}) {msg}",
        jqfmt::kind_name(a),
        jqfmt::dump_trunc(a, 15),
        jqfmt::kind_name(b),
        jqfmt::dump_trunc(b, 15)
    ))
}

impl jaq_core::ValT for Val {
    fn from_num(n: &str) -> ValR {
        // Literal do programa: o lexer do jq passa o texto pelo leitor de JSON.
        Num::from_literal(n).map(Val::Num).ok_or_else(|| err(format!("Invalid numeric literal {n}")))
    }

    fn from_map<I: IntoIterator<Item = (Self, Self)>>(iter: I) -> ValR {
        let mut m = Map::default();
        for (k, v) in iter {
            if !k.is_str() {
                return Err(err(format!(
                    "Cannot use {} ({}) as object key",
                    jqfmt::kind_name(&k),
                    jqfmt::dump_trunc(&k, 15)
                )));
            }
            m.insert(k, v);
        }
        Ok(Self::obj(m))
    }

    fn key_values(self) -> Box<dyn Iterator<Item = Result<(Val, Val), Error>>> {
        let arr_idx = |(i, x)| Ok((Self::from(i), x));
        match self {
            Self::Arr(a) => Box::new(rc_unwrap_or_clone(a).into_iter().enumerate().map(arr_idx)),
            Self::Obj(o) => Box::new(rc_unwrap_or_clone(o).into_iter().map(Ok)),
            _ => box_once(Err(iterate_error(&self))),
        }
    }

    fn values(self) -> Box<dyn Iterator<Item = ValR>> {
        match self {
            Self::Arr(a) => Box::new(rc_unwrap_or_clone(a).into_iter().map(Ok)),
            Self::Obj(o) => Box::new(rc_unwrap_or_clone(o).into_iter().map(|(_k, v)| Ok(v))),
            _ => box_once(Err(iterate_error(&self))),
        }
    }

    fn index(self, index: &Self) -> ValR {
        jqpath::get(&self, index)
    }

    fn range(self, range: val::Range<&Self>) -> ValR {
        jqpath::get(&self, &slice_key(range.start.cloned(), range.end.cloned()))
    }

    fn as_bool(&self) -> bool {
        !matches!(self, Self::Null | Self::Bool(false))
    }

    fn into_string(self) -> Self {
        match self {
            Self::BStr(b) | Self::TStr(b) => Self::TStr(b),
            _ => Self::from(jqfmt::dump_compact(&self)),
        }
    }

    fn null() -> Self {
        Self::Null
    }

    fn identical(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => a.identical(b),
            (Self::TStr(a) | Self::BStr(a), Self::TStr(b) | Self::BStr(b)) => {
                a.as_ptr() == b.as_ptr() && a.len() == b.len()
            }
            (Self::Arr(a), Self::Arr(b)) => Rc::ptr_eq(a, b),
            (Self::Obj(a), Self::Obj(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn dump_trunc(&self, bufsize: usize) -> String {
        jqfmt::dump_trunc(self, bufsize)
    }

    fn kind_name(&self) -> &'static str {
        jqfmt::kind_name(self)
    }

    fn getpath(&self, path: &[Self]) -> ValR {
        jqpath::getpath(self.clone(), path)
    }

    fn setpath(self, path: &[Self], value: Self) -> ValR {
        jqpath::setpath(self, path, value)
    }

    fn delpaths(self, paths: Vec<Self>) -> ValR {
        jqpath::delpaths(self, Val::Arr(Rc::new(paths)))
    }
}

/// "Cannot iterate over <tipo> (<valor>)" (o `EACH` do jq).
pub(crate) fn iterate_error(v: &Val) -> Error {
    err(format!("Cannot iterate over {} ({})", jqfmt::kind_name(v), jqfmt::dump_trunc(v, 15)))
}

/// A chave de fatia do jq: `{"start": s, "end": e}`, sempre com as duas chaves.
pub fn slice_key(start: Option<Val>, end: Option<Val>) -> Val {
    let mut m = Map::default();
    m.insert(Val::from(String::from("start")), start.unwrap_or(Val::Null));
    m.insert(Val::from(String::from("end")), end.unwrap_or(Val::Null));
    Val::obj(m)
}

impl jaq_std::ValT for Val {
    fn into_seq<S: FromIterator<Self>>(self) -> Result<S, Self> {
        match self {
            Self::Arr(a) => match Rc::try_unwrap(a) {
                Ok(a) => Ok(a.into_iter().collect()),
                Err(a) => Ok(a.iter().cloned().collect()),
            },
            _ => Err(self),
        }
    }

    fn is_int(&self) -> bool {
        self.as_num().is_some_and(Num::is_integer)
    }

    fn as_isize(&self) -> Option<isize> {
        self.as_num().and_then(Num::as_isize)
    }

    fn as_f64(&self) -> Option<f64> {
        self.as_num().map(Num::as_f64)
    }

    fn is_utf8_str(&self) -> bool {
        self.is_str()
    }

    fn as_bytes(&self) -> Option<&[u8]> {
        self.str_bytes()
    }

    fn as_sub_str(&self, sub: &[u8]) -> Self {
        match self {
            Self::BStr(b) | Self::TStr(b) => Self::TStr(Box::new(b.slice_ref(sub))),
            _ => panic!(),
        }
    }

    fn from_utf8_bytes(b: impl AsRef<[u8]> + Send + 'static) -> Self {
        Self::from(jqparse::utf8_lossy(b.as_ref()))
    }
}

impl Val {
    /// Construct an object value.
    pub fn obj(m: Map) -> Self {
        Self::Obj(m.into())
    }

    /// String a partir de bytes já em UTF-8 válido (por exemplo, fatias de outra string).
    pub fn utf8_str(s: impl Into<Bytes>) -> Self {
        Self::TStr(Box::new(s.into()))
    }

    /// String a partir de texto.
    pub fn str(s: &str) -> Self {
        Self::TStr(Box::new(Bytes::copy_from_slice(s.as_bytes())))
    }

    /// True for strings.
    pub fn is_str(&self) -> bool {
        matches!(self, Self::TStr(_) | Self::BStr(_))
    }

    /// Bytes da string, se for string.
    pub fn str_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::BStr(b) | Self::TStr(b) => Some(b),
            _ => None,
        }
    }

    /// Texto da string, se for string (sempre UTF-8 válido).
    pub fn as_str(&self) -> Option<&str> {
        self.str_bytes().and_then(|b| core::str::from_utf8(b).ok())
    }

    /// Número, se for número.
    pub fn as_num(&self) -> Option<&Num> {
        match self {
            Self::Num(n) => Some(n),
            _ => None,
        }
    }

    /// `jv_number_value`, se for número.
    pub fn as_f64(&self) -> Option<f64> {
        self.as_num().map(Num::as_f64)
    }

    /// Array, se for array.
    pub fn as_arr(&self) -> Option<&Rc<Vec<Val>>> {
        match self {
            Self::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// Objeto, se for objeto.
    pub fn as_obj(&self) -> Option<&Rc<Map>> {
        match self {
            Self::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// `jv_kind_name`.
    pub fn kind_name(&self) -> &'static str {
        jqfmt::kind_name(self)
    }

    /// Número do jq a partir de um double.
    pub fn num(x: f64) -> Self {
        Self::Num(Num::from_f64(x))
    }

    /// Serialização compacta (o `tojson`).
    pub fn to_json(&self) -> String {
        jqfmt::dump_compact(self)
    }
}

impl From<bool> for Val {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<isize> for Val {
    fn from(i: isize) -> Self {
        Self::Num(Num::from_f64(i as f64))
    }
}

impl From<usize> for Val {
    fn from(i: usize) -> Self {
        Self::Num(Num::from_integral(i))
    }
}

impl From<f64> for Val {
    fn from(f: f64) -> Self {
        Self::Num(Num::from_f64(f))
    }
}

impl From<String> for Val {
    fn from(s: String) -> Self {
        Self::TStr(Box::new(Bytes::from(s)))
    }
}

impl From<val::Range<Val>> for Val {
    fn from(r: val::Range<Val>) -> Self {
        slice_key(r.start, r.end)
    }
}

impl FromIterator<Self> for Val {
    fn from_iter<T: IntoIterator<Item = Self>>(iter: T) -> Self {
        Self::Arr(Rc::new(iter.into_iter().collect()))
    }
}

fn concat_bytes(l: &[u8], r: &[u8]) -> Val {
    let mut v = Vec::with_capacity(l.len() + r.len());
    v.extend_from_slice(l);
    v.extend_from_slice(r);
    Val::TStr(Box::new(Bytes::from(v)))
}

/// `binop_plus`.
impl core::ops::Add for Val {
    type Output = ValR;
    fn add(self, rhs: Self) -> Self::Output {
        use Val::*;
        match (self, rhs) {
            (Null, x) | (x, Null) => Ok(x),
            (Num(x), Num(y)) => Ok(Val::num(x.as_f64() + y.as_f64())),
            (l @ (BStr(_) | TStr(_)), r @ (BStr(_) | TStr(_))) => {
                Ok(concat_bytes(l.str_bytes().unwrap_or_default(), r.str_bytes().unwrap_or_default()))
            }
            (Arr(mut l), Arr(r)) => {
                if l.is_empty() {
                    return Ok(Arr(r));
                }
                Rc::make_mut(&mut l).extend(r.iter().cloned());
                Ok(Arr(l))
            }
            (Obj(mut l), Obj(r)) => {
                Rc::make_mut(&mut l).extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
                Ok(Obj(l))
            }
            (l, r) => Err(type_error2(&l, &r, "cannot be added")),
        }
    }
}

/// `binop_minus`.
impl core::ops::Sub for Val {
    type Output = ValR;
    fn sub(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Num(x), Self::Num(y)) => Ok(Val::num(x.as_f64() - y.as_f64())),
            (Self::Arr(mut l), Self::Arr(r)) => {
                // `jv_equal` e a igualdade do `Ord` concordam (NaN nunca é igual), então o conjunto
                // ordenado dá o mesmo resultado que o laço duplo do jq.
                let r = r.iter().collect::<BTreeSet<_>>();
                Rc::make_mut(&mut l).retain(|x| !r.contains(x));
                Ok(Self::Arr(l))
            }
            (l, r) => Err(type_error2(&l, &r, "cannot be subtracted")),
        }
    }
}

/// `jv_object_merge_recursive`.
fn obj_merge(l: &mut Rc<Map>, r: Rc<Map>) {
    let l = Rc::make_mut(l);
    let r = rc_unwrap_or_clone(r).into_iter();
    r.for_each(|(k, v)| match (l.get_mut(&k), v) {
        (Some(Val::Obj(l)), Val::Obj(r)) => obj_merge(l, r),
        (Some(l), r) => *l = r,
        (None, r) => {
            l.insert(k, r);
        }
    });
}

/// `binop_multiply` (com o limite de repetição do jq 1.7.1 do Debian).
impl core::ops::Mul for Val {
    type Output = ValR;
    fn mul(self, rhs: Self) -> Self::Output {
        use Val::*;
        match (self, rhs) {
            (Num(x), Num(y)) => Ok(Val::num(x.as_f64() * y.as_f64())),
            (s @ (BStr(_) | TStr(_)), Num(n)) | (Num(n), s @ (BStr(_) | TStr(_))) => {
                let d = n.as_f64();
                if d < 0.0 || d.is_nan() {
                    return Ok(Null);
                }
                let bytes = s.str_bytes().unwrap_or_default();
                if d >= INT_MAX as f64 || (bytes.len() as f64) * d.trunc() >= INT_MAX as f64 {
                    return Err(err("Repeat string result too long"));
                }
                let n = d as usize;
                let total = bytes.len() * n;
                if total as i64 > INT_MAX - 64 {
                    return Err(err("String too long"));
                }
                let mut out: Vec<u8> = Vec::new();
                if out.try_reserve(total).is_err() {
                    return Err(err("String too long"));
                }
                for _ in 0..n {
                    out.extend_from_slice(bytes);
                }
                Ok(TStr(Box::new(Bytes::from(out))))
            }
            (Obj(mut l), Obj(r)) => {
                obj_merge(&mut l, r);
                Ok(Obj(l))
            }
            (l, r) => Err(type_error2(&l, &r, "cannot be multiplied")),
        }
    }
}

/// `jv_string_split`: separador vazio quebra em caracteres; string vazia dá `[]`; separador no fim
/// deixa uma string vazia no fim.
pub(crate) fn split_str(s: &[u8], sep: &[u8]) -> Val {
    let mut out = Vec::new();
    if sep.is_empty() {
        let mut i = 0;
        while i < s.len() {
            let (cp, len) = jqparse::utf8_next(&s[i..]);
            let c = cp.and_then(char::from_u32).unwrap_or('\u{FFFD}');
            out.push(Val::from(String::from(c)));
            i += len;
        }
    } else {
        let mut p = 0;
        while p < s.len() {
            let found = s[p..].find(sep).map(|i| p + i);
            let end = found.unwrap_or(s.len());
            out.push(Val::utf8_str(Bytes::copy_from_slice(&s[p..end])));
            if found.is_some() && end + sep.len() == s.len() {
                out.push(Val::str(""));
            }
            p = end + sep.len();
        }
    }
    Val::Arr(Rc::new(out))
}

use bstr::ByteSlice as _;

/// `binop_divide`.
impl core::ops::Div for Val {
    type Output = ValR;
    fn div(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Num(x), Self::Num(y)) => {
                if y.as_f64() == 0.0 {
                    return Err(type_error2(&Self::Num(x), &Self::Num(y), "cannot be divided because the divisor is zero"));
                }
                Ok(Val::num(x.as_f64() / y.as_f64()))
            }
            (l, r) if l.is_str() && r.is_str() => {
                Ok(split_str(l.str_bytes().unwrap_or_default(), r.str_bytes().unwrap_or_default()))
            }
            (l, r) => Err(type_error2(&l, &r, "cannot be divided")),
        }
    }
}

/// `dtoi` do `builtin.c`: double para `intmax_t` saturando.
fn dtoi(n: f64) -> i64 {
    if n < i64::MIN as f64 {
        i64::MIN
    } else if -n < i64::MIN as f64 {
        i64::MAX
    } else {
        n as i64
    }
}

/// `binop_mod`.
impl core::ops::Rem for Val {
    type Output = ValR;
    fn rem(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Num(x), Self::Num(y)) => {
                let (na, nb) = (x.as_f64(), y.as_f64());
                if na.is_nan() || nb.is_nan() {
                    return Ok(Val::num(f64::NAN));
                }
                let bi = dtoi(nb);
                if bi == 0 {
                    return Err(type_error2(
                        &Self::Num(x),
                        &Self::Num(y),
                        "cannot be divided (remainder) because the divisor is zero",
                    ));
                }
                let r = if bi == -1 { 0 } else { dtoi(na) % bi };
                Ok(Val::num(r as f64))
            }
            (l, r) => Err(type_error2(&l, &r, "cannot be divided (remainder)")),
        }
    }
}

/// `f_negate`.
impl core::ops::Neg for Val {
    type Output = ValR;
    fn neg(self) -> Self::Output {
        match self {
            Self::Num(n) => Ok(Val::num(-n.as_f64())),
            x => Err(type_error(&x, "cannot be negated")),
        }
    }
}

fn kind_order(v: &Val) -> u8 {
    match v {
        Val::Null => 1,
        Val::Bool(false) => 2,
        Val::Bool(true) => 3,
        Val::Num(_) => 4,
        Val::TStr(_) | Val::BStr(_) => 5,
        Val::Arr(_) => 6,
        Val::Obj(_) => 7,
    }
}

/// Chaves de um objeto em ordem (`jv_keys`: bytes, e a mais curta primeiro quando uma é prefixo).
pub(crate) fn sorted_keys(o: &Map) -> Vec<&Val> {
    let mut keys: Vec<&Val> = o.keys().collect();
    keys.sort_by(|a, b| jqfmt::key_bytes(a).cmp(jqfmt::key_bytes(b)));
    keys
}

impl PartialOrd for Val {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `jv_cmp`.
impl Ord for Val {
    fn cmp(&self, other: &Self) -> Ordering {
        let (ka, kb) = (kind_order(self), kind_order(other));
        if ka != kb {
            return ka.cmp(&kb);
        }
        match (self, other) {
            (Self::Num(x), Self::Num(y)) => x.jv_cmp(y),
            (Self::BStr(x) | Self::TStr(x), Self::BStr(y) | Self::TStr(y)) => x.as_ref().cmp(y.as_ref()),
            (Self::Arr(x), Self::Arr(y)) => x.iter().cmp(y.iter()),
            (Self::Obj(x), Self::Obj(y)) => {
                let (kx, ky) = (sorted_keys(x), sorted_keys(y));
                let keys = kx.iter().map(|k| jqfmt::key_bytes(k)).cmp(ky.iter().map(|k| jqfmt::key_bytes(k)));
                keys.then_with(|| {
                    for k in kx {
                        let r = x[k].cmp(&y[k]);
                        if r != Ordering::Equal {
                            return r;
                        }
                    }
                    Ordering::Equal
                })
            }
            _ => Ordering::Equal,
        }
    }
}

/// `jv_equal`.
impl PartialEq for Val {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(x), Self::Bool(y)) => x == y,
            (Self::Num(x), Self::Num(y)) => x.jv_equal(y),
            (Self::BStr(x) | Self::TStr(x), Self::BStr(y) | Self::TStr(y)) => x == y,
            (Self::Arr(x), Self::Arr(y)) => Rc::ptr_eq(x, y) || x == y,
            (Self::Obj(x), Self::Obj(y)) => {
                Rc::ptr_eq(x, y) || (x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| v == w)))
            }
            _ => false,
        }
    }
}

impl Eq for Val {}

impl Hash for Val {
    fn hash<H: Hasher>(&self, state: &mut H) {
        fn hash_with(u: u8, x: impl Hash, state: &mut impl Hasher) {
            state.write_u8(u);
            x.hash(state)
        }
        match self {
            Self::Num(n) => n.hash(state),
            Self::Null => state.write_u8(2),
            Self::Bool(b) => state.write_u8(if *b { 3 } else { 4 }),
            Self::BStr(b) | Self::TStr(b) => hash_with(5, b, state),
            Self::Arr(a) => hash_with(6, a, state),
            Self::Obj(o) => {
                state.write_u8(7);
                for k in sorted_keys(o) {
                    (k, &o[k]).hash(state);
                }
            }
        }
    }
}

/// Serialização compacta do jq.
impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&jqfmt::dump_compact(self))
    }
}
