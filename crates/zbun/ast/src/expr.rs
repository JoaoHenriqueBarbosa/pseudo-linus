//! `src/ast/expr.rs` do upstream, o núcleo: `Expr`, `Tag`, `Data`, `IntoExprData`, `init` e os acessores. Os nós
//! vivem no `Store` recebido (seção "Arena tipada" do README), no lugar do `Data::Store` de thread. O resto pesado
//! (`deep_clone`, `write_to_hasher`, `known_primitive`, as dobras) entra com o parser.

use core::fmt;

use crate::Loc;
use crate::new_store::NodeStore as _;
use crate::store::Store;
use crate::{E, Ref, StoreRef};

// ───────────────────────────────────────────────────────────────────────────
// Expr
// ───────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Expr {
    pub loc: Loc,
    pub data: Data,
}

impl Default for Expr {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Expr {
    pub const EMPTY: Expr = Expr {
        data: Data::EMissing(E::Missing {}),
        loc: Loc::EMPTY,
    };
}

impl Expr {
    pub fn is_anonymous_named(&self, store: &Store) -> bool {
        match self.data {
            Data::EArrow(_) => true,
            Data::EFunction(func) => store[func].func.name.is_none(),
            Data::EClass(class) => store[class].class_name.is_none(),
            _ => false,
        }
    }

    pub fn can_be_inlined_from_property_access(&self, store: &Store) -> bool {
        match self.data {
            // if the array has a spread we must keep it
            // https://github.com/oven-sh/bun/issues/2594
            Data::ESpread(_) => false,
            Data::EMissing(_) => false,
            // `[[a?.b]][0]?.[0].c` must not become `a?.b.c`: inlining would
            // splice this chain onto the parent's `?.` continuation.
            Data::EDot(e) => store[e].optional_chain.is_none(),
            Data::EIndex(e) => store[e].optional_chain.is_none(),
            Data::ECall(e) => store[e].optional_chain.is_none(),
            _ => true,
        }
    }

    pub fn can_be_const_value(&self, store: &Store) -> bool {
        self.data.can_be_const_value(store)
    }

    pub fn can_be_moved(&self, store: &Store) -> bool {
        self.data.can_be_moved(store)
    }

    pub fn unwrap_inlined(self, store: &Store) -> Expr {
        if let Data::EInlinedEnum(inlined) = self.data {
            return store[inlined].value;
        }
        self
    }

    #[inline]
    pub fn init_identifier(ref_: Ref, loc: Loc) -> Expr {
        Expr {
            loc,
            data: Data::EIdentifier(E::Identifier::init(ref_)),
        }
    }

