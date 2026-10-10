//! `bun_js_parser`: o lexer, o parser e as transformações do bun 1.4.2, portados para Rust seguro.

pub mod lexer;

#[cfg(test)]
mod lexer_tests;

// O parser copiado do upstream (README, "Próximos passos", passo 2) fica atrás da feature `parser` até compilar:
// `cargo check -p bun_js_parser --features parser` mede o que falta. O `Macro` (FFI com o `bun_js_parser_jsc`) e o
// `react_compiler_host` ficam fora por decisão da coordenação.
#[cfg(feature = "parser")]
pub mod error;
#[cfg(feature = "parser")]
pub use error::Error;
#[cfg(feature = "parser")]
pub use error::Result as CrateResult;

#[cfg(feature = "parser")]
pub mod parser;
// Re-export parser-helper types at crate root so p.rs can `use crate::{...}`.
#[cfg(feature = "parser")]
pub use parser::*;

#[cfg(feature = "parser")]
pub(crate) mod fold;
#[cfg(feature = "parser")]
pub mod lower;
#[cfg(feature = "parser")]
pub mod p;
#[cfg(feature = "parser")]
pub mod parse;
#[cfg(feature = "parser")]
pub(crate) mod repl_transforms;
#[cfg(feature = "parser")]
pub mod scan;
#[cfg(feature = "parser")]
pub mod typescript;
#[cfg(feature = "parser")]
pub mod visit;

#[cfg(feature = "parser")]
pub use p::P;
#[cfg(feature = "parser")]
pub use parse::parse_entry::{Options as ParserOptions, Parser};

/// Only the JSC-free constants. No upstream, o `MacroContext` leva a um runtime de JS por FFI
/// (`bun_js_parser_jsc`), que fica fora do porte por decisão da coordenação. Aqui ele é um tipo sem valor: o
/// `options.macro_context` é sempre `None`, e os caminhos que chamariam a macro são inalcançáveis pelo tipo, sem
/// fachada que finja rodar uma.
#[allow(non_snake_case)]
pub mod Macro {
    pub const NAMESPACE: &[u8] = b"macro";
    pub const NAMESPACE_WITH_COLON: &[u8] = b"macro:";

    #[inline]
    pub fn is_macro_path(str_: &[u8]) -> bool {
        str_.starts_with(NAMESPACE_WITH_COLON)
    }

    /// Values are owned (`Box<[u8]>`).
    pub type MacroRemapEntry = bun_collections::StringArrayHashMap<Box<[u8]>>;

    pub enum MacroContext {}

    #[cfg(feature = "parser")]
    impl MacroContext {
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn call(
            &mut self,
            _import_record_path: &[u8],
            _source_dir: &[u8],
            _log: &mut bun_ast::Log,
            _source: &bun_ast::Source,
            _import_range: bun_ast::Range,
            _caller: bun_ast::Expr,
            _function_name: &[u8],
        ) -> Result<bun_ast::Expr, crate::Error> {
            match *self {}
        }
        pub(crate) fn get_remap(&self, _path: &[u8]) -> Option<&'static MacroRemapEntry> {
            match *self {}
        }
    }
}

#[cfg(feature = "parser")]
use bun_ast::NodeStore as _;
use bun_ast::{Ast, Ref};

