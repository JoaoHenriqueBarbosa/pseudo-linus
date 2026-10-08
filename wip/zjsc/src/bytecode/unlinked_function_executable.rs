//! Porte de `JavaScriptCore/bytecode/UnlinkedFunctionExecutable.h`: por ora só `ClassElementDefinition`
//! (struct aninhada no C++, com o enum `Kind`), que o parser e o bytecompiler já usam.

use crate::parser::parser_tokens::JSTextPosition;
use crate::runtime::identifier::Identifier;

/// `UnlinkedFunctionExecutable::ClassElementDefinition::Kind` (`ClassElementDefinitionKind` no porte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ClassElementDefinitionKind {
    #[default]
    FieldWithLiteralPropertyKey = 0,
    FieldWithComputedPropertyKey = 1,
    FieldWithPrivatePropertyKey = 2,
    StaticInitializationBlock = 3,
}

/// `UnlinkedFunctionExecutable::ClassElementDefinition`.
#[derive(Clone, Debug, Default)]
pub struct ClassElementDefinition {
    pub ident: Identifier,
    pub position: JSTextPosition,
    pub initializer_position: Option<JSTextPosition>,
    pub kind: ClassElementDefinitionKind,
}
