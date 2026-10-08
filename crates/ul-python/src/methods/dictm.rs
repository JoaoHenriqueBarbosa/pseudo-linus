//! Métodos de `dict` (`Objects/dictobject.c`).
//!
//! `keys`, `values` e `items` devolvem listas por ora (o núcleo não tem as views). Nenhum método
//! segura um `borrow()` do dicionário enquanto chama código Python.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{repr, Dict, ExcObj, Kw, NativeFnPtr, Value};
use crate::vm::{exc, iterate, type_error, PyException, PyResult, Vm};

type DictRef = Rc<RefCell<Dict>>;

fn this(args: &[Value]) -> PyResult<DictRef> {
    match args.first() {
        Some(Value::Dict(d)) => Ok(d.clone()),
        _ => Err(type_error("descriptor requires a 'dict' object")),
    }
}

fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("dict.{fname}() takes no keyword arguments")))
    }
}

/// `dict.<fname>() takes no arguments (N given)`.
fn noargs(fname: &str, rest: &[Value]) -> PyResult<()> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("dict.{fname}() takes no arguments ({} given)", rest.len())))
    }
}

/// Mensagens de `METH_FASTCALL` do CPython: `get expected at least 1 argument, got 0`.
fn fastcall(fname: &str, n: usize, min: usize, max: usize) -> PyResult<()> {
    let plural = |k: usize| if k == 1 { "" } else { "s" };
    if n < min {
        return Err(type_error(format!("{fname} expected at least {min} argument{}, got {n}", plural(min))));
    }
    if n > max {
        return Err(type_error(format!("{fname} expected at most {max} argument{}, got {n}", plural(max))));
    }
    Ok(())
}

/// `KeyError(key)`: a mensagem é o `repr` da chave, e a instância guarda a chave como `args[0]`.
pub(crate) fn key_error(key: Value) -> PyException {
    PyException {
        kind: "KeyError",
        msg: repr(&key),
        value: Some(Value::Exception(Rc::new(ExcObj::new("KeyError", vec![key])))),
        tb: Vec::new(),
    }
}

fn get(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("get", &kw)?;
    fastcall("get", args.len() - 1, 1, 2)?;
    let d = this(&args)?;
    let found = d.borrow().get(&args[1])?;
    Ok(found.unwrap_or_else(|| args.get(2).cloned().unwrap_or(Value::None)))
}

fn setdefault(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("setdefault", &kw)?;
    fastcall("setdefault", args.len() - 1, 1, 2)?;
    let d = this(&args)?;
    let found = d.borrow().get(&args[1])?;
    if let Some(v) = found {
        return Ok(v);
    }
    let default = args.get(2).cloned().unwrap_or(Value::None);
    d.borrow_mut().set(args[1].clone(), default.clone())?;
    Ok(default)
}

fn pop(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("pop", &kw)?;
    fastcall("pop", args.len() - 1, 1, 2)?;
    let d = this(&args)?;
    let removed = d.borrow_mut().remove(&args[1])?;
    match removed {
        Some(v) => Ok(v),
        None => match args.get(2) {
            Some(default) => Ok(default.clone()),
            None => Err(key_error(args[1].clone())),
        },
    }
}

fn popitem(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("popitem", &kw)?;
    noargs("popitem", &args[1..])?;
    let d = this(&args)?;
    let last = d.borrow().iter().last().map(|(k, v)| (k.clone(), v.clone()));
    match last {
        Some((k, v)) => {
            d.borrow_mut().remove(&k)?;
            Ok(Value::tuple(vec![k, v]))
        }
        None => Err(key_error(Value::str("popitem(): dictionary is empty"))),
    }
}

fn update(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if args.len() > 2 {
        return Err(type_error(format!("update expected at most 1 argument, got {}", args.len() - 1)));
    }
    let d = this(&args)?;
    let mut pairs: Vec<(Value, Value)> = Vec::new();
    if let Some(src) = args.get(1) {
        match crate::vm::mapping_pairs(src)? {
            Some(found) => pairs.extend(found),
            None => {
                let other = src;
                for (i, item) in iterate(other)?.into_iter().enumerate() {
                    let parts = iterate(&item).map_err(|_| {
                        type_error(format!("cannot convert dictionary update sequence element #{i} to a sequence"))
                    })?;
                    if parts.len() != 2 {
                        return Err(exc(
                            "ValueError",
                            format!(
                                "dictionary update sequence element #{i} has length {}; 2 is required",
                                parts.len()
                            ),
                        ));
                    }
                    let mut it = parts.into_iter();
                    if let (Some(k), Some(v)) = (it.next(), it.next()) {
                        pairs.push((k, v));
                    }
                }
            }
        }
    }
    for (k, v) in kw {
        pairs.push((Value::str(k), v));
    }
    let mut dm = d.borrow_mut();
    for (k, v) in pairs {
        dm.set(k, v)?;
    }
    Ok(Value::None)
}

