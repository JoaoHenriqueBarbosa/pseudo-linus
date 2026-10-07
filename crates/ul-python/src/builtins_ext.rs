//! Funções embutidas que dependem da VM inteira: `setattr`, `delattr`, `slice`, `vars`, `dir`,
//! `globals`, `format`, `input`, `exit`, `quit`, `__import__`, `eval` e `exec`.

use std::rc::Rc;


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
    // Fora de função `locals()` é o mesmo dicionário das globais; dentro, o compilador emite `Op::Locals`.
    ("locals", b_locals),
    ("format", b_format),
    ("input", b_input),
    ("exit", b_exit),
    ("quit", b_exit),
    ("help", b_help),
    ("__import__", b_import),
    ("eval", b_eval),
    ("exec", b_exec),
    ("compile", b_compile),
    ("aiter", b_aiter),
    ("anext", b_anext),
    ("breakpoint", b_breakpoint),
];

/// `aiter(obj)`: o `__aiter__` do objeto, que precisa devolver um iterador assíncrono.
fn b_aiter(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("aiter", &kw)?;
    if args.len() != 1 {
        return Err(type_error(format!("aiter() takes exactly one argument ({} given)", args.len())));
    }
    let obj = &args[0];
    let Ok(method) = vm.getattr(obj, "__aiter__") else {
        return Err(type_error(format!("'{}' object is not an async iterable", obj.type_name())));
    };
    let it = vm.call_value(&method, Vec::new(), Vec::new())?;
    if vm.getattr(&it, "__anext__").is_err() {
        return Err(type_error(format!("aiter() returned not an async iterator of type '{}'", it.type_name())));
    }
    Ok(it)
}

/// `anext(iterador[, padrão])`: o aguardável do `__anext__`; com padrão, um `anext_awaitable`
/// que troca o `StopAsyncIteration` pelo padrão.
fn b_anext(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("anext", &kw)?;
    if args.is_empty() || args.len() > 2 {
        return Err(type_error(format!("anext expected at least 1 argument, got {}", args.len())));
    }
    let it = &args[0];
    let Ok(method) = vm.getattr(it, "__anext__") else {
        return Err(type_error(format!("'{}' object is not an async iterator", it.type_name())));
    };
    let awaitable = vm.call_value(&method, Vec::new(), Vec::new())?;
    let Some(default) = args.get(1) else { return Ok(awaitable) };
    let module = crate::modules::import_value(vm, "_anext")?;
    let cls = vm.getattr(&module, "anext_awaitable")?;
    vm.call_value(&cls, vec![awaitable, default.clone()], Vec::new())
}

/// `breakpoint(*args, **kws)`: chama `sys.breakpointhook`.
fn b_breakpoint(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let sys = crate::modules::import_value(vm, "sys")?;
    let Ok(hook) = vm.getattr(&sys, "breakpointhook") else {
        return Err(exc("RuntimeError", "lost sys.breakpointhook"));
    };
    vm.call_value(&hook, args, kw)
}

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
    Ok(crate::globalsview::view_for(&vm.globals, None))
}

/// `locals()` no nível do módulo: as globais, como no CPython. É uma função à parte de `globals`
/// porque cada uma tem a sua docstring (registrada pelo endereço da função).
fn b_locals(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    b_globals(vm, args, kw)
}

/// Nomes que os protocolos (`collections.abc`, `numbers`, `io`) e os tipos embutidos costumam expor: o
/// interpretador não enumera os métodos de um tipo embutido, então `dir`/`__dict__` sondam esta lista.
const PROBE_NAMES: &[&str] = &[
    "__abs__", "__add__", "__aenter__", "__aexit__", "__aiter__", "__and__", "__anext__", "__await__",
    "__bool__", "__buffer__", "__bytes__", "__call__", "__class_getitem__", "__contains__", "__delitem__",
    "__enter__", "__eq__", "__exit__", "__float__", "__floordiv__", "__format__", "__fspath__", "__ge__",
    "__getitem__", "__gt__", "__hash__", "__iadd__", "__index__", "__init__", "__int__", "__invert__",
    "__iter__", "__le__", "__len__", "__lshift__", "__lt__", "__mod__", "__mul__", "__ne__", "__neg__",
    "__next__", "__or__", "__pos__", "__pow__", "__radd__", "__repr__", "__reversed__", "__rmul__",
    "__rshift__", "__setitem__", "__str__", "__sub__", "__truediv__", "__xor__", "__length_hint__",
    "append", "clear", "close", "copy", "count", "decode", "encode", "extend", "get", "index", "insert",
    "items", "join", "keys", "pop", "popitem", "remove", "reverse", "send", "setdefault", "sort", "split",
    "throw", "update", "values", "add", "discard", "difference", "intersection", "union", "isdisjoint",
    "issubset", "issuperset", "read", "readable", "readline", "readlines", "seek", "seekable", "tell",
    "truncate", "writable", "write", "writelines", "flush", "fileno", "isatty", "detach", "real", "imag",
    "numerator", "denominator", "conjugate", "bit_length", "bit_count", "to_bytes", "from_bytes", "startswith",
    "endswith", "strip", "replace", "format", "lower", "upper", "find", "fromkeys", "move_to_end",
];

