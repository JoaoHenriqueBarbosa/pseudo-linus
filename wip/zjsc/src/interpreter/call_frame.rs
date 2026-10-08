//! Porte da parte pura de `interpreter/CallFrame.h`: `CallFrameSlot` e os cálculos de offset.
//!
//! Valores resolvidos para Linux x86_64: `CallerFrameAndPC::sizeInRegisters` = 2 (dois registros
//! de 8 bytes: `callerFrame` e `returnPC`), então `codeBlock` = 2, `callee` = 3,
//! `argumentCountIncludingThis` = 4, `thisArgument` = 5, `firstArgument` = 6.
//! O `CallFrame` de verdade (acesso aos registros na pilha do interpretador) entra com o
//! interpretador; aqui `CallFrame` só carrega as constantes estáticas.

/// `CallerFrameAndPC::sizeInRegisters`.
pub const CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS: i32 = 2;

/// `enum class CallFrameSlot`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallFrameSlot {
    CodeBlock = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS,
    Callee = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 1,
    ArgumentCountIncludingThis = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 2,
    ThisArgument = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 3,
    FirstArgument = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 4,
}

/// Os mesmos valores como `i32`, para as contas que o C++ faz com
/// `OVERLOAD_MATH_OPERATORS_FOR_ENUM_CLASS_WITH_INTEGRALS(CallFrameSlot)`.
impl CallFrameSlot {
    pub const CODE_BLOCK: i32 = CallFrameSlot::CodeBlock as i32;
    pub const CALLEE: i32 = CallFrameSlot::Callee as i32;
    pub const ARGUMENT_COUNT_INCLUDING_THIS: i32 = CallFrameSlot::ArgumentCountIncludingThis as i32;
    pub const THIS_ARGUMENT: i32 = CallFrameSlot::ThisArgument as i32;
    pub const FIRST_ARGUMENT: i32 = CallFrameSlot::FirstArgument as i32;
}

/// `CallFrame::headerSizeInRegisters`.
pub const HEADER_SIZE_IN_REGISTERS: i32 = CallFrameSlot::ArgumentCountIncludingThis as i32 + 1;

/// Namespace das constantes e funções estáticas de `CallFrame`.
pub struct CallFrame;

impl CallFrame {
    pub const HEADER_SIZE_IN_REGISTERS: i32 = HEADER_SIZE_IN_REGISTERS;
}

/// `CallFrame::argumentOffset`.
pub const fn argument_offset(argument: i32) -> i32 {
    CallFrameSlot::FIRST_ARGUMENT + argument
}

/// `CallFrame::argumentOffsetIncludingThis`.
pub const fn argument_offset_including_this(argument: i32) -> i32 {
    CallFrameSlot::THIS_ARGUMENT + argument
}

/// `CallFrame::thisArgumentOffset`.
pub const fn this_argument_offset() -> i32 {
    argument_offset_including_this(0)
}
