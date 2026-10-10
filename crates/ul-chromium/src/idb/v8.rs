//! `ValueSerializer` do V8 (versão 16) com o envelope do Blink (`SerializedScriptValue`, versão 21), como o
//! Chromium 154 grava o valor de um registro do IndexedDB. Só os tipos medidos no oráculo (`idb-basic.html`,
//! `idb-twobyte.html`): primitivos, texto Latin1 e de dois bytes, objeto, array denso e esparso, Date, BigInt e
//! `Uint8Array`.

use super::coding::varint;

/// O envelope do Blink: `ff 15`, a marca `fe` do trailer e 12 bytes zero (deslocamento e tamanho do trailer vazio).
const BLINK_ENVELOPE: [u8; 15] = [0xff, 0x15, 0xfe, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const V8_VERSION: u8 = 16;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    /// Sinal negativo e magnitude em palavras de 64 bits, da menos significativa para a mais.
    BigInt(bool, Vec<u64>),
    String(Vec<u16>),
    Date(f64),
    /// `None` é um buraco: um array com buraco sai esparso, como no V8.
    Array(Vec<Option<Value>>),
    Object(Vec<(Vec<u16>, Value)>),
    Uint8Array(Vec<u8>),
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    /// A posição no buffer inteiro que o Blink entrega ao V8: o alinhamento das strings conta o envelope.
    fn position(&self) -> usize {
        BLINK_ENVELOPE.len() + self.out.len()
    }

    fn tag(&mut self, tag: u8) {
        self.out.push(tag);
    }

    fn varint(&mut self, value: u64) {
        varint(&mut self.out, value);
    }

    fn zigzag(&mut self, value: i32) {
        self.varint(((value << 1) ^ (value >> 31)) as u32 as u64);
    }

    fn double(&mut self, value: f64) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, units: &[u16]) {
        if units.iter().all(|unit| *unit <= 0xff) {
            self.tag(b'"');
            self.varint(units.len() as u64);
            self.out.extend(units.iter().map(|unit| *unit as u8));
            return;
        }
        let byte_length = units.len() * 2;
        let mut length_bytes = Vec::new();
        varint(&mut length_bytes, byte_length as u64);
        if (self.position() + 1 + length_bytes.len()) & 1 == 1 {
            self.tag(0);
        }
        self.tag(b'c');
        self.out.extend(length_bytes);
        for unit in units {
            self.out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn number(&mut self, value: f64) {
        let integral = value.fract() == 0.0 && (-1073741824.0..=1073741823.0).contains(&value) && !(value == 0.0 && value.is_sign_negative());
        if integral {
            self.tag(b'I');
            self.zigzag(value as i32);
        } else {
            self.tag(b'N');
            self.double(value);
        }
    }

    fn bigint(&mut self, negative: bool, words: &[u64]) {
        let mut words = words.to_vec();
        while words.last() == Some(&0) {
            words.pop();
        }
        self.tag(b'Z');
        self.varint(((words.len() as u64 * 8) << 1) | negative as u64);
        for word in words {
            self.out.extend_from_slice(&word.to_le_bytes());
        }
    }

    /// Chave de objeto: índice de array vira número, o resto vira texto (`WriteJSObjectPropertiesSlow`).
    fn property_key(&mut self, key: &[u16]) {
        match array_index(key) {
            Some(index) => self.number(index as f64),
            None => self.string(key),
        }
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Undefined => self.tag(b'_'),
            Value::Null => self.tag(b'0'),
            Value::Bool(true) => self.tag(b'T'),
            Value::Bool(false) => self.tag(b'F'),
            Value::Number(number) => self.number(*number),
            Value::BigInt(negative, words) => self.bigint(*negative, words),
            Value::String(units) => self.string(units),
            Value::Date(time) => {
                self.tag(b'D');
                self.double(*time);
            }
            Value::Array(items) if items.iter().all(Option::is_some) => {
                self.tag(b'A');
                self.varint(items.len() as u64);
                items.iter().flatten().for_each(|item| self.value(item));
                self.tag(b'$');
                self.varint(0);
                self.varint(items.len() as u64);
            }
            Value::Array(items) => {
                self.tag(b'a');
                self.varint(items.len() as u64);
                let mut present = 0;
                for (index, item) in items.iter().enumerate() {
                    if let Some(item) = item {
                        self.number(index as f64);
                        self.value(item);
                        present += 1;
                    }
                }
                self.tag(b'@');
                self.varint(present);
                self.varint(items.len() as u64);
            }
            Value::Object(properties) => {
                self.tag(b'o');
                for (key, item) in properties {
                    self.property_key(key);
                    self.value(item);
                }
                self.tag(b'{');
                self.varint(properties.len() as u64);
            }
            Value::Uint8Array(bytes) => {
                self.tag(b'B');
                self.varint(bytes.len() as u64);
                self.out.extend_from_slice(bytes);
                self.tag(b'V');
                self.tag(b'B');
                self.varint(0);
                self.varint(bytes.len() as u64);
                self.varint(0);
            }
        }
    }
}

