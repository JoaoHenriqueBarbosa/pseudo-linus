//! Funções embutidas registradas por tabela (`isinstance`, `map`, `getattr`...).
//!
//! Resolvidas por `LoadName` depois das globais do usuário e antes das classes de exceção. As
//! funções antigas (`print`, `len`, `sorted`...) ainda são despachadas por nome em `Vm::call`; ao
//! migrá-las, mova-as para esta tabela.

use crate::object::{NativeFn, NativeFnPtr, Value};
use std::rc::Rc;

pub const TABLE: &[(&str, NativeFnPtr)] = &[];

/// A função embutida `name`, se existe na tabela.
pub fn get(name: &str) -> Option<Value> {
    TABLE.iter().find(|(n, _)| *n == name).map(|(n, f)| Value::NativeFn(Rc::new(NativeFn { name: n, f: *f })))
}
