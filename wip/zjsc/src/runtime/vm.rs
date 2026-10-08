//! Esqueleto de `JavaScriptCore/runtime/VM.h`.
//!
//! Carrega o que o parser e o `Identifier` usam: `propertyNames`, os dois `SymbolRegistry`, o
//! estado de exceção pendente (`m_exception`, `m_terminationException`) e o contador de
//! `DeferTermination`. A tabela de átomos é por thread em `crate::wtf::text::atom_string_impl`,
//! então o `VM` não a carrega.

use crate::bytecode::bytecode_intrinsic_registry::BytecodeIntrinsicRegistry;
use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use crate::runtime::common_identifiers::CommonIdentifiers;
use crate::runtime::symbol_registry::{SymbolRegistry, SymbolRegistryType};

/// `class Exception`. A célula do GC ainda não foi portada; só a identidade importa para
/// `m_exception == m_terminationException`.
#[derive(Debug)]
pub struct Exception {
    _private: (),
}

/// `CommonIdentifiers* propertyNames { nullptr }`: o C++ a deixa nula até o corpo do construtor
/// (`propertyNames = new CommonIdentifiers(*this)`), porque o construtor recebe o `VM&`. O
/// `OnceCell` guarda a mesma ordem; ler antes de atribuir é falha de programação.
#[derive(Debug, Default)]
pub struct PropertyNames(OnceCell<Box<CommonIdentifiers>>);

impl Deref for PropertyNames {
    type Target = CommonIdentifiers;

    fn deref(&self) -> &CommonIdentifiers {
        self.0.get().expect("VM::propertyNames lido antes de ser criado")
    }
}

/// `class VM`.
#[derive(Debug)]
pub struct VM {
    exception: RefCell<Option<Rc<Exception>>>,
    termination_exception: RefCell<Option<Rc<Exception>>>,
    /// `VMTraps::m_deferTerminationCount`.
    defer_termination_count: Cell<u32>,
    symbol_registry: Box<SymbolRegistry>,
    private_symbol_registry: Box<SymbolRegistry>,
    pub property_names: PropertyNames,
    /// `m_softStackLimit` (`softStackLimit()`): endereço abaixo do qual a pilha da VM é considerada
    /// cheia. 0 até o `StackBounds` real ser portado.
    soft_stack_limit: Cell<usize>,
    /// `m_executingRegExp`: identidade do `RegExp*` em execução (0 é o `nullptr`).
    executing_reg_exp: Cell<usize>,
    /// `m_bytecodeIntrinsicRegistry`: criado no primeiro uso, porque precisa dos `BuiltinNames`.
    bytecode_intrinsic_registry: std::cell::OnceCell<BytecodeIntrinsicRegistry>,
    /// `sourceProviderCacheMap`: a chave é a identidade do `SourceProvider` (o `RefPtr` guarda o objeto vivo).
    source_provider_cache_map: RefCell<std::collections::HashMap<usize, (Rc<dyn crate::parser::source_provider::SourceProvider>, Rc<RefCell<crate::parser::source_provider_cache::SourceProviderCache>>)>>,
}

impl Default for VM {
    fn default() -> VM {
        VM::new()
    }
}

impl VM {
    /// `VM::VM(VmType, HeapType, WTF::RunLoop*, bool*)`, só no que este porte tem.
    pub fn new() -> VM {
        let vm = VM {
            exception: RefCell::new(None),
            termination_exception: RefCell::new(None),
            defer_termination_count: Cell::new(0),
            symbol_registry: Box::new(SymbolRegistry::new(SymbolRegistryType::PublicSymbol)),
            private_symbol_registry: Box::new(SymbolRegistry::new(SymbolRegistryType::PrivateSymbol)),
            property_names: PropertyNames::default(),
            soft_stack_limit: Cell::new(0),
            executing_reg_exp: Cell::new(0),
            bytecode_intrinsic_registry: std::cell::OnceCell::new(),
            source_provider_cache_map: RefCell::new(std::collections::HashMap::new()),
        };
        let property_names = Box::new(CommonIdentifiers::new(&vm));
        assert!(vm.property_names.0.set(property_names).is_ok());
        vm
    }

    /// `bytecodeIntrinsicRegistry()`.
    pub fn bytecode_intrinsic_registry(&self) -> &BytecodeIntrinsicRegistry {
        self.bytecode_intrinsic_registry.get_or_init(|| {
            let builtin_names = self.property_names.builtin_names();
            BytecodeIntrinsicRegistry::new(|name| builtin_names.look_up_private_name(name.as_bytes())?.impl_())
        })
    }

    /// `addSourceProviderCache(SourceProvider*)`.
    pub fn add_source_provider_cache(
        &self,
        source_provider: &Rc<dyn crate::parser::source_provider::SourceProvider>,
    ) -> Rc<RefCell<crate::parser::source_provider_cache::SourceProviderCache>> {
        let key = Rc::as_ptr(source_provider) as *const () as usize;
        let mut map = self.source_provider_cache_map.borrow_mut();
        let entry = map.entry(key).or_insert_with(|| {
            let length = source_provider.source().length();
            (
                source_provider.clone(),
                Rc::new(RefCell::new(crate::parser::source_provider_cache::SourceProviderCache::create(length))),
            )
        });
        entry.1.clone()
    }

