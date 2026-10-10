//! `ValueSerializer` do V8 (versão 16) com o envelope do Blink (`SerializedScriptValue`, versão 21), como o
//! Chromium 154 grava o valor de um registro do IndexedDB, nos dois sentidos. Os tipos são os que o clone estruturado
//! do IndexedDB aceita; os bytes vêm das medições do oráculo (`idb-basic.html`, `idb-twobyte.html`, `idb-types.html`).
//!
//! Identidade: cada objeto recebe um número na ordem em que o V8 o escreve (um `ArrayBuffer` antes da vista dele) e a
//! segunda ocorrência vira `Ref`. O [`Value`] guarda só o número da referência; quem o monta (a ponte com o JavaScript
//! do prelúdio, em `js_value`) calcula esses números na ordem do V8.

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
    /// O texto do padrão e a máscara de flags do V8 (`g` 1, `i` 2, `m` 4, `y` 8, `u` 16, `s` 32, `d` 128, `v` 256).
    RegExp(Vec<u16>, u32),
    /// `None` é um buraco: um array com buraco sai esparso, como no V8.
    Array(Vec<Option<Value>>),
    Object(Vec<(Vec<u16>, Value)>),
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
    ArrayBuffer(Vec<u8>),
    /// Uma vista: o buffer (escrito antes), o subtag do V8 (`B` Uint8Array, `?` DataView...), o deslocamento e o
    /// tamanho em bytes.
    View(Box<Value>, u8, u64, u64),
    /// `Boolean`, `Number`, `String` e `BigInt` embrulhados em objeto.
    Wrapper(Box<Value>),
    /// O subtag do protótipo (`None` para `Error`), a mensagem e a pilha.
    Error(Option<u8>, Option<Vec<u16>>, Option<Vec<u16>>),
    /// O objeto que já foi escrito, pelo número que o V8 deu a ele.
    Ref(u32),
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
        let mut length_bytes = Vec::new();
        varint(&mut length_bytes, units.len() as u64 * 2);
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
        let smi = value.fract() == 0.0 && (-1073741824.0..=1073741823.0).contains(&value) && !(value == 0.0 && value.is_sign_negative());
        if smi {
            self.tag(b'I');
            self.zigzag(value as i32);
        } else {
            self.tag(b'N');
            self.double(value);
        }
    }

    fn bigint_body(&mut self, negative: bool, words: &[u64]) {
        let mut words = words.to_vec();
        while words.last() == Some(&0) {
            words.pop();
        }
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
            Value::BigInt(negative, words) => {
                self.tag(b'Z');
                self.bigint_body(*negative, words);
            }
            Value::String(units) => self.string(units),
            Value::Date(time) => {
                self.tag(b'D');
                self.double(*time);
            }
            Value::RegExp(source, flags) => {
                self.tag(b'R');
                self.string(source);
                self.varint(*flags as u64);
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
            Value::Map(entries) => {
                self.tag(b';');
                for (key, item) in entries {
                    self.value(key);
                    self.value(item);
                }
                self.tag(b':');
                self.varint(entries.len() as u64 * 2);
            }
            Value::Set(items) => {
                self.tag(b'\'');
                items.iter().for_each(|item| self.value(item));
                self.tag(b',');
                self.varint(items.len() as u64);
            }
            Value::ArrayBuffer(bytes) => {
                self.tag(b'B');
                self.varint(bytes.len() as u64);
                self.out.extend_from_slice(bytes);
            }
            Value::View(buffer, subtag, offset, length) => {
                self.value(buffer);
                self.tag(b'V');
                self.tag(*subtag);
                self.varint(*offset);
                self.varint(*length);
                self.varint(0);
            }
            Value::Wrapper(inner) => match &**inner {
                Value::Bool(true) => self.tag(b'y'),
                Value::Bool(false) => self.tag(b'x'),
                Value::Number(number) => {
                    self.tag(b'n');
                    self.double(*number);
                }
                Value::BigInt(negative, words) => {
                    self.tag(b'z');
                    self.bigint_body(*negative, words);
                }
                Value::String(units) => {
                    self.tag(b's');
                    self.string(units);
                }
                other => self.value(other),
            },
            Value::Error(prototype, message, stack) => {
                self.tag(b'r');
                if let Some(prototype) = prototype {
                    self.tag(*prototype);
                }
                if let Some(message) = message {
                    self.tag(b'm');
                    self.string(message);
                }
                if let Some(stack) = stack {
                    self.tag(b's');
                    self.string(stack);
                }
                self.tag(b'.');
            }
            Value::Ref(id) => {
                self.tag(b'^');
                self.varint(*id as u64);
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

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.position)?;
        self.position += 1;
        Some(byte)
    }

    /// A próxima marca, pulando o preenchimento (`\0`) que alinha as strings de dois bytes.
    fn peek_tag(&mut self) -> Option<u8> {
        while self.data.get(self.position) == Some(&0) {
            self.position += 1;
        }
        self.data.get(self.position).copied()
    }

    fn tag(&mut self) -> Option<u8> {
        let tag = self.peek_tag()?;
        self.position += 1;
        Some(tag)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f).checked_shl(shift)?;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, length: usize) -> Option<&[u8]> {
        let bytes = self.data.get(self.position..self.position.checked_add(length)?)?;
        self.position += length;
        Some(bytes)
    }

    fn double(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<Vec<u16>> {
        match self.tag()? {
            b'"' => {
                let length = self.varint()? as usize;
                Some(self.take(length)?.iter().map(|byte| u16::from(*byte)).collect())
            }
            b'c' => {
                let length = self.varint()? as usize;
                Some(self.take(length)?.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect())
            }
            b'S' => {
                let length = self.varint()? as usize;
                Some(String::from_utf8_lossy(self.take(length)?).encode_utf16().collect())
            }
            _ => None,
        }
    }

    fn bigint_body(&mut self) -> Option<Value> {
        let bits = self.varint()?;
        let bytes = self.take((bits >> 1) as usize)?;
        let words = bytes.chunks(8).map(|chunk| chunk.iter().rev().fold(0u64, |word, byte| (word << 8) | u64::from(*byte))).collect();
        Some(Value::BigInt(bits & 1 == 1, words))
    }

    fn value(&mut self) -> Option<Value> {
        let tag = self.tag()?;
        self.value_of(tag)
    }

    fn value_of(&mut self, tag: u8) -> Option<Value> {
        Some(match tag {
            b'_' => Value::Undefined,
            b'0' => Value::Null,
            b'T' => Value::Bool(true),
            b'F' => Value::Bool(false),
            b'I' => {
                let zigzag = self.varint()? as u32;
                Value::Number(f64::from(((zigzag >> 1) as i32) ^ -((zigzag & 1) as i32)))
            }
            b'U' => Value::Number(self.varint()? as u32 as f64),
            b'N' => Value::Number(self.double()?),
            b'Z' => self.bigint_body()?,
            b'"' | b'c' | b'S' => {
                self.position -= 1;
                Value::String(self.string()?)
            }
            b'D' => Value::Date(self.double()?),
            b'R' => {
                let source = self.string()?;
                Value::RegExp(source, self.varint()? as u32)
            }
            b'^' => Value::Ref(self.varint()? as u32),
            b'o' => {
                let mut properties = Vec::new();
                while self.peek_tag()? != b'{' {
                    let key = self.value()?;
                    let key = match key {
                        Value::String(units) => units,
                        Value::Number(number) => number.to_string().encode_utf16().collect(),
                        _ => return None,
                    };
                    properties.push((key, self.value()?));
                }
                self.tag()?;
                self.varint()?;
                Value::Object(properties)
            }
            b'A' => {
                let length = self.varint()? as usize;
                let mut items = Vec::new();
                for _ in 0..length {
                    items.push(if self.peek_tag()? == b'-' {
                        self.tag()?;
                        None
                    } else {
                        Some(self.value()?)
                    });
                }
                self.skip_properties(b'$')?;
                Value::Array(items)
            }
            b'a' => {
                let length = self.varint()? as usize;
                let mut items = vec![None; length];
                while self.peek_tag()? != b'@' {
                    let Value::Number(index) = self.value()? else { return None };
                    *items.get_mut(index as usize)? = Some(self.value()?);
                }
                self.tag()?;
                self.varint()?;
                self.varint()?;
                Value::Array(items)
            }
            b';' => {
                let mut entries = Vec::new();
                while self.peek_tag()? != b':' {
                    let key = self.value()?;
                    entries.push((key, self.value()?));
                }
                self.tag()?;
                self.varint()?;
                Value::Map(entries)
            }
            b'\'' => {
                let mut items = Vec::new();
                while self.peek_tag()? != b',' {
                    items.push(self.value()?);
                }
                self.tag()?;
                self.varint()?;
                Value::Set(items)
            }
            b'B' => {
                let length = self.varint()? as usize;
                let buffer = Value::ArrayBuffer(self.take(length)?.to_vec());
                if self.peek_tag() != Some(b'V') {
                    return Some(buffer);
                }
                self.tag()?;
                let subtag = self.byte()?;
                let offset = self.varint()?;
                let length = self.varint()?;
                self.varint()?;
                Value::View(Box::new(buffer), subtag, offset, length)
            }
            b'y' => Value::Wrapper(Box::new(Value::Bool(true))),
            b'x' => Value::Wrapper(Box::new(Value::Bool(false))),
            b'n' => Value::Wrapper(Box::new(Value::Number(self.double()?))),
            b'z' => Value::Wrapper(Box::new(self.bigint_body()?)),
            b's' => Value::Wrapper(Box::new(Value::String(self.string()?))),
            b'r' => {
                let mut prototype = None;
                let (mut message, mut stack) = (None, None);
                loop {
                    match self.tag()? {
                        b'.' => break,
                        b'm' => message = Some(self.string()?),
                        b's' => stack = Some(self.string()?),
                        tag @ (b'E' | b'R' | b'F' | b'S' | b'T' | b'U') => prototype = Some(tag),
                        _ => return None,
                    }
                }
                Value::Error(prototype, message, stack)
            }
            _ => return None,
        })
    }

    /// O fim de um array denso: a marca, a contagem de propriedades extras (que o IndexedDB não produz) e o tamanho.
    fn skip_properties(&mut self, end: u8) -> Option<()> {
        if self.tag()? != end || self.varint()? != 0 {
            return None;
        }
        self.varint()?;
        Some(())
    }
}

