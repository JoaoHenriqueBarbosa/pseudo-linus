//! Ordens do `PROCINFO["sorted_in"]` e as funções `asort`/`asorti` do gawk 5.2.1.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::array::Subscript;
use crate::ast::Expr;
use crate::interp::{ArrRef, Array, Cell, Flow, Interp, R};
use crate::value::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    IndStr,
    IndNum,
    ValType,
    ValStr,
    ValNum,
}

enum How {
    Builtin(Key, bool),
    User(u32),
}

fn parse_how(it: &mut Interp<'_>, how: &[u8]) -> R<How> {
    let s = String::from_utf8_lossy(how).into_owned();
    let builtin = match s.as_str() {
        "@ind_str_asc" => Some((Key::IndStr, false)),
        "@ind_str_desc" => Some((Key::IndStr, true)),
        "@ind_num_asc" => Some((Key::IndNum, false)),
        "@ind_num_desc" => Some((Key::IndNum, true)),
        "@val_type_asc" => Some((Key::ValType, false)),
        "@val_type_desc" => Some((Key::ValType, true)),
        "@val_str_asc" => Some((Key::ValStr, false)),
        "@val_str_desc" => Some((Key::ValStr, true)),
        "@val_num_asc" => Some((Key::ValNum, false)),
        "@val_num_desc" => Some((Key::ValNum, true)),
        _ => None,
    };
    if let Some((k, d)) = builtin {
        return Ok(How::Builtin(k, d));
    }
    match it.p.functions.iter().position(|f| f.defined && *f.name == *s) {
        Some(i) => Ok(How::User(i as u32)),
        None => {
            if s.starts_with('@') {
                Err(it.fatal(format!("`{s}' is invalid as a function name")))
            } else {
                Err(it.fatal(format!("sort comparison function `{s}' is not defined")))
            }
        }
    }
}

fn type_rank(c: &Cell) -> u8 {
    match c {
        Cell::Arr(_) => 2,
        Cell::Val(v) => match v {
            Value::Num(_) | Value::Bool(_) | Value::Uninit => 0,
            Value::StrNum(s) if looks_numeric(s) => 0,
            _ => 1,
        },
        _ => 0,
    }
}

fn cell_value(c: &Cell) -> Value {
    match c {
        Cell::Val(v) => v.clone(),
        _ => Value::Uninit,
    }
}

fn cmp_index_str(a: &Subscript, b: &Subscript) -> Ordering {
    a.text().as_ref().cmp(b.text().as_ref())
}

/// Comparação de textos com o IGNORECASE (o gawk compara sem caixa quando ele está ligado).
fn cmp_text(it: &Interp<'_>, a: &[u8], b: &[u8]) -> Ordering {
    if it.ignorecase {
        crate::builtins::lower_bytes(a).cmp(&crate::builtins::lower_bytes(b))
    } else {
        a.cmp(b)
    }
}

fn cmp_f64(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or_else(|| a.is_nan().cmp(&b.is_nan()))
}

fn compare(it: &Interp<'_>, key: Key, a: &(Subscript, Cell), b: &(Subscript, Cell)) -> Ordering {
    match key {
        Key::IndStr => cmp_text(it, a.0.text(), b.0.text()).then_with(|| cmp_index_str(&a.0, &b.0)),
        Key::IndNum => {
            let x = str_to_num(a.0.text());
            let y = str_to_num(b.0.text());
            cmp_f64(x, y).then_with(|| cmp_index_str(&a.0, &b.0))
        }
        Key::ValType => {
            let (ra, rb) = (type_rank(&a.1), type_rank(&b.1));
            if ra != rb {
                return ra.cmp(&rb);
            }
            match ra {
                0 => cmp_f64(cell_value(&a.1).to_num(), cell_value(&b.1).to_num()).then_with(|| cmp_index_str(&a.0, &b.0)),
                1 => {
                    let sa = it.to_str(&cell_value(&a.1));
                    let sb = it.to_str(&cell_value(&b.1));
                    cmp_text(it, &sa, &sb).then_with(|| cmp_index_str(&a.0, &b.0))
                }
                _ => cmp_index_str(&a.0, &b.0),
            }
        }
        Key::ValStr => {
            let aa = matches!(a.1, Cell::Arr(_));
            let ba = matches!(b.1, Cell::Arr(_));
            if aa || ba {
                return aa.cmp(&ba).then_with(|| cmp_index_str(&a.0, &b.0));
            }
            let sa = it.to_str(&cell_value(&a.1));
            let sb = it.to_str(&cell_value(&b.1));
            cmp_text(it, &sa, &sb).then_with(|| cmp_index_str(&a.0, &b.0))
        }
        Key::ValNum => {
            let aa = matches!(a.1, Cell::Arr(_));
            let ba = matches!(b.1, Cell::Arr(_));
            if aa || ba {
                return aa.cmp(&ba).then_with(|| cmp_index_str(&a.0, &b.0));
            }
            let va = cell_value(&a.1);
            let vb = cell_value(&b.1);
            cmp_f64(va.to_num(), vb.to_num())
                .then_with(|| cmp_text(it, &it.to_str(&va), &it.to_str(&vb)))
                .then_with(|| cmp_index_str(&a.0, &b.0))
        }
    }
}

