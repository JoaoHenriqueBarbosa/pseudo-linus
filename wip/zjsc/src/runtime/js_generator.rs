//! Tradução de `runtime/JSGenerator.h` (constantes e enums; o objeto vive na camada do heap).

/// `JSInternalFieldObjectImpl<4>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 4;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeMode {
    NormalMode = 0,
    ReturnMode = 1,
    ThrowMode = 2,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Completed = -1,
    Executing = -2,
    Init = 0,
}

/// `[this], @generator, @generatorState, @generatorValue, @generatorResumeMode, @generatorFrame.`
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argument {
    ThisValue = 0,
    Generator = 1,
    State = 2,
    Value = 3,
    ResumeMode = 4,
    Frame = 5,
}

impl Argument {
    /// `NumberOfArguments = Frame`.
    pub const NUMBER_OF_ARGUMENTS: usize = Argument::Frame as usize;
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Next = 1,
    This = 2,
    Frame = 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(Argument::NUMBER_OF_ARGUMENTS, 5);
        assert_eq!(Field::Frame as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
        assert_eq!(State::Executing as i32, -2);
    }
}