/// O inverso de [`wire_value`]: `None` quando os bytes não são um valor que o IndexedDB do Chromium grava.
pub fn read_wire_value(bytes: &[u8]) -> Option<Value> {
    let body = bytes.strip_prefix(&BLINK_ENVELOPE)?;
    let mut reader = Reader { data: body, position: 0 };
    if reader.byte()? != 0xff || reader.byte()? != V8_VERSION {
        return None;
    }
    let value = reader.value()?;
    (reader.position == body.len()).then_some(value)
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

    /// Escreve, confere contra os bytes do oráculo e lê de volta o mesmo valor.
    fn check(value: &Value, body: &str) {
        let bytes = wire_value(value);
        assert_eq!(bytes, wire(body));
        assert_eq!(read_wire_value(&bytes).as_ref(), Some(value));
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
        check(&value, "6f 22026964 4902 22046e616d65 220161 22047461 6773 41 02 220178 220179 24 00 02 22047768656e 44 0000000000000000 7b04");
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
            ("bytes", Value::View(Box::new(Value::ArrayBuffer(vec![1, 2, 3])), b'B', 0, 3)),
        ]);
        check(&value, "6f 22026964 4904 22046e616d65 220162 22016e 4e 000000000000f83f 220174 54 22017a 30 2205 6279746573 42 03 010203 56 42 00 03 00 7b06");
    }

    /// `idb-twobyte.html`: o padding depende da posição no buffer inteiro, envelope de 15 bytes incluído.
    #[test]
    fn two_byte_strings_align_against_the_whole_buffer() {
        check(&text("日"), "00 63 02 e565");
        check(&text("a日"), "00 63 04 6100 e565");
        check(&Value::Array(vec![Some(text("x")), Some(text("日"))]), "41 02 220178 63 02 e565 24 00 02");
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
        check(&value, "6f 22016b 00 63 02 e565 220175 5f 220166 46 22036269 67 4e 000000000000e041 22036e6567 49 09 220173 2203616263 7b06");
    }

    #[test]
    fn date_sparse_array_and_bigint() {
        check(&Value::Date(86400000.0), "44 0000000070999441");
        check(&Value::Array(vec![Some(Value::Number(1.0)), None, Some(Value::Number(3.0))]), "61 03 49 00 49 02 49 04 49 06 40 02 03");
        check(&Value::BigInt(false, vec![1]), "5a 10 0100000000000000");
    }

    /// `idb-types.html`: os registros de 2 a 25 (a chave 1 é o RegExp).
    #[test]
    fn the_collections_buffers_and_wrappers_of_idb_types() {
        check(&Value::RegExp(units("ab+c"), 3), "52 22046162 2b63 03");
        check(&Value::Map(vec![(text("a"), Value::Number(1.0)), (Value::Number(2.0), text("b"))]), "3b 220161 4902 4904 220162 3a 04");
        check(&Value::Set(vec![Value::Number(1.0), text("x")]), "27 4902 220178 2c 02");
        check(&Value::ArrayBuffer(vec![0; 3]), "42 03 000000");
        let words = Value::ArrayBuffer(vec![1, 0, 2, 0]);
        check(&Value::View(Box::new(words), b'W', 0, 4), "42 04 01000200 56 57 00 04 00");
        let data_view = Value::ArrayBuffer(vec![0; 4]);
        check(&Value::View(Box::new(data_view), b'?', 1, 2), "42 04 00000000 56 3f 01 02 00");
        let shared = object(vec![("a", object(vec![("s", Value::Number(1.0))])), ("b", Value::Ref(1))]);
        check(&shared, "6f 220161 6f 220173 4902 7b01 220162 5e 01 7b02");
        check(&object(vec![("name", text("c")), ("self", Value::Ref(0))]), "6f 22046e616d65 220163 220473656c66 5e 00 7b02");
        check(&Value::Wrapper(Box::new(Value::Bool(true))), "79");
        check(&Value::Wrapper(Box::new(Value::Number(7.0))), "6e 0000000000001c40");
        check(&Value::Wrapper(Box::new(text("w"))), "73 220177");
    }

    #[test]
    fn numbers_at_the_smi_edges_and_special_values() {
        check(&Value::Number(-0.0), "4e 0000000000000080");
        let nan = wire_value(&Value::Number(f64::NAN));
        assert_eq!(nan, wire("4e 000000000000f87f"));
        assert!(matches!(read_wire_value(&nan), Some(Value::Number(number)) if number.is_nan()));
        check(&Value::Number(-1073741825.0), "4e 000040000000d0c1");
        check(&Value::BigInt(true, vec![1]), "5a 11 0100000000000000");
        check(&Value::BigInt(false, vec![0, 1]), "5a 20 0000000000000000 0100000000000000");
        check(&text("é"), "22 01 e9");
        check(&object(vec![("5", text("five")), ("x y", Value::Number(1.0))]), "6f 490a 220466697665 220378207949 02 7b02");
        check(&Value::Array(vec![]), "41 00 24 00 00");
        check(&object(vec![]), "6f 7b00");
        check(&Value::Null, "30");
    }

    #[test]
    fn errors_carry_the_prototype_message_and_stack() {
        let error = Value::Error(None, Some(units("boom")), Some(units("Error: boom")));
        check(&error, "72 6d 2204626f6f6d 73 220b 4572726f723a20626f6f6d 2e");
        let range = Value::Error(Some(b'R'), Some(units("r")), None);
        check(&range, "72 52 6d 220172 2e");
    }
}
