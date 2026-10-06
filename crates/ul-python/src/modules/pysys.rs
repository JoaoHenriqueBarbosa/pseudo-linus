//! `_sys`: o que o módulo `sys` (em Python embutido) pede à VM: argv, os três fluxos padrão,
//! `exit`, `exc_info` e o limite de recursão. Os valores constantes batem com o `python3` do
//! oráculo Debian 13.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::no_kwargs;
use crate::object::{ExcObj, Kw, ModuleObj, Value};
use crate::vm::{exc, PyException, PyResult, Vm};

fn exit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("exit", &kw)?;
    if args.len() > 1 {
        return Err(crate::vm::type_error(format!("exit expected at most 1 argument, got {}", args.len())));
    }
    Err(PyException::from_value(&Value::Exception(Rc::new(ExcObj::new("SystemExit", args)))))
}

fn exc_info(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match vm.handled_top() {
        Some(v) => {
            let ty = vm.type_of(&v);
            let tb = match &v {
                Value::Exception(e) => e.traceback.borrow().clone().unwrap_or(Value::None),
                Value::Instance(i) => i.dict.borrow().get("__traceback__").cloned().unwrap_or(Value::None),
                _ => Value::None,
            };
            Ok(Value::tuple(vec![ty, v, tb]))
        }
        None => Ok(Value::tuple(vec![Value::None, Value::None, Value::None])),
    }
}

fn getframe(vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let depth = match args.first() {
        Some(v) => crate::native_util::want_int(v)?.max(0) as usize,
        None => 0,
    };
    // Do quadro mais interno para o `<module>`: cada função guarda a linha do seu chamador.
    let script: std::rc::Rc<str> = match vm.argv.first().map(String::as_str) {
        Some(a) if !a.is_empty() && a != "-c" => a.into(),
        _ => "<string>".into(),
    };
    let mut chain: Vec<(usize, String, std::rc::Rc<str>)> = Vec::new();
    let mut line = vm.cur_line.get();
    for (code, caller_line) in vm.frames.borrow().iter().rev() {
        let file: std::rc::Rc<str> = if code.filename.is_empty() { script.clone() } else { code.filename.as_str().into() };
        chain.push((line, code.name.clone(), file));
        line = *caller_line;
    }
    chain.push((line, "<module>".to_string(), script));
    crate::tbobj::frame_at(chain, depth).ok_or_else(|| exc("ValueError", "call stack is not deep enough"))
}

fn getrecursionlimit(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::Int(1000))
}

fn setrecursionlimit(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::None)
}

/// Instantâneo dos módulos carregados (`sys.modules`), com `__main__` montado das globais do script.
fn modules_snapshot(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let mut d = crate::object::Dict::new();
    let mut names: Vec<(String, Rc<ModuleObj>)> =
        vm.modules.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, m) in names {
        d.set(Value::str(name), Value::Module(m))?;
    }
    let main = ModuleObj { name: "__main__", attrs: std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    for (k, v) in vm.globals.borrow().iter() {
        main.attrs.borrow_mut().insert(k.clone(), v.clone());
    }
    d.set(Value::str("__main__"), Value::Module(Rc::new(main)))?;
    Ok(Value::dict(d))
}

pub fn build(vm: &mut Vm) -> Rc<ModuleObj> {
    let argv = vm.argv.iter().map(|a| Value::str(a.clone())).collect();
    ModuleBuilder::new("_sys")
        .value("argv", Value::list(argv))
        .value("stdin", Value::Native(vm.std_files[0].clone()))
        .value("stdout", Value::Native(vm.std_files[1].clone()))
        .value("stderr", Value::Native(vm.std_files[2].clone()))
        .func("exit", exit)
        .func("exc_info", exc_info)
        .func("_getframe", getframe)
        .func("_modules", modules_snapshot)
        .func("getrecursionlimit", getrecursionlimit)
        .func("setrecursionlimit", setrecursionlimit)
        .build()
}