/// O índice canônico de array (`0` a `2^32 - 2`, sem zeros à esquerda), ou `None`.
fn array_index(key: &[u16]) -> Option<u32> {
    let text = String::from_utf16(key).ok()?;
    let index: u32 = text.parse().ok()?;
    (index != u32::MAX && index.to_string() == text).then_some(index)
}

/// Os bytes que o Chromium grava depois da versão do registro: envelope do Blink, cabeçalho do V8 e o valor.
pub fn wire_value(value: &Value) -> Vec<u8> {
    let mut writer = Writer { out: vec![0xff, V8_VERSION] };
    writer.value(value);
    let mut out = BLINK_ENVELOPE.to_vec();
    out.extend(writer.out);
    out
}

#[cfg(test)]
mod tests {
    use super::super::coding::units;
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
        digits.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex")).collect()
    }

    fn wire(body: &str) -> Vec<u8> {
        hex(&format!("ff15fe 000000000000000000000000 ff10 {body}"))
    }

    fn text(value: &str) -> Value {
        Value::String(units(value))
    }

    fn object(properties: Vec<(&str, Value)>) -> Value {
        Value::Object(properties.into_iter().map(|(key, value)| (units(key), value)).collect())
    }

    /// `idb-basic.html`, registro 1 (lote seq=41 do oráculo).
    #[test]
    fn first_record_of_idb_basic() {
        let value = object(vec![
            ("id", Value::Number(1.0)),
            ("name", text("a")),
            ("tags", Value::Array(vec![Some(text("x")), Some(text("y"))])),
            ("when", Value::Date(0.0)),
        ]);
        assert_eq!(
            wire_value(&value),
            wire("6f 22026964 4902 22046e616d65 220161 22047461 6773 41 02 220178 220179 24 00 02 22047768656e 44 0000000000000000 7b04")
        );
    }

    /// `idb-basic.html`, registro 2 (lote seq=49).
    #[test]
    fn second_record_of_idb_basic() {
        let value = object(vec![
            ("id", Value::Number(2.0)),
            ("name", text("b")),
            ("n", Value::Number(1.5)),
            ("t", Value::Bool(true)),
            ("z", Value::Null),
            ("bytes", Value::Uint8Array(vec![1, 2, 3])),
        ]);
        assert_eq!(
            wire_value(&value),
            wire("6f 22026964 4904 22046e616d65 220162 22016e 4e 000000000000f83f 220174 54 22017a 30 2205 6279746573 42 03 010203 56 42 00 03 00 7b06")
        );
    }

    /// `idb-twobyte.html`: o padding depende da posição no buffer inteiro, envelope de 15 bytes incluído.
    #[test]
    fn two_byte_strings_align_against_the_whole_buffer() {
        assert_eq!(wire_value(&text("日")), wire("00 63 02 e565"));
        assert_eq!(wire_value(&text("a日")), wire("00 63 04 6100 e565"));
        let list = Value::Array(vec![Some(text("x")), Some(text("日"))]);
        assert_eq!(wire_value(&list), wire("41 02 220178 63 02 e565 24 00 02"));
    }

    #[test]
    fn mixed_object_of_idb_twobyte() {
        let value = object(vec![
            ("k", text("日")),
            ("u", Value::Undefined),
            ("f", Value::Bool(false)),
            ("big", Value::Number(2147483648.0)),
            ("neg", Value::Number(-5.0)),
            ("s", text("abc")),
        ]);
        assert_eq!(
            wire_value(&value),
            wire("6f 22016b 00 63 02 e565 220175 5f 220166 46 22036269 67 4e 000000000000e041 22036e6567 49 09 220173 2203616263 7b06")
        );
    }

    #[test]
    fn date_sparse_array_and_bigint() {
        assert_eq!(wire_value(&Value::Date(86400000.0)), wire("44 0000000070999441"));
        let sparse = Value::Array(vec![Some(Value::Number(1.0)), None, Some(Value::Number(3.0))]);
        assert_eq!(wire_value(&sparse), wire("61 03 49 00 49 02 49 04 49 06 40 02 03"));
        assert_eq!(wire_value(&Value::BigInt(false, vec![1])), wire("5a 10 0100000000000000"));
    }
}
