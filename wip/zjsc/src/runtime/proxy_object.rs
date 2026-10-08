//! Tradução das constantes de `runtime/ProxyObject.h`.

/// `JSInternalFieldObjectImpl<2>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Target = 0,
    Handler = 1,
}
