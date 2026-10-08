//! Tradução das constantes de `runtime/JSIteratorHelper.h`.

/// `JSInternalFieldObjectImpl<2>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Generator = 0,
    UnderlyingIterator = 1,
}