fn clear(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("clear", &kw)?;
    noargs("clear", &args[1..])?;
    this(&args)?.borrow_mut().clear();
    Ok(Value::None)
}

fn copy(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("copy", &kw)?;
    noargs("copy", &args[1..])?;
    let c = this(&args)?.borrow().clone();
    Ok(Value::dict(c))
}

fn keys(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("keys", &kw)?;
    noargs("keys", &args[1..])?;
    Ok(crate::dictview::DictView::make(this(&args)?.clone(), crate::dictview::Kind::Keys))
}

fn values(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("values", &kw)?;
    noargs("values", &args[1..])?;
    Ok(crate::dictview::DictView::make(this(&args)?.clone(), crate::dictview::Kind::Values))
}

fn items(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("items", &kw)?;
    noargs("items", &args[1..])?;
    Ok(crate::dictview::DictView::make(this(&args)?.clone(), crate::dictview::Kind::Items))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("get", get),
    ("setdefault", setdefault),
    ("pop", pop),
    ("popitem", popitem),
    ("update", update),
    ("clear", clear),
    ("copy", copy),
    ("keys", keys),
    ("values", values),
    ("items", items),
];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> (String, String, i32) {
        let o = crate::run_source(src);
        (String::from_utf8(o.stdout).unwrap(), o.stderr, o.status)
    }

    #[test]
    fn get_setdefault() {
        let (out, _, st) = run("d = {'a': 1}\nprint(d.get('a'), d.get('b'), d.get('b', 5))\nprint(d.setdefault('b', 2), d.setdefault('b', 9), d.setdefault('c'))\nprint(d)\n");
        assert_eq!(st, 0);
        assert_eq!(out, "1 None 5\n2 2 None\n{'a': 1, 'b': 2, 'c': None}\n");
    }

    #[test]
    fn pop_and_popitem() {
        let (out, _, _) = run("d = {'a': 1, 'b': 2, 'c': 3}\nprint(d.pop('a'), d.pop('z', 0))\nprint(d.popitem())\nprint(d)\n");
        assert_eq!(out, "1 0\n('c', 3)\n{'b': 2}\n");
    }

    #[test]
    fn pop_missing_is_key_error() {
        let (_, err, st) = run("d = {}\nd.pop('x')\n");
        assert_ne!(st, 0);
        assert!(err.contains("KeyError: 'x'"), "{err}");
        let (_, err, _) = run("d = {}\nd.popitem()\n");
        assert!(err.contains("KeyError: 'popitem(): dictionary is empty'"), "{err}");
    }

    #[test]
    fn update_forms() {
        let (out, _, st) = run("d = {'a': 1}\nd.update({'b': 2})\nd.update([('c', 3), ['d', 4]])\nd.update(e=5, a=0)\nprint(d)\n");
        assert_eq!(st, 0);
        assert_eq!(out, "{'a': 0, 'b': 2, 'c': 3, 'd': 4, 'e': 5}\n");
    }

    #[test]
    fn update_bad_sequence() {
        let (_, err, st) = run("d = {}\nd.update([(1, 2, 3)])\n");
        assert_ne!(st, 0);
        assert!(err.contains("ValueError: dictionary update sequence element #0 has length 3; 2 is required"), "{err}");
    }

    #[test]
    fn clear_copy_views() {
        let (out, _, _) = run("d = {'a': 1, 'b': 2}\nc = d.copy()\nd.clear()\nprint(d, c)\nprint(c.keys(), c.values(), c.items())\n");
        assert_eq!(
            out,
            "{} {'a': 1, 'b': 2}\ndict_keys(['a', 'b']) dict_values([1, 2]) dict_items([('a', 1), ('b', 2)])\n"
        );
    }

    #[test]
    fn unhashable_key() {
        let (_, err, st) = run("d = {}\nd.setdefault([1], 2)\n");
        assert_ne!(st, 0);
        assert!(err.contains("TypeError: unhashable type: 'list'"), "{err}");
    }
}
