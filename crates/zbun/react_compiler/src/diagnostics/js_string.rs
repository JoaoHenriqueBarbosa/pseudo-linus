//! Um valor de string JavaScript como aparece no HIR (`PrimitiveValue::String`).
//!
//! No upstream é um embrulho de ponteiro sobre o nó `E::EString` da arena do parser. No porte o HIR é um mundo à
//! parte do `Store` (ver `HirStr`): o `JsString` é dono do conteúdo, com igualdade e hash por valor. A forma é
//! canônica, para a igualdade derivada valer: ASCII puro fica em 8 bits; qualquer outra coisa vira UTF-16, como a
//! convenção do `EString` do parser. Só as fronteiras (`from_flat` na descida, `to_estring` na subida) tocam o `Store`.

use core::fmt;
use std::sync::Arc;

use bstr::BStr;
use bun_ast::{E, NodeStore, Store, StoreRef};
use bun_core::strings;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum JsString {
    /// ASCII puro.
    Ascii(Arc<[u8]>),
    /// Unidades UTF-16, com as metades de substituto soltas que o JavaScript permite.
    Utf16(Arc<[u16]>),
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JsString({self})")
    }
}

impl JsString {
    /// De bytes WTF-8: ASCII fica em 8 bits, o resto vira UTF-16.
    pub fn from_wtf8_bytes(bytes: &[u8]) -> Self {
        match strings::to_utf16_alloc(bytes, false, false) {
            Ok(Some(units)) => Self::Utf16(units.into()),
            _ => Self::Ascii(bytes.into()),
        }
    }

    /// De unidades UTF-16. Se todas forem ASCII a forma canônica é a de 8 bits.
    pub fn from_code_units(units: &[u16]) -> Self {
        if units.iter().all(|&u| u < 0x80) {
            Self::Ascii(units.iter().map(|&u| u as u8).collect())
        } else {
            Self::Utf16(units.into())
        }
    }

    /// Do texto de um `EString` do parser, depois de juntar a corda (a descida do AST para o HIR).
    pub fn from_flat(text: &E::FlatString<'_>) -> Self {
        if text.is_utf8() {
            Self::from_wtf8_bytes(text.slice8())
        } else {
            Self::from_code_units(text.slice16())
        }
    }

    /// Grava no `Store` um `EString` com este conteúdo (a subida do HIR para o AST).
    pub fn to_estring(&self, store: &mut Store) -> StoreRef<E::EString> {
        let node = match self {
            Self::Ascii(bytes) => E::EString::init(store.alloc_str(bytes)),
            Self::Utf16(units) => E::EString::init_utf16(store.alloc_str16(units)),
        };
        store.alloc(node)
    }

    /// Os bytes quando guardada em 8 bits (ASCII); `None` para UTF-16.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Ascii(bytes) => Some(bytes),
            Self::Utf16(_) => None,
        }
    }

    /// As unidades UTF-16, qualquer que seja a forma.
    pub fn units(&self) -> impl Iterator<Item = u16> + '_ {
        let (ascii, wide): (&[u8], &[u16]) = match self {
            Self::Ascii(bytes) => (bytes, &[]),
            Self::Utf16(units) => (&[], units),
        };
        ascii.iter().map(|&b| u16::from(b)).chain(wide.iter().copied())
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Ascii(bytes) => bytes.is_empty(),
            Self::Utf16(units) => units.is_empty(),
        }
    }

    /// O `String.prototype.length` do JavaScript (contagem de unidades UTF-16).
    pub fn len_utf16(&self) -> usize {
        match self {
            Self::Ascii(bytes) => bytes.len(),
            Self::Utf16(units) => units.len(),
        }
    }
}

impl fmt::Display for JsString {
    /// A forma do printer de depuração: bytes via `BStr`; UTF-16 com as metades soltas como `\udXXX` em minúsculas
    /// (o mesmo que o `JSON.stringify`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let units = match self {
            Self::Ascii(bytes) => return fmt::Display::fmt(BStr::new(bytes), f),
            Self::Utf16(units) => units,
        };
        for r in char::decode_utf16(units.iter().copied()) {
            match r {
                Ok(c) => f.write_fmt(format_args!("{c}"))?,
                Err(e) => write!(f, "\\u{:04x}", e.unpaired_surrogate())?,
            }
        }
        Ok(())
    }
}
