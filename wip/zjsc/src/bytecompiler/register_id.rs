//! Porte de `bytecompiler/RegisterID.h`.
//!
//! `RegisterID` tem contagem de referência intrusiva (`m_refCount`) que o gerador observa
//! (`refCount()`), e os registradores vivem no armazenamento do gerador com contagem 0. Por isso
//! a contagem fica no próprio objeto e o `RefPtr<RegisterID>` do C++ vira `RegisterRef`, que
//! incrementa ao clonar e decrementa ao soltar. O compartilhamento é `Rc<RefCell<RegisterID>>`.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use crate::bytecode::virtual_register::VirtualRegister;

/// `RegisterID` com a posse compartilhada que o gerador usa.
pub type RegisterIDRef = Rc<RefCell<RegisterID>>;

#[derive(Debug)]
pub struct RegisterID {
    ref_count: i32,
    virtual_register: VirtualRegister,
    is_temporary: bool,
    did_set_index: bool,
}

impl Default for RegisterID {
    fn default() -> Self {
        RegisterID {
            ref_count: 0,
            virtual_register: VirtualRegister::default(),
            is_temporary: false,
            did_set_index: false,
        }
    }
}

impl RegisterID {
    /// `RegisterID()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `RegisterID(VirtualRegister)`.
    pub fn from_virtual_register(virtual_register: VirtualRegister) -> Self {
        RegisterID { ref_count: 0, virtual_register, is_temporary: false, did_set_index: true }
    }

    /// `explicit RegisterID(int index)`.
    pub fn from_index(index: i32) -> Self {
        Self::from_virtual_register(VirtualRegister::new(index))
    }

    pub fn set_index(&mut self, index: VirtualRegister) {
        self.did_set_index = true;
        self.virtual_register = index;
    }

    pub fn set_temporary(&mut self) {
        self.is_temporary = true;
    }

    pub fn index(&self) -> i32 {
        debug_assert!(self.did_set_index);
        self.virtual_register.offset()
    }

    pub fn virtual_register(&self) -> VirtualRegister {
        debug_assert!(self.virtual_register.is_valid());
        self.virtual_register
    }

    /// Acesso de `friend class VirtualRegister`, sem os `ASSERT`.
    pub(crate) fn raw_virtual_register(&self) -> VirtualRegister {
        self.virtual_register
    }

    pub fn is_temporary(&self) -> bool {
        self.is_temporary
    }

    pub fn ref_(&mut self) {
        self.ref_count += 1;
    }

    pub fn deref(&mut self) {
        self.ref_count -= 1;
        debug_assert!(self.ref_count >= 0);
    }

    pub fn ref_count(&self) -> i32 {
        self.ref_count
    }
}

/// `RefPtr<RegisterID>`: segura uma referência contada do registrador (nula é `None` em volta).
#[derive(Debug)]
pub struct RegisterRef {
    register: RegisterIDRef,
}

impl RegisterRef {
    /// Adota o registrador incrementando a contagem (`RefPtr(RegisterID*)`).
    pub fn new(register: &RegisterIDRef) -> Self {
        register.borrow_mut().ref_();
        RegisterRef { register: Rc::clone(register) }
    }

    pub fn get(&self) -> &RegisterIDRef {
        &self.register
    }

    pub fn borrow(&self) -> Ref<'_, RegisterID> {
        self.register.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, RegisterID> {
        self.register.borrow_mut()
    }
}

impl Clone for RegisterRef {
    fn clone(&self) -> Self {
        RegisterRef::new(&self.register)
    }
}

impl Drop for RegisterRef {
    fn drop(&mut self) {
        self.register.borrow_mut().deref();
    }
}
