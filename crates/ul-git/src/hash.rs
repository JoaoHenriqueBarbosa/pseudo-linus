//! Ids de objeto (SHA-1) e hash de objetos no formato do git (`<tipo> <tamanho>\0<dados>`).

use std::fmt;

use sha1::{Digest, Sha1};

/// Id de objeto SHA-1.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Oid(pub [u8; 20]);

impl Oid {
    pub const ZERO: Oid = Oid([0; 20]);
    pub const HEX_LEN: usize = 40;

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 20]
    }

    pub fn hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(40);
        for b in self.0 {
            s.push(DIGITS[(b >> 4) as usize] as char);
            s.push(DIGITS[(b & 15) as usize] as char);
        }
        s
    }

    /// Os primeiros `n` dígitos hexadecimais.
    pub fn short(&self, n: usize) -> String {
        let mut h = self.hex();
        h.truncate(n.min(40));
        h
    }

    /// 40 dígitos hexadecimais (maiúsculas aceitas).
    pub fn from_hex(s: &[u8]) -> Option<Oid> {
        if s.len() != 40 {
            return None;
        }
        let mut out = [0u8; 20];
        for (i, pair) in s.chunks(2).enumerate() {
            out[i] = (hex_val(pair[0])? << 4) | hex_val(pair[1])?;
        }
        Some(Oid(out))
    }

    pub fn from_bytes(b: &[u8]) -> Option<Oid> {
        let arr: [u8; 20] = b.try_into().ok()?;
        Some(Oid(arr))
    }

    /// Se o hexadecimal do id começa por `prefix` (já em minúsculas).
    pub fn hex_starts_with(&self, prefix: &[u8]) -> bool {
        let hex = self.hex();
        hex.as_bytes().starts_with(prefix)
    }
}

pub fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn is_hex(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(|c| c.is_ascii_hexdigit())
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Oid({})", self.hex())
    }
}

/// Tipo de objeto.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Commit,
    Tree,
    Blob,
    Tag,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Commit => "commit",
            Kind::Tree => "tree",
            Kind::Blob => "blob",
            Kind::Tag => "tag",
        }
    }

    pub fn from_name(s: &[u8]) -> Option<Kind> {
        match s {
            b"commit" => Some(Kind::Commit),
            b"tree" => Some(Kind::Tree),
            b"blob" => Some(Kind::Blob),
            b"tag" => Some(Kind::Tag),
            _ => None,
        }
    }

    /// Número do tipo no packfile.
    pub fn from_pack_type(t: u8) -> Option<Kind> {
        match t {
            1 => Some(Kind::Commit),
            2 => Some(Kind::Tree),
            3 => Some(Kind::Blob),
            4 => Some(Kind::Tag),
            _ => None,
        }
    }

    pub fn pack_type(self) -> u8 {
        match self {
            Kind::Commit => 1,
            Kind::Tree => 2,
            Kind::Blob => 3,
            Kind::Tag => 4,
        }
    }
}

/// Cabeçalho de objeto solto: `<tipo> <tamanho>\0`.
pub fn header(kind: Kind, len: usize) -> Vec<u8> {
    format!("{} {}\0", kind.name(), len).into_bytes()
}

/// Id do objeto com esses dados.
pub fn hash_object(kind: Kind, data: &[u8]) -> Oid {
    let mut h = Sha1::new();
    h.update(header(kind, data.len()));
    h.update(data);
    Oid(h.finalize().into())
}

/// SHA-1 cru de um buffer (checksum do índice e do pack).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(data);
    h.finalize().into()
}

/// Id do blob vazio e da tree vazia, que o git conhece mesmo sem estarem no repositório.
pub const EMPTY_BLOB: Oid = Oid([
    0xe6, 0x9d, 0xe2, 0x9b, 0xb2, 0xd1, 0xd6, 0x43, 0x4b, 0x8b, 0x29, 0xae, 0x77, 0x5a, 0xd8, 0xc2, 0xe4, 0x8c, 0x53, 0x91,
]);
pub const EMPTY_TREE: Oid = Oid([
    0x4b, 0x82, 0x5d, 0xc6, 0x42, 0xcb, 0x6e, 0xb9, 0xa0, 0x60, 0xe5, 0x4b, 0xf8, 0xd6, 0x92, 0x88, 0xfb, 0xee, 0x49, 0x04,
]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids() {
        assert_eq!(hash_object(Kind::Blob, b""), EMPTY_BLOB);
        assert_eq!(hash_object(Kind::Tree, b""), EMPTY_TREE);
        assert_eq!(hash_object(Kind::Blob, b"hello\n").hex(), "ce013625030ba8dba906f756967f9e9ca394464a");
        let id = Oid::from_hex(b"CE013625030BA8DBA906F756967F9E9CA394464A").unwrap();
        assert_eq!(id.hex(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }
}
