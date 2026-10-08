//! Tradução da parte pura de `runtime/Error.cpp`/`Error.h` (`addErrorInfo(VM&, JSObject*, int,
//! const SourceCode&)`, `createSyntaxError`, `createRangeError`, `createOutOfMemoryError`,
//! `createStackOverflowError`) e de `parser/ParserError.h::toErrorObject`.
//!
//! Divergência documentada: o C++ cria um `ErrorInstance` (um `JSCell` no heap, com `Structure` do
//! `JSGlobalObject`) e grava `line`/`column`/`sourceURL` por `putDirect`/`setLine`. Aqui o
//! `ErrorInstance` é um struct de valor, sem célula e sem heap, que guarda o contrato de dados
//! observável (tipo, mensagem, linha, coluna, `sourceURL`, pilha opcional e as flags `setParseError`,
//! `setOutOfMemoryError`, `setStackOverflowError`). A camada com heap materializa a célula a partir
//! dele. O `ErrorHandlingScope` do ramo `StackOverflow` só afeta o limite de pilha do VM e não tem
//! efeito sobre o dado, então não aparece. Como `source.provider()` nulo derrefenciaria um nulo no
//! C++, aqui vale `sourceURL` vazio.

use crate::parser::parser_error::{ErrorType as ParserErrorType, ParserError};
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProvider;
use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::wtf::text::wtf_string::String as WtfString;

/// `ErrorInstance` reduzido ao dado observável de um erro de parse.
#[derive(Clone, Debug, Default)]
pub struct ErrorInstance {
    /// `errorType()`: `None` só no `Default`.
    pub error_type: Option<ErrorType>,
    pub message: WtfString,
    /// `line()`; `-1` quando não definida.
    pub line: i32,
    pub column: i32,
    pub source_url: WtfString,
    pub stack: Option<WtfString>,
    pub is_parse_error: bool,
    pub is_out_of_memory_error: bool,
    pub is_stack_overflow_error: bool,
}

impl ErrorInstance {
    /// `ErrorInstance::create(vm, structure, message, ..., errorType)` sem heap.
    pub fn create(error_type: ErrorType, message: WtfString) -> ErrorInstance {
        ErrorInstance { error_type: Some(error_type), message, line: -1, ..ErrorInstance::default() }
    }

    /// O `name` da protótipo do tipo (`errorTypeName`).
    pub fn name(&self) -> &'static str {
        match self.error_type {
            Some(error_type) => error_type_name(error_type),
            None => "",
        }
    }
}

/// `createSyntaxError(globalObject, message)`.
pub fn create_syntax_error(message: &WtfString) -> ErrorInstance {
    ErrorInstance::create(ErrorType::SyntaxError, message.clone())
}

/// `createRangeError(globalObject, message)`.
pub fn create_range_error(message: &str) -> ErrorInstance {
    ErrorInstance::create(ErrorType::RangeError, WtfString::from_latin1(message.as_bytes()))
}

/// `createOutOfMemoryError(globalObject)`.
pub fn create_out_of_memory_error() -> ErrorInstance {
    let mut error = create_range_error("Out of memory");
    error.is_out_of_memory_error = true;
    error
}

/// `createStackOverflowError(globalObject)`.
pub fn create_stack_overflow_error() -> ErrorInstance {
    let mut error = create_range_error("Maximum call stack size exceeded.");
    error.is_stack_overflow_error = true;
    error
}

/// `addErrorInfo(VM&, JSObject*, int line, const SourceCode&)`, ramo `USE(BUN_JSC_ADDITIONS)`.
pub fn add_error_info(mut error: ErrorInstance, line: i32, source: &SourceCode) -> ErrorInstance {
    let source_url = match source.provider() {
        Some(provider) => provider.source_url().clone(),
        None => WtfString::default(),
    };
    if line != -1 {
        error.line = line;
        error.column = 0;
    }
    if !source_url.is_empty() {
        error.source_url = source_url;
    }
    error
}

