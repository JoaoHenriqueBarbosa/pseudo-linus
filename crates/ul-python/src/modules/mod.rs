//! Módulos embutidos do interpretador, escritos em Rust (sem depender de `Lib/` em Python).
//!
//! Cada módulo é um construtor `fn(&mut Vm) -> Rc<ModuleObj>` registrado em [`import`]; os atributos
//! são funções nativas (`ModuleBuilder::func`), constantes ou outros valores. A VM guarda o módulo já
//! construído em `Vm::modules`, então `import x` repetido devolve o mesmo objeto.
//!
//! Para acrescentar um módulo: crie `modules/<nome>.rs` com `pub fn build(vm: &mut Vm) -> Rc<ModuleObj>`,
//! declare `pub mod <nome>;` aqui e acrescente o nome na tabela de [`import`].

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
pub mod string;
pub mod weakrefmod;
pub mod textwrap;
pub mod zlibnative;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::object::{ModuleObj, NativeFn, NativeFnPtr, Value};
use crate::vm::Vm;

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
        "csv" => builtin::csv(),
        "re" => re::build(vm),
        "math" => math::build(vm),
        "base64" => base64::build(vm),
        "binascii" => binascii::build(vm),
        "builtins" => builtinsmod::build(vm),
        "hashlib" => hashlib::build(vm),
        "html" => html::build(vm),
        "textwrap" => textwrap::build(vm),
        "shlex" => shlex::build(vm),
        "fnmatch" => fnmatch::build(vm),
        "struct" => pystruct::build(vm),
        "_operator" => operator::build(vm),
        "_os" => osnative::build(vm),
        "_zlib" => zlibnative::build(vm),
        "_weakref" => weakrefmod::build(vm),
        _ => return pysrc::import(vm, name),
    };
    vm.modules.borrow_mut().insert(name.to_string(), m.clone());
    Some(m)
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
        ModuleBuilder::new("csv")
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
