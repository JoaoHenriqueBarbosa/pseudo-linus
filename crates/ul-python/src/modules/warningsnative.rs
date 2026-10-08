//! Módulo `_warnings` do CPython 3.13 (`Python/_warnings.c`): `warn`, `warn_explicit`, `filters`,
//! `_defaultaction`, `_onceregistry` e `_filters_mutated`.
//!
//! O `warnings.py` do disco faz `from _warnings import ...` e fica com estas funções no lugar das
//! suas versões em Python, como no CPython: o quadro de `warn` não existe (a função é nativa), então
//! o `stacklevel` conta a partir de quem chamou. A mensagem sai por `warnings._showwarnmsg`, que
//! continua em Python.

use std::cell::RefCell;
use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, want_int};
use crate::object::{Dict, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// O `WarningsState` do CPython: as listas que valem enquanto o módulo `warnings` não as substitui.
struct State {
    filters: Value,
    default_action: Value,
    once_registry: Value,
    version: i64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("_warnings ainda não foi construído")))
}

fn as_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Chama a função embutida `name` (`str`, `isinstance`...) da tabela de `builtins`.
fn call_builtin(vm: &mut Vm, name: &'static str, args: Vec<Value>) -> PyResult<Value> {
    let (_, f) = crate::builtins::TABLE.iter().find(|(n, _)| *n == name).expect("função embutida");
    f(vm, args, Vec::new())
}

/// `str(v)` ou `repr(v)` (`builtin`) como texto.
fn text_of(vm: &mut Vm, builtin: &'static str, v: &Value) -> PyResult<String> {
    Ok(as_str(&call_builtin(vm, builtin, vec![v.clone()])?).unwrap_or_default().to_string())
}

fn truthy(vm: &mut Vm, builtin: &'static str, args: Vec<Value>) -> PyResult<bool> {
    Ok(call_builtin(vm, builtin, args)?.is_true())
}

/// `get_warnings_attr`: o atributo do módulo `warnings`, se ele já foi importado (ou se `try_import`).
fn warnings_attr(vm: &mut Vm, name: &str, try_import: bool) -> Option<Value> {
    if try_import && !vm.modules.borrow().contains_key("warnings") {
        let _ = crate::modules::import_value(vm, "warnings");
    }
    let module = vm.modules.borrow().get("warnings").cloned()?;
    vm.load_attr(&Value::Module(module), name).ok()
}

/// `check_matched`: `None` casa tudo, um `str` casa por igualdade (os filtros padrão), o resto é
/// um regex e tem `match`.
fn check_matched(vm: &mut Vm, obj: &Value, arg: &Value) -> PyResult<bool> {
    match obj {
        Value::None => Ok(true),
        Value::Str(s) => match arg {
            Value::Str(a) => Ok(s.as_str() == a.as_str()),
            other => Err(type_error(format!("Can't compare str and {}", other.type_name()))),
        },
        _ => {
            let matcher = vm.load_attr(obj, "match")?;
            Ok(vm.call(&matcher, vec![arg.clone()], Vec::new())?.is_true())
        }
    }
}

/// `get_filter`: a ação e o filtro que casam, ou a ação padrão com `None` no lugar do filtro.
fn get_filter(vm: &mut Vm, category: &Value, text: &Value, lineno: i64, module: &Value) -> PyResult<(Value, Value)> {
    if let Some(f) = warnings_attr(vm, "filters", false) {
        with_state(|s| s.filters = f);
    }
    let filters = with_state(|s| s.filters.clone());
    let Value::List(list) = &filters else {
        return Err(exc("ValueError", "_warnings.filters must be a list"));
    };
    let mut i = 0;
    loop {
        // A lista pode mudar durante a busca (`match` de um regex roda código do programa).
        let Some(item) = list.borrow().get(i).cloned() else { break };
        let Value::Tuple(t) = &item else {
            return Err(exc("ValueError", format!("_warnings.filters item {i} isn't a 5-tuple")));
        };
        if t.len() != 5 {
            return Err(exc("ValueError", format!("_warnings.filters item {i} isn't a 5-tuple")));
        }
        if as_str(&t[0]).is_none() {
            return Err(type_error(format!("action must be a string, not '{}'", t[0].type_name())));
        }
        let good_msg = check_matched(vm, &t[1], text)?;
        let good_mod = check_matched(vm, &t[3], module)?;
        let is_subclass = truthy(vm, "issubclass", vec![category.clone(), t[2].clone()])?;
        let ln = want_int(&t[4])?;
        if good_msg && is_subclass && good_mod && (ln == 0 || lineno == ln) {
            return Ok((t[0].clone(), item.clone()));
        }
        i += 1;
    }
    if let Some(a) = warnings_attr(vm, "defaultaction", false) {
        with_state(|s| s.default_action = a);
    }
    let action = with_state(|s| s.default_action.clone());
    if as_str(&action).is_none() {
        return Err(type_error(format!("_warnings.defaultaction must be a string, not '{}'", action.type_name())));
    }
    Ok((action, Value::None))
}

