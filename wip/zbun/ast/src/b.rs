//! `src/ast/b.rs` do upstream sobre o `Store` (seção "Arena tipada" do README).

use crate::StoreRef;
use crate::base::Ref;
use crate::binding::Binding;
use crate::expr::Expr;
use crate::store::Store;
use crate::{ArrayBinding, ExprNodeIndex, flags};

// The tag enum lives on `Binding::Tag`.
enum_tag! {
/// B is for Binding! Bindings are on the left side of variable
/// declarations (s_local), which is how destructuring assignments
/// are represented in memory. Consider a basic example.
///
/// ```text
/// let hello = world;
///     ^       ^
///     |       E.Identifier
///     B.Identifier
/// ```
///
/// Bindings can be nested
///
/// ```text
///            B.Array
///            | B.Identifier
///            | |
/// let { foo: [ bar ] } = ...
///     ----------------
///     B.Object
/// ```
    #[derive(Copy, Clone)]
    pub enum B: Tag {
        // let x = ...
        BIdentifier(StoreRef<Identifier>),
        // let [a, b] = ...
        BArray(StoreRef<Array>),
        // let { a, b: c } = ...
        BObject(StoreRef<Object>),
        // this is used to represent array holes
        BMissing(Missing),
    }
}

use crate::binding::Tag;

impl Default for B {
    fn default() -> Self {
        B::BMissing(Missing {})
    }
}

// ── Layout guards ─────────────────────────────────────────────────────────
// Três `StoreRef<T>` de 4 bytes e um ZST: `B` tem 8 bytes e `Binding` 12 (12 e 16 no upstream).
const _: () = assert!(core::mem::size_of::<B>() == 8);
const _: () = assert!(core::mem::align_of::<B>() == 4);
const _: () = assert!(core::mem::size_of::<Binding>() == 12);
const _: () = assert!(core::mem::size_of::<Option<B>>() == core::mem::size_of::<B>());

#[derive(Clone, Copy)]
pub struct Identifier {
    pub r#ref: Ref,
}

#[derive(Clone, Copy)]
pub struct Property {
    pub flags: flags::PropertySet,
    pub key: ExprNodeIndex,
    pub value: Binding,
    pub default_value: Option<Expr>,
}
// No `impl Default` on purpose: only `flags`/`default_value` have sensible
// defaults; `key`/`value` do not, so every constructor must supply them.

#[derive(Clone, Copy)]
pub struct Object {
    pub properties: crate::StoreSlice<Property>,
    pub is_single_line: bool,
}
// No `impl Default` on purpose: only `is_single_line` has a sensible default;
// `properties` does not, so every constructor must supply it.

#[derive(Clone, Copy)]
pub struct Array {
    pub items: crate::StoreSlice<ArrayBinding>,
    pub has_spread: bool,
    pub is_single_line: bool,
}
// No `impl Default` on purpose: only `has_spread`/`is_single_line` have
// sensible defaults; `items` does not, so every constructor must supply it.

#[derive(Default, Copy, Clone)]
pub struct Missing {}

impl B {
    /// A parte do hash do React Fast Refresh que cabe às ligações, campo a campo (ver `Data::write_to_hasher`).
    pub fn write_to_hasher<S: crate::base::SymbolTable + ?Sized>(
        &self,
        store: &Store,
        hasher: &mut bun_wyhash::Wyhash,
        symbol_table: &mut S,
    ) {
        match *self {
            B::BIdentifier(id) => {
                let original_name = symbol_table.get_symbol(store[id].r#ref).original_name;
                hasher.update(&[self.tag() as u8]);
                hasher.update(&original_name.len().to_ne_bytes());
            }
            B::BArray(array) => {
                let array = &store[array];
                hasher.update(&[self.tag() as u8, array.has_spread as u8]);
                hasher.update(&array.items.len().to_ne_bytes());
                for item in store[array.items].iter() {
                    hasher.update(&[item.default_value.is_some() as u8]);
                    if let Some(default) = &item.default_value {
                        default.data.write_to_hasher(store, hasher, symbol_table);
                    }
                    item.binding.data.write_to_hasher(store, hasher, symbol_table);
                }
            }
            B::BObject(object) => {
                let object = &store[object];
                hasher.update(&[self.tag() as u8]);
                hasher.update(&object.properties.len().to_ne_bytes());
                for property in store[object.properties].iter() {
                    hasher.update(&[property.default_value.is_some() as u8, property.flags.as_u8()]);
                    if let Some(default) = &property.default_value {
                        default.data.write_to_hasher(store, hasher, symbol_table);
                    }
                    property.key.data.write_to_hasher(store, hasher, symbol_table);
                    property.value.data.write_to_hasher(store, hasher, symbol_table);
                }
            }
            B::BMissing(_) => {}
        }
    }
}

// Ergonomic slice accessors so P-helpers can `for item in arr.items(store)`.
impl Array {
    #[inline]
    pub fn items<'s>(&self, store: &'s Store) -> &'s [ArrayBinding] {
        &store[self.items]
    }
    #[inline]
    pub fn items_mut<'s>(&self, store: &'s mut Store) -> &'s mut [ArrayBinding] {
        &mut store[self.items]
    }
}
impl Object {
    #[inline]
    pub fn properties<'s>(&self, store: &'s Store) -> &'s [Property] {
        &store[self.properties]
    }
    #[inline]
    pub fn properties_mut<'s>(&self, store: &'s mut Store) -> &'s mut [Property] {
        &mut store[self.properties]
    }
}
