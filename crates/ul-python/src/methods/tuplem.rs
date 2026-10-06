//! Métodos de `tuple` (`Objects/tupleobject.c`).

use std::rc::Rc;

use crate::native_util::want_int;
use crate::object::{is, py_eq, Kw, NativeFnPtr, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn this(args: &[Value]) -> PyResult<Rc<[Value]>> {
    match args.first() {
        Some(Value::Tuple(t)) => Ok(t.clone()),
        _ => Err(type_error("descriptor requires a 'tuple' object")),
    }
}

fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("tuple.{fname}() takes no keyword arguments")))
    }
}

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

fn index(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("index", &kw)?;
    let n = args.len() - 1;
    if !(1..=3).contains(&n) {
        let msg = if n == 0 {
            "tuple.index() takes at least 1 argument (0 given)".to_string()
        } else {
            format!("tuple.index() takes at most 3 arguments ({n} given)")
        };
        return Err(type_error(msg));
    }
    let t = this(&args)?;
    let start = norm_bound(args.get(2), t.len(), 0)?;
    let stop = norm_bound(args.get(3), t.len(), t.len())?;
    let mut i = start;
    while i < stop {
        if is(&t[i], &args[1]) || py_eq(&t[i], &args[1]) {
            return Ok(Value::Int(i as i64));
        }
        i += 1;
    }
    Err(exc("ValueError", "tuple.index(x): x not in tuple"))
}

fn count(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("count", &kw)?;
    if args.len() != 2 {
        return Err(type_error(format!("tuple.count() takes exactly one argument ({} given)", args.len() - 1)));
    }
    let t = this(&args)?;
    let n = t.iter().filter(|x| is(x, &args[1]) || py_eq(x, &args[1])).count();
    Ok(Value::Int(n as i64))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[("index", index), ("count", count)];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> (String, String, i32) {
        let o = crate::run_source(src);
        (String::from_utf8(o.stdout).unwrap(), o.stderr, o.status)
    }

    #[test]
    fn index_and_count() {
        let (out, _, st) = run("t = (1, 2, 1, 3)\nprint(t.index(1), t.index(1, 1), t.index(3), t.count(1), t.count(7))\n");
        assert_eq!(st, 0);
        assert_eq!(out, "0 2 3 2 0\n");
    }

    #[test]
    fn index_missing() {
        let (_, err, st) = run("t = (1,)\nt.index(5)\n");
        assert_ne!(st, 0);
        assert!(err.contains("ValueError: tuple.index(x): x not in tuple"), "{err}");
    }
}
