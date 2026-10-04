//! Implements a tokenizer and parsers for POSIX / bash shell syntax.
//!
//! Fork do brush-parser 0.4.0 pro shell do pseudo-linus (ver `FORK.md`).

// TODO(unwrap): remove or scope this allow attribute
#![allow(clippy::unwrap_used)]
#![allow(clippy::all)]

pub mod arithmetic;
pub mod ast;
pub mod pattern;
pub mod prompt;
pub mod readline_binding;
pub mod test_command;
pub mod word;

mod error;
mod parser;
mod source;
mod tokenizer;

pub use error::{
    BindingParseError, ParseError, ParseErrorLocation, TestCommandParseError, WordParseError,
};

pub use parser::{ParserImpl, Parser, ParserOptions, SourceInfo, parse_tokens};

pub use source::{SourcePosition, SourcePositionOffset, SourceSpan};
pub use tokenizer::{
    Token, TokenLocation, TokenizerError, TokenizerOptions, scan_command_substitution, tokenize_str,
    tokenize_str_with_options, uncached_tokenize_str, unquote_str,
};
