//! Standard library for the jq language.
//!
//! The standard library provides a set of filters.
//! These filters are either implemented as definitions or as functions.
//! For example, the standard library provides the `map(f)` filter,
//! which is defined using the more elementary filter `[.[] | f]`.
//!
//! If you want to use the standard library in jaq, then
//! you'll likely only need [`funs`] and [`defs`].
//! Most other functions are relevant if you
//! want to implement your own native filters.
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

// Porte pseudo-linus: saíram os módulos `regex` (regex-bites) e `time` (jiff com fuso do sistema) e as
// nativas `env`, `now`, `debug`/`stderr` via `log`. O jq do pseudo-linus implementa essas no ul-jq,
// com Oniguruma (ferroni), relógio e fuso do sandbox.
pub mod input;
#[cfg(feature = "math")]
mod math;
pub mod onig;

#[cfg(feature = "format")]
use alloc::string::ToString;
use alloc::{boxed::Box, vec::Vec};
use bstr::ByteSlice;
use jaq_core::box_iter::{box_once, BoxIter};
use jaq_core::native::{bome, run, unary, v, Filter, Fun};
use jaq_core::{load, Bind, DataT, Error, Exn, RunPtr, ValR, ValX, ValXs};

/// Definitions of the standard library.
pub fn defs() -> impl Iterator<Item = load::parse::Def<&'static str>> {
    load::parse(include_str!("defs.jq"), |p| p.defs())
        .unwrap()
        .into_iter()
}

/// Named filters available by default in jaq
/// which are implemented as native filters, such as `length`, `keys`, ...
///
/// Porte pseudo-linus: só as nativas que não tocam o host ([`base_funs`] e [`extra_funs`]).
pub fn funs<D: DataT>() -> impl Iterator<Item = Fun<D>>
where
    for<'a> D::V<'a>: ValT,
{
    base_funs().chain(extra_funs())
}

/// Minimal set of filters that are generic over the value type.
/// Return the minimal set of named filters available in jaq
/// which are implemented as native filters, such as `length`, `keys`, ...,
/// but not `now`, `debug`, `fromdateiso8601`, ...
///
/// Does not return filters from the standard library, such as `map`.
pub fn base_funs<D: DataT>() -> impl Iterator<Item = Fun<D>>
where
    for<'a> D::V<'a>: ValT,
{
    base_run().into_vec().into_iter().map(run)
}

/// Supplementary set of filters that are generic over the value type.
///
/// Porte pseudo-linus: só `format` e `math`.
pub fn extra_funs<D: DataT>() -> impl Iterator<Item = Fun<D>>
where
    for<'a> D::V<'a>: ValT,
{
    [format(), math()]
        .into_iter()
        .flat_map(|fs| fs.into_vec().into_iter().map(run))
}

/// Values that the standard library can operate on.
pub trait ValT: jaq_core::ValT + Ord + From<f64> + From<usize> {
    /// Convert an array into a sequence.
    ///
    /// This returns the original value as `Err` if it is not an array.
    fn into_seq<S: FromIterator<Self>>(self) -> Result<S, Self>;

    /// True if the value is integer.
    fn is_int(&self) -> bool;

    /// Use the value as machine-sized integer.
    ///
    /// If this function returns `Some(_)`, then [`Self::is_int`] must return true.
    /// However, the other direction must not necessarily be the case, because
    /// there may be integer values that are not representable by `isize`.
    fn as_isize(&self) -> Option<isize>;

    /// Use the value as floating-point number.
    ///
    /// This succeeds for all numeric values,
    /// rounding too large/small ones to +/- Infinity.
    fn as_f64(&self) -> Option<f64>;

    /// True if the value is interpreted as UTF-8 string.
    fn is_utf8_str(&self) -> bool;

    /// If the value is a string (whatever its interpretation), return its bytes.
    fn as_bytes(&self) -> Option<&[u8]>;

    /// If the value is interpreted as UTF-8 string, return its bytes.
    fn as_utf8_bytes(&self) -> Option<&[u8]> {
        self.is_utf8_str().then(|| self.as_bytes()).flatten()
    }

