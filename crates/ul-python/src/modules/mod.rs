//! Módulos embutidos do interpretador, escritos em Rust (sem depender de `Lib/` em Python).
//!
//! Cada módulo é um construtor `fn(&mut Vm) -> Rc<ModuleObj>` registrado em [`import`]; os atributos
//! são funções nativas (`ModuleBuilder::func`), constantes ou outros valores. A VM guarda o módulo já
//! construído em `Vm::modules`, então `import x` repetido devolve o mesmo objeto.
//!
//! Para acrescentar um módulo: crie `modules/<nome>.rs` com `pub fn build(vm: &mut Vm) -> Rc<ModuleObj>`,
//! declare `pub mod <nome>;` aqui e acrescente o nome na tabela de [`import`].

pub mod archivenative;
pub mod base64;
pub mod binascii;
pub mod builtinsmod;
pub mod cpydocs;
pub mod csv;
pub mod hashlib;
pub mod html;
pub mod imaging;
pub mod json;
pub mod math;
pub mod operator;
pub mod osnative;
pub mod pystruct;
pub mod pysrc;
pub mod pysys;
pub mod re;
pub mod re_engine;
pub mod astnative;
pub mod sqlitenative;
pub mod string;
pub mod weakrefmod;
pub mod lsprof;
pub mod mtrandom;
pub mod textwrap;
pub mod ucd;
pub mod unicodedata;
pub mod userimport;
pub mod zlibnative;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::object::{ModuleObj, NativeFn, NativeFnPtr, Value};
use crate::vm::{exc, PyResult, Vm};

/// Monta um [`ModuleObj`] atributo a atributo.
pub struct ModuleBuilder {
    name: &'static str,
    attrs: BTreeMap<String, Value>,
}

impl ModuleBuilder {
    pub fn new(name: &'static str) -> ModuleBuilder {
        let mut attrs = BTreeMap::new();
        attrs.insert("__name__".to_string(), Value::str(name));
        ModuleBuilder { name, attrs }
    }

    /// Função nativa `modulo.nome(...)`.
    pub fn func(mut self, name: &'static str, f: NativeFnPtr) -> ModuleBuilder {
        self.attrs.insert(name.to_string(), Value::NativeFn(Rc::new(NativeFn { name, f })));
        self
    }

    /// Constante ou qualquer outro valor.
    pub fn value(mut self, name: &str, v: Value) -> ModuleBuilder {
        self.attrs.insert(name.to_string(), v);
        self
    }

    pub fn build(self) -> Rc<ModuleObj> {
        Rc::new(ModuleObj { name: self.name, attrs: RefCell::new(self.attrs) })
    }
}

/// `import nome`: o módulo embutido, construído na primeira vez. `None` se não existe.
pub fn import(vm: &mut Vm, name: &str) -> Option<Rc<ModuleObj>> {
    if let Some(m) = vm.modules.borrow().get(name) {
        return Some(m.clone());
    }
    let m = match name {
        "_sys" => pysys::build(vm),
        "_csvimpl" => builtin::csv(),
        "_re" => re::build(vm),
        "math" => math::build(vm),
        "_base64" => base64::build(vm),
        "binascii" => binascii::build(vm),
        "builtins" => builtinsmod::build(vm),
        "hashlib" => hashlib::build(vm),
        "html" => html::build(vm),
        "textwrap" => textwrap::build(vm),
        "_struct" => pystruct::build(vm),
        "unicodedata" => unicodedata::build(vm),
        "_operator" => operator::build(vm),
        "_os" => osnative::build(vm),
        "_zlib" => zlibnative::build(vm),
        "PIL._imaging" => imaging::build(vm),
        "PIL._imagingft" => imaging::ft::build(vm),
        "PIL._imagingmath" => imaging::math::build_math(vm),
        "PIL._imagingmorph" => imaging::math::build_morph(vm),
        "_archive" => archivenative::build(vm),
        "_sqlite3" => sqlitenative::build(vm),
        "_ast_native" => astnative::build(vm),
        "_weakref" => weakrefmod::build(vm),
        "_mt" => mtrandom::build(vm),
        "_json_native" => json::build(vm),
        "_prof" => lsprof::build(vm),
        _ => return pysrc::import(vm, name),
    };
    // Os nativos que no Debian são C embutido no executável têm `__doc__` e `__package__` vazio.
    if crate::object::BUILTIN_MODULES.contains(&name) {
        let mut attrs = m.attrs.borrow_mut();
        if !attrs.contains_key("__doc__") {
            let doc = cpydocs::runtime_module_doc(name).map_or(Value::None, Value::str);
            attrs.insert("__doc__".to_string(), doc);
        }
        attrs.entry("__package__".to_string()).or_insert_with(|| Value::str(""));
    }
    cpydocs::register_native(name, &m.attrs.borrow());
    vm.modules.borrow_mut().insert(name.to_string(), m.clone());
    Some(m)
}

