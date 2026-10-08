//! Tradução de `runtime/AbstractModuleRecord.h` (campos internos; o objeto vive na camada do heap).

/// `JSInternalFieldObjectImpl<2>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

pub use crate::runtime::js_generator::{Argument, ResumeMode, State};

/// `enum class Field : uint32_t`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Frame = 1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(Field::Frame as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
    }
}
