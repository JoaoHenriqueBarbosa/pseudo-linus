//! RustPython 0.6 sem a feature `host_env`, sobre o protocolo comum do F17.
//!
//! Sem `host_env` não existem os módulos `posix`/`os`, `_socket`, `select`, `fcntl`, `_posixsubprocess`
//! nem o `FileIO` do `_io`, e o importlib só instala os importadores de builtin e congelado (não há
//! busca em `sys.path`). Os módulos em Python (json, re, collections...) vêm congelados no binário pelo
//! `rustpython-pylib` com `freeze-stdlib`. O `print` vai pro `sys.stdout`, que trocamos por um
//! `_io.StringIO`; o `open` é nosso (prelúdio em Python sobre um dict que espelha `Snippet::files`).

use std::collections::BTreeMap;

use f15_interp_common::{Engine, Snippet, SnippetResult, serve};
use rustpython_vm::builtins::{PyDictRef, PyStr};
use rustpython_vm::{Interpreter, PyObjectRef, PyResult, Settings, VirtualMachine};

/// Prelúdio que roda antes de cada trecho, no mesmo escopo. Troca stdout/stderr por buffers em
/// memória e instala um `open` que lê e escreve no dict `__f15_vfs__`.
const PRELUDE: &str = r#"
import sys as __f15_sys, _io as __f15_io, builtins as __f15_builtins
__f15_sys.stdout = __f15_io.StringIO()
__f15_sys.stderr = __f15_io.StringIO()

def __f15_make_open(vfs, io):
    class VFile:
        def __init__(self, path, mode, data):
            self._path = path
            self._mode = mode
            self._binary = 'b' in mode
            if self._binary:
                self._buf = io.BytesIO(data.encode('utf-8'))
            else:
                self._buf = io.StringIO(data)
            if 'a' in mode:
                self._buf.seek(0, 2)
        @property
        def closed(self):
            return self._buf.closed
        @property
        def name(self):
            return self._path
        @property
        def mode(self):
            return self._mode
        def read(self, *a):
            return self._buf.read(*a)
        def readline(self, *a):
            return self._buf.readline(*a)
        def readlines(self, *a):
            return self._buf.readlines(*a)
        def write(self, s):
            return self._buf.write(s)
        def writelines(self, lines):
            for line in lines:
                self._buf.write(line)
        def seek(self, *a):
            return self._buf.seek(*a)
        def tell(self):
            return self._buf.tell()
        def flush(self):
            pass
        def __iter__(self):
            return self
        def __next__(self):
            line = self._buf.readline()
            if not line:
                raise StopIteration
            return line
        def close(self):
            if self._buf.closed:
                return
            if any(c in self._mode for c in 'wax+'):
                v = self._buf.getvalue()
                vfs[self._path] = v.decode('utf-8') if self._binary else v
            self._buf.close()
        def __enter__(self):
            return self
        def __exit__(self, *exc):
            self.close()
            return False
        def __del__(self):
            self.close()

    def open(file, mode='r', buffering=-1, encoding=None, errors=None, newline=None, closefd=True, opener=None):
        path = str(file)
        if path.startswith('./'):
            path = path[2:]
        if 'x' in mode:
            if path in vfs:
                raise FileExistsError(17, 'File exists', file)
            vfs[path] = ''
            return VFile(path, mode, '')
        if 'w' in mode:
            vfs[path] = ''
            return VFile(path, mode, '')
        if 'a' in mode:
            return VFile(path, mode, vfs.get(path, ''))
        if path not in vfs:
            raise FileNotFoundError(2, 'No such file or directory', file)
        return VFile(path, mode, vfs[path])
    return open

__f15_builtins.open = __f15_make_open(__f15_vfs__, __f15_io)
del __f15_make_open
"#;

struct RustPythonEngine;

fn interpreter() -> Interpreter {
    let builder = Interpreter::builder(Settings::default());
    let defs = rustpython_stdlib::stdlib_module_defs(&builder.ctx);
    builder.add_native_modules(&defs).add_frozen_modules(rustpython_pylib::FROZEN_STDLIB).build()
}

fn format_exc(vm: &VirtualMachine, exc: &rustpython_vm::builtins::PyBaseExceptionRef) -> String {
    let mut s = String::new();
    let _ = vm.write_exception(&mut s, exc);
    s.trim_end().to_string()
}

fn obj_to_string(vm: &VirtualMachine, obj: PyObjectRef) -> String {
    match obj.downcast_ref::<PyStr>() {
        Some(s) => s.to_string_lossy().into_owned(),
        None => obj.str(vm).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

fn run_snippet(vm: &VirtualMachine, snippet: &Snippet) -> (String, Option<String>, BTreeMap<String, String>) {
    let scope = vm.new_scope_with_builtins();
    let vfs: PyDictRef = vm.ctx.new_dict();
    for (k, v) in &snippet.files {
        let _ = vfs.set_item(k.as_str(), vm.ctx.new_str(v.as_str()).into(), vm);
    }
    let setup: PyResult<()> = (|| {
        scope.globals.set_item("__f15_vfs__", vfs.clone().into(), vm)?;
        vm.run_string(scope.clone(), PRELUDE, "<f15-prelude>")?;
        Ok(())
    })();
    if let Err(exc) = setup {
        return (String::new(), Some(format!("prelúdio: {}", format_exc(vm, &exc))), snippet.files.clone());
    }
    let error = vm.run_string(scope.clone(), &snippet.code, "<string>").err().map(|e| format_exc(vm, &e));
    let stdout = vm
        .import("sys", 0)
        .and_then(|sys| sys.get_attr("stdout", vm))
        .and_then(|out| vm.call_method(&out, "getvalue", ()))
        .map(|v| obj_to_string(vm, v))
        .unwrap_or_default();
    let mut files = BTreeMap::new();
    for (k, v) in vfs.items_vec() {
        files.insert(obj_to_string(vm, k), obj_to_string(vm, v));
    }
    (stdout, error, files)
}

impl Engine for RustPythonEngine {
    fn name(&self) -> &'static str {
        "rustpython"
    }
    fn version(&self) -> &'static str {
        "0.6.0"
    }
    fn language(&self) -> &'static str {
        "python"
    }

    fn run(&self, snippet: &Snippet) -> SnippetResult {
        let interp = interpreter();
        let (stdout, error, files) = interp.enter(|vm| run_snippet(vm, snippet));
        let _ = vm_finalize(interp);
        SnippetResult { id: snippet.id.clone(), stdout, error, files, elapsed_us: 0 }
    }

    fn notes(&self) -> Vec<String> {
        vec![
            "rustpython-vm e rustpython-stdlib sem default-features (sem host_env, stdio, wasmbind); stdlib em Python congelada pelo rustpython-pylib (freeze-stdlib).".into(),
            "open é um prelúdio Python nosso sobre um dict; stdout/stderr são _io.StringIO.".into(),
            "O crate rustpython-host_env continua na árvore como dependência obrigatória do rustpython-vm (relógio, errno), mesmo sem a feature host_env.".into(),
        ]
    }
}

/// Finaliza o interpretador (roda atexit e libera o VM) e devolve o código de saída que ele daria.
fn vm_finalize(interp: Interpreter) -> u32 {
    interp.finalize(None)
}

fn main() {
    serve(&RustPythonEngine);
}
