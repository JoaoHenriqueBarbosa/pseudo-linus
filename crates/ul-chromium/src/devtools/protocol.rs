//! A validação dos parâmetros de cada comando pelo esquema do protocolo (`data/devtools/protocol.json`), como o
//! `crdtp` do Chromium: a mensagem vira CBOR e os parâmetros são desserializados campo a campo, na ordem em que
//! chegaram. O erro leva o caminho do campo e a posição do símbolo culpado no CBOR dos parâmetros (mapa e lista com o
//! envelope de 7 bytes, texto ASCII em UTF-8 e o resto em UTF-16, inteiro de 32 bits como inteiro e o resto como
//! double), medidos no oráculo.

use std::collections::HashMap;

use super::json::{self, Json};

const PROTOCOL: &str = include_str!("../../data/devtools/protocol.json");

/// O tipo esperado de um campo, já com o `$ref` resolvido.
#[derive(Clone, Debug)]
enum Kind {
    Any,
    String,
    Integer,
    Number,
    Boolean,
    Array(Box<Kind>),
    Object(Vec<Field>),
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    optional: bool,
    kind: Kind,
}

pub struct Schema {
    commands: HashMap<String, Vec<Field>>,
}

fn kind_of(spec: &Json, domain: &str, types: &HashMap<String, Json>, depth: usize) -> Kind {
    if depth > 16 {
        return Kind::Any;
    }
    if let Some(reference) = spec.get("$ref").and_then(Json::as_str) {
        let name = if reference.contains('.') { reference.to_owned() } else { format!("{domain}.{reference}") };
        let owner = name.split('.').next().unwrap_or(domain).to_owned();
        return types.get(&name).map_or(Kind::Any, |t| kind_of(t, &owner, types, depth + 1));
    }
    match spec.get("type").and_then(Json::as_str).unwrap_or("any") {
        "string" | "binary" => Kind::String,
        "integer" => Kind::Integer,
        "number" => Kind::Number,
        "boolean" => Kind::Boolean,
        "array" => Kind::Array(Box::new(spec.get("items").map_or(Kind::Any, |items| kind_of(items, domain, types, depth + 1)))),
        "object" => match spec.get("properties") {
            Some(Json::Array(properties)) => Kind::Object(fields(properties, domain, types, depth + 1)),
            _ => Kind::Any,
        },
        _ => Kind::Any,
    }
}

fn fields(list: &[Json], domain: &str, types: &HashMap<String, Json>, depth: usize) -> Vec<Field> {
    list.iter()
        .map(|p| Field {
            name: p.get("name").and_then(Json::as_str).unwrap_or_default().to_owned(),
            optional: p.get("optional").and_then(Json::as_bool) == Some(true),
            kind: kind_of(p, domain, types, depth),
        })
        .collect()
}

/// Tamanho do envelope (`0xd8 0x18 0x5a` e o comprimento em 4 bytes) que abre mapa e lista.
const ENVELOPE: usize = 7;

fn head_len(n: u64) -> usize {
    match n {
        0..=23 => 1,
        24..=0xFF => 2,
        0x100..=0xFFFF => 3,
        0x1_0000..=0xFFFF_FFFF => 5,
        _ => 9,
    }
}

fn int32(n: f64) -> Option<i32> {
    (n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64).then_some(n as i32)
}

fn text_len(text: &str) -> usize {
    if text.is_ascii() {
        head_len(text.len() as u64) + text.len()
    } else {
        let bytes = text.encode_utf16().count() * 2;
        head_len(bytes as u64) + bytes
    }
}

/// Bytes do valor em CBOR.
fn cbor_len(value: &Json) -> usize {
    match value {
        Json::Null | Json::Bool(_) => 1,
        Json::Number(n) => match int32(*n) {
            Some(i) if i >= 0 => head_len(i as u64),
            Some(i) => head_len((-1 - i as i64) as u64),
            None => 9,
        },
        Json::String(s) => text_len(s),
        Json::Array(items) => ENVELOPE + 1 + items.iter().map(cbor_len).sum::<usize>() + 1,
        Json::Object(entries) => ENVELOPE + 1 + entries.iter().map(|(k, v)| text_len(k) + cbor_len(v)).sum::<usize>() + 1,
    }
}

impl Schema {
    pub fn load() -> Schema {
        let protocol = json::parse(PROTOCOL).expect("protocol.json do oráculo");
        let mut types = HashMap::new();
        let mut raw = Vec::new();
        if let Some(Json::Array(domains)) = protocol.get("domains") {
            for domain in domains {
                let name = domain.get("domain").and_then(Json::as_str).unwrap_or_default().to_owned();
                if let Some(Json::Array(list)) = domain.get("types") {
                    for t in list {
                        types.insert(format!("{name}.{}", t.get("id").and_then(Json::as_str).unwrap_or_default()), t.clone());
                    }
                }
                if let Some(Json::Array(list)) = domain.get("commands") {
                    for c in list {
                        raw.push((name.clone(), c.clone()));
                    }
                }
            }
        }
        let commands = raw
            .into_iter()
            .map(|(domain, command)| {
                let params = match command.get("parameters") {
                    Some(Json::Array(list)) => fields(list, &domain, &types, 0),
                    _ => Vec::new(),
                };
                (format!("{domain}.{}", command.get("name").and_then(Json::as_str).unwrap_or_default()), params)
            })
            .collect();
        Schema { commands }
    }

    pub fn has(&self, method: &str) -> bool {
        self.commands.contains_key(method)
    }

