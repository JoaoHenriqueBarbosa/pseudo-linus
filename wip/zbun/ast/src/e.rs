//! E: expression node payloads for the JS AST.
//!
//! `src/ast/e.rs` do upstream sobre o `Store` (seção "Arena tipada" do README): o que o upstream lê por `Deref` do
//! `StoreRef` e do `StoreStr` passa pelo `Store`, e o que ele grava no bump ou no `Data::Store` de thread vai para o
//! `&mut Store` recebido. Os nós de JSON (`JsonValue`, `JsonTape`, `ObjectJSON`, `ArrayJSON` e o `TapeAlloc`) ficam
//! fora até o loader de JSON; `Rope.next` deixa de ser ponteiro cru e vira `Option<StoreRef<Rope>>`.

use core::cmp::Ordering;

use bun_core::strings;

use crate::new_store::NodeStore as _;
use crate::store::Store;
use crate::{Expr, ExprNodeIndex, ExprNodeList, G, OptionalChain, Ref, StoreRef};

// AST string fields are arena-owned: no porte, faixas do vetor de bytes do `Store`.
pub use crate::StoreStr as Str;

/// This represents an internal property name that can be mangled. The symbol
/// referenced by this expression should be a "SymbolMangledProp" symbol.
#[derive(Clone, Copy)]
pub struct NameOfSymbol {
    pub ref_: Ref,

    /// If true, a preceding comment contains "@__KEY__"
    ///
    /// Currently not used
    pub has_property_key_comment: bool,
}
impl Default for NameOfSymbol {
    fn default() -> Self {
        Self {
            ref_: Ref::NONE,
            has_property_key_comment: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Array {
    pub items: ExprNodeList,
    pub comma_after_spread: crate::Loc,
    pub is_single_line: bool,
    pub is_parenthesized: bool,
    pub was_originally_macro: bool,
    pub close_bracket_loc: crate::Loc,
}
impl Default for Array {
    fn default() -> Self {
        Self {
            items: ExprNodeList::EMPTY,
            comma_after_spread: crate::Loc::EMPTY,
            is_single_line: false,
            is_parenthesized: false,
            was_originally_macro: false,
            close_bracket_loc: crate::Loc::EMPTY,
        }
    }
}
// Live subset of `Array` accessors needed by downstream crates.
impl Array {
    pub fn push(store: &mut Store, this: StoreRef<Array>, item: Expr) {
        let items = store[this].items;
        store[this].items = store.append(items, item);
    }

    #[inline]
    pub fn slice<'s>(&self, store: &'s Store) -> &'s [Expr] {
        &store[self.items]
    }
}

impl Array {
    pub fn inline_spread_of_array_literals(&self, store: &mut Store, estimated_count: usize) -> ExprNodeList {
        // This over-allocates a little but it's fine
        let mut out: Vec<Expr> = Vec::with_capacity(estimated_count + self.items.len());
        for idx in 0..self.items.len() {
            let item = store[self.items][idx];
            if let crate::expr::Data::ESpread(val) = item.data {
                if let crate::expr::Data::EArray(inner) = store[val].value.data {
                    let inner_items = store[inner].items;
                    for inner_idx in 0..inner_items.len() {
                        let inner_item = store[inner_items][inner_idx];
                        if matches!(inner_item.data, crate::expr::Data::EMissing(_)) {
                            out.push(Expr::init(store, Undefined {}, inner_item.loc));
                        } else {
                            out.push(inner_item);
                        }
                    }
                    // skip empty arrays
                    // don't include the inlined spread.
                    continue;
                }
                // non-arrays are kept in
            }

            out.push(item);
        }
        store.alloc_slice(out)
    }

    // `toJS` lives in the `js_parser_jsc` crate's extension trait.

    /// Assumes each item in the array is a string
    pub fn alphabetize_strings(&self, store: &mut Store) {
        if cfg!(debug_assertions) {
            for item in store[self.items].iter() {
                debug_assert!(matches!(item.data, crate::expr::Data::EString(_)));
            }
        }
        // O `index_sort::sort_slice_by` do upstream é estável, como o `sort_by` da biblioteca padrão: a mesma ordem.
        let mut items = store[self.items].to_vec();
        items.sort_by(|lhs, rhs| array_sorter_is_less_than(store, lhs, rhs));
        store[self.items].copy_from_slice(&items);
    }
}

#[derive(Clone, Copy)]
pub struct Unary {
    pub op: crate::OpCode,
    pub value: ExprNodeIndex,
    pub flags: UnaryFlags,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
    #[repr(transparent)]
    pub struct UnaryFlags: u8 {
        /// The expression "typeof (0, x)" must not become "typeof x" if "x"
        /// is unbound because that could suppress a ReferenceError from "x".
        ///
        /// Also if we know a typeof operator was originally an identifier, then
        /// we know that this typeof operator always has no side effects (even if
        /// we consider the identifier by itself to have a side effect).
        ///
        /// Note that there *is* actually a case where "typeof x" can throw an error:
        /// when "x" is being referenced inside of its TDZ (temporal dead zone). TDZ
        /// checks are not yet handled correctly by Bun, so this possibility is
        /// currently ignored.
        const WAS_ORIGINALLY_TYPEOF_IDENTIFIER = 1 << 0;

        /// Similarly the expression "delete (0, x)" must not become "delete x"
        /// because that syntax is invalid in strict mode. We also need to make sure
        /// we don't accidentally change the return value:
        ///
        /// ```text
        /// Returns false:
        ///   "var a; delete (a)"
        ///   "var a = Object.freeze({b: 1}); delete (a.b)"
        ///   "var a = Object.freeze({b: 1}); delete (a?.b)"
        ///   "var a = Object.freeze({b: 1}); delete (a['b'])"
        ///   "var a = Object.freeze({b: 1}); delete (a?.['b'])"
        ///
        /// Returns true:
        ///   "var a; delete (0, a)"
        ///   "var a = Object.freeze({b: 1}); delete (true && a.b)"
        ///   "var a = Object.freeze({b: 1}); delete (false || a?.b)"
        ///   "var a = Object.freeze({b: 1}); delete (null ?? a?.['b'])"
        ///   "var a = Object.freeze({b: 1}); delete (true ? a['b'] : a['b'])"
        /// ```
        const WAS_ORIGINALLY_DELETE_OF_IDENTIFIER_OR_PROPERTY_ACCESS = 1 << 1;
    }
}

#[derive(Clone, Copy)]
pub struct Binary {
    pub left: ExprNodeIndex,
    pub right: ExprNodeIndex,
    pub op: crate::OpCode,
}

// ── Leaf scalar payloads ───────────────────────────────────────────────────
// `toJS` impls live in the `js_parser_jsc` extension trait.

#[derive(Clone, Copy)]
pub struct Boolean {
    pub value: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Null;
#[derive(Clone, Copy, Default)]
pub struct Undefined;
#[derive(Clone, Copy, Default)]
pub struct Missing;
#[derive(Clone, Copy, Default)]
pub struct This;
#[derive(Clone, Copy, Default)]
pub struct Super;
#[derive(Clone, Copy, Default)]
pub struct ImportMeta;

#[derive(Clone, Copy, Default)]
pub struct ImportMetaMain {
    /// If true, print `!import.meta.main` (or `require.main != module`).
    pub inverted: bool,
}

#[derive(Clone, Copy)]
pub struct NewTarget {
    pub range: crate::Range,
}

#[derive(Clone, Copy)]
pub struct New {
    pub target: ExprNodeIndex,
    pub args: ExprNodeList,

    /// True if there is a comment containing "@__PURE__" or "#__PURE__" preceding
    /// this call expression. See the comment inside ECall for more details.
    pub can_be_unwrapped_if_unused: CallUnwrap,