// NOTE: shadows the prelude `Result` for this module. All error-union return
// types in this file are spelled `core::result::Result<T, E>` to disambiguate.
//
// PERF NOTE: `bun_ast::Ast` is ~1 KB. Boxing the `Ast` variant collapses `Result` to 16 B so only a thin pointer
// is moved up the stack.
// No porte, o `Ast` não tem tempo de vida (os nós são índices no `Store`), então `Result` também perde o `'a`.
#[cfg(feature = "parser")]
pub enum Result {
    AlreadyBundled(AlreadyBundled),
    Cached,
    Ast(Box<Ast>),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AlreadyBundled {
    Bun,
    BunCjs,
    Bytecode,
    BytecodeCjs,
}

/// `impl EqlParser for P`: moved out of `bun_ast::expr` (next to `P`).
#[cfg(feature = "parser")]
impl<'a, const IS_TS: bool, const SCAN: bool> bun_ast::expr::EqlParser for crate::p::P<'a, IS_TS, SCAN> {
    #[inline]
    fn store_mut(&mut self) -> &mut bun_ast::Store {
        &mut self.store
    }
    #[inline]
    fn module_ref(&self) -> Ref {
        self.module_ref
    }
}

pub mod defines_table;

// from bun_bundler::defines: canonical `Define` / `DefineData` / `DotDefine` live here so the parser and the
// bundler share one nominal type. The pure-global fallback table also lives at this tier (`defines_table`).
//
// No porte, o `Define` é dono do `store` onde moram os nós dos valores (`DefineData.value`), e o `Store` de cada
// arquivo nasce como `define.store.clone()`: os índices dos valores valem em qualquer arquivo sem conversão
// (README, "`Define` com um `Store` por arquivo").
pub mod defines {
    use bun_collections::{StringArrayHashMap, StringHashMap};
    use bun_core::strings;

    use bun_ast::E;
    use bun_ast::ExprData;
    use bun_ast::{NodeStore, Store};

    pub type RawDefines = StringArrayHashMap<Box<[u8]>>;
    pub type UserDefines = StringHashMap<DefineData>;
    pub type UserDefinesArray = StringArrayHashMap<DefineData>;

    pub type IdentifierDefine = DefineData;

    #[derive(Clone)]
    pub struct DotDefine {
        // Owned part strings (small, allocated once at startup).
        pub parts: Vec<Box<[u8]>>,
        pub data: DefineData,
    }

    /// Bit-packed flags (LSB-first): 3 padding bits, `valueless`,
    /// `can_be_removed_if_unused`, `call_can_be_unwrapped_if_unused`
    /// (`E.CallUnwrap`, 2 bits), `method_call_must_be_replaced_with_undefined`.
    #[repr(transparent)]
    #[derive(Clone, Copy, Default, PartialEq, Eq)]
    pub struct Flags(u8);

    impl Flags {
        const VALUELESS_SHIFT: u8 = 3;
        const CAN_BE_REMOVED_SHIFT: u8 = 4;
        const CALL_UNWRAP_SHIFT: u8 = 5;
        const CALL_UNWRAP_MASK: u8 = 0b11 << Self::CALL_UNWRAP_SHIFT;
        const METHOD_CALL_UNDEF_SHIFT: u8 = 7;

