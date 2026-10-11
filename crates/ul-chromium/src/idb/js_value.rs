//! A ponte entre o texto que o prelúdio JavaScript troca com o perfil (`serializeValue`, em `69-persistence.js`) e
//! os valores do disco: o diário de uma transação entra como [`Js`], o valor de um registro vira o
//! [`v8::Value`](super::v8::Value) que o Chromium grava, e o banco lido do disco volta como o mesmo texto.
//!
//! O texto do prelúdio numera os objetos na ordem da visita (a segunda visita vira `['r', n]`); o V8 também, mas com
//! o `ArrayBuffer` antes da vista dele. As duas numerações convivem aqui: `Js` guarda a do prelúdio, `Value::Ref` a do V8.

use std::collections::HashMap;

use serde_json::Value as Json;

use super::coding::{units, IdbKey};
use super::v8::Value;

/// Um valor do prelúdio, com o número que o prelúdio deu a cada objeto.
#[derive(Clone, Debug, PartialEq)]
pub enum Js {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    /// O inteiro grande em decimal, como o prelúdio o escreve.
    BigInt(String),
    String(Vec<u16>),
    Ref(usize),
    Node(usize, Box<Kind>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Object(Vec<(Vec<u16>, Js)>),
    /// O tamanho e os pares (índice, valor) das posições que existem.
    Array(usize, Vec<(usize, Js)>),
    Date(f64),
    RegExp(Vec<u16>, String),
    Map(Vec<(Js, Js)>),
    Set(Vec<Js>),
    Buffer(Vec<u8>),
    /// O tipo (`Uint8Array`...), o buffer, o deslocamento e o tamanho em elementos.
    View(String, Js, u64, u64),
    Wrapper(String, Js),
    /// O nome da classe, a mensagem e a pilha.
    Error(String, Js, Js),
}

struct Parser {
    next_id: usize,
}

fn text(json: &Json) -> Option<Vec<u16>> {
    Some(json.as_str()?.encode_utf16().collect())
}

fn special(name: &str) -> Option<f64> {
    Some(match name {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        "-0" => -0.0,
        other => other.parse().ok()?,
    })
}

impl Parser {
    fn claim(&mut self) -> usize {
        self.next_id += 1;
        self.next_id - 1
    }

    fn pairs(&mut self, list: &Json) -> Option<Vec<(Json, Js)>> {
        list.as_array()?.iter().map(|pair| Some((pair.get(0)?.clone(), self.value(pair.get(1)?)?))).collect()
    }

