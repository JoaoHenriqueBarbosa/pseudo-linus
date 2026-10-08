//! Porte de `bytecompiler/LabelScope.h`.
//!
//! O refcount intrusivo se mantém no objeto porque `hasOneRef()` é observado por
//! `breakTargetMayBeBound()`. `LabelScopeRef` faz o papel de `Ref<LabelScope>`/`RefPtr<LabelScope>`
//! (o `LabelScopePtr` do gerador): incrementa ao clonar, decrementa ao soltar.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use crate::bytecompiler::label::{LabelRef};
use crate::runtime::identifier::Identifier;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelScopeType {
    Loop,
    Switch,
    NamedLabel,
}

pub struct LabelScope {
    ref_count: i32,
    type_: LabelScopeType,
    name: Option<Identifier>,
    scope_depth: i32,
    break_target: LabelRef,
    continue_target: Option<LabelRef>,
}

impl crate::wtf::ref_counted::RefCounted for LabelScope {
    fn ref_count(&self) -> i32 {
        self.ref_count
    }
}

impl LabelScope {
    pub fn new(
        type_: LabelScopeType,
        name: Option<Identifier>,
        scope_depth: i32,
        break_target: LabelRef,
        continue_target: Option<LabelRef>,
    ) -> Self {
        LabelScope { ref_count: 0, type_, name, scope_depth, break_target, continue_target }
    }

    pub fn break_target(&self) -> &LabelRef {
        &self.break_target
    }

    pub fn continue_target(&self) -> Option<&LabelRef> {
        self.continue_target.as_ref()
    }

    pub fn type_(&self) -> LabelScopeType {
        self.type_
    }

    pub fn name(&self) -> Option<&Identifier> {
        self.name.as_ref()
    }

    pub fn scope_depth(&self) -> i32 {
        self.scope_depth
    }

    pub fn ref_(&mut self) {
        self.ref_count += 1;
    }

    pub fn deref(&mut self) {
        self.ref_count -= 1;
        debug_assert!(self.ref_count >= 0);
    }

    pub fn has_one_ref(&self) -> bool {
        self.ref_count == 1
    }

    pub fn break_target_may_be_bound(&self) -> bool {
        if !self.has_one_ref() {
            return true;
        }
        if !self.break_target.borrow().has_one_ref() {
            return true;
        }
        self.break_target.borrow().is_bound()
    }
}

/// `Ref<LabelScope>`/`RefPtr<LabelScope>`.
pub struct LabelScopeRef {
    scope: Rc<RefCell<LabelScope>>,
}

impl LabelScopeRef {
    pub fn new(scope: &Rc<RefCell<LabelScope>>) -> Self {
        scope.borrow_mut().ref_();
        LabelScopeRef { scope: Rc::clone(scope) }
    }

    pub fn get(&self) -> &Rc<RefCell<LabelScope>> {
        &self.scope
    }

    pub fn borrow(&self) -> Ref<'_, LabelScope> {
        self.scope.borrow()
    }
}

impl Clone for LabelScopeRef {
    fn clone(&self) -> Self {
        LabelScopeRef::new(&self.scope)
    }
}

impl Drop for LabelScopeRef {
    fn drop(&mut self) {
        self.scope.borrow_mut().deref();
    }
}
