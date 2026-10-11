//! A string do HIR: os bytes de um nome, de um literal ou de uma anotação, dono do próprio conteúdo.
//!
//! No upstream o HIR guarda `StoreStr`, uma fatia da arena global do parser. No porte o `StoreStr` é um índice no
//! `Store` de um arquivo, que só se lê com o `Store` na mão e que compara por posição, não por conteúdo: dois nomes
//! iguais gravados em lugares diferentes seriam diferentes como chave de mapa. O HIR é um mundo à parte do AST,
//! então as strings dele são `HirStr`: igualdade, ordem e hash pelos bytes, cópia barata (`Arc`) e nenhum acesso ao
//! `Store`. Só as fronteiras (a descida do AST para o HIR e a subida de volta) tocam o `Store`: lá lê-se com
//! `store.str(x)` e grava-se com `store.alloc_str(x.slice())`.

use core::fmt;
use core::ops::Deref;
use std::sync::Arc;

use bstr::BStr;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirStr(Arc<[u8]>);

impl HirStr {
    pub fn new(bytes: &[u8]) -> Self {
        Self(Arc::from(bytes))
    }

    pub fn slice(&self) -> &[u8] {
        &self.0
    }
}

impl Default for HirStr {
    fn default() -> Self {
        Self::new(b"")
    }
}

impl Deref for HirStr {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl core::borrow::Borrow<[u8]> for HirStr {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for HirStr {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<&[u8]> for HirStr {
    fn from(bytes: &[u8]) -> Self {
        Self::new(bytes)
    }
}

impl From<&str> for HirStr {
    fn from(text: &str) -> Self {
        Self::new(text.as_bytes())
    }
}

impl PartialEq<[u8]> for HirStr {
    fn eq(&self, other: &[u8]) -> bool {
        *self.0 == *other
    }
}

impl PartialEq<&[u8]> for HirStr {
    fn eq(&self, other: &&[u8]) -> bool {
        *self.0 == **other
    }
}

impl<const N: usize> PartialEq<[u8; N]> for HirStr {
    fn eq(&self, other: &[u8; N]) -> bool {
        *self.0 == other[..]
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for HirStr {
    fn eq(&self, other: &&[u8; N]) -> bool {
        *self.0 == other[..]
    }
}

impl<const N: usize> From<&[u8; N]> for HirStr {
    fn from(bytes: &[u8; N]) -> Self {
        Self::new(bytes)
    }
}

impl fmt::Debug for HirStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(BStr::new(&self.0), f)
    }
}

impl fmt::Display for HirStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(BStr::new(&self.0), f)
    }
}
