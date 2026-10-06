//! Métodos de `set` (`Objects/setobject.c`).
//!
//! A ordem de iteração do resultado segue a inserção na tabela (`Set::add`), que é port fiel da do
//! CPython; em operações com vários conjuntos a ordem pode divergir do CPython nos casos em que ele
//! escolhe iterar o menor operando por desempenho.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{Kw, NativeFnPtr, Set, Value};
use crate::vm::{iterate, type_error, PyResult, Vm};

use super::dictm::key_error;

type SetRef = Rc<RefCell<Set>>;

fn this(args: &[Value]) -> PyResult<SetRef> {
    match args.first() {
        Some(Value::Set(s)) => Ok(s.clone()),
        _ => Err(type_error("descriptor requires a 'set' object")),
    }
}

fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("set.{fname}() takes no keyword arguments")))
    }
}

fn argc(fname: &str, rest: &[Value], n: usize) -> PyResult<()> {
    if rest.len() == n {
        return Ok(());
    }
    let msg = if n == 0 {
        format!("set.{fname}() takes no arguments ({} given)", rest.len())
    } else if n == 1 {
        format!("set.{fname}() takes exactly one argument ({} given)", rest.len())
    } else {
        format!("set.{fname}() takes exactly {n} arguments ({} given)", rest.len())
    };
    Err(type_error(msg))
}

/// Itens de qualquer iterável como `Set` (conjunto já existente é só copiado).
fn to_set(v: &Value) -> PyResult<Set> {
    if let Value::Set(s) = v {
        return Ok(s.borrow().clone());
    }
    let mut out = Set::new();
    for it in iterate(v)? {
        out.add(it)?;
    }
    Ok(out)
}

fn snapshot(s: &SetRef) -> Vec<Value> {
    s.borrow().iter().cloned().collect()
}

fn add(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("add", &kw)?;
    argc("add", &args[1..], 1)?;
    this(&args)?.borrow_mut().add(args[1].clone())?;
    Ok(Value::None)
}

fn remove(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("remove", &kw)?;
    argc("remove", &args[1..], 1)?;
    let found = this(&args)?.borrow_mut().discard(&args[1])?;
    if found {
        Ok(Value::None)
    } else {
        Err(key_error(args[1].clone()))
    }
}

fn discard(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("discard", &kw)?;
    argc("discard", &args[1..], 1)?;
    this(&args)?.borrow_mut().discard(&args[1])?;
    Ok(Value::None)
}

fn pop(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("pop", &kw)?;
    argc("pop", &args[1..], 0)?;
    let s = this(&args)?;
    let first = s.borrow().iter().next().cloned();
    match first {
        Some(v) => {
            s.borrow_mut().discard(&v)?;
            Ok(v)
        }
        None => Err(key_error(Value::str("pop from an empty set"))),
    }
}

fn clear(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("clear", &kw)?;
    argc("clear", &args[1..], 0)?;
    *this(&args)?.borrow_mut() = Set::new();
    Ok(Value::None)
}

fn copy(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("copy", &kw)?;
    argc("copy", &args[1..], 0)?;
    let c = this(&args)?.borrow().clone();
    Ok(Value::set(c))
}

/// Acrescenta os itens de `src` a `dst`.
fn add_all(dst: &mut Set, src: &Value) -> PyResult<()> {
    for it in iterate(src)? {
        dst.add(it)?;
    }
    Ok(())
}

fn update(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("update", &kw)?;
    let s = this(&args)?;
    for other in &args[1..] {
        let items = iterate(other)?;
        let mut m = s.borrow_mut();
        for it in items {
            m.add(it)?;
        }
    }
    Ok(Value::None)
}

fn union(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("union", &kw)?;
    let mut out = this(&args)?.borrow().clone();
    for other in &args[1..] {
        add_all(&mut out, other)?;
    }
    Ok(Value::set(out))
}

/// Interseção de `base` com um operando. Com dois conjuntos o CPython itera o menor (o `other`
/// em caso de empate).
fn intersect_one(base: &Set, other: &Value) -> PyResult<Set> {
    let mut out = Set::new();
    if let Value::Set(o) = other {
        let o = o.borrow();
        if o.len() > base.len() {
            for k in base.iter() {
                if o.contains(k)? {
                    out.add(k.clone())?;
                }
            }
        } else {
            for k in o.iter() {
                if base.contains(k)? {
                    out.add(k.clone())?;
                }
            }
        }
        return Ok(out);
    }
    for it in iterate(other)? {
        if base.contains(&it)? {
            out.add(it)?;
        }
    }
    Ok(out)
}

fn intersection_of(args: &[Value]) -> PyResult<Set> {
    let mut cur = this(args)?.borrow().clone();
    for other in &args[1..] {
        cur = intersect_one(&cur, other)?;
    }
    Ok(cur)
}

fn intersection(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("intersection", &kw)?;
    Ok(Value::set(intersection_of(&args)?))
}

fn intersection_update(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("intersection_update", &kw)?;
    let result = intersection_of(&args)?;
    *this(&args)?.borrow_mut() = result;
    Ok(Value::None)
}

