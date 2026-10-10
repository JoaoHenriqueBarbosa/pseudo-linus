//! `src/ast/ts.rs` do upstream: `TSNamespaceScope`, `TSNamespaceMember` e `Metadata`, o tipo dos metadados de
//! decorators que `G::Property`, `G::Arg` e `G::Fn` carregam. Os mapas por nome são o `StringArrayHashMap` do
//! `bun_collections` (chave própria, ordem de inserção) e vivem no `Store`, como os nós.

use crate::Loc;
use crate::base::Ref;
use crate::e::EString;
use crate::nodes::StoreRef;
use bun_collections::StringArrayHashMap;

/// This is for TypeScript "enum" and "namespace" blocks. Each block can
/// potentially be instantiated multiple times. The exported members of each
/// block are merged into a single namespace while the non-exported code is
/// still scoped to just within that block:
///
///    let x = 1;
///    namespace Foo {
///      let x = 2;
///      export let y = 3;
///    }
///    namespace Foo {
///      console.log(x); // 1
///      console.log(y); // 3
///    }
///
/// This is a form of identifier lookup that works differently than the
/// hierarchical scope-based identifier lookup in JavaScript. Lookup now needs
/// to search sibling scopes in addition to parent scopes. This is accomplished
/// by sharing the map of exported members between all matching sibling scopes.
pub struct TSNamespaceScope {
    /// This is specific to this namespace block. It's the argument of the
    /// immediately-invoked function expression that the namespace block is
    /// compiled into:
    ///
    ///   var ns;
    ///   (function (ns2) {
    ///     ns2.x = 123;
    ///   })(ns || (ns = {}));
    ///
    /// This variable is "ns2" in the above example.
    pub arg_ref: Ref,

    /// This is shared between all sibling namespace blocks: the same index in the `Store`.
    pub exported_members: StoreRef<TSNamespaceMemberMap>,

    /// This is a lazily-generated map of identifiers that actually represent
    /// property accesses to this namespace's properties. Unique per namespace
    /// block because the argument symbol is specific to that block.
    pub property_accesses: StringArrayHashMap<Ref>,

    /// Implicit references only work namespace-to-namespace and enum-to-enum,
    /// so we only allow enum/enum and namespace/namespace interactions.
    pub is_enum_scope: bool,
}

pub type TSNamespaceMemberMap = StringArrayHashMap<TSNamespaceMember>;

#[derive(Clone, Copy)]
pub struct TSNamespaceMember {
    pub loc: Loc,
    pub data: Data,
}

#[derive(Clone, Copy)]
pub enum Data {
    /// "namespace ns { export let it }"
    Property,
    /// "namespace ns { export namespace it {} }"
    Namespace(StoreRef<TSNamespaceMemberMap>),
    /// "enum ns { it }"
    EnumNumber(f64),
    /// "enum ns { it = 'it' }"
    EnumString(StoreRef<EString>),
    /// "enum ns { it = something() }"
    EnumProperty,
}

impl Data {
    pub fn is_enum(&self) -> bool {
        matches!(self, Data::EnumNumber(_) | Data::EnumString(_) | Data::EnumProperty)
    }
}

// ── TypeScript::Metadata ───────────────────────────────────────────────────
// Decorator-metadata type tag attached to `G.Property` / `G.FnArg` / `G.Fn`.
// Data-only; the parser-state predicates that depend on `P` stay in
// `bun_js_parser::typescript`.

#[derive(Clone, Default)]
pub enum Metadata {
    #[default]
    MNone,

    MNever,
    MUnknown,
    MAny,
    MVoid,
    MNull,
    MUndefined,
    MFunction,
    MArray,
    MBoolean,
    MString,
    MObject,
    MNumber,
    MBigint,
    MSymbol,
    MPromise,
    MIdentifier(Ref),
    // A heap `Vec` is used here because `Metadata` is lifetime-free.
    // Decorator metadata is rare and the lists are tiny.
    MDot(Vec<Ref>),
}

