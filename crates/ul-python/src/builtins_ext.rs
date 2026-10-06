//! Funções embutidas que dependem da VM inteira: `setattr`, `delattr`, `slice`, `vars`, `dir`,
//! `globals`, `format`, `input`, `exit`, `quit`, `__import__`, `eval` e `exec`.

use std::rc::Rc;

use sysabi::{sys, Fd};

use crate::native_util::{bind, want_str};
use crate::object::{Dict, Kw, NativeFnPtr, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("setattr", b_setattr),
    ("delattr", b_delattr),
    ("slice", b_slice),
    ("vars", b_vars),
    ("dir", b_dir),
    ("globals", b_globals),
    ("format", b_format),
    ("input", b_input),
    ("exit", b_exit),
    ("quit", b_exit),
    ("__import__", b_import),
    ("eval", b_eval),
    ("exec", b_exec),
];

fn attr_name(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        other => Err(type_error(format!("attribute name must be string, not '{}'", other.type_name()))),
    }
}

fn b_setattr(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("setattr", args, kw, &["obj", "name", "value"], 3)?;
    let (obj, name, value) = (a[0].clone().unwrap_or(Value::None), a[1].clone().unwrap_or(Value::None), a[2].clone().unwrap_or(Value::None));
    vm.store_attr(&obj, &attr_name(&name)?, value)?;
    Ok(Value::None)
}

fn b_delattr(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("delattr", args, kw, &["obj", "name"], 2)?;
    let (obj, name) = (a[0].clone().unwrap_or(Value::None), a[1].clone().unwrap_or(Value::None));
    vm.delete_attr(&obj, &attr_name(&name)?)?;
    Ok(Value::None)
}

fn b_slice(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("slice", &kw)?;
    let (lo, hi, step) = match args.as_slice() {
        [stop] => (Value::None, stop.clone(), Value::None),
        [start, stop] => (start.clone(), stop.clone(), Value::None),
        [start, stop, step] => (start.clone(), stop.clone(), step.clone()),
        other => {
            return Err(type_error(format!(
                "slice expected at most 3 arguments, got {}",
                other.len()
            )))
        }
    };
    Ok(Value::Slice(Rc::new((lo, hi, step))))
}

fn b_vars(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("vars", &kw)?;
    match args.first() {
        None => b_globals(vm, Vec::new(), Vec::new()),
        Some(obj) => vm
            .getattr(obj, "__dict__")
            .map_err(|_| type_error("vars() argument must have __dict__ attribute")),
    }
}

fn b_globals(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let mut d = Dict::new();
    let mut items: Vec<(String, Value)> = vm.globals.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in items {
        d.set(Value::str(k), v)?;
    }
    Ok(Value::dict(d))
}

/// `dir(obj)`: nomes de atributo ordenados (instância, classe e módulo; o resto sai vazio).
fn b_dir(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("dir", &kw)?;
    let mut names: Vec<String> = Vec::new();
    match args.first() {
        None => names.extend(vm.globals.borrow().keys().cloned()),
        Some(Value::Instance(i)) => {
            names.extend(i.dict.borrow().keys().cloned());
            for c in i.class.mro() {
                names.extend(c.dict.borrow().keys().cloned());
            }
        }
        Some(Value::Class(c)) => {
            for k in c.mro() {
                names.extend(k.dict.borrow().keys().cloned());
            }
        }
        Some(Value::Module(m)) => names.extend(m.attrs.borrow().keys().cloned()),
        Some(Value::Ext(e)) => names.extend(e.methods().iter().map(|s| (*s).to_string())),
        Some(_) => {}
    }
    names.sort();
    names.dedup();
    Ok(Value::list(names.into_iter().map(Value::str).collect()))
}

fn b_format(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("format", args, kw, &["value", "format_spec"], 1)?;
    let value = a[0].clone().unwrap_or(Value::None);
    let spec = match &a[1] {
        Some(Value::Str(s)) => s.as_str().to_string(),
        Some(other) => {
            return Err(type_error(format!("format() argument 2 must be str, not {}", other.type_name())))
        }
        None => String::new(),
    };
    Ok(Value::str(vm.format_value(&value, &spec)?))
}

/// `input(prompt='')`: escreve o prompt no stdout e lê uma linha do stdin do pseudo-processo.
fn b_input(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("input", &kw)?;
    if let Some(p) = args.first() {
        let text = crate::object::to_str(p);
        vm.stdout.borrow_mut().extend_from_slice(text.as_bytes());
    }
    let mut line: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match sys::read(Fd::STDIN, &mut byte) {
            Ok(0) => {
                if line.is_empty() {
                    return Err(exc("EOFError", "EOF when reading a line"));
                }
                break;
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                line.push(byte[0]);
            }
            Err(e) => return Err(exc("OSError", format!("[Errno {}] {}", e.0, e.message()))),
        }
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Value::str(String::from_utf8_lossy(&line).into_owned()))
}

fn b_exit(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Err(crate::vm::PyException::from_value(&Value::Exception(Rc::new(crate::object::ExcObj::new("SystemExit", args)))))
}

fn b_import(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("__import__", args, kw, &["name", "globals", "locals", "fromlist", "level"], 1)?;
    let name = want_str("__import__", a[0].as_ref().unwrap_or(&Value::None))?.to_string();
    let top = name.split('.').next().unwrap_or("").to_string();
    let wants_leaf = a[3].as_ref().is_some_and(Value::is_true);
    let target = if wants_leaf { name.clone() } else { top };
    match crate::modules::import(vm, &target) {
        Some(m) => Ok(Value::Module(m)),
        None => Err(exc("ModuleNotFoundError", format!("No module named '{name}'"))),
    }
}

/// Executa `src` como módulo na VM atual.
fn run_text(vm: &mut Vm, src: &str) -> PyResult<()> {
    let mut text = src.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let module = crate::parser::parse_module(&text)
        .map_err(|e| exc("SyntaxError", e.msg))?;
    let code = crate::compile::compile_module(&module).map_err(|e| exc("SyntaxError", e.msg))?;
    vm.run(&Rc::new(code)).map_err(|e| e.exc)
}

fn b_eval(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("eval", args, kw, &["source", "globals", "locals"], 1)?;
    let src = want_str("eval", a[0].as_ref().unwrap_or(&Value::None))?.trim().to_string();
    run_text(vm, &format!("__eval_value__ = ({src})"))?;
    let v = vm.globals.borrow_mut().remove("__eval_value__").unwrap_or(Value::None);
    Ok(v)
}

fn b_exec(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("exec", args, kw, &["source", "globals", "locals"], 1)?;
    let src = want_str("exec", a[0].as_ref().unwrap_or(&Value::None))?.to_string();
    run_text(vm, &src)?;
    Ok(Value::None)
}
