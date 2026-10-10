//! `src/ast/nodes.rs` do upstream, portado item a item na ordem em que os outros módulos pedem.
//!
//! `StoreRef`, `StoreSlice` e `StoreStr` mantêm os nomes do upstream, mas deixam de ser ponteiros: são índices no
//! `new_store::Store` que é dono dos nós (ver a seção "Arena tipada" do README de `wip/zbun`).

use core::marker::PhantomData;
use core::num::NonZeroU32;

use crate::binding::Binding;
use crate::expr::Expr;
use crate::stmt::Stmt;
use crate::store::Store;
use crate::{E, Ref};

// ───────────────────────────────────────────────────────────────────────────
// StoreRef: índice de um nó no vetor do seu tipo dentro do `Store`.
// ───────────────────────────────────────────────────────────────────────────

/// Um nó do tipo `T` guardado num `Store`. O índice é a posição mais um, para o nicho de `Option<StoreRef<T>>`.
/// A igualdade é a do índice, como a do upstream é a do endereço.
pub struct StoreRef<T> {
    index: NonZeroU32,
    _type: PhantomData<fn() -> T>,
}

const _: () = assert!(core::mem::size_of::<StoreRef<u8>>() == 4);
const _: () = assert!(core::mem::size_of::<Option<StoreRef<u8>>>() == 4);

impl<T> StoreRef<T> {
    /// O nó na posição `position` do vetor do tipo.
    #[inline]
    pub const fn from_position(position: u32) -> Self {
        match NonZeroU32::new(position.wrapping_add(1)) {
            Some(index) => StoreRef { index, _type: PhantomData },
            None => panic!("StoreRef: posição fora do alcance de u32"),
        }
    }

    /// A posição no vetor do tipo.
    #[inline]
    pub const fn position(self) -> usize {
        (self.index.get() - 1) as usize
    }
}

impl<T> Clone for StoreRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for StoreRef<T> {}
impl<T> PartialEq for StoreRef<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for StoreRef<T> {}
impl<T> core::hash::Hash for StoreRef<T> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state)
    }
}
impl<T> core::fmt::Debug for StoreRef<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "StoreRef({})", self.position())
    }
}

// ───────────────────────────────────────────────────────────────────────────
// StoreSlice: faixa contígua do vetor do tipo.
// ───────────────────────────────────────────────────────────────────────────

/// Os elementos `start..start + len` do vetor do tipo `T` no `Store`.
pub struct StoreSlice<T> {
    start: u32,
    len: u32,
    _type: PhantomData<fn() -> T>,
}

const _: () = assert!(core::mem::size_of::<StoreSlice<u8>>() == 8);

impl<T> StoreSlice<T> {
    pub const EMPTY: StoreSlice<T> = StoreSlice { start: 0, len: 0, _type: PhantomData };

    #[inline]
    pub(crate) const fn from_range(start: u32, len: u32) -> Self {
        StoreSlice { start, len, _type: PhantomData }
    }

    #[inline]
    pub const fn len(self) -> usize {
        self.len as usize
    }

    #[inline]
    pub const fn len_u32(self) -> u32 {
        self.len
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// A faixa no vetor do tipo. Pública porque o `define_store!` expande em outros crates.
    #[doc(hidden)]
    #[inline]
    pub const fn range(self) -> core::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }

    /// Tira `count` elementos da frente da faixa (o par de `truncate`).
    #[inline]
    pub fn skip(&mut self, count: usize) {
        assert!(count <= self.len as usize);
        self.start += count as u32;
        self.len -= count as u32;
    }

    /// Encurta a faixa. Os elementos de trás continuam no `Store`, só deixam de ser alcançados por esta faixa.
    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        assert!(new_len <= self.len as usize);
        self.len = new_len as u32;
    }
}

