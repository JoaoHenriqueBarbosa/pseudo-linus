//! O módulo `builtins`: os mesmos nomes que a VM resolve quando um identificador não é global.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::object::{ModuleObj, EXC_CLASSES};
use crate::vm::Vm;

const EXTRA: &[&str] = &[
    "object", "NotImplemented", "Ellipsis", "staticmethod", "classmethod", "property", "super", "type", "IOError",
    "EnvironmentError",
];

pub fn build(vm: &mut Vm) -> Rc<ModuleObj> {
    // Uma VM com globais vazias: um global do chamador chamado `list` não pode vazar para cá.
    let mut clean = vm.clone();
    clean.globals = std::rc::Rc::default();
    let mut b = ModuleBuilder::new("builtins");
    let names = crate::builtins::TABLE
        .iter()
        .map(|(n, _)| *n)
        .chain(crate::builtins_ext::TABLE.iter().map(|(n, _)| *n))
        .chain(crate::vm::BUILTINS.iter().copied())
        .chain(crate::builtins::TYPE_NAMES.iter().copied())
        .chain(EXC_CLASSES.iter().map(|(n, _)| *n))
        .chain(EXTRA.iter().copied());
    for name in names {
        if let Ok(v) = clean.global_or_builtin(name) {
            b = b.value(name, v);
        }
    }
    b.value("True", crate::object::Value::Bool(true))
        .value("False", crate::object::Value::Bool(false))
        .value("None", crate::object::Value::None)
        .build()
}
