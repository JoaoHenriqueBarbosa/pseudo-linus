//! Values that can be processed by jaq.
//!
//! To process your own value type with jaq,
//! you need to implement the [`ValT`] trait.

use crate::box_iter::BoxIter;
use core::fmt::Display;
use core::ops::{Add, Div, Mul, Neg, Rem, Sub};

// Makes `f64::from_str` accessible as intra-doc link.
#[cfg(doc)]
use core::str::FromStr;

/// Value or eRror.
pub type ValR<T, V = T> = Result<T, crate::Error<V>>;
/// Stream of values and eRrors.
pub type ValRs<'a, T, V = T> = BoxIter<'a, ValR<T, V>>;
/// Value or eXception.
///
/// Porte pseudo-linus: o `unwrap_valr` original saiu do fork (no `halt` ele chamava
/// `std::process::exit` e derrubava o processo host). Quem roda o filtro trata `Exn::get_halt`.
pub type ValX<'a, T, V = T> = Result<T, crate::Exn<'a, V>>;
/// Stream of values and eXceptions.
pub type ValXs<'a, T, V = T> = BoxIter<'a, ValX<'a, T, V>>;

/// Range of options, used for iteration operations.
pub type Range<V> = core::ops::Range<Option<V>>;

/// Values that can be processed by jaq.
///
/// Implement this trait if you want jaq to process your own type of values.
pub trait ValT:
    Clone
    + Display
    + From<bool>
    + From<isize>
    + From<alloc::string::String>
    + From<Range<Self>>
    + FromIterator<Self>
    + PartialEq
    + PartialOrd
    + Add<Output = ValR<Self>>
    + Sub<Output = ValR<Self>>
    + Mul<Output = ValR<Self>>
    + Div<Output = ValR<Self>>
    + Rem<Output = ValR<Self>>
    + Neg<Output = ValR<Self>>
{
    /// Create a number from a string.
    ///
    /// The number should adhere to the format accepted by [`f64::from_str`].
    fn from_num(n: &str) -> ValR<Self>;

    /// Create an associative map (or object) from a sequence of key-value pairs.
    ///
    /// This is used when creating values with the syntax `{k: v}`.
    fn from_map<I: IntoIterator<Item = (Self, Self)>>(iter: I) -> ValR<Self>;

    /// Yield the key-value pairs of a value.
    ///
    /// This is used to collect the paths of `.[]`.
    /// It should yield any `key` for which `value | .[key]` is defined,
    /// as well as its output.
    fn key_values(self) -> BoxIter<'static, ValR<(Self, Self), Self>>;

    /// Yield the children of a value.
    ///
    /// This is used by `.[]`.
    fn values(self) -> alloc::boxed::Box<dyn Iterator<Item = ValR<Self>>>;

    /// Yield the child of a value at the given index.
    ///
    /// This is used by `.[k]`.
    ///
    /// If `v.index(k)` is `Ok(_)`, then it is contained in `v.values()`.
    fn index(self, index: &Self) -> ValR<Self>;

    /// Yield a slice of the value with the given range.
    ///
    /// This is used by `.[s:e]`, `.[s:]`, and `.[:e]`.
    fn range(self, range: Range<&Self>) -> ValR<Self>;

    // Porte pseudo-linus: saíram `map_values`, `map_index` e `map_range`. As atualizações (`|=`, `=`,
    // `+=`...) seguem o `_modify`/`_assign` do jq 1.7.1: caminhos calculados na entrada original,
    // `getpath`/`setpath` em cada um e `delpaths` no fim para os que a atualização esvaziou.

    /// Return a boolean representation of the value.
    ///
    /// This is used by `if v then ...`.
    fn as_bool(&self) -> bool;

    /// Convert value into a string value.
    ///
    /// This is used by `"\(v)"`.
    fn into_string(self) -> Self;

    /// Porte pseudo-linus: o `null` (estado do `reduce`/`foreach` quando a atualização não tem saída).
    fn null() -> Self;

    /// Porte pseudo-linus: `jv_identical`, a identidade que o jq usa para decidir se um valor ainda
    /// está no caminho rastreado por `path(...)`.
    fn identical(&self, other: &Self) -> bool;

    /// Porte pseudo-linus: `jv_dump_string_trunc` com um buffer de `bufsize` bytes (mensagens).
    fn dump_trunc(&self, bufsize: usize) -> alloc::string::String;

    /// Porte pseudo-linus: `jv_kind_name` ("null", "boolean", "number"...), para mensagens.
    fn kind_name(&self) -> &'static str;

    /// Porte pseudo-linus: `jv_getpath`.
    fn getpath(&self, path: &[Self]) -> ValR<Self>;

    /// Porte pseudo-linus: `jv_setpath`.
    fn setpath(self, path: &[Self], value: Self) -> ValR<Self>;

    /// Porte pseudo-linus: `jv_delpaths`, com cada caminho já como array.
    fn delpaths(self, paths: alloc::vec::Vec<Self>) -> ValR<Self>;
}