/// Módulos de apoio dos embutidos, que não existem no CPython: só código embutido os importa, e o
/// programa os vê como ausentes (`No module named`), inclusive em `sys.modules`.
const INTERNAL: &[&str] = &[
    "_os", "_sys", "_mt", "_net", "_archive", "_archivefile", "_prof", "_csvimpl", "_re", "_base64",
    "_zlib", "_ast_native", "_match", "_memoryview", "_complex", "_excgroup", "asyncio.loopback", "_json_native", "_anext",
];

pub fn is_internal(name: &str) -> bool {
    INTERNAL.contains(&name)
}

/// `import nome` vindo de código do programa: os módulos de apoio não existem para ele.
pub fn import_visible(vm: &mut Vm, name: &str, internal_caller: bool) -> PyResult<Value> {
    if !internal_caller && is_internal(name) {
        return Err(exc("ModuleNotFoundError", format!("No module named '{name}'")));
    }
    import_value(vm, name)
}

/// `import nome` como o `importlib._bootstrap._find_and_load`: devolve o que estiver em `sys.modules`
/// (módulo ou objeto qualquer) e, se o nome não existir no disco nem embutido, pergunta aos finders
/// que o programa pôs em `sys.meta_path` (o `_SixMetaPathImporter` do `six`, por exemplo). Finders
/// postos antes do `PathFinder` são consultados antes da busca em `sys.path`.
/// O `__dict__` do módulo `builtins`, que o CPython põe em `__builtins__` de todo módulo importado.
pub fn builtins_dict(vm: &mut Vm) -> Option<Value> {
    let module = import(vm, "builtins")?;
    vm.load_attr(&Value::Module(module), "__dict__").ok()
}

/// O `__cached__` de um módulo em `file`: o `.pyc` do `__pycache__` ao lado, como o
/// `importlib.util.cache_from_source`.
pub fn cached_path(file: &str) -> Option<String> {
    let (head, tail) = file.rsplit_once('/')?;
    let stem = tail.strip_suffix(".py")?;
    Some(format!("{head}/__pycache__/{stem}.cpython-313.pyc"))
}

