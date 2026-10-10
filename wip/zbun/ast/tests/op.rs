//! Tabela de operadores: o texto, a precedência e a associatividade de cada código, como o bun 1.4.2 imprime.
use bun_ast::op::{Code, Level, TABLE};
use bun_ast::AssignTarget;

#[test]
fn operator_text_and_level_match_upstream() {
    let op = TABLE.get_ptr_const(Code::BinNullishCoalescingAssign);
    assert_eq!((op.text, op.level as u8), (&b"??="[..], Level::Assign as u8));
    let op = TABLE.get_ptr_const(Code::UnTypeof);
    assert!(op.is_keyword);
    assert_eq!(op.text, b"typeof");
}

#[test]
fn associativity_and_assign_targets() {
    assert!(Code::is_right_associative(Code::BinPow));
    assert!(!Code::is_left_associative(Code::BinPow));
    assert_eq!(Code::BinAssign.binary_assign_target(), AssignTarget::Replace);
    assert_eq!(Code::BinAddAssign.binary_assign_target(), AssignTarget::Update);
    assert_eq!(Code::unary_assign_target(Code::UnPostInc), AssignTarget::Update);
    assert!(Level::Member.gt(Level::Call));
}