        #[inline]
        pub(crate) const fn valueless(self) -> bool {
            (self.0 >> Self::VALUELESS_SHIFT) & 1 != 0
        }
        #[inline]
        pub(crate) fn set_valueless(&mut self, v: bool) {
            self.0 = (self.0 & !(1 << Self::VALUELESS_SHIFT)) | ((v as u8) << Self::VALUELESS_SHIFT);
        }
        #[inline]
        pub(crate) const fn can_be_removed_if_unused(self) -> bool {
            (self.0 >> Self::CAN_BE_REMOVED_SHIFT) & 1 != 0
        }
        #[inline]
        pub(crate) fn set_can_be_removed_if_unused(&mut self, v: bool) {
            self.0 = (self.0 & !(1 << Self::CAN_BE_REMOVED_SHIFT)) | ((v as u8) << Self::CAN_BE_REMOVED_SHIFT);
        }
        #[inline]
        pub(crate) fn call_can_be_unwrapped_if_unused(self) -> E::CallUnwrap {
            // 2-bit field; `E::CallUnwrap` only has discriminants 0/1/2, so
            // an explicit match keeps bit-pattern 3 sound.
            match (self.0 & Self::CALL_UNWRAP_MASK) >> Self::CALL_UNWRAP_SHIFT {
                1 => E::CallUnwrap::IfUnused,
                2 => E::CallUnwrap::IfUnusedAndToStringSafe,
                _ => E::CallUnwrap::Never,
            }
        }
        #[inline]
        pub(crate) fn set_call_can_be_unwrapped_if_unused(&mut self, v: E::CallUnwrap) {
            self.0 = (self.0 & !Self::CALL_UNWRAP_MASK) | (((v as u8) & 0b11) << Self::CALL_UNWRAP_SHIFT);
        }
        #[inline]
        pub(crate) const fn method_call_must_be_replaced_with_undefined(self) -> bool {
            (self.0 >> Self::METHOD_CALL_UNDEF_SHIFT) & 1 != 0
        }
        #[inline]
        pub(crate) fn set_method_call_must_be_replaced_with_undefined(&mut self, v: bool) {
            self.0 = (self.0 & !(1 << Self::METHOD_CALL_UNDEF_SHIFT)) | ((v as u8) << Self::METHOD_CALL_UNDEF_SHIFT);
        }
        pub fn new(
            valueless: bool,
            can_be_removed_if_unused: bool,
            call_can_be_unwrapped_if_unused: E::CallUnwrap,
            method_call_must_be_replaced_with_undefined: bool,
        ) -> Self {
            let mut f = Flags(0);
            f.set_valueless(valueless);
            f.set_can_be_removed_if_unused(can_be_removed_if_unused);
            f.set_call_can_be_unwrapped_if_unused(call_can_be_unwrapped_if_unused);
            f.set_method_call_must_be_replaced_with_undefined(method_call_must_be_replaced_with_undefined);
            f
        }
    }

    /// `value` guarda índices no `store` do `Define` que o contém. Sem ponteiros, `DefineData` é `Send + Sync` por
    /// construção e os `unsafe impl` do upstream saem.
    #[derive(Clone)]
    pub struct DefineData {
        pub value: ExprData,
        // The `RawDefines` value bytes are owned (`Box<[u8]>`), so borrowing
        // would be a use-after-free once the `RawDefines` map is dropped after
        // `Define::init`. Own the bytes here instead: these are tiny
        // startup-time copies.
        pub original_name: Option<Box<[u8]>>,
        pub flags: Flags,
    }

    impl Default for DefineData {
        fn default() -> Self {
            Self { value: ExprData::EMissing(E::Missing), original_name: None, flags: Flags::default() }
        }
    }

    /// Named-init shim.
    #[derive(Clone, Copy)]
    pub struct Options<'a> {
        pub original_name: Option<&'a [u8]>,
        pub value: ExprData,
        pub valueless: bool,
        pub can_be_removed_if_unused: bool,
        pub call_can_be_unwrapped_if_unused: E::CallUnwrap,
        pub method_call_must_be_replaced_with_undefined: bool,
    }
    impl<'a> Default for Options<'a> {
        fn default() -> Self {
            Self {
                original_name: None,
                value: ExprData::EMissing(E::Missing),
                valueless: false,
                can_be_removed_if_unused: false,
                call_can_be_unwrapped_if_unused: E::CallUnwrap::Never,
                method_call_must_be_replaced_with_undefined: false,
            }
        }
    }

    impl DefineData {
        pub fn init(options: Options<'_>) -> DefineData {
            DefineData {
                value: options.value,
                flags: Flags::new(
                    options.valueless,
                    options.can_be_removed_if_unused,
                    options.call_can_be_unwrapped_if_unused,
                    options.method_call_must_be_replaced_with_undefined,
                ),
                original_name: options.original_name.map(Box::<[u8]>::from),
            }
        }

        #[inline]
        pub(crate) fn original_name(&self) -> Option<&[u8]> {
            match &self.original_name {
                Some(name) if !name.is_empty() => Some(name.as_ref()),
                _ => None,
            }
        }