    pub close_parens_loc: crate::Loc,
}
impl Default for New {
    fn default() -> Self {
        Self {
            target: ExprNodeIndex::EMPTY,
            args: ExprNodeList::EMPTY,
            can_be_unwrapped_if_unused: CallUnwrap::Never,
            close_parens_loc: crate::Loc::EMPTY,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Special {
    /// emits `exports` or `module.exports` depending on `commonjs_named_exports_deoptimized`
    ModuleExports,
    /// `import.meta.hot`
    HotEnabled,
    /// Acts as .e_undefined, but allows property accesses to the rest of the HMR API.
    HotDisabled,
    /// `import.meta.hot.data` when HMR is enabled. Not reachable when it is disabled.
    HotData,
    /// `import.meta.hot.accept` when HMR is enabled. Truthy.
    HotAccept,
    /// Converted from `hot_accept` by the parser's import.meta.hot.accept
    /// handling when passed strings. Printed as `hmr.acceptSpecifiers`
    HotAcceptVisited,
    /// Prints the resolved specifier string for an import record
    /// (an `ImportRecord` index).
    ResolvedSpecifierString(u32),
}

#[derive(Clone, Copy)]
pub struct Call {
    // Node:
    pub target: ExprNodeIndex,
    pub args: ExprNodeList,
    pub optional_chain: Option<OptionalChain>,
    pub is_direct_eval: bool,
    pub close_paren_loc: crate::Loc,

    /// True if there is a comment containing "@__PURE__" or "#__PURE__" preceding
    /// this call expression. This is an annotation used for tree shaking, and
    /// means that the call can be removed if it's unused. It does not mean the
    /// call is pure (e.g. it may still return something different if called twice).
    ///
    /// Note that the arguments are not considered to be part of the call. If the
    /// call itself is removed due to this annotation, the arguments must remain
    /// if they have side effects.
    pub can_be_unwrapped_if_unused: CallUnwrap,

    /// Used when printing to generate the source prop on the fly
    pub was_jsx_element: bool,
}
impl Default for Call {
    fn default() -> Self {
        Self {
            target: ExprNodeIndex::EMPTY,
            args: ExprNodeList::EMPTY,
            optional_chain: None,
            is_direct_eval: false,
            close_paren_loc: crate::Loc::EMPTY,
            can_be_unwrapped_if_unused: CallUnwrap::Never,
            was_jsx_element: false,
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CallUnwrap {
    #[default]
    Never,
    IfUnused,
    IfUnusedAndToStringSafe,
}

#[derive(Clone, Copy)]
pub struct Dot {
    // target is Node
    pub target: ExprNodeIndex,
    pub name: Str,
    pub name_loc: crate::Loc,
    pub optional_chain: Option<OptionalChain>,

    /// If true, this property access is known to be free of side-effects. That
    /// means it can be removed if the resulting value isn't used.
    pub can_be_removed_if_unused: bool,

    /// If true, this property access is a function that, when called, can be
    /// unwrapped if the resulting value is unused. Unwrapping means discarding
    /// the call target but keeping any arguments with side effects.
    pub call_can_be_unwrapped_if_unused: CallUnwrap,

    /// `target` is an `EImportIdentifier` and this read was counted in
    /// `Part::import_symbol_property_uses` instead of as a use of the import,
    /// so the linker may bind it straight to the export it names
    /// (`LinkerGraph::import_member_bindings`).
    pub is_import_property_use: bool,
}
impl Default for Dot {
    fn default() -> Self {
        Self {
            target: ExprNodeIndex::EMPTY,
            name: Str::EMPTY,
            name_loc: crate::Loc::EMPTY,
            optional_chain: None,
            can_be_removed_if_unused: false,
            call_can_be_unwrapped_if_unused: CallUnwrap::Never,
            is_import_property_use: false,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Index {
    pub index: ExprNodeIndex,
    pub target: ExprNodeIndex,
    pub optional_chain: Option<OptionalChain>,
    /// See `Dot::is_import_property_use`.
    pub is_import_property_use: bool,
}

#[derive(Clone, Copy)]
pub struct Arrow {
    pub args: crate::StoreSlice<G::Arg>,
    pub body: G::FnBody,

    pub is_async: bool,
    pub has_rest_arg: bool,
    /// Use shorthand if true and "Body" is a single return statement
    pub prefer_expr: bool,
    /// See `flags::Function::HasReactHooksSuppression`.
    pub has_react_hooks_suppression: bool,
}
// The fourth bool fits in existing trailing padding. Com índices de 4 bytes, `args` tem 8 e `body` 12.
const _: () = assert!(core::mem::size_of::<Arrow>() == 24);
impl Arrow {
    pub const NOOP_RETURN_UNDEFINED: Arrow = Arrow {
        args: crate::StoreSlice::EMPTY,
        body: G::FnBody {
            loc: crate::Loc::EMPTY,
            stmts: crate::StoreSlice::EMPTY,
        },
        is_async: false,
        has_rest_arg: false,
        prefer_expr: false,
        has_react_hooks_suppression: false,
    };
}
impl Default for Arrow {
    fn default() -> Self {
        Self::NOOP_RETURN_UNDEFINED
    }
}

#[derive(Clone, Default)]
pub struct Function {
    pub func: G::Fn,
}

/// 8-byte identifier expression payload. The three side-effect flags are packed
/// into `Ref`'s user-bit lane (bits 28..31, masked out of `Ref` identity) so
/// this, the most common `expr::Data` variant, fits in a single word.
///
/// `ref_` remains a public field so the ~100 existing `id.ref_` /
/// `Identifier { ref_, ..Default::default() }` sites stay untouched; flag
/// access goes through the accessor methods below.
///
/// **Hazard:** assigning a fresh `Ref` to `ref_` *clears the flags*. This is
/// fine for `visit_expr`'s `e_identifier` (sets `ref_` first then re-derives
/// the flags), but any `id.ref_ = new_ref` site that expects the
/// flags to survive must instead write
/// `id.ref_ = new_ref.with_user_bits_from(id.ref_)`: see `handle_identifier`.
#[derive(Clone, Copy)]
pub struct Identifier {
    pub ref_: Ref,
}
impl Default for Identifier {
    #[inline]
    fn default() -> Self {
        Self { ref_: Ref::NONE }
    }
}
impl Identifier {
    #[inline]
    pub const fn init(ref_: Ref) -> Identifier {
        Identifier { ref_ }
    }

    /// If we're inside a "with" statement, this identifier may be a property
    /// access. In that case it would be incorrect to remove this identifier since
    /// the property access may be a getter or setter with side effects.
    #[inline]
    pub const fn must_keep_due_to_with_stmt(self) -> bool {
        self.ref_.user_bit(0)
    }
    #[inline]
    pub fn set_must_keep_due_to_with_stmt(&mut self, v: bool) {
        self.ref_.set_user_bit(0, v);
    }

    /// If true, this identifier is known to not have a side effect (i.e. to not
    /// throw an exception) when referenced. If false, this identifier may or may
    /// not have side effects when referenced. This is used to allow the removal
    /// of known globals such as "Object" if they aren't used.
    #[inline]
    pub const fn can_be_removed_if_unused(self) -> bool {
        self.ref_.user_bit(1)
    }
    #[inline]
    pub fn set_can_be_removed_if_unused(&mut self, v: bool) {
        self.ref_.set_user_bit(1, v);
    }

    /// If true, this identifier represents a function that, when called, can be
    /// unwrapped if the resulting value is unused. Unwrapping means discarding
    /// the call target but keeping any arguments with side effects.
    #[inline]
    pub const fn call_can_be_unwrapped_if_unused(self) -> bool {
        self.ref_.user_bit(2)
    }
    #[inline]
    pub fn set_call_can_be_unwrapped_if_unused(&mut self, v: bool) {
        self.ref_.set_user_bit(2, v);
    }

    // Builder-style setters for the handful of sites that set flags up front.
    #[inline]
    pub const fn with_must_keep_due_to_with_stmt(self, v: bool) -> Self {
        Self {
            ref_: self.ref_.with_user_bit(0, v),
        }
    }
    #[inline]
    pub const fn with_can_be_removed_if_unused(self, v: bool) -> Self {
        Self {
            ref_: self.ref_.with_user_bit(1, v),
        }
    }
    #[inline]
    pub const fn with_call_can_be_unwrapped_if_unused(self, v: bool) -> Self {
        Self {
            ref_: self.ref_.with_user_bit(2, v),
        }
    }
}

/// This is similar to an `Identifier` but it represents a reference to an ES6
/// import item.
///
/// Depending on how the code is linked, the file containing this EImportIdentifier
/// may or may not be in the same module group as the file it was imported from.
///
/// If it's the same module group than we can just merge the import item symbol
/// with the corresponding symbol that was imported, effectively renaming them
/// to be the same thing and statically binding them together.
///
/// But if it's a different module group, then the import must be dynamically
/// evaluated using a property access off the corresponding namespace symbol,
/// which represents the result of a require() call.
///
/// It's stored as a separate type so it's not easy to confuse with a plain
/// identifier. For example, it'd be bad if code trying to convert "{x: x}" into
/// "{x}" shorthand syntax wasn't aware that the "x" in this case is actually
/// "{x: importedNamespace.x}". This separate type forces code to opt-in to
/// doing this instead of opt-out.
/// 8-byte import-identifier payload: `was_originally_identifier` rides in
/// `Ref` user bit 0 (see `Identifier` doc for the packing rationale).
#[derive(Clone, Copy)]
pub struct ImportIdentifier {
    pub ref_: Ref,
}
impl Default for ImportIdentifier {
    #[inline]
    fn default() -> Self {
        Self { ref_: Ref::NONE }
    }
}
impl ImportIdentifier {
    #[inline]
    pub const fn new(ref_: Ref, was_originally_identifier: bool) -> Self {
        // Strip any incoming user bits (the caller may pass an
        // `E::Identifier.ref_` carrying its own flags in bits 1/2) before
        // applying ours, so foreign flags can't leak into this node.
        Self {
            ref_: ref_
                .without_user_bits()
                .with_user_bit(0, was_originally_identifier),
        }
    }

    /// If true, this was originally an identifier expression such as "foo". If
    /// false, this could potentially have been a member access expression such
    /// as "ns.foo" off of an imported namespace object.
    #[inline]
    pub const fn was_originally_identifier(self) -> bool {
        self.ref_.user_bit(0)
    }
}

/// This is a dot expression on exports, such as `exports.<ref>`. It is given
/// it's own AST node to allow CommonJS unwrapping, in which this can just be
/// the identifier in the Ref
/// 8-byte CJS-export-identifier payload: `base` rides in `Ref` user bit 0
/// (`Exports` = 0, `ModuleDotExports` = 1; see `Identifier` doc for packing
/// rationale).
#[derive(Clone, Copy)]
pub struct CommonJSExportIdentifier {
    pub ref_: Ref,
}
impl Default for CommonJSExportIdentifier {
    #[inline]
    fn default() -> Self {
        Self { ref_: Ref::NONE }
    }
}
impl CommonJSExportIdentifier {
    #[inline]
    pub const fn new(ref_: Ref, base: CommonJSExportIdentifierBase) -> Self {
        // Strip any incoming user bits before applying ours: see
        // `ImportIdentifier::new`.
        Self {
            ref_: ref_.without_user_bits().with_user_bit(
                0,
                matches!(base, CommonJSExportIdentifierBase::ModuleDotExports),
            ),
        }
    }
    #[inline]
    pub const fn base(self) -> CommonJSExportIdentifierBase {
        if self.ref_.user_bit(0) {
            CommonJSExportIdentifierBase::ModuleDotExports
        } else {
            CommonJSExportIdentifierBase::Exports
        }
    }
}

/// The original variant of the dot expression must be known so that in the case that we
/// - fail to convert this to ESM
/// - ALSO see an assignment to `module.exports` (commonjs_module_exports_assigned_deoptimized)
/// It must be known if `exports` or `module.exports` was written in source
/// code, as the distinction will alter behavior. The fixup happens in the printer when
/// printing this node.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CommonJSExportIdentifierBase {
    Exports,
    ModuleDotExports,
}

/// This is similar to EIdentifier but it represents class-private fields and
/// methods. It can be used where computed properties can be used, such as
/// EIndex and Property.
#[derive(Clone, Copy)]
pub struct PrivateIdentifier {
    pub ref_: Ref,
}

/// In development mode, the new JSX transform has a few special props
/// - `React.jsxDEV(type, arguments, key, isStaticChildren, source, self)`
/// - `arguments`:
///      `{ ...props, children: children, }`
/// - `source`: https://github.com/babel/babel/blob/ef87648f3f05ccc393f89dea7d4c7c57abf398ce/packages/babel-plugin-transform-react-jsx-source/src/index.js#L24-L48
///   ```text
///   {
///      fileName: string | null,
///      columnNumber: number | null,
///      lineNumber: number | null,
///   }
///   ```
/// - `children`:
///     - static the function is React.jsxsDEV, "jsxs" instead of "jsx"
///     - one child? the function is React.jsxDEV,
///     - no children? the function is React.jsxDEV and children is an empty array.
/// `isStaticChildren`: https://github.com/facebook/react/blob/4ca62cac45c288878d2532e5056981d177f9fdac/packages/react/src/jsx/ReactJSXElementValidator.js#L369-L384
///     This flag means children is an array of JSX Elements literals.
///     The documentation on this is sparse, but it appears that
///     React just calls Object.freeze on the children array.
///     Object.freeze, historically, is quite a bit slower[0] than just not doing that.
///     Given that...I am choosing to always pass "false" to this.
///     This also skips extra state that we'd need to track.
///     If React Fast Refresh ends up using this later, then we can revisit this decision.
///  [0]: https://github.com/automerge/automerge/issues/177
#[derive(Clone, Copy)]
pub struct JSXElement {
    /// JSX tag name
    /// `<div>` => E.String.init("div")
    /// `<MyComponent>` => E.Identifier{.ref = symbolPointingToMyComponent }
    /// null represents a fragment
    pub tag: Option<ExprNodeIndex>,

    /// JSX props
    pub properties: G::PropertyList,

    /// JSX element children `<div>{this_is_a_child_element}</div>`
    pub children: ExprNodeList,

    /// needed to make sure parse and visit happen in the same order
    pub key_prop_index: i32,

    pub flags: crate::flags::JSXElementBitset,

    pub close_tag_loc: crate::Loc,
}
impl Default for JSXElement {
    fn default() -> Self {
        Self {
            tag: None,
            properties: G::PropertyList::EMPTY,
            children: ExprNodeList::EMPTY,
            key_prop_index: -1,
            flags: crate::flags::JSXElementBitset::default(),
            close_tag_loc: crate::Loc::EMPTY,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JSXSpecialProp {
    /// old react transform used this as a prop
    UnderscoreSelf,
    UnderscoreSource,
    Key,
    Ref,
    Any,
}
bun_core::comptime_string_map! {
    static JSX_SPECIAL_PROP_MAP: JSXSpecialProp = {
        b"key" => JSXSpecialProp::Key,
        b"ref" => JSXSpecialProp::Ref,
        b"__self" => JSXSpecialProp::UnderscoreSelf,
        b"__source" => JSXSpecialProp::UnderscoreSource,
    };
}

impl JSXSpecialProp {
    #[inline]
    pub fn from_bytes(s: &[u8]) -> Option<Self> {
        JSX_SPECIAL_PROP_MAP.get(s).copied()
    }
}

/// `packed(4)` lowers the alignment to 4 so `expr::Data` (and therefore `Expr`)
/// drop to align 4 and pack into 12/16 bytes; the `f64` stays a single scalar so
/// `.value()` is one `movsd`/`ldr`. The field is private because comparisons on
/// a packed `f64` field would require `&self.value` (an unaligned reference);
/// callers go through `value()` / `new()` which copy by value.
#[derive(Clone, Copy)]
#[repr(C, packed(4))]
pub struct Number {
    value: f64,
}

const _: () = assert!(core::mem::size_of::<Number>() == 8);
const _: () = assert!(core::mem::align_of::<Number>() == 4);

impl Number {
    #[inline(always)]
    pub const fn new(value: f64) -> Self {
        Number { value }
    }
    #[inline(always)]
    pub const fn value(self) -> f64 {
        self.value
    }
}
impl From<f64> for Number {
    #[inline]
    fn from(v: f64) -> Self {
        Number::new(v)
    }
}

impl Number {
    /// String concatenation with numbers is required by the TypeScript compiler for
    /// "constant expression" handling in enums. We can match the behavior of a JS VM
    /// by calling out to the APIs in WebKit which are responsible for this operation.
    ///
    /// No upstream, a faixa de -100 a 100 vem de duas tabelas `'static` para não alocar; no porte toda string mora
    /// no `Store`, então o texto (o mesmo) é gravado nele. A conversão do WebKit vira `FormatDouble::dtoa`.
    pub(crate) fn to_string(self, store: &mut Store) -> Option<Str> {
        Self::to_string_from_f64(self.value(), store)
    }

    pub(crate) fn to_string_from_f64(value: f64, store: &mut Store) -> Option<Str> {
        if value == value.trunc() && (value < i32::MAX as f64 && value > i32::MIN as f64) {
            let int_value = value as i64;
            return Some(store.alloc_str(format!("{}", int_value as i32).as_bytes()));
        }

        if value.is_nan() {
            return Some(store.alloc_str(b"NaN"));
        }

        if value.is_infinite() && value.is_sign_negative() {
            return Some(store.alloc_str(b"-Infinity"));
        }

        if value.is_infinite() {
            return Some(store.alloc_str(b"Infinity"));
        }

        let mut buf = [0u8; 124];
        let s = bun_core::fmt::FormatDouble::dtoa(&mut buf, value);
        Some(store.alloc_str(s))
    }

    #[inline]
    pub fn to_usize(self) -> usize {
        self.to::<usize>()
    }

    #[inline]
    pub(crate) fn to_u32(self) -> u32 {
        self.to::<u32>()
    }

    pub(crate) fn to<T: NumberCast>(self) -> T {
        let clamped = self.value().trunc().max(0.0).min(T::MAX_AS_F64);
        T::from_f64(clamped)
    }
}

/// Helper trait for `Number::to<T>()`.
pub trait NumberCast: Copy {
    const MAX_AS_F64: f64;
    fn from_f64(v: f64) -> Self;
}
macro_rules! impl_number_cast {
    ($($t:ty),*) => {$(
        impl NumberCast for $t {
            const MAX_AS_F64: f64 = {
                let max = <$t>::MAX as f64;
                if max > f64::MAX { f64::MAX } else { max }
            };
            #[inline] fn from_f64(v: f64) -> Self { v as $t }
        }
    )*};
}
impl_number_cast!(u32, usize);

#[derive(Clone, Copy)]
pub struct BigInt {
    pub value: Str,
}
impl BigInt {
    /// `value` may carry a `0x`/`0o`/`0b` prefix. A leading zero is otherwise
    /// a syntax error, so any literal that starts with `0` and has more than
    /// one character is a radix literal.
    #[inline]
    pub(crate) fn has_radix(v: &[u8]) -> bool {
        v.len() >= 2 && v[0] == b'0'
    }

    /// `Some(equal)` when the comparison is decidable from the source text, or
    /// `None` when either side has a radix prefix and the values differ.
    pub fn check_equality(a: &[u8], b: &[u8]) -> Option<bool> {
        if a == b {
            return Some(true);
        }
        if !Self::has_radix(a) && !Self::has_radix(b) {
            return Some(false);
        }
        None
    }
}

#[derive(Clone, Copy)]
pub struct Object {
    pub properties: G::PropertyList,
    pub comma_after_spread: crate::Loc,
    pub is_single_line: bool,
    pub is_parenthesized: bool,
    pub was_originally_macro: bool,

    pub close_brace_loc: crate::Loc,
}
impl Default for Object {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Dotted-key path used by the INI parser (`get_or_put_object`).
///
/// No upstream `next` é um `*mut Rope` no bump; no porte é o índice do próximo segmento no `Store`.
#[derive(Clone, Copy)]
pub struct Rope {
    pub head: Expr,
    pub next: Option<StoreRef<Rope>>,
}
impl Rope {
    pub fn append(store: &mut Store, this: StoreRef<Rope>, expr: Expr) -> StoreRef<Rope> {
        // Walk to the tail iteratively: recursing once per node overflows the
        // native stack on adversarially deep ropes (e.g. an `.npmrc` section
        // header with thousands of dot-separated segments).
        let mut tail = this;
        while let Some(next) = store[tail].next {
            tail = next;
        }
        let rope = store.alloc(Rope { head: expr, next: None });
        store[tail].next = Some(rope);
        rope
    }
}

#[derive(Debug, strum::IntoStaticStr)]
pub enum SetError {
    OutOfMemory,
    Clobber,
}

impl core::fmt::Display for SetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(<&'static str>::from(self))
    }
}

impl core::error::Error for SetError {}

// ── live Object accessor surface ───────────────────────────────────────────
impl Object {
    pub const EMPTY: Object = Object {
        properties: G::PropertyList::EMPTY,
        comma_after_spread: crate::Loc::EMPTY,
        is_single_line: false,
        is_parenthesized: false,
        was_originally_macro: false,
        close_brace_loc: crate::Loc::EMPTY,
    };

    pub fn get(&self, store: &Store, key: &[u8]) -> Option<Expr> {
        self.as_property(store, key).map(|q| q.expr)
    }

    pub fn as_property(&self, store: &Store, name: &[u8]) -> Option<crate::expr::Query> {
        for (i, prop) in store[self.properties].iter().enumerate() {
            let Some(value) = prop.value else { continue };
            let Some(key) = &prop.key else { continue };
            let crate::expr::Data::EString(key_str) = key.data else {
                continue;
            };
            if store[key_str].eql_bytes(store, name) {
                return Some(crate::expr::Query {
                    expr: value,
                    loc: key.loc,
                    i: i as u32,
                });
            }
        }
        None
    }

    pub fn has_property(&self, store: &Store, name: &[u8]) -> bool {
        for prop in store[self.properties].iter() {
            let Some(key) = &prop.key else { continue };
            let crate::expr::Data::EString(key_str) = key.data else {
                continue;
            };
            if store[key_str].eql_bytes(store, name) {
                return true;
            }
        }
        false
    }

    pub fn put(store: &mut Store, this: StoreRef<Object>, key: &[u8], expr: Expr) {
        let object = store[this];
        if let Some(q) = object.as_property(store, key) {
            store[object.properties][q.i as usize].value = Some(expr);
        } else {
            let key_str = store.alloc_str(key);
            let key = Expr::init(store, EString::init(key_str), expr.loc);
            let property = G::Property {
                flags: own_key_property_flags(store, &key),
                key: Some(key),
                value: Some(expr),
                ..G::Property::default()
            };
            store[this].properties = store.append(object.properties, property);
        }
    }

    pub fn put_string(store: &mut Store, this: StoreRef<Object>, key: &[u8], value: &[u8]) {
        let value_str = store.alloc_str(value);
        let value = Expr::init(store, EString::init(value_str), crate::Loc::EMPTY);
        Self::put(store, this, key, value)
    }

    /// Walks `rope` segments, creating nested objects as needed, and returns
    /// the leaf `E.Object` expression.
    pub fn get_or_put_object(
        store: &mut Store,
        this: StoreRef<Object>,
        rope: StoreRef<Rope>,
    ) -> Result<Expr, SetError> {
        let Rope { head, next } = store[rope];
        let head_key = match head.data.e_string() {
            Some(s) => store[s].data,
            None => return Err(SetError::Clobber),
        };
        let head_key = store.str(head_key).to_vec();
        if let Some(existing) = store[this].get(store, &head_key) {
            match existing.data {
                crate::expr::Data::EArray(array) => {
                    let Some(next) = next else {
                        return Err(SetError::Clobber);
                    };
                    if let Some(last) = store[store[array].items].last() {
                        if let crate::expr::Data::EObject(obj) = last.data {
                            return Self::get_or_put_object(store, obj, next);
                        }
                        return Err(SetError::Clobber);
                    }
                    return Err(SetError::Clobber);
                }
                crate::expr::Data::EObject(object) => {
                    if let Some(next) = next {
                        return Self::get_or_put_object(store, object, next);
                    }
                    return Ok(existing);
                }
                _ => return Err(SetError::Clobber),
            }
        }

        if let Some(next) = next {
            let obj = Expr::init(store, Object::default(), head.loc);
            let out = match obj.data {
                crate::expr::Data::EObject(o) => Self::get_or_put_object(store, o, next)?,
                _ => unreachable!(),
            };
            Self::append_owned_key(store, this, head, obj);
            return Ok(out);
        }

        let out = Expr::init(store, Object::default(), head.loc);
        Self::append_owned_key(store, this, head, out);
        Ok(out)
    }

    /// As duas pontas de `get_or_put_object` acrescentam a mesma propriedade (`key`, `value` e as flags de chave
    /// própria); é o `append_property` do upstream, com a ordem de campos do literal dele.
    fn append_owned_key(store: &mut Store, this: StoreRef<Object>, key: Expr, value: Expr) {
        let property = G::Property {
            key: Some(key),
            value: Some(value),
            flags: own_key_property_flags(store, &key),
            ..G::Property::default()
        };
        let properties = store[this].properties;
        store[this].properties = store.append(properties, property);
    }
}

/// Data-file parsers (JSON/JSON5/TOML/YAML) define every key as an own
/// property, so an own `"__proto__"` string key must be marked computed:
/// a plain `"__proto__":` key in a printed object literal sets the prototype.
pub fn own_key_property_flags(store: &Store, key: &Expr) -> crate::flags::PropertySet {
    match key.data {
        crate::expr::Data::EString(key_str) if store[key_str].eql_comptime(store, b"__proto__") => {
            crate::flags::Property::IsComputed.into()
        }
        _ => crate::flags::PROPERTY_NONE,
    }
}

impl Object {
    /// Appends a property without checking for an existing key. Callers that
    /// need duplicate detection must check `as_property` with UTF-8 bytes
    /// first: a UTF-16 EString key's raw `data` view is not byte-comparable.
    pub fn append_property(store: &mut Store, this: StoreRef<Object>, key: Expr, value: Expr) {
        Self::append_owned_key(store, this, key, value)
    }

    /// Assumes each key in the property is a string
    pub fn alphabetize_properties(&self, store: &mut Store) {
        #[cfg(debug_assertions)]
        {
            for prop in store[self.properties].iter() {
                debug_assert!(matches!(
                    prop.key.as_ref().expect("infallible: prop has key").data,
                    crate::expr::Data::EString(_)
                ));
            }
        }
        self.sort_properties(store, object_sorter_is_less_than);
    }

    pub fn package_json_sort(&self, store: &mut Store) {
        self.sort_properties(store, package_json_sort_is_less_than);
    }

    /// O `index_sort::sort_slice_by` do upstream sobre `properties`: ordenação estável, como o `sort_by`. As
    /// propriedades são copiadas para fora para a comparação poder ler as chaves no `Store`.
    fn sort_properties(&self, store: &mut Store, cmp: fn(&Store, &G::Property, &G::Property) -> Ordering) {
        let mut properties = store[self.properties].to_vec();
        properties.sort_by(|lhs, rhs| cmp(store, lhs, rhs));
        store[self.properties].clone_from_slice(&properties);
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PackageJsonSortFields {
    Name = 0,
    Version = 1,
    Author = 2,
    Repository = 3,
    Config = 4,
    Main = 5,
    Module = 6,
    Dependencies = 7,
    DevDependencies = 8,
    OptionalDependencies = 9,
    PeerDependencies = 10,
    Exports = 11,
    Fake = 12,
}

bun_core::comptime_string_map! {
    static PACKAGE_JSON_SORT_MAP: PackageJsonSortFields = {
        b"name" => PackageJsonSortFields::Name,
        b"version" => PackageJsonSortFields::Version,
        b"author" => PackageJsonSortFields::Author,
        b"repository" => PackageJsonSortFields::Repository,
        b"config" => PackageJsonSortFields::Config,
        b"main" => PackageJsonSortFields::Main,
        b"module" => PackageJsonSortFields::Module,
        b"dependencies" => PackageJsonSortFields::Dependencies,
        b"devDependencies" => PackageJsonSortFields::DevDependencies,
        b"optionalDependencies" => PackageJsonSortFields::OptionalDependencies,
        b"peerDependencies" => PackageJsonSortFields::PeerDependencies,
        b"exports" => PackageJsonSortFields::Exports,
    };
}

/// Os bytes da chave `EString` de uma propriedade (as duas ordenações supõem chave string).
fn property_key_bytes<'s>(store: &'s Store, property: &G::Property) -> &'s [u8] {
    let key = property.key.as_ref().unwrap().data.e_string().expect("infallible: variant checked");
    store.str(store[key].data)
}

fn package_json_sort_is_less_than(store: &Store, lhs: &G::Property, rhs: &G::Property) -> Ordering {
    let key_size = |property: &G::Property| -> u8 {
        if let Some(k) = &property.key {
            if let crate::expr::Data::EString(s) = k.data {
                return *PACKAGE_JSON_SORT_MAP
                    .get(store.str(store[s].data))
                    .unwrap_or(&PackageJsonSortFields::Fake) as u8;
            }
        }
        PackageJsonSortFields::Fake as u8
    };

    match key_size(lhs).cmp(&key_size(rhs)) {
        // Full `Ordering` so this is usable with `sort_by`.
        Ordering::Equal => object_sorter_is_less_than(store, lhs, rhs),
        ord => ord,
    }
}

fn object_sorter_is_less_than(store: &Store, lhs: &G::Property, rhs: &G::Property) -> Ordering {
    property_key_bytes(store, lhs).cmp(property_key_bytes(store, rhs))
}

#[derive(Clone, Copy)]
pub struct Spread {
    pub value: ExprNodeIndex,
}

/// Discriminants are shared with the C++ switch in
/// `Bun__Temporal__fromDateTimeLiteral`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum TomlDateTimeKind {
    /// `1979-05-27T00:32:00-07:00` → `Temporal.Instant`
    OffsetDateTime = 1,
    /// `1979-05-27T07:32:00` → `Temporal.PlainDateTime`
    LocalDateTime = 2,
    /// `1979-05-27` → `Temporal.PlainDate`
    LocalDate = 3,
    /// `07:32:00` → `Temporal.PlainTime`
    LocalTime = 4,
}

impl TomlDateTimeKind {
    /// Unqualified Temporal class name (`Instant`, `PlainDateTime`, …).
    pub fn temporal_class(self) -> &'static [u8] {
        match self {
            TomlDateTimeKind::OffsetDateTime => b"Instant",
            TomlDateTimeKind::LocalDateTime => b"PlainDateTime",
            TomlDateTimeKind::LocalDate => b"PlainDate",
            TomlDateTimeKind::LocalTime => b"PlainTime",
        }
    }
}

/// JavaScript string literal type
///
/// Todos os campos são `Copy` (faixa, índices e escalares), então o tipo é `Copy`; o upstream não deriva para deixar
/// explícita a posse da corda, e `shallow_clone` continua existindo com esse nome. Quando `is_utf16`, `data` é uma
/// faixa do vetor UTF-16 do `Store` e o tamanho é a contagem de unidades, como no upstream.
#[derive(Clone, Copy)]
pub struct EString {
    // A version of this where `utf8` and `value` are stored in a packed union, with len as a single u32 was attempted.
    // It did not improve benchmarks. Neither did converting this from a heap-allocated type to a stack-allocated type.
    // TODO: change this to *const anyopaque and change all uses to either .slice8() or .slice16()
    pub data: Str,

    // A very simple rope implementation
    // We only use this for string folding, so this is kind of overkill
    // We don't need to deal with substrings
    pub next: Option<StoreRef<EString>>,
    pub end: Option<StoreRef<EString>>,
    pub rope_len: u32,
    pub prefer_template: bool,
    pub is_utf16: bool,
    /// Set only by the TOML parser on a date/time literal (`data` is its
    /// ASCII source text). The TOML AST never enters the JS visit passes;
    /// the sinks that materialize or print it check this tag and produce a
    /// Temporal value instead of a string.
    pub toml_datetime: Option<TomlDateTimeKind>,
}
// Also exported as `String`; `EString` avoids colliding with bun_core::String.
pub use EString as String;

impl Default for EString {
    fn default() -> Self {
        Self::EMPTY
    }
}

// Minimal live surface for `IntoExprData` / `Data` / `lexer.rs` callers.
impl EString {
    #[inline]
    pub const fn is_utf8(&self) -> bool {
        !self.is_utf16
    }
    #[inline]
    pub fn slice8<'s>(&self, store: &'s Store) -> &'s [u8] {
        debug_assert!(!self.is_utf16);
        store.str(self.data)
    }
    #[inline]
    pub fn slice16<'s>(&self, store: &'s Store) -> &'s [u16] {
        debug_assert!(self.is_utf16);
        store.str16(self.data)
    }
    /// `data` is arena-owned (source text or `Store`).
    pub const fn init(data: Str) -> Self {
        Self {
            data,
            prefer_template: false,
            next: None,
            end: None,
            rope_len: 0,
            is_utf16: false,
            toml_datetime: None,
        }
    }

    /// A TOML date/time literal; `data` must be ASCII text `Temporal.*.from`
    /// accepts verbatim.
    pub fn init_toml_datetime(data: Str, kind: TomlDateTimeKind) -> Self {
        Self {
            data,
            toml_datetime: Some(kind),
            ..Default::default()
        }
    }
    /// Construct from a UTF-16 range of the `Store` (written by `alloc_str16`). The range length is the **u16
    /// element count**; `slice16()` and friends rely on this.
    pub fn init_utf16(data: Str) -> Self {
        Self {
            data,
            is_utf16: true,
            ..Default::default()
        }
    }
    /// E.String containing non-ascii characters may not fully work.
    /// https://github.com/oven-sh/bun/issues/11963
    /// More investigation is needed.
    pub fn init_re_encode_utf8(store: &mut Store, utf8: Str) -> EString {
        if strings::first_non_ascii(store.str(utf8)).is_none() {
            Self::init(utf8)
        } else {
            // `fail_if_invalid = false` means the only possible error is `OutOfMemory`.
            let utf16 = strings::to_utf16_alloc_for_real(store.str(utf8), false, false)
                .expect("to_utf16_alloc_for_real com fail_if_invalid = false");
            let data = store.alloc_str16(&utf16);
            Self::init_utf16(data)
        }
    }
    /// Ensure `data` is UTF-8 (transcode from UTF-16 rope if needed).
    pub fn to_utf8(&mut self, store: &mut Store) {
        if !self.is_utf16 {
            return;
        }
        let v = strings::to_utf8_alloc(self.slice16(store));
        self.data = store.alloc_str(&v);
        self.is_utf16 = false;
    }
}

// ── live EString accessor surface ──────────────────────────────────────────
impl EString {
    #[inline]
    pub fn len(&self) -> usize {
        if self.rope_len > 0 {
            self.rope_len as usize
        } else {
            self.data.len()
        }
    }
    #[inline]
    pub fn is_blank(&self) -> bool {
        self.len() == 0
    }
    #[inline]
    pub fn is_present(&self) -> bool {
        self.len() > 0
    }

    /// Flatten any rope and return UTF-8 bytes.
    /// Resolves the rope into the `Store`, then transcodes if UTF-16.
    pub fn slice(&mut self, store: &mut Store) -> Str {
        self.resolve_rope_if_needed(store);
        self.string(store)
    }

    pub fn eql_bytes(&self, store: &Store, other: &[u8]) -> bool {
        if self.is_utf8() {
            strings::eql_long(store.str(self.data), other, true)
        } else {
            strings::utf16_eql_string(self.slice16(store), other)
        }
    }

    pub fn eql_comptime(&self, store: &Store, value: &'static [u8]) -> bool {
        if !self.is_utf8() {
            debug_assert!(self.next.is_none(), "transpiler: utf-16 string is a rope");
            return strings::eql_comptime_utf16(self.slice16(store), value);
        }
        if self.next.is_none() {
            return store.str(self.data) == value;
        }
        self.eql8_rope(store, value)
    }

    fn eql8_rope(&self, store: &Store, value: &[u8]) -> bool {
        debug_assert!(self.next.is_some() && self.is_utf8());
        if self.rope_len as usize != value.len() {
            return false;
        }
        let mut i = 0usize;
        let mut next: Option<&EString> = Some(self);
        while let Some(cur) = next {
            if !strings::eql_long(store.str(cur.data), &value[i..i + cur.data.len()], false) {
                return false;
            }
            i += cur.data.len();
            next = cur.next.map(|r| &store[r]);
        }
        true
    }

    /// Flatten in place. Parser only; shared-AST readers use [`Self::flattened`].
    pub fn resolve_rope_if_needed(&mut self, store: &mut Store) {
        if self.next.is_none() || !self.is_utf8() {
            return;
        }
        self.data = self.flatten_rope(store);
        self.next = None;
    }

    /// `self` if not a rope, else a copy flattened into the `Store`. Never writes to `self`.
    ///
    /// No upstream o resultado é `Flattened` (o nó emprestado ou uma cópia); com `EString: Copy` a cópia por valor
    /// basta nos dois casos.
    pub fn flattened(&self, store: &mut Store) -> EString {
        if self.next.is_none() || !self.is_utf8() {
            return *self;
        }
        let mut copy = self.shallow_clone();
        copy.data = self.flatten_rope(store);
        copy.next = None;
        copy
    }

    /// The rope's bytes, concatenated into a fresh `Store` range.
    fn flatten_rope(&self, store: &mut Store) -> Str {
        let mut bytes = Vec::<u8>::with_capacity(self.rope_len as usize);
        bytes.extend_from_slice(store.str(self.data));
        let mut str_ = self.next;
        while let Some(part) = str_ {
            bytes.extend_from_slice(store.str(store[part].data));
            str_ = store[part].next;
        }
        store.alloc_str(&bytes)
    }

    /// Return UTF-8 bytes, transcoding into the `Store` if UTF-16.
    pub fn string(&self, store: &mut Store) -> Str {
        if self.is_utf8() {
            self.data
        } else {
            let v = strings::to_utf8_alloc(self.slice16(store));
            store.alloc_str(&v)
        }
    }

    pub(crate) fn string_cloned(&self, store: &mut Store) -> Str {
        if self.is_utf8() {
            let v = store.str(self.data).to_vec();
            store.alloc_str(&v)
        } else {
            self.string(store)
        }
    }

    pub fn hash(&self, store: &Store) -> u64 {
        if self.is_blank() {
            return 0;
        }
        if self.is_utf8() {
            bun_wyhash::hash(store.str(self.data))
        } else {
            // O upstream reinterpreta as unidades como bytes (`bytemuck::cast_slice`), na ordem nativa; aqui os
            // mesmos bytes são montados em little-endian, a ordem das máquinas que o bun suporta.
            let bytes: Vec<u8> = self.slice16(store).iter().flat_map(|unit| unit.to_le_bytes()).collect();
            bun_wyhash::hash(&bytes)
        }
    }
}

// ── EString surface ────────────────────────────────────────────────────────
// Ordering / equality / const-literal / rope-mutation helpers.
impl EString {
    pub const EMPTY: EString = EString::init(Str::EMPTY);

    /// Compares two strings lexicographically for JavaScript semantics.
    /// Both strings must share the same encoding (UTF-8 vs UTF-16).
    #[inline]
    pub fn order(&self, store: &Store, other: &EString) -> Ordering {
        debug_assert!(self.is_utf8() == other.is_utf8());
        if self.is_utf8() {
            strings::order(store.str(self.data), store.str(other.data))
        } else {
            strings::order_t(self.slice16(store), other.slice16(store))
        }
    }

    pub fn javascript_length(&self, store: &Store) -> Option<u32> {
        if self.rope_len > 0 {
            // We only support ascii ropes for now
            return Some(self.rope_len);
        }
        if self.is_utf8() {
            if !strings::is_all_ascii(store.str(self.data)) {
                return None;
            }
            return Some(self.data.len() as u32);
        }
        Some(self.slice16(store).len() as u32)
    }

    // `eql`, split by operand type.
    pub fn eql_string(&self, store: &Store, other: &EString) -> bool {
        if self.is_utf8() {
            if other.is_utf8() {
                strings::eql_long(store.str(self.data), store.str(other.data), true)
            } else {
                strings::utf16_eql_string(other.slice16(store), store.str(self.data))
            }
        } else if other.is_utf8() {
            strings::utf16_eql_string(self.slice16(store), store.str(other.data))
        } else {
            self.slice16(store) == other.slice16(store)
        }
    }

    /// Shallow field-wise copy.
    #[inline]
    pub fn shallow_clone(&self) -> EString {
        *self
    }

    /// Link `other` onto this string's rope tail.
    ///
    /// `other` is a `Store` node; `self` may be a copy taken out of the `Store` (the caller writes it back) or a
    /// cooked template part.
    pub(crate) fn push(&mut self, store: &mut Store, other: StoreRef<EString>) {
        debug_assert!(self.is_utf8());
        debug_assert!(store[other].is_utf8());

        if store[other].rope_len == 0 {
            store[other].rope_len = store[other].data.len() as u32;
        }
        if self.rope_len == 0 {
            self.rope_len = self.data.len() as u32;
        }
        self.rope_len += store[other].rope_len;

        if self.next.is_none() {
            self.next = Some(other);
            self.end = Some(other);
        } else {
            let mut end = self.end.unwrap();
            while store[end].next.is_some() {
                end = store[end].end.unwrap();
            }
            store[end].next = Some(other);
            self.end = Some(other);
        }
    }
}

fn array_sorter_is_less_than(store: &Store, lhs: &Expr, rhs: &Expr) -> Ordering {
    let lhs = lhs.data.e_string().unwrap();
    let rhs = rhs.data.e_string().expect("infallible: variant checked");
    store[lhs].order(store, &store[rhs])
}

// value is in the Node
#[derive(Clone, Copy)]
pub struct TemplatePart {
    pub value: ExprNodeIndex,
    pub tail_loc: crate::Loc,
    pub tail: TemplateContents,
}

#[derive(Clone, Copy)]
pub struct Template {
    pub tag: Option<ExprNodeIndex>,
    pub parts: crate::StoreSlice<TemplatePart>,
    pub head: TemplateContents,
}

impl Template {
    /// Empty `StoreSlice<TemplatePart>` for parts-less templates (e.g. tagged
    /// no-substitution literals).
    #[inline]
    pub fn empty_parts() -> crate::StoreSlice<TemplatePart> {
        crate::StoreSlice::EMPTY
    }

    #[inline]
    pub fn parts<'s>(&self, store: &'s Store) -> &'s [TemplatePart] {
        &store[self.parts]
    }

    #[inline]
    pub fn parts_mut<'s>(&self, store: &'s mut Store) -> &'s mut [TemplatePart] {
        &mut store[self.parts]
    }
}

#[derive(Clone, Copy)]
pub enum TemplateContents {
    Cooked(EString),
    Raw(Str),
}
impl TemplateContents {
    pub(crate) fn is_utf8(&self) -> bool {
        matches!(self, TemplateContents::Cooked(c) if c.is_utf8())
    }

