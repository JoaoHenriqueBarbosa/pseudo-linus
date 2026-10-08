//! Tradução das constantes de `runtime/JSAsyncDisposableStack.h`.

/// `JSAsyncDisposableStackNumberOfInternalFields`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Capability = 1,
}
