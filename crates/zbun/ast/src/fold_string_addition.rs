//! `src/ast/fold_string_addition.rs` do bun 1.4.2: a dobra de `"a" + "b"` e de templates. As cordas (`next`/`end`)
//! são índices no `Store`, e cada nó de corda é alterado no próprio índice (`store[r]`), como o `DerefMut` do
//! `StoreRef` fazia no upstream.

use crate::expr::{Data, PrimitiveType};
use crate::new_store::NodeStore as _;
use crate::store::Store;
use crate::{E, Expr, StoreRef, StoreSlice, e};

/// Link `other` onto `lhs`'s rope tail.
fn estring_push(store: &mut Store, lhs: &mut E::EString, other: StoreRef<E::EString>) {
    debug_assert!(lhs.is_utf8());
    debug_assert!(store[other].is_utf8());

    if store[other].rope_len == 0 {
        store[other].rope_len = store[other].data.len() as u32;
    }
    if lhs.rope_len == 0 {
        lhs.rope_len = lhs.data.len() as u32;
    }
    lhs.rope_len += store[other].rope_len;

    if lhs.next.is_none() {
        lhs.next = Some(other);
        lhs.end = Some(other);
    } else {
        let mut end = lhs.end.expect("corda com next tem end");
        while store[end].next.is_some() {
            end = store[end].end.expect("corda com next tem end");
        }
        store[end].next = Some(other);
        lhs.end = Some(other);
    }
}

/// Deep-copy the `next` chain into fresh Store nodes so mutating the result
/// can't alias an inlined-enum's string.
fn clone_rope_nodes(store: &mut Store, s: &E::EString) -> E::EString {
    let mut root = s.shallow_clone();
    if let Some(first) = root.next {
        let mut tail = store.alloc(store[first].shallow_clone());
        root.next = Some(tail);
        while let Some(next) = store[tail].next {
            let cloned = store.alloc(store[next].shallow_clone());
            store[tail].next = Some(cloned);
            tail = cloned;
        }
        root.end = Some(tail);
    }
    root
}

/// Concatenate two `E::String`s, mutating BOTH inputs
/// unless `has_inlined_enum_poison` is set.
///
/// Currently inlined enum poison refers to where mutation would cause output
/// bugs due to inlined enum values sharing `E::String`s. If a new use case
/// besides inlined enums comes up to set this to true, please rename the
/// variable and document it.
fn join_strings(store: &mut Store, left: &E::EString, right: &E::EString, has_inlined_enum_poison: bool) -> E::EString {
    let mut new = if has_inlined_enum_poison {
        // Inlined enums can be shared by multiple call sites. In
        // this case, we need to ensure that the ENTIRE rope is
        // cloned. In other situations, the lhs doesn't have any
        // other owner, so it is fine to mutate `lhs.data.end.next`.
        //
        // Consider the following case:
        //   const enum A {
        //     B = "a" + "b",
        //     D = B + "d",
        //   };
        //   console.log(A.B, A.D);
        clone_rope_nodes(store, left)
    } else {
        left.shallow_clone()
    };

    // Similarly, the right side has to be cloned for an enum rope too.
    //
    // Consider the following case:
    //   const enum A {
    //     B = "1" + "2",
    //     C = ("3" + B) + "4",
    //   };
    //   console.log(A.B, A.C);
    let rhs = if has_inlined_enum_poison { clone_rope_nodes(store, right) } else { right.shallow_clone() };
    let rhs_clone = store.alloc(rhs);

    estring_push(store, &mut new, rhs_clone);
    new.prefer_template = new.prefer_template || store[rhs_clone].prefer_template;

    new
}

/// Concat two `TemplatePart` slices into the `Store`.
fn concat_parts(store: &mut Store, a: StoreSlice<e::TemplatePart>, b: StoreSlice<e::TemplatePart>) -> StoreSlice<e::TemplatePart> {
    let parts: Vec<e::TemplatePart> = store[a].iter().chain(store[b].iter()).copied().collect();
    store.alloc_slice(parts)
}

