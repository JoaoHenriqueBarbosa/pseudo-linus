//! A fidelidade do `with_node!` e do `sync_node!` (README, "Regras do porte"). No upstream a função visitava o nó
//! por um ponteiro para dentro da arena, e quem recebia o mesmo `Stmt` lia o nó pelo mesmo ponteiro, então via as
//! mudanças em andamento. No porte o `with_node!` trabalha numa cópia: uma função que relê o nó pelo `StoreRef`
//! (o `lower_class` no `s_class`) vê o nó velho. O `sync_node!` grava a cópia antes e relê depois.

use bun_ast::store::Store;
use bun_ast::{NodeStore as _, S};

/// O `P` de brinquedo: só o `store`, que é tudo o que as duas macros tocam.
struct Fake {
    store: Store,
}

fn class_node(store: &mut Store, is_export: bool) -> bun_ast::StoreRef<S::Class> {
    store.alloc(S::Class { class: Default::default(), is_export })
}

/// Faz o papel do `lower_class`: recebe só o `StoreRef`, lê o nó pelo `Store` e o muda.
fn relayer(p: &mut Fake, node: bun_ast::StoreRef<S::Class>) -> bool {
    let seen = p.store[node].is_export;
    p.store[node].class.has_decorators = true;
    seen
}

#[test]
fn a_function_that_rereads_the_node_sees_the_stale_copy_without_sync() {
    let mut p = Fake { store: Store::default() };
    let node = class_node(&mut p.store, false);
    let seen = with_node!(p, node, data => {
        data.is_export = true;
        relayer(&mut p, node)
    });
    // O que o upstream faria: ver `true`. Sem o `sync_node!` a função vê `false`, e a escrita dela se perde.
    assert!(!seen);
    assert!(!p.store[node].class.has_decorators, "a escrita do relayer foi sobrescrita pela cópia");
    assert!(p.store[node].is_export);
}

#[test]
fn sync_node_makes_both_views_agree_in_both_directions() {
    let mut p = Fake { store: Store::default() };
    let node = class_node(&mut p.store, false);
    let (seen, decorated_in_copy) = with_node!(p, node, data => {
        data.is_export = true;
        let seen = sync_node!(p, node, data => relayer(&mut p, node));
        (seen, data.class.has_decorators)
    });
    assert!(seen, "a função que relê o nó vê a mudança em andamento");
    assert!(decorated_in_copy, "e quem segurava a cópia vê a mudança da função");
    assert!(p.store[node].is_export && p.store[node].class.has_decorators, "nada se perde na gravação final");
}

#[test]
fn nodes_outside_the_borrowed_one_are_untouched() {
    let mut p = Fake { store: Store::default() };
    let first = class_node(&mut p.store, false);
    let second = class_node(&mut p.store, false);
    with_node!(p, first, data => {
        data.is_export = true;
        sync_node!(p, first, data => relayer(&mut p, first));
    });
    assert!(p.store[first].is_export);
    assert!(!p.store[second].is_export && !p.store[second].class.has_decorators);
}