    /// If the value is a string (whatever its interpretation), return its bytes, else fail.
    fn try_as_bytes(&self) -> Result<&[u8], Error<Self>> {
        self.as_bytes().ok_or_else(|| self.fail_str())
    }

    /// If the value is interpreted as UTF-8 string, return its bytes, else fail.
    fn try_as_utf8_bytes(&self) -> Result<&[u8], Error<Self>> {
        self.as_utf8_bytes().ok_or_else(|| self.fail_str())
    }

    /// If the value is a string and `sub` points to a slice of the string,
    /// shorten the string to `sub`, else panic.
    fn as_sub_str(&self, sub: &[u8]) -> Self;

    /// Interpret bytes as UTF-8 string value.
    fn from_utf8_bytes(b: impl AsRef<[u8]> + Send + 'static) -> Self;
}

/// Convenience trait for implementing the core functions.
trait ValTx: ValT + Sized {
    fn into_vec(self) -> Result<Vec<Self>, Error<Self>> {
        self.into_seq().map_err(|v| Error::typ(v, "array"))
    }

    fn try_as_isize(&self) -> Result<isize, Error<Self>> {
        self.as_isize()
            .ok_or_else(|| Error::typ(self.clone(), "integer"))
    }

    fn try_as_i32(&self) -> Result<i32, Error<Self>> {
        self.try_as_isize()?.try_into().map_err(Error::str)
    }

    fn try_as_f64(&self) -> Result<f64, Error<Self>> {
        self.as_f64()
            .ok_or_else(|| Error::typ(self.clone(), "number"))
    }

    /// Apply a function to an array.
    fn mutate_arr(self, f: impl FnOnce(&mut Vec<Self>)) -> ValR<Self> {
        let mut a = self.into_vec()?;
        f(&mut a);
        Ok(Self::from_iter(a))
    }

    /// Apply a function to an array.
    fn try_mutate_arr<'a, F>(self, f: F) -> ValX<'a, Self>
    where
        F: FnOnce(&mut Vec<Self>) -> Result<(), Exn<'a, Self>>,
    {
        let mut a = self.into_vec()?;
        f(&mut a)?;
        Ok(Self::from_iter(a))
    }

    fn round(self, f: impl FnOnce(f64) -> f64) -> ValR<Self> {
        Ok(if self.is_int() {
            self
        } else {
            let f = f(self.try_as_f64()?);
            if f.is_finite() {
                if isize::MIN as f64 <= f && f <= isize::MAX as f64 {
                    Self::from(f as isize)
                } else {
                    // print floating-point number without decimal places,
                    // i.e. like an integer
                    Self::from_num(&alloc::format!("{f:.0}"))?
                }
            } else {
                Self::from(f)
            }
        })
    }

    fn map_utf8_str<B>(self, f: impl FnOnce(&[u8]) -> B) -> ValR<Self>
    where
        B: AsRef<[u8]> + Send + 'static,
    {
        Ok(Self::from_utf8_bytes(f(self.try_as_utf8_bytes()?)))
    }

    fn trim_utf8_with(&self, f: impl FnOnce(&[u8]) -> &[u8]) -> ValR<Self> {
        Ok(self.as_sub_str(f(self.try_as_utf8_bytes()?)))
    }

    /// Helper function to strip away the prefix or suffix of a string.
    fn strip_fix<F>(self, fix: &Self, f: F) -> Result<Self, Error<Self>>
    where
        F: for<'a> FnOnce(&'a [u8], &[u8]) -> Option<&'a [u8]>,
    {
        Ok(match f(self.try_as_bytes()?, fix.try_as_bytes()?) {
            Some(sub) => self.as_sub_str(sub),
            None => self,
        })
    }

    fn fail_str(&self) -> Error<Self> {
        Error::typ(self.clone(), "string")
    }
}
impl<T: ValT> ValTx for T {}

