//! `src/ast/stmt.rs` do upstream sobre o `Store` (seção "Arena tipada" do README). O `Data::Store` de thread e as
//! vias de bump (`allocate`, `allocate_data`, `allocate_expr`) dão lugar a `Stmt::alloc(store, ...)`.

use crate::expr::{self, Expr};
use crate::new_store::NodeStore as _;
use crate::s as S;
use crate::store::Store;
use crate::StoreRef;

#[derive(Clone, Copy)]
pub struct Stmt {
    pub loc: crate::Loc,
    pub data: Data,
}

impl Stmt {
    pub fn assign(store: &mut Store, a: Expr, b: Expr) -> Stmt {
        let value = Expr::assign(store, a, b);
        Stmt::alloc(
            store,
            S::SExpr {
                value,
                ..Default::default()
            },
            a.loc,
        )
    }
}

impl Stmt {
    pub fn is_super_call(self, store: &Store) -> bool {
        if let Data::SExpr(s_expr) = self.data {
            if let expr::Data::ECall(e_call) = store[s_expr].value.data {
                return matches!(store[e_call].target.data, expr::Data::ESuper(_));
            }
        }
        false
    }

    pub fn is_missing_expr(self, store: &Store) -> bool {
        if let Data::SExpr(s_expr) = self.data {
            return matches!(store[s_expr].value.data, expr::Data::EMissing(_));
        }
        false
    }

    pub fn empty() -> Stmt {
        Stmt {
            data: Data::SEmpty(NONE),
            loc: crate::Loc::default(),
        }
    }

    pub fn to_empty(self) -> Stmt {
        Stmt {
            data: Data::SEmpty(NONE),
            loc: self.loc,
        }
    }
}

impl Default for Stmt {
    /// Used to zero-init `loop_body` and bulk-fill stmt slices before
    /// population.
    #[inline]
    fn default() -> Self {
        Stmt::empty()
    }
}

const NONE: S::Empty = S::Empty {};

/// Each `S::*` payload type implements this to map itself onto the
/// corresponding `Data` variant; the arm list is the `impl_statement_data!`
/// invocation below.
pub trait StatementData: Sized {
    /// Wrap an already-allocated payload.
    fn wrap_ref(ptr: StoreRef<Self>) -> Data;
    /// Store-append `self` and wrap.
    fn store_alloc(self, store: &mut Store) -> Data;
}

impl Stmt {
    #[inline]
    pub fn init<T: StatementData>(orig_data: StoreRef<T>, loc: crate::Loc) -> Stmt {
        Stmt {
            loc,
            data: T::wrap_ref(orig_data),
        }
    }

    #[inline]
    pub fn alloc<T: StatementData>(store: &mut Store, orig_data: T, loc: crate::Loc) -> Stmt {
        Stmt {
            loc,
            data: orig_data.store_alloc(store),
        }
    }
}

// ─── StatementData impls ───────────────────────────────────────────────────

macro_rules! impl_statement_data {
    // Pointer-payload variants: stored via Store.
    ( ptr: $( ($ty:ty, $variant:ident) ),* $(,)?
      ; inline: $( ($ity:ty, $ivariant:ident) ),* $(,)? ) => {
        $(
            impl StatementData for $ty {
                #[inline]
                fn wrap_ref(ptr: StoreRef<Self>) -> Data { Data::$variant(ptr) }
                #[inline]
                fn store_alloc(self, store: &mut Store) -> Data {
                    Data::$variant(store.alloc(self))
                }
            }
        )*
        $(
            impl StatementData for $ity {
                #[inline]
                fn wrap_ref(_ptr: StoreRef<Self>) -> Data { Data::$ivariant(<$ity>::default()) }
                #[inline]
                fn store_alloc(self, _store: &mut Store) -> Data { Data::$ivariant(self) }
            }
        )*
    };
}

