//! `src/ast/nodes.rs` do upstream, portado item a item na ordem em que os outros módulos pedem.
//!
//! `StoreRef`, `StoreSlice` e `StoreStr` mantêm os nomes do upstream, mas deixam de ser ponteiros: são índices no
//! `new_store::Store` que é dono dos nós (ver a seção "Arena tipada" do README de `wip/zbun`).

use core::marker::PhantomData;
use core::num::NonZeroU32;

// ───────────────────────────────────────────────────────────────────────────
// StoreRef: índice de um nó no vetor do seu tipo dentro do `Store`.
// ───────────────────────────────────────────────────────────────────────────

/// Um nó do tipo `T` guardado num `Store`. O índice é a posição mais um, para o nicho de `Option<StoreRef<T>>`.
/// A igualdade é a do índice, como a do upstream é a do endereço.
pub struct StoreRef<T> {
    index: NonZeroU32,
    _type: PhantomData<fn() -> T>,
}

const _: () = assert!(core::mem::size_of::<StoreRef<u8>>() == 4);
const _: () = assert!(core::mem::size_of::<Option<StoreRef<u8>>>() == 4);

impl<T> StoreRef<T> {
    /// O nó na posição `position` do vetor do tipo.
    #[inline]
    pub const fn from_position(position: u32) -> Self {
        match NonZeroU32::new(position.wrapping_add(1)) {
            Some(index) => StoreRef { index, _type: PhantomData },
            None => panic!("StoreRef: posição fora do alcance de u32"),
        }
    }

    /// A posição no vetor do tipo.
    #[inline]
    pub const fn position(self) -> usize {
        (self.index.get() - 1) as usize
    }
}

impl<T> Clone for StoreRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for StoreRef<T> {}
impl<T> PartialEq for StoreRef<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for StoreRef<T> {}
impl<T> core::hash::Hash for StoreRef<T> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state)
    }
}
impl<T> core::fmt::Debug for StoreRef<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "StoreRef({})", self.position())
    }
}

// ───────────────────────────────────────────────────────────────────────────
// StoreSlice: faixa contígua do vetor do tipo.
// ───────────────────────────────────────────────────────────────────────────

/// Os elementos `start..start + len` do vetor do tipo `T` no `Store`.
pub struct StoreSlice<T> {
    start: u32,
    len: u32,
    _type: PhantomData<fn() -> T>,
}

const _: () = assert!(core::mem::size_of::<StoreSlice<u8>>() == 8);

impl<T> StoreSlice<T> {
    pub const EMPTY: StoreSlice<T> = StoreSlice { start: 0, len: 0, _type: PhantomData };

    #[inline]
    pub(crate) const fn from_range(start: u32, len: u32) -> Self {
        StoreSlice { start, len, _type: PhantomData }
    }

    #[inline]
    pub const fn len(self) -> usize {
        self.len as usize
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// A faixa no vetor do tipo. Pública porque o `define_store!` expande em outros crates.
    #[doc(hidden)]
    #[inline]
    pub const fn range(self) -> core::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }

    /// Encurta a faixa. Os elementos de trás continuam no `Store`, só deixam de ser alcançados por esta faixa.
    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        assert!(new_len <= self.len as usize);
        self.len = new_len as u32;
    }
}

impl<T> Clone for StoreSlice<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for StoreSlice<T> {}
impl<T> Default for StoreSlice<T> {
    #[inline]
    fn default() -> Self {
        StoreSlice::EMPTY
    }
}
impl<T> core::fmt::Debug for StoreSlice<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "StoreSlice({}..{})", self.start, self.start + self.len)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// StoreStr: faixa do vetor de bytes do `Store`.
// ───────────────────────────────────────────────────────────────────────────

/// Os bytes `start..start + len` do vetor de strings do `Store`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Hash)]
pub struct StoreStr {
    start: u32,
    len: u32,
}

impl StoreStr {
    pub const EMPTY: StoreStr = StoreStr { start: 0, len: 0 };

    #[inline]
    pub(crate) const fn from_range(start: u32, len: u32) -> Self {
        StoreStr { start, len }
    }

    #[inline]
    pub const fn len(self) -> usize {
        self.len as usize
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// A faixa no vetor de bytes.
    #[inline]
    pub(crate) const fn range(self) -> core::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ImportItemStatus {
    None,
    /// The linker doesn't report import/export mismatch errors
    Generated,
    /// The printer will replace this import with "undefined"
    Missing,
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum OptionalChain {
    /// "a?.b"
    Start,

    /// "a?.b.c" => ".c" is OptionalChain::Continuation
    /// "(a?.b).c" => ".c" is None
    Continuation,
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AssignTarget {
    #[default]
    None = 0,
    /// "a = b"
    Replace = 1,
    /// "a += b"
    Update = 2,
}
