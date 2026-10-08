//! Tradução de `runtime/JSAsyncFunctionGenerator.h` (campos internos; o objeto vive na camada do heap).

/// `JSInternalFieldObjectImpl<5>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 5;

pub use crate::runtime::js_generator::{Argument, ResumeMode, State};

/// `enum class Field : uint32_t`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Next = 1,
    This = 2,
    Frame = 3,
    Context = 4,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_generator;

    #[test]
    fn values() {
        assert_eq!(Field::Context as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
        assert_eq!(Field::State as u32, js_generator::Field::State as u32);
        assert_eq!(Field::Next as u32, js_generator::Field::Next as u32);
        assert_eq!(Field::This as u32, js_generator::Field::This as u32);
        assert_eq!(Field::Frame as u32, js_generator::Field::Frame as u32);
    }
}