/// Sort array by the given function.
fn sort_by<'a, V: ValT>(xs: &mut [V], f: impl Fn(V) -> ValXs<'a, V>) -> Result<(), Exn<'a, V>> {
    // Some(e) iff an error has previously occurred
    let mut err = None;
    xs.sort_by_cached_key(|x| {
        if err.is_some() {
            return Vec::new();
        };
        match f(x.clone()).collect() {
            Ok(y) => y,
            Err(e) => {
                err = Some(e);
                Vec::new()
            }
        }
    });
    err.map_or(Ok(()), Err)
}

/// Group an array by the given function.
fn group_by<'a, V: ValT>(xs: Vec<V>, f: impl Fn(V) -> ValXs<'a, V>) -> ValX<'a, V> {
    let mut yx: Vec<(Vec<V>, V)> = xs
        .into_iter()
        .map(|x| Ok((f(x.clone()).collect::<Result<_, _>>()?, x)))
        .collect::<Result<_, Exn<_>>>()?;

    yx.sort_by(|(y1, _), (y2, _)| y1.cmp(y2));

    let mut grouped = Vec::new();
    let mut yx = yx.into_iter();
    if let Some((mut group_y, first_x)) = yx.next() {
        let mut group = Vec::from([first_x]);
        for (y, x) in yx {
            if group_y != y {
                grouped.push(V::from_iter(core::mem::take(&mut group)));
                group_y = y;
            }
            group.push(x);
        }
        if !group.is_empty() {
            grouped.push(V::from_iter(group));
        }
    }

    Ok(V::from_iter(grouped))
}

/// Get the minimum or maximum element from an array according to the given function.
fn cmp_by<'a, V: Clone, F, R>(xs: Vec<V>, f: F, replace: R) -> Result<Option<V>, Exn<'a, V>>
where
    F: Fn(V) -> ValXs<'a, V>,
    R: Fn(&[V], &[V]) -> bool,
{
    let iter = xs.into_iter();
    let mut iter = iter.map(|x| (x.clone(), f(x).collect::<Result<Vec<_>, _>>()));
    let (mut mx, mut my) = if let Some((x, y)) = iter.next() {
        (x, y?)
    } else {
        return Ok(None);
    };
    for (x, y) in iter {
        let y = y?;
        if replace(&my, &y) {
            (mx, my) = (x, y);
        }
    }
    Ok(Some(mx))
}

/// Convert a string into an array of its Unicode codepoints (with negative integers representing UTF-8 errors).
fn explode<V: ValT>(s: &[u8]) -> impl Iterator<Item = ValR<V>> + '_ {
    let invalid = [].iter();
    Explode { s, invalid }.map(|r| match r {
        Err(b) => Ok((-(b as isize)).into()),
        // conversion from u32 to isize may fail on 32-bit systems for high values of c
        Ok(c) => Ok(isize::try_from(c as u32).map_err(Error::str)?.into()),
    })
}

