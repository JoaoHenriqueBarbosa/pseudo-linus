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

    /// Copia `s` para a arena como texto.
    pub fn alloc_str(&self, s: &str) -> &str {
        match core::str::from_utf8(self.alloc_slice_copy(s.as_bytes())) {
            Ok(s) => s,
            Err(_) => unreachable!("cópia byte a byte de um str"),
        }
    }
}

// Per PORTING.md type map: `OOM!T` / `error{OutOfMemory}!T` → `Result<T, bun_alloc::AllocError>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocError;

impl core::fmt::Display for AllocError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("OutOfMemory")
    }
}
impl core::error::Error for AllocError {}

/// `ArenaString`: o texto que `arena_format!` monta. Cresce num `String` e só vai para a arena no
/// `into_bump_str`.
pub struct ArenaString<'a> {
    arena: &'a Arena,
    buf: String,
}

impl<'a> ArenaString<'a> {
    pub fn new_in(arena: &'a Arena) -> Self {
        Self { arena, buf: String::new() }
    }

    /// `bumpalo::collections::String::into_bump_str` parity.
    pub fn into_bump_str(self) -> &'a str {
        self.arena.alloc_str(&self.buf)
    }
}

impl core::fmt::Write for ArenaString<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.buf.push_str(s);
        Ok(())
    }
}

impl core::ops::Deref for ArenaString<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.buf
    }
}

/// `bumpalo::format!` parity: `arena_format!(in arena, "...", ..)` →
/// [`ArenaString`].
#[macro_export]
macro_rules! arena_format {
    (in $arena:expr, $($arg:tt)*) => {{
        let mut __s = $crate::ArenaString::new_in($arena);
        ::core::fmt::Write::write_fmt(&mut __s, ::core::format_args!($($arg)*))
            .expect("ArenaString::write_fmt is infallible");
        __s
    }};
}

/// `ArenaVec`: no upstream um `Vec` cujo buffer vem da arena. Aqui o buffer é um `Vec` comum e a arena fica
/// guardada para o tempo de vida e para o `bump()`. As listas de nós não passam pela arena: o porte as copia
/// em bloco para o `Store` (`store.alloc_slice`, seção "Arena tipada" do README); só as de `u8` e `u16`
/// vazam para a arena com `into_bump_slice`.
pub struct ArenaVec<'a, T> {
    arena: &'a Arena,
    vec: Vec<T>,
}

impl<'a, T> ArenaVec<'a, T> {
    pub fn new_in(arena: &'a Arena) -> Self {
        Self { arena, vec: Vec::new() }
    }

    pub fn with_capacity_in(capacity: usize, arena: &'a Arena) -> Self {
        Self { arena, vec: Vec::with_capacity(capacity) }
    }

    /// O `Vec` de dentro, para quem copia a lista para o `Store`.
    pub fn into_vec(self) -> Vec<T> {
        self.vec
    }
}

impl<T> core::ops::Deref for ArenaVec<'_, T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.vec
    }
}

impl<T> core::ops::DerefMut for ArenaVec<'_, T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.vec
    }
}

impl<T> Extend<T> for ArenaVec<'_, T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.vec.extend(iter);
    }
}

impl<T> IntoIterator for ArenaVec<'_, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.vec.into_iter()
    }
}

/// `bumpalo::collections::Vec::from_iter_in` parity for [`ArenaVec`].
pub fn vec_from_iter_in<'a, T, I>(iter: I, arena: &'a Arena) -> ArenaVec<'a, T>
where
    I: IntoIterator<Item = T>,
{
    let iter = iter.into_iter();
    let (lo, _) = iter.size_hint();
    let mut v = ArenaVec::with_capacity_in(lo, arena);
    v.extend(iter);
    v
}

/// Extension methods to cover the `bumpalo::collections::Vec` API gaps.
pub trait ArenaVecExt<'a, T> {
    /// `bumpalo::collections::Vec::into_bump_slice` parity.
    fn into_bump_slice(self) -> &'a [T];
    /// `bumpalo::collections::Vec::into_bump_slice_mut` parity.
    fn into_bump_slice_mut(self) -> &'a mut [T];
    /// `bumpalo::collections::Vec::bump` parity: recover the backing arena.
    fn bump(&self) -> &'a Arena;
}

impl<'a, T: ArenaItem> ArenaVecExt<'a, T> for ArenaVec<'a, T> {
    fn into_bump_slice(self) -> &'a [T] {
        self.into_bump_slice_mut()
    }
    fn into_bump_slice_mut(self) -> &'a mut [T] {
        self.arena.alloc_slice_copy(&self.vec)
    }
    fn bump(&self) -> &'a Arena {
        self.arena
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_vec_and_format() {
        let arena = Arena::new();
        let mut v = ArenaVec::<u8>::new_in(&arena);
        v.extend_from_slice(b"ab");
        v.push(b'c');
        assert!(core::ptr::eq(v.bump(), &arena));
        assert_eq!(v.into_bump_slice(), b"abc");
        let nodes = vec_from_iter_in([1u32, 2, 3], &arena);
        assert_eq!(nodes.into_vec(), [1, 2, 3]);
        let name = arena_format!(in &arena, "__bun_temp_ref_{:x}$", 255).into_bump_str();
        assert_eq!(name, "__bun_temp_ref_ff$");
        assert_eq!(AllocError.to_string(), "OutOfMemory");
    }
}
