//! `Index` e `SymbolTable` de `base.rs` e `Symbol`, `Kind` e `Use` de `symbol.rs`.
use bun_ast::base::SymbolTable;
use bun_ast::symbol::{Kind, SlotNamespace, Use};
use bun_ast::{ImportItemStatus, Index, Ref, Symbol};

#[test]
fn index_constants() {
    assert_eq!(Index::default(), Index::INVALID);
    assert!(Index::RUNTIME.is_runtime());
    assert!(Index::INVALID.is_invalid());
    assert_eq!(Index::source(7usize).get(), 7);
    assert_eq!(Index::init(9u64).value(), 9);
}

#[test]
fn symbol_defaults_and_slots() {
    let mut s = Symbol::default();
    assert_eq!(s.kind, Kind::Other);
    assert_eq!(s.chunk_index(), None);
    assert_eq!(s.nested_scope_slot(), None);
    assert_eq!(s.import_item_status, ImportItemStatus::None);
    assert!(!s.has_link());
    assert_eq!(s.slot_namespace(), SlotNamespace::Default);
    s.kind = Kind::PrivateGet;
    assert_eq!(s.slot_namespace(), SlotNamespace::PrivateName);
    s.kind = Kind::Label;
    assert_eq!(s.slot_namespace(), SlotNamespace::Label);
    s.set_must_not_be_renamed(true);
    assert_eq!(s.slot_namespace(), SlotNamespace::MustNotBeRenamed);
    assert!(Kind::GeneratorOrAsyncFunction.is_function());
    assert_eq!(<&'static str>::from(Kind::TsNamespace), "ts_namespace");
}

#[test]
fn symbol_table_follows_inner_index() {
    let mut table = vec![Symbol::default(), Symbol::default()];
    let r = Ref::init(1, 0, false);
    table.get_symbol(r).use_count_estimate = 3;
    assert_eq!(table[1].use_count_estimate, 3);
    assert_eq!(table[0].use_count_estimate, 0);
}

#[test]
fn use_counts() {
    let mut u = Use::default();
    u.add_scoped(2);
    assert_eq!(u.count_estimate(), 2);
    assert!(!u.has_unscoped());
    u.merge(Use::unscoped(3));
    assert_eq!(u.count_estimate(), 5);
    assert!(u.has_unscoped());
    u.subtract(10);
    assert_eq!(u.count_estimate(), 0);
    assert!(u.has_unscoped());
}
