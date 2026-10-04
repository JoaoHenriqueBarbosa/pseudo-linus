//! Porte pseudo-linus: indexação e caminhos iguais ao `src/jv_aux.c` do jq 1.7.1 (`jv_get`,
//! `jv_set`, `jv_has`, `jv_getpath`, `jv_setpath`, `jv_delpaths`, `parse_slice`).

use crate::{err, jqfmt, jqparse, Error, Map, Rc, Val, ValR, INT_MAX};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use bytes::Bytes;

fn kind(v: &Val) -> &'static str {
    jqfmt::kind_name(v)
}

/// Número de caracteres (codepoints) de uma string do jq.
pub(crate) fn codepoints(s: &[u8]) -> usize {
    match core::str::from_utf8(s) {
        Ok(s) => s.chars().count(),
        Err(_) => {
            let (mut n, mut i) = (0, 0);
            while i < s.len() {
                i += jqparse::utf8_next(&s[i..]).1;
                n += 1;
            }
            n
        }
    }
}

/// Converte um double para `int` como o C (`(int)d`, depois de saturar em `INT_MIN`/`INT_MAX`).
fn clamp_int(d: f64) -> i64 {
    let d = d.clamp(i32::MIN as f64, INT_MAX as f64);
    d as i64
}

/// `parse_slice`: início e fim (em elementos ou codepoints) de `{"start": s, "end": e}`.
fn parse_slice(j: &Val, slice: &Map) -> Result<(usize, usize), Error> {
    let start_jv = slice.get(&Val::str("start")).cloned().unwrap_or(Val::Null);
    let end_jv = slice.get(&Val::str("end")).cloned().unwrap_or(Val::Null);
    let start_jv = if matches!(start_jv, Val::Null) { Val::from(0usize) } else { start_jv };
    let len = match j {
        Val::Arr(a) => a.len(),
        Val::TStr(s) | Val::BStr(s) => codepoints(s),
        _ => return Err(err("Only arrays and strings can be sliced")),
    };
    let end_jv = if matches!(end_jv, Val::Null) { Val::from(len) } else { end_jv };
    let (Some(dstart), Some(dend)) = (start_jv.as_f64(), end_jv.as_f64()) else {
        return Err(err("Array/string slice indices must be integers"));
    };
    let len_f = len as f64;
    let mut dstart = if dstart.is_nan() { 0.0 } else { dstart };
    if dstart < 0.0 {
        dstart += len_f;
    }
    if dstart < 0.0 {
        dstart = 0.0;
    }
    if dstart > len_f {
        dstart = len_f;
    }
    let start = if dstart > INT_MAX as f64 { INT_MAX } else { dstart as i64 };
    let mut dend = if dend.is_nan() { len_f } else { dend };
    if dend < 0.0 {
        dend += len_f;
    }
    if dend < 0.0 {
        dend = start as f64;
    }
    let mut end = if dend > INT_MAX as f64 { INT_MAX } else { dend as i64 };
    let len = len as i64;
    if end > len {
        end = len;
    }
    if end < len && (end as f64) < dend {
        end += 1;
    }
    if end < start {
        end = start;
    }
    Ok((start as usize, end as usize))
}

/// Fatia de string por codepoints (`jv_string_slice`).
fn string_slice(s: &[u8], start: usize, end: usize) -> Val {
    let mut bounds = Vec::with_capacity(s.len() + 1);
    bounds.push(0);
    let mut i = 0;
    while i < s.len() {
        i += jqparse::utf8_next(&s[i..]).1;
        bounds.push(i);
    }
    let from = bounds.get(start).copied().unwrap_or(s.len());
    let to = bounds.get(end).copied().unwrap_or(s.len()).max(from);
    Val::utf8_str(Bytes::copy_from_slice(&s[from..to]))
}

/// `jv_array_indexes`.
fn array_indexes(a: &[Val], b: &[Val]) -> Val {
    let mut res = Vec::new();
    for ai in 0..a.len() {
        let mut idx: i64 = -1;
        for (bi, belem) in b.iter().enumerate() {
            match a.get(ai + bi) {
                Some(x) if x == belem => {
                    if bi == 0 && idx == -1 {
                        idx = ai as i64;
                    }
                }
                _ => idx = -1,
            }
        }
        if idx > -1 {
            res.push(Val::from(idx as usize));
        }
    }
    Val::Arr(Rc::new(res))
}

