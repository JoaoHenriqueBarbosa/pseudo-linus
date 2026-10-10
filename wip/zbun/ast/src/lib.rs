//! Porte para Rust seguro do crate `bun_ast` do bun 1.4.2 (`src/ast` do tag `bun-v1.4.2`). Os módulos mantêm o
//! nome e a estrutura do upstream para a ressincronização; o README de `wip/zbun` diz quais já foram portados e o
//! que mudou em cada um.
#![forbid(unsafe_code)]

use core::fmt;

/// O `#[derive(bun_core::EnumTag)]` com `#[enum_tag(existing = Tag)]` do upstream: declara o enum de dados e o
/// `tag()` que devolve a variante de mesmo nome no enum de tags já existente. O padrão `Variante { .. }` casa tanto
/// as variantes com carga quanto as sem.
macro_rules! enum_tag {
    (
        $(#[$meta:meta])*
        pub enum $name:ident: $tag:ident {
            $($(#[$vmeta:meta])* $variant:ident $(($payload:ty))?),* $(,)?
        }
    ) => {
        $(#[$meta])*
        pub enum $name {
            $($(#[$vmeta])* $variant $(($payload))?,)*
        }

        impl $name {
            #[inline]
            pub fn tag(&self) -> $tag {
                match self {
                    $($name::$variant { .. } => $tag::$variant,)*
                }
            }
        }
    };
}

/// Os acessores de campo do upstream (`data.e_string()`, `data.s_function()`): devolvem o índice (`Copy`). As
/// versões `_mut` recebem o `Store`, que é quem tem o nó.
macro_rules! data_accessors {
    ($data:ident; $($get:ident $(/ $get_mut:ident)? => $variant:ident: $ty:ty),* $(,)?) => {
        impl $data {
            $(
                #[inline]
                pub fn $get(&self) -> Option<$crate::StoreRef<$ty>> {
                    if let $data::$variant(v) = *self { Some(v) } else { None }
                }
                $(
                    #[inline]
                    pub fn $get_mut<'s>(&self, store: &'s mut $crate::store::Store) -> Option<&'s mut $ty> {
                        self.$get().map(|v| &mut store[v])
                    }
                )?
            )*
        }
    };
}

pub mod b;
pub mod base;
pub mod binding;
pub mod e;
pub mod expr;
pub mod g;
pub mod import_record;
pub mod lexer_log;
pub mod lexer_tables;
pub mod loader;
pub mod logger;
pub mod new_store;
pub mod nodes;
pub mod op;
pub mod s;
pub mod scope;
pub mod stmt;
pub mod store;
pub mod symbol;
pub mod target;
pub mod ts;
pub mod use_directive;

pub use b as B;
pub use base::{Index, IndexInt};
pub use binding::Binding;
pub use e as E;
pub use e::CallUnwrap as CanBeUnwrapped;
pub use expr::{Data as ExprData, Expr, PrimitiveType as KnownPrimitive, Tag as ExprTag};
pub use g as G;
pub use g::NamespaceAlias;
pub use import_record::{Flags as ImportRecordFlags, ImportRecord, Tag as ImportRecordTag};
pub use lexer_log::LexerLog;
pub use loader::{Loader, LoaderHashTable, SideEffects};
pub use logger::{
    AddErrorOptions, Data, IntoText, Kind, Level, Location, Log, Msg, Path, Source, alloc_print, range_data,
};
pub use new_store::{NodeStore, Stored};
pub use nodes::{
    ArrayBinding, AssignTarget, BindingNodeIndex, Case, Catch, ClauseItem, EnumValue, ExprNodeIndex, ExprNodeList,
    Finally, ImportItemStatus, LocRef, OptionalChain, Span, StmtNodeIndex, StmtNodeList, StmtOrExpr, StoreRef,
    StoreSlice, StoreStr, StrictModeKind,
};
pub use scope::Scope;

/// `Part.{SymbolUseMap, SymbolPropertyUseMap, List}`: módulo para `crate::part::{SymbolUseMap, List}` resolver.
pub mod part {
    pub use crate::nodes::{
        Part, PartList as List, PartSymbolPropertyUseMap as SymbolPropertyUseMap, PartSymbolUseMap as SymbolUseMap,
    };
}
pub use nodes::{
    DeclaredSymbol, DeclaredSymbolList, Dependency, ExportsKind, InlinedEnumValue, InlinedEnumValueDecoded,
    NamedExport, NamedImport, Part, PartTag, PropertyUse, TlaCheck,
};
pub use ts::{TSNamespaceMember, TSNamespaceMemberMap, TSNamespaceScope};
pub use use_directive::UseDirective;

/// In range for every position in a source that passed [`Source::check_parseable_len`].
#[inline]
pub fn usize2loc(loc: usize) -> Loc {
    Loc { start: i32::try_from(loc).expect("int cast") }
}
pub use op as Op;
pub use op::Code as OpCode;
pub use s as S;
pub use s::Kind as LocalKind;
pub use stmt::{Data as StmtData, Stmt, Tag as StmtTag};
pub use store::Store;
pub use symbol::Symbol;
pub use target::Target;

/// `flags`: bitset enums on AST nodes (`G.rs`/`B.rs` import `crate::flags`).
pub mod flags {
    use enumset::{EnumSet, EnumSetType};

    #[derive(EnumSetType, Debug)]
    pub enum JSXElement {
        IsKeyAfterSpread,
    }
    pub type JSXElementBitset = EnumSet<JSXElement>;

    #[derive(EnumSetType, Debug)]
    pub enum Property {
        IsComputed,
        IsMethod,
        IsStatic,
        WasShorthand,
        IsSpread,
    }
    pub type PropertySet = EnumSet<Property>;
    pub const PROPERTY_NONE: PropertySet = EnumSet::empty();

    #[derive(EnumSetType, Debug)]
    pub enum Function {
        IsAsync,
        IsGenerator,
        HasRestArg,
        HasIfScope,

        IsForwardDeclaration,

        /// This is true if the function is a method
        IsUniqueFormalParameters,

        /// Only applicable to function statements.
        IsExport,

        /// A `// eslint-disable… react-hooks/…` comment was scanned at or before
        /// this function's body close. The React Compiler skips such functions.
        HasReactHooksSuppression,
    }
    pub type FunctionSet = EnumSet<Function>;
    pub const FUNCTION_NONE: FunctionSet = EnumSet::empty();
}

// ───────────────────────────────────────────────────────────────────────────
// Loc
// ───────────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct Loc {
    pub start: i32,
}

impl Default for Loc {
    fn default() -> Self {
        Loc { start: -1 }
    }
}

impl Loc {
    pub const EMPTY: Loc = Loc { start: -1 };

    #[inline]
    pub fn to_nullable(self) -> Option<Loc> {
        if self.start == -1 { None } else { Some(self) }
    }

    #[inline]
    pub fn to_usize(self) -> usize {
        self.i()
    }

    #[inline]
    pub fn i(self) -> usize {
        usize::try_from(self.start.max(0)).expect("int cast")
    }

    #[inline]
    pub fn eql(self, other: Loc) -> bool {
        self.start == other.start
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.eql(Self::EMPTY)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Range
// ───────────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Range {
    pub loc: Loc,
    pub len: i32,
}

impl Default for Range {
    fn default() -> Self {
        Range {
            loc: Loc::EMPTY,
            len: 0,
        }
    }
}

impl Range {
    /// Deprecated: use `NONE`
    #[allow(non_upper_case_globals)]
    pub const None: Range = Self::NONE;
    pub const NONE: Range = Range {
        loc: Loc::EMPTY,
        len: 0,
    };

    pub fn is_empty(self) -> bool {
        self.len == 0 && self.loc.start == Loc::EMPTY.start
    }

    pub fn end(self) -> Loc {
        Loc {
            start: self.loc.start + self.len,
        }
    }

    pub fn end_i(self) -> usize {
        // Saturates negatives to 0.
        (self.loc.start + self.len).max(0) as usize
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, enum_map::Enum, strum::IntoStaticStr)]
pub enum ImportKind {
    /// An entry point provided to `bun run` or `bun`
    #[strum(serialize = "entry_point_run")]
    EntryPointRun = 0,
    /// An entry point provided to `bun build` or `Bun.build`
    #[strum(serialize = "entry_point_build")]
    EntryPointBuild = 1,
    /// An ES6 import or re-export statement
    #[default]
    #[strum(serialize = "stmt")]
    Stmt = 2,
    /// A call to "require()"
    #[strum(serialize = "require")]
    Require = 3,
    /// An "import()" expression with a string argument
    #[strum(serialize = "dynamic")]
    Dynamic = 4,
    /// A call to "require.resolve()"
    #[strum(serialize = "require_resolve")]
    RequireResolve = 5,
    /// A CSS "@import" rule
    #[strum(serialize = "at")]
    At = 6,
    /// A CSS "@import" rule with import conditions
    #[strum(serialize = "at_conditional")]
    AtConditional = 7,
    /// A CSS "url(...)" token
    #[strum(serialize = "url")]
    Url = 8,
    /// A CSS "composes" property
    #[strum(serialize = "composes")]
    Composes = 9,
    #[strum(serialize = "html_manifest")]
    HtmlManifest = 10,
    #[strum(serialize = "internal")]
    Internal = 11,
}

impl ImportKind {
    /// With code splitting, whether an edge of this kind puts its target in a
    /// chunk of its own that is loaded when the expression runs.
    #[inline]
    pub fn can_be_lazy_chunk(self, target_is_bun: bool) -> bool {
        match self {
            ImportKind::Dynamic => true,
            ImportKind::Require => target_is_bun,
            _ => false,
        }
    }

    #[inline]
    pub fn label(self) -> &'static [u8] {
        match self {
            ImportKind::EntryPointRun => b"entry-point-run",
            ImportKind::EntryPointBuild => b"entry-point-build",
            ImportKind::Stmt => b"import-statement",
            ImportKind::Require => b"require-call",
            ImportKind::Dynamic => b"dynamic-import",
            ImportKind::RequireResolve => b"require-resolve",
            ImportKind::At => b"import-rule",
            ImportKind::AtConditional => b"",
            ImportKind::Url => b"url-token",
            ImportKind::Composes => b"composes",
            ImportKind::Internal => b"internal",
            ImportKind::HtmlManifest => b"html_manifest",
        }
    }

    #[inline]
    pub fn error_label(self) -> &'static [u8] {
        match self {
            ImportKind::EntryPointRun => b"entry point (run)",
            ImportKind::EntryPointBuild => b"entry point (build)",
            ImportKind::Stmt => b"import",
            ImportKind::Require => b"require()",
            ImportKind::Dynamic => b"import()",
            ImportKind::RequireResolve => b"require.resolve()",
            ImportKind::At => b"@import",
            ImportKind::AtConditional => b"",
            ImportKind::Url => b"url()",
            ImportKind::Internal => b"<bun internal>",
            ImportKind::Composes => b"composes",
            ImportKind::HtmlManifest => b"HTML import",
        }
    }

    #[inline]
    pub fn is_common_js(self) -> bool {
        matches!(self, Self::Require | Self::RequireResolve)
    }

    #[inline]
    pub fn is_entry_point(self) -> bool {
        matches!(self, Self::EntryPointRun | Self::EntryPointBuild)
    }

    pub fn is_from_css(self) -> bool {
        self == Self::AtConditional || self == Self::At || self == Self::Url || self == Self::Composes
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Ref / Symbol
// ───────────────────────────────────────────────────────────────────────────

/// Tag bits of `Ref`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum RefTag {
    Invalid = 0,
    AllocatedName = 1,
    SourceContentsSlice = 2,
    Symbol = 3,
}

/// Packed-u64 symbol reference: `{inner_index: u28, user: u3, tag: u2, source_index: u31}`.
///
/// LSB-first packing for the `tag`/`source_index` fields, with 3 bits stolen
/// from `inner_index` (31 → 28 bits, max 268M symbols/file -
/// three.js peaks at ~50K) so that `E::Identifier` / `E::ImportIdentifier` /
/// `E::CommonJSExportIdentifier` can pack their boolean side-flags inline,
/// shrinking `expr::Data` from 24→16 bytes and `Expr` from 32→24. The
/// rarely-set flags (`with`-stmt guard, known-pure-global hints) ride in
/// otherwise-dead bits instead of forcing 8 bytes of struct padding on every
/// identifier node.
///
/// User bits are *not* part of the ref's identity: `eq`/`hash`/`eql`/`as_u64`
/// all mask them off, so `id.ref_` (which may carry flags) compares/hashes
/// identically to the symbol-table key it indexes. `pack()` always writes 0
/// into the user-bit lane, so for every `Ref` constructed via `new`/`init`
/// the masking is a no-op and hashing is bit-identical to the pre-shrink
/// layout: preserving output sha-identity.
///
/// `packed(4)` lowers the alignment to 4 without changing the single-scalar
/// representation, so `Ref` is still passed/returned in one register and
/// `self.0` is still one `mov`. The align-4 part is what lets the inline
/// identifier payloads: and therefore `expr::Data` and `Expr`: pack into 16
/// bytes. All accessors take `self` by value (`Copy`), so the packed-field
/// reference restriction never applies.
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct Ref(u64);

const _: () = assert!(core::mem::size_of::<Ref>() == 8);
const _: () = assert!(core::mem::align_of::<Ref>() == 4);

/// We mask to 31 bits for `source_index`, 28 for `inner_index`.
pub type RefInt = u32;

impl Ref {
    const INNER_MASK: u64 = (1u64 << 31) - 1;
    /// `inner_index` width: 28 bits, leaving 3 user bits.
    /// `debug_assert!` in `pack()` catches any source large enough to overflow
    /// (would require >268M symbols or a >268MB source-contents-slice offset).
    const INNER_BITS: u64 = (1u64 << 28) - 1;
    /// Bits 28..31: opaque per-node flags (E::Identifier side-effect hints,
    /// E::ImportIdentifier `was_originally_identifier`, E::CommonJSExportIdentifier
    /// `base`). Never set by `pack()`; only via `set_user_bit`. Masked out of
    /// identity (`eq`/`hash`/`as_u64`).
    const USER_BITS_MASK: u64 = 0b111 << 28;
    const SRC_SHIFT: u32 = 33;

    /// Represents a null state without using an extra bit.
    pub const NONE: Ref = Ref(0); // tag=Invalid, inner=0, src=0

    /// Raw 64-bit representation **including** user bits. For round-tripping
    /// through external pointer-packed storage (e.g. css `IdentOrRef`). Differs
    /// from [`Self::as_u64`], which masks user bits for hashing/equality.
    #[inline]
    pub const fn to_raw_bits(self) -> u64 {
        self.0
    }
    /// Reconstruct from a value previously returned by [`Self::to_raw_bits`].
    #[inline]
    pub const fn from_raw_bits(bits: u64) -> Ref {
        Ref(bits)
    }

    /// General constructor exposing all three packed fields. Prefer `init` for
    /// the common source-contents/allocated-name case; this exists for callers
    /// that need to set `tag` explicitly (e.g. `RefTag::Symbol`).
    #[inline]
    pub const fn new(inner_index: RefInt, source_index: RefInt, tag: RefTag) -> Ref {
        Self::pack(inner_index, tag, source_index)
    }

    #[inline]
    const fn pack(inner: u32, tag: RefTag, src: u32) -> Ref {
        debug_assert!(
            (inner as u64) <= Self::INNER_BITS,
            "Ref.inner_index overflows 28 bits: file has >268M symbols or >268MB source slice",
        );
        Ref((inner as u64 & Self::INNER_BITS)
            | ((tag as u64) << 31)
            | ((src as u64 & Self::INNER_MASK) << Self::SRC_SHIFT))
    }

    #[inline]
    pub const fn inner_index(self) -> u32 {
        (self.0 & Self::INNER_BITS) as u32
    }
    #[inline]
    pub const fn source_index(self) -> u32 {
        (self.0 >> Self::SRC_SHIFT) as u32 & Self::INNER_MASK as u32
    }
    #[inline]
    pub const fn tag(self) -> RefTag {
        match (self.0 >> 31) as u8 & 0b11 {
            0 => RefTag::Invalid,
            1 => RefTag::AllocatedName,
            2 => RefTag::SourceContentsSlice,
            _ => RefTag::Symbol,
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        // Mask user bits so a flagged `Ref::NONE` (e.g. the define-template
        // `E::Identifier::init(Ref::NONE).with_can_be_removed_if_unused(true)`)
        // still reports null: keeps `is_empty`/`is_null` consistent with
        // `eq`/`hash`/`eql`/`as_u64`, which all ignore the user-bit lane.
        (self.0 & !Self::USER_BITS_MASK) == 0
    }
    #[inline]
    pub const fn is_valid(self) -> bool {
        !matches!(self.tag(), RefTag::Invalid)
    }
    #[inline]
    pub const fn is_symbol(self) -> bool {
        matches!(self.tag(), RefTag::Symbol)
    }
    #[inline]
    pub const fn is_source_contents_slice(self) -> bool {
        matches!(self.tag(), RefTag::SourceContentsSlice)
    }
    #[inline]
    pub(crate) fn is_source_index_null(i: u32) -> bool {
        i == Self::INNER_MASK as u32 // maxInt(u31)
    }

    pub fn init(inner_index: u32, source_index: u32, is_source_contents_slice: bool) -> Ref {
        let tag = if is_source_contents_slice {
            RefTag::SourceContentsSlice
        } else {
            RefTag::AllocatedName
        };
        Self::pack(inner_index, tag, source_index)
    }

    /// Identity bits (user/flag lane masked off). For all refs constructed via
    /// `new`/`init`/`pack` this equals the raw `self.0` (user bits are 0 there),
    /// so wyhash output is unchanged vs the pre-shrink layout.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0 & !Self::USER_BITS_MASK
    }

    // ── User bits (E::Identifier-family side flags) ──────────────────────
    // Three spare bits at 28..31, freed by narrowing `inner_index` to u28.
    // These ride along on the inline `pub ref_: Ref` field of identifier
    // expression nodes so the node fits in 8 bytes (the niche-free size of
    // every other inline `expr::Data` payload). They are masked out of
    // identity (eq/hash/as_u64/inner_index) so `id.ref_` remains a valid
    // symbol-map key regardless of flag state.
    #[inline]
    pub(crate) const fn user_bit(self, n: u32) -> bool {
        debug_assert!(n < 3);
        (self.0 >> (28 + n)) & 1 != 0
    }
    #[inline]
    pub(crate) fn set_user_bit(&mut self, n: u32, v: bool) {
        debug_assert!(n < 3);
        let bit = 1u64 << (28 + n);
        self.0 = (self.0 & !bit) | ((v as u64) << (28 + n));
    }
    #[inline]
    pub(crate) const fn with_user_bit(mut self, n: u32, v: bool) -> Ref {
        debug_assert!(n < 3);
        let bit = 1u64 << (28 + n);
        self.0 = (self.0 & !bit) | ((v as u64) << (28 + n));
        self
    }
    /// Identity bits only (user/flag lane zeroed). Use when handing a
    /// flag-carrying `E::Identifier.ref_` to a context that stores its own
    /// flags in the same lane (e.g. `E::ImportIdentifier::new`), so stale
    /// `can_be_removed_if_unused`/`call_can_be_unwrapped_if_unused` bits don't
    /// leak across node kinds.
    #[inline]
    pub(crate) const fn without_user_bits(self) -> Ref {
        Ref(self.0 & !Self::USER_BITS_MASK)
    }
    /// Replace the identity bits with those of `self` while keeping `src`'s
    /// user-bit lane. Used by `handle_identifier`'s `id_clone.ref_ = result.ref`
    /// assignment: the flags ride in `ref_` and would otherwise be silently
    /// zeroed by a whole-word write.
    #[inline]
    pub const fn with_user_bits_from(self, src: Ref) -> Ref {
        Ref((self.0 & !Self::USER_BITS_MASK) | (src.0 & Self::USER_BITS_MASK))
    }
    #[inline]
    pub const fn eql(self, other: Ref) -> bool {
        // User-bit lane is not part of identity: see type-level doc.
        self.as_u64() == other.as_u64()
    }
    /// deprecated alias
    #[inline]
    pub const fn is_null(self) -> bool {
        self.is_empty()
    }
    /// `Ref::NONE` → `None`, otherwise `Some(self)`. For sites that previously
    /// stored `Option<Ref>` and want option combinators on the sentinel form.
    #[inline]
    pub fn to_nullable(self) -> Option<Ref> {
        if self.is_empty() { None } else { Some(self) }
    }
}

impl Default for Ref {
    fn default() -> Self {
        Ref::NONE
    }
}

// Identity excludes the user-bit lane (bits 28..31). For every Ref produced by
// `pack()` those bits are 0, so this is bit-identical to `#[derive(...)]` on
// the raw u64: the mask only matters for `E::Identifier.ref_` & friends where
// flag bits may be set, and there it ensures `HashMap<Ref, _>` lookups via
// `id.ref_` resolve to the same bucket as the flag-free symbol-table key.
impl PartialEq for Ref {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.eql(*other)
    }
}
impl Eq for Ref {}
/// O `Ref` é a própria chave guardada nos `ArrayHashMap` por símbolo.
impl bun_collections::OwnKey<Ref> for Ref {
    fn own(key: &Ref) -> Self {
        *key
    }
}
impl core::hash::Hash for Ref {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_u64().hash(state)
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Ref[inner={}, src={}, .{}]",
            self.inner_index(),
            self.source_index(),
            <&'static str>::from(self.tag()),
        )
    }
}

impl fmt::Debug for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