    /// `clearSourceProviderCaches()`.
    pub fn clear_source_provider_caches(&self) {
        self.source_provider_cache_map.borrow_mut().clear();
    }

    /// `isSafeToRecurse()`: o endereço de uma local aproxima o ponteiro de pilha (`currentStackPointer()`).
    pub fn is_safe_to_recurse(&self) -> bool {
        let marker = 0u8;
        (&marker as *const u8 as usize) >= self.soft_stack_limit.get()
    }

    /// `softStackLimit()`.
    pub fn soft_stack_limit(&self) -> usize {
        self.soft_stack_limit.get()
    }

    /// `setSoftStackLimit` (a parte que grava `m_softStackLimit`).
    pub fn set_soft_stack_limit(&self, limit: usize) {
        self.soft_stack_limit.set(limit);
    }

    /// `m_executingRegExp`, como identidade (0 é `nullptr`).
    pub fn executing_reg_exp(&self) -> usize {
        self.executing_reg_exp.get()
    }

    /// `m_executingRegExp = regExp`.
    pub fn set_executing_reg_exp(&self, reg_exp: usize) {
        self.executing_reg_exp.set(reg_exp);
    }

    /// `symbolRegistry()`.
    pub fn symbol_registry(&self) -> &SymbolRegistry {
        &self.symbol_registry
    }

    /// `privateSymbolRegistry()`.
    pub fn private_symbol_registry(&self) -> &SymbolRegistry {
        &self.private_symbol_registry
    }

    /// `exception()`.
    pub fn exception(&self) -> Option<Rc<Exception>> {
        self.exception.borrow().clone()
    }

    /// `clearException()`. O `traps().clearTrap(NeedExceptionHandling)` fica para o `VMTraps`.
    pub fn clear_exception(&self) {
        *self.exception.borrow_mut() = None;
    }

    /// `ensureTerminationException()`.
    pub fn ensure_termination_exception(&self) -> Rc<Exception> {
        Rc::clone(self.termination_exception.borrow_mut().get_or_insert_with(|| Rc::new(Exception { _private: () })))
    }

    /// `isTerminationException(Exception*)`.
    pub fn is_termination_exception(&self, exception: &Rc<Exception>) -> bool {
        self.termination_exception.borrow().as_ref().is_some_and(|termination| Rc::ptr_eq(termination, exception))
    }

    /// `hasPendingTerminationException()`.
    pub fn has_pending_termination_exception(&self) -> bool {
        self.exception().is_some_and(|exception| self.is_termination_exception(&exception))
    }

    /// `m_exception = exception`, o que `VM::throwException` faz no fim.
    pub fn set_exception(&self, exception: Rc<Exception>) {
        *self.exception.borrow_mut() = Some(exception);
    }

    /// `traps().deferTermination(...)`: só o contador; o caminho lento (`deferTerminationSlow`)
    /// pertence ao `VMTraps`.
    fn defer_termination(&self) {
        self.defer_termination_count.set(self.defer_termination_count.get() + 1);
    }

    /// `traps().undoDeferTermination(...)`.
    fn undo_defer_termination(&self) {
        debug_assert!(self.defer_termination_count.get() > 0);
        self.defer_termination_count.set(self.defer_termination_count.get() - 1);
    }
}

/// `class DeferTermination<DeferUntilEndOfScope>`: o `~DeferTermination` desfaz no `Drop`.
pub struct DeferTermination<'vm> {
    vm: &'vm VM,
}

impl<'vm> DeferTermination<'vm> {
    pub fn new(vm: &'vm VM) -> DeferTermination<'vm> {
        vm.defer_termination();
        DeferTermination { vm }
    }
}

impl Drop for DeferTermination<'_> {
    fn drop(&mut self) {
        self.vm.undo_defer_termination();
    }
}

/// `class TopExceptionScope` (build sem `EXCEPTION_SCOPE_VERIFICATION`, que é o caso do
/// `ExceptionScope` simples: só guarda o `VM&`).
pub struct TopExceptionScope<'vm> {
    vm: &'vm VM,
}

impl<'vm> TopExceptionScope<'vm> {
    pub fn new(vm: &'vm VM) -> TopExceptionScope<'vm> {
        TopExceptionScope { vm }
    }

    /// `exception()`.
    pub fn exception(&self) -> Option<Rc<Exception>> {
        self.vm.exception()
    }

    /// `assertNoException()`.
    pub fn assert_no_exception(&self) {
        debug_assert!(self.exception().is_none());
    }

    /// `releaseAssertNoException()`.
    pub fn release_assert_no_exception(&self) {
        assert!(self.exception().is_none());
    }

    /// `assertNoExceptionExceptTermination()`.
    pub fn assert_no_exception_except_termination(&self) {
        debug_assert!(self.exception().is_none() || self.vm.has_pending_termination_exception());
    }

    /// `releaseAssertNoExceptionExceptTermination()`.
    pub fn release_assert_no_exception_except_termination(&self) {
        assert!(self.exception().is_none() || self.vm.has_pending_termination_exception());
    }

    /// `clearException()`.
    pub fn clear_exception(&self) {
        self.vm.clear_exception();
    }

    /// `clearExceptionExceptTermination()`.
    pub fn clear_exception_except_termination(&self) -> bool {
        if self.vm.has_pending_termination_exception() {
            return false;
        }
        self.vm.clear_exception();
        true
    }
}