impl ParserError {
    /// `ParserError::toErrorObject(globalObject, source, overrideLineNumber)`. `None` é o
    /// `nullptr` do caso `ErrorNone`.
    pub fn to_error_object_override_line(&self, source: &SourceCode, override_line_number: i32) -> Option<ErrorInstance> {
        let mut error = match self.type_() {
            ParserErrorType::ErrorNone => return None,
            ParserErrorType::SyntaxError => add_error_info(
                create_syntax_error(self.message()),
                if override_line_number == -1 { self.line() } else { override_line_number },
                source,
            ),
            ParserErrorType::EvalError => create_syntax_error(self.message()),
            ParserErrorType::StackOverflow => create_stack_overflow_error(),
            ParserErrorType::OutOfMemory => create_out_of_memory_error(),
        };
        error.is_parse_error = true;
        Some(error)
    }

    /// `toErrorObject(globalObject, source)` com `overrideLineNumber = -1`.
    pub fn to_error_object(&self, source: &SourceCode) -> Option<ErrorInstance> {
        self.to_error_object_override_line(source, -1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parser_error::SyntaxErrorType;
    use crate::parser::parser_tokens::JSToken;
    use crate::parser::source_provider::{SourceProviderSourceType, StringSourceProvider};
    use crate::parser::source_tainted_origin::SourceTaintedOrigin;
    use crate::runtime::source_origin::SourceOrigin;
    use crate::wtf::text::text_position::TextPosition;

    fn source(url: &str) -> SourceCode {
        SourceCode::from_provider(StringSourceProvider::create(
            &WtfString::from_latin1(b"let x"),
            &SourceOrigin::default(),
            WtfString::from_latin1(url.as_bytes()),
            SourceTaintedOrigin::Untainted,
            TextPosition::default(),
            SourceProviderSourceType::Program,
        ))
    }

    fn syntax(line: i32) -> ParserError {
        ParserError::with_message(
            ParserErrorType::SyntaxError,
            SyntaxErrorType::SyntaxErrorIrrecoverable,
            JSToken::default(),
            &WtfString::from_latin1(b"Unexpected token"),
            line,
        )
    }

    #[test]
    fn syntax_error_carries_line_column_and_url() {
        let error = syntax(3).to_error_object(&source("a.js")).unwrap();
        assert_eq!(error.name(), "SyntaxError");
        assert_eq!((error.line, error.column), (3, 0));
        assert_eq!(error.source_url, WtfString::from_latin1(b"a.js"));
        assert!(error.is_parse_error);
        assert_eq!(syntax(3).to_error_object_override_line(&source(""), 9).unwrap().line, 9);
        assert!(syntax(3).to_error_object(&source("")).unwrap().source_url.is_empty());
    }

    #[test]
    fn eval_stack_overflow_and_oom() {
        let src = source("a.js");
        let eval = ParserError::with_message(
            ParserErrorType::EvalError,
            SyntaxErrorType::SyntaxErrorNone,
            JSToken::default(),
            &WtfString::from_latin1(b"m"),
            5,
        );
        let e = eval.to_error_object(&src).unwrap();
        assert_eq!((e.name(), e.line), ("SyntaxError", -1));
        let so = ParserError::with_type(ParserErrorType::StackOverflow).to_error_object(&src).unwrap();
        assert_eq!((so.name(), so.is_stack_overflow_error), ("RangeError", true));
        assert_eq!(so.message, WtfString::from_latin1(b"Maximum call stack size exceeded."));
        let oom = ParserError::with_type(ParserErrorType::OutOfMemory).to_error_object(&src).unwrap();
        assert!(oom.is_out_of_memory_error && oom.is_parse_error);
        assert_eq!(oom.message, WtfString::from_latin1(b"Out of memory"));
        assert!(ParserError::new().to_error_object(&src).is_none());
    }
}