pub fn import_value(vm: &mut Vm, name: &str) -> PyResult<Value> {
    if let Some(m) = vm.modules.borrow().get(name) {
        return Ok(Value::Module(m.clone()));
    }
    if let Some(v) = vm.foreign_modules.borrow().get(name).cloned() {
        return Ok(v);
    }
    let mut parent_path = Value::None;
    if let Some((parent, _)) = name.rsplit_once('.') {
        let p = import_value(vm, parent)?;
        if let Some(m) = vm.modules.borrow().get(name) {
            return Ok(Value::Module(m.clone()));
        }
        if let Some(v) = vm.foreign_modules.borrow().get(name).cloned() {
            return Ok(v);
        }
        if let Ok(path) = vm.load_attr(&p, "__path__") {
            parent_path = path;
        }
    }
    let (before, after) = meta_path_finders(vm);
    for finder in &before {
        if let Some(v) = load_with_finder(vm, finder, name, &parent_path)? {
            return Ok(v);
        }
    }
    match import_checked(vm, name) {
        Ok(m) => return Ok(Value::Module(m)),
        Err(e) if e.kind == "ModuleNotFoundError" && !after.is_empty() => {
            for finder in &after {
                if let Some(v) = load_with_finder(vm, finder, name, &parent_path)? {
                    return Ok(v);
                }
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// Os finders do programa em `sys.meta_path`, separados em antes e depois do `PathFinder` (os três
/// importadores padrão ficam de fora: o interpretador já faz o trabalho deles).
fn meta_path_finders(vm: &mut Vm) -> (Vec<Value>, Vec<Value>) {
    let Some(sys) = vm.modules.borrow().get("sys").cloned() else { return (Vec::new(), Vec::new()) };
    let Ok(Value::List(list)) = vm.load_attr(&Value::Module(sys), "meta_path") else {
        return (Vec::new(), Vec::new());
    };
    let entries = list.borrow().clone();
    let (mut before, mut after, mut seen_path) = (Vec::new(), Vec::new(), false);
    for entry in entries {
        if let Value::Class(c) = &entry {
            let standard = matches!(c.name.as_str(), "BuiltinImporter" | "FrozenImporter" | "PathFinder")
                && matches!(
                    vm.load_attr(&entry, "__module__"),
                    Ok(Value::Str(m)) if m.as_str().starts_with("_frozen_importlib")
                );
            if standard {
                seen_path |= c.name.as_str() == "PathFinder";
                continue;
            }
        }
        if seen_path { after.push(entry) } else { before.push(entry) }
    }
    (before, after)
}

/// `finder.find_spec(nome, path)` e, se achou, cria o módulo pelo loader, registra em `sys.modules`
/// e roda `exec_module`, como o `_load_unlocked` do CPython.
fn load_with_finder(vm: &mut Vm, finder: &Value, name: &str, path: &Value) -> PyResult<Option<Value>> {
    let Ok(find_spec) = vm.load_attr(finder, "find_spec") else { return Ok(None) };
    let spec = vm.call_value(&find_spec, vec![Value::str(name), path.clone(), Value::None], Vec::new())?;
    if matches!(spec, Value::None) {
        return Ok(None);
    }
    let loader = vm.load_attr(&spec, "loader")?;
    let mut module = Value::None;
    if let Ok(create) = vm.load_attr(&loader, "create_module") {
        module = vm.call_value(&create, vec![spec.clone()], Vec::new())?;
    }
    if matches!(module, Value::None) {
        module = vm.call_value(&Value::Builtin("module"), vec![Value::str(name)], Vec::new())?;
    }
    // Os atributos que o `_init_module_attrs` do CPython copia da spec (sem sobrescrever os que o
    // `create_module` já preencheu).
    let mut attrs = vec![("__spec__", spec.clone()), ("__loader__", loader.clone())];
    if let Ok(parent) = vm.load_attr(&spec, "parent") {
        attrs.push(("__package__", parent));
    }
    if let Ok(smsl) = vm.load_attr(&spec, "submodule_search_locations") {
        if !matches!(smsl, Value::None) {
            attrs.push(("__path__", smsl));
        }
    }
    if matches!(vm.load_attr(&spec, "has_location"), Ok(Value::Bool(true))) {
        if let Ok(origin) = vm.load_attr(&spec, "origin") {
            attrs.push(("__file__", origin));
        }
        if let Ok(cached) = vm.load_attr(&spec, "cached") {
            if !matches!(cached, Value::None) {
                attrs.push(("__cached__", cached));
            }
        }
    }
    for (attr, value) in attrs {
        if !matches!(vm.load_attr(&module, attr), Ok(ref v) if !matches!(v, Value::None)) {
            let _ = vm.store_attr(&module, attr, value);
        }
    }
    register(vm, name, &module);
    if let Ok(exec) = vm.load_attr(&loader, "exec_module") {
        if let Err(e) = vm.call_value(&exec, vec![module.clone()], Vec::new()) {
            vm.modules.borrow_mut().remove(name);
            vm.foreign_modules.borrow_mut().remove(name);
            return Err(e);
        }
    }
    // O loader pode ter trocado a entrada de `sys.modules` durante a execução.
    if let Some(m) = vm.modules.borrow().get(name) {
        return Ok(Some(Value::Module(m.clone())));
    }
    Ok(Some(vm.foreign_modules.borrow().get(name).cloned().unwrap_or(module)))
}

fn register(vm: &mut Vm, name: &str, value: &Value) {
    match value {
        Value::Module(m) => {
            vm.modules.borrow_mut().insert(name.to_string(), m.clone());
        }
        other => {
            vm.foreign_modules.borrow_mut().insert(name.to_string(), other.clone());
        }
    }
}

/// `import nome` vindo do programa: arquivos do usuário em `sys.path` primeiro (como o CPython), depois
/// os módulos embutidos. Importa os pais de `a.b.c` antes.
pub fn import_checked(vm: &mut Vm, name: &str) -> PyResult<Rc<ModuleObj>> {
    if let Some(m) = vm.modules.borrow().get(name) {
        return Ok(m.clone());
    }
    if name == "__main__" {
        // O script principal como módulo: as globais dele, vivas.
        let m = Rc::new(ModuleObj { name: "__main__", attrs: RefCell::new(BTreeMap::new()) });
        vm.modules.borrow_mut().insert("__main__".to_string(), m.clone());
        return Ok(m);
    }
    if let Some((parent, _)) = name.rsplit_once('.') {
        if !vm.foreign_modules.borrow().contains_key(parent) {
            import_checked(vm, parent)?;
        }
        if let Some(m) = vm.modules.borrow().get(name) {
            return Ok(m.clone());
        }
    }
    if let Some(m) = userimport::load(vm, name)? {
        return Ok(m);
    }
    match import(vm, name) {
        Some(m) => Ok(m),
        None => {
            if let Some(e) = pysrc::take_error() {
                return Err(e);
            }
            match userimport::load_stdlib(vm, name)? {
                Some(m) => Ok(m),
                None => Err(exc("ModuleNotFoundError", format!("No module named '{name}'"))),
            }
        }
    }
}

/// O nome absoluto de `from <level pontos><rel> import ...` a partir do pacote das globais atuais.
pub fn resolve_relative(vm: &mut Vm, rel: &str, level: usize) -> PyResult<String> {
    let (package, has_path) = {
        let g = vm.globals.borrow();
        let name = match g.get("__name__") {
            Some(Value::Str(s)) => s.as_str().to_string(),
            _ => String::new(),
        };
        let has_path = g.contains_key("__path__");
        match g.get("__package__") {
            Some(Value::Str(s)) => (s.as_str().to_string(), has_path),
            _ if has_path => (name, has_path),
            _ => (name.rsplit_once('.').map(|(p, _)| p.to_string()).unwrap_or_default(), has_path),
        }
    };
    let _ = has_path;
    if package.is_empty() {
        return Err(exc("ImportError", "attempted relative import with no known parent package"));
    }
    let mut parts: Vec<&str> = package.split('.').collect();
    if level - 1 >= parts.len() {
        return Err(exc("ImportError", "attempted relative import beyond top-level package"));
    }
    parts.truncate(parts.len() - (level - 1));
    let base = parts.join(".");
    Ok(if rel.is_empty() { base } else { format!("{base}.{rel}") })
}

/// `types.ModuleType(name, doc=None)`: um módulo vazio, com globais vivas e sem entrada em `sys.modules`.
pub fn new_module(vm: &mut Vm, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
    let a = crate::native_util::bind("module", args, kwargs, &["name", "doc"], 1)?;
    let name = crate::native_util::want_str("module", a[0].as_ref().unwrap_or(&Value::None))?.to_string();
    let globals: Rc<RefCell<crate::object::VarMap>> = Rc::new(RefCell::new(Default::default()));
    {
        let mut g = globals.borrow_mut();
        g.insert("__name__".into(), Value::str(name.clone()));
        g.insert("__doc__".into(), a[1].clone().unwrap_or(Value::None));
        g.insert("__package__".into(), Value::None);
        g.insert("__loader__".into(), Value::None);
        g.insert("__spec__".into(), Value::None);
    }
    let key: &'static str = crate::object::intern(&name);
    let module = Rc::new(ModuleObj { name: key, attrs: RefCell::new(BTreeMap::new()) });
    vm.module_globals.borrow_mut().insert(key, globals);
    Ok(Value::Module(module))
}

/// Módulos escritos em Rust, além dos que `pysrc` embute em Python.
const NATIVE_MODULES: &[&str] = &[
    "_sys", "_csvimpl", "_re", "math", "_base64", "binascii", "builtins", "hashlib", "html", "textwrap",
    "_struct", "unicodedata", "_operator", "_os", "_zlib", "_archive", "_sqlite3", "_ast_native", "_weakref", "_mt", "_json_native", "_prof",
    "PIL._imaging", "PIL._imagingft", "PIL._imagingmath", "PIL._imagingmorph",
];

/// `name` é um módulo que o interpretador traz embutido (nativo ou em Python).
pub fn is_builtin_module(name: &str) -> bool {
    NATIVE_MODULES.contains(&name) || pysrc::source(name).is_some()
}

/// Se `name` é um pacote embutido (algum módulo embutido tem `name.` como prefixo).
pub fn is_embedded_package(name: &str) -> bool {
    pysrc::names().iter().any(|n| n.strip_prefix(name).is_some_and(|r| r.starts_with('.')))
}

/// Módulos que já existiam antes do registro; os atributos ainda apontam para os nomes que a
/// VM resolve em `Vm::call`.
mod builtin {
    use super::*;

    pub fn csv() -> Rc<ModuleObj> {
        use crate::modules::csv as c;
        ModuleBuilder::new("_csvimpl")
            .value("reader", Value::Builtin("csv.reader"))
            .value("writer", Value::Builtin("csv.writer"))
            .value("Error", Value::Builtin("_csv.Error"))
            .value("QUOTE_MINIMAL", Value::Int(i64::from(c::QUOTE_MINIMAL)))
            .value("QUOTE_ALL", Value::Int(i64::from(c::QUOTE_ALL)))
            .value("QUOTE_NONNUMERIC", Value::Int(i64::from(c::QUOTE_NONNUMERIC)))
            .value("QUOTE_NONE", Value::Int(i64::from(c::QUOTE_NONE)))
            .value("QUOTE_STRINGS", Value::Int(i64::from(c::QUOTE_STRINGS)))
            .value("QUOTE_NOTNULL", Value::Int(i64::from(c::QUOTE_NOTNULL)))
            .build()
    }

}