fn registry_dict(registry: &Value) -> PyResult<Option<Rc<RefCell<Dict>>>> {
    match registry {
        Value::None => Ok(None),
        Value::Dict(d) => Ok(Some(d.clone())),
        _ => Err(type_error("'registry' must be a dict or None")),
    }
}

/// `already_warned`: confere a versão dos filtros no registro (zera o registro se mudou) e procura a chave.
fn already_warned(registry: &Rc<RefCell<Dict>>, key: &Value, should_set: bool) -> PyResult<bool> {
    let version = with_state(|s| s.version);
    let version_key = Value::str("version");
    let current = registry.borrow().get(&version_key)?;
    if matches!(current, Some(Value::Int(n)) if n == version) {
        let seen = registry.borrow().get(key)?;
        if seen.is_some_and(|v| v.is_true()) {
            return Ok(true);
        }
    } else {
        let mut d = registry.borrow_mut();
        let keys: Vec<Value> = d.keys().cloned().collect();
        for k in keys {
            d.remove(&k)?;
        }
        d.set(version_key, Value::Int(version))?;
    }
    if should_set {
        registry.borrow_mut().set(key.clone(), Value::Bool(true))?;
    }
    Ok(false)
}

/// `update_registry`: marca `(text, category)` (ou `(text, category, 0)` no `module`) e diz se já estava.
fn update_registry(registry: &Rc<RefCell<Dict>>, text: &Value, category: &Value, add_zero: bool) -> PyResult<bool> {
    let mut parts = vec![text.clone(), category.clone()];
    if add_zero {
        parts.push(Value::Int(0));
    }
    already_warned(registry, &Value::tuple(parts), true)
}

fn once_registry(vm: &mut Vm) -> PyResult<Rc<RefCell<Dict>>> {
    if let Some(r) = warnings_attr(vm, "onceregistry", false) {
        with_state(|s| s.once_registry = r);
    }
    match with_state(|s| s.once_registry.clone()) {
        Value::Dict(d) => Ok(d),
        _ => Err(exc("ValueError", "_warnings.onceregistry must be a dict")),
    }
}

/// `show_warning`: o que sai no `sys.stderr` quando o módulo `warnings` não tem `_showwarnmsg`.
fn show_warning_fallback(vm: &mut Vm, filename: &Value, lineno: i64, text: &Value, category: &Value) -> PyResult<()> {
    let name = vm.load_attr(category, "__name__")?;
    let mut out = format!(
        "{}:{}: {}: {}\n",
        text_of(vm, "str", filename)?,
        lineno,
        text_of(vm, "str", &name)?,
        text_of(vm, "str", text)?
    );
    if let (Some(file), true) = (as_str(filename), lineno > 0) {
        if let Some(line) = crate::vm::source_line(file, lineno as usize) {
            out.push_str(&format!("  {}\n", line.trim()));
        }
    }
    let sys = crate::modules::import_value(vm, "sys")?;
    let stderr = vm.load_attr(&sys, "stderr")?;
    let write = vm.load_attr(&stderr, "write")?;
    vm.call(&write, vec![Value::str(out)], Vec::new())?;
    Ok(())
}