/// `jv_get`.
pub(crate) fn get(t: &Val, k: &Val) -> ValR {
    match (t, k) {
        (Val::Obj(o), k) if k.is_str() => Ok(o.get(k).cloned().unwrap_or(Val::Null)),
        (Val::Arr(a), Val::Num(n)) => {
            if n.is_nan() {
                return Ok(Val::Null);
            }
            let mut idx = clamp_int(n.as_f64());
            if idx < 0 {
                idx += a.len() as i64;
            }
            Ok(if idx >= 0 { a.get(idx as usize).cloned().unwrap_or(Val::Null) } else { Val::Null })
        }
        (Val::Arr(a), Val::Obj(o)) => {
            let (start, end) = parse_slice(t, o)?;
            Ok(Val::Arr(Rc::new(a[start..end].to_vec())))
        }
        (Val::TStr(s) | Val::BStr(s), Val::Obj(o)) => {
            let (start, end) = parse_slice(t, o)?;
            Ok(string_slice(s, start, end))
        }
        (Val::Arr(a), Val::Arr(b)) => Ok(array_indexes(a, b)),
        (Val::Null, Val::TStr(_) | Val::BStr(_) | Val::Num(_) | Val::Obj(_)) => Ok(Val::Null),
        _ => Err(index_error(t, k)),
    }
}

/// "Cannot index <tipo> with ..." do `jv_get`.
pub(crate) fn index_error(t: &Val, k: &Val) -> Error {
    match k.str_bytes() {
        Some(b) if b.len() < 30 => {
            err(format!("Cannot index {} with string \"{}\"", kind(t), String::from_utf8_lossy(b)))
        }
        _ => err(format!("Cannot index {} with {}", kind(t), kind(k))),
    }
}

/// Reserva espaço para um array crescer até `n`; falta de memória termina como o jq.
fn reserve_to(a: &mut Vec<Val>, n: usize) {
    if n > a.len() && a.try_reserve(n - a.len()).is_err() {
        jaq_core::out_of_memory();
    }
}

/// `jv_array_set` (com o índice já convertido para `int`).
fn array_set(mut a: Rc<Vec<Val>>, idx: i64, v: Val) -> ValR {
    let len = a.len() as i64;
    let idx = if idx < 0 { idx + len } else { idx };
    if idx < 0 {
        return Err(err("Out of bounds negative array index"));
    }
    let idx = idx as usize;
    let m = Rc::make_mut(&mut a);
    if idx >= m.len() {
        reserve_to(m, idx + 1);
        m.resize(idx + 1, Val::Null);
    }
    m[idx] = v;
    Ok(Val::Arr(a))
}

/// `jv_set`.
pub(crate) fn set(t: Val, k: &Val, v: Val) -> ValR {
    let isnull = matches!(t, Val::Null);
    match (&t, k) {
        (Val::Obj(_) | Val::Null, k) if k.is_str() => {
            let mut o = match t {
                Val::Obj(o) => o,
                _ => Rc::new(Map::default()),
            };
            Rc::make_mut(&mut o).insert(k.clone(), v);
            Ok(Val::Obj(o))
        }
        (Val::Arr(_) | Val::Null, Val::Num(n)) => {
            if n.is_nan() {
                return Err(err("Cannot set array element at NaN index"));
            }
            let a = match t {
                Val::Arr(a) => a,
                _ => Rc::new(Vec::new()),
            };
            array_set(a, clamp_int(n.as_f64()), v)
        }
        (Val::Arr(_) | Val::Null, Val::Obj(slice)) => {
            let mut a = match t {
                Val::Arr(a) => a,
                _ => Rc::new(Vec::new()),
            };
            let arr = Val::Arr(a.clone());
            let (start, end) = parse_slice(&arr, slice)?;
            drop(arr);
            match v {
                Val::Arr(ins) => {
                    let m = Rc::make_mut(&mut a);
                    m.splice(start..end, ins.iter().cloned());
                    Ok(Val::Arr(a))
                }
                _ => Err(err("A slice of an array can only be assigned another array")),
            }
        }
        (Val::TStr(_) | Val::BStr(_), Val::Obj(_)) => Err(err("Cannot update string slices")),
        _ => {
            let _ = isnull;
            Err(err(format!("Cannot update field at {} index of {}", kind(k), kind(&t))))
        }
    }
}

/// `jv_has`.
pub(crate) fn has(t: &Val, k: &Val) -> ValR {
    Ok(Val::Bool(match (t, k) {
        (Val::Null, _) => false,
        (Val::Obj(o), k) if k.is_str() => o.contains_key(k),
        (Val::Arr(a), Val::Num(n)) => {
            if n.is_nan() {
                false
            } else {
                let i = clamp_int(n.as_f64());
                i >= 0 && (i as usize) < a.len()
            }
        }
        _ => return Err(err(format!("Cannot check whether {} has a {} key", kind(t), kind(k)))),
    }))
}

/// `jv_getpath` com o caminho já como fatia.
pub(crate) fn getpath(mut t: Val, path: &[Val]) -> ValR {
    for k in path {
        t = get(&t, k)?;
    }
    Ok(t)
}

/// `jv_getpath` com o caminho como valor (`getpath/1`): `null` devolve a entrada.
pub(crate) fn getpath_val(t: Val, path: &Val) -> ValR {
    match path {
        Val::Null => Ok(t),
        Val::Arr(p) => getpath(t, p),
        _ => Err(err("Path must be specified as an array")),
    }
}