    /// `Err` é o `data` do `-32602 Invalid parameters`.
    pub fn validate(&self, method: &str, params: Option<&Json>) -> Result<(), String> {
        let Some(fields) = self.commands.get(method) else { return Ok(()) };
        match params {
            Some(value @ Json::Object(_)) => check_map(value, fields, "params", 0),
            Some(Json::Array(_)) => Err(format!("Failed to deserialize params - CBOR: map start expected at position {ENVELOPE}")),
            // Sem `params` (ou `null`) o comando só passa quando nenhum campo é obrigatório.
            _ if fields.iter().all(|f| f.optional) => Ok(()),
            _ => Err("Failed to deserialize params - CBOR: map start expected at position 0".to_owned()),
        }
    }
}

/// Confere o mapa que começa em `at` (o envelope) contra `fields`.
fn check_map(value: &Json, fields: &[Field], path: &str, at: usize) -> Result<(), String> {
    let Json::Object(entries) = value else { return Ok(()) };
    let mut position = at + ENVELOPE + 1;
    for (key, item) in entries {
        position += text_len(key);
        if let Some(field) = fields.iter().find(|f| f.name == *key) {
            check(item, &field.kind, &format!("{path}.{key}"), position)?;
        }
        position += cbor_len(item);
    }
    if let Some(missing) = fields.iter().find(|f| !f.optional && !entries.iter().any(|(k, _)| *k == f.name)) {
        return Err(format!("Failed to deserialize {path}.{} - BINDINGS: mandatory field missing at position {position}", missing.name));
    }
    Ok(())
}

fn check(value: &Json, kind: &Kind, path: &str, at: usize) -> Result<(), String> {
    let bindings = |what: &str| Err(format!("Failed to deserialize {path} - BINDINGS: {what} value expected at position {at}"));
    let cbor = |what: &str, offset: usize| Err(format!("Failed to deserialize {path} - CBOR: {what} start expected at position {}", at + offset));
    match (kind, value) {
        (Kind::Any, _) => Ok(()),
        (Kind::String, Json::String(_)) => Ok(()),
        (Kind::String, _) => bindings("string"),
        (Kind::Integer, Json::Number(n)) if int32(*n).is_some() => Ok(()),
        (Kind::Integer, _) => bindings("int32"),
        (Kind::Number, Json::Number(_)) => Ok(()),
        (Kind::Number, _) => bindings("double"),
        (Kind::Boolean, Json::Bool(_)) => Ok(()),
        (Kind::Boolean, _) => bindings("bool"),
        (Kind::Array(item), Json::Array(items)) => {
            let mut position = at + ENVELOPE + 1;
            for element in items {
                check(element, item, path, position)?;
                position += cbor_len(element);
            }
            Ok(())
        }
        // O leitor pula o envelope antes de olhar o byte que abre o contêiner.
        (Kind::Array(_), Json::Object(_)) => cbor("array", ENVELOPE),
        (Kind::Array(_), _) => cbor("array", 0),
        (Kind::Object(fields), Json::Object(_)) => check_map(value, fields, path, at),
        (Kind::Object(_), Json::Array(_)) => cbor("map", ENVELOPE),
        (Kind::Object(_), _) => cbor("map", 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(method: &str, params: &str) -> String {
        let params = json::parse(params).ok();
        Schema::load().validate(method, params.as_ref()).unwrap_err()
    }

    /// Os casos medidos no Chromium 154 do oráculo.
    #[test]
    fn positions_as_the_oracle() {
        let field = |path: &str, what: &str, at: usize| format!("Failed to deserialize params.{path} - BINDINGS: {what} at position {at}");
        assert_eq!(data("Runtime.evaluate", "{}"), field("expression", "mandatory field missing", 8));
        assert_eq!(data("Runtime.evaluate", "{\"expression\":5}"), field("expression", "string value expected", 19));
        assert_eq!(data("Runtime.evaluate", "{\"expression\":\"1\",\"returnByValue\":1}"), field("returnByValue", "bool value expected", 35));
        assert_eq!(data("Runtime.evaluate", "{\"expression\":\"\u{e9}\u{e9}\",\"objectGroup\":5}"), field("objectGroup", "string value expected", 36));
        assert_eq!(data("Runtime.evaluate", "{\"expression\":\"1\",\"contextId\":3000000000}"), field("contextId", "int32 value expected", 31));
        assert_eq!(data("Runtime.evaluate", "{\"objectGroup\":\"g\"}"), field("expression", "mandatory field missing", 22));
        assert_eq!(data("Runtime.callFunctionOn", "{\"arguments\":[{}]}"), field("functionDeclaration", "mandatory field missing", 36));
        assert_eq!(data("Runtime.callFunctionOn", "{\"arguments\":[{\"value\":1},{\"objectId\":5}]}"), field("arguments.objectId", "string value expected", 59));
        assert_eq!(data("Runtime.callFunctionOn", "{\"arguments\":[5]}"), "Failed to deserialize params.arguments - CBOR: map start expected at position 26");
        assert_eq!(
            data("Runtime.callFunctionOn", "{\"functionDeclaration\":\"()=>1\",\"arguments\":{}}"),
            "Failed to deserialize params.arguments - CBOR: array start expected at position 51"
        );
        assert_eq!(data("Runtime.evaluate", "[]"), "Failed to deserialize params - CBOR: map start expected at position 7");
        assert_eq!(Schema::load().validate("Runtime.evaluate", None).unwrap_err(), "Failed to deserialize params - CBOR: map start expected at position 0");
        assert!(Schema::load().validate("Target.getTargets", None).is_ok());
        assert!(Schema::load().validate("Runtime.evaluate", json::parse("{\"zzz\":1,\"expression\":\"1\",\"timeout\":1.5}").ok().as_ref()).is_ok());
    }
}
