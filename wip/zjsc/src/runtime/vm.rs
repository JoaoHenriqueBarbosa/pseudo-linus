//! Esqueleto de `JavaScriptCore/runtime/VM.h`.
//!
//! Hoje só existe para que `Identifier` tenha o parâmetro `VM&` do C++. A tabela de átomos é por
//! thread em `crate::wtf::text::atom_string_impl`, então o `VM` não a carrega.

/// `class VM`.
#[derive(Debug, Default)]
pub struct VM {
    // Camada runtime: o resto do VM.h entra aqui.
}