/// `jv_setpath`.
pub(crate) fn setpath(root: Val, path: &[Val], value: Val) -> ValR {
    let Some((cur, rest)) = path.split_first() else {
        return Ok(value);
    };
    stacker_grow(move || {
        if let Val::Obj(_) = cur {
            let sub = get(&root, cur)?;
            let newsub = setpath(sub, rest, value)?;
            return set(root, cur, newsub);
        }
        let sub = get(&root, cur)?;
        // Tira a referência do pai antes de descer, para o filho ser alterado no lugar.
        let root = set(root, cur, Val::Null)?;
        let newsub = setpath(sub, rest, value)?;
        set(root, cur, newsub)
    })
}

/// `jv_setpath` com o caminho como valor (`setpath/2`).
pub(crate) fn setpath_val(root: Val, path: &Val, value: Val) -> ValR {
    match path {
        Val::Arr(p) => setpath(root, p, value),
        _ => Err(err("Path must be specified as an array")),
    }
}

fn stacker_grow<R>(f: impl FnOnce() -> R) -> R {
    jaq_core::with_stack(f)
}

/// `jv_dels`: apaga as chaves (já ordenadas) de um array ou objeto.
fn dels(t: Val, keys: Vec<Val>) -> ValR {
    if matches!(t, Val::Null) || keys.is_empty() {
        return Ok(t);
    }
    match t {
        Val::Arr(a) => {
            let mut neg: Vec<i64> = Vec::new();
            let mut nonneg: Vec<i64> = Vec::new();
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            let arr = Val::Arr(a.clone());
            for key in &keys {
                match key {
                    Val::Num(n) => {
                        let f = n.as_f64();
                        if f < 0.0 {
                            neg.push(clamp_int(f));
                        } else {
                            nonneg.push(clamp_int(f));
                        }
                    }
                    Val::Obj(o) => ranges.push(parse_slice(&arr, o)?),
                    k => return Err(err(format!("Cannot delete {} element of array", kind(k)))),
                }
            }
            let len = a.len() as i64;
            let (mut ni, mut pi) = (0, 0);
            let mut out = Vec::new();
            for (i, elem) in a.iter().enumerate() {
                let i = i as i64;
                let mut del = false;
                while ni < neg.len() {
                    let delidx = len + neg[ni];
                    if i == delidx {
                        del = true;
                    }
                    if i < delidx {
                        break;
                    }
                    ni += 1;
                }
                while pi < nonneg.len() {
                    let delidx = nonneg[pi];
                    if i == delidx {
                        del = true;
                    }
                    if i < delidx {
                        break;
                    }
                    pi += 1;
                }
                if !del {
                    del = ranges.iter().any(|(s, e)| (*s as i64) <= i && i < (*e as i64));
                }
                if !del {
                    out.push(elem.clone());
                }
            }
            Ok(Val::Arr(Rc::new(out)))
        }
        Val::Obj(mut o) => {
            for k in &keys {
                if !k.is_str() {
                    return Err(err(format!("Cannot delete {} field of object", kind(k))));
                }
                Rc::make_mut(&mut o).shift_remove(k);
            }
            Ok(Val::Obj(o))
        }
        t => Err(err(format!("Cannot delete fields from {}", kind(&t)))),
    }
}

/// `delpaths_sorted`.
fn delpaths_sorted(mut object: Val, paths: &[Rc<Vec<Val>>], start: usize) -> ValR {
    let mut delkeys = Vec::new();
    let mut i = 0;
    while i < paths.len() {
        let key = paths[i][start].clone();
        let delkey = paths[i].len() == start + 1;
        let mut j = i;
        while j < paths.len() && paths[j][start] == key {
            j += 1;
        }
        if delkey {
            delkeys.push(key);
        } else {
            let sub = get(&object, &key)?;
            if !matches!(sub, Val::Null) {
                let newsub = jaq_core::with_stack(|| delpaths_sorted(sub, &paths[i..j], start + 1))?;
                object = set(object, &key, newsub)?;
            }
        }
        i = j;
    }
    dels(object, delkeys)
}

/// `jv_delpaths`.
pub(crate) fn delpaths(object: Val, paths: Val) -> ValR {
    let Val::Arr(paths) = paths else {
        return Err(err("Paths must be specified as an array"));
    };
    let mut paths: Vec<Val> = paths.as_ref().clone();
    paths.sort();
    let mut arrs = Vec::with_capacity(paths.len());
    for p in paths {
        match p {
            Val::Arr(a) => arrs.push(a),
            p => return Err(err(format!("Path must be specified as array, not {}", kind(&p)))),
        }
    }
    if arrs.is_empty() {
        return Ok(object);
    }
    if arrs[0].is_empty() {
        return Ok(Val::Null);
    }
    delpaths_sorted(object, &arrs, 0)
}