/// Transforming the left operand into a string is not safe if it comes from a
/// nested AST node.
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum FoldStringAdditionKind {
    /// "x" + "y" -> "xy"
    /// 1 + "y" -> "1y"
    Normal,
    /// a + "x" + "y" -> a + "xy"
    /// a + 1 + "y" -> a + 1 + y
    NestedLeft,
}

/// NOTE: unlike esbuild's js_ast_helpers.FoldStringAddition, this does mutate
/// the input AST in the case of rope strings
pub fn fold_string_addition(l: Expr, r: Expr, store: &mut Store, kind: FoldStringAdditionKind) -> Option<Expr> {
    // "See through" inline enum constants
    // TODO: implement foldAdditionPreProcess to fold some more things :)
    let mut lhs = l.unwrap_inlined(store);
    let mut rhs = r.unwrap_inlined(store);

    if kind != FoldStringAdditionKind::NestedLeft {
        // See comment on `FoldStringAdditionKind` for examples
        if let Data::EString(_) | Data::ETemplate(_) = rhs.data {
            if let Some(str) = lhs.to_string_expr_without_side_effects(store) {
                lhs = str;
            }
        }
    }

    match lhs.data {
        Data::EString(left) => {
            if let Some(str) = rhs.to_string_expr_without_side_effects(store) {
                rhs = str;
            }

            if store[left].is_utf8() {
                match rhs.data {
                    // "bar" + "baz" => "barbaz"
                    Data::EString(right) => {
                        if store[right].is_utf8() {
                            let has_inlined_enum_poison =
                                matches!(l.data, Data::EInlinedEnum(_)) || matches!(r.data, Data::EInlinedEnum(_));

                            let (left, right) = (store[left], store[right]);
                            let joined = join_strings(store, &left, &right, has_inlined_enum_poison);
                            return Some(Expr::init(store, joined, lhs.loc));
                        }
                    }
                    // "bar" + `baz${bar}` => `barbaz${bar}`
                    Data::ETemplate(right) => {
                        let right = store[right];
                        if right.head.is_utf8() {
                            let left = store[left];
                            let head = join_strings(
                                store,
                                &left,
                                right.head.cooked(),
                                matches!(l.data, Data::EInlinedEnum(_)),
                            );
                            return Some(Expr::init(
                                store,
                                E::Template { tag: None, parts: right.parts, head: e::TemplateContents::Cooked(head) },
                                l.loc,
                            ));
                        }
                    }
                    _ => {
                        // other constant-foldable ast nodes would have been converted to .e_string
                    }
                }
            }

            if store[left].len() == 0 && rhs.known_primitive(store) == PrimitiveType::String {
                return Some(rhs);
            }

            return None;
        }

        Data::ETemplate(left_ref) => {
            // "`${x}` + 0" => "`${x}` + '0'"
            if let Some(str) = rhs.to_string_expr_without_side_effects(store) {
                rhs = str;
            }

            let left = store[left_ref];
            if left.tag.is_none() {
                match rhs.data {
                    // `foo${bar}` + "baz" => `foo${bar}baz`
                    Data::EString(right) => {
                        let right = store[right];
                        if right.is_utf8() {
                            // Mutation of this node is fine because it will be not
                            // be shared by other places. Note that e_template will
                            // be treated by enums as strings, but will not be
                            // inlined unless they could be converted into
                            // .e_string.
                            if !left.parts.is_empty() {
                                let i = left.parts.len() - 1;
                                let last_tail = left.parts(store)[i].tail;
                                if last_tail.is_utf8() {
                                    let joined = join_strings(
                                        store,
                                        last_tail.cooked(),
                                        &right,
                                        matches!(r.data, Data::EInlinedEnum(_)),
                                    );
                                    left.parts_mut(store)[i].tail = e::TemplateContents::Cooked(joined);
                                    return Some(lhs);
                                }
                            } else if left.head.is_utf8() {
                                let new_head = join_strings(
                                    store,
                                    left.head.cooked(),
                                    &right,
                                    matches!(r.data, Data::EInlinedEnum(_)),
                                );
                                store[left_ref].head = e::TemplateContents::Cooked(new_head);
                                return Some(lhs);
                            }
                        }
                    }
                    // `foo${bar}` + `a${hi}b` => `foo${bar}a${hi}b`
                    Data::ETemplate(right) => {
                        let right = store[right];
                        if right.tag.is_none() && right.head.is_utf8() {
                            if !left.parts.is_empty() {
                                let i = left.parts.len() - 1;
                                let last_tail = left.parts(store)[i].tail;
                                if last_tail.is_utf8() && right.head.is_utf8() {
                                    let joined = join_strings(
                                        store,
                                        last_tail.cooked(),
                                        right.head.cooked(),
                                        matches!(r.data, Data::EInlinedEnum(_)),
                                    );
                                    left.parts_mut(store)[i].tail = e::TemplateContents::Cooked(joined);

                                    let new_parts = if right.parts.is_empty() {
                                        left.parts
                                    } else {
                                        concat_parts(store, left.parts, right.parts)
                                    };
                                    store[left_ref].parts = new_parts;
                                    return Some(lhs);
                                }
                            } else if left.head.is_utf8() && right.head.is_utf8() {
                                let new_head = join_strings(
                                    store,
                                    left.head.cooked(),
                                    right.head.cooked(),
                                    matches!(r.data, Data::EInlinedEnum(_)),
                                );
                                store[left_ref].head = e::TemplateContents::Cooked(new_head);
                                store[left_ref].parts = right.parts;
                                return Some(lhs);
                            }
                        }
                    }
                    _ => {
                        // other constant-foldable ast nodes would have been converted to .e_string
                    }
                }
            }
        }

        _ => {
            // other constant-foldable ast nodes would have been converted to .e_string
        }
    }

    if let Data::EString(right) = rhs.data {
        if store[right].len() == 0 && lhs.known_primitive(store) == PrimitiveType::String {
            return Some(lhs);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{FoldStringAdditionKind, fold_string_addition};
    use crate::expr::Data;
    use crate::new_store::NodeStore as _;
    use crate::store::Store;
    use crate::{E, Expr, Loc};

    fn string(store: &mut Store, text: &[u8]) -> Expr {
        let s = store.alloc_str(text);
        Expr::init(store, E::EString::init(s), Loc::EMPTY)
    }

    fn flat(store: &mut Store, expr: Expr) -> Vec<u8> {
        let Data::EString(s) = expr.data else { panic!("não é string") };
        let mut s = store[s];
        s.resolve_rope_if_needed(store);
        store.str(s.data).to_vec()
    }

    #[test]
    fn folds_strings_numbers_and_null() {
        let mut store = Store::default();
        let (a, b) = (string(&mut store, b"bar"), string(&mut store, b"baz"));
        let ab = fold_string_addition(a, b, &mut store, FoldStringAdditionKind::Normal).expect("dobra");
        let c = string(&mut store, b"!");
        let abc = fold_string_addition(ab, c, &mut store, FoldStringAdditionKind::Normal).expect("dobra");
        assert_eq!(flat(&mut store, abc), b"barbaz!");

        let one = Expr::init(&mut store, E::Number::new(1.0), Loc::EMPTY);
        let y = string(&mut store, b"y");
        let folded = fold_string_addition(one, y, &mut store, FoldStringAdditionKind::Normal).expect("dobra");
        assert_eq!(flat(&mut store, folded), b"1y");
        let one = Expr::init(&mut store, E::Number::new(1.0), Loc::EMPTY);
        let y = string(&mut store, b"y");
        assert!(fold_string_addition(one, y, &mut store, FoldStringAdditionKind::NestedLeft).is_none());

        let x = string(&mut store, b"x");
        let null = Expr::init(&mut store, E::Null {}, Loc::EMPTY);
        let folded = fold_string_addition(x, null, &mut store, FoldStringAdditionKind::Normal).expect("dobra");
        assert_eq!(flat(&mut store, folded), b"xnull");
    }
}