    pub fn to_empty(self) -> Expr {
        Expr {
            data: Data::EMissing(E::Missing {}),
            loc: self.loc,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Query {
    pub expr: Expr,
    pub loc: Loc,
    pub i: u32,
}

impl Expr {
    #[inline]
    pub fn is_array(&self) -> bool {
        matches!(self.data, Data::EArray(_))
    }
    #[inline]
    pub fn is_object(&self) -> bool {
        matches!(self.data, Data::EObject(_))
    }
    #[inline]
    pub fn is_string(&self) -> bool {
        matches!(self.data, Data::EString(_))
    }
}

// ───────────────────────────────────────────────────────────────────────────
// IntoExprData
// ───────────────────────────────────────────────────────────────────────────

/// No upstream há duas vias (`into_data_store`, no `Data::Store` de thread, e `into_data_alloc`, num bump); no porte
/// há um dono só, o `Store` recebido, e `Expr::allocate` deixa de existir.
pub trait IntoExprData: Sized {
    fn into_data_store(self, store: &mut Store) -> Data;
}

macro_rules! impl_into_expr_data_boxed {
    ($($ty:ident => $variant:ident),* $(,)?) => {
        $(
            impl IntoExprData for E::$ty {
                #[inline]
                fn into_data_store(self, store: &mut Store) -> Data {
                    Data::$variant(store.alloc(self))
                }
            }
        )*
    };
}

macro_rules! impl_into_expr_data_inline {
    ($($ty:ident => $variant:ident),* $(,)?) => {
        $(
            impl IntoExprData for E::$ty {
                #[inline]
                fn into_data_store(self, _store: &mut Store) -> Data { Data::$variant(self) }
            }
        )*
    };
}

impl_into_expr_data_boxed! {
    NameOfSymbol => ENameOfSymbol,
    Array => EArray,
    Class => EClass,
    Unary => EUnary,
    Binary => EBinary,
    New => ENew,
    Function => EFunction,
    Call => ECall,
    Dot => EDot,
    Index => EIndex,
    Arrow => EArrow,
    JSXElement => EJsxElement,
    BigInt => EBigInt,
    Object => EObject,
    Spread => ESpread,
    Template => ETemplate,
    RegExp => ERegExp,
    Await => EAwait,
    Yield => EYield,
    If => EIf,
    Import => EImport,
    InlinedEnum => EInlinedEnum,
    EString => EString,
}

impl_into_expr_data_inline! {
    This => EThis,
    Boolean => EBoolean,
    Super => ESuper,
    Null => ENull,
    Undefined => EUndefined,
    NewTarget => ENewTarget,
    ImportMeta => EImportMeta,
    PrivateIdentifier => EPrivateIdentifier,
    Missing => EMissing,
    Number => ENumber,
    RequireResolveString => ERequireResolveString,
    RequireString => ERequireString,
    Identifier => EIdentifier,
    ImportIdentifier => EImportIdentifier,
    CommonJSExportIdentifier => ECommonjsExportIdentifier,
}

// &E::EString: construct from a node to copy from.
impl IntoExprData for &E::EString {
    #[inline]
    fn into_data_store(self, store: &mut Store) -> Data {
        Data::EString(store.alloc(self.shallow_clone()))
    }
}

impl Expr {
    #[inline]
    pub fn init<T: IntoExprData>(store: &mut Store, st: T, loc: Loc) -> Expr {
        Expr {
            loc,
            data: st.into_data_store(store),
        }
    }

    // Trivial predicates with `is_missing` callers in G.rs/B.rs.
    #[inline]
    pub fn is_missing(&self) -> bool {
        matches!(self.data, Data::EMissing(_))
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        matches!(self.data, Data::EMissing(_))
    }
    #[inline]
    pub fn assign(store: &mut Store, a: Expr, b: Expr) -> Expr {
        Expr::init(
            store,
            E::Binary {
                op: crate::OpCode::BinAssign,
                left: a,
                right: b,
            },
            a.loc,
        )
    }

    #[inline]
    pub fn is_primitive_literal(&self) -> bool {
        Tag::is_primitive_literal(self.data.tag())
    }

    /// Uma chamada com esta expressão como alvo passa um valor de `this` (`a.b()` e `a[b]()`).
    #[inline]
    pub fn has_value_for_this_in_call(&self) -> bool {
        matches!(self.data, Data::EDot(_) | Data::EIndex(_))
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Tag
// ───────────────────────────────────────────────────────────────────────────

/// Todas as variantes do upstream, inclusive as de JSON que o `Data` ainda não tem: os discriminantes ficam iguais
/// (o hash do React Fast Refresh usa `tag as u8`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
#[repr(u8)]
pub enum Tag {
    EArray,
    EUnary,
    EBinary,
    EClass,
    ENew,
    EFunction,
    ECall,
    EDot,
    EIndex,
    EArrow,
    EJsxElement,
    EObject,
    EObjectJSON,
    EArrayJSON,
    ESpread,
    ETemplate,
    ERegExp,
    EAwait,
    EYield,
    EIf,
    EImport,
    EIdentifier,
    EImportIdentifier,
    EPrivateIdentifier,
    ECommonjsExportIdentifier,
    EBoolean,
    /// Like e_boolean, but produced by `feature()` from `bun:bundle`.
    /// This tag ensures feature() can only be used directly in conditional
    /// contexts (if statements, ternaries). Invalid usage is caught during
    /// the visit phase when this expression appears outside a branch condition.
    EBranchBoolean,
    ENumber,
    EBigInt,
    EString,
    ERequireString,
    ERequireResolveString,
    ERequireCallTarget,
    ERequireResolveCallTarget,
    EMissing,
    EThis,
    ESuper,
    ENull,
    EUndefined,
    ENewTarget,
    EImportMeta,
    EImportMetaMain,
    ERequireMain,
    ESpecial,
    EInlinedEnum,
    ENameOfSymbol,
}

impl Tag {
    // object, regex and array may have had side effects
    pub fn is_primitive_literal(self) -> bool {
        matches!(
            self,
            Tag::ENull
                | Tag::EUndefined
                | Tag::EString
                | Tag::EBoolean
                | Tag::EBranchBoolean
                | Tag::ENumber
                | Tag::EBigInt
        )
    }

    pub(crate) fn typeof_(tag: Tag) -> Option<&'static [u8]> {
        // This must only return `Some` when the operand is guaranteed to have
        // no side effects. Array/object/class literals are omitted because
        // their elements, properties, and static initializers can run code.
        Some(match tag {
            Tag::EArrayJSON | Tag::EObjectJSON | Tag::ENull | Tag::ERegExp => b"object",
            Tag::EUndefined => b"undefined",
            Tag::EBoolean | Tag::EBranchBoolean => b"boolean",
            Tag::ENumber => b"number",
            Tag::EBigInt => b"bigint",
            Tag::EString => b"string",
            Tag::EFunction | Tag::EArrow => b"function",
            _ => return None,
        })
    }

    /// Human-readable variant name for diagnostics (`"string"`, `"boolean"`, …).
    pub fn type_name(self) -> &'static str {
        match self {
            Tag::EString => "string",
            Tag::EArray | Tag::EArrayJSON => "array",
            Tag::EUnary => "unary",
            Tag::EBinary => "binary",
            Tag::EBoolean | Tag::EBranchBoolean => "boolean",
            Tag::ESuper => "super",
            Tag::ENull => "null",
            Tag::EUndefined => "undefined",
            Tag::ENew => "new",
            Tag::EFunction => "function",
            Tag::ENewTarget => "new target",
            Tag::EImportMeta => "import.meta",
            Tag::ECall => "call",
            Tag::EDot => "dot",
            Tag::EIndex => "index",
            Tag::EArrow => "arrow",
            Tag::EIdentifier => "identifier",
            Tag::EImportIdentifier => "import identifier",
            Tag::EPrivateIdentifier => "#privateIdentifier",
            Tag::EJsxElement => "<jsx>",
            Tag::EMissing => "<missing>",
            Tag::ENumber => "number",
            Tag::EBigInt => "BigInt",
            Tag::EObject | Tag::EObjectJSON => "object",
            Tag::ESpread => "...",
            Tag::ETemplate => "template",
            Tag::ERegExp => "regexp",
            Tag::EAwait => "await",
            Tag::EYield => "yield",
            Tag::EIf => "if",
            Tag::ERequireResolveString => "require_or_require_resolve",
            Tag::EImport => "import",
            Tag::EThis => "this",
            Tag::EClass => "class",
            Tag::ERequireString => "require",
            other => <&'static str>::from(other),
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.type_name())
    }
}

// ───────────────────────────────────────────────────────────────────────────
// PrimitiveType
// ───────────────────────────────────────────────────────────────────────────

// EnumSetType derives Clone/Copy/PartialEq/Eq itself.
#[derive(Debug, enumset::EnumSetType)]
pub enum PrimitiveType {
    Unknown,
    Mixed,
    Null,
    Undefined,
    Boolean,
    Number,
    String,
    Bigint,
}

impl PrimitiveType {
    pub(crate) fn merge(left_known: PrimitiveType, right_known: PrimitiveType) -> PrimitiveType {
        if right_known == PrimitiveType::Unknown || left_known == PrimitiveType::Unknown {
            return PrimitiveType::Unknown;
        }
        if left_known == right_known {
            left_known
        } else {
            PrimitiveType::Mixed
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Data
// ───────────────────────────────────────────────────────────────────────────

enum_tag! {
    /// Tagged union of expression payloads. Pointer variants are `StoreRef<E::*>` (índices no `Store`); inline
    /// variants are stored by value. `Data` is `Copy`. `EObjectJSON` e `EArrayJSON` ficam fora até o loader de JSON.
    #[derive(Clone, Copy)]
    pub enum Data: Tag {
        EArray(StoreRef<E::Array>),
        EUnary(StoreRef<E::Unary>),
        EBinary(StoreRef<E::Binary>),
        EClass(StoreRef<E::Class>),

        ENew(StoreRef<E::New>),
        EFunction(StoreRef<E::Function>),
        ECall(StoreRef<E::Call>),
        EDot(StoreRef<E::Dot>),
        EIndex(StoreRef<E::Index>),
        EArrow(StoreRef<E::Arrow>),

        EJsxElement(StoreRef<E::JSXElement>),
        EObject(StoreRef<E::Object>),
        ESpread(StoreRef<E::Spread>),
        ETemplate(StoreRef<E::Template>),
        ERegExp(StoreRef<E::RegExp>),
        EAwait(StoreRef<E::Await>),
        EYield(StoreRef<E::Yield>),
        EIf(StoreRef<E::If>),
        EImport(StoreRef<E::Import>),

        EIdentifier(E::Identifier),
        EImportIdentifier(E::ImportIdentifier),
        EPrivateIdentifier(E::PrivateIdentifier),
        ECommonjsExportIdentifier(E::CommonJSExportIdentifier),

        EBoolean(E::Boolean),
        EBranchBoolean(E::Boolean),
        ENumber(E::Number),
        EBigInt(StoreRef<E::BigInt>),
        EString(StoreRef<E::EString>),

        ERequireString(E::RequireString),
        ERequireResolveString(E::RequireResolveString),
        ERequireCallTarget,
        ERequireResolveCallTarget,

        EMissing(E::Missing),
        EThis(E::This),
        ESuper(E::Super),
        ENull(E::Null),
        EUndefined(E::Undefined),
        ENewTarget(E::NewTarget),
        EImportMeta(E::ImportMeta),

        EImportMetaMain(E::ImportMetaMain),
        ERequireMain,

        /// Covers some exotic AST node types under one namespace, since the
        /// places this is found it all follows similar handling.
        ESpecial(E::Special),

        EInlinedEnum(StoreRef<E::InlinedEnum>),

        ENameOfSymbol(StoreRef<E::NameOfSymbol>),
    }
}

// ── Layout guards ─────────────────────────────────────────────────────────
// O maior payload (`E::Number`, os identificadores) tem 8 bytes com alinhamento 4, então `Data` continua com 12
// bytes e `Expr` com 16, como no upstream, mesmo com o `StoreRef` de 4.
const _: () = assert!(core::mem::size_of::<Data>() == 12); // Do not increase the size of Expr
const _: () = assert!(core::mem::align_of::<Data>() == 4);
const _: () = assert!(core::mem::size_of::<Expr>() == 16);
const _: () = assert!(core::mem::size_of::<Option<Data>>() == core::mem::size_of::<Data>());
const _: () = assert!(core::mem::size_of::<Option<Expr>>() == core::mem::size_of::<Expr>());
const _: () = assert!(core::mem::size_of::<E::Identifier>() == 8);
const _: () = assert!(core::mem::align_of::<E::Identifier>() == 4);
const _: () = assert!(core::mem::size_of::<E::Special>() <= 8);
const _: () = assert!(core::mem::size_of::<E::RequireString>() <= 8);
const _: () = assert!(core::mem::size_of::<E::NewTarget>() <= 8);
const _: () = assert!(core::mem::size_of::<crate::G::FnBody>() == 12);

data_accessors! {
    Data;
    e_string / e_string_mut => EString: E::EString,
    e_object / e_object_mut => EObject: E::Object,
    e_array / e_array_mut => EArray: E::Array,
    e_unary => EUnary: E::Unary,
    e_binary / e_binary_mut => EBinary: E::Binary,
    e_class => EClass: E::Class,
    e_new => ENew: E::New,
    e_function => EFunction: E::Function,
    e_call / e_call_mut => ECall: E::Call,
    e_dot / e_dot_mut => EDot: E::Dot,
    e_index => EIndex: E::Index,
    e_arrow => EArrow: E::Arrow,
    e_jsx_element => EJsxElement: E::JSXElement,
    e_spread => ESpread: E::Spread,
    e_template => ETemplate: E::Template,
    e_await / e_await_mut => EAwait: E::Await,
    e_if => EIf: E::If,
    e_import => EImport: E::Import,
}

impl Data {
    #[inline]
    pub fn is_e_object(&self) -> bool {
        matches!(self, Data::EObject(_))
    }
    #[inline]
    pub fn e_identifier(&self) -> Option<E::Identifier> {
        if let Data::EIdentifier(v) = *self { Some(v) } else { None }
    }
    #[inline]
    pub fn e_import_identifier(&self) -> Option<E::ImportIdentifier> {
        if let Data::EImportIdentifier(v) = *self { Some(v) } else { None }
    }
    #[inline]
    pub fn e_number(&self) -> Option<E::Number> {
        if let Data::ENumber(v) = *self { Some(v) } else { None }
    }
    #[inline]
    pub fn e_special(&self) -> Option<E::Special> {
        if let Data::ESpecial(v) = *self { Some(v) } else { None }
    }

    /// Human-readable variant name for diagnostics (`"string"`, `"object"`, …).
    #[inline]
    pub fn tag_name(&self) -> &'static str {
        self.tag().type_name()
    }
}

impl Data {
    /// "const values" here refers to expressions that can participate in constant
    /// inlining, as they have no side effects on instantiation, and there would be
    /// no observable difference if duplicated. This is a subset of canBeMoved()
    pub(crate) fn can_be_const_value(&self, store: &Store) -> bool {
        match *self {
            Data::ENumber(_)
            | Data::EBoolean(_)
            | Data::EBranchBoolean(_)
            | Data::ENull(_)
            | Data::EUndefined(_)
            | Data::EInlinedEnum(_) => true,
            Data::EString(str) => store[str].next.is_none(),
            Data::EArray(array) => store[array].was_originally_macro,
            Data::EObject(object) => store[object].was_originally_macro,
            _ => false,
        }
    }

    /// Expressions that can be moved are those that do not have side
    /// effects on their own. This is used to determine what can be moved
    /// outside of a module wrapper (__esm/__commonJS).
    pub(crate) fn can_be_moved(&self, store: &Store) -> bool {
        match *self {
            // TODO: identifiers can be removed if unused, however code that
            // moves expressions around sometimes does so incorrectly when
            // doing destructures. test case: https://github.com/oven-sh/bun/issues/14027
            // Data::EIdentifier(id) => id.can_be_removed_if_unused,
            Data::EClass(class) => store[class].can_be_moved(store),

            Data::EArrow(_)
            | Data::EFunction(_)
            | Data::ENumber(_)
            | Data::EBoolean(_)
            | Data::EBranchBoolean(_)
            | Data::ENull(_)
            | Data::EUndefined(_)
            // | Data::ERegExp(_)
            | Data::EBigInt(_)
            | Data::EString(_)
            | Data::EInlinedEnum(_)
            | Data::EImportMeta(_) => true,

            Data::ETemplate(template) => store[template].tag.is_none() && store[template].parts.is_empty(),

            Data::EArray(array) => store[array].was_originally_macro,
            Data::EObject(object) => store[object].was_originally_macro,

            // TODO: experiment with allowing some e_binary, e_unary, e_if as movable
            _ => false,
        }
    }

    pub fn is_safe_to_string(&self, store: &Store) -> bool {
        match *self {
            // rope strings can throw when toString is called.
            Data::EString(str) => store[str].next.is_none(),

            Data::ENumber(_)
            | Data::EBoolean(_)
            | Data::EBranchBoolean(_)
            | Data::EUndefined(_)
            | Data::ENull(_) => true,
            // BigInt is deliberately excluded as a large enough BigInt could throw an out of memory error.
            //
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EFlags {
    None,
    TsDecorator,
    /// Between the `?` and `:` of a conditional, where an arrow return type is ambiguous.
    AfterQuestionAndBeforeColon,
}

// ───────────────────────────────────────────────────────────────────────────
// Equality
// ───────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Equality {
    /// Nothing is known about the equality of the two operands.
    Unknown,
    Equal,
    NotEqual,
    /// `require.main === module` (or `!==`/`==`/`!=`, in either order); the
    /// caller rewrites this to an import_meta_main node.
    RequireMainAndModule,
}

impl From<bool> for Equality {
    #[inline]
    fn from(equal: bool) -> Self {
        if equal { Equality::Equal } else { Equality::NotEqual }
    }
}

// `adt_const_params` (enum const-generic) is nightly-only. Lower to a sealed
// ZST trait; callers use `Data::eql::<P, LooseEql>(...)` / `<P, StrictEql>`.
pub trait EqlKindT: Copy {
    const STRICT: bool;
}
#[derive(Clone, Copy)]
pub struct LooseEql;
impl EqlKindT for LooseEql {
    const STRICT: bool = false;
}
#[derive(Clone, Copy)]
pub struct StrictEql;
impl EqlKindT for StrictEql {
    const STRICT: bool = true;
}

/// Minimal parser surface needed by `Data::eql`. No upstream é a arena e o `module_ref`; aqui o `Store` do
/// arquivo, onde as cordas são achatadas.
pub trait EqlParser {
    fn store_mut(&mut self) -> &mut Store;
    fn module_ref(&self) -> Ref;
}
// `impl EqlParser for P<...>` lives in `bun_js_parser` (next to `P`).

/// Achata a corda do `EString` em `r` no próprio nó, como o `resolve_rope_if_needed` sobre o `StoreRef` do
/// upstream.
fn resolve_rope_in_store(store: &mut Store, r: StoreRef<E::EString>) {
    let mut s = store[r];
    s.resolve_rope_if_needed(store);
    store[r] = s;
}

impl Data {
    pub fn eql<P: EqlParser, K: EqlKindT>(left: &Data, right: &Data, p: &mut P) -> Equality {
        // https://dorey.github.io/JavaScript-Equality-Table/
        match *left {
            Data::EInlinedEnum(inlined) => {
                let value = p.store_mut()[inlined].value.data;
                return Data::eql::<P, K>(&value, right, p);
            }

            Data::ENull(_) | Data::EUndefined(_) => {
                let right_tag = right.tag();
                if !matches!(right_tag, Tag::ENull | Tag::EUndefined) && !right_tag.is_primitive_literal() {
                    return Equality::Unknown;
                }
                return Equality::from(if K::STRICT {
                    right_tag == left.tag()
                } else {
                    matches!(right_tag, Tag::ENull | Tag::EUndefined)
                });
            }
            Data::EBoolean(l) | Data::EBranchBoolean(l) => match *right {
                Data::EBoolean(r) | Data::EBranchBoolean(r) => {
                    return Equality::from(l.value == r.value);
                }
                Data::ENumber(num) => {
                    if K::STRICT {
                        // "true === 1" is false
                        // "false === 0" is false
                        return Equality::NotEqual;
                    }
                    return Equality::from(if l.value { num.value() == 1.0 } else { num.value() == 0.0 });
                }
                Data::ENull(_) | Data::EUndefined(_) => {
                    return Equality::NotEqual;
                }
                _ => {}
            },
            Data::ENumber(l) => match *right {
                Data::ENumber(r) => {
                    return Equality::from(l.value() == r.value());
                }
                Data::EInlinedEnum(r) => {
                    if let Data::ENumber(rn) = p.store_mut()[r].value.data {
                        return Equality::from(l.value() == rn.value());
                    }
                }
                Data::EBoolean(r) | Data::EBranchBoolean(r) => {
                    if !K::STRICT {
                        // "1 == true" is true
                        // "0 == false" is true
                        return Equality::from(if r.value { l.value() == 1.0 } else { l.value() == 0.0 });
                    }
                    // "1 === true" is false
                    // "0 === false" is false
                    return Equality::NotEqual;
                }
                Data::ENull(_) | Data::EUndefined(_) => {
                    // "(not null or undefined) == undefined" is false
                    return Equality::NotEqual;
                }
                _ => {}
            },
            Data::EBigInt(l) => {
                return match *right {
                    Data::EBigInt(r) => {
                        let store = p.store_mut();
                        match E::BigInt::check_equality(store.str(store[l].value), store.str(store[r].value)) {
                            Some(equal) => Equality::from(equal),
                            None => Equality::Unknown,
                        }
                    }
                    Data::ENull(_) | Data::EUndefined(_) => Equality::NotEqual,
                    _ => Equality::Unknown,
                };
            }
            Data::EString(l) => match *right {
                Data::EString(r) => {
                    let store = p.store_mut();
                    resolve_rope_in_store(store, r);
                    resolve_rope_in_store(store, l);
                    return Equality::from(store[r].eql_string(store, &store[l]));
                }
                Data::EInlinedEnum(inlined) => {
                    let store = p.store_mut();
                    if let Data::EString(r) = store[inlined].value.data {
                        resolve_rope_in_store(store, r);
                        resolve_rope_in_store(store, l);
                        return Equality::from(store[r].eql_string(store, &store[l]));
                    }
                }
                Data::ENull(_) | Data::EUndefined(_) => {
                    return Equality::NotEqual;
                }
                Data::ENumber(r) => {
                    if !K::STRICT {
                        let store = p.store_mut();
                        resolve_rope_in_store(store, l);
                        let ls = &store[l];
                        if r.value() == 0.0 && (ls.is_blank() || ls.eql_comptime(store, b"0")) {
                            return Equality::Equal;
                        }
                        if r.value() == 1.0 && ls.eql_comptime(store, b"1") {
                            return Equality::Equal;
                        }
                        // the string could still equal 0 or 1 but it could be hex, binary, octal, ...
                        return Equality::Unknown;
                    } else {
                        return Equality::NotEqual;
                    }
                }
                _ => {}
            },

            _ => {
                // Do not need to check left because e_require_main is
                // always re-ordered to the right side.
                if matches!(right, Data::ERequireMain) {
                    if let Some(id) = left.e_identifier() {
                        if id.ref_.eql(p.module_ref()) {
                            return Equality::RequireMainAndModule;
                        }
                    }
                }
            }
        }

        Equality::Unknown
    }

    pub fn known_primitive(&self, store: &Store) -> PrimitiveType {
        self.known_primitive_with_check(store, bun_core::StackCheck::init())
    }

    fn known_primitive_with_check(&self, store: &Store, stack_check: bun_core::StackCheck) -> PrimitiveType {
        use crate::OpCode;
        if !stack_check.is_safe_to_recurse() {
            return PrimitiveType::Unknown;
        }
        match *self {
            Data::EBigInt(_) => PrimitiveType::Bigint,
            Data::EBoolean(_) | Data::EBranchBoolean(_) => PrimitiveType::Boolean,
            Data::ENull(_) => PrimitiveType::Null,
            Data::ENumber(_) => PrimitiveType::Number,
            Data::EString(_) => PrimitiveType::String,
            Data::EUndefined(_) => PrimitiveType::Undefined,
            Data::ETemplate(t) => {
                if store[t].tag.is_none() {
                    PrimitiveType::String
                } else {
                    PrimitiveType::Unknown
                }
            }
            Data::EIf(e_if) => {
                let e_if = &store[e_if];
                e_if.yes.data.merge_known_primitive_with_check(&e_if.no.data, store, stack_check)
            }
            Data::EBinary(binary) => 'brk: {
                let binary = &store[binary];
                match binary.op {
                    OpCode::BinStrictEq
                    | OpCode::BinStrictNe
                    | OpCode::BinLooseEq
                    | OpCode::BinLooseNe
                    | OpCode::BinLt
                    | OpCode::BinGt
                    | OpCode::BinLe
                    | OpCode::BinGe
                    | OpCode::BinInstanceof
                    | OpCode::BinIn => break 'brk PrimitiveType::Boolean,
                    OpCode::BinLogicalOr | OpCode::BinLogicalAnd => {
                        break 'brk binary.left.data.merge_known_primitive_with_check(
                            &binary.right.data,
                            store,
                            stack_check,
                        );
                    }

                    OpCode::BinNullishCoalescing => {
                        let left = binary.left.data.known_primitive_with_check(store, stack_check);
                        let right = binary.right.data.known_primitive_with_check(store, stack_check);
                        if left == PrimitiveType::Null || left == PrimitiveType::Undefined {
                            break 'brk right;
                        }

                        if left != PrimitiveType::Unknown {
                            if left != PrimitiveType::Mixed {
                                break 'brk left; // Definitely not null or undefined
                            }

                            if right != PrimitiveType::Unknown {
                                break 'brk PrimitiveType::Mixed; // Definitely some kind of primitive
                            }
                        }
                    }

                    OpCode::BinAdd => {
                        let left = binary.left.data.known_primitive_with_check(store, stack_check);
                        let right = binary.right.data.known_primitive_with_check(store, stack_check);

                        if left == PrimitiveType::String || right == PrimitiveType::String {
                            break 'brk PrimitiveType::String;
                        }

                        if left == PrimitiveType::Bigint || right == PrimitiveType::Bigint {
                            break 'brk PrimitiveType::Bigint;
                        }

                        let left_ok =
                            !matches!(left, PrimitiveType::Unknown | PrimitiveType::Mixed | PrimitiveType::Bigint);
                        let right_ok =
                            !matches!(right, PrimitiveType::Unknown | PrimitiveType::Mixed | PrimitiveType::Bigint);
                        if left_ok && right_ok {
                            break 'brk PrimitiveType::Number;
                        }

                        break 'brk PrimitiveType::Mixed; // Can be number or bigint or string (or an exception)
                    }

                    OpCode::BinSub
                    | OpCode::BinSubAssign
                    | OpCode::BinMul
                    | OpCode::BinMulAssign
                    | OpCode::BinDiv
                    | OpCode::BinDivAssign
                    | OpCode::BinRem
                    | OpCode::BinRemAssign
                    | OpCode::BinPow
                    | OpCode::BinPowAssign
                    | OpCode::BinBitwiseAnd
                    | OpCode::BinBitwiseAndAssign
                    | OpCode::BinBitwiseOr
                    | OpCode::BinBitwiseOrAssign
                    | OpCode::BinBitwiseXor
                    | OpCode::BinBitwiseXorAssign
                    | OpCode::BinShl
                    | OpCode::BinShlAssign
                    | OpCode::BinShr
                    | OpCode::BinShrAssign
                    | OpCode::BinUShr
                    | OpCode::BinUShrAssign => break 'brk PrimitiveType::Mixed, // Can be number or bigint (or an exception)

                    OpCode::BinAssign | OpCode::BinComma => {
                        break 'brk binary.right.data.known_primitive_with_check(store, stack_check);
                    }

                    _ => {}
                }

                PrimitiveType::Unknown
            }

            Data::EUnary(unary) => {
                let unary = &store[unary];
                match unary.op {
                    OpCode::UnVoid => PrimitiveType::Undefined,
                    OpCode::UnTypeof => PrimitiveType::String,
                    OpCode::UnNot | OpCode::UnDelete => PrimitiveType::Boolean,
                    OpCode::UnPos => PrimitiveType::Number, // Cannot be bigint because that throws an exception
                    OpCode::UnNeg | OpCode::UnCpl => {
                        match unary.value.data.known_primitive_with_check(store, stack_check) {
                            PrimitiveType::Bigint => PrimitiveType::Bigint,
                            PrimitiveType::Unknown | PrimitiveType::Mixed => PrimitiveType::Mixed,
                            _ => PrimitiveType::Number, // Can be number or bigint
                        }
                    }
                    OpCode::UnPreDec | OpCode::UnPreInc | OpCode::UnPostDec | OpCode::UnPostInc => {
                        PrimitiveType::Mixed // Can be number or bigint
                    }

                    _ => PrimitiveType::Unknown,
                }
            }

            Data::EInlinedEnum(inlined) => store[inlined].value.data.known_primitive_with_check(store, stack_check),

            _ => PrimitiveType::Unknown,
        }
    }

    fn merge_known_primitive_with_check(
        &self,
        rhs: &Data,
        store: &Store,
        stack_check: bun_core::StackCheck,
    ) -> PrimitiveType {
        PrimitiveType::merge(
            self.known_primitive_with_check(store, stack_check),
            rhs.known_primitive_with_check(store, stack_check),
        )
    }
}

impl Expr {
    pub fn known_primitive(&self, store: &Store) -> PrimitiveType {
        self.data.known_primitive(store)
    }

    /// O texto de uma string literal, achatado e em UTF-8 no `Store`.
    pub fn as_string_literal(&self, store: &mut Store) -> Option<crate::StoreStr> {
        let Data::EString(s) = self.data else {
            return None;
        };
        let string = store[s];
        Some(string.string(store))
    }

    // Wraps the provided expression in the "!" prefix operator. The expression
    // will potentially be simplified to avoid generating unnecessary extra "!"
    // operators. For example, calling this with "!!x" will return "!x" instead
    // of returning "!!!x".
    pub fn not(&self, store: &mut Store) -> Expr {
        self.maybe_simplify_not(store).unwrap_or_else(|| {
            Expr::init(
                store,
                E::Unary {
                    op: crate::OpCode::UnNot,
                    value: *self,
                    flags: E::UnaryFlags::empty(),
                },
                self.loc,
            )
        })
    }

    /// The given "expr" argument should be the operand of a "!" prefix operator
    /// (i.e. the "x" in "!x"). This returns a simplified expression for the
    /// whole operator (i.e. the "!x") if it can be simplified, or false if not.
    /// It's separate from "Not()" above to avoid allocation on failure in case
    /// that is undesired.
    pub fn maybe_simplify_not(&self, store: &mut Store) -> Option<Expr> {
        let expr = *self;
        let boolean = |value: bool| Expr { loc: expr.loc, data: Data::EBoolean(E::Boolean { value }) };
        match expr.data {
            Data::ENull(_) | Data::EUndefined(_) => return Some(boolean(true)),
            Data::EBoolean(b) | Data::EBranchBoolean(b) => return Some(boolean(!b.value)),
            Data::ENumber(n) => return Some(boolean(n.value() == 0.0 || n.value().is_nan())),
            Data::EBigInt(b) => {
                if let Some(equal) = E::BigInt::check_equality(store.str(store[b].value), b"0") {
                    return Some(boolean(equal));
                }
            }
            Data::EFunction(_) | Data::EArrow(_) | Data::ERegExp(_) => return Some(boolean(false)),
            // "!!!a" => "!a"
            Data::EUnary(un) => {
                let un = store[un];
                if un.op == crate::OpCode::UnNot && un.value.known_primitive(store) == PrimitiveType::Boolean {
                    return Some(un.value);
                }
            }
            Data::EBinary(ex) => {
                // TODO: evaluate whether or not it is safe to do this mutation since it's modifying in-place.
                // Make sure that these transformations are all safe for special values.
                // For example, "!(a < b)" is not the same as "a >= b" if a and/or b are
                // NaN (or undefined, or null, or possibly other problem cases too).
                let swapped = match store[ex].op {
                    // "!(a == b)" => "a != b"
                    crate::OpCode::BinLooseEq => Some(crate::OpCode::BinLooseNe),
                    // "!(a != b)" => "a == b"
                    crate::OpCode::BinLooseNe => Some(crate::OpCode::BinLooseEq),
                    // "!(a === b)" => "a !== b"
                    crate::OpCode::BinStrictEq => Some(crate::OpCode::BinStrictNe),
                    // "!(a !== b)" => "a === b"
                    crate::OpCode::BinStrictNe => Some(crate::OpCode::BinStrictEq),
                    _ => None,
                };
                if let Some(op) = swapped {
                    store[ex].op = op;
                    return Some(expr);
                }
                if store[ex].op == crate::OpCode::BinComma {
                    // "!(a, b)" => "a, !b"
                    let right = store[ex].right;
                    store[ex].right = right.not(store);
                    return Some(expr);
                }
            }
            Data::EInlinedEnum(inlined) => {
                let value = store[inlined].value;
                return value.maybe_simplify_not(store);
            }
            _ => {}
        }

        None
    }

    /// O texto que o valor teria como string, quando ele é conhecido sem efeito colateral. O upstream aponta o
    /// `E::String` novo para uma fatia `'static` ou da arena; aqui o texto é gravado no `Store`.
    pub(crate) fn to_string_expr_without_side_effects(&self, store: &mut Store) -> Option<Expr> {
        let expr = *self;
        let unwrapped = expr.unwrap_inlined(store);
        let fixed: &'static [u8] = match unwrapped.data {
            Data::ENull(_) => b"null",
            Data::EString(_) => return Some(expr),
            Data::EUndefined(_) => b"undefined",
            Data::EBoolean(data) | Data::EBranchBoolean(data) => {
                if data.value {
                    b"true"
                } else {
                    b"false"
                }
            }
            Data::EBigInt(bigint) => {
                let value = store[bigint].value;
                if E::BigInt::has_radix(store.str(value)) {
                    return None;
                }
                return Some(Expr::init(store, E::EString::init(value), expr.loc));
            }
            Data::ENumber(num) => {
                let text = num.to_string(store)?;
                return Some(Expr::init(store, E::EString::init(text), expr.loc));
            }
            Data::ERegExp(regexp) => {
                let value = store[regexp].value;
                return Some(Expr::init(store, E::EString::init(value), expr.loc));
            }
            Data::EDot(dot) => {
                // This is dumb but some JavaScript obfuscators use this to generate string literals
                let dot = &store[dot];
                if store.str(dot.name) != b"constructor" {
                    return None;
                }
                match dot.target.data {
                    Data::EString(_) => b"function String() { [native code] }",
                    Data::ERegExp(_) => b"function RegExp() { [native code] }",
                    _ => return None,
                }
            }
            _ => return None,
        };
        let text = store.alloc_str(fixed);
        Some(Expr::init(store, E::EString::init(text), expr.loc))
    }
}

// ───────────────────────────────────────────────────────────────────────────
// join_*, extract_*, to_finite_number: o `Store` vem como argumento, no lugar do `Data::Store` de thread
// ───────────────────────────────────────────────────────────────────────────

impl Expr {
    /// Joins two expressions with a left-associative operator, flattening comma and same-operator chains. "a op (b op
    /// c)" becomes "(a op b) op c". Only for associative operators ("-" is not associative for floating-point).
    pub fn join_with_left_associative_op(store: &mut Store, op: crate::Op::Code, a: Expr, b: Expr) -> Expr {
        Self::join_with_left_associative_op_with_check(store, op, a, b, bun_core::StackCheck::init())
    }

    fn join_with_left_associative_op_with_check(
        store: &mut Store,
        op: crate::Op::Code,
        a: Expr,
        b: Expr,
        stack_check: bun_core::StackCheck,
    ) -> Expr {
        if stack_check.is_safe_to_recurse() {
            // "(a, b) op c" => "a, b op c"
            if let Data::EBinary(comma) = a.data {
                if store[comma].op == crate::OpCode::BinComma {
                    let right = store[comma].right;
                    let joined = Self::join_with_left_associative_op_with_check(store, op, right, b, stack_check);
                    store[comma].right = joined;
                    return a;
                }
            }

            // "a op (b op c)" => "(a op b) op c"
            // "a op (b op (c op d))" => "((a op b) op c) op d"
            if let Data::EBinary(binary) = b.data {
                if store[binary].op == op {
                    let (left, right) = (store[binary].left, store[binary].right);
                    let inner = Self::join_with_left_associative_op_with_check(store, op, a, left, stack_check);
                    return Self::join_with_left_associative_op_with_check(store, op, inner, right, stack_check);
                }
            }
        }

        // "a op b" => "a op b"
        // "(a op b) op c" => "(a op b) op c"
        Expr::init(store, E::Binary { op, left: a, right: b }, a.loc)
    }

    pub fn join_with_comma(store: &mut Store, a: Expr, b: Expr) -> Expr {
        if a.is_missing() {
            return b;
        }
        if b.is_missing() {
            return a;
        }
        Expr::init(store, E::Binary { op: crate::OpCode::BinComma, left: a, right: b }, a.loc)
    }

    pub fn join_all_with_comma(store: &mut Store, all: &[Expr]) -> Expr {
        debug_assert!(!all.is_empty());
        match all.len() {
            1 => all[0],
            2 => Expr::join_with_comma(store, all[0], all[1]),
            _ => {
                let mut expr = all[0];
                for it in &all[1..] {
                    expr = Expr::join_with_comma(store, expr, *it);
                }
                expr
            }
        }
    }

    pub fn extract_numeric_values_in_safe_range(store: &Store, left: &Data, right: &Data) -> Option<[f64; 2]> {
        let l_value = left.extract_numeric_value(store)?;
        let r_value = right.extract_numeric_value(store)?;

        // Check for NaN and return null if either value is NaN
        if l_value.is_nan() || r_value.is_nan() {
            return None;
        }

        if l_value.is_infinite() || r_value.is_infinite() {
            return Some([l_value, r_value]);
        }

        if l_value > crate::math::MAX_SAFE_INTEGER || r_value > crate::math::MAX_SAFE_INTEGER {
            return None;
        }
        if l_value < crate::math::MIN_SAFE_INTEGER || r_value < crate::math::MIN_SAFE_INTEGER {
            return None;
        }

        Some([l_value, r_value])
    }

    pub fn extract_numeric_values(store: &Store, left: &Data, right: &Data) -> Option<[f64; 2]> {
        Some([left.extract_numeric_value(store)?, right.extract_numeric_value(store)?])
    }

    pub fn extract_string_values(store: &mut Store, left: &Data, right: &Data) -> Option<[StoreRef<E::EString>; 2]> {
        let l_string = Data::extract_string_value(store, *left)?;
        let r_string = Data::extract_string_value(store, *right)?;
        // `extract_string_value` devolve o índice do nó; resolver o rope grava no `Store`.
        let mut l = store[l_string].clone();
        let mut r = store[r_string].clone();
        l.resolve_rope_if_needed(store);
        r.resolve_rope_if_needed(store);
        store[l_string] = l;
        store[r_string] = r;

        if store[l_string].is_utf8() != store[r_string].is_utf8() {
            return None;
        }

        Some([l_string, r_string])
    }
}

impl Data {
    #[inline]
    pub fn as_e_string(&self) -> Option<StoreRef<E::EString>> {
        self.e_string()
    }

    /// Returns true if the result of the "typeof" operator on this expression is
    /// statically determined and this expression has no side effects (i.e. can be
    /// removed without consequence).
    #[inline]
    pub fn to_typeof(&self) -> Option<&'static [u8]> {
        Tag::typeof_(self.tag())
    }

    /// O `+x` conhecido em tempo de transpilação. O texto da string passa pelo `StringToNumber` do JavaScriptCore
    /// (`ul_common::js_number`), que no upstream é a FFI `JSC__jsToNumber`.
    pub fn to_number(&self, store: &Store) -> Option<f64> {
        match self {
            Data::ENull(_) => Some(0.0),
            Data::EUndefined(_) => Some(f64::NAN),
            Data::EString(str) => string_to_number(store, &store[*str]),
            Data::EBoolean(b) | Data::EBranchBoolean(b) => Some(if b.value { 1.0 } else { 0.0 }),
            Data::ENumber(n) => Some(n.value()),
            Data::EInlinedEnum(inlined) => match &store[*inlined].value.data {
                Data::ENumber(num) => Some(num.value()),
                Data::EString(str) => string_to_number(store, &store[*str]),
                _ => None,
            },
            _ => None,
        }
    }

    #[inline]
    pub fn is_string_value(&self) -> bool {
        matches!(self, Data::EString(_))
    }

    /// `hasher` recebe os campos de cada nó um a um (sem bytes de preenchimento), e a tabela de símbolos troca cada
    /// `Ref` pelo nome original, para o hash não depender da numeração. Só a assinatura do React Refresh consome o
    /// resultado; o que importa é a estabilidade.
    pub fn write_to_hasher<S: crate::base::SymbolTable + ?Sized>(
        &self,
        store: &Store,
        hasher: &mut bun_wyhash::Wyhash,
        symbol_table: &mut S,
    ) {
        fn name_of<S: crate::base::SymbolTable + ?Sized>(
            store: &Store,
            hasher: &mut bun_wyhash::Wyhash,
            symbol_table: &mut S,
            r: Ref,
        ) {
            let original_name = symbol_table.get_symbol(r).original_name;
            hasher.update(store.str(original_name));
        }

        hasher.update(&[self.tag() as u8]);
        match *self {
            Data::ENameOfSymbol(e) => name_of(store, hasher, symbol_table, store[e].ref_),
            Data::EArray(e) => {
                let e = &store[e];
                hasher.update(&[e.is_single_line as u8, e.is_parenthesized as u8, e.was_originally_macro as u8]);
                hasher.update(&e.items.len_u32().to_ne_bytes());
                for item in store[e.items].iter() {
                    item.data.write_to_hasher(store, hasher, symbol_table);
                }
            }
            Data::EUnary(e) => {
                let e = &store[e];
                hasher.update(&[e.flags.bits(), e.op as u8]);
                e.value.data.write_to_hasher(store, hasher, symbol_table);
            }
            Data::EBinary(e) => {
                let e = &store[e];
                hasher.update(&[e.op as u8]);
                e.left.data.write_to_hasher(store, hasher, symbol_table);
                e.right.data.write_to_hasher(store, hasher, symbol_table);
            }
            Data::EClass(_) | Data::ENew(_) | Data::ECall(_) | Data::EFunction(_) => {}
            Data::EDot(e) => {
                let e = &store[e];
                // `Option<#[repr(u8)] OptionalChain>` como o byte do nicho (Some(Start)=0,
                // Some(Continuation)=1, None=2), os mesmos bytes que a leitura crua do upstream produz.
                hasher.update(&[e.optional_chain.map_or(2u8, |c| c as u8)]);
                hasher.update(&e.name.len().to_ne_bytes());
                e.target.data.write_to_hasher(store, hasher, symbol_table);
                hasher.update(store.str(e.name));
            }
            Data::EIndex(e) => {
                let e = &store[e];
                hasher.update(&[e.optional_chain.map_or(2u8, |c| c as u8)]);
                e.target.data.write_to_hasher(store, hasher, symbol_table);
                e.index.data.write_to_hasher(store, hasher, symbol_table);
            }
            Data::EArrow(_)
            | Data::EJsxElement(_)
            | Data::EObject(_)
            | Data::ETemplate(_)
            | Data::EIf(_)
            | Data::EImport(_) => {}
            Data::ESpread(e) => store[e].value.data.write_to_hasher(store, hasher, symbol_table),
            Data::EAwait(e) => store[e].value.data.write_to_hasher(store, hasher, symbol_table),
            Data::EYield(e) => {
                let e = &store[e];
                hasher.update(&[e.is_star as u8, e.value.is_some() as u8]);
                if let Some(value) = &e.value {
                    value.data.write_to_hasher(store, hasher, symbol_table);
                }
            }
            Data::EIdentifier(e) => name_of(store, hasher, symbol_table, e.ref_),
            Data::EImportIdentifier(e) => name_of(store, hasher, symbol_table, e.ref_),
            Data::EPrivateIdentifier(e) => name_of(store, hasher, symbol_table, e.ref_),
            Data::ECommonjsExportIdentifier(e) => name_of(store, hasher, symbol_table, e.ref_),
            Data::EBoolean(e) | Data::EBranchBoolean(e) => hasher.update(&[e.value as u8]),
            Data::ENumber(e) => hasher.update(&e.value().to_ne_bytes()),
            Data::EBigInt(e) => hasher.update(store.str(store[e].value)),
            Data::ERegExp(e) => hasher.update(store.str(store[e].value)),
            Data::EString(e) => {
                // Only the *first* rope segment is hashed.
                let current = &store[e];
                if current.is_utf8() {
                    hasher.update(store.str(current.data));
                } else {
                    let bytes: Vec<u8> = current.slice16(store).iter().flat_map(|unit| unit.to_ne_bytes()).collect();
                    hasher.update(&bytes);
                }
                hasher.update(b"\x00");
            }
            // preferably, i'd like to write the filepath
            Data::ERequireString(e) => hasher.update(&e.import_record_index.to_ne_bytes()),
            Data::ERequireResolveString(e) => hasher.update(&e.import_record_index.to_ne_bytes()),
            Data::EImportMetaMain(e) => hasher.update(&[e.inverted as u8]),
            // pretend there is no comment
            Data::EInlinedEnum(e) => store[e].value.data.write_to_hasher(store, hasher, symbol_table),
            // no data
            Data::ERequireCallTarget
            | Data::ERequireResolveCallTarget
            | Data::EMissing(_)
            | Data::EThis(_)
            | Data::ESuper(_)
            | Data::ENull(_)
            | Data::EUndefined(_)
            | Data::ENewTarget(_)
            | Data::ERequireMain
            | Data::EImportMeta(_)
            | Data::ESpecial(_) => {}
        }
    }

    pub fn to_finite_number(&self, store: &Store) -> Option<f64> {
        match self {
            Data::EBoolean(b) | Data::EBranchBoolean(b) => Some(if b.value { 1.0 } else { 0.0 }),
            Data::ENumber(n) => n.value().is_finite().then(|| n.value()),
            Data::EInlinedEnum(inlined) => match &store[*inlined].value.data {
                Data::ENumber(num) => num.value().is_finite().then(|| num.value()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn extract_numeric_value(&self, store: &Store) -> Option<f64> {
        match self {
            Data::ENumber(n) => Some(n.value()),
            Data::EInlinedEnum(inlined) => match &store[*inlined].value.data {
                Data::ENumber(num) => Some(num.value()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn extract_string_value(store: &Store, data: Data) -> Option<StoreRef<E::EString>> {
        match data {
            Data::EString(s) => Some(s),
            Data::EInlinedEnum(inlined) => match store[inlined].value.data {
                Data::EString(str) => Some(str),
                _ => None,
            },
            _ => None,
        }
    }
}

/// `+"texto"`: só a string inteira (sem corda) e em UTF-8; o vazio dá 0 e o que não é ASCII dá NaN antes de chegar
/// ao `StringToNumber`, como no upstream.
fn string_to_number(store: &Store, string: &E::EString) -> Option<f64> {
    if string.next.is_some() || !string.is_utf8() {
        return None;
    }
    let text = string.slice8(store);
    // +"" -> 0
    if text.is_empty() {
        return Some(0.0);
    }
    if !text.is_ascii() {
        return Some(f64::NAN);
    }
    Some(ul_common::js_number::js_to_number_characters(text))
}
