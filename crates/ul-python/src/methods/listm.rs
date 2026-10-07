//! Métodos de `list` (`Objects/listobject.c`).
//!
//! A lista vive em `Rc<RefCell<Vec<Value>>>`. Nenhum método segura um `borrow()` enquanto chama
//! código Python (`key=`, comparações): os itens são copiados antes.

use std::cell::RefCell;
use std::rc::Rc;

use crate::native_util::want_int;
use crate::object::{is, py_eq, repr, Kw, NativeFnPtr, Value};
use crate::vm::{exc, iterate, py_lt, type_error, PyResult, Vm};

type ListRef = Rc<RefCell<Vec<Value>>>;

fn this(args: &[Value]) -> PyResult<ListRef> {
    match args.first() {
        Some(Value::List(l)) => Ok(l.clone()),
        _ => Err(type_error("descriptor requires a 'list' object")),
    }
}

fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("list.{fname}() takes no keyword arguments")))
    }
}

/// Confere a quantidade de argumentos (sem o receptor) com as mensagens do CPython.
fn argc(fname: &str, rest: &[Value], min: usize, max: usize) -> PyResult<()> {
    let n = rest.len();
    if n >= min && n <= max {
        return Ok(());
    }
    let plural = |k: usize| if k == 1 { "" } else { "s" };
    let msg = if min == max {
        if min == 1 {
            format!("list.{fname}() takes exactly one argument ({n} given)")
        } else {
            format!("list.{fname}() takes exactly {min} argument{} ({n} given)", plural(min))
        }
    } else if n < min {
        format!("list.{fname}() takes at least {min} argument{} ({n} given)", plural(min))
    } else {
        format!("list.{fname}() takes at most {max} argument{} ({n} given)", plural(max))
    };
    Err(type_error(msg))
}

/// Normaliza `start`/`stop` de `index`: negativo soma o tamanho, e tudo é limitado a `0..=len`.
fn norm_bound(v: Option<&Value>, len: usize, default: usize) -> PyResult<usize> {
    match v {
        None | Some(Value::None) => Ok(default),
        Some(x) => {
            let i = want_int(x)?;
            if i < 0 {
                let j = i.saturating_add(len as i64);
                Ok(if j < 0 { 0 } else { j as usize })
            } else {
                Ok(i.min(len as i64) as usize)
            }
        }
    }
}

fn append(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("append", &kw)?;
    argc("append", &args[1..], 1, 1)?;
    this(&args)?.borrow_mut().push(args[1].clone());
    Ok(Value::None)
}

fn extend(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("extend", &kw)?;
    argc("extend", &args[1..], 1, 1)?;
    let l = this(&args)?;
    // `iterate` já copia os itens: `a.extend(a)` não empresta a lista duas vezes.
    let items = iterate(&args[1])?;
    l.borrow_mut().extend(items);
    Ok(Value::None)
}

fn insert(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("insert", &kw)?;
    argc("insert", &args[1..], 2, 2)?;
    let l = this(&args)?;
    let mut idx = want_int(&args[1])?;
    let mut v = l.borrow_mut();
    let len = v.len() as i64;
    if idx < 0 {
        idx = idx.saturating_add(len);
        if idx < 0 {
            idx = 0;
        }
    }
    if idx > len {
        idx = len;
    }
    v.insert(idx as usize, args[2].clone());
    Ok(Value::None)
}

fn remove(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("remove", &kw)?;
    argc("remove", &args[1..], 1, 1)?;
    let l = this(&args)?;
    let pos = l.borrow().iter().position(|x| is(x, &args[1]) || py_eq(x, &args[1]));
    match pos {
        Some(i) => {
            l.borrow_mut().remove(i);
            Ok(Value::None)
        }
        None => Err(exc("ValueError", "list.remove(x): x not in list")),
    }
}

