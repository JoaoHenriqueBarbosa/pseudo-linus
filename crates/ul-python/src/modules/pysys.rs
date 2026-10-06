//! `_sys`: o que o módulo `sys` (em Python embutido) pede à VM: argv, os três fluxos padrão,
//! `exit`, `exc_info` e o limite de recursão. Os valores constantes batem com o `python3` do
//! oráculo Debian 13.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::no_kwargs;
use crate::object::{ExcObj, Kw, ModuleObj, Value};
use crate::vm::{PyException, PyResult, Vm};

fn exit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("exit", &kw)?;
    if args.len() > 1 {
        return Err(crate::vm::type_error(format!("exit expected at most 1 argument, got {}", args.len())));
    }
    Err(PyException::from_value(&Value::Exception(Rc::new(ExcObj { kind: "SystemExit", args }))))
}

fn exc_info(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    match vm.handled_top() {
        Some(v) => {
            let ty = vm.type_of(&v);
            Ok(Value::tuple(vec![ty, v, Value::None]))
        }
        None => Ok(Value::tuple(vec![Value::None, Value::None, Value::None])),
    }
}

fn getrecursionlimit(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::Int(1000))
}

fn setrecursionlimit(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::None)
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
        .func("getrecursionlimit", getrecursionlimit)
        .func("setrecursionlimit", setrecursionlimit)
        .build()
}