impl<T> Clone for StoreSlice<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for StoreSlice<T> {}
impl<T> Default for StoreSlice<T> {
    #[inline]
    fn default() -> Self {
        StoreSlice::EMPTY
    }
}
impl<T> core::fmt::Debug for StoreSlice<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "StoreSlice({}..{})", self.start, self.start + self.len)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// StoreStr: faixa do vetor de bytes do `Store`.
// ───────────────────────────────────────────────────────────────────────────

/// Os bytes `start..start + len` do vetor de strings do `Store`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Hash)]
pub struct StoreStr {
    start: u32,
    len: u32,
}

impl StoreStr {
    pub const EMPTY: StoreStr = StoreStr { start: 0, len: 0 };

    /// Faixa crua do vetor de bytes. O código-fonte ocupa o começo do vetor (seção "Arena tipada" do README),
    /// então o lexer descreve uma fatia do fonte pelo deslocamento dela, sem gravar nada.
    #[inline]
    pub const fn from_range(start: u32, len: u32) -> Self {
        StoreStr { start, len }
    }

    #[inline]
    pub const fn len(self) -> usize {
        self.len as usize
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// A faixa no vetor de bytes.
    #[inline]
    pub(crate) const fn range(self) -> core::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ImportItemStatus {
    None,
    /// The linker doesn't report import/export mismatch errors
    Generated,
    /// The printer will replace this import with "undefined"
    Missing,
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum OptionalChain {
    /// "a?.b"
    Start,

    /// "a?.b.c" => ".c" is OptionalChain::Continuation
    /// "(a?.b).c" => ".c" is None
    Continuation,
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AssignTarget {
    #[default]
    None = 0,
    /// "a = b"
    Replace = 1,
    /// "a += b"
    Update = 2,
}

// ───────────────────────────────────────────────────────────────────────────
// Nomes de índice e listas. No upstream `ExprNodeList` é `Vec<Expr, AstAlloc>`; no porte toda lista é faixa do
// vetor do tipo (ver "Listas" na seção "Arena tipada" do README).
// ───────────────────────────────────────────────────────────────────────────

pub type ExprNodeIndex = Expr;
pub type StmtNodeIndex = Stmt;
pub type BindingNodeIndex = Binding;

pub type ExprNodeList = StoreSlice<Expr>;
pub type StmtNodeList = StoreSlice<Stmt>;

pub(crate) type ArenaStr = StoreStr;

#[derive(Copy, Clone)]
pub struct LocRef {
    pub loc: crate::Loc,
    pub ref_: Ref,
}

const _: () = assert!(core::mem::size_of::<LocRef>() == 12);

impl Default for LocRef {
    fn default() -> Self {
        Self {
            loc: crate::Loc::EMPTY,
            ref_: Ref::NONE,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClauseItem {
    /// The local alias used for the imported/exported symbol in the current module.
    /// For imports: `import { foo as bar }` - "bar" is the alias
    /// For exports: `export { foo as bar }` - "bar" is the alias
    /// For re-exports: `export { foo as bar } from 'path'` - "bar" is the alias
    pub alias: ArenaStr,
    pub alias_loc: crate::Loc,
    /// Reference to the actual symbol being imported/exported.
    /// For imports: `import { foo as bar }` - ref to the symbol representing "foo" from the source module
    /// For exports: `export { foo as bar }` - ref to the local symbol "foo"
    /// For re-exports: `export { foo as bar } from 'path'` - ref to an intermediate symbol
    pub name: LocRef,

    /// This is the original name of the symbol stored in "Name". It's needed for
    /// "SExportClause" statements such as this:
    ///
    ///   export {foo as bar} from 'path'
    ///
    /// In this case both "foo" and "bar" are aliases because it's a re-export.
    /// We need to preserve both aliases in case the symbol is renamed. In this
    /// example, "foo" is "OriginalName" and "bar" is "Alias".
    pub original_name: ArenaStr,
}

impl ClauseItem {
    pub const DEFAULT_ALIAS: &'static [u8] = b"default";
}

impl Default for ClauseItem {
    fn default() -> Self {
        Self {
            alias: StoreStr::EMPTY,
            alias_loc: crate::Loc::EMPTY,
            name: LocRef::default(),
            original_name: StoreStr::EMPTY,
        }
    }
}

#[derive(Clone, Copy)]
pub struct EnumValue {
    pub loc: crate::Loc,
    pub ref_: Ref,
    pub name: ArenaStr,
    pub value: Option<ExprNodeIndex>,
}

impl EnumValue {
    pub fn name_as_e_string(&self, store: &mut Store) -> E::String {
        E::String::init_re_encode_utf8(store, self.name)
    }
}

#[derive(Clone, Copy)]
pub struct Catch {
    pub loc: crate::Loc,
    pub binding: Option<BindingNodeIndex>,
    pub body: StmtNodeList,
    pub body_loc: crate::Loc,
}

#[derive(Clone, Copy)]
pub struct Finally {
    pub loc: crate::Loc,
    pub stmts: StmtNodeList,
}

#[derive(Clone, Copy)]
pub struct Case {
    pub loc: crate::Loc,
    pub value: Option<ExprNodeIndex>,
    pub body: StmtNodeList,
}

#[derive(Clone, Copy)]
pub struct ArrayBinding {
    pub binding: BindingNodeIndex,
    pub default_value: Option<ExprNodeIndex>,
}

#[derive(Clone, Copy)]
pub enum StmtOrExpr {
    Stmt(Stmt),
    Expr(Expr),
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum StrictModeKind {
    SloppyMode,
    ExplicitStrictMode,
    ImplicitStrictModeImport,
    ImplicitStrictModeExport,
    ImplicitStrictModeTopLevelAwait,
    ImplicitStrictModeClass,
}

#[derive(Copy, Clone, Default)]
pub struct Span {
    pub text: StoreStr,
    pub range: crate::Range,
}

// EnumMap<_, u32>::default() zero-fills.
#[derive(Copy, Clone, Default)]
pub struct SlotCounts {
    pub slots: crate::symbol::SlotNamespaceCountsArray,
}

impl SlotCounts {
    pub fn union_max(&mut self, other: SlotCounts) {
        for (a, b) in self.slots.values_mut().zip(other.slots.values()) {
            if *a < *b {
                *a = *b;
            }
        }
    }
}

pub struct NameMinifier {
    pub head: Vec<u8>,
    pub(crate) tail: Vec<u8>,
}

impl NameMinifier {
    pub(crate) const DEFAULT_HEAD: &'static [u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_$";
    pub(crate) const DEFAULT_TAIL: &'static [u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_$";

    pub fn init() -> NameMinifier {
        NameMinifier { head: Vec::new(), tail: Vec::new() }
    }

    /// No upstream devolve `Result<(), AllocError>`; a alocação aqui não falha.
    pub fn number_to_minified_name(&self, name: &mut Vec<u8>, i_: isize) {
        name.clear();
        Self::append_minified_name(name, &self.head, &self.tail, i_);
    }

    pub fn default_number_to_minified_name(i_: isize) -> Vec<u8> {
        let mut name = Vec::new();
        Self::append_minified_name(&mut name, Self::DEFAULT_HEAD, Self::DEFAULT_TAIL, i_);
        name
    }

    /// O corpo comum das duas funções do upstream (que o repetem com a cabeça e a cauda trocadas).
    fn append_minified_name(name: &mut Vec<u8>, head: &[u8], tail: &[u8], i_: isize) {
        const COUNT: isize = crate::char_freq::CHAR_FREQ_COUNT as isize;
        let mut i = i_;
        let mut j = usize::try_from(i.rem_euclid(54)).expect("int cast");
        name.extend_from_slice(&head[j..j + 1]);
        i = i.div_euclid(54);

        while i > 0 {
            i -= 1;
            j = usize::try_from(i.rem_euclid(COUNT)).expect("int cast");
            name.extend_from_slice(&tail[j..j + 1]);
            i = i.div_euclid(COUNT);
        }
    }
}

/// TLA => Top Level Await
#[derive(Copy, Clone)]
pub struct TlaCheck {
    pub depth: u32,
    pub parent: crate::base::IndexInt,
    pub import_record_index: crate::base::IndexInt,
}

impl Default for TlaCheck {
    fn default() -> Self {
        Self { depth: 0, parent: crate::Index::INVALID.get(), import_record_index: crate::Index::INVALID.get() }
    }
}

/// Inlined enum values can only be numbers and strings. No upstream o valor é um `u64` com NaN-boxing (ponteiro de
/// 48 bits ou `f64`); com a string no `Store`, a mesma codificação guarda o índice da `E::String` no lugar do
/// endereço, e o número continua com o deslocamento do JSCJSValue.
#[derive(Copy, Clone)]
pub struct InlinedEnumValue {
    pub(crate) raw_data: u64,
}

#[derive(Copy, Clone)]
pub enum InlinedEnumValueDecoded {
    String(StoreRef<E::EString>),
    Number(f64),
}

impl InlinedEnumValue {
    /// See JSCJSValue.h in WebKit for more details
    const DOUBLE_ENCODE_OFFSET: u64 = 1 << 49;
    /// See PureNaN.h in WebKit for more details
    const PURE_NAN: f64 = f64::from_bits(0x7ff8000000000000);

    fn purify_nan(value: f64) -> f64 {
        if value.is_nan() { Self::PURE_NAN } else { value }
    }

    pub fn encode(decoded: InlinedEnumValueDecoded) -> InlinedEnumValue {
        InlinedEnumValue {
            raw_data: match decoded {
                InlinedEnumValueDecoded::String(s) => u64::from(s.index.get()),
                InlinedEnumValueDecoded::Number(num) => Self::purify_nan(num).to_bits() + Self::DOUBLE_ENCODE_OFFSET,
            },
        }
    }

    pub fn decode(self) -> InlinedEnumValueDecoded {
        if self.raw_data > 0x0000_FFFF_FFFF_FFFF {
            InlinedEnumValueDecoded::Number(f64::from_bits(self.raw_data - Self::DOUBLE_ENCODE_OFFSET))
        } else {
            let index = NonZeroU32::new(self.raw_data as u32).expect("InlinedEnumValue de string com índice zero");
            InlinedEnumValueDecoded::String(StoreRef { index, _type: PhantomData })
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ExportsKind {
    // This file doesn't have any kind of export, so it's impossible to say what
    // kind of file this is. An empty file is in this category, for example.
    None,

    // The exports are stored on "module" and/or "exports". Calling "require()"
    // on this module returns "module.exports". All imports to this module are
    // allowed but may return undefined.
    Cjs,

    // All export names are known explicitly. Calling "require()" on this module
    // generates an exports object (stored in "exports") with getters for the
    // export names. Named imports to this module are only allowed if they are
    // in the set of export names.
    Esm,

    // Some export names are known explicitly, but others fall back to a dynamic
    // run-time object.
    EsmWithDynamicFallback,

    // Like "EsmWithDynamicFallback", but the module was originally a CommonJS
    // module.
    EsmWithDynamicFallbackFromCjs,
}

impl ExportsKind {
    pub fn is_dynamic(self) -> bool {
        matches!(self, Self::Cjs | Self::EsmWithDynamicFallback | Self::EsmWithDynamicFallbackFromCjs)
    }

    pub fn is_esm_with_dynamic_fallback(self) -> bool {
        matches!(self, Self::EsmWithDynamicFallback | Self::EsmWithDynamicFallbackFromCjs)
    }
}

#[derive(Copy, Clone)]
pub struct DeclaredSymbol {
    pub ref_: Ref,
    pub is_top_level: bool,
}

/// No upstream é um `MultiArrayList` (uma coluna por campo); aqui as duas colunas são dois `Vec`, o que dá o mesmo
/// `refs()` contíguo.
#[derive(Default, Clone)]
pub struct DeclaredSymbolList {
    refs: Vec<Ref>,
    is_top_level: Vec<bool>,
}

impl DeclaredSymbolList {
    pub fn refs(&self) -> &[Ref] {
        &self.refs
    }

    pub fn to_owned_slice(&mut self) -> DeclaredSymbolList {
        core::mem::take(self)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.refs.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }

    pub fn append(&mut self, entry: DeclaredSymbol) {
        self.refs.push(entry.ref_);
        self.is_top_level.push(entry.is_top_level);
    }

    /// O `append_assume_capacity` do `MultiArrayList`: a capacidade já foi reservada por `ensure_total_capacity`.
    #[inline]
    pub fn append_assume_capacity(&mut self, entry: DeclaredSymbol) {
        self.append(entry);
    }

    pub fn append_list(&mut self, other: &DeclaredSymbolList) {
        self.refs.extend_from_slice(&other.refs);
        self.is_top_level.extend_from_slice(&other.is_top_level);
    }

    pub fn ensure_unused_capacity(&mut self, count: usize) {
        self.refs.reserve(count);
        self.is_top_level.reserve(count);
    }

    /// Reserva até `count` entradas no total, como o `MultiArrayList.ensureTotalCapacity` (falha só sem memória).
    pub fn ensure_total_capacity(&mut self, count: usize) -> Result<(), bun_collections::AllocError> {
        let additional = count.saturating_sub(self.refs.len());
        self.refs.try_reserve(additional).map_err(|_| bun_collections::AllocError)?;
        self.is_top_level.try_reserve(additional).map_err(|_| bun_collections::AllocError)?;
        Ok(())
    }

    pub fn clear_retaining_capacity(&mut self) {
        self.refs.clear();
        self.is_top_level.clear();
    }

    pub fn init_capacity(capacity: usize) -> Result<DeclaredSymbolList, bun_collections::AllocError> {
        let mut this = DeclaredSymbolList::default();
        this.ensure_total_capacity(capacity)?;
        Ok(this)
    }

    pub fn from_slice(entries: &[DeclaredSymbol]) -> DeclaredSymbolList {
        let mut this = DeclaredSymbolList {
            refs: Vec::with_capacity(entries.len()),
            is_top_level: Vec::with_capacity(entries.len()),
        };
        for &entry in entries {
            this.append(entry);
        }
        this
    }
}

impl DeclaredSymbol {
    pub fn for_each_top_level_symbol<C>(decls: &DeclaredSymbolList, ctx: &mut C, f: impl Fn(&mut C, Ref)) {
        for (top, ref_) in decls.is_top_level.iter().zip(decls.refs.iter()) {
            if *top {
                f(ctx, *ref_);
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct Dependency {
    pub source_index: crate::Index,
    pub part_index: u32, // Index.Int
}

pub type DependencyList = Vec<Dependency>;

/// Each file is made up of multiple parts, and each part consists of one or
/// more top-level statements. Parts are used for tree shaking and code
/// splitting analysis. Individual parts of a file can be discarded by tree
/// shaking and can be assigned to separate chunks (i.e. output files) by code
/// splitting.
pub struct Part {
    pub stmts: StoreSlice<Stmt>,
    pub scopes: Vec<StoreRef<crate::Scope>>,

    /// Each is an index into the file-level import record list
    pub import_record_indices: PartImportRecordIndices,

    /// All symbols that are declared in this part. Note that a given symbol may
    /// have multiple declarations, and so may end up being declared in multiple
    /// parts (e.g. multiple "var" declarations with the same name). Also note
    /// that this list isn't deduplicated and may contain duplicates.
    pub declared_symbols: DeclaredSymbolList,

    /// An estimate of the number of uses of all symbols used within this part.
    pub symbol_uses: PartSymbolUseMap,

    /// This tracks property accesses off of imported symbols. We don't know
    /// during parsing if an imported symbol is going to be an inlined enum
    /// value or not. This is only known during linking.
    pub import_symbol_property_uses: Option<Box<PartSymbolPropertyUseMap>>,

    /// The indices of the other parts in this file that are needed if this part
    /// is needed.
    pub dependencies: DependencyList,

    /// If true, this part can be removed if none of the declared symbols are
    /// used. If the file containing this part is imported, then all parts that
    /// don't have this flag enabled must be included.
    pub can_be_removed_if_unused: bool,

    /// This is used for generated parts that we don't want to be present if they
    /// aren't needed. This enables tree shaking for these parts even if global
    /// tree shaking isn't enabled.
    pub force_tree_shaking: bool,

    pub tag: PartTag,
}

pub type PartImportRecordIndices = Vec<u32>;
pub type PartList = Vec<Part>;

/// This is the index to the automatically-generated part containing code that
/// calls "__export(exports, { ... getters ... })". This is used to generate
/// getters on an exports object for ES6 export statements, and is both for
/// ES6 star imports and CommonJS-style modules. All files have one of these,
/// although it may contain no statements if there is nothing to export.
pub const NAMESPACE_EXPORT_PART_INDEX: u32 = 0;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PartTag {
    None,
    JsxImport,
    Runtime,
    ReactCompiler,
    DirnameFilename,
    BunTest,
    DeadDueToInlining,
    CommonjsNamedExport,
    ImportToConvertFromRequire,
}

pub type PartSymbolUseMap = bun_collections::ArrayHashMap<Ref, crate::symbol::Use>;
pub type PartSymbolPropertyUseMap =
    bun_collections::ArrayHashMap<Ref, std::collections::HashMap<Box<[u8]>, PropertyUse>>;

/// The reads of one `X.name` in `Part::import_symbol_property_uses`.
#[derive(Default, Clone, Copy)]
pub struct PropertyUse {
    pub count_estimate: u32,
    /// Some read is called, as `X.name()` or a template tag, with `X` as `this`.
    pub is_call_target: bool,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            stmts: StoreSlice::EMPTY,
            scopes: Vec::new(),
            import_record_indices: Vec::new(),
            declared_symbols: DeclaredSymbolList::default(),
            symbol_uses: PartSymbolUseMap::default(),
            import_symbol_property_uses: None,
            dependencies: Vec::new(),
            can_be_removed_if_unused: false,
            force_tree_shaking: false,
            tag: PartTag::None,
        }
    }
}

pub struct NamedImport {
    /// Parts within this file that use this import
    pub local_parts_with_uses: Vec<u32>,

    /// The original export name from the source module being imported.
    pub alias: Option<StoreStr>,
    pub alias_loc: crate::Loc,
    pub namespace_ref: Ref,
    pub import_record_index: u32,

    /// If true, the alias refers to the entire export namespace object of a
    /// module.
    pub alias_is_star: bool,

    /// It's useful to flag exported imports because if they are in a TypeScript
    /// file, we can't tell if they are a type or a value.
    pub is_exported: bool,
}

#[derive(Copy, Clone)]
pub struct NamedExport {
    pub ref_: Ref,
    pub alias_loc: crate::Loc,
}

impl Default for StmtOrExpr {
    fn default() -> Self {
        StmtOrExpr::Expr(Expr::default())
    }
}

impl StmtOrExpr {
    pub fn to_expr(self, store: &mut Store) -> Expr {
        match self {
            StmtOrExpr::Expr(expr) => expr,
            StmtOrExpr::Stmt(stmt) => match stmt.data {
                crate::stmt::Data::SFunction(s) => {
                    let func = core::mem::take(&mut store[s].func);
                    Expr::init(store, E::Function { func }, stmt.loc)
                }
                crate::stmt::Data::SClass(s) => {
                    let class = core::mem::take(&mut store[s].class);
                    Expr::init::<E::Class>(store, class, stmt.loc)
                }
                other => panic!(
                    "Unexpected statement type in default export: .{}",
                    <&'static str>::from(other.tag())
                ),
            },
        }
    }
}