    #[inline]
    #[track_caller]
    pub fn cooked(&self) -> &EString {
        match self {
            TemplateContents::Cooked(x) => x,
            TemplateContents::Raw(_) => unreachable!("TemplateContents::cooked on non-Cooked variant"),
        }
    }
    #[inline]
    #[track_caller]
    pub fn cooked_mut(&mut self) -> &mut EString {
        match self {
            TemplateContents::Cooked(x) => x,
            TemplateContents::Raw(_) => unreachable!("TemplateContents::cooked_mut on non-Cooked variant"),
        }
    }

    /// Field-wise copy.
    #[inline]
    pub(crate) fn shallow_clone(&self) -> TemplateContents {
        *self
    }
}

impl Template {
    /// "`a${'b'}c`" => "`abc`"
    pub fn fold(store: &mut Store, this: StoreRef<Template>, loc: crate::Loc) -> Expr {
        let template = store[this];
        if template.tag.is_some()
            || (matches!(template.head, TemplateContents::Cooked(_)) && !template.head.cooked().is_utf8())
        {
            // we only fold utf-8/ascii for now
            return Expr {
                data: crate::expr::Data::ETemplate(this),
                loc,
            };
        }

        debug_assert!(matches!(template.head, TemplateContents::Cooked(_)));

        if template.parts.is_empty() {
            let cooked = core::mem::take(store[this].head.cooked_mut());
            return Expr::init(store, cooked, loc);
        }

        let mut parts = Vec::<TemplatePart>::with_capacity(template.parts.len());
        let head_cooked = core::mem::take(store[this].head.cooked_mut());
        let head = Expr::init(store, head_cooked, loc);
        let head_ref = head.data.e_string().expect("infallible: variant checked");
        for idx in 0..template.parts.len() {
            let mut part = store[template.parts][idx];
            debug_assert!(matches!(part.tail, TemplateContents::Cooked(_)));

            part.value = part.value.unwrap_inlined(store);

            match part.value.data {
                crate::expr::Data::ENumber(n) => {
                    if let Some(s) = n.to_string(store) {
                        part.value = Expr::init(store, EString::init(s), part.value.loc);
                    }
                }
                crate::expr::Data::ENull(_) => {
                    let s = store.alloc_str(b"null");
                    part.value = Expr::init(store, EString::init(s), part.value.loc);
                }
                crate::expr::Data::EBoolean(b) => {
                    let s = store.alloc_str(if b.value { &b"true"[..] } else { &b"false"[..] });
                    part.value = Expr::init(store, EString::init(s), part.value.loc);
                }
                crate::expr::Data::EUndefined(_) => {
                    let s = store.alloc_str(b"undefined");
                    part.value = Expr::init(store, EString::init(s), part.value.loc);
                }
                crate::expr::Data::EBigInt(value) => {
                    let value = store[value].value;
                    if !BigInt::has_radix(store.str(value)) {
                        part.value = Expr::init(store, EString::init(value), part.value.loc);
                    }
                }
                _ => {}
            }

            let value_str = match part.value.data {
                crate::expr::Data::EString(s) if part.tail.cooked().is_utf8() && store[s].is_utf8() => s,
                _ => {
                    parts.push(part);
                    continue;
                }
            };

            // O alvo da corda: a cabeça (gravada no `Store`, copiada para fora e de volta) enquanto nenhuma parte
            // ficou, senão a cauda da última parte mantida.
            let mut target = match parts.last() {
                None => store[head_ref],
                Some(prev_part) => {
                    debug_assert!(matches!(prev_part.tail, TemplateContents::Cooked(_)));
                    if !prev_part.tail.cooked().is_utf8() {
                        parts.push(part);
                        continue;
                    }
                    *prev_part.tail.cooked()
                }
            };

            if store[value_str].len() > 0 {
                let copy = store[value_str].shallow_clone();
                let copy = Expr::init(store, copy, crate::Loc::EMPTY);
                target.push(store, copy.data.e_string().unwrap());
            }

            if part.tail.cooked().len() > 0 {
                let tail = core::mem::take(part.tail.cooked_mut());
                let tail = Expr::init(store, tail, part.tail_loc);
                target.push(store, tail.data.e_string().unwrap());
            }

            match parts.last_mut() {
                None => store[head_ref] = target,
                Some(prev_part) => *prev_part.tail.cooked_mut() = target,
            }
        }

        if parts.is_empty() {
            let mut head_str = store[head_ref];
            head_str.resolve_rope_if_needed(store);
            store[head_ref] = head_str;
            return head;
        }

        let parts = store.alloc_slice(parts);
        let head_copy = store[head_ref].shallow_clone();
        Expr::init(
            store,
            Template {
                tag: None,
                parts,
                head: TemplateContents::Cooked(head_copy),
            },
            loc,
        )
    }
}

#[derive(Clone, Copy)]
pub struct RegExp {
    pub value: Str,

