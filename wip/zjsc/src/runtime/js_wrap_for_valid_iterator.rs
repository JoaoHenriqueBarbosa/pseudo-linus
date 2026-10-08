//! Tradução das constantes de `runtime/JSWrapForValidIterator.h`.

/// `JSWrapForValidIteratorNumberOfInternalFields`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    IteratedIterator = 0,
    IteratedNextMethod = 1,
}
