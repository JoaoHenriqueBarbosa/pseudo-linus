use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes que viram string legível no JSON quando são UTF-8 válido, e `{"b64": "..."}` quando não são.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Representação curta pra mensagens de divergência.
    pub fn preview(&self, max: usize) -> String {
        let text = String::from_utf8_lossy(&self.0);
        if text.chars().count() <= max {
            format!("{text:?}")
        } else {
            let cut: String = text.chars().take(max).collect();
            format!("{cut:?}... ({} bytes)", self.0.len())
        }
    }
}

impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.preview(80))
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Bytes(v)
    }
}

impl From<&[u8]> for Bytes {
    fn from(v: &[u8]) -> Self {
        Bytes(v.to_vec())
    }
}

impl From<&str> for Bytes {
    fn from(v: &str) -> Self {
        Bytes(v.as_bytes().to_vec())
    }
}

impl From<String> for Bytes {
    fn from(v: String) -> Self {
        Bytes(v.into_bytes())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Repr {
    Text(String),
    Binary { b64: String },
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(&self.0) {
            Ok(text) => Repr::Text(text.to_owned()).serialize(s),
            Err(_) => Repr::Binary { b64: STANDARD.encode(&self.0) }.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Repr::deserialize(d)? {
            Repr::Text(text) => Ok(Bytes(text.into_bytes())),
            Repr::Binary { b64 } => STANDARD
                .decode(b64.as_bytes())
                .map(Bytes)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text_and_binary() {
        for raw in [b"hello\n".to_vec(), vec![0xff, 0x00, 0x80]] {
            let b = Bytes(raw.clone());
            let json = serde_json::to_string(&b).unwrap();
            let back: Bytes = serde_json::from_str(&json).unwrap();
            assert_eq!(back.0, raw);
        }
        assert_eq!(serde_json::to_string(&Bytes::from("a")).unwrap(), "\"a\"");
    }
}