fn pop(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("pop", &kw)?;
    argc("pop", &args[1..], 0, 1)?;
    let l = this(&args)?;
    let n = l.borrow().len() as i64;
    if n == 0 {
        return Err(exc("IndexError", "pop from empty list"));
    }
    let i = if args.len() == 2 { want_int(&args[1])? } else { -1 };
    let j = if i < 0 { i.saturating_add(n) } else { i };
    if j < 0 || j >= n {
        return Err(exc("IndexError", "pop index out of range"));
    }
    let v = l.borrow_mut().remove(j as usize);
    Ok(v)
}

fn clear(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("clear", &kw)?;
    argc("clear", &args[1..], 0, 0)?;
    this(&args)?.borrow_mut().clear();
    Ok(Value::None)
}

fn copy(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("copy", &kw)?;
    argc("copy", &args[1..], 0, 0)?;
    let items = this(&args)?.borrow().clone();
    Ok(Value::list(items))
}

fn reverse(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("reverse", &kw)?;
    argc("reverse", &args[1..], 0, 0)?;
    this(&args)?.borrow_mut().reverse();
    Ok(Value::None)
}

fn index(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("index", &kw)?;
    argc("index", &args[1..], 1, 3)?;
    let items = this(&args)?.borrow().clone();
    let len = items.len();
    let start = norm_bound(args.get(2), len, 0)?;
    let stop = norm_bound(args.get(3), len, len)?;
    let mut i = start;
    while i < stop && i < items.len() {
        if is(&items[i], &args[1]) || py_eq(&items[i], &args[1]) {
            return Ok(Value::Int(i as i64));
        }
        i += 1;
    }
    Err(exc("ValueError", format!("{} is not in list", repr(&args[1]))))
}

fn count(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("count", &kw)?;
    argc("count", &args[1..], 1, 1)?;
    let n = this(&args)?.borrow().iter().filter(|x| is(x, &args[1]) || py_eq(x, &args[1])).count();
    Ok(Value::Int(n as i64))
}

type Pair = (Value, Value);

/// Merge sort estável sobre pares (chave, item). `b < a` só leva o da direita à frente quando é
/// estritamente menor, o que mantém a ordem original dos iguais. O erro de comparação propaga.
fn merge_sort(mut v: Vec<Pair>) -> PyResult<Vec<Pair>> {
    if v.len() <= 1 {
        return Ok(v);
    }
    let right = v.split_off(v.len() / 2);
    let left = merge_sort(v)?;
    let right = merge_sort(right)?;
    let mut out: Vec<Pair> = Vec::with_capacity(left.len() + right.len());
    let mut l = left.into_iter().peekable();
    let mut r = right.into_iter().peekable();
    loop {
        let take_right = match (l.peek(), r.peek()) {
            (Some(a), Some(b)) => py_lt(&b.0, &a.0)?,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => break,
        };
        let next = if take_right { r.next() } else { l.next() };
        if let Some(p) = next {
            out.push(p);
        }
    }
    Ok(out)
}