/// `call_show_warning`: monta o `warnings.WarningMessage` e o entrega a `warnings._showwarnmsg`.
fn call_show_warning(
    vm: &mut Vm,
    category: &Value,
    text: &Value,
    message: &Value,
    filename: &Value,
    lineno: i64,
    source: &Value,
) -> PyResult<()> {
    let Some(show) = warnings_attr(vm, "_showwarnmsg", true) else {
        return show_warning_fallback(vm, filename, lineno, text, category);
    };
    let Some(cls) = warnings_attr(vm, "WarningMessage", false) else {
        return Err(exc("RuntimeError", "unable to get warnings.WarningMessage"));
    };
    let args = vec![
        message.clone(),
        category.clone(),
        filename.clone(),
        Value::Int(lineno),
        Value::None,
        Value::None,
        source.clone(),
    ];
    let msg = vm.call(&cls, args, Vec::new())?;
    if !truthy(vm, "callable", vec![show.clone()])? {
        return Err(type_error("warnings._showwarnmsg() must be set to a callable"));
    }
    vm.call(&show, vec![msg], Vec::new())?;
    Ok(())
}

/// `warn_explicit` do C: normaliza a mensagem, consulta o registro e os filtros e age.
#[allow(clippy::too_many_arguments)]
fn warn_explicit_core(
    vm: &mut Vm,
    category: Value,
    message: Value,
    filename: &Value,
    lineno: i64,
    module: &Value,
    registry: Option<Rc<RefCell<Dict>>>,
    source: &Value,
) -> PyResult<Value> {
    let (text, message, category) = if truthy(vm, "isinstance", vec![message.clone(), Value::Builtin("Warning")])? {
        let text = Value::str(text_of(vm, "str", &message)?);
        let class = vm.load_attr(&message, "__class__")?;
        (text, message, class)
    } else {
        let built = vm.call(&category, vec![message.clone()], Vec::new())?;
        (message, built, category)
    };
    let key = Value::tuple(vec![text.clone(), category.clone(), Value::Int(lineno)]);
    if let Some(reg) = &registry {
        if already_warned(reg, &key, false)? {
            return Ok(Value::None);
        }
    }
    let (action, item) = get_filter(vm, &category, &text, lineno, module)?;
    let action_name = as_str(&action).unwrap_or_default().to_string();
    match action_name.as_str() {
        "error" => return Err(PyException::from_value(&message)),
        "ignore" => return Ok(Value::None),
        _ => {}
    }
    // Só o `always` deixa de gravar no registro que já passou por aqui.
    let mut already = false;
    if action_name != "always" {
        if let Some(reg) = &registry {
            reg.borrow_mut().set(key, Value::Bool(true))?;
        }
        match action_name.as_str() {
            "once" => {
                let reg = match &registry {
                    Some(r) => r.clone(),
                    None => once_registry(vm)?,
                };
                already = update_registry(&reg, &text, &category, false)?;
            }
            "module" => {
                if let Some(reg) = &registry {
                    already = update_registry(reg, &text, &category, true)?;
                }
            }
            "default" => {}
            _ => {
                let a = text_of(vm, "repr", &action)?;
                let i = text_of(vm, "repr", &item)?;
                return Err(exc("RuntimeError", format!("Unrecognized action ({a}) in warnings.filters:\n {i}")));
            }
        }
    }
    if !already {
        call_show_warning(vm, &category, &text, &message, filename, lineno, source)?;
    }
    Ok(Value::None)
}

fn frame_back(vm: &mut Vm, frame: &Value) -> PyResult<Option<Value>> {
    Ok(Some(vm.load_attr(frame, "f_back")?).filter(|b| !matches!(b, Value::None)))
}

fn frame_filename(vm: &mut Vm, frame: &Value) -> PyResult<String> {
    let code = vm.load_attr(frame, "f_code")?;
    let name = vm.load_attr(&code, "co_filename")?;
    Ok(as_str(&name).unwrap_or_default().to_string())
}

/// Frames da importação interna do CPython (`importlib._bootstrap`), que o `stacklevel` não conta.
fn is_internal_filename(filename: &str) -> bool {
    filename.contains("importlib") && filename.contains("_bootstrap")
}

