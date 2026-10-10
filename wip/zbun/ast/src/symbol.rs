//! `src/ast/symbol.rs` do upstream: `Symbol`, as flags, `SlotNamespace`, `Kind` e `Use`. O `Map` de duas camadas
//! (tabela de símbolos do bundler, com os blocos `unsafe` de acesso por fonte) não entra: o transpilador de um
//! arquivo usa a tabela plana (`Vec<Symbol>`, ver `SymbolTable` em `base.rs`).
//!
//! Mudança do porte: `namespace_alias` é `Option<Box<G::NamespaceAlias>>` no lugar do `AstBox` da arena do
//! mimalloc; o tamanho e o comportamento são os mesmos (`None` na quase totalidade dos símbolos).
use core::sync::atomic::{AtomicU32, Ordering};
use std::cell::Cell;

use crate::ImportItemStatus;
use crate::base::Ref;
use crate::g as G;

pub struct Symbol {
    /// This is the name that came from the parser. Printed names may be renamed
    /// during minification or to avoid name collisions. Do not use the original
    /// name during printing.
    pub original_name: crate::StoreStr,

    /// This is used for symbols that represent items in the import clause of an
    /// ES6 import statement. These should always be referenced by EImportIdentifier
    /// instead of an EIdentifier. When this is present, the expression should
    /// be printed as a property access off the namespace instead of as a bare
    /// identifier.
    ///
    /// For correctness, this must be stored on the symbol instead of indirectly
    /// associated with the Ref for the symbol somehow. In ES6 "flat bundling"
    /// mode, re-exported symbols are collapsed using MergeSymbols() and renamed
    /// symbols from other files that end up at this symbol must be able to tell
    /// if it has a namespace alias.
    pub namespace_alias: Option<Box<G::NamespaceAlias>>,

    /// Used by the parser for single pass parsing.
    ///
    /// `Cell` because union-find (`merge`/`follow`) mutates this through
    /// `&Symbol` while other shared refs to the same table are live.
    pub link: Cell<Ref>,

    /// An estimate of the number of uses of this symbol. This is used to detect
    /// whether a symbol is used or not. For example, TypeScript imports that are
    /// unused must be removed because they are probably type-only imports. This
    /// is an estimate and may not be completely accurate due to oversights in the
    /// code. But it should always be non-zero when the symbol is used.
    pub use_count_estimate: u32,

    /// This is for generating cross-chunk imports and exports for code splitting.
    ///
    /// Do not use this directly. Use `chunkIndex()` instead.
    pub chunk_index: AtomicU32,

    /// This is used for minification. Symbols that are declared in sibling scopes
    /// can share a name. A good heuristic (from Google Closure Compiler) is to
    /// assign names to symbols from sibling scopes in declaration order. That way
    /// local variable names are reused in each global function like this, which
    /// improves gzip compression:
    ///
    ///   function x(a, b) { ... }
    ///   function y(a, b, c) { ... }
    ///
    /// The parser fills this in for symbols inside nested scopes. There are three
    /// slot namespaces: regular symbols, label symbols, and private symbols.
    ///
    /// Do not use this directly. Use `nestedScopeSlot()` instead.
    pub nested_scope_slot: u32,

    /// The kind of symbol. This is used to determine how to print the symbol
    /// and how to deal with conflicts, renaming, etc.
    pub kind: Kind,

    /// We automatically generate import items for property accesses off of
    /// namespace imports. This lets us remove the expensive namespace imports
    /// while bundling in many cases, replacing them with a cheap import item
    /// instead:
    ///
    ///   import * as ns from 'path'
    ///   ns.foo()
    ///
    /// That can often be replaced by this, which avoids needing the namespace:
    ///
    ///   import {foo} from 'path'
    ///   foo()
    ///
    /// However, if the import is actually missing then we don't want to report a
    /// compile-time error like we do for real import items. This status lets us
    /// avoid this. We also need to be able to replace such import items with
    /// undefined, which this status is also used for.
    pub import_item_status: ImportItemStatus,

    /// Packed boolean state: see [`SymbolFlags`].
    pub flags: SymbolFlags,
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Eq, PartialEq, Default, Debug)]
    pub struct SymbolFlags: u8 {
        const MUST_START_WITH_CAPITAL_LETTER_FOR_JSX = 1 << 1;

        /// Certain symbols must not be renamed or minified. For example, the
        /// "arguments" variable is declared by the runtime for every function.
        /// Renaming can also break any identifier used inside a "with" statement.
        const MUST_NOT_BE_RENAMED = 1 << 2;

        const REMOVE_OVERWRITTEN_FUNCTION_DECLARATION = 1 << 4;

        /// The file assigns this variable after its declaration (or a mapped
        /// `arguments` object can). Set on the root of the symbol's link
        /// chain. Read by HMR live bindings and the printer's same-target fold.
        const HAS_BEEN_ASSIGNED_TO = 1 << 5;

        /// An import item (`ns.name`) or a lifted export that a use calls as a method.
        const CALLED_AS_METHOD = 1 << 6;

        /// A call of this function declaration or lifted export ignores `this`.
        const CALL_IGNORES_THIS = 1 << 7;
    }
}