        /// True if accessing this value is known to not have any side effects.
        #[inline]
        pub(crate) fn can_be_removed_if_unused(&self) -> bool {
            self.flags.can_be_removed_if_unused()
        }
        /// True if a call to this value is known to not have any side effects.
        #[inline]
        pub(crate) fn call_can_be_unwrapped_if_unused(&self) -> E::CallUnwrap {
            self.flags.call_can_be_unwrapped_if_unused()
        }
        #[inline]
        pub(crate) fn method_call_must_be_replaced_with_undefined(&self) -> bool {
            self.flags.method_call_must_be_replaced_with_undefined()
        }
        #[inline]
        pub(crate) fn valueless(&self) -> bool {
            self.flags.valueless()
        }

        pub fn init_boolean(value: bool) -> DefineData {
            let mut flags = Flags::default();
            flags.set_can_be_removed_if_unused(true);
            DefineData { value: ExprData::EBoolean(E::Boolean { value }), flags, ..Default::default() }
        }

        /// No upstream o nó é um `&'static E::EString`; aqui ele é gravado no `store` do `Define`.
        pub fn init_static_string(store: &mut Store, str: E::EString) -> DefineData {
            let mut flags = Flags::default();
            flags.set_can_be_removed_if_unused(true);
            DefineData { value: ExprData::EString(store.alloc(str)), flags, ..Default::default() }
        }

        pub(crate) fn merge(a: &DefineData, b: DefineData) -> DefineData {
            DefineData {
                value: b.value,
                flags: Flags::new(
                    // TODO: investigate if this is correct. This is what it was before.
                    a.method_call_must_be_replaced_with_undefined() || b.method_call_must_be_replaced_with_undefined(),
                    a.can_be_removed_if_unused(),
                    a.call_can_be_unwrapped_if_unused(),
                    a.method_call_must_be_replaced_with_undefined() || b.method_call_must_be_replaced_with_undefined(),
                ),
                original_name: b.original_name,
            }
        }
    }

    #[derive(Default)]
    pub struct Define {
        pub identifiers: StringHashMap<IdentifierDefine>,
        pub dots: StringHashMap<Vec<DotDefine>>,
        /// Which `DotFilter::bit`s the keys of `dots` set. The visitor asks
        /// about every property access; nearly all miss, and this says so
        /// without hashing the name.
        pub dots_filter: DotFilter,
        pub drop_debugger: bool,
        /// `hash_user_inputs` of this table's inputs, for the runtime transpiler cache key.
        pub user_hash: Option<u64>,
        /// O dono dos nós dos valores. O `Store` de cada arquivo nasce como `store.clone()`; com um `--define` de
        /// objeto grande essa cópia vira custo por arquivo, e aí a cópia no uso (a opção (A) do README) volta à mesa.
        pub store: Store,
    }

    #[derive(Clone)]
    pub struct DotFilter([u64; 64]);

    impl Default for DotFilter {
        fn default() -> Self {
            DotFilter([0; 64])
        }
    }

    impl DotFilter {
        #[inline]
        fn bit(name: &[u8]) -> usize {
            let (first, last) = match name {
                [] => (0, 0),
                [first, .., last] | [first @ last] => (*first as usize, *last as usize),
            };
            (name.len().wrapping_mul(131) ^ first.wrapping_mul(31) ^ last.wrapping_mul(7)) & 4095
        }

        #[inline]
        pub fn insert(&mut self, name: &[u8]) {
            let bit = Self::bit(name);
            self.0[bit / 64] |= 1 << (bit % 64);
        }

        #[inline]
        pub fn may_contain(&self, name: &[u8]) -> bool {
            let bit = Self::bit(name);
            self.0[bit / 64] & (1 << (bit % 64)) != 0
        }
    }

    impl Define {
        /// The dot defines whose last part is `name` (`NODE_ENV` for
        /// `process.env.NODE_ENV`).
        #[inline]
        pub fn dots_for(&self, name: &[u8]) -> Option<&Vec<DotDefine>> {
            if !self.dots_filter.may_contain(name) {
                return None;
            }
            self.dots.get(name)
        }

