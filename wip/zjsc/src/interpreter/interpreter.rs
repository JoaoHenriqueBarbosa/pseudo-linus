//! Trecho de `interpreter/Interpreter.h`: `DebugHookType` (Interpreter.h:96).

/// `enum DebugHookType` (valores sequenciais a partir de 0).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DebugHookType {
    WillExecuteProgram = 0,
    DidExecuteProgram = 1,
    DidEnterCallFrame = 2,
    DidReachDebuggerStatement = 3,
    WillLeaveCallFrame = 4,
    WillExecuteStatement = 5,
    WillExecuteExpression = 6,
    WillAwait = 7,
    DidAwait = 8,
}
