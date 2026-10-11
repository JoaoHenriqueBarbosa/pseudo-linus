//! `Ref` empacotado em 64 bits como no bun 1.4.2: índice de 28 bits, três bits de usuário fora da identidade,
//! etiqueta de 2 bits e índice de fonte de 31 bits.
use bun_ast::{Ref, RefTag};

#[test]
fn ref_packs_fields_like_upstream() {
    let r = Ref::new(7, 3, RefTag::Symbol);
    assert_eq!((r.inner_index(), r.source_index(), r.tag()), (7, 3, RefTag::Symbol));
    assert!(r.is_symbol() && r.is_valid() && !r.is_empty());
    assert_eq!(r.to_raw_bits(), 7 | (3 << 31) | (3 << 33));
    assert!(Ref::NONE.is_empty());
    assert_eq!(Ref::init(1, 0, true).tag(), RefTag::SourceContentsSlice);
    assert_eq!(core::mem::size_of::<Ref>(), 8);
}
