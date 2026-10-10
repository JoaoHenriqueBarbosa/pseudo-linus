use core::marker::PhantomData;

/// Trait for keys usable with `IdentityContext`.
/// Implement for any int (`self as u64`) or `#[repr(uN)]` enum (`self as uN as u64`).
pub trait IdentityHash: Copy + Eq {
    fn identity_hash(self) -> u64;
}

macro_rules! identity_hash_int {
    ($($t:ty),*) => { $(
        impl IdentityHash for $t {
            #[inline]
            fn identity_hash(self) -> u64 { self as u64 }
        }
    )* };
}
identity_hash_int!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

#[derive(Default, Clone, Copy)]
pub struct IdentityContext<Key>(PhantomData<Key>);
