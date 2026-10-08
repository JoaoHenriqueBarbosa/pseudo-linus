//! Porte de `JavaScriptCore/bytecompiler`: os headers pequenos.
//! O `BytecodeGenerator` fica em `crate::bytecompiler::bytecode_generator` (ainda não portado).

pub mod label;
pub mod label_scope;
pub mod profile_type_bytecode_flag;
pub mod register_id;
pub mod static_property_analysis;
pub mod static_property_analyzer;
