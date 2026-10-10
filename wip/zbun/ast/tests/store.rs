//! A arena tipada que substitui os ponteiros do upstream: índice, faixa, cópia do `clone_in` e o padrão de escrita
//! da visita (copiar o filho, visitar com `&mut` no dono, devolver).
use bun_ast::{NodeStore, StoreRef, StoreSlice, StoreStr};

#[derive(Clone, Debug, PartialEq)]
struct Binary {
    left: Expr,
    right: Expr,
}

#[derive(Clone, Debug)]
struct Array {
    items: StoreSlice<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expr {
    Number(i32),
    Binary(StoreRef<Binary>),
    Array(StoreRef<Array>),
}

bun_ast::define_store! {
    struct Store {
        binaries: Binary,
        arrays: Array,
        exprs: Expr,
    }
}

#[test]
fn option_keeps_the_niche() {
    assert_eq!(core::mem::size_of::<Option<StoreRef<Binary>>>(), 4);
    assert_eq!(core::mem::size_of::<StoreSlice<Expr>>(), 8);
}

#[test]
fn alloc_and_index() {
    let mut store = Store::default();
    let a = store.alloc(Binary { left: Expr::Number(1), right: Expr::Number(2) });
    let b = store.alloc(Binary { left: Expr::Number(3), right: Expr::Number(4) });
    assert_ne!(a, b);
    assert_eq!(store[a].right, Expr::Number(2));
    store[b].left = Expr::Binary(a);
    assert_eq!(store[b].left, Expr::Binary(a));
}

#[test]
fn slices_are_contiguous_and_mutable() {
    let mut store = Store::default();
    let first = store.alloc_slice([Expr::Number(1), Expr::Number(2)]);
    let mut second = store.alloc_slice([Expr::Number(3), Expr::Number(4), Expr::Number(5)]);
    assert_eq!(store[first], [Expr::Number(1), Expr::Number(2)]);
    store[second][0] = Expr::Number(30);
    assert_eq!(store[second], [Expr::Number(30), Expr::Number(4), Expr::Number(5)]);
    second.truncate(1);
    assert_eq!(store[second], [Expr::Number(30)]);
    assert!(StoreSlice::<Expr>::EMPTY.is_empty());
    assert_eq!(store[StoreSlice::<Expr>::EMPTY], []);
}

#[test]
fn clone_in_shares_the_inner_slice_like_upstream() {
    let mut store = Store::default();
    let items = store.alloc_slice([Expr::Number(1), Expr::Number(2)]);
    let original = store.alloc(Array { items });
    let copy = store.clone_in(original);
    assert_ne!(original, copy);
    // O nó é outro; a faixa é a mesma, como os vetores compartilhados do upstream.
    let copied_items = store[copy].items;
    store[copied_items][1] = Expr::Number(20);
    assert_eq!(store[store[original].items], [Expr::Number(1), Expr::Number(20)]);
    // Mudar o nó copiado não mexe no original.
    store[copy].items = StoreSlice::EMPTY;
    assert_eq!(store[original].items.len(), 2);
}

/// Soma os números de uma árvore, trocando cada binário por um número no lugar: o filho é copiado para fora, visitado
/// com `&mut` no dono e devolvido.
fn fold(store: &mut Store, expr: Expr) -> Expr {
    match expr {
        Expr::Number(_) => expr,
        Expr::Binary(node) => {
            let left = store[node].left;
            let left = fold(store, left);
            store[node].left = left;
            let right = store[node].right;
            let right = fold(store, right);
            store[node].right = right;
            match (store[node].left, store[node].right) {
                (Expr::Number(a), Expr::Number(b)) => Expr::Number(a + b),
                _ => expr,
            }
        }
        Expr::Array(node) => {
            let items = store[node].items;
            for i in 0..items.len() {
                let item = store[items][i];
                let item = fold(store, item);
                store[items][i] = item;
            }
            expr
        }
    }
}

#[test]
fn visiting_writes_back_through_the_owner() {
    let mut store = Store::default();
    let inner = store.alloc(Binary { left: Expr::Number(1), right: Expr::Number(2) });
    let outer = store.alloc(Binary { left: Expr::Binary(inner), right: Expr::Number(3) });
    let items = store.alloc_slice([Expr::Binary(outer), Expr::Number(7)]);
    let array = store.alloc(Array { items });
    fold(&mut store, Expr::Array(array));
    assert_eq!(store[items], [Expr::Number(6), Expr::Number(7)]);
    assert_eq!(store[outer].left, Expr::Number(3));
}

#[test]
fn strings_are_ranges_of_the_byte_vector() {
    let mut store = Store::default();
    let source = store.alloc_str(b"let x = 1;");
    let name = store.alloc_str(b"default");
    assert_eq!(store.str(source), b"let x = 1;");
    assert_eq!(store.str(name), b"default");
    assert_eq!(store.str(StoreStr::EMPTY), b"");
}