/// `next_external_frame`: o próximo quadro que não é da importação interna nem de um prefixo a pular.
fn next_external_frame(vm: &mut Vm, frame: &Value, skip: &[String]) -> PyResult<Option<Value>> {
    let mut current = frame_back(vm, frame)?;
    while let Some(f) = current.clone() {
        let name = frame_filename(vm, &f)?;
        if !(is_internal_filename(&name) || skip.iter().any(|p| name.starts_with(p.as_str()))) {
            break;
        }
        current = frame_back(vm, &f)?;
    }
    Ok(current)
}

/// `setup_context`: as globais, o arquivo e a linha do quadro que o `stacklevel` aponta. Sem quadro
/// (pilha curta demais) valem o `sys.__dict__`, `<sys>` e a linha 0.
fn setup_context(vm: &mut Vm, mut stack_level: i64, skip: &[String]) -> PyResult<(Value, Value, i64)> {
    let mut frame = Some(crate::modules::pysys::current_frame(vm)?);
    let internal = match &frame {
        Some(f) => is_internal_filename(&frame_filename(vm, f)?),
        None => false,
    };
    loop {
        stack_level -= 1;
        if stack_level <= 0 {
            break;
        }
        let Some(cur) = frame.take() else { break };
        // Partindo de um quadro interno da importação, nenhum quadro é escondido.
        frame = if internal { frame_back(vm, &cur)? } else { next_external_frame(vm, &cur, skip)? };
    }
    match frame {
        None => {
            let sys = crate::modules::import_value(vm, "sys")?;
            Ok((vm.load_attr(&sys, "__dict__")?, Value::str("<sys>"), 0))
        }
        Some(f) => {
            let globals = vm.load_attr(&f, "f_globals")?;
            let filename = vm.load_attr(&f, "f_code").and_then(|c| vm.load_attr(&c, "co_filename"))?;
            let lineno = want_int(&vm.load_attr(&f, "f_lineno")?)?;
            Ok((globals, filename, lineno))
        }
    }
}

fn category_error(category: &Value) -> PyException {
    type_error(format!("category must be a Warning subclass, not '{}'", category.type_name()))
}

/// `warnings.warn(message, category=None, stacklevel=1, source=None, *, skip_file_prefixes=())`.
fn warn(vm: &mut Vm, args: Vec<Value>, mut kw: Kw) -> PyResult<Value> {
    let skip_arg = kw.iter().position(|(k, _)| k == "skip_file_prefixes").map(|i| kw.remove(i).1);
    if args.len() > 4 {
        return Err(type_error(format!("warn() takes at most 4 positional arguments ({} given)", args.len())));
    }
    let mut a = bind("warn", args, kw, &["message", "category", "stacklevel", "source"], 1)?.into_iter();
    let message = a.next().flatten().unwrap_or(Value::None);
    let category_arg = a.next().flatten().unwrap_or(Value::None);
    let mut stacklevel = match a.next().flatten() {
        Some(v) => want_int(&v)?,
        None => 1,
    };
    let source = a.next().flatten().unwrap_or(Value::None);
    let mut skip: Vec<String> = Vec::new();
    if let Some(prefixes) = skip_arg {
        let Value::Tuple(t) = &prefixes else {
            return Err(type_error(format!(
                "warn() argument 'skip_file_prefixes' must be tuple, not {}",
                prefixes.type_name()
            )));
        };
        for p in t.iter() {
            match as_str(p) {
                Some(s) => skip.push(s.to_string()),
                None => {
                    return Err(type_error(format!(
                        "startswith first arg must be str or a tuple of str, not {}",
                        p.type_name()
                    )))
                }
            }
        }
        if !skip.is_empty() {
            stacklevel = stacklevel.max(2);
        }
    }
    let category = if truthy(vm, "isinstance", vec![message.clone(), Value::Builtin("Warning")])? {
        vm.load_attr(&message, "__class__")?
    } else if matches!(category_arg, Value::None) {
        Value::Builtin("UserWarning")
    } else {
        category_arg
    };
    let is_class = truthy(vm, "isinstance", vec![category.clone(), Value::Builtin("type")])?;
    if !is_class || !truthy(vm, "issubclass", vec![category.clone(), Value::Builtin("Warning")])? {
        return Err(category_error(&category));
    }
    let (globals, filename, lineno) = setup_context(vm, stacklevel, &skip)?;
    let Value::Dict(g) = &globals else {
        return Err(type_error("globals must be a dict"));
    };
    let registry_key = Value::str("__warningregistry__");
    let existing = g.borrow().get(&registry_key)?;
    let registry = match existing {
        Some(Value::Dict(d)) => d,
        Some(_) => return Err(type_error("'registry' must be a dict or None")),
        None => {
            let d = Rc::new(RefCell::new(Dict::default()));
            g.borrow_mut().set(registry_key, Value::Dict(d.clone()))?;
            d
        }
    };
    let name = g.borrow().get(&Value::str("__name__"))?;
    let module = match name {
        Some(v @ (Value::None | Value::Str(_))) => v,
        _ => Value::str("<string>"),
    };
    warn_explicit_core(vm, category, message, &filename, lineno, &module, Some(registry), &source)
}