/// Atributos de um tipo embutido que o interpretador resolve, na ordem de `PROBE_NAMES`.
pub(crate) fn probe_type_attrs(vm: &mut Vm, ty: &Value) -> Vec<(String, Value)> {
    PROBE_NAMES.iter().filter_map(|n| vm.getattr(ty, n).ok().map(|v| ((*n).to_string(), v))).collect()
}

/// `dir(obj)`: nomes de atributo ordenados (instância, classe e módulo; o resto sai vazio).
fn b_dir(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("dir", &kw)?;
    // Objeto cuja classe define `__dir__` (o `Enum`, por exemplo): o resultado é o dele, ordenado.
    if let Some(Value::Instance(i)) = args.first() {
        if i.class.lookup("__dir__").is_some() {
            let f = vm.getattr(&args[0], "__dir__")?;
            let listed = vm.call_value(&f, Vec::new(), Vec::new())?;
            return vm.call_value(&Value::Builtin("sorted"), vec![listed], Vec::new());
        }
    }
    let mut names = dir_names(vm, args.first());
    names.sort();
    names.dedup();
    Ok(Value::list(names.into_iter().map(Value::str).collect()))
}

/// Os atributos do tipo `object` no CPython 3.13.
const OBJECT_ATTRS: &[&str] = &[
    "__class__", "__delattr__", "__dir__", "__doc__", "__eq__", "__format__", "__ge__", "__getattribute__", "__getstate__",
    "__gt__", "__hash__", "__init__", "__init_subclass__", "__le__", "__lt__", "__ne__", "__new__", "__reduce__",
    "__reduce_ex__", "__repr__", "__setattr__", "__sizeof__", "__str__", "__subclasshook__",
];

/// Nomes de atributos de `obj` (ou das globais, sem argumento), na ordem em que o `dir()` os junta.
pub(crate) fn dir_names(vm: &mut Vm, obj: Option<&Value>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    match obj {
        None => names.extend(vm.globals.borrow().keys().map(|k| k.to_string())),
        // `dir(object)` e `dir(object())`: os slots do tipo `object` do CPython.
        Some(Value::Builtin("object")) => names.extend(OBJECT_ATTRS.iter().map(|s| (*s).to_string())),
        Some(Value::Ext(e)) if e.type_name() == "object" => names.extend(OBJECT_ATTRS.iter().map(|s| (*s).to_string())),
        Some(t @ (Value::Builtin(_) | Value::NativeFn(_))) if crate::builtins::class_name(t).is_some() => {
            match crate::builtins::class_name(t).and_then(type_dir) {
                Some(listed) => names.extend(listed.iter().map(|s| (*s).to_string())),
                None => names.extend(probe_type_attrs(vm, t).into_iter().map(|(n, _)| n)),
            }
        }
        Some(Value::Instance(i)) => {
            // Classe que emula um tipo embutido (`memoryview`, `complex`): no CPython a instância
            // não tem dicionário, e o estado interno do shim não aparece.
            if !emulates_builtin(&i.class) {
                names.extend(i.dict.borrow().keys().cloned());
            }
            class_dir_names(vm, &i.class, &mut names);
        }
        Some(Value::Class(c)) => class_dir_names(vm, c, &mut names),
        Some(Value::Module(m)) => {
            // `__spec__` e `__loader__` são criados sob demanda; o `dir()` os lista como no CPython.
            let module = Value::Module(m.clone());
            let _ = vm.getattr(&module, "__spec__");
            names.extend(m.attrs.borrow().keys().cloned());
            if let Some(g) = vm.module_globals.borrow().get(m.name) {
                names.extend(g.borrow().keys().map(|k| k.to_string()));
            }
        }
        Some(Value::Ext(e)) => match type_dir(e.type_name()) {
            Some(listed) => names.extend(listed.iter().map(|s| (*s).to_string())),
            None => names.extend(e.methods().iter().map(|s| (*s).to_string())),
        },
        // `NotImplemented` e `...`: valores únicos dos seus tipos, não funções.
        Some(Value::Builtin(n @ ("NotImplemented" | "Ellipsis"))) => {
            let t = if *n == "Ellipsis" { "ellipsis" } else { "NotImplementedType" };
            names.extend(type_dir(t).unwrap_or_default().iter().map(|s| (*s).to_string()));
        }
        Some(Value::Function(f)) => {
            names.extend(type_dir("function").unwrap_or_default().iter().map(|s| (*s).to_string()));
            names.extend(f.attrs.borrow().keys().map(|k| k.to_string()));
        }
        Some(v) => match type_dir(v.type_name()) {
            Some(listed) => names.extend(listed.iter().map(|s| (*s).to_string())),
            None => names.extend(crate::suggest::builtin_methods(v.type_name()).iter().map(|s| (*s).to_string())),
        },
    }
    names
}