macro_rules! symbol_flag_accessors {
    ($($getter:ident, $setter:ident => $flag:ident;)*) => {
        impl Symbol {
            $(
                #[inline]
                pub fn $getter(&self) -> bool {
                    self.flags.contains(SymbolFlags::$flag)
                }
                #[inline]
                pub fn $setter(&mut self, v: bool) {
                    self.flags.set(SymbolFlags::$flag, v)
                }
            )*
        }
    };
}

symbol_flag_accessors! {
    must_start_with_capital_letter_for_jsx, set_must_start_with_capital_letter_for_jsx => MUST_START_WITH_CAPITAL_LETTER_FOR_JSX;
    must_not_be_renamed, set_must_not_be_renamed => MUST_NOT_BE_RENAMED;
    remove_overwritten_function_declaration, set_remove_overwritten_function_declaration => REMOVE_OVERWRITTEN_FUNCTION_DECLARATION;
    has_been_assigned_to, set_has_been_assigned_to => HAS_BEEN_ASSIGNED_TO;
    called_as_method, set_called_as_method => CALLED_AS_METHOD;
    call_ignores_this, set_call_ignores_this => CALL_IGNORES_THIS;
}

const INVALID_CHUNK_INDEX: u32 = u32::MAX;
pub const INVALID_NESTED_SCOPE_SLOT: u32 = u32::MAX;

