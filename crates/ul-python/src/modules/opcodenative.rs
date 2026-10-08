//! Módulo `_opcode` do CPython 3.13 (`Modules/_opcode.c`): as consultas de metadados que o `opcode.py` e o
//! `dis.py` do Debian fazem sobre a tabela de opcodes. A tabela vive em [`crate::cpyops`].

use std::rc::Rc;

use crate::cpyops;
use crate::modules::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs, value_error, want_int};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{type_error, PyResult, Vm};

/// A marca `flag` do opcode do único argumento; opcode desconhecido não tem marca nenhuma.
fn has_flag(name: &str, args: &[Value], kw: &Kw, flag: u16) -> PyResult<Value> {
    no_kwargs(name, kw)?;
    exactly(name, args, 1)?;
    let code = want_int(&args[0])?;
    Ok(Value::Bool(cpyops::info(code).is_some_and(|o| o.flags & flag != 0)))
}

macro_rules! flag_fn {
    ($f:ident, $py:literal, $flag:expr) => {
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            has_flag($py, &args, &kw, $flag)
        }
    };
}

flag_fn!(has_arg, "has_arg", cpyops::ARG);
flag_fn!(has_const, "has_const", cpyops::CONST);
flag_fn!(has_name, "has_name", cpyops::NAME);
flag_fn!(has_jump, "has_jump", cpyops::JUMP);
flag_fn!(has_free, "has_free", cpyops::FREE);
flag_fn!(has_local, "has_local", cpyops::LOCAL);
flag_fn!(has_exc, "has_exc", cpyops::EXC);

fn is_valid(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("is_valid", &kw)?;
    exactly("is_valid", &args, 1)?;
    Ok(Value::Bool(cpyops::info(want_int(&args[0])?).is_some()))
}

/// `stack_effect(opcode, oparg=None, /, *, jump=None)`.
fn stack_effect(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if args.len() > 2 {
        return Err(type_error(format!("stack_effect() takes at most 2 positional arguments ({} given)", args.len())));
    }
    let mut jump_arg = None;
    let mut rest = Vec::new();
    for (k, v) in kw {
        if k == "jump" {
            jump_arg = Some(v);
        } else {
            rest.push((k, v));
        }
    }
    let a = bind("stack_effect", args, rest, &["opcode", "oparg"], 1)?;
    let code = want_int(a[0].as_ref().unwrap_or(&Value::None))?;
    let oparg = match a[1].as_ref() {
        None | Some(Value::None) => None,
        Some(v) => Some(want_int(v)?),
    };
    let jump = match jump_arg {
        None | Some(Value::None) => None,
        Some(Value::Bool(b)) => Some(b),
        Some(_) => return Err(value_error("stack_effect: jump must be False, True or None")),
    };
    let Some(info) = cpyops::info(code) else {
        return Err(value_error("invalid opcode or oparg"));
    };
    let has_oparg = info.flags & cpyops::ARG != 0;
    if oparg.is_none() && has_oparg {
        return Err(value_error("stack_effect: opcode requires oparg but oparg was not specified"));
    }
    if oparg.is_some() && !has_oparg {
        return Err(value_error("stack_effect: opcode does not permit oparg but oparg was specified"));
    }
    match cpyops::stack_effect_any(info.name, oparg.unwrap_or(0), jump) {
        Some(n) => Ok(Value::Int(n)),
        None => Err(value_error("invalid opcode or oparg")),
    }
}

fn names_list(items: &[&str]) -> Value {
    Value::list(items.iter().map(|s| Value::str(*s)).collect())
}

fn intrinsic1_descs(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(names_list(cpyops::INTRINSIC_1))
}

fn intrinsic2_descs(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(names_list(cpyops::INTRINSIC_2))
}

fn nb_ops(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::list(
        cpyops::NB_OPS.iter().map(|(n, s)| Value::tuple(vec![Value::str(*n), Value::str(*s)])).collect(),
    ))
}

/// `get_executor(code, offset)`: o interpretador não tem o otimizador de traços, então nenhum
/// deslocamento tem executor, que é o que o CPython diz de um deslocamento sem `ENTER_EXECUTOR`.
fn get_executor(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("get_executor", &kw)?;
    exactly("get_executor", &args, 2)?;
    want_int(&args[1])?;
    Err(value_error("no executor at given offset"))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_opcode")
        .func("stack_effect", stack_effect)
        .func("is_valid", is_valid)
        .func("has_arg", has_arg)
        .func("has_const", has_const)
        .func("has_name", has_name)
        .func("has_jump", has_jump)
        .func("has_free", has_free)
        .func("has_local", has_local)
        .func("has_exc", has_exc)
        .func("get_intrinsic1_descs", intrinsic1_descs)
        .func("get_intrinsic2_descs", intrinsic2_descs)
        .func("get_nb_ops", nb_ops)
        .func("get_executor", get_executor)
        .value("ENABLE_SPECIALIZATION", Value::Bool(true))
        .build()
}
