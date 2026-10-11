//! `Members`: a passagem do arranjo de 4 para o de 8 e daí para a tabela preserva os membros, e as buscas comparam o
//! nome pelo `Store`.

use bun_ast::scope::{Member, Members};
use bun_ast::store::Store;
use bun_ast::{Loc, NodeStore};

#[test]
fn members_grow_to_table() {
    let mut store = Store::default();
    let names: Vec<_> = (0..20).map(|i| store.alloc_str(format!("n{i}").as_bytes())).collect();
    let mut members = Members::default();
    for (i, &name) in names.iter().enumerate() {
        let hash = Members::hash(store.str(name));
        let entry = members.get_or_put_hashed(&store, hash, name);
        assert!(!entry.found_existing);
        *entry.value_ptr = Member { loc: Loc { start: i as i32 }, ..Member::EMPTY };
        assert_eq!(members.len(), i + 1);
    }
    for i in 0..20 {
        let name = format!("n{i}");
        assert_eq!(members.get(&store, name.as_bytes()).map(|m| m.loc.start), Some(i));
    }
    // Uma segunda faixa com o mesmo texto acha o membro existente.
    let again = store.alloc_str(b"n7");
    let hash = Members::hash(b"n7");
    assert!(members.get_or_put_hashed(&store, hash, again).found_existing);
    assert!(members.get(&store, b"n20").is_none());
    assert_eq!(members.iter().count(), 20);
}
