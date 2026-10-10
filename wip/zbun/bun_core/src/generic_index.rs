//! `GenericIndex` e `GenericIndexOptional` de `src/bun_core/util.rs` do upstream.

// ── GenericIndex ──────────────────────────────────────────────────────────
// Nominally-typed index newtype; the phantom
// marker keeps indexes for different containers distinct.
// `MAX` is reserved as the "none" sentinel for `Optional`.
#[repr(transparent)]
pub struct GenericIndex<I, M = ()>(I, core::marker::PhantomData<M>);

impl<I: Copy, M> Clone for GenericIndex<I, M> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<I: Copy, M> Copy for GenericIndex<I, M> {}
impl<I: PartialEq, M> PartialEq for GenericIndex<I, M> {
    #[inline]
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<I: Eq, M> Eq for GenericIndex<I, M> {}
impl<I: core::hash::Hash, M> core::hash::Hash for GenericIndex<I, M> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}
impl<I: core::fmt::Display, M> core::fmt::Display for GenericIndex<I, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}
impl<I: core::fmt::Debug, M> core::fmt::Debug for GenericIndex<I, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}
/// `Default` = index 0 (matches the hand-rolled `#[derive(Default)]` newtypes
/// this replaced). NOT the `Optional::none` sentinel.
impl<I: Default, M> Default for GenericIndex<I, M> {
    #[inline]
    fn default() -> Self {
        Self(I::default(), core::marker::PhantomData)
    }
}

impl<I: GenericIndexInt, M> GenericIndex<I, M> {
    /// Prefer over a raw cast: asserts `int != MAX` (would alias `.none`).
    #[inline]
    pub fn init(int: I) -> Self {
        debug_assert!(int != I::NULL_VALUE, "GenericIndex::init: maxInt is reserved for Optional::none");
        Self(int, core::marker::PhantomData)
    }
    #[inline]
    pub fn get(self) -> I {
        debug_assert!(self.0 != I::NULL_VALUE, "GenericIndex::get: corrupted (== none sentinel)");
        self.0
    }
    /// `get()` widened to `usize` for slice indexing.
    #[inline]
    pub fn get_usize(self) -> usize {
        I::to_usize(self.get())
    }
    #[inline]
    pub fn to_optional(self) -> GenericIndexOptional<I, M> {
        GenericIndexOptional(self.0, core::marker::PhantomData)
    }
}

/// `GenericIndex::Optional`: `MAX` is `none`.
#[repr(transparent)]
pub struct GenericIndexOptional<I, M = ()>(I, core::marker::PhantomData<M>);
impl<I: Copy, M> Clone for GenericIndexOptional<I, M> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<I: Copy, M> Copy for GenericIndexOptional<I, M> {}
impl<I: PartialEq, M> PartialEq for GenericIndexOptional<I, M> {
    #[inline]
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<I: Eq, M> Eq for GenericIndexOptional<I, M> {}
impl<I: core::fmt::Debug, M> core::fmt::Debug for GenericIndexOptional<I, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}
impl<I: GenericIndexInt, M> GenericIndexOptional<I, M> {
    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == I::NULL_VALUE
    }

    #[inline]
    pub fn is_some(self) -> bool {
        !self.is_none()
    }

    pub const NONE: Self = Self(I::NULL_VALUE, core::marker::PhantomData);

    #[inline]
    pub fn get(self) -> Option<GenericIndex<I, M>> {
        self.unwrap()
    }
    #[inline]
    pub fn unwrap(self) -> Option<GenericIndex<I, M>> {
        if self.0 == I::NULL_VALUE { None } else { Some(GenericIndex(self.0, core::marker::PhantomData)) }
    }
}

/// Backing-integer bound for `GenericIndex`.
pub trait GenericIndexInt: Copy + Eq + PartialOrd {
    const NULL_VALUE: Self;
    fn to_usize(self) -> usize;
}
macro_rules! generic_index_int { ($($t:ty),*) => { $(
    impl GenericIndexInt for $t {
        const NULL_VALUE: Self = <$t>::MAX;
        #[inline] fn to_usize(self) -> usize { self as usize }
    }
)* } }
generic_index_int!(u8, u16, u32, u64, usize, i32, i64);