fn difference_of(args: &[Value]) -> PyResult<Set> {
    let mut cur = this(args)?.borrow().clone();
    for other in &args[1..] {
        let o = to_set(other)?;
        let mut next = Set::new();
        for k in cur.iter() {
            if !o.contains(k)? {
                next.add(k.clone())?;
            }
        }
        cur = next;
    }
    Ok(cur)
}

fn difference(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("difference", &kw)?;
    Ok(Value::set(difference_of(&args)?))
}

fn difference_update(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("difference_update", &kw)?;
    let result = difference_of(&args)?;
    *this(&args)?.borrow_mut() = result;
    Ok(Value::None)
}

fn symmetric_difference(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("symmetric_difference", &kw)?;
    argc("symmetric_difference", &args[1..], 1)?;
    let mine = snapshot(&this(&args)?);
    let mut out = to_set(&args[1])?;
    for k in mine {
        if out.contains(&k)? {
            out.discard(&k)?;
        } else {
            out.add(k)?;
        }
    }
    Ok(Value::set(out))
}

fn issubset(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("issubset", &kw)?;
    argc("issubset", &args[1..], 1)?;
    let mine = snapshot(&this(&args)?);
    let other = to_set(&args[1])?;
    for k in mine {
        if !other.contains(&k)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn issuperset(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("issuperset", &kw)?;
    argc("issuperset", &args[1..], 1)?;
    let s = this(&args)?;
    for it in iterate(&args[1])? {
        if !s.borrow().contains(&it)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn isdisjoint(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("isdisjoint", &kw)?;
    argc("isdisjoint", &args[1..], 1)?;
    let s = this(&args)?;
    for it in iterate(&args[1])? {
        if s.borrow().contains(&it)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("add", add),
    ("remove", remove),
    ("discard", discard),
    ("pop", pop),
    ("clear", clear),
    ("copy", copy),
    ("update", update),
    ("union", union),
    ("intersection", intersection),
    ("difference", difference),
    ("symmetric_difference", symmetric_difference),
    ("issubset", issubset),
    ("issuperset", issuperset),
    ("isdisjoint", isdisjoint),
    ("intersection_update", intersection_update),
    ("difference_update", difference_update),
];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> (String, String, i32) {
        let o = crate::run_source(src);
        (String::from_utf8(o.stdout).unwrap(), o.stderr, o.status)
    }

    #[test]
    fn add_remove_discard() {
        let (out, _, st) = run("s = {1, 2}\ns.add(3)\ns.add(3)\ns.discard(9)\ns.remove(1)\nprint(sorted(s), len(s))\n");
        assert_eq!(st, 0);
        assert_eq!(out, "[2, 3] 2\n");
    }

    #[test]
    fn remove_missing_is_key_error() {
        let (_, err, st) = run("s = {1}\ns.remove(5)\n");
        assert_ne!(st, 0);
        assert!(err.contains("KeyError: 5"), "{err}");
    }

    #[test]
    fn pop_and_clear() {
        let (out, _, _) = run("s = {7}\nprint(s.pop())\nprint(len(s))\nt = {1, 2}\nt.clear()\nprint(len(t))\n");
        assert_eq!(out, "7\n0\n0\n");
        let (_, err, _) = run("s = set()\ns.pop()\n");
        assert!(err.contains("KeyError: 'pop from an empty set'"), "{err}");
    }

    #[test]
    fn algebra() {
        let (out, _, st) = run("a = {1, 2, 3}\nb = {3, 4}\nprint(sorted(a.union(b)))\nprint(sorted(a.intersection(b)))\nprint(sorted(a.difference(b)))\nprint(sorted(a.symmetric_difference(b)))\nprint(sorted(a.union([9], (8,))))\n");
        assert_eq!(st, 0);
        assert_eq!(out, "[1, 2, 3, 4]\n[3]\n[1, 2]\n[1, 2, 4]\n[1, 2, 3, 8, 9]\n");
    }

    #[test]
    fn predicates() {
        let (out, _, _) = run("a = {1, 2}\nb = {1, 2, 3}\nprint(a.issubset(b), b.issubset(a), b.issuperset(a), a.isdisjoint({5}), a.isdisjoint(b))\n");
        assert_eq!(out, "True False True True False\n");
    }

    #[test]
    fn in_place_updates() {
        let (out, _, _) = run("a = {1, 2, 3}\na.update([4], {5})\nprint(sorted(a))\na.intersection_update({1, 2, 9})\nprint(sorted(a))\na.difference_update([1])\nprint(sorted(a))\nc = a.copy()\nc.add(100)\nprint(len(a), len(c))\n");
        assert_eq!(out, "[1, 2, 3, 4, 5]\n[1, 2]\n[2]\n1 2\n");
    }

    #[test]
    fn unhashable_item() {
        let (_, err, st) = run("s = set()\ns.add([1])\n");
        assert_ne!(st, 0);
        assert!(err.contains("TypeError: unhashable type: 'list'"), "{err}");
    }
}