/// O `dir()` do tipo embutido `name` no CPython 3.13 do Debian (`builtin-type-dir.tsv`, gerado
/// no oráculo): `dict`, `list`, as exceções...
pub(crate) fn type_dir(name: &str) -> Option<Vec<&'static str>> {
    const TABLE: &str = include_str!("../data/cpython-docs/builtin-type-dir.tsv");
    TABLE.lines().find_map(|line| {
        let (t, names) = line.split_once('\t')?;
        (t == name).then(|| names.split(' ').collect())
    })
}

/// A classe é um shim de tipo embutido (`__module__` igual a `builtins`, como `memoryview`).
/// A classe é o shim de um tipo escrito em C: de `builtins` ou de um módulo que no Debian é C
/// embutido no executável (o `Scanner` do `_json`, por exemplo).
fn emulates_builtin(c: &Rc<crate::object::ClassObj>) -> bool {
    matches!(c.dict.borrow().get("__module__"), Some(Value::Str(m))
        if m.as_str() == "builtins" || crate::object::BUILTIN_MODULES.contains(&m.as_str()))
}

/// Os nomes que o `dir()` do CPython junta de uma classe: o dicionário de cada classe do MRO, o que
/// o `type` põe em toda classe (`__doc__`, `__module__` e, sem `__slots__`, `__dict__` e
/// `__weakref__`), os métodos do tipo embutido de base e os de `object`.
fn class_dir_names(vm: &mut Vm, cls: &Rc<crate::object::ClassObj>, names: &mut Vec<String>) {
    for c in cls.mro() {
        if emulates_builtin(&c) {
            // O shim de um tipo embutido mostra a API do tipo real (tabela do oráculo), nunca os
            // auxiliares dele (`_check`) nem o que o `class` acrescenta.
            if let Some(listed) = type_dir(c.name.as_str()) {
                names.extend(listed.iter().map(|s| (*s).to_string()));
                continue;
            }
            let in_builtins = matches!(c.dict.borrow().get("__module__"), Some(Value::Str(m)) if m.as_str() == "builtins");
            names.extend(
                c.dict
                    .borrow()
                    .keys()
                    .filter(|k| !k.starts_with('_') || is_dunder(k))
                    .filter(|k| !matches!(k.as_str(), "__slots__" | "__firstlineno__" | "__static_attributes__"))
                    .filter(|k| !(in_builtins && k.as_str() == "__module__"))
                    .cloned(),
            );
            continue;
        }
        names.extend(c.dict.borrow().keys().cloned());
        names.extend(["__doc__", "__module__"].map(String::from));
        let slots = c.dict.borrow().get("__slots__").cloned();
        match &slots {
            None => names.extend(["__dict__", "__weakref__"].map(String::from)),
            // Cada nome de `__slots__` vira um descritor na classe.
            Some(Value::Str(s)) => names.push(s.as_str().to_string()),
            Some(other) => {
                if let Ok(items) = crate::vm::iterate(other) {
                    names.extend(items.iter().filter_map(|v| match v {
                        Value::Str(s) => Some(s.as_str().to_string()),
                        _ => None,
                    }));
                }
            }
        }
        if let Some(base) = c.data_base.or(c.builtin_base) {
            if let Some(listed) = type_dir(base) {
                names.extend(listed.iter().map(|s| (*s).to_string()));
            } else if let Some(t) = crate::builtins::get(base) {
                names.extend(probe_type_attrs(vm, &t).into_iter().map(|(n, _)| n));
            }
        }
    }
    names.extend(OBJECT_ATTRS.iter().map(|s| (*s).to_string()));
}

fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
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
        vm.push_stdout(text.as_bytes())?;
    }
    // O CPython descarrega stdout (e stderr) em todo `input()`, com ou sem prompt.
    vm.flush_stdout()?;
    let stdin = vm.std_files[0].clone();
    let line = match &mut *stdin.borrow_mut() {
        crate::object::Native::File(f) => crate::stdin::text_line(f),
        _ => None,
    };
    let Some(mut line) = line else {
        return Err(exc("EOFError", "EOF when reading a line"));
    };
    if line.ends_with('\n') {
        line.pop();
    }
    Ok(Value::str(line))
}

fn b_help(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("help", &kw)?;
    let m = crate::modules::import_checked(vm, "pydoc")?;
    let helper = vm.getattr(&Value::Module(m), "help")?;
    vm.call_value(&helper, args, Vec::new())
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
    // Só código embutido importa os módulos de apoio; o `importlib.import_module` repassa o nome
    // que o programa pediu, então não conta como embutido.
    let trusted = vm.frames.borrow().last().is_some_and(|(c, _)| c.internal && c.name != "import_module");
    if !trusted && crate::modules::is_internal(&full) {
        return Err(crate::vm::exc("ModuleNotFoundError", format!("No module named '{full}'")));
    }
    // Importa a cadeia inteira (`a.b.c` carrega `a`, `a.b`, `a.b.c`); sem `fromlist` devolve a raiz. O
    // caminho é o da instrução `import`, com os finders do programa em `sys.meta_path` (o
    // `_distutils_hack` do setuptools troca o `distutils` por um deles via `importlib.import_module`).
    let leaf = crate::modules::import_value(vm, &full)?;
    if wants_leaf || level > 0 {
        return Ok(leaf);
    }
    let top = full.split('.').next().unwrap_or("").to_string();
    crate::modules::import_value(vm, &top)
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

/// Registra `dict` (a visão viva das globais do módulo) como o `__dict__` de `name`: `exec(codigo, mod.__dict__)`
/// passa a rodar direto nas globais do módulo.
pub(crate) fn module_dict_register(name: &'static str, dict: &Value) {
    MODULE_DICTS.with(|m| {
        let mut m = m.borrow_mut();
        match m.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = dict.clone(),
            None => m.push((name, dict.clone())),
        }
    });
}

/// Substitui o conteúdo de `target` pelas globais `map`: nomes existentes na ordem de antes, os
/// novos em ordem alfabética (o mapa de globais não guarda a ordem de inserção).
fn write_back(target: &Value, map: &crate::object::VarMap, was: &[String]) -> PyResult<()> {
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
    let mut added: Vec<&Rc<str>> = map.keys().filter(|k| !k.starts_with("__builtins__") && !fresh.contains(&Value::str(&***k)).unwrap_or(false)).collect();
    added.sort();
    for k in added {
        fresh.set(Value::str(&**k), map[k].clone())?;
    }
    *d.borrow_mut() = fresh;
    Ok(())
}

