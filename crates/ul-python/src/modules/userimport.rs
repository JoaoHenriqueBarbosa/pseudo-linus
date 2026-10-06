//! Importação de módulos e pacotes escritos em arquivos `.py`: busca em `sys.path` (e no `__path__`
//! do pacote), execução com globais próprias e vivas, `__file__`, `__package__`, `__path__`.
//!
//! `import a.b.c` importa `a`, depois `a.b` (procurando em `a.__path__`), depois `a.b.c`. Um diretório
//! com `__init__.py` é um pacote; vale antes do arquivo `nome.py`, como no CPython.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::object::{intern, ModuleObj, Value};
use crate::vm::{exc, PyException, PyResult, Vm};

/// O que foi achado no disco para um módulo.
struct Found {
    file: String,
    /// Diretório do pacote (`Some` se achou `nome/__init__.py`).
    package_dir: Option<String>,
}

fn is_file(path: &str) -> bool {
    if sysabi::sys::try_current().is_none() {
        return false;
    }
    match sysabi::sys::stat(path.as_bytes()) {
        Ok(st) => st.mode & 0o170_000 == 0o100_000,
        Err(_) => false,
    }
}

fn read_text(path: &str) -> Option<String> {
    let bytes = sysabi::sys::read_file(path.as_bytes()).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Entradas de `sys.path` (o módulo `sys` pode não ter sido carregado ainda: então só o diretório atual).
fn sys_path(vm: &mut Vm) -> Vec<String> {
    let Some(sys) = crate::modules::import(vm, "sys") else { return vec![String::new()] };
    let list = sys.attrs.borrow().get("path").cloned();
    match list {
        Some(Value::List(l)) => l.borrow().iter().filter_map(|v| if let Value::Str(s) = v { Some(s.as_str().to_string()) } else { None }).collect(),
        _ => vec![String::new()],
    }
}

/// O `__path__` de um pacote já importado.
fn package_path(vm: &mut Vm, package: &str) -> Vec<String> {
    let live = vm.module_globals.borrow().get(package).and_then(|g| g.borrow().get("__path__").cloned());
    let path = live.or_else(|| {
        vm.modules.borrow().get(package).and_then(|m| m.attrs.borrow().get("__path__").cloned())
    });
    match path {
        Some(Value::List(l)) => l.borrow().iter().filter_map(|v| if let Value::Str(s) = v { Some(s.as_str().to_string()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

fn join(dir: &str, leaf: &str) -> String {
    if dir.is_empty() {
        leaf.to_string()
    } else {
        format!("{}/{leaf}", dir.trim_end_matches('/'))
    }
}

/// Procura o módulo `name` no disco.
fn find(vm: &mut Vm, name: &str) -> Option<Found> {
    let (parent, leaf) = match name.rsplit_once('.') {
        Some((p, l)) => (Some(p), l),
        None => (None, name),
    };
    let dirs = match parent {
        Some(p) => package_path(vm, p),
        None => sys_path(vm),
    };
    for dir in dirs {
        let pkg = join(&dir, leaf);
        let init = join(&pkg, "__init__.py");
        if is_file(&init) {
            return Some(Found { file: init, package_dir: Some(pkg) });
        }
        let file = join(&dir, &format!("{leaf}.py"));
        if is_file(&file) {
            return Some(Found { file, package_dir: None });
        }
    }
    None
}

/// Importa `name` de um arquivo, se existir. `Ok(None)`: não está no disco.
pub fn load(vm: &mut Vm, name: &str) -> PyResult<Option<Rc<ModuleObj>>> {
    let Some(found) = find(vm, name) else { return Ok(None) };
    exec_file(vm, name, &found.file, found.package_dir.as_deref()).map(Some)
}

/// Executa o arquivo `file` como o módulo `name` (`package_dir`: é o `__init__.py` de um pacote).
pub fn exec_file(vm: &mut Vm, name: &str, file: &str, package_dir: Option<&str>) -> PyResult<Rc<ModuleObj>> {
    let Some(src) = read_text(file) else {
        return Err(exc("ModuleNotFoundError", format!("No module named '{name}'")));
    };
    let mut text = src;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let module = crate::parser::parse_module(&text).map_err(|e| {
        let kind = match e.kind {
            crate::parser::ErrorKind::Syntax => "SyntaxError",
            crate::parser::ErrorKind::Indentation => "IndentationError",
            crate::parser::ErrorKind::Tab => "TabError",
        };
        exc(kind, format!("{} ({file}, line {})", e.msg, e.lineno))
    })?;
    let mut code = crate::compile::compile_module(&module).map_err(|e| exc(e.kind, e.msg))?;
    code.set_filename(file);
    crate::vm::register_source(file, &text);
    let key: &'static str = intern(name);
    let package = match package_dir {
        Some(_) => name.to_string(),
        None => name.rsplit_once('.').map(|(p, _)| p.to_string()).unwrap_or_default(),
    };
    let globals: Rc<RefCell<crate::object::VarMap>> = Rc::new(RefCell::new(Default::default()));
    {
        let mut g = globals.borrow_mut();
        g.insert("__name__".into(), Value::str(name));
        g.insert("__file__".into(), Value::str(file));
        g.insert("__package__".into(), Value::str(package));
        g.insert("__doc__".into(), Value::None);
        if let Some(dir) = package_dir {
            g.insert("__path__".into(), Value::list(vec![Value::str(dir)]));
        }
    }
    let module = Rc::new(ModuleObj { name: key, attrs: RefCell::new(BTreeMap::new()) });
    // Registrado antes de rodar, para que importações circulares enxerguem o módulo.
    vm.modules.borrow_mut().insert(name.to_string(), module.clone());
    vm.module_globals.borrow_mut().insert(key, globals.clone());
    let mut inner = vm.clone();
    inner.globals = globals;
    if let Err(e) = inner.run(&Rc::new(code)) {
        vm.modules.borrow_mut().remove(name);
        vm.module_globals.borrow_mut().remove(key);
        return Err(into_exception(e));
    }
    // `import a.b` deixa `b` como atributo de `a`.
    if let Some((parent, child)) = name.rsplit_once('.') {
        let parent_module = vm.modules.borrow().get(parent).cloned();
        if let Some(p) = parent_module {
            let v = Value::Module(module.clone());
            if let Some(g) = vm.module_globals.borrow().get(p.name) {
                g.borrow_mut().insert(child.to_string(), v.clone());
            }
            p.attrs.borrow_mut().insert(child.to_string(), v);
        }
    }
    Ok(module)
}

/// A exceção de um módulo que falhou, com os quadros dele no traceback.
fn into_exception(e: crate::vm::RuntimeError) -> PyException {
    e.exc
}

/// `importlib.reload(módulo)`: roda o arquivo de novo nas mesmas globais, sem apagar o que já existe.
pub fn reload(vm: &mut Vm, module: &Rc<ModuleObj>) -> PyResult<()> {
    let globals = vm.module_globals.borrow().get(module.name).cloned();
    let file = globals.as_ref().and_then(|g| match g.borrow().get("__file__") {
        Some(Value::Str(f)) => Some(f.as_str().to_string()),
        _ => None,
    });
    let (Some(globals), Some(file)) = (globals, file) else {
        // Módulo embutido ou sem arquivo: não há o que reler.
        return Ok(());
    };
    let Some(src) = read_text(&file) else {
        return Err(exc("ModuleNotFoundError", format!("spec not found for the module '{}'", module.name)));
    };
    let mut text = src;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let parsed = crate::parser::parse_module(&text).map_err(|e| exc("SyntaxError", format!("{} ({file}, line {})", e.msg, e.lineno)))?;
    let mut code = crate::compile::compile_module(&parsed).map_err(|e| exc(e.kind, e.msg))?;
    code.set_filename(&file);
    crate::vm::register_source(&file, &text);
    let mut inner = vm.clone();
    inner.globals = globals;
    inner.run(&Rc::new(code)).map_err(into_exception)
}
