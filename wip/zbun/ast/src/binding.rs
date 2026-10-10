//! `src/ast/binding.rs` do upstream sobre o `Store` (seção "Arena tipada" do README).
//!
//! O `ToExprWrapper` do upstream apaga o tipo do parser (`*mut c_void` e um trampolim) e guarda um `BackRef` para o
//! bump. No porte ele é genérico no contexto `C` do chamador, sem ponteiro cru, e o `Store` vem do próprio contexto
//! (`C: AsMut<Store>`); a ordem das chamadas e a árvore gerada são as do upstream.

use crate::b::B;
use crate::base::Ref;
use crate::expr::{Data as ExprData, Expr};
use crate::new_store::NodeStore as _;
use crate::store::Store;
use crate::{e as E, g as G};
use crate::flags;

/// `Binding`'s payload union lives at `crate::b::B`; re-export it as `Data`
/// so downstream crates can `use crate::binding::Data`.
pub use crate::b::B as Data;

#[derive(Copy, Clone, Default)]
pub struct Binding {
    pub loc: crate::Loc,
    pub data: B,
}

#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Debug, strum::IntoStaticStr)]
pub enum Tag {
    // strum serialize = snake_case tag names (JSON/snapshot compat).
    #[strum(serialize = "b_identifier")]
    BIdentifier,
    #[strum(serialize = "b_array")]
    BArray,
    #[strum(serialize = "b_object")]
    BObject,
    #[strum(serialize = "b_missing")]
    BMissing,
}

// ──────────────────────────────────────────────────────────────────────────
// `alloc`: a small trait implemented for each payload type picks the `B`
// variant; `Binding::alloc` stays monomorphic per call-site.
// ──────────────────────────────────────────────────────────────────────────

pub trait BindingAlloc: Sized {
    fn alloc_into_b(self, store: &mut Store) -> B;
}
impl BindingAlloc for crate::b::Identifier {
    #[inline]
    fn alloc_into_b(self, store: &mut Store) -> B {
        B::BIdentifier(store.alloc(self))
    }
}
impl BindingAlloc for crate::b::Array {
    #[inline]
    fn alloc_into_b(self, store: &mut Store) -> B {
        B::BArray(store.alloc(self))
    }
}
impl BindingAlloc for crate::b::Object {
    #[inline]
    fn alloc_into_b(self, store: &mut Store) -> B {
        B::BObject(store.alloc(self))
    }
}
impl BindingAlloc for crate::b::Missing {
    #[inline]
    fn alloc_into_b(self, _store: &mut Store) -> B {
        B::BMissing(crate::b::Missing {})
    }
}

impl Binding {
    #[inline]
    pub fn alloc(store: &mut Store, t: impl BindingAlloc, loc: crate::Loc) -> Binding {
        Binding {
            loc,
            data: t.alloc_into_b(store),
        }
    }
}

/// O conversor de identificador que o parser instala (`wrap_identifier_namespace` ou `wrap_identifier_hoisting`).
pub struct ToExprWrapper<C> {
    wrap: fn(&mut C, crate::Loc, Ref) -> Expr,
}

impl<C> Clone for ToExprWrapper<C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<C> Copy for ToExprWrapper<C> {}

impl<C> ToExprWrapper<C> {
    #[inline]
    pub const fn new(wrap: fn(&mut C, crate::Loc, Ref) -> Expr) -> Self {
        Self { wrap }
    }
}

impl Binding {
    pub fn to_expr<C: AsMut<Store>>(binding: &Binding, ctx: &mut C, wrapper: ToExprWrapper<C>) -> Expr {
        let loc = binding.loc;
        match binding.data {
            B::BMissing(_) => Expr {
                data: ExprData::EMissing(E::Missing {}),
                loc,
            },
            B::BIdentifier(b) => {
                let ref_ = ctx.as_mut()[b].r#ref;
                (wrapper.wrap)(ctx, loc, ref_)
            }
            B::BArray(b) => {
                let b = ctx.as_mut()[b];
                let len = b.items.len();
                let mut exprs = Vec::with_capacity(len);
                for i in 0..len {
                    let item = ctx.as_mut()[b.items][i];
                    let expr = Self::to_expr(&item.binding, ctx, wrapper);
                    let store = ctx.as_mut();
                    let converted = if b.has_spread && i == len - 1 {
                        Expr::init(store, E::Spread { value: expr }, expr.loc)
                    } else if let Some(default) = item.default_value {
                        Expr::assign(store, expr, default)
                    } else {
                        expr
                    };
                    exprs.push(converted);
                }
                let store = ctx.as_mut();
                let items = store.alloc_slice(exprs);
                Expr::init(
                    store,
                    E::Array {
                        items,
                        is_single_line: b.is_single_line,
                        ..Default::default()
                    },
                    loc,
                )
            }
            B::BObject(b) => {
                let b = ctx.as_mut()[b];
                let mut properties = Vec::with_capacity(b.properties.len());
                for i in 0..b.properties.len() {
                    let item = ctx.as_mut()[b.properties][i];
                    properties.push(G::Property {
                        flags: item.flags,
                        key: Some(item.key),
                        kind: if item.flags.contains(flags::Property::IsSpread) {
                            G::PropertyKind::Spread
                        } else {
                            G::PropertyKind::Normal
                        },
                        value: Some(Self::to_expr(&item.value, ctx, wrapper)),
                        initializer: item.default_value,
                        ..Default::default()
                    });
                }
                let store = ctx.as_mut();
                let properties = store.alloc_slice(properties);
                Expr::init(
                    store,
                    E::Object {
                        properties,
                        is_single_line: b.is_single_line,
                        ..Default::default()
                    },
                    loc,
                )
            }
        }
    }
}