/// Ordena pares (índice, valor) pela ordem pedida.
fn sort_pairs(it: &mut Interp<'_>, pairs: &mut Vec<(Subscript, Cell)>, how: &How) -> R<()> {
    match how {
        How::Builtin(key, desc) => {
            let key = *key;
            let desc = *desc;
            pairs.sort_by(|a, b| {
                let o = compare(it, key, a, b);
                if desc { o.reverse() } else { o }
            });
            Ok(())
        }
        How::User(f) => {
            // Ordenação por intercalação com a função do usuário (que pode falhar no meio).
            let f = *f;
            let mut err: Option<Flow> = None;
            let mut v = std::mem::take(pairs);
            merge_sort(&mut v, &mut |a, b| {
                if err.is_some() {
                    return Ordering::Equal;
                }
                match user_cmp(it, f, a, b) {
                    Ok(o) => o,
                    Err(e) => {
                        err = Some(e);
                        Ordering::Equal
                    }
                }
            });
            *pairs = v;
            match err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
    }
}

fn user_cmp(it: &mut Interp<'_>, f: u32, a: &(Subscript, Cell), b: &(Subscript, Cell)) -> R<Ordering> {
    let nparams = it.p.functions[f as usize].params.len();
    let mut cells = vec![
        Cell::Val(Value::Str(a.0.text().clone())),
        a.1.clone(),
        Cell::Val(Value::Str(b.0.text().clone())),
        b.1.clone(),
    ];
    cells.truncate(nparams);
    while cells.len() < nparams {
        cells.push(Cell::Uninit);
    }
    let r = it.invoke(f, cells)?;
    let n = r.to_num();
    Ok(if n < 0.0 {
        Ordering::Less
    } else if n > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    })
}

fn merge_sort<T: Clone>(v: &mut Vec<T>, cmp: &mut dyn FnMut(&T, &T) -> Ordering) {
    if v.len() <= 1 {
        return;
    }
    let mid = v.len() / 2;
    let mut right = v.split_off(mid);
    merge_sort(v, cmp);
    merge_sort(&mut right, cmp);
    let left = std::mem::take(v);
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if cmp(&right[j], &left[i]) == Ordering::Less {
            out.push(right[j].clone());
            j += 1;
        } else {
            out.push(left[i].clone());
            i += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    *v = out;
}

fn pairs_of(a: &ArrRef) -> Vec<(Subscript, Cell)> {
    let b = a.borrow();
    b.keys().into_iter().map(|k| {
        let c = b.get(&k).cloned().unwrap_or(Cell::Uninit);
        (k, c)
    }).collect()
}

/// Chaves na ordem de `sorted_in`.
pub fn sorted_keys(it: &mut Interp<'_>, a: &ArrRef, how: &[u8]) -> R<Vec<Subscript>> {
    let how = parse_how(it, how)?;
    let mut pairs = pairs_of(a);
    sort_pairs(it, &mut pairs, &how)?;
    Ok(pairs.into_iter().map(|p| p.0).collect())
}

fn deep_copy(c: &Cell) -> Cell {
    match c {
        Cell::Arr(a) => {
            let src = a.borrow();
            let mut dst = Array::new();
            for k in src.keys() {
                if let Some(v) = src.get(&k) {
                    dst.insert(k, deep_copy(v));
                }
            }
            Cell::Arr(Rc::new(RefCell::new(dst)))
        }
        other => other.clone(),
    }
}

/// `asort(src [, dest [, how]])` e `asorti`.
pub fn asort(it: &mut Interp<'_>, args: &[Expr], indices: bool) -> R<Value> {
    let fname = if indices { "asorti" } else { "asort" };
    let src = match &args[0] {
        Expr::Var(v) => it.get_array(*v)?,
        Expr::Index(v, g) => it.array_at(*v, g)?,
        _ => return Err(it.fatal(format!("{fname}: first argument not an array"))),
    };
    let dest = match args.get(1) {
        Some(Expr::Var(v)) => Some(it.get_array(*v)?),
        Some(Expr::Index(v, g)) => Some(it.array_at(*v, g)?),
        Some(_) => return Err(it.fatal(format!("{fname}: second argument not an array"))),
        None => None,
    };
    let how = match args.get(2) {
        Some(e) => {
            let s = it.eval_str(e)?;
            s.to_vec()
        }
        None => if indices { b"@ind_str_asc".to_vec() } else { b"@val_type_asc".to_vec() },
    };
    let how = parse_how(it, &how)?;
    let mut pairs = pairs_of(&src);
    sort_pairs(it, &mut pairs, &how)?;
    let n = pairs.len();
    let target = dest.unwrap_or_else(|| src.clone());
    let mut out = Array::new();
    for (i, (k, c)) in pairs.into_iter().enumerate() {
        let v = if indices { Cell::Val(Value::Str(k.text().clone())) } else { deep_copy(&c) };
        out.insert(Subscript::from_int(i as i64 + 1), v);
    }
    *target.borrow_mut() = out;
    Ok(Value::Num(n as f64))
}
