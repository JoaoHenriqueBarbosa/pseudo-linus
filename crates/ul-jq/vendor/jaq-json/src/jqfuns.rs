//! Porte pseudo-linus: nativas do jq 1.7.1 que só dependem do valor (as `cfunction` de
//! `src/builtin.c` e as bytecodadas `range/2`), com as mesmas mensagens de erro. Substitui o
//! `funs.rs` do jaq-json. As que dependem do processo (entradas, ambiente, tempo, `debug`,
//! `stderr`, `halt`) ficam no ul-jq; regex e datas, no jaq-std.

use crate::{err, jqfmt, jqparse, jqpath, sorted_keys, split_str, type_error, type_error2, Error, Map, Rc, Val, ValR};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use bytes::Bytes;
use jaq_core::box_iter::box_once;
use jaq_core::data::HasLut;
use jaq_core::native::{bome, Fun};
use jaq_core::{Bind, DataT, Exn, Native, RunPtr, ValX, ValXs};

type Filter<D> = (&'static str, Box<[Bind]>, RunPtr<D>);

fn v(n: usize) -> Box<[Bind]> {
    core::iter::repeat_n(Bind::Var(()), n).collect()
}

fn bstr(b: &[u8]) -> Val {
    Val::utf8_str(Bytes::copy_from_slice(b))
}

/// Nativas de valor do jq.
pub fn funs<D: for<'a> DataT<V<'a> = Val>>() -> impl Iterator<Item = Fun<D>> {
    let mut out: Vec<Fun<D>> = base::<D>().into_vec().into_iter().map(jaq_core::native::run::<D>).collect();
    out.extend(math::<D>().into_vec().into_iter().map(jaq_core::native::run::<D>));
    out.push(("getpath", v(1), Native::<D>::new(getpath_run::<D>).with_paths(getpath_paths::<D>)));
    out.into_iter()
}

fn getpath_run<D: for<'a> DataT<V<'a> = Val>>(mut cv: jaq_core::Cv<'_, D>) -> ValXs<'_, Val> {
    let path = cv.0.pop_var();
    bome(jqpath::getpath_val(cv.1, &path))
}

/// Valor com caminho rastreado opcional (ver `path(...)` no jaq-core).
type Tracked = (Val, Option<jaq_core::RcList<Val>>);

/// `f_getpath` com o `_jq_path_append` do jq: dentro de `path(...)`, estende o caminho rastreado.
fn getpath_paths<D: for<'a> DataT<V<'a> = Val>>(mut cv: jaq_core::Cv<'_, D, Tracked>) -> ValXs<'_, Tracked, Val> {
    let path = cv.0.pop_var();
    let (v, p) = cv.1;
    let r = jqpath::getpath_val(v, &path).map(|res| {
        let p = p.map(|p| match &path {
            Val::Arr(a) => a.iter().cloned().fold(p, |p, k| p.cons(k)),
            other => p.cons(other.clone()),
        });
        (res, p)
    });
    box_once(r.map_err(Exn::from))
}

fn base<D: for<'a> DataT<V<'a> = Val>>() -> Box<[Filter<D>]> {
    Box::new([
        ("error", v(0), |cv| bome(Err(Error::new(cv.1)))),
        ("length", v(0), |cv| bome(length(&cv.1))),
        ("utf8bytelength", v(0), |cv| {
            bome(match cv.1.str_bytes() {
                Some(b) => Ok(Val::from(b.len())),
                None => Err(type_error(&cv.1, "only strings have UTF-8 byte length")),
            })
        }),
        ("type", v(0), |cv| bome(Ok(Val::str(cv.1.kind_name())))),
        ("keys", v(0), |cv| bome(keys(&cv.1, true))),
        ("keys_unsorted", v(0), |cv| bome(keys(&cv.1, false))),
        ("has", v(1), |mut cv| {
            let k = cv.0.pop_var();
            bome(jqpath::has(&cv.1, &k))
        }),
        ("contains", v(1), |mut cv| {
            let b = cv.0.pop_var();
            bome(if kind_of(&cv.1) == kind_of(&b) {
                Ok(Val::Bool(contains(&cv.1, &b)))
            } else {
                Err(type_error2(&cv.1, &b, "cannot have their containment checked"))
            })
        }),
        ("tojson", v(0), |cv| bome(Ok(Val::from(jqfmt::dump_compact(&cv.1))))),
        ("tostring", v(0), |cv| bome(Ok(tostring(cv.1)))),
        ("fromjson", v(0), |cv| {
            bome(match cv.1.str_bytes() {
                Some(b) => jqparse::parse_single(b).map_err(err),
                None => Err(type_error(&cv.1, "only strings can be parsed")),
            })
        }),
        ("tonumber", v(0), |cv| bome(tonumber(cv.1))),
        ("startswith", v(1), |mut cv| {
            let b = cv.0.pop_var();
            bome(match (cv.1.str_bytes(), b.str_bytes()) {
                (Some(a), Some(b)) => Ok(Val::Bool(a.starts_with(b))),
                _ => Err(err("startswith() requires string inputs")),
            })
        }),
        ("endswith", v(1), |mut cv| {
            let b = cv.0.pop_var();
            bome(match (cv.1.str_bytes(), b.str_bytes()) {
                (Some(a), Some(b)) => Ok(Val::Bool(a.ends_with(b))),
                _ => Err(err("endswith() requires string inputs")),
            })
        }),
        ("ltrimstr", v(1), |mut cv| {
            let pre = cv.0.pop_var();
            bome(Ok(match (cv.1.str_bytes(), pre.str_bytes()) {
                (Some(s), Some(p)) if s.starts_with(p) => bstr(&s[p.len()..]),
                _ => cv.1,
            }))
        }),
        ("rtrimstr", v(1), |mut cv| {
            let suf = cv.0.pop_var();
            bome(Ok(match (cv.1.str_bytes(), suf.str_bytes()) {
                (Some(s), Some(p)) if s.ends_with(p) => bstr(&s[..s.len() - p.len()]),
                _ => cv.1,
            }))
        }),
        ("split", v(1), |mut cv| {
            let sep = cv.0.pop_var();
            bome(match (cv.1.str_bytes(), sep.str_bytes()) {
                (Some(s), Some(p)) => Ok(split_str(s, p)),
                _ => Err(err("split input and separator must be strings")),
            })
        }),
        ("explode", v(0), |cv| bome(explode(&cv.1))),
        ("implode", v(0), |cv| bome(implode(&cv.1))),
        ("ascii_downcase", v(0), |cv| bome(ascii_case(&cv.1, false))),
        ("ascii_upcase", v(0), |cv| bome(ascii_case(&cv.1, true))),
        ("_strindices", v(1), |mut cv| {
            let k = cv.0.pop_var();
            bome(strindices(&cv.1, &k))
        }),
        ("setpath", v(2), |mut cv| {
            let value = cv.0.pop_var();
            let path = cv.0.pop_var();
            bome(jqpath::setpath_val(cv.1, &path, value))
        }),
        ("delpaths", v(1), |mut cv| {
            let paths = cv.0.pop_var();
            bome(jqpath::delpaths(cv.1, paths))
        }),
        ("isinfinite", v(0), |cv| bome(Ok(Val::Bool(cv.1.as_f64().is_some_and(f64::is_infinite))))),
        ("isnan", v(0), |cv| bome(Ok(Val::Bool(cv.1.as_f64().is_some_and(f64::is_nan))))),
        ("isnormal", v(0), |cv| bome(Ok(Val::Bool(cv.1.as_f64().is_some_and(f64::is_normal))))),
        ("infinite", v(0), |_| bome(Ok(Val::num(f64::INFINITY)))),
        ("nan", v(0), |_| bome(Ok(Val::num(f64::NAN)))),
        ("sort", v(0), |cv| {
            bome(match cv.1 {
                Val::Arr(mut a) => {
                    Rc::make_mut(&mut a).sort();
                    Ok(Val::Arr(a))
                }
                v => Err(type_error(&v, "cannot be sorted, as it is not an array")),
            })
        }),
        ("_sort_by_impl", v(1), |mut cv| {
            let keys = cv.0.pop_var();
            bome(sort_by_keys(cv.1, keys, false))
        }),
        ("_group_by_impl", v(1), |mut cv| {
            let keys = cv.0.pop_var();
            bome(sort_by_keys(cv.1, keys, true))
        }),
        ("min", v(0), |cv| bome(minmax_by(&cv.1, &cv.1, true))),
        ("max", v(0), |cv| bome(minmax_by(&cv.1, &cv.1, false))),
        ("_min_by_impl", v(1), |mut cv| {
            let keys = cv.0.pop_var();
            bome(minmax_by(&cv.1, &keys, true))
        }),
        ("_max_by_impl", v(1), |mut cv| {
            let keys = cv.0.pop_var();
            bome(minmax_by(&cv.1, &keys, false))
        }),
        ("format", v(1), |mut cv| {
            let fmt = cv.0.pop_var();
            bome(format(cv.1, &fmt))
        }),
        ("range", v(2), |mut cv| {
            let to = cv.0.pop_var();
            let from = cv.0.pop_var();
            range2::<D>(cv.0.data().clone(), from, to)
        }),
        ("range", v(3), |mut cv| {
            let by = cv.0.pop_var();
            let upto = cv.0.pop_var();
            let init = cv.0.pop_var();
            range3::<D>(cv.0.data().clone(), init, upto, by)
        }),
    ])
}

/// Ordem dos tipos (`jv_get_kind`), onde `true` e `false` são tipos diferentes.
fn kind_of(v: &Val) -> u8 {
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

/// `f_length`.
fn length(v: &Val) -> ValR {
    match v {
        Val::Arr(a) => Ok(Val::from(a.len())),
        Val::Obj(o) => Ok(Val::from(o.len())),
        Val::TStr(s) | Val::BStr(s) => Ok(Val::from(jqpath::codepoints(s))),
        Val::Num(n) => Ok(Val::num(n.as_f64().abs())),
        Val::Null => Ok(Val::from(0usize)),
        Val::Bool(_) => Err(type_error(v, "has no length")),
    }
}

/// `f_keys` / `f_keys_unsorted`.
fn keys(v: &Val, sorted: bool) -> ValR {
    match v {
        Val::Obj(o) => {
            let ks: Vec<Val> = if sorted { sorted_keys(o).into_iter().cloned().collect() } else { o.keys().cloned().collect() };
            Ok(Val::Arr(Rc::new(ks)))
        }
        Val::Arr(a) => Ok((0..a.len()).map(Val::from).collect()),
        _ => Err(type_error(v, "has no keys")),
    }
}

/// `jv_contains`.
fn contains(a: &Val, b: &Val) -> bool {
    if kind_of(a) != kind_of(b) {
        return false;
    }
    match (a, b) {
        (Val::Obj(a), Val::Obj(b)) => b.iter().all(|(k, bv)| a.get(k).is_some_and(|av| contains(av, bv))),
        (Val::Arr(a), Val::Arr(b)) => b.iter().all(|bv| a.iter().any(|av| contains(av, bv))),
        (Val::TStr(a) | Val::BStr(a), Val::TStr(b) | Val::BStr(b)) => {
            b.is_empty() || a.windows(b.len()).any(|w| w == b.as_ref())
        }
        _ => a == b,
    }
}

/// `f_tostring`.
pub(crate) fn tostring(v: Val) -> Val {
    if v.is_str() {
        v
    } else {
        Val::from(jqfmt::dump_compact(&v))
    }
}

/// `f_tonumber`.
fn tonumber(v: Val) -> ValR {
    match &v {
        Val::Num(_) => Ok(v),
        Val::TStr(s) | Val::BStr(s) => match jqparse::parse_single(s) {
            Ok(n @ Val::Num(_)) => Ok(n),
            Ok(_) => Err(type_error(&v, "cannot be parsed as a number")),
            Err(e) => Err(err(e)),
        },
        _ => Err(type_error(&v, "cannot be parsed as a number")),
    }
}

/// `jv_string_explode`.
fn explode(v: &Val) -> ValR {
    let Some(s) = v.str_bytes() else {
        return Err(err("explode input must be a string"));
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let (cp, len) = jqparse::utf8_next(&s[i..]);
        out.push(Val::from(cp.unwrap_or(0xFFFD) as usize));
        i += len;
    }
    Ok(Val::Arr(Rc::new(out)))
}

/// `f_string_implode`.
fn implode(v: &Val) -> ValR {
    let Val::Arr(a) = v else {
        return Err(err("implode input must be an array"));
    };
    let mut s = String::new();
    for n in a.iter() {
        let Some(f) = n.as_f64().filter(|f| !f.is_nan()) else {
            return Err(type_error(n, "can't be imploded, unicode codepoint needs to be numeric"));
        };
        let nv = f.clamp(i32::MIN as f64, i32::MAX as f64) as i64;
        let c = if nv < 0 || nv > 0x10FFFF || (0xD800..=0xDFFF).contains(&nv) { 0xFFFD } else { nv as u32 };
        s.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
    }
    Ok(Val::from(s))
}

/// `ascii_downcase`/`ascii_upcase` (no jq são `explode | map(...) | implode`).
fn ascii_case(v: &Val, upper: bool) -> ValR {
    let Some(s) = v.str_bytes() else {
        return Err(err("explode input must be a string"));
    };
    let out: Vec<u8> = s.iter().map(|c| if upper { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect();
    Ok(Val::utf8_str(Bytes::from(out)))
}

/// `jv_string_indexes` (posições em bytes, como no jq 1.7.1).
fn strindices(a: &Val, b: &Val) -> ValR {
    let (Some(s), Some(k)) = (a.str_bytes(), b.str_bytes()) else {
        // O jq 1.7.1 só chama `_strindices` com duas strings (a definição de `indices` garante).
        return Err(err(format!("Cannot index {} with {}", a.kind_name(), b.kind_name())));
    };
    let mut out = Vec::new();
    if !k.is_empty() && k.len() <= s.len() {
        for i in 0..=s.len() - k.len() {
            if &s[i..i + k.len()] == k {
                out.push(Val::from(i));
            }
        }
    }
    Ok(Val::Arr(Rc::new(out)))
}

/// `f_sort_by_impl` e `f_group_by_impl` (`jv_sort`/`jv_group`: ordenação estável pela chave).
fn sort_by_keys(input: Val, keys: Val, group: bool) -> ValR {
    match (&input, &keys) {
        (Val::Arr(a), Val::Arr(k)) if a.len() == k.len() => {
            let mut entries: Vec<(&Val, &Val)> = k.iter().zip(a.iter()).collect();
            entries.sort_by(|x, y| x.0.cmp(y.0));
            if !group {
                return Ok(entries.into_iter().map(|(_, v)| v.clone()).collect());
            }
            let mut groups: Vec<Val> = Vec::new();
            let mut cur: Vec<Val> = Vec::new();
            let mut cur_key: Option<&Val> = None;
            for (k, v) in entries {
                match cur_key {
                    Some(ck) if ck == k => {}
                    Some(_) => {
                        groups.push(Val::Arr(Rc::new(core::mem::take(&mut cur))));
                        cur_key = Some(k);
                    }
                    None => cur_key = Some(k),
                }
                cur.push(v.clone());
            }
            if cur_key.is_some() {
                groups.push(Val::Arr(Rc::new(cur)));
            }
            Ok(Val::Arr(Rc::new(groups)))
        }
        _ => Err(type_error2(&input, &keys, "cannot be sorted, as they are not both arrays")),
    }
}

/// `minmax_by`.
fn minmax_by(values: &Val, keys: &Val, is_min: bool) -> ValR {
    let (Val::Arr(vs), Val::Arr(ks)) = (values, keys) else {
        return Err(type_error2(values, keys, "cannot be iterated over"));
    };
    if vs.len() != ks.len() {
        return Err(type_error2(values, keys, "have wrong length"));
    }
    if vs.is_empty() {
        return Ok(Val::Null);
    }
    let (mut ret, mut retkey) = (0, &ks[0]);
    for (i, item) in ks.iter().enumerate().skip(1) {
        let less = item.cmp(retkey) == core::cmp::Ordering::Less;
        if less == is_min {
            retkey = item;
            ret = i;
        }
    }
    Ok(vs[ret].clone())
}

/// `escape_string`.
fn escape(s: &[u8], map: &[(u8, &str)]) -> String {
    let mut out = String::new();
    let text = jqparse::utf8_lossy(s);
    for c in text.chars() {
        if (c as u32) < 128 {
            if c == '\0' {
                out.push_str("\\0");
                continue;
            }
            if let Some((_, rep)) = map.iter().find(|(k, _)| *k == c as u8) {
                out.push_str(rep);
                continue;
            }
        }
        out.push(c);
    }
    out
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a') as u32 + 26),
        b'0'..=b'9' => Some((c - b'0') as u32 + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// `f_format`.
fn format(input: Val, fmt: &Val) -> ValR {
    let Some(f) = fmt.as_str() else {
        return Err(type_error(fmt, "is not a valid format"));
    };
    match f {
        "json" => Ok(Val::from(jqfmt::dump_compact(&input))),
        "text" => Ok(tostring(input)),
        "csv" | "tsv" => {
            let csv = f == "csv";
            let Val::Arr(a) = &input else {
                let msg = if csv { "cannot be csv-formatted, only array" } else { "cannot be tsv-formatted, only array" };
                return Err(type_error(&input, msg));
            };
            let mut line = String::new();
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    line.push_str(if csv { "," } else { "\t" });
                }
                match x {
                    Val::Null => {}
                    Val::Bool(_) => line.push_str(&jqfmt::dump_compact(x)),
                    Val::Num(n) => {
                        if !n.as_f64().is_nan() {
                            line.push_str(&jqfmt::dump_compact(x));
                        }
                    }
                    Val::TStr(s) | Val::BStr(s) => {
                        if csv {
                            line.push('"');
                            line.push_str(&escape(s, &[(b'"', "\"\"")]));
                            line.push('"');
                        } else {
                            line.push_str(&escape(s, &[(b'\t', "\\t"), (b'\r', "\\r"), (b'\n', "\\n"), (b'\\', "\\\\")]));
                        }
                    }
                    _ => return Err(type_error(x, "is not valid in a csv row")),
                }
            }
            Ok(Val::from(line))
        }
        "html" => {
            let s = tostring(input);
            let map = [(b'&', "&amp;"), (b'<', "&lt;"), (b'>', "&gt;"), (b'\'', "&apos;"), (b'"', "&quot;")];
            Ok(Val::from(escape(s.str_bytes().unwrap_or_default(), &map)))
        }
        "uri" => {
            let s = tostring(input);
            let mut line = String::new();
            for &ch in s.str_bytes().unwrap_or_default() {
                if ch.is_ascii_alphanumeric() || b"-_.~".contains(&ch) {
                    line.push(ch as char);
                } else {
                    line.push_str(&format!("%{ch:02X}"));
                }
            }
            Ok(Val::from(line))
        }
        "sh" => {
            let items: Vec<Val> = match input {
                Val::Arr(a) => a.as_ref().clone(),
                other => alloc::vec![other],
            };
            let mut line = String::new();
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                match x {
                    Val::Null | Val::Bool(_) | Val::Num(_) => line.push_str(&jqfmt::dump_compact(x)),
                    Val::TStr(s) | Val::BStr(s) => {
                        line.push('\'');
                        line.push_str(&escape(s, &[(b'\'', "'\\''")]));
                        line.push('\'');
                    }
                    _ => return Err(type_error(x, "can not be escaped for shell")),
                }
            }
            Ok(Val::from(line))
        }
        "base64" => {
            let s = tostring(input);
            let data = s.str_bytes().unwrap_or_default();
            let mut line = String::new();
            for chunk in data.chunks(3) {
                let n = chunk.len();
                let mut code: u32 = 0;
                for j in 0..3 {
                    code <<= 8;
                    code |= if j < n { chunk[j] as u32 } else { 0 };
                }
                let mut buf = [0u8; 4];
                for (j, b) in buf.iter_mut().enumerate() {
                    *b = BASE64[((code >> (18 - j * 6)) & 0x3f) as usize];
                }
                if n < 3 {
                    buf[3] = b'=';
                }
                if n < 2 {
                    buf[2] = b'=';
                }
                line.push_str(core::str::from_utf8(&buf).unwrap_or_default());
            }
            Ok(Val::from(line))
        }
        "base64d" => {
            let s = tostring(input);
            let data = s.str_bytes().unwrap_or_default();
            let mut result = Vec::new();
            let mut code: u32 = 0;
            let mut read = 0;
            for &c in data {
                if c == b'=' {
                    break;
                }
                let Some(val) = base64_value(c) else {
                    return Err(type_error(&s, "is not valid base64 data"));
                };
                code = (code << 6) | val;
                read += 1;
                if read == 4 {
                    result.push(((code >> 16) & 0xFF) as u8);
                    result.push(((code >> 8) & 0xFF) as u8);
                    result.push((code & 0xFF) as u8);
                    read = 0;
                    code = 0;
                }
            }
            match read {
                3 => {
                    result.push(((code >> 10) & 0xFF) as u8);
                    result.push(((code >> 2) & 0xFF) as u8);
                }
                2 => result.push(((code >> 4) & 0xFF) as u8),
                1 => return Err(type_error(&s, "trailing base64 byte found")),
                _ => {}
            }
            Ok(Val::from(jqparse::utf8_lossy(&result)))
        }
        other => Err(err(format!("{other} is not a valid format"))),
    }
}

/// `range/2` (o `RANGE` bytecodado do jq): limites numéricos, passo 1, comparação em double.
fn range2<'a, D: DataT<V<'a> = Val>>(data: D::Data<'a>, from: Val, to: Val) -> ValXs<'a, Val> {
    let (Some(_), Some(upto)) = (from.as_f64(), to.as_f64()) else {
        return box_once(Err(Exn::from(err("Range bounds must be numeric"))));
    };
    let mut cur = Some(from);
    Box::new(core::iter::from_fn(move || {
        data.lut();
        let x = cur.take()?;
        let f = x.as_f64().unwrap_or(f64::NAN);
        if f >= upto || f.is_nan() {
            return None;
        }
        cur = Some(Val::num(f + 1.0));
        Some(Ok(x))
    }))
}

/// `range/3` com a semântica da definição do jq (`$init | while(. < $upto; . + $by)` etc.).
fn range3<'a, D: DataT<V<'a> = Val>>(data: D::Data<'a>, init: Val, upto: Val, by: Val) -> ValXs<'a, Val> {
    let zero = Val::from(0usize);
    let up = if by > zero {
        true
    } else if by < zero {
        false
    } else {
        return Box::new(core::iter::empty());
    };
    let mut cur = Some(init);
    let mut pending: Option<Error> = None;
    Box::new(core::iter::from_fn(move || -> Option<ValX<'a, Val>> {
        data.lut();
        if let Some(e) = pending.take() {
            return Some(Err(Exn::from(e)));
        }
        let x = cur.take()?;
        let go = if up { x < upto } else { x > upto };
        if !go {
            return None;
        }
        // O `while` do jq emite o valor antes de calcular o próximo; um erro na soma sai depois.
        match x.clone() + by.clone() {
            Ok(next) => cur = Some(next),
            Err(e) => pending = Some(e),
        }
        Some(Ok(x))
    }))
}

/// Número exigido pelas funções da libm (`type_error(.., "number required")`).
fn num_arg(v: &Val) -> Result<f64, Error> {
    v.as_f64().ok_or_else(|| type_error(v, "number required"))
}

fn ok_num(x: f64) -> ValR {
    Ok(Val::num(x))
}

macro_rules! dd {
    ($name:literal, $f:expr) => {
        ($name, v(0), |cv| bome(num_arg(&cv.1).and_then(|x| ok_num(($f)(x)))))
    };
}

macro_rules! ddd {
    ($name:literal, $f:expr) => {
        ($name, v(2), |mut cv| {
            let b = cv.0.pop_var();
            let a = cv.0.pop_var();
            bome((|| ok_num(($f)(num_arg(&a)?, num_arg(&b)?)))())
        })
    };
}

/// `ilogb` do C.
fn ilogb(x: f64) -> i32 {
    if x == 0.0 {
        i32::MIN
    } else if x.is_nan() {
        i32::MIN
    } else if x.is_infinite() {
        i32::MAX
    } else {
        libm::ilogb(x)
    }
}

fn logb(x: f64) -> f64 {
    if x == 0.0 {
        f64::NEG_INFINITY
    } else if x.is_infinite() {
        f64::INFINITY
    } else if x.is_nan() {
        x
    } else {
        ilogb(x) as f64
    }
}

fn significand(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        x
    } else {
        libm::scalbn(x, -ilogb(x))
    }
}

/// `(int)d` do C para argumentos inteiros (`jn`, `yn`, `ldexp`).
fn c_int(d: f64) -> i32 {
    if d.is_nan() {
        i32::MIN
    } else {
        d.clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}

fn scalb(x: f64, e: f64) -> f64 {
    if x.is_nan() || e.is_nan() {
        return f64::NAN;
    }
    if e.is_infinite() {
        return if e > 0.0 { x * e } else { x / -e };
    }
    if e.fract() != 0.0 {
        return f64::NAN;
    }
    libm::scalbn(x, e.clamp(-65000.0, 65000.0) as i32)
}

fn math<D: for<'a> DataT<V<'a> = Val>>() -> Box<[Filter<D>]> {
    Box::new([
        dd!("acos", f64::acos),
        dd!("acosh", f64::acosh),
        dd!("asin", f64::asin),
        dd!("asinh", f64::asinh),
        dd!("atan", f64::atan),
        dd!("atanh", f64::atanh),
        dd!("cbrt", f64::cbrt),
        dd!("cos", f64::cos),
        dd!("cosh", f64::cosh),
        dd!("exp", f64::exp),
        dd!("exp2", f64::exp2),
        dd!("exp10", libm::exp10),
        dd!("expm1", f64::exp_m1),
        dd!("fabs", f64::abs),
        dd!("floor", f64::floor),
        dd!("ceil", f64::ceil),
        dd!("round", f64::round),
        dd!("rint", f64::round_ties_even),
        dd!("nearbyint", f64::round_ties_even),
        dd!("trunc", f64::trunc),
        dd!("gamma", libm::lgamma),
        dd!("lgamma", libm::lgamma),
        dd!("tgamma", libm::tgamma),
        dd!("j0", libm::j0),
        dd!("j1", libm::j1),
        dd!("y0", libm::y0),
        dd!("y1", libm::y1),
        dd!("log", f64::ln),
        dd!("log10", f64::log10),
        dd!("log1p", f64::ln_1p),
        dd!("log2", f64::log2),
        dd!("logb", logb),
        dd!("significand", significand),
        dd!("sin", f64::sin),
        dd!("sinh", f64::sinh),
        dd!("sqrt", f64::sqrt),
        dd!("tan", f64::tan),
        dd!("tanh", f64::tanh),
        dd!("erf", libm::erf),
        dd!("erfc", libm::erfc),
        ("frexp", v(0), |cv| {
            bome(num_arg(&cv.1).map(|x| {
                let (m, e) = libm::frexp(x);
                [Val::num(m), Val::num(e as f64)].into_iter().collect()
            }))
        }),
        ("modf", v(0), |cv| {
            bome(num_arg(&cv.1).map(|x| {
                let (f, i) = libm::modf(x);
                [Val::num(f), Val::num(i)].into_iter().collect()
            }))
        }),
        ("lgamma_r", v(0), |cv| {
            bome(num_arg(&cv.1).map(|x| {
                let (l, s) = libm::lgamma_r(x);
                [Val::num(l), Val::num(s as f64)].into_iter().collect()
            }))
        }),
        ddd!("atan2", f64::atan2),
        ddd!("copysign", f64::copysign),
        ddd!("drem", libm::remainder),
        ddd!("fdim", libm::fdim),
        ddd!("fmax", f64::max),
        ddd!("fmin", f64::min),
        ddd!("fmod", |a: f64, b: f64| a % b),
        ddd!("hypot", f64::hypot),
        ddd!("jn", |a: f64, b: f64| libm::jn(c_int(a), b)),
        ddd!("yn", |a: f64, b: f64| libm::yn(c_int(a), b)),
        ddd!("ldexp", |a: f64, b: f64| libm::scalbn(a, c_int(b))),
        ddd!("nextafter", libm::nextafter),
        ddd!("nexttoward", libm::nextafter),
        ddd!("pow", f64::powf),
        ddd!("remainder", libm::remainder),
        ddd!("scalb", scalb),
        ddd!("scalbln", |a: f64, b: f64| libm::scalbn(a, c_int(b))),
        ("fma", v(3), |mut cv| {
            let c = cv.0.pop_var();
            let b = cv.0.pop_var();
            let a = cv.0.pop_var();
            bome((|| ok_num(num_arg(&a)?.mul_add(num_arg(&b)?, num_arg(&c)?)))())
        }),
    ])
}

/// Objeto com as chaves dadas, na ordem (para quem monta objetos fora daqui).
pub fn object(pairs: impl IntoIterator<Item = (Val, Val)>) -> Val {
    let mut m = Map::default();
    m.extend(pairs);
    Val::obj(m)
}