/// Roda `src` (`exec`) ou avalia a expressão (`eval`) nos espaços de nomes dados; sem eles, nas
/// globais atuais. `globals`/`locals` são dicts: o conteúdo entra numa tabela de globais, o código
/// roda nela, e o resultado volta para o dict (um dict de módulo roda direto nas globais do módulo).
fn run_ns(
    vm: &mut Vm,
    src: &str,
    filename: Option<&str>,
    globals: Option<Value>,
    locals: Option<Value>,
    eval: bool,
    who: &str,
) -> PyResult<Value> {
    let mut text = if eval { format!("__eval_value__ = ({})", src.trim()) } else { src.to_string() };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let module = crate::parser::parse_module(&text).map_err(|e| {
        if eval {
            let kind = match e.kind {
                crate::parser::ErrorKind::Syntax => "SyntaxError",
                crate::parser::ErrorKind::Indentation => "IndentationError",
                crate::parser::ErrorKind::Tab => "TabError",
            };
            exc(kind, e.msg)
        } else {
            crate::vm::syntax_exc(e, filename.unwrap_or("<string>"), src)
        }
    })?;
    let mut code = crate::compile::compile_module(&module).map_err(|e| exc("SyntaxError", e.msg))?;
    if let Some(f) = filename {
        code.set_filename(f);
    }
    let code = Rc::new(code);
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
    // Como o CPython, globais sem `__builtins__` ganham o dict do módulo `builtins`.
    if !g.borrow().contains(&Value::str("__builtins__")).unwrap_or(true) {
        if let Some(b) = crate::modules::builtins_dict(vm) {
            g.borrow_mut().set(Value::str("__builtins__"), b)?;
        }
    }
    if let Some(l) = &locals {
        if !matches!(l, Value::Dict(_)) {
            return Err(type_error("locals must be a mapping"));
        }
    }
    let live = crate::globalsview::map_of_dict(&gdict)
        .or_else(|| module_of_dict(&gdict).and_then(|n| vm.module_globals.borrow().get(n).cloned()));
    let map: Rc<std::cell::RefCell<crate::object::VarMap>> = match &live {
        Some(m) => m.clone(),
        None => Rc::new(std::cell::RefCell::new(Default::default())),
    };
    let mut was: Vec<String> = Vec::new();
    let gitems: Vec<(Value, Value)> = g.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in gitems {
        if let Value::Str(name) = &k {
            was.push(name.as_str().to_string());
            if live.is_none() {
                map.borrow_mut().insert(name.as_str().into(), v);
            }
        }
    }
    let separate = locals.as_ref().filter(|l| !crate::object::is(l, &gdict));
    // Com `locals` separado, o que o código cria não pode vazar para as globais vivas do módulo.
    let backup = if live.is_some() && separate.is_some() { Some(map.borrow().clone()) } else { None };
    let mut lwas: Vec<String> = Vec::new();
    if let Some(Value::Dict(l)) = separate {
        let litems: Vec<(Value, Value)> = l.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in litems {
            if let Value::Str(name) = &k {
                lwas.push(name.as_str().to_string());
                map.borrow_mut().insert(name.as_str().into(), v);
            }
        }
    }
    let mut inner = vm.clone();
    inner.globals = map.clone();
    let result = inner.run(&code).map_err(|e| e.exc);
    let value = if eval { map.borrow_mut().remove("__eval_value__").unwrap_or(Value::None) } else { Value::None };
    let snapshot = map.borrow().clone();
    if let Some(original) = backup {
        *map.borrow_mut() = original;
    }
    match separate {
        Some(l) => {
            // Só o que o código criou ou mudou vai para o `locals`; o resto é das globais.
            let initial: Vec<(String, Value)> = was
                .iter()
                .filter_map(|n| g.borrow().get(&Value::str(n.clone())).ok().flatten().map(|v| (n.clone(), v)))
                .collect();
            let mut mine = snapshot.clone();
            mine.retain(|k, v| {
                lwas.iter().any(|w| **w == **k)
                    || !initial.iter().any(|(n, old)| **n == **k && crate::object::is(old, v)) && &**k != "__eval_value__"
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
    let (src, filename) = source_text(vm, "eval", a[0].as_ref().unwrap_or(&Value::None))?;
    run_ns(vm, &src, filename.as_deref(), a[1].clone(), a[2].clone(), true, "eval")
}

fn b_exec(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("exec", args, kw, &["source", "globals", "locals"], 1)?;
    let (src, filename) = source_text(vm, "exec", a[0].as_ref().unwrap_or(&Value::None))?;
    run_ns(vm, &src, filename.as_deref(), a[1].clone(), a[2].clone(), false, "exec")?;
    Ok(Value::None)
}

/// O texto de um argumento de `exec`/`eval`: uma string ou o resultado de `compile` (que traz
/// também o nome de arquivo dos quadros).
fn source_text(vm: &mut Vm, who: &str, v: &Value) -> PyResult<(String, Option<String>)> {
    if let Value::Ext(e) = v {
        if e.type_name() == "code" {
            if let Some(Ok(Value::Str(s))) = e.getattr(vm, "_source") {
                let filename = match e.getattr(vm, "co_filename") {
                    Some(Ok(Value::Str(f))) => Some(f.as_str().to_string()),
                    _ => None,
                };
                return Ok((s.as_str().to_string(), filename));
            }
        }
    }
    Ok((want_str(who, v)?.to_string(), None))
}

/// Resultado de `compile()`: o fonte já validado, que `exec`/`eval` executam depois.
struct CodeSource {
    src: String,
    filename: String,
    /// O módulo compilado, para os atributos `co_*` que olham dentro do código.
    code: Rc<crate::compile::Code>,
}

impl crate::object::ExtObject for CodeSource {
    fn type_name(&self) -> &'static str {
        "code"
    }
    fn repr(&self) -> String {
        format!(
            "<code object <module> at {:#x}, file \"{}\", line 1>",
            crate::object::py_addr(self as *const Self as usize),
            self.filename
        )
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        Some(Ok(match name {
            "_source" => Value::str(self.src.clone()),
            "co_filename" => Value::str(self.filename.clone()),
            "co_name" => Value::str("<module>".to_string()),
            "co_flags" => Value::Int(0x40),
            "co_firstlineno" => Value::Int(1),
            "co_consts" | "co_names" => {
                let inner = crate::tbobj::function_code(&self.code, &self.filename);
                let Value::Ext(e) = &inner else { return None };
                let v = e.getattr(vm, name)?;
                // O `eval` compila `__eval_value__ = (expr)`: o nome auxiliar não é do código do usuário.
                return Some(v.map(|v| match (&v, name) {
                    (Value::Tuple(t), "co_names") => Value::tuple(
                        t.iter().filter(|n| !matches!(n, Value::Str(s) if s.as_str() == "__eval_value__")).cloned().collect(),
                    ),
                    _ => v,
                }));
            }
            _ => return None,
        }))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(type_error(format!("'code' object has no method '{name}'")))
    }
}

fn b_compile(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("compile", args, kw, &["source", "filename", "mode", "flags", "dont_inherit", "optimize"], 3)?;
    let mut source = a[0].clone().unwrap_or(Value::None);
    let flags = match a[3].as_ref() {
        Some(Value::Int(n)) => *n,
        _ => 0,
    };
    // Uma árvore (`ast.AST`) vira texto com `ast.unparse` e segue o caminho de sempre.
    if let Value::Bytes(b) = &source {
        source = Value::str(String::from_utf8_lossy(b).into_owned());
    }
    if let Value::Instance(_) = &source {
        let ast = crate::modules::import_checked(vm, "ast")?;
        let unparse = vm.getattr(&Value::Module(ast), "unparse")?;
        source = vm.call_value(&unparse, vec![source], Vec::new())?;
        if flags & 1024 != 0 {
            return Ok(a[0].clone().unwrap_or(Value::None));
        }
    }
    // `PyCF_ONLY_AST`: devolve a árvore em vez do código.
    if flags & 1024 != 0 {
        let m = crate::modules::import_checked(vm, "_ast")?;
        let parse = vm.getattr(&Value::Module(m), "_parse")?;
        let rest = vec![source, a[1].clone().unwrap_or(Value::None), a[2].clone().unwrap_or(Value::None)];
        return vm.call_value(&parse, rest, Vec::new());
    }
    let src = want_str("compile", &source)?.to_string();
    let filename = a[1].as_ref().map(|f| crate::object::to_str(f)).unwrap_or_default();
    let mode = a[2].as_ref().map(|m| crate::object::to_str(m)).unwrap_or_default();
    if !matches!(mode.as_str(), "exec" | "eval" | "single") {
        return Err(exc("ValueError", "compile() mode must be 'exec', 'eval' or 'single'"));
    }
    let mut text = if mode == "eval" { format!("__eval_value__ = ({})", src.trim()) } else { src.clone() };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let module = crate::parser::parse_module(&text).map_err(|e| {
        if mode == "eval" {
            let kind = match e.kind {
                crate::parser::ErrorKind::Syntax => "SyntaxError",
                crate::parser::ErrorKind::Indentation => "IndentationError",
                crate::parser::ErrorKind::Tab => "TabError",
            };
            exc(kind, e.msg)
        } else {
            crate::vm::syntax_exc(e, &filename, &src)
        }
    })?;
    let mut code = crate::compile::compile_module(&module).map_err(|e| exc("SyntaxError", e.msg))?;
    // Os quadros do código compilado levam o nome de arquivo dado (o `setup.py` do setuptools).
    code.set_filename(&filename);
    let code = Rc::new(code);
    // Modo `single`: uma expressão solta passa pelo `sys.displayhook` (é o que o doctest espera).
    let src = if mode == "single" && crate::parser::parse_module(&format!("__eval_value__ = ({})\n", src.trim())).is_ok() {
        format!("import sys as __single_sys__\n__single_sys__.displayhook({})\n", src.trim())
    } else {
        src
    };
    Ok(Value::Ext(Rc::new(CodeSource { src, filename, code })))
}
