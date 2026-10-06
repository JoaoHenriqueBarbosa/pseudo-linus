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
pub mod csv;
pub mod fnmatch;
pub mod hashlib;
pub mod html;
pub mod json;
pub mod math;
pub mod operator;
pub mod osnative;
pub mod pystruct;
pub mod pysrc;
pub mod pysys;
pub mod re;
pub mod re_engine;
pub mod shlex;
pub mod astnative;
pub mod sqlitenative;
pub mod string;
pub mod weakrefmod;
pub mod textwrap;
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
        "shlex" => shlex::build(vm),
        "fnmatch" => fnmatch::build(vm),
        "_struct" => pystruct::build(vm),
        "unicodedata" => unicodedata::build(vm),
        "_operator" => operator::build(vm),
        "_os" => osnative::build(vm),
        "_zlib" => zlibnative::build(vm),
        "_archive" => archivenative::build(vm),
        "_sqlite3" => sqlitenative::build(vm),
        "_ast_native" => astnative::build(vm),
        "_weakref" => weakrefmod::build(vm),
        _ => return pysrc::import(vm, name),
    };
    vm.modules.borrow_mut().insert(name.to_string(), m.clone());
    Some(m)
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
        import_checked(vm, parent)?;
        if let Some(m) = vm.modules.borrow().get(name) {
            return Ok(m.clone());
        }
    }
    if let Some(m) = userimport::load(vm, name)? {
        return Ok(m);
    }
    import(vm, name).ok_or_else(|| exc("ModuleNotFoundError", format!("No module named '{name}'")))
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
    "_sys", "_csvimpl", "_re", "math", "_base64", "binascii", "builtins", "hashlib", "html", "textwrap", "shlex",
    "fnmatch", "_struct", "unicodedata", "_operator", "_os", "_zlib", "_archive", "_sqlite3", "_ast_native", "_weakref",
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

    pub fn sys(vm: &mut Vm) -> Rc<ModuleObj> {
        let argv = vm.argv.iter().map(|a| Value::str(a.clone())).collect();
        ModuleBuilder::new("sys")
            .value("argv", Value::list(argv))
            .value("stdin", Value::Native(vm.std_files[0].clone()))
            .value("stdout", Value::Native(vm.std_files[1].clone()))
            .value("stderr", Value::Native(vm.std_files[2].clone()))
            .build()
    }

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

    pub fn json() -> Rc<ModuleObj> {
        ModuleBuilder::new("json")
            .value("dumps", Value::Builtin("json.dumps"))
            .value("loads", Value::Builtin("json.loads"))
            .value("JSONDecodeError", Value::Builtin("json.decoder.JSONDecodeError"))
            .build()
    }
}