        /// Order-independent, length-prefixed hash of the three inputs. `None` when all are empty.
        pub fn hash_user_inputs<'i>(
            defines: impl IntoIterator<Item = (&'i [u8], &'i [u8])>,
            env_defines: impl IntoIterator<Item = (&'i [u8], &'i [u8])>,
            drop: impl IntoIterator<Item = &'i [u8]>,
        ) -> Option<u64> {
            let mut defines: Vec<(&[u8], &[u8])> = defines.into_iter().collect();
            let mut env_defines: Vec<(&[u8], &[u8])> = env_defines.into_iter().collect();
            let mut drop: Vec<&[u8]> = drop.into_iter().filter(|item| !item.is_empty()).collect();
            if defines.is_empty() && env_defines.is_empty() && drop.is_empty() {
                return None;
            }
            defines.sort_unstable();
            env_defines.sort_unstable();
            drop.sort_unstable();
            drop.dedup();

            let mut hasher = bun_wyhash::Wyhash::init(0);
            let mut update = |bytes: &[u8]| {
                hasher.update(&(bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            };
            // Separate sections: `init` lets a later env pair override a user pair.
            for pairs in [defines, env_defines] {
                update(&(pairs.len() as u64).to_le_bytes());
                for (key, value) in pairs {
                    update(key);
                    update(value);
                }
            }
            update(&(drop.len() as u64).to_le_bytes());
            for item in drop {
                update(item);
            }
            Some(hasher.final_())
        }

        pub(crate) fn for_identifier(&self, name: &[u8]) -> Option<&IdentifierDefine> {
            if let Some(data) = self.identifiers.get(name) {
                return Some(data);
            }
            crate::defines_table::lookup_pure_global_identifier(name).map(|v| v.value())
        }

        pub fn insert_from_iterator<'a, I>(&mut self, iter: I) -> Result<(), bun_alloc::AllocError>
        where
            I: Iterator<Item = (&'a [u8], &'a DefineData)>,
        {
            for (key, value) in iter {
                self.insert(key, value.clone())?;
            }
            Ok(())
        }

        pub fn insert(&mut self, key: &[u8], value: DefineData) -> Result<(), bun_alloc::AllocError> {
            // If it has a dot, then it's a DotDefine. e.g. process.env.NODE_ENV
            if let Some(last_dot) = strings::last_index_of_char(key, b'.') {
                let tail = &key[last_dot + 1..key.len()];
                let remainder = &key[0..last_dot];
                let count = strings::count_char(remainder, b'.') + 1;
                let mut parts: Vec<Box<[u8]>> = Vec::with_capacity(count + 1);
                for split in strings::split(remainder, b".") {
                    parts.push(Box::from(split));
                }
                parts.push(Box::from(tail));

                self.dots_filter.insert(tail);
                if let Some(existing) = self.dots.get_mut(tail) {
                    for part in existing.iter_mut() {
                        if are_parts_equal(&part.parts, &parts) {
                            part.data = DefineData::merge(&part.data, value);
                            return Ok(());
                        }
                    }
                    existing.push(DotDefine { data: value, parts });
                    return Ok(());
                }

                self.dots.put(tail, vec![DotDefine { data: value, parts }])?;
            } else {
                // e.g. IS_BROWSER
                self.identifiers.put(key, value)?;
            }
            Ok(())
        }
    }

    pub fn are_parts_equal(a: &[Box<[u8]>], b: &[Box<[u8]>]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        for i in 0..a.len() {
            if !strings::eql(&a[i], &b[i]) {
                return false;
            }
        }
        true
    }
}
pub use defines::{Define, DefineData};

// from bun_js_printer::renamer: only the slot-assignment helpers the parser calls live here; the full
// `NumberRenamer`/`MinifyRenamer` machinery stays in `bun_js_printer`. No porte, os filhos de um escopo são índices
// no `Store`, que entra como parâmetro no lugar da arena.
#[cfg(feature = "parser")]
pub mod renamer {
    use bun_ast::SlotCounts;
    use bun_ast::Store;
    use bun_ast::scope::Scope;
    use bun_ast::symbol::{INVALID_NESTED_SCOPE_SLOT, SlotNamespace, Symbol};

