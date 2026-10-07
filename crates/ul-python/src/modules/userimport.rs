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
    /// Diretórios de um pacote de namespace (sem `__init__.py`); vazio nos demais casos.
    namespace: Vec<String>,
}

/// `python3 app.pyz` ou `python3 diretório`: o `__main__.py` de um zip ou de um diretório, como
/// `(caminho do __main__.py, texto)`.
pub fn main_of_archive(path: &str) -> Option<(String, String)> {
    let main = format!("{}/__main__.py", path.trim_end_matches('/'));
    let text = read_text(&main)?;
    Some((main, text))
}

fn is_regular(path: &str) -> bool {
    match sysabi::sys::stat(path.as_bytes()) {
        Ok(st) => st.mode & 0o170_000 == 0o100_000,
        Err(_) => false,
    }
}

fn is_file(path: &str) -> bool {
    if sysabi::sys::try_current().is_none() {
        return false;
    }
    is_regular(path) || zip_member(path).is_some()
}

fn read_text(path: &str) -> Option<String> {
    let bytes = match sysabi::sys::read_file(path.as_bytes()) {
        Ok(b) => b,
        Err(_) => zip_member(path)?,
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Divide `caminho/a.zip/dir/mod.py` em (arquivo zip, `dir/mod.py`): o prefixo mais longo que é arquivo regular.
fn zip_split(path: &str) -> Option<(String, String)> {
    let mut end = path.len();
    while let Some(i) = path[..end].rfind('/') {
        let prefix = &path[..i];
        if !prefix.is_empty() && is_regular(prefix) {
            return Some((prefix.to_string(), path[i + 1..].to_string()));
        }
        end = i;
    }
    None
}

/// O conteúdo do membro `inner` do zip `archive` (guardado/deflate), como o `zipimport` do CPython lê o módulo.
fn zip_member(path: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let (archive, inner) = zip_split(path)?;
    let data = sysabi::sys::read_file(archive.as_bytes()).ok()?;
    let u16le = |at: usize| data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32le = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    // Fim do diretório central: assinatura PK\x05\x06 nos últimos 64 KiB.
    let floor = data.len().saturating_sub(22 + 65_535);
    let eocd = (floor..=data.len().checked_sub(22)?).rev().find(|&i| data[i..].starts_with(b"PK\x05\x06"))?;
    let count = u16le(eocd + 10)?;
    let mut at = u32le(eocd + 16)?;
    for _ in 0..count {
        if data.get(at..at + 4)? != b"PK\x01\x02" {
            return None;
        }
        let method = u16le(at + 10)?;
        let csize = u32le(at + 20)?;
        let (nlen, xlen, clen) = (u16le(at + 28)?, u16le(at + 30)?, u16le(at + 32)?);
        let local = u32le(at + 42)?;
        let name = data.get(at + 46..at + 46 + nlen)?;
        if name == inner.as_bytes() {
            // O cabeçalho local repete nome e extra com tamanhos próprios.
            let start = local + 30 + u16le(local + 26)? + u16le(local + 28)?;
            let raw = data.get(start..start + csize)?;
            return match method {
                0 => Some(raw.to_vec()),
                8 => {
                    let mut out = Vec::new();
                    flate2::read::DeflateDecoder::new(raw).read_to_end(&mut out).ok()?;
                    Some(out)
                }
                _ => None,
            };
        }
        at += 46 + nlen + xlen + clen;
    }
    None
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
        vm.modules.borrow().get(package).and_then(|m| {
            // Um módulo posto em `sys.modules` com outro nome (o `setuptools._distutils` que o
            // `_distutils_hack` instala como `distutils`) guarda o `__path__` nas globais do nome dele.
            let own = vm.module_globals.borrow().get(m.name).and_then(|g| g.borrow().get("__path__").cloned());
            own.or_else(|| m.attrs.borrow().get("__path__").cloned())
        })
    });
    // Como o `_find_and_load`, vale o `__path__` de qualquer objeto em `sys.modules`.
    let foreign = vm.foreign_modules.borrow().get(package).cloned();
    let path = match path {
        Some(p) => Some(p),
        None => match foreign {
            Some(v) => vm.load_attr(&v, "__path__").ok(),
            None => None,
        },
    };
    match path {
        Some(Value::List(l)) => l.borrow().iter().filter_map(|v| if let Value::Str(s) = v { Some(s.as_str().to_string()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

fn join(dir: &str, leaf: &str) -> String {
    // Como o `FileFinder`: entrada relativa de `sys.path` (inclusive o `''` do diretório atual) vira
    // absoluta, e é assim que `__file__` e os tracebacks mostram o módulo.
    let base = if dir.starts_with('/') { dir.to_string() } else { crate::absolute_path(dir) };
    format!("{}/{leaf}", base.trim_end_matches('/'))
}

/// Procura o módulo `name` no disco. Com `stdlib`, só vale o que mora nos diretórios da stdlib
/// (lidos depois dos módulos embutidos); sem, esses diretórios são pulados.
fn find(vm: &mut Vm, name: &str, stdlib: bool) -> Option<Found> {
    let (parent, leaf) = match name.rsplit_once('.') {
        Some((p, l)) => (Some(p), l),
        None => (None, name),
    };
    let dirs = match parent {
        Some(p) => package_path(vm, p),
        None => sys_path(vm),
    };
    // Diretórios sem `__init__.py` formam um pacote de namespace, se nenhum pacote ou módulo regular aparecer antes.
    let mut namespace: Vec<String> = Vec::new();
    for dir in dirs {
        let pkg = join(&dir, leaf);
        if is_embedded_location(&pkg) != stdlib {
            continue;
        }
        let init = join(&pkg, "__init__.py");
        if is_file(&init) {
            return Some(Found { file: init, package_dir: Some(pkg), namespace: Vec::new() });
        }
        let file = join(&dir, &format!("{leaf}.py"));
        if is_file(&file) {
            return Some(Found { file, package_dir: None, namespace: Vec::new() });
        }
        if is_dir(&pkg) {
            namespace.push(pkg);
        }
    }
    if namespace.is_empty() {
        return None;
    }
    Some(Found { file: String::new(), package_dir: None, namespace })
}

/// A stdlib (`/usr/lib/python3.13`) e o Pillow do Debian estão no disco como no oráculo, mas o
/// interpretador usa as versões embutidas deles; só o resto de `sys.path` é lido do disco.
fn is_embedded_location(path: &str) -> bool {
    const DIST: &str = "/usr/lib/python3/dist-packages/";
    path.starts_with("/usr/lib/python3.13/")
        || path.starts_with("/usr/lib/python313.zip")
        || path.strip_prefix(DIST).is_some_and(|rest| {
            let top = rest.split('/').next().unwrap_or(rest);
            let top = top.strip_suffix(".py").unwrap_or(top);
            matches!(top, "PIL" | "olefile")
        })
}

fn is_dir(path: &str) -> bool {
    if sysabi::sys::try_current().is_none() {
        return false;
    }
    match sysabi::sys::stat(if path.is_empty() { b"." } else { path.as_bytes() }) {
        Ok(st) => st.mode & 0o170_000 == 0o040_000,
        Err(_) => false,
    }
}

/// Importa `name` de um arquivo, se existir. `Ok(None)`: não está no disco.
pub fn load(vm: &mut Vm, name: &str) -> PyResult<Option<Rc<ModuleObj>>> {
    let found = find(vm, name, false);
    load_found(vm, name, found)
}

/// Módulo da stdlib que o interpretador não traz embutido (o pacote `encodings`, por exemplo):
/// roda o `.py` do CPython que está em `/usr/lib/python3.13`.
pub fn load_stdlib(vm: &mut Vm, name: &str) -> PyResult<Option<Rc<ModuleObj>>> {
    let found = find(vm, name, true);
    load_found(vm, name, found)
}

fn load_found(vm: &mut Vm, name: &str, found: Option<Found>) -> PyResult<Option<Rc<ModuleObj>>> {
    let Some(found) = found else { return Ok(None) };
    if !found.namespace.is_empty() {
        return Ok(Some(make_namespace(vm, name, found.namespace)));
    }
    exec_file(vm, name, &found.file, found.package_dir.as_deref()).map(Some)
}

/// Pacote de namespace: sem código nem `__file__`, só o `__path__` com os diretórios achados.
fn make_namespace(vm: &mut Vm, name: &str, dirs: Vec<String>) -> Rc<ModuleObj> {
    let key: &'static str = intern(name);
    let globals: Rc<RefCell<crate::object::VarMap>> = Rc::new(RefCell::new(Default::default()));
    {
        let mut g = globals.borrow_mut();
        g.insert("__name__".into(), Value::str(name));
        g.insert("__package__".into(), Value::str(name));
        g.insert("__doc__".into(), Value::None);
        g.insert("__file__".into(), Value::None);
        g.insert("__path__".into(), Value::list(dirs.into_iter().map(Value::str).collect()));
    }
    let module = Rc::new(ModuleObj { name: key, attrs: RefCell::new(BTreeMap::new()) });
    for (k, v) in globals.borrow().iter() {
        module.attrs.borrow_mut().insert(k.to_string(), v.clone());
    }
    vm.modules.borrow_mut().insert(name.to_string(), module.clone());
    vm.module_globals.borrow_mut().insert(key, globals);
    if let Some((parent, child)) = name.rsplit_once('.') {
        let parent_module = vm.modules.borrow().get(parent).cloned();
        if let Some(p) = parent_module {
            let v = Value::Module(module.clone());
            if let Some(g) = vm.module_globals.borrow().get(p.name) {
                g.borrow_mut().insert(child.into(), v.clone());
            }
            p.attrs.borrow_mut().insert(child.to_string(), v);
        }
    }
    module
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
    // Módulo que o CPython congela (o `runpy`, por exemplo): o código leva o nome `<frozen runpy>`,
    // e o traceback mostra o quadro sem a linha do fonte, mesmo com o `.py` no disco.
    if file.starts_with("/usr/lib/python3.13/") && crate::object::FROZEN_MODULES.contains(&name) {
        code.set_filename(&format!("<frozen {name}>"));
    } else {
        code.set_filename(file);
        crate::vm::register_source(file, &text);
    }
    let key: &'static str = intern(name);
    let package = match package_dir {
        Some(_) => name.to_string(),
        None => name.rsplit_once('.').map(|(p, _)| p.to_string()).unwrap_or_default(),
    };
    let frozen = file.starts_with("/usr/lib/python3.13/") && crate::object::FROZEN_MODULES.contains(&name);
    let builtins = crate::modules::builtins_dict(vm);
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
        // Módulo congelado não tem `__cached__`; os outros apontam o `.pyc` do `__pycache__`.
        if !frozen {
            if let Some(cached) = crate::modules::cached_path(file) {
                g.insert("__cached__".into(), Value::str(cached));
            }
        }
        if let Some(b) = builtins {
            g.insert("__builtins__".into(), b);
        }
    }
    let module = Rc::new(ModuleObj { name: key, attrs: RefCell::new(BTreeMap::new()) });
    // As globais vivas são a fonte; o `repr` do módulo lê o arquivo daqui.
    if let Some(f) = globals.borrow().get("__file__") {
        module.attrs.borrow_mut().insert("__file__".to_string(), f.clone());
    }
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
                g.borrow_mut().insert(child.into(), v.clone());
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