impl Default for Symbol {
    fn default() -> Self {
        Self {
            original_name: crate::StoreStr::EMPTY,
            namespace_alias: None,
            link: Cell::new(Ref::NONE),
            use_count_estimate: 0,
            chunk_index: AtomicU32::new(INVALID_CHUNK_INDEX),
            nested_scope_slot: INVALID_NESTED_SCOPE_SLOT,
            kind: Kind::Other,
            import_item_status: ImportItemStatus::None,
            flags: SymbolFlags::empty(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, enum_map::Enum)]
pub enum SlotNamespace {
    MustNotBeRenamed,
    Default,
    Label,
    PrivateName,
    MangledProp,
}

// Inherent associated types are nightly-only; expose as a free alias.
pub(crate) type SlotNamespaceCountsArray = enum_map::EnumMap<SlotNamespace, u32>;

/// A tabela de símbolos de um arquivo; no upstream é um `ArenaVec`.
pub type List = Vec<Symbol>;

impl Symbol {
    /// This is for generating cross-chunk imports and exports for code splitting.
    #[inline]
    pub fn chunk_index(&self) -> Option<u32> {
        let i = self.chunk_index.load(Ordering::Relaxed);
        if i == INVALID_CHUNK_INDEX {
            None
        } else {
            Some(i)
        }
    }

    #[inline]
    pub fn nested_scope_slot(&self) -> Option<u32> {
        let i = self.nested_scope_slot;
        if i == INVALID_NESTED_SCOPE_SLOT {
            None
        } else {
            Some(i)
        }
    }

    pub fn slot_namespace(&self) -> SlotNamespace {
        let kind = self.kind;

        if kind == Kind::Unbound || self.must_not_be_renamed() {
            return SlotNamespace::MustNotBeRenamed;
        }

        if kind.is_private() {
            return SlotNamespace::PrivateName;
        }

        match kind {
            // Kind::MangledProp => SlotNamespace::MangledProp,
            Kind::Label => SlotNamespace::Label,
            _ => SlotNamespace::Default,
        }
    }

    #[inline]
    pub fn has_link(&self) -> bool {
        self.link.get().is_valid()
    }

    /// An import item the linker merged into the export it names. A local
    /// that a hoisting merge links has no import item status.
    #[inline]
    pub fn is_bound_import_item(&self) -> bool {
        self.import_item_status != ImportItemStatus::None && self.has_link()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Kind {
    /// An unbound symbol is one that isn't declared in the file it's referenced
    /// in. For example, using "window" without declaring it will be unbound.
    Unbound,

    /// This has special merging behavior. You're allowed to re-declare these
    /// symbols more than once in the same scope. These symbols are also hoisted
    /// out of the scope they are declared in to the closest containing function
    /// or module scope. These are the symbols with this kind:
    ///
    /// - Function arguments
    /// - Function statements
    /// - Variables declared using "var"
    Hoisted,
    HoistedFunction,

    /// There's a weird special case where catch variables declared using a simple
    /// identifier (i.e. not a binding pattern) block hoisted variables instead of
    /// becoming an error:
    ///
    ///   var e = 0;
    ///   try { throw 1 } catch (e) {
    ///     print(e) // 1
    ///     var e = 2
    ///     print(e) // 2
    ///   }
    ///   print(e) // 0 (since the hoisting stops at the catch block boundary)
    ///
    /// However, other forms are still a syntax error:
    ///
    ///   try {} catch (e) { let e }
    ///   try {} catch ({e}) { var e }
    ///
    /// This symbol is for handling this weird special case.
    CatchIdentifier,

    /// Generator and async functions are not hoisted, but still have special
    /// properties such as being able to overwrite previous functions with the
    /// same name
    GeneratorOrAsyncFunction,

    /// This is the special "arguments" variable inside functions
    Arguments,

    /// Classes can merge with TypeScript namespaces.
    Class,

    /// A class-private identifier (i.e. "#foo").
    PrivateField,
    PrivateMethod,
    PrivateGet,
    PrivateSet,
    PrivateGetSetPair,
    PrivateStaticField,
    PrivateStaticMethod,
    PrivateStaticGet,
    PrivateStaticSet,
    PrivateStaticGetSetPair,

    /// Labels are in their own namespace
    Label,

    /// TypeScript enums can merge with TypeScript namespaces and other TypeScript
    /// enums.
    TsEnum,

    /// TypeScript namespaces can merge with classes, functions, TypeScript enums,
    /// and other TypeScript namespaces.
    TsNamespace,

    /// In TypeScript, imports are allowed to silently collide with symbols within
    /// the module. Presumably this is because the imports may be type-only.
    /// Import statement namespace references should NOT have this set.
    Import,

    /// Assigning to a "const" symbol will throw a TypeError at runtime
    Constant,

    // CSS identifiers that are renamed to be unique to the file they are in
    LocalCss,

    /// This annotates all other symbols that don't have special behavior.
    Other,
}

impl Kind {
    #[inline]
    pub(crate) fn is_private(self) -> bool {
        (self as u8) >= (Kind::PrivateField as u8)
            && (self as u8) <= (Kind::PrivateStaticGetSetPair as u8)
    }

    #[inline]
    pub fn is_hoisted(self) -> bool {
        matches!(self, Kind::Hoisted | Kind::HoistedFunction)
    }

    #[inline]
    pub fn is_hoisted_or_function(self) -> bool {
        matches!(
            self,
            Kind::Hoisted | Kind::HoistedFunction | Kind::GeneratorOrAsyncFunction
        )
    }

    #[inline]
    pub fn is_function(self) -> bool {
        matches!(self, Kind::HoistedFunction | Kind::GeneratorOrAsyncFunction)
    }
}

/// A part's uses of one symbol: an estimated count, plus whether any of them
/// was added without its scope being recorded in `Ast::scope_uses` (by the
/// linker or a transform), in which case the renamer must assume the symbol
/// may be printed in any scope of the part. Packed into one word because
/// every part holds one per referenced symbol.
#[derive(Default, Clone, Copy)]
pub struct Use(u32);

impl Use {
    const UNSCOPED: u32 = 1 << 31;

    /// `count` uses whose scope is not on record.
    pub const fn unscoped(count: u32) -> Use {
        Use(if count >= Self::UNSCOPED {
            u32::MAX
        } else {
            count | Self::UNSCOPED
        })
    }

    #[inline]
    pub const fn count_estimate(self) -> u32 {
        self.0 & !Self::UNSCOPED
    }

    #[inline]
    pub const fn has_unscoped(self) -> bool {
        self.0 & Self::UNSCOPED != 0
    }

    /// The parser's `record_usage`: the use's scope is on record.
    #[inline]
    pub fn add_scoped(&mut self, count: u32) {
        debug_assert!(self.count_estimate() + count < Self::UNSCOPED);
        self.0 += count;
    }

    #[inline]
    pub fn subtract(&mut self, count: u32) {
        self.0 = self.count_estimate().saturating_sub(count) | (self.0 & Self::UNSCOPED);
    }

    pub fn merge(&mut self, other: Use) {
        let count = self.count_estimate().saturating_add(other.count_estimate());
        self.0 = count.min(Self::UNSCOPED - 1) | ((self.0 | other.0) & Self::UNSCOPED);
    }
}

impl Symbol {
    #[allow(dead_code)]
    pub(crate) fn merge_contents_with(&mut self, old: &mut Symbol) {
        self.use_count_estimate += old.use_count_estimate;
        if old.must_not_be_renamed() {
            self.original_name = old.original_name;
            self.set_must_not_be_renamed(true);
        }

        // TODO: MustStartWithCapitalLetterForJSX
    }
}