impl Metadata {
    pub const DEFAULT: Self = Metadata::MNone;

    // the logic in finish_union, merge_union, finish_intersection and merge_intersection is
    // translated from:
    // https://github.com/microsoft/TypeScript/blob/e0a324b0503be479f2b33fd2e17c6e86c94d1297/src/compiler/transformers/typeSerializer.ts#L402

    /// Return the final union type if possible, or return None to continue merging.
    ///
    /// If the current type is MNever, MNull, or MUndefined assign the current type
    /// to MNone and return None to ensure it's always replaced by the next type.
    /// `load_name`: closure form of `p.load_name_from_ref` to avoid coupling Metadata to P.
    pub fn finish_union<'b, F: Fn(Ref) -> &'b [u8]>(&mut self, load_name: F) -> Option<Self> {
        let current = self;
        match current {
            Metadata::MIdentifier(r) => {
                if load_name(*r) == b"Object" {
                    return Some(Metadata::MObject);
                }
                None
            }

            Metadata::MUnknown | Metadata::MAny | Metadata::MObject => Some(Metadata::MObject),

            Metadata::MNever | Metadata::MNull | Metadata::MUndefined => {
                *current = Metadata::MNone;
                None
            }

            _ => None,
        }
    }

    pub fn merge_union(&mut self, left: Self) {
        let result = self;
        if !matches!(left, Metadata::MNone) {
            if core::mem::discriminant(result) != core::mem::discriminant(&left) {
                *result = match result {
                    Metadata::MNever | Metadata::MUndefined | Metadata::MNull => left,

                    _ => Metadata::MObject,
                };
            } else {
                // Reshaped for borrowck: copy Ref out before reassigning *result
                if let Metadata::MIdentifier(r) = result {
                    let r = *r;
                    if let Metadata::MIdentifier(l) = left {
                        if !r.eql(l) {
                            *result = Metadata::MObject;
                        }
                    }
                }
            }
        } else {
            // always take the next value if left is MNone
        }
    }

    /// Return the final intersection type if possible, or return None to continue merging.
    ///
    /// If the current type is MUnknown, MNull, or MUndefined assign the current type
    /// to MNone and return None to ensure it's always replaced by the next type.
    pub fn finish_intersection<'b, F: Fn(Ref) -> &'b [u8]>(&mut self, load_name: F) -> Option<Self> {
        let current = self;
        match current {
            Metadata::MIdentifier(r) => {
                if load_name(*r) == b"Object" {
                    return Some(Metadata::MObject);
                }
                None
            }

            // ensure MNever is the final type
            Metadata::MNever => Some(Metadata::MNever),

            Metadata::MAny | Metadata::MObject => Some(Metadata::MObject),

            Metadata::MUnknown | Metadata::MNull | Metadata::MUndefined => {
                *current = Metadata::MNone;
                None
            }

            _ => None,
        }
    }

    pub fn merge_intersection(&mut self, left: Self) {
        let result = self;
        if !matches!(left, Metadata::MNone) {
            if core::mem::discriminant(result) != core::mem::discriminant(&left) {
                *result = match result {
                    Metadata::MUnknown | Metadata::MUndefined | Metadata::MNull => left,

                    // ensure MNever is the final type
                    Metadata::MNever => Metadata::MNever,

                    _ => Metadata::MObject,
                };
            } else {
                // Reshaped for borrowck: copy Ref out before reassigning *result
                if let Metadata::MIdentifier(r) = result {
                    let r = *r;
                    if let Metadata::MIdentifier(l) = left {
                        if !r.eql(l) {
                            *result = Metadata::MObject;
                        }
                    }
                }
            }
        } else {
            // make sure intersection of only MUnknown serializes to "undefined"
            // instead of "Object"
            if matches!(result, Metadata::MUnknown) {
                *result = Metadata::MUndefined;
            }
        }
    }
}