impl_statement_data! {
    ptr:
        (S::Block,         SBlock),
        (S::Break,         SBreak),
        (S::Class,         SClass),
        (S::Comment,       SComment),
        (S::Continue,      SContinue),
        (S::Directive,     SDirective),
        (S::DoWhile,       SDoWhile),
        (S::Enum,          SEnum),
        (S::ExportClause,  SExportClause),
        (S::ExportDefault, SExportDefault),
        (S::ExportEquals,  SExportEquals),
        (S::ExportFrom,    SExportFrom),
        (S::ExportStar,    SExportStar),
        (S::SExpr,         SExpr),
        (S::ForIn,         SForIn),
        (S::ForOf,         SForOf),
        (S::For,           SFor),
        (S::Function,      SFunction),
        (S::If,            SIf),
        (S::Import,        SImport),
        (S::Label,         SLabel),
        (S::Local,         SLocal),
        (S::Namespace,     SNamespace),
        (S::Return,        SReturn),
        (S::Switch,        SSwitch),
        (S::Throw,         SThrow),
        (S::Try,           STry),
        (S::While,         SWhile),
        (S::With,          SWith),
    ; inline:
        (S::Empty,      SEmpty),
        (S::Debugger,   SDebugger),
        (S::TypeScript, STypeScript),
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Tag {
    SBlock,
    SBreak,
    SClass,
    SComment,
    SContinue,
    SDirective,
    SDoWhile,
    SEnum,
    SExportClause,
    SExportDefault,
    SExportEquals,
    SExportFrom,
    SExportStar,
    SExpr,
    SForIn,
    SForOf,
    SFor,
    SFunction,
    SIf,
    SImport,
    SLabel,
    SLocal,
    SNamespace,
    SReturn,
    SSwitch,
    SThrow,
    STry,
    SWhile,
    SWith,
    STypeScript,
    SEmpty,
    SDebugger,
    SLazyExport,
}

impl Tag {
    pub fn is_export_like(self) -> bool {
        matches!(
            self,
            Tag::SExportClause
                | Tag::SExportDefault
                | Tag::SExportEquals
                | Tag::SExportFrom
                | Tag::SExportStar
                | Tag::SEmpty
        )
    }
}

enum_tag! {
    #[derive(Clone, Copy)]
    pub enum Data: Tag {
        SBlock(StoreRef<S::Block>),
        SBreak(StoreRef<S::Break>),
        SClass(StoreRef<S::Class>),
        SComment(StoreRef<S::Comment>),
        SContinue(StoreRef<S::Continue>),
        SDirective(StoreRef<S::Directive>),
        SDoWhile(StoreRef<S::DoWhile>),
        SEnum(StoreRef<S::Enum>),
        SExportClause(StoreRef<S::ExportClause>),
        SExportDefault(StoreRef<S::ExportDefault>),
        SExportEquals(StoreRef<S::ExportEquals>),
        SExportFrom(StoreRef<S::ExportFrom>),
        SExportStar(StoreRef<S::ExportStar>),
        SExpr(StoreRef<S::SExpr>),
        SForIn(StoreRef<S::ForIn>),
        SForOf(StoreRef<S::ForOf>),
        SFor(StoreRef<S::For>),
        SFunction(StoreRef<S::Function>),
        SIf(StoreRef<S::If>),
        SImport(StoreRef<S::Import>),
        SLabel(StoreRef<S::Label>),
        SLocal(StoreRef<S::Local>),
        SNamespace(StoreRef<S::Namespace>),
        SReturn(StoreRef<S::Return>),
        SSwitch(StoreRef<S::Switch>),
        SThrow(StoreRef<S::Throw>),
        STry(StoreRef<S::Try>),
        SWhile(StoreRef<S::While>),
        SWith(StoreRef<S::With>),

        STypeScript(S::TypeScript),
        SEmpty(S::Empty), // special case, its a zero value type
        SDebugger(S::Debugger),

        SLazyExport(StoreRef<expr::Data>),
    }
}

// ── Layout guards ─────────────────────────────────────────────────────────
// Todo payload é um `StoreRef<T>` (4 bytes) ou um ZST: `Data` tem 8 bytes e `Stmt` 12 (16 no upstream, com o
// ponteiro de 8). `Option<Data>`/`Option<Stmt>` niche-pack via spare discriminant values.
const _: () = assert!(core::mem::size_of::<Data>() == 8);
const _: () = assert!(core::mem::align_of::<Data>() == 4);
const _: () = assert!(core::mem::size_of::<Stmt>() == 12);
const _: () = assert!(core::mem::size_of::<Option<Data>>() == core::mem::size_of::<Data>());
const _: () = assert!(core::mem::size_of::<Option<Stmt>>() == core::mem::size_of::<Stmt>());

data_accessors! {
    Data;
    s_class / s_class_mut => SClass: S::Class,
    s_expr / s_expr_mut => SExpr: S::SExpr,
    s_function / s_function_mut => SFunction: S::Function,
    s_import => SImport: S::Import,
    s_local / s_local_mut => SLocal: S::Local,
    s_lazy_export => SLazyExport: expr::Data,
}
