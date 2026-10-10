//! `Loc` e `Range` do `lib.rs` do upstream.
use bun_ast::{Loc, Range};

#[test]
fn loc_empty_and_index() {
    assert!(Loc::default().is_empty());
    assert_eq!(Loc::EMPTY.to_nullable(), None);
    assert_eq!(Loc { start: 5 }.to_nullable(), Some(Loc { start: 5 }));
    assert_eq!(Loc::EMPTY.i(), 0);
    assert_eq!(Loc { start: 7 }.to_usize(), 7);
}

#[test]
fn range_end_saturates() {
    assert!(Range::default().is_empty());
    assert!(Range::NONE.is_empty());
    let r = Range { loc: Loc { start: 3 }, len: 4 };
    assert_eq!(r.end(), Loc { start: 7 });
    assert_eq!(r.end_i(), 7);
    assert_eq!(Range { loc: Loc::EMPTY, len: 0 }.end_i(), 0);
}