    /// This exists for JavaScript bindings
    /// The RegExp constructor expects flags as a second argument.
    /// We want to avoid re-lexing the flags, so we store them here.
    /// This is the index of the first character in a flag, not the "/"
    /// /foo/gim
    ///      ^
    pub flags_offset: Option<u16>,
}
impl RegExp {
    pub fn pattern<'s>(&self, store: &'s Store) -> &'s [u8] {
        let value = store.str(self.value);
        // rewind until we reach the /foo/gim
        //                               ^
        // should only ever be a single character
        // but we're being cautious
        if let Some(i_) = self.flags_offset {
            let mut i = i_;
            while i > 0 && value[i as usize] != b'/' {
                i -= 1;
            }

            return bun_core::trim(&value[..i as usize], b"/");
        }

        bun_core::trim(value, b"/")
    }

    pub fn flags<'s>(&self, store: &'s Store) -> &'s [u8] {
        // rewind until we reach the /foo/gim
        //                               ^
        // should only ever be a single character
        // but we're being cautious
        if let Some(i) = self.flags_offset {
            return &store.str(self.value)[i as usize..];
        }

        b""
    }
}

#[derive(Clone, Copy)]
pub struct Await {
    pub value: ExprNodeIndex,
}

#[derive(Clone, Copy, Default)]
pub struct Yield {
    pub value: Option<ExprNodeIndex>,
    pub is_star: bool,
}