    fn value(&mut self, json: &Json) -> Option<Js> {
        let Some(list) = json.as_array() else {
            return Some(match json {
                Json::Null => Js::Null,
                Json::Bool(flag) => Js::Bool(*flag),
                Json::Number(number) => Js::Number(number.as_f64()?),
                Json::String(string) => Js::String(string.encode_utf16().collect()),
                _ => return None,
            });
        };
        let tag = list.first()?.as_str()?;
        Some(match tag {
            "u" => Js::Undefined,
            "d" => Js::Number(special(list.get(1)?.as_str()?)?),
            "i" => Js::BigInt(list.get(1)?.as_str()?.to_owned()),
            "r" => Js::Ref(list.get(1)?.as_u64()? as usize),
            _ => {
                let id = self.claim();
                let kind = match tag {
                    "B" => Kind::Buffer(list.get(1)?.as_str()?.chars().map(|c| c as u32 as u8).collect()),
                    "V" => {
                        let name = list.get(1)?.as_str()?.to_owned();
                        let buffer = self.value(list.get(2)?)?;
                        Kind::View(name, buffer, list.get(3)?.as_u64()?, list.get(4)?.as_u64()?)
                    }
                    "D" => Kind::Date(match list.get(1)? {
                        Json::String(name) => special(name)?,
                        number => number.as_f64()?,
                    }),
                    "R" => Kind::RegExp(text(list.get(1)?)?, list.get(2)?.as_str()?.to_owned()),
                    "W" => Kind::Wrapper(list.get(1)?.as_str()?.to_owned(), self.value(list.get(2)?)?),
                    "M" => Kind::Map(list.get(1)?.as_array()?.iter().map(|pair| Some((self.value(pair.get(0)?)?, self.value(pair.get(1)?)?))).collect::<Option<_>>()?),
                    "S" => Kind::Set(list.get(1)?.as_array()?.iter().map(|item| self.value(item)).collect::<Option<_>>()?),
                    "E" => Kind::Error(list.get(1)?.as_str()?.to_owned(), self.value(list.get(2)?)?, self.value(list.get(3)?)?),
                    "a" => {
                        let items = self.pairs(list.get(2)?)?;
                        let items = items.into_iter().map(|(key, item)| Some((key.as_str()?.parse().ok()?, item))).collect::<Option<_>>()?;
                        Kind::Array(list.get(1)?.as_u64()? as usize, items)
                    }
                    _ => {
                        let fields = self.pairs(list.get(1)?)?;
                        Kind::Object(fields.into_iter().map(|(key, item)| Some((text(&key)?, item))).collect::<Option<_>>()?)
                    }
                };
                Js::Node(id, Box::new(kind))
            }
        })
    }
}

/// Lê o texto do prelúdio; `None` quando não é um valor que ele escreve.
pub fn parse(source: &str) -> Option<Js> {
    let json: Json = serde_json::from_str(source).ok()?;
    Parser { next_id: 0 }.value(&json)
}

impl Js {
    /// O campo `name` de um objeto.
    pub fn field(&self, name: &str) -> Option<&Js> {
        let Js::Node(_, kind) = self else { return None };
        let Kind::Object(fields) = &**kind else { return None };
        let wanted = units(name);
        fields.iter().find(|(key, _)| *key == wanted).map(|(_, value)| value)
    }

