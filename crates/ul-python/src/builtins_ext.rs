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
        Some(Value::Module(m)) => {
            names.extend(m.attrs.borrow().keys().cloned());
            if let Some(g) = vm.module_globals.borrow().get(m.name) {
                names.extend(g.borrow().keys().cloned());
            }
        }
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
    let level = match &a[4] {
        Some(v) => crate::native_util::want_int(v)?.max(0) as usize,
        None => 0,
    };
    let wants_leaf = a[3].as_ref().is_some_and(Value::is_true);
    let full = if level > 0 { crate::modules::resolve_relative(vm, &name, level)? } else { name };
    // Importa a cadeia inteira (`a.b.c` carrega `a`, `a.b`, `a.b.c`); sem `fromlist` devolve a raiz.
    let leaf = crate::modules::import_checked(vm, &full)?;
    if wants_leaf || level > 0 {
        return Ok(Value::Module(leaf));
    }
    let top = full.split('.').next().unwrap_or("").to_string();
    Ok(Value::Module(crate::modules::import_checked(vm, &top)?))
}

thread_local! {
    /// `m.__dict__` de cada módulo: o mesmo objeto a cada leitura, para `exec(src, m.__dict__)` poder
    /// rodar nas globais vivas do módulo.
    static MODULE_DICTS: std::cell::RefCell<Vec<(&'static str, Value)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// O dict de `name.__dict__`, atualizado com o conteúdo `fresh` e sempre o mesmo objeto.
pub(crate) fn module_dict_value(name: &'static str, fresh: Dict) -> Value {
    MODULE_DICTS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((_, Value::Dict(d))) = m.iter().find(|(n, _)| *n == name) {
            *d.borrow_mut() = fresh;
            return Value::Dict(d.clone());
        }
        let v = Value::dict(fresh);
        m.push((name, v.clone()));
        v
    })
}

fn module_of_dict(d: &Value) -> Option<&'static str> {
    MODULE_DICTS.with(|m| m.borrow().iter().find(|(_, v)| crate::object::is(v, d)).map(|(n, _)| *n))
}

/// Substitui o conteúdo de `target` pelas globais `map`: nomes existentes na ordem de antes, os
/// novos em ordem alfabética (o mapa de globais não guarda a ordem de inserção).
fn write_back(target: &Value, map: &std::collections::HashMap<String, Value>, was: &[String]) -> PyResult<()> {
    let Value::Dict(d) = target else { return Ok(()) };
    let mut fresh = Dict::new();
    let old: Vec<(Value, Value)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in old {
        match &k {
            Value::Str(name) if was.iter().any(|w| w == name.as_str()) => {
                if let Some(nv) = map.get(name.as_str()) {
                    fresh.set(k.clone(), nv.clone())?;
                }
            }
            _ => fresh.set(k, v)?,
        }
    }
    let mut added: Vec<&String> = map.keys().filter(|k| !k.starts_with("__builtins__") && !fresh.contains(&Value::str((*k).clone())).unwrap_or(false)).collect();
    added.sort();
    for k in added {
        fresh.set(Value::str(k.clone()), map[k].clone())?;
    }
    *d.borrow_mut() = fresh;
    Ok(())
}

/// Roda `src` (`exec`) ou avalia a expressão (`eval`) nos espaços de nomes dados; sem eles, nas
/// globais atuais. `globals`/`locals` são dicts: o conteúdo entra numa tabela de globais, o código
/// roda nela, e o resultado volta para o dict (um dict de módulo roda direto nas globais do módulo).
fn run_ns(vm: &mut Vm, src: &str, globals: Option<Value>, locals: Option<Value>, eval: bool, who: &str) -> PyResult<Value> {
    let mut text = if eval { format!("__eval_value__ = ({})", src.trim()) } else { src.to_string() };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let module = crate::parser::parse_module(&text).map_err(|e| {
        let kind = match e.kind {
            crate::parser::ErrorKind::Syntax => "SyntaxError",
            crate::parser::ErrorKind::Indentation => "IndentationError",
            crate::parser::ErrorKind::Tab => "TabError",
        };
        exc(kind, e.msg)
    })?;
    let code = Rc::new(crate::compile::compile_module(&module).map_err(|e| exc("SyntaxError", e.msg))?);
    let globals = globals.filter(|g| !matches!(g, Value::None));
    let locals = locals.filter(|l| !matches!(l, Value::None));
    let Some(gdict) = globals else {
        vm.run(&code).map_err(|e| e.exc)?;
        let v = if eval { vm.globals.borrow_mut().remove("__eval_value__").unwrap_or(Value::None) } else { Value::None };
        return Ok(v);
    };
    let Value::Dict(g) = &gdict else {
        return Err(type_error(format!("{who}() globals must be a dict, not {}", gdict.type_name())));
    };
    if let Some(l) = &locals {
        if !matches!(l, Value::Dict(_)) {
            return Err(type_error("locals must be a mapping"));
        }
    }
    let live = module_of_dict(&gdict).and_then(|n| vm.module_globals.borrow().get(n).cloned());
    let map: Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>> = match &live {
        Some(m) => m.clone(),
        None => Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
    };
    let mut was: Vec<String> = Vec::new();
    let gitems: Vec<(Value, Value)> = g.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in gitems {
        if let Value::Str(name) = &k {
            was.push(name.as_str().to_string());
            if live.is_none() {
                map.borrow_mut().insert(name.as_str().to_string(), v);
            }
        }
    }
    let separate = locals.as_ref().filter(|l| !crate::object::is(l, &gdict));
    let mut lwas: Vec<String> = Vec::new();
    if let Some(Value::Dict(l)) = separate {
        let litems: Vec<(Value, Value)> = l.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in litems {
            if let Value::Str(name) = &k {
                lwas.push(name.as_str().to_string());
                map.borrow_mut().insert(name.as_str().to_string(), v);
            }
        }
    }
    let mut inner = vm.clone();
    inner.globals = map.clone();
    let result = inner.run(&code).map_err(|e| e.exc);
    let value = if eval { map.borrow_mut().remove("__eval_value__").unwrap_or(Value::None) } else { Value::None };
    let snapshot = map.borrow().clone();
    match separate {
        Some(l) => {
            // Só o que o código criou ou mudou vai para o `locals`; o resto é das globais.
            let initial: Vec<(String, Value)> = was
                .iter()
                .filter_map(|n| g.borrow().get(&Value::str(n.clone())).ok().flatten().map(|v| (n.clone(), v)))
                .collect();
            let mut mine = snapshot.clone();
            mine.retain(|k, v| {
                lwas.contains(k) || !initial.iter().any(|(n, old)| n == k && crate::object::is(old, v)) && k != "__eval_value__"
            });
            write_back(l, &mine, &lwas)?
        }
        None => write_back(&gdict, &snapshot, &was)?,
    }
    result?;
    Ok(value)
}

fn b_eval(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("eval", args, kw, &["source", "globals", "locals"], 1)?;
    let src = want_str("eval", a[0].as_ref().unwrap_or(&Value::None))?.to_string();
    run_ns(vm, &src, a[1].clone(), a[2].clone(), true, "eval")
}

fn b_exec(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("exec", args, kw, &["source", "globals", "locals"], 1)?;
    let src = want_str("exec", a[0].as_ref().unwrap_or(&Value::None))?.to_string();
    run_ns(vm, &src, a[1].clone(), a[2].clone(), false, "exec")?;
    Ok(Value::None)
}
