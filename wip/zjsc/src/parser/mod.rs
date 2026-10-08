//! Porte de `JavaScriptCore/parser`.
pub mod keyword_lookup;
pub mod lexer;
pub mod lexer_lut;
pub mod lexer_unicode_properties;
pub mod module_scope_data;
pub mod nodes;
pub mod parser_arena;
pub mod parser_error;
pub mod parser_modes;
pub mod parser_tokens;
pub mod result_type;
pub mod source_code;
pub mod source_code_key;
pub mod source_provider;
pub mod source_tainted_origin;
pub mod unlinked_source_code;
pub mod variable_environment;