    pub fn string(&self) -> Option<String> {
        match self {
            Js::String(units) => String::from_utf16(units).ok(),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Js::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn flag(&self) -> bool {
        matches!(self, Js::Bool(true))
    }

    /// Os itens de um array (as posições que existem, em ordem).
    pub fn items(&self) -> Vec<&Js> {
        match self {
            Js::Node(_, kind) => match &**kind {
                Kind::Array(_, items) => items.iter().map(|(_, item)| item).collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }
}

/// A chave do IndexedDB que o prelúdio representa como `{ type, value }` (1 número, 2 data, 3 texto, 4 binário, 5 array).
pub fn idb_key(key: &Js) -> Option<IdbKey> {
    let value = key.field("value")?;
    Some(match key.field("type")?.number()? as u8 {
        1 => IdbKey::Number(value.number()?),
        2 => IdbKey::Date(value.number()?),
        3 => IdbKey::String(match value {
            Js::String(text) => text.clone(),
            _ => return None,
        }),
        4 => match value {
            Js::Node(_, kind) => match &**kind {
                Kind::View(_, Js::Node(_, buffer), _, _) => match &**buffer {
                    Kind::Buffer(bytes) => IdbKey::Binary(bytes.clone()),
                    _ => return None,
                },
                _ => return None,
            },
            _ => return None,
        },
        5 => IdbKey::Array(value.items().into_iter().map(idb_key).collect::<Option<_>>()?),
        _ => return None,
    })
}

/// O `keyPath` do prelúdio (nulo, texto ou lista de textos).
pub fn key_path(path: &Js) -> Option<super::coding::KeyPath> {
    Some(match path {
        Js::Null => super::coding::KeyPath::Null,
        Js::String(text) => super::coding::KeyPath::String(text.clone()),
        other => super::coding::KeyPath::Array(other.items().into_iter().map(|item| if let Js::String(text) = item { Some(text.clone()) } else { None }).collect::<Option<_>>()?),
    })
}

fn view_tag(name: &str) -> Option<(u8, u64)> {
    Some(match name {
        "Int8Array" => (b'b', 1),
        "Uint8Array" => (b'B', 1),
        "Uint8ClampedArray" => (b'C', 1),
        "Int16Array" => (b'w', 2),
        "Uint16Array" => (b'W', 2),
        "Int32Array" => (b'd', 4),
        "Uint32Array" => (b'D', 4),
        "Float32Array" => (b'f', 4),
        "Float64Array" => (b'F', 8),
        "BigInt64Array" => (b'q', 8),
        "BigUint64Array" => (b'Q', 8),
        "DataView" => (b'?', 1),
        _ => return None,
    })
}

fn regexp_flags(flags: &str) -> u32 {
    flags.chars().fold(0, |mask, flag| {
        mask | match flag {
            'g' => 1,
            'i' => 2,
            'm' => 4,
            'y' => 8,
            'u' => 16,
            's' => 32,
            'd' => 128,
            'v' => 256,
            _ => 0,
        }
    })
}

/// Os 64 bits por palavra de um inteiro decimal (a magnitude, da palavra menos significativa para a mais).
fn decimal_words(digits: &str) -> (bool, Vec<u64>) {
    let (negative, digits) = digits.strip_prefix('-').map_or((false, digits), |rest| (true, rest));
    let mut words: Vec<u64> = Vec::new();
    for digit in digits.bytes().map(|byte| u128::from(byte - b'0')) {
        let mut carry = digit;
        for word in &mut words {
            let product = u128::from(*word) * 10 + carry;
            *word = product as u64;
            carry = product >> 64;
        }
        if carry > 0 {
            words.push(carry as u64);
        }
    }
    (negative, words)
}

/// O inverso de [`decimal_words`].
fn words_decimal(negative: bool, words: &[u64]) -> String {
    let mut words = words.to_vec();
    while words.last() == Some(&0) {
        words.pop();
    }
    let mut digits = Vec::new();
    while !words.is_empty() {
        let mut remainder = 0u128;
        for word in words.iter_mut().rev() {
            let current = (remainder << 64) | u128::from(*word);
            *word = (current / 10) as u64;
            remainder = current % 10;
        }
        digits.push(b'0' + remainder as u8);
        while words.last() == Some(&0) {
            words.pop();
        }
    }
    if digits.is_empty() {
        digits.push(b'0');
    }
    if negative {
        digits.push(b'-');
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

struct ToV8 {
    next: u32,
    /// O número do V8 de cada objeto do prelúdio já escrito.
    ids: HashMap<usize, u32>,
}

impl ToV8 {
    fn convert(&mut self, js: &Js) -> Option<Value> {
        let (id, kind) = match js {
            Js::Undefined => return Some(Value::Undefined),
            Js::Null => return Some(Value::Null),
            Js::Bool(flag) => return Some(Value::Bool(*flag)),
            Js::Number(number) => return Some(Value::Number(*number)),
            Js::BigInt(digits) => {
                let (negative, words) = decimal_words(digits);
                return Some(Value::BigInt(negative, words));
            }
            Js::String(text) => return Some(Value::String(text.clone())),
            Js::Ref(id) => return Some(Value::Ref(*self.ids.get(id)?)),
            Js::Node(id, kind) => (*id, kind),
        };
        // A vista só ganha o número depois do buffer dela (a ordem em que o V8 escreve).
        if !matches!(&**kind, Kind::View(..)) {
            self.ids.insert(id, self.next);
            self.next += 1;
        }
        Some(match &**kind {
            Kind::Object(fields) => Value::Object(fields.iter().map(|(key, item)| Some((key.clone(), self.convert(item)?))).collect::<Option<_>>()?),
            Kind::Array(length, items) => {
                let mut slots = vec![None; *length];
                for (index, item) in items {
                    *slots.get_mut(*index)? = Some(self.convert(item)?);
                }
                Value::Array(slots)
            }
            Kind::Date(time) => Value::Date(*time),
            Kind::RegExp(source, flags) => Value::RegExp(source.clone(), regexp_flags(flags)),
            Kind::Map(entries) => Value::Map(entries.iter().map(|(key, item)| Some((self.convert(key)?, self.convert(item)?))).collect::<Option<_>>()?),
            Kind::Set(items) => Value::Set(items.iter().map(|item| self.convert(item)).collect::<Option<_>>()?),
            Kind::Buffer(bytes) => Value::ArrayBuffer(bytes.clone()),
            Kind::View(name, buffer, offset, length) => {
                let (subtag, size) = view_tag(name)?;
                let buffer = self.convert(buffer)?;
                self.ids.insert(id, self.next);
                self.next += 1;
                Value::View(Box::new(buffer), subtag, *offset, length * size)
            }
            Kind::Wrapper(_, inner) => Value::Wrapper(Box::new(self.convert(inner)?)),
            Kind::Error(name, message, stack) => {
                let prototype = match name.as_str() {
                    "EvalError" => Some(b'E'),
                    "RangeError" => Some(b'R'),
                    "ReferenceError" => Some(b'F'),
                    "SyntaxError" => Some(b'S'),
                    "TypeError" => Some(b'T'),
                    "URIError" => Some(b'U'),
                    _ => None,
                };
                let part = |js: &Js| if let Js::String(text) = js { Some(text.clone()) } else { None };
                Value::Error(prototype, part(message), part(stack))
            }
        })
    }
}

/// O valor de um registro, na forma que o V8 o escreve; `None` para o que o clone estruturado não produz.
pub fn to_v8(value: &Js) -> Option<Value> {
    ToV8 { next: 0, ids: HashMap::new() }.convert(value)
}

/// O texto que o prelúdio lê (`deserializeValue`), montado na ordem em que ele numera os objetos.
pub struct Writer {
    pub out: String,
    next_id: usize,
}

impl Default for Writer {
    fn default() -> Self {
        Writer { out: String::new(), next_id: 0 }
    }
}

impl Writer {
    pub fn claim(&mut self) -> usize {
        self.next_id += 1;
        self.next_id - 1
    }

    pub fn raw(&mut self, text: &str) {
        self.out.push_str(text);
    }

    pub fn string(&mut self, units: &[u16]) {
        self.out.push('"');
        for decoded in char::decode_utf16(units.iter().copied()) {
            match decoded {
                Ok('"') => self.out.push_str("\\\""),
                Ok('\\') => self.out.push_str("\\\\"),
                Ok(character) if (character as u32) < 0x20 => self.out.push_str(&format!("\\u{:04x}", character as u32)),
                Ok(character) => self.out.push(character),
                Err(error) => self.out.push_str(&format!("\\u{:04x}", error.unpaired_surrogate())),
            }
        }
        self.out.push('"');
    }

    pub fn number(&mut self, value: f64) {
        if value.is_nan() {
            self.out.push_str("[\"d\",\"NaN\"]");
        } else if value == f64::INFINITY {
            self.out.push_str("[\"d\",\"Infinity\"]");
        } else if value == f64::NEG_INFINITY {
            self.out.push_str("[\"d\",\"-Infinity\"]");
        } else if value == 0.0 && value.is_sign_negative() {
            self.out.push_str("[\"d\",\"-0\"]");
        } else {
            self.out.push_str(&format!("{value}"));
        }
    }

    /// `items` escreve cada elemento, separado por vírgula.
    pub fn list<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Writer, &T)) {
        for (position, item) in items.iter().enumerate() {
            if position > 0 {
                self.out.push(',');
            }
            each(self, item);
        }
    }

    /// Um objeto `['o', [[campo, valor], ...]]` com os valores já escritos por `fields`.
    pub fn object(&mut self, fields: impl FnOnce(&mut Writer)) {
        self.claim();
        self.out.push_str("[\"o\",[");
        fields(self);
        self.out.push_str("]]");
    }

    pub fn field(&mut self, first: bool, name: &str, value: impl FnOnce(&mut Writer)) {
        if !first {
            self.out.push(',');
        }
        self.out.push_str(&format!("[\"{name}\","));
        value(self);
        self.out.push(']');
    }

    /// Escreve um valor do disco. `v8` é o contador de números do V8 deste valor; `ids` liga cada um ao número que o
    /// prelúdio dá ao mesmo objeto.
    pub fn value(&mut self, value: &Value, v8: &mut u32, ids: &mut HashMap<u32, usize>) {
        match value {
            Value::Undefined => self.raw("[\"u\"]"),
            Value::Null => self.raw("null"),
            Value::Bool(flag) => self.raw(if *flag { "true" } else { "false" }),
            Value::Number(number) => self.number(*number),
            Value::BigInt(negative, words) => self.raw(&format!("[\"i\",\"{}\"]", words_decimal(*negative, words))),
            Value::String(text) => self.string(text),
            Value::Ref(id) => {
                let target = ids.get(id).copied().unwrap_or(0);
                self.raw(&format!("[\"r\",{target}]"));
            }
            Value::View(buffer, subtag, offset, length) => {
                let js_id = self.claim();
                let (name, size) = view_name(*subtag);
                self.raw(&format!("[\"V\",\"{name}\","));
                self.value(buffer, v8, ids);
                ids.insert(*v8, js_id);
                *v8 += 1;
                self.raw(&format!(",{offset},{}]", length / size));
            }
            other => {
                ids.insert(*v8, self.claim());
                *v8 += 1;
                self.container(other, v8, ids);
            }
        }
    }

    fn container(&mut self, value: &Value, v8: &mut u32, ids: &mut HashMap<u32, usize>) {
        match value {
            Value::Date(time) => {
                self.raw("[\"D\",");
                if time.is_finite() { self.number(*time) } else { self.raw("\"NaN\"") }
                self.raw("]");
            }
            Value::RegExp(source, flags) => {
                self.raw("[\"R\",");
                self.string(source);
                let letters: String = [(1, 'g'), (2, 'i'), (4, 'm'), (8, 'y'), (16, 'u'), (32, 's'), (128, 'd'), (256, 'v')].iter().filter(|(bit, _)| flags & bit != 0).map(|(_, letter)| *letter).collect();
                self.raw(&format!(",\"{letters}\"]"));
            }
            Value::Array(items) => {
                self.raw(&format!("[\"a\",{},[", items.len()));
                let mut first = true;
                for (index, item) in items.iter().enumerate() {
                    let Some(item) = item else { continue };
                    if !first {
                        self.raw(",");
                    }
                    first = false;
                    self.raw(&format!("[\"{index}\","));
                    self.value(item, v8, ids);
                    self.raw("]");
                }
                self.raw("]]");
            }
            Value::Object(properties) => {
                self.raw("[\"o\",[");
                for (position, (key, item)) in properties.iter().enumerate() {
                    if position > 0 {
                        self.raw(",");
                    }
                    self.raw("[");
                    self.string(key);
                    self.raw(",");
                    self.value(item, v8, ids);
                    self.raw("]");
                }
                self.raw("]]");
            }
            Value::Map(entries) => {
                self.raw("[\"M\",[");
                for (position, (key, item)) in entries.iter().enumerate() {
                    if position > 0 {
                        self.raw(",");
                    }
                    self.raw("[");
                    self.value(key, v8, ids);
                    self.raw(",");
                    self.value(item, v8, ids);
                    self.raw("]");
                }
                self.raw("]]");
            }
            Value::Set(items) => {
                self.raw("[\"S\",[");
                for (position, item) in items.iter().enumerate() {
                    if position > 0 {
                        self.raw(",");
                    }
                    self.value(item, v8, ids);
                }
                self.raw("]]");
            }
            Value::ArrayBuffer(bytes) => {
                self.raw("[\"B\",");
                let latin1: Vec<u16> = bytes.iter().map(|byte| u16::from(*byte)).collect();
                self.string(&latin1);
                self.raw("]");
            }
            Value::Wrapper(inner) => {
                let name = match &**inner {
                    Value::Bool(_) => "Boolean",
                    Value::Number(_) => "Number",
                    Value::String(_) => "String",
                    _ => "BigInt",
                };
                self.raw(&format!("[\"W\",\"{name}\","));
                self.value(inner, v8, ids);
                self.raw("]");
            }
            Value::Error(prototype, message, stack) => {
                let name = match prototype {
                    Some(b'E') => "EvalError",
                    Some(b'R') => "RangeError",
                    Some(b'F') => "ReferenceError",
                    Some(b'S') => "SyntaxError",
                    Some(b'T') => "TypeError",
                    Some(b'U') => "URIError",
                    _ => "Error",
                };
                self.raw(&format!("[\"E\",\"{name}\","));
                for (position, part) in [message, stack].into_iter().enumerate() {
                    if position > 0 {
                        self.raw(",");
                    }
                    match part {
                        Some(text) => self.string(text),
                        None => self.raw("[\"u\"]"),
                    }
                }
                self.raw("]");
            }
            _ => {}
        }
    }
}

fn view_name(subtag: u8) -> (&'static str, u64) {
    match subtag {
        b'b' => ("Int8Array", 1),
        b'C' => ("Uint8ClampedArray", 1),
        b'w' => ("Int16Array", 2),
        b'W' => ("Uint16Array", 2),
        b'd' => ("Int32Array", 4),
        b'D' => ("Uint32Array", 4),
        b'f' => ("Float32Array", 4),
        b'F' => ("Float64Array", 8),
        b'q' => ("BigInt64Array", 8),
        b'Q' => ("BigUint64Array", 8),
        b'?' => ("DataView", 1),
        _ => ("Uint8Array", 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idb::v8::{read_wire_value, wire_value};

    /// O valor que o prelúdio escreve, como o Chromium o grava, e de volta ao texto do prelúdio.
    fn round_trip(source: &str) -> String {
        let js = parse(source).expect("texto do prelúdio");
        let value = to_v8(&js).expect("valor do clone estruturado");
        let wire = wire_value(&value);
        let read = read_wire_value(&wire).expect("bytes do V8");
        assert_eq!(read, value);
        let mut writer = Writer::default();
        writer.value(&read, &mut 0, &mut HashMap::new());
        writer.out
    }

    #[test]
    fn a_plain_record_survives_the_disk_format() {
        let source = r#"["o",[["id",1],["name","a"],["tags",["a",2,[["0","x"],["1","y"]]]],["when",["D",0]]]]"#;
        assert_eq!(round_trip(source), source);
    }

    #[test]
    fn a_view_is_numbered_before_its_buffer_by_the_prelude_and_after_it_by_v8() {
        // o objeto (0), a vista (1), o buffer dela (2) e a referência à vista: o V8 escreve objeto (0), buffer (1),
        // vista (2), e a referência sai com o número 2, que volta como 1.
        let source = r#"["o",[["v",["V","Uint8Array",["B","\u0001\u0002"],0,2]],["again",["r",1]]]]"#;
        let value = to_v8(&parse(source).expect("texto")).expect("valor");
        let Value::Object(properties) = &value else { panic!("objeto") };
        assert_eq!(properties[1].1, Value::Ref(2));
        assert_eq!(round_trip(source), source);
    }

    #[test]
    fn big_integers_keep_their_digits() {
        assert_eq!(round_trip(r#"["i","18446744073709551616"]"#), r#"["i","18446744073709551616"]"#);
        assert_eq!(round_trip(r#"["i","-1"]"#), r#"["i","-1"]"#);
        assert_eq!(round_trip(r#"["i","0"]"#), r#"["i","0"]"#);
    }

    #[test]
    fn keys_of_every_type_convert() {
        let parsed = parse(r#"["o",[["type",3],["value","ab"]]]"#).expect("chave");
        assert_eq!(idb_key(&parsed), Some(IdbKey::String(units("ab"))));
        let array = parse(r#"["o",[["type",5],["value",["a",2,[["0",["o",[["type",1],["value",1]]]],["1",["o",[["type",2],["value",5]]]]]]]]]"#).expect("chave");
        assert_eq!(idb_key(&array), Some(IdbKey::Array(vec![IdbKey::Number(1.0), IdbKey::Date(5.0)])));
    }
}