#[derive(Clone, Copy)]
pub struct If {
    pub test: ExprNodeIndex,
    pub yes: ExprNodeIndex,
    pub no: ExprNodeIndex,
}

pub enum UnwrappedRequireMarker {}
/// Index into the parser's `imports_to_convert_from_require`.
pub type UnwrappedRequireIndex = bun_core::GenericIndex<u32, UnwrappedRequireMarker>;
pub type UnwrappedRequireIndexOptional = bun_core::GenericIndexOptional<u32, UnwrappedRequireMarker>;

#[derive(Clone, Copy)]
pub struct RequireString {
    pub import_record_index: u32,

    /// Set when `unwrap_commonjs_to_esm` turned this `require()` into an import.
    pub unwrapped_id: UnwrappedRequireIndexOptional,
}
impl Default for RequireString {
    fn default() -> Self {
        Self {
            import_record_index: 0,
            unwrapped_id: UnwrappedRequireIndexOptional::NONE,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RequireResolveString {
    pub import_record_index: u32,
    // close_paren_loc: logger.Loc = logger.Loc.Empty,
}

#[derive(Clone, Copy)]
pub struct InlinedEnum {
    pub value: ExprNodeIndex,
    pub comment: Str,
}

#[derive(Clone, Copy)]
pub struct Import {
    pub expr: ExprNodeIndex,
    pub options: ExprNodeIndex,
    pub import_record_index: u32,
    /// When bundling a string-literal `import("...")`, this is the synthetic
    /// namespace symbol minted by `transpose_import`. Property accesses /
    /// destructuring of the awaited result are recorded under this ref so the
    /// linker can tree-shake unused exports of the importee. `Ref::NONE` when
    /// not bundling or the specifier is non-constant.
    pub namespace_ref: Ref,
    // TODO:
    // Comments inside "import()" expressions have special meaning for Webpack.
    // Preserving comments inside these expressions makes it possible to use
    // esbuild as a TypeScript-to-JavaScript frontend for Webpack to improve
    // performance. We intentionally do not interpret these comments in esbuild
    // because esbuild is not Webpack. But we do preserve them since doing so is
    // harmless, easy to maintain, and useful to people. See the Webpack docs for
    // more info: https://webpack.js.org/api/module-methods/#magic-comments.
    // leading_interior_comments: []G.Comment = &([_]G.Comment{}),
}
impl Import {
    pub fn is_import_record_null(&self) -> bool {
        self.import_record_index == u32::MAX
    }

    pub fn import_record_loader(&self, store: &Store) -> Option<crate::Loader> {
        let crate::ExprData::EObject(obj) = self.options.data else {
            return None;
        };
        let obj = &store[obj];
        let with = obj.get(store, b"with").or_else(|| obj.get(store, b"assert"))?;
        let crate::ExprData::EObject(with_obj) = with.data else {
            return None;
        };
        let with_obj = &store[with_obj];
        let str_ = &store[with_obj.get(store, b"type")?.data.e_string()?];

        if !str_.is_utf16 {
            if let Some(loader) = crate::Loader::from_string(store.str(str_.data)) {
                if loader == crate::Loader::Sqlite {
                    let Some(embed) = with_obj.get(store, b"embed") else {
                        return Some(loader);
                    };
                    let Some(embed_str) = embed.data.e_string() else {
                        return Some(loader);
                    };
                    if store[embed_str].eql_comptime(store, b"true") {
                        return Some(crate::Loader::SqliteEmbedded);
                    }
                }
                return Some(loader);
            }
        }

        None
    }
}

pub use G::Class;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Data;

    fn string(store: &mut Store, text: &[u8]) -> Expr {
        let data = store.alloc_str(text);
        Expr::init(store, EString::init(data), crate::Loc { start: 0 })
    }

    /// "`a${'b'}c${1}d`" dobra para "abc1d", como o `Template.fold` do upstream.
    #[test]
    fn template_fold_joins_string_parts() {
        let mut store = Store::default();
        let b = string(&mut store, b"b");
        let one = Expr::init(&mut store, Number::new(1.0), crate::Loc { start: 0 });
        let [a, c, d] = [&b"a"[..], b"c", b"d"].map(|text| EString::init(store.alloc_str(text)));
        let parts = store.alloc_slice([
            TemplatePart { value: b, tail_loc: crate::Loc::EMPTY, tail: TemplateContents::Cooked(c) },
            TemplatePart { value: one, tail_loc: crate::Loc::EMPTY, tail: TemplateContents::Cooked(d) },
        ]);
        let template = store.alloc(Template { tag: None, parts, head: TemplateContents::Cooked(a) });
        let folded = Template::fold(&mut store, template, crate::Loc { start: 0 });
        let Data::EString(folded) = folded.data else { panic!("a dobra não deu string") };
        assert!(store[folded].next.is_none());
        assert_eq!(store.str(store[folded].data), b"abc1d");
    }

    #[test]
    fn object_put_get_and_sort() {
        let mut store = Store::default();
        let object = store.alloc(Object::default());
        for key in [&b"version"[..], b"zeta", b"name", b"alpha"] {
            Object::put_string(&mut store, object, key, b"x");
        }
        let one = Expr::init(&mut store, Number::new(1.0), crate::Loc::EMPTY);
        Object::put(&mut store, object, b"zeta", one);
        assert!(store[object].has_property(&store, b"alpha"));
        assert!(matches!(store[object].get(&store, b"zeta").unwrap().data, Data::ENumber(_)));

        let keys = |store: &Store| -> Vec<Vec<u8>> {
            store[store[object].properties].iter().map(|p| property_key_bytes(store, p).to_vec()).collect()
        };
        let copy = store[object];
        copy.package_json_sort(&mut store);
        assert_eq!(keys(&store), [&b"name"[..], b"version", b"alpha", b"zeta"]);
        copy.alphabetize_properties(&mut store);
        assert_eq!(keys(&store), [&b"alpha"[..], b"name", b"version", b"zeta"]);
    }

    #[test]
    fn re_encode_utf8_goes_to_utf16_when_not_ascii() {
        let mut store = Store::default();
        let ascii = store.alloc_str(b"abc");
        assert!(EString::init_re_encode_utf8(&mut store, ascii).is_utf8());
        let text = store.alloc_str("ação".as_bytes());
        let s = EString::init_re_encode_utf8(&mut store, text);
        assert!(s.is_utf16);
        assert_eq!(s.len(), 4);
        assert!(s.eql_bytes(&store, "ação".as_bytes()));
        assert_eq!(s.javascript_length(&store), Some(4));
    }
}
