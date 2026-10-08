//! Tradução de `runtime/JSAsyncGenerator.h` (constantes e enums; o objeto vive na camada do heap).

use crate::runtime::js_generator;

/// `JSInternalFieldObjectImpl<10>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 10;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncGeneratorState {
    Completed = -1,
    Executing = -2,
    Init = 0,
    DrainingQueue = -3,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncGeneratorSuspendReason {
    Await = 0,
    Yield = 1,
    /// `yield*`: entrega o valor sem `Await` envolvente.
    YieldNoAwait = 2,
}

pub const REASON_MASK: i32 = 0x3;
pub const REASON_SHIFT: i32 = 2;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncGeneratorResumeMode {
    Empty = -1,
    Normal = 0,
    Return = 1,
    Throw = 2,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Next = 1,
    This = 2,
    Frame = 3,
    Queue = 4,
    ResumeValue = 5,
    ResumeMode = 6,
    ResumePromise = 7,
    CachedDriverResult = 8,
    CachedDriverResultTarget = 9,
}

/// `static bool isSuspendedYieldState(int32_t)`: estado positivo cujos bits de razão são `Yield`.
pub fn is_suspended_yield_state(state: i32) -> bool {
    state > 0 && (state & REASON_MASK) == AsyncGeneratorSuspendReason::Yield as i32
}

/// `static bool isExecutingState(int32_t)`.
pub fn is_executing_state(state: i32) -> bool {
    if state == AsyncGeneratorState::Executing as i32 {
        return true;
    }
    state > 0 && (state & REASON_MASK) == AsyncGeneratorSuspendReason::Await as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_generator() {
        assert_eq!(AsyncGeneratorState::Completed as i32, js_generator::State::Completed as i32);
        assert_eq!(AsyncGeneratorState::Executing as i32, js_generator::State::Executing as i32);
        assert_eq!(AsyncGeneratorState::Init as i32, js_generator::State::Init as i32);
        assert_eq!(AsyncGeneratorResumeMode::Normal as i32, js_generator::ResumeMode::NormalMode as i32);
        assert_eq!(AsyncGeneratorResumeMode::Return as i32, js_generator::ResumeMode::ReturnMode as i32);
        assert_eq!(AsyncGeneratorResumeMode::Throw as i32, js_generator::ResumeMode::ThrowMode as i32);
        assert_eq!(Field::CachedDriverResultTarget as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
        assert!(is_executing_state(-2));
        assert!(is_executing_state(4));
        assert!(is_suspended_yield_state(5));
        assert!(!is_suspended_yield_state(4));
    }
}