    pub(crate) fn assign_nested_scope_slots(store: &Store, module_scope: &Scope, symbols: &mut [Symbol]) -> SlotCounts {
        let mut slot_counts = SlotCounts::default();
        let mut sorted_members: Vec<u32> = Vec::new();

        // Temporarily set the nested scope slots of top-level symbols to valid so
        // they aren't renamed in nested scopes. This prevents us from accidentally
        // assigning nested scope slots to variables declared using "var" in a nested
        // scope that are actually hoisted up to the module scope to become a top-
        // level symbol.
        const VALID_SLOT: u32 = 0;
        for member in module_scope.members.values() {
            symbols[member.ref_.inner_index() as usize].nested_scope_slot = VALID_SLOT;
        }
        for ref_ in module_scope.generated.iter() {
            symbols[ref_.inner_index() as usize].nested_scope_slot = VALID_SLOT;
        }

        for &child in module_scope.children.iter() {
            slot_counts.union_max(assign_nested_scope_slots_helper(
                store,
                &mut sorted_members,
                &store[child],
                symbols,
                SlotCounts::default(),
            ));
        }

        // Then set the nested scope slots of top-level symbols back to zero. Top-
        // level symbols are not supposed to have nested scope slots.
        for member in module_scope.members.values() {
            symbols[member.ref_.inner_index() as usize].nested_scope_slot = INVALID_NESTED_SCOPE_SLOT;
        }
        for ref_ in module_scope.generated.iter() {
            symbols[ref_.inner_index() as usize].nested_scope_slot = INVALID_NESTED_SCOPE_SLOT;
        }

        slot_counts
    }

    fn assign_nested_scope_slots_helper(
        store: &Store,
        sorted_members: &mut Vec<u32>,
        scope: &Scope,
        symbols: &mut [Symbol],
        slot_to_copy: SlotCounts,
    ) -> SlotCounts {
        let mut slot = slot_to_copy;

        // Sort member map keys for determinism
        {
            sorted_members.clear();
            sorted_members.reserve(scope.members.len());
            for member in scope.members.values() {
                sorted_members.push(member.ref_.inner_index());
            }
            sorted_members.sort_unstable();

            // Assign slots for this scope's symbols. Only do this if the slot is
            // not already assigned. Nested scopes have copies of symbols from parent
            // scopes and we want to use the slot from the parent scope, not child scopes.
            for &inner_index in sorted_members.iter() {
                let symbol = &mut symbols[inner_index as usize];
                let ns = symbol.slot_namespace();
                if ns != SlotNamespace::MustNotBeRenamed && symbol.nested_scope_slot().is_none() {
                    symbol.nested_scope_slot = slot.slots[ns];
                    slot.slots[ns] += 1;
                }
            }
        }

        for ref_ in scope.generated.iter() {
            let symbol = &mut symbols[ref_.inner_index() as usize];
            let ns = symbol.slot_namespace();
            if ns != SlotNamespace::MustNotBeRenamed && symbol.nested_scope_slot().is_none() {
                symbol.nested_scope_slot = slot.slots[ns];
                slot.slots[ns] += 1;
            }
        }

        // Labels are always declared in a nested scope, so we don't need to check.
        if let Some(ref_) = scope.label_ref.to_nullable() {
            let symbol = &mut symbols[ref_.inner_index() as usize];
            let ns = SlotNamespace::Label;
            symbol.nested_scope_slot = slot.slots[ns];
            slot.slots[ns] += 1;
        }

        // Assign slots for the symbols of child scopes
        let mut slot_counts = slot;
        for &child in scope.children.iter() {
            slot_counts.union_max(assign_nested_scope_slots_helper(store, sorted_members, &store[child], symbols, slot));
        }

        slot_counts
    }
}

#[cfg(test)]
mod defines_tests;