/// `module` de `warn_explicit` sem o argumento: o nome do arquivo sem o `.py`.
fn normalize_module(filename: &str) -> String {
    if filename.is_empty() {
        return "<unknown>".to_string();
    }
    filename.strip_suffix(".py").unwrap_or(filename).to_string()
}

/// `_warnings.warn_explicit(message, category, filename, lineno, module=None, registry=None,
/// module_globals=None, source=None)`.
fn warn_explicit(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let names = ["message", "category", "filename", "lineno", "module", "registry", "module_globals", "source"];
    let mut a = bind("warn_explicit", args, kw, &names, 4)?.into_iter();
    let mut next = || a.next().flatten().unwrap_or(Value::None);
    let (message, category, filename, lineno) = (next(), next(), next(), next());
    let (module, registry, module_globals, source) = (next(), next(), next(), next());
    let Some(file) = as_str(&filename) else {
        return Err(type_error(format!("warn_explicit() argument 3 must be str, not {}", filename.type_name())));
    };
    let lineno = want_int(&lineno)?;
    if !matches!(module_globals, Value::None | Value::Dict(_)) {
        return Err(type_error(format!("module_globals must be a dict, not '{}'", module_globals.type_name())));
    }
    let registry = registry_dict(&registry)?;
    let module = match module {
        Value::None => Value::str(normalize_module(file)),
        other => other,
    };
    warn_explicit_core(vm, category, message, &filename, lineno, &module, registry, &source)
}

/// `_filters_mutated()`: os registros guardados com a versão antiga deixam de valer.
fn filters_mutated(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("_filters_mutated", &kw)?;
    crate::native_util::exactly("_filters_mutated", &args, 0)?;
    with_state(|s| s.version += 1);
    Ok(Value::None)
}

fn default_filters() -> Value {
    let filter = |category: &'static str, action: &str, module: Option<&str>| {
        Value::tuple(vec![
            Value::str(action),
            Value::None,
            Value::Builtin(category),
            module.map_or(Value::None, Value::str),
            Value::Int(0),
        ])
    };
    Value::list(vec![
        filter("DeprecationWarning", "default", Some("__main__")),
        filter("DeprecationWarning", "ignore", None),
        filter("PendingDeprecationWarning", "ignore", None),
        filter("ImportWarning", "ignore", None),
        filter("ResourceWarning", "ignore", None),
    ])
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    let filters = default_filters();
    let default_action = Value::str("default");
    let once_registry = Value::dict(Dict::default());
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            filters: filters.clone(),
            default_action: default_action.clone(),
            once_registry: once_registry.clone(),
            version: 0,
        })
    });
    ModuleBuilder::new("_warnings")
        .func("warn", warn)
        .func("warn_explicit", warn_explicit)
        .func("_filters_mutated", filters_mutated)
        .value("filters", filters)
        .value("_defaultaction", default_action)
        .value("_onceregistry", once_registry)
        .build()
}
