//! `src/ast/g.rs` do upstream. Os vetores do `AstAlloc` (`DeclList`, `PropertyList`, as listas de decorators e de
//! sentenças) viram faixas do `Store`, e o que o upstream lê por `Deref` do `StoreRef` passa pelo `Store` (seção
//! "Arena tipada" do README). Os `deep_clone` entram com o `Data::deep_clone` de `expr.rs`.

use crate::base::Ref;
use crate::binding::Binding as BindingNodeIndex;
use crate::expr::Expr as ExprNodeIndex;
use crate::stmt::Stmt;
use crate::store::Store;
use crate::ts as TypeScript;
use crate::{ExprData, ExprNodeList, LocRef, StmtNodeList, StoreRef, StoreSlice, StoreStr, flags};

/// Downstream crates address the flag enum via `G::FnFlags::IsExport` etc.;
/// re-export the enum + set type here.
pub use crate::flags::Function as FnFlags;

#[derive(Clone, Copy, Default)]
pub struct Decl {
    pub binding: BindingNodeIndex,
    pub value: Option<ExprNodeIndex>,
}

pub type DeclList = StoreSlice<Decl>;

pub struct NamespaceAlias {
    pub namespace_ref: Ref,
    pub alias: StoreStr,

    pub was_originally_property_access: bool,

    pub import_record_index: u32,
}

impl Default for NamespaceAlias {
    fn default() -> Self {
        Self {
            namespace_ref: Ref::default(),
            alias: StoreStr::EMPTY,
            was_originally_property_access: false,
            import_record_index: u32::MAX,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ExportStarAlias {
    pub loc: crate::Loc,

    // Although this alias name starts off as being the same as the statement's
    // namespace symbol, it may diverge if the namespace symbol name is minified.
    // The original alias name is preserved here to avoid this scenario.
    pub original_name: StoreStr,
}

#[derive(Clone, Copy)]
pub struct Class {
    pub class_keyword: crate::Range,
    pub ts_decorators: ExprNodeList,
    pub class_name: Option<LocRef>,
    pub extends: Option<ExprNodeIndex>,
    pub body_loc: crate::Loc,
    pub close_brace_loc: crate::Loc,
    pub properties: StoreSlice<Property>,
    pub has_decorators: bool,
    pub should_lower_standard_decorators: bool,
}

impl Default for Class {
    fn default() -> Self {
        Self {
            class_keyword: crate::Range::NONE,
            ts_decorators: ExprNodeList::EMPTY,
            class_name: None,
            extends: None,
            body_loc: crate::Loc::EMPTY,
            close_brace_loc: crate::Loc::EMPTY,
            properties: StoreSlice::EMPTY,
            has_decorators: false,
            should_lower_standard_decorators: false,
        }
    }
}

impl Class {
    pub fn can_be_moved(&self, store: &Store) -> bool {
        if self.extends.is_some() {
            return false;
        }

        if self.has_decorators {
            return false;
        }

        for property in store[self.properties].iter() {
            if property.kind == PropertyKind::ClassStaticBlock {
                return false;
            }

            let f = property.flags;
            if f.contains(flags::Property::IsComputed) || f.contains(flags::Property::IsSpread) {
                return false;
            }

            if property.kind == PropertyKind::Normal && f.contains(flags::Property::IsStatic) {
                for val in [property.value, property.initializer].into_iter().flatten() {
                    match val.data {
                        ExprData::EArrow(..) | ExprData::EFunction(..) => {}
                        _ => {
                            if !val.data.can_be_moved(store) {
                                return false;
                            }
                        }
                    }
                }
            }
        }

        true
    }
}

// invalid shadowing if left as Comment
#[derive(Clone, Copy)]
pub struct Comment {
    pub loc: crate::Loc,
    pub text: StoreStr,
}

#[derive(Clone, Copy)]
pub struct ClassStaticBlock {
    pub stmts: StmtNodeList,
    pub loc: crate::Loc,
}

#[derive(Clone)]
pub struct Property {
    /// This is used when parsing a pattern that uses default values:
    ///
    ///   [a = 1] = [];
    ///   ({a = 1} = {});
    ///
    /// It's also used for class fields:
    ///
    ///   class Foo { a = 1 }
    ///
    pub initializer: Option<ExprNodeIndex>,
    pub kind: PropertyKind,
    pub flags: flags::PropertySet,

    pub class_static_block: Option<StoreRef<ClassStaticBlock>>,
    pub ts_decorators: ExprNodeList,
    // Key is optional for spread
    pub key: Option<ExprNodeIndex>,

    // This is omitted for class fields
    pub value: Option<ExprNodeIndex>,

    pub ts_metadata: TypeScript::Metadata,
}

pub type PropertyList = StoreSlice<Property>;

impl Default for Property {
    fn default() -> Self {
        Self {
            initializer: None,
            kind: PropertyKind::Normal,
            flags: flags::PROPERTY_NONE,
            class_static_block: None,
            ts_decorators: ExprNodeList::EMPTY,
            key: None,
            value: None,
            ts_metadata: TypeScript::Metadata::MNone,
        }
    }
}

impl Property {
    /// `class_static_block` read through the `Store`.
    #[inline]
    pub fn class_static_block_ref<'s>(&self, store: &'s Store) -> Option<&'s ClassStaticBlock> {
        self.class_static_block.map(|block| &store[block])
    }
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PropertyKind {
    Normal,
    Get,
    Set,
    Spread,
    Declare,
    Abstract,
    ClassStaticBlock,
    AutoAccessor,
}

#[derive(Clone, Copy)]
pub struct FnBody {
    pub loc: crate::Loc,
    pub stmts: StmtNodeList,
}

impl FnBody {
    pub fn init_return_expr(store: &mut Store, expr: ExprNodeIndex) -> FnBody {
        let stmt = Stmt::alloc(store, crate::s::Return { value: Some(expr) }, expr.loc);
        FnBody {
            stmts: store.alloc_slice([stmt]),
            loc: expr.loc,
        }
    }
}

#[derive(Clone)]
pub struct Fn {
    pub name: Option<LocRef>,
    pub open_parens_loc: crate::Loc,
    pub args: StoreSlice<Arg>,
    // This was originally nullable, but doing so I believe caused a miscompilation
    // Specifically, the body was always null.
    pub body: FnBody,
    pub arguments_ref: Ref,

    pub flags: flags::FunctionSet,

    pub return_ts_metadata: TypeScript::Metadata,
}

impl Default for Fn {
    fn default() -> Self {
        Self {
            name: None,
            open_parens_loc: crate::Loc::EMPTY,
            args: StoreSlice::EMPTY,
            body: FnBody {
                loc: crate::Loc::EMPTY,
                stmts: StmtNodeList::EMPTY,
            },
            arguments_ref: Ref::NONE,
            flags: flags::FUNCTION_NONE,
            return_ts_metadata: TypeScript::Metadata::MNone,
        }
    }
}

#[derive(Clone)]
pub struct Arg {
    pub ts_decorators: ExprNodeList,
    pub binding: BindingNodeIndex,
    pub default: Option<ExprNodeIndex>,

    // "constructor(public x: boolean) {}"
    pub is_typescript_ctor_field: bool,

    pub ts_metadata: TypeScript::Metadata,
}

impl Default for Arg {
    fn default() -> Self {
        Self {
            ts_decorators: ExprNodeList::EMPTY,
            binding: BindingNodeIndex::default(),
            default: None,
            is_typescript_ctor_field: false,
            ts_metadata: TypeScript::Metadata::MNone,
        }
    }
}

use crate::new_store::NodeStore as _;