struct Explode<'a> {
    s: &'a [u8],
    invalid: core::slice::Iter<'a, u8>,
}
impl Iterator for Explode<'_> {
    type Item = Result<char, u8>;
    fn next(&mut self) -> Option<Self::Item> {
        self.invalid.next().map(|next| Err(*next)).or_else(|| {
            let (c, size) = bstr::decode_utf8(self.s);
            let (consumed, rest) = self.s.split_at(size);
            self.s = rest;
            c.map(Ok).or_else(|| {
                // invalid UTF-8 sequence, emit all invalid bytes
                self.invalid = consumed.iter();
                self.invalid.next().map(|next| Err(*next))
            })
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let max = self.s.len();
        let min = self.s.len() / 4;
        let inv = self.invalid.as_slice().len();
        (min + inv, Some(max + inv))
    }
}

/// Convert an array of Unicode codepoints (with negative integers representing UTF-8 errors) into a string.
fn implode<V: ValT>(xs: &[V]) -> Result<Vec<u8>, Error<V>> {
    let mut v = Vec::with_capacity(xs.len());
    for x in xs {
        // on 32-bit systems, some high u32 values cannot be represented as isize
        let i = x.try_as_isize()?;
        if let Ok(b) = u8::try_from(-i) {
            v.push(b)
        } else {
            // may fail e.g. on `[1114112] | implode`
            let c = u32::try_from(i).ok().and_then(char::from_u32);
            let c = c.ok_or_else(|| Error::str(format_args!("cannot use {i} as character")))?;
            v.extend(c.encode_utf8(&mut [0; 4]).as_bytes())
        }
    }
    Ok(v)
}

fn once_or_empty<'a, T: 'a, E: 'a>(r: Result<Option<T>, E>) -> BoxIter<'a, Result<T, E>> {
    Box::new(r.transpose().into_iter())
}

// Primitive float rounding methods are unavailable without `std`.
// These can be dropped after `core_float_math` lands: https://github.com/rust-lang/rust/issues/137578
// Porte pseudo-linus: o crate é sempre `no_std` aqui.
fn floor(x: f64) -> f64 {
    no_std_float::floor(x)
}

fn round(x: f64) -> f64 {
    no_std_float::round(x)
}

fn ceil(x: f64) -> f64 {
    no_std_float::ceil(x)
}

mod no_std_float {
    const SIGN_MASK: u64 = 1 << 63;
    const SIG_BITS: i32 = 52;
    const SIG_MASK: u64 = (1 << SIG_BITS) - 1;
    const EXPONENT_BIAS: i32 = 1023;

    // Adapted from Rust's MIT-licensed `libm` implementation:
    // https://github.com/rust-lang/compiler-builtins/blob/5c5f07851b1878013ac81e129b0517feaaf8661d/libm/src/math/generic/trunc.rs
    fn trunc(x: f64) -> f64 {
        let xi = x.to_bits();
        let e = ((xi >> SIG_BITS) & 0x7ff) as i32 - EXPONENT_BIAS;
        if e >= SIG_BITS {
            return x;
        }
        let clear_mask = if e < 0 { !SIGN_MASK } else { SIG_MASK >> e };
        let cleared = xi & clear_mask;
        f64::from_bits(xi ^ cleared)
    }

    pub fn floor(x: f64) -> f64 {
        let trunc = trunc(x);
        if x < trunc {
            trunc - 1.0
        } else {
            trunc
        }
    }

    pub fn round(x: f64) -> f64 {
        let trunc = trunc(x);
        let fract = x - trunc;
        if fract >= 0.5 {
            trunc + 1.0
        } else if fract <= -0.5 {
            trunc - 1.0
        } else {
            trunc
        }
    }

    pub fn ceil(x: f64) -> f64 {
        let trunc = trunc(x);
        if x > trunc {
            trunc + 1.0
        } else {
            trunc
        }
    }

    // Porte pseudo-linus: o teste compara com o `f64` do std, que existe em `cfg(test)`.
    #[cfg(test)]
    mod tests {
        extern crate std;
        #[track_caller]
        fn assert_same(actual: f64, expected: f64) {
            if expected.is_nan() {
                assert!(actual.is_nan());
            } else {
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
        }

        #[test]
        fn matches_std() {
            let values = [
                f64::NEG_INFINITY,
                -((1_u64 << 53) as f64),
                -1.5,
                -0.5,
                -f64::from_bits(1),
                -0.0,
                0.0,
                f64::from_bits(1),
                0.5,
                1.5,
                (1_u64 << 53) as f64,
                f64::INFINITY,
                f64::NAN,
            ];
            for x in values {
                assert_same(super::trunc(x), x.trunc());
                assert_same(super::floor(x), x.floor());
                assert_same(super::round(x), x.round());
                assert_same(super::ceil(x), x.ceil());
            }
        }
    }
}

#[allow(clippy::unit_arg)]
fn base_run<D: DataT>() -> Box<[Filter<RunPtr<D>>]>
where
    for<'a> D::V<'a>: ValT,
{
    let f = || [Bind::Fun(())].into();
    Box::new([
        ("floor", v(0), |cv| bome(cv.1.round(floor))),
        ("round", v(0), |cv| bome(cv.1.round(round))),
        ("ceil", v(0), |cv| bome(cv.1.round(ceil))),
        ("utf8bytelength", v(0), |cv| {
            bome(cv.1.try_as_utf8_bytes().map(|s| (s.len() as isize).into()))
        }),
        ("explode", v(0), |cv| {
            bome(cv.1.try_as_utf8_bytes().and_then(|s| explode(s).collect()))
        }),
        ("implode", v(0), |cv| {
            let implode = |s: Vec<_>| implode(&s);
            bome(cv.1.into_vec().and_then(implode).map(D::V::from_utf8_bytes))
        }),
        ("ascii_downcase", v(0), |cv| {
            bome(cv.1.map_utf8_str(ByteSlice::to_ascii_lowercase))
        }),
        ("ascii_upcase", v(0), |cv| {
            bome(cv.1.map_utf8_str(ByteSlice::to_ascii_uppercase))
        }),
        ("reverse", v(0), |cv| bome(cv.1.mutate_arr(|a| a.reverse()))),
        ("sort", v(0), |cv| bome(cv.1.mutate_arr(|a| a.sort()))),
        ("sort_by", f(), |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            let f = move |v| f.run((fc.clone(), v));
            box_once(cv.1.try_mutate_arr(|a| sort_by(a, f)))
        }),
        ("group_by", f(), |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            let f = move |v| f.run((fc.clone(), v));
            box_once((|| group_by(cv.1.into_vec()?, f))())
        }),
        ("min_by_or_empty", f(), |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            let f = move |a| cmp_by(a, |v| f.run((fc.clone(), v)), |my, y| y < my);
            once_or_empty(cv.1.into_vec().map_err(Exn::from).and_then(f))
        }),
        ("max_by_or_empty", f(), |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            let f = move |a| cmp_by(a, |v| f.run((fc.clone(), v)), |my, y| y >= my);
            once_or_empty(cv.1.into_vec().map_err(Exn::from).and_then(f))
        }),
        ("startswith", v(1), |cv| {
            unary(cv, |v, s| {
                Ok(v.try_as_bytes()?.starts_with(s.try_as_bytes()?).into())
            })
        }),
        ("endswith", v(1), |cv| {
            unary(cv, |v, s| {
                Ok(v.try_as_bytes()?.ends_with(s.try_as_bytes()?).into())
            })
        }),
        ("ltrimstr", v(1), |cv| {
            unary(cv, |v, pre| v.strip_fix(&pre, <[u8]>::strip_prefix))
        }),
        ("rtrimstr", v(1), |cv| {
            unary(cv, |v, suf| v.strip_fix(&suf, <[u8]>::strip_suffix))
        }),
        ("trim", v(0), |cv| {
            bome(cv.1.trim_utf8_with(ByteSlice::trim))
        }),
        ("ltrim", v(0), |cv| {
            bome(cv.1.trim_utf8_with(ByteSlice::trim_start))
        }),
        ("rtrim", v(0), |cv| {
            bome(cv.1.trim_utf8_with(ByteSlice::trim_end))
        }),
        ("escape_sh", v(0), |cv| {
            bome(
                cv.1.try_as_utf8_bytes()
                    .map(|s| ValT::from_utf8_bytes(s.replace(b"'", b"'\\''"))),
            )
        }),
        ("halt", v(1), |mut cv| {
            let exit_code = cv.0.pop_var().try_as_i32().map_err(Exn::from);
            box_once(exit_code.and_then(|exit_code| Err(Exn::halt(exit_code))))
        }),
    ])
}

