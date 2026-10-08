//! Tradução das constantes de `runtime/JSArrayIterator.h`.

/// `JSInternalFieldObjectImpl<3>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 3;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Index = 0,
    IteratedObject = 1,
    Kind = 2,
}