fn sort(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if args.len() > 1 {
        return Err(type_error("sort() takes no positional arguments"));
    }
    let l = this(&args)?;
    let mut key = Value::None;
    let mut rev = false;
    for (k, v) in kw {
        match k.as_str() {
            "key" => key = v,
            "reverse" => rev = v.is_true(),
            other => return Err(type_error(format!("sort() got an unexpected keyword argument '{other}'"))),
        }
    }
    let items = l.borrow().clone();
    let mut pairs: Vec<Pair> = Vec::with_capacity(items.len());
    for it in items {
        let k = if matches!(key, Value::None) { it.clone() } else { vm.call(&key, vec![it.clone()], Vec::new())? };
        pairs.push((k, it));
    }
    if rev {
        pairs.reverse();
    }
    let mut sorted = merge_sort(pairs)?;
    if rev {
        sorted.reverse();
    }
    *l.borrow_mut() = sorted.into_iter().map(|(_, it)| it).collect();
    Ok(Value::None)
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("append", append),
    ("extend", extend),
    ("insert", insert),
    ("remove", remove),
    ("pop", pop),
    ("clear", clear),
    ("index", index),
    ("count", count),
    ("sort", sort),
    ("reverse", reverse),
    ("copy", copy),
];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> (String, String, i32) {
        let o = crate::run_source(src);
        (String::from_utf8(o.stdout).unwrap(), o.stderr, o.status)
    }

    #[test]
    fn append_extend_insert() {
        let (out, _, st) = run("a = [1, 2]\na.append(3)\na.extend([4, 5])\na.insert(0, 0)\nprint(a)\na.insert(-1, 9)\nprint(a)\na.insert(100, 7)\nprint(a)\n");
        assert_eq!(st, 0);
        assert_eq!(out, "[0, 1, 2, 3, 4, 5]\n[0, 1, 2, 3, 4, 9, 5]\n[0, 1, 2, 3, 4, 9, 5, 7]\n");
    }

    #[test]
    fn extend_self() {
        let (out, _, _) = run("a = [1, 2]\na.extend(a)\nprint(a)\n");
        assert_eq!(out, "[1, 2, 1, 2]\n");
    }

    #[test]
    fn pop_remove_clear() {
        let (out, _, _) = run("a = [1, 2, 3, 2]\nprint(a.pop())\nprint(a.pop(0))\na.remove(2)\nprint(a)\na.clear()\nprint(a)\n");
        assert_eq!(out, "2\n1\n[3]\n[]\n");
    }

    #[test]
    fn pop_errors() {
        let (_, err, st) = run("a = []\na.pop()\n");
        assert_ne!(st, 0);
        assert!(err.contains("IndexError: pop from empty list"), "{err}");
        let (_, err, _) = run("a = [1]\na.pop(5)\n");
        assert!(err.contains("IndexError: pop index out of range"), "{err}");
    }

    #[test]
    fn remove_missing() {
        let (_, err, st) = run("a = [1]\na.remove(2)\n");
        assert_ne!(st, 0);
        assert!(err.contains("ValueError: list.remove(x): x not in list"), "{err}");
    }

    #[test]
    fn index_count_copy_reverse() {
        let (out, _, _) = run("a = [1, 2, 1, 3]\nprint(a.index(1), a.index(1, 1), a.count(1), a.count(9))\nb = a.copy()\nb.reverse()\nprint(a)\nprint(b)\n");
        assert_eq!(out, "0 2 2 0\n[1, 2, 1, 3]\n[3, 1, 2, 1]\n");
        let (_, err, _) = run("a = [1]\na.index(5)\n");
        assert!(err.contains("ValueError: 5 is not in list"), "{err}");
    }

    #[test]
    fn sort_plain_key_reverse() {
        let (out, _, st) = run("def neg(x):\n    return -x\na = [3, 1, 2]\na.sort()\nprint(a)\na.sort(key=neg)\nprint(a)\na.sort(reverse=True)\nprint(a)\nprint(a.sort())\n");
        assert_eq!(st, 0);
        assert_eq!(out, "[1, 2, 3]\n[3, 2, 1]\n[3, 2, 1]\nNone\n");
    }

    #[test]
    fn sort_is_stable() {
        let (out, _, _) = run("def first(t):\n    return t[0]\na = [(1, 'b'), (0, 'z'), (1, 'a')]\na.sort(key=first)\nprint(a)\na.sort(key=first, reverse=True)\nprint(a)\n");
        assert_eq!(out, "[(0, 'z'), (1, 'b'), (1, 'a')]\n[(1, 'b'), (1, 'a'), (0, 'z')]\n");
    }

    #[test]
    fn sort_error() {
        let (_, err, st) = run("a = [1, 'a']\na.sort()\n");
        assert_ne!(st, 0);
        assert!(err.contains("TypeError: '<' not supported between instances of 'str' and 'int'"), "{err}");
    }
}