// Porte pseudo-linus: `env` e `now` saíram (liam `std::env::vars` e `SystemTime` do host).

#[cfg(feature = "format")]
fn replace(s: &[u8], patterns: &[&str], replacements: &[&str]) -> Vec<u8> {
    let ac = aho_corasick::AhoCorasick::new(patterns).unwrap();
    ac.replace_all_bytes(s, replacements)
}

#[cfg(feature = "format")]
fn format<D: DataT>() -> Box<[Filter<RunPtr<D>>]>
where
    for<'a> D::V<'a>: ValT,
{
    const HTML_PATS: [&str; 5] = ["<", ">", "&", "\'", "\""];
    const HTML_REPS: [&str; 5] = ["&lt;", "&gt;", "&amp;", "&apos;", "&quot;"];
    Box::new([
        ("escape_html", v(0), |cv| {
            bome(cv.1.map_utf8_str(|s| replace(s, &HTML_PATS, &HTML_REPS)))
        }),
        ("unescape_html", v(0), |cv| {
            bome(cv.1.map_utf8_str(|s| replace(s, &HTML_REPS, &HTML_PATS)))
        }),
        ("encode_uri", v(0), |cv| {
            bome(cv.1.map_utf8_str(|s| urlencoding::encode_binary(s).to_string()))
        }),
        ("decode_uri", v(0), |cv| {
            bome(cv.1.map_utf8_str(|s| urlencoding::decode_binary(s).to_vec()))
        }),
        ("encode_base64", v(0), |cv| {
            use base64::{engine::general_purpose::STANDARD, Engine};
            bome(cv.1.map_utf8_str(|s| STANDARD.encode(s)))
        }),
        ("decode_base64", v(0), |cv| {
            use base64::{engine::general_purpose::STANDARD, Engine};
            bome(cv.1.try_as_utf8_bytes().and_then(|s| {
                STANDARD
                    .decode(s)
                    .map_err(Error::str)
                    .map(ValT::from_utf8_bytes)
            }))
        }),
    ])
}

