//! `bun_alloc::Arena` na parte que o lexer usa: cópias de fatias de bytes e de unidades UTF-16 que vivem tanto
//! quanto a arena. No upstream é o `MimallocArena` (um heap do mimalloc liberado de uma vez); aqui são duas
//! `typed_arena::Arena`, uma por tipo de elemento, que dão a mesma garantia (a fatia devolvida vive o mesmo que
//! `&Arena`) sem `unsafe` neste crate.

#![forbid(unsafe_code)]

use typed_arena::Arena as TypedArena;

#[derive(Default)]
pub struct Arena {
    bytes: TypedArena<u8>,
    units: TypedArena<u16>,
}

/// Os tipos de elemento que a arena guarda (`u8` e `u16`, os dois que o lexer copia).
pub trait ArenaItem: Copy + Default + Sized {
    fn arena(arena: &Arena) -> &TypedArena<Self>;
}

impl ArenaItem for u8 {
    #[inline]
    fn arena(arena: &Arena) -> &TypedArena<u8> {
        &arena.bytes
    }
}

impl ArenaItem for u16 {
    #[inline]
    fn arena(arena: &Arena) -> &TypedArena<u16> {
        &arena.units
    }
}

impl Arena {
    pub fn new() -> Self {
        Self::default()
    }

    /// Copia `src` para a arena.
    pub fn alloc_slice_copy<T: ArenaItem>(&self, src: &[T]) -> &mut [T] {
        T::arena(self).alloc_extend(src.iter().copied())
    }

    /// `len` elementos com o valor padrão.
    pub fn alloc_slice_fill_default<T: ArenaItem>(&self, len: usize) -> &mut [T] {
        T::arena(self).alloc_extend(core::iter::repeat_n(T::default(), len))
    }
}