#[cfg(feature = "math")]
fn math<D: DataT>() -> Box<[Filter<RunPtr<D>>]>
where
    for<'a> D::V<'a>: ValT,
{
    let rename = |name, (_name, arity, f): Filter<RunPtr<D>>| (name, arity, f);
    Box::new([
        math::f_f!(acos),
        math::f_f!(acosh),
        math::f_f!(asin),
        math::f_f!(asinh),
        math::f_f!(atan),
        math::f_f!(atanh),
        math::f_f!(cbrt),
        math::f_f!(cos),
        math::f_f!(cosh),
        math::f_f!(erf),
        math::f_f!(erfc),
        math::f_f!(exp),
        math::f_f!(exp10),
        math::f_f!(exp2),
        math::f_f!(expm1),
        math::f_f!(fabs),
        math::f_fi!(frexp),
        math::f_i!(ilogb),
        math::f_f!(j0),
        math::f_f!(j1),
        math::f_f!(lgamma),
        math::f_f!(log),
        math::f_f!(log10),
        math::f_f!(log1p),
        math::f_f!(log2),
        // logb is implemented in jaq-std
        math::f_ff!(modf),
        rename("nearbyint", math::f_f!(round)),
        // pow10 is implemented in jaq-std
        math::f_f!(rint),
        // significand is implemented in jaq-std
        math::f_f!(sin),
        math::f_f!(sinh),
        math::f_f!(sqrt),
        math::f_f!(tan),
        math::f_f!(tanh),
        math::f_f!(tgamma),
        math::f_f!(trunc),
        math::f_f!(y0),
        math::f_f!(y1),
        math::ff_f!(atan2),
        math::ff_f!(copysign),
        // drem is implemented in jaq-std
        math::ff_f!(fdim),
        math::ff_f!(fmax),
        math::ff_f!(fmin),
        math::ff_f!(fmod),
        math::ff_f!(hypot),
        math::if_f!(jn),
        math::fi_f!(ldexp),
        math::ff_f!(nextafter),
        // nexttoward is implemented in jaq-std
        math::ff_f!(pow),
        math::ff_f!(remainder),
        // scalb is implemented in jaq-std
        rename("scalbln", math::fi_f!(scalbn)),
        math::if_f!(yn),
        math::fff_f!(fma),
    ])
}

// Porte pseudo-linus: `regex`, `time` e `log` saíram daqui (ver o cabeçalho do crate).
