//! O JSON do DevTools: valor com a ordem das chaves preservada, o leitor das mensagens que chegam (com a posição do
//! erro como o `JSON: invalid token at position N` do Chromium) e os dois escritores, o compacto das mensagens do
//! WebSocket e o identado de três espaços do `base::JSONWriter` que os endpoints HTTP usam.

/// Um valor JSON. O objeto guarda as chaves na ordem em que chegaram ou foram postas.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// O campo `key` de um objeto.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// O número inteiro que cabe num `int` do Chromium (o `id` das mensagens).
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Json::Number(n) if n.fract() == 0.0 && n.abs() <= i32::MAX as f64 => Some(*n as i64),
            _ => None,
        }
    }

    /// O número, inteiro ou não.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Objeto a partir de pares, na ordem dada.
    pub fn object<const N: usize>(fields: [(&str, Json); N]) -> Json {
        Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    pub fn str(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }

    /// A forma compacta, a das mensagens do protocolo.
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        write_compact(self, &mut out);
        out
    }

    /// A forma do `base::JSONWriter::WriteWithOptions(kOptionsPrettyPrint)`: chaves em ordem alfabética, três
    /// espaços por nível, lista de objetos como `[ {` e `}, {`, e quebra de linha no fim.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        write_pretty(self, 0, &mut out);
        out.push('\n');
        out
    }
}

fn write_number(n: f64, out: &mut String) {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        out.push_str(&format!("{}", n as i64));
    } else {
        out.push_str(&format!("{n}"));
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_compact(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => write_number(*n, out),
        Json::String(s) => write_string(s, out),
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Json::Object(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_compact(item, out);
            }
            out.push('}');
        }
    }
}

fn indent(level: usize, out: &mut String) {
    out.push_str(&"   ".repeat(level));
}

fn write_pretty(value: &Json, level: usize, out: &mut String) {
    match value {
        Json::Array(items) => {
            out.push('[');
            if !items.is_empty() {
                out.push(' ');
            }
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_pretty(item, level, out);
            }
            if !items.is_empty() {
                out.push(' ');
            }
            out.push(']');
        }
        Json::Object(fields) => {
            let mut sorted: Vec<&(String, Json)> = fields.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push_str("{\n");
            for (i, (key, item)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                indent(level + 1, out);
                write_string(key, out);
                out.push_str(": ");
                write_pretty(item, level + 1, out);
            }
            out.push('\n');
            indent(level, out);
            out.push('}');
        }
        other => write_compact(other, out),
    }
}

/// Lê um documento JSON inteiro. `Err` traz a posição (em bytes) do primeiro símbolo inválido.
pub fn parse(text: &str) -> Result<Json, usize> {
    let mut reader = Reader { bytes: text.as_bytes(), at: 0 };
    reader.space();
    let value = reader.value()?;
    reader.space();
    if reader.at != reader.bytes.len() {
        return Err(reader.at);
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.at < self.bytes.len() && matches!(self.bytes[self.at], b' ' | b'\t' | b'\n' | b'\r') {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, usize> {
        if self.bytes[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.at)
        }
    }

    fn value(&mut self) -> Result<Json, usize> {
        match self.bytes.get(self.at) {
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    self.space();
                    items.push(self.value()?);
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(self.at),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut fields = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Object(fields));
                }
                loop {
                    self.space();
                    if self.bytes.get(self.at) != Some(&b'"') {
                        return Err(self.at);
                    }
                    let key = self.string()?;
                    self.space();
                    if self.bytes.get(self.at) != Some(&b':') {
                        return Err(self.at);
                    }
                    self.at += 1;
                    self.space();
                    let value = self.value()?;
                    fields.push((key, value));
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(fields));
                        }
                        _ => return Err(self.at),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.at),
        }
    }

    fn number(&mut self) -> Result<Json, usize> {
        let start = self.at;
        while self.at < self.bytes.len() && matches!(self.bytes[self.at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at]).ok().and_then(|t| t.parse().ok()).map(Json::Number).ok_or(start)
    }

    fn hex4(&mut self) -> Result<u32, usize> {
        let digits = self.bytes.get(self.at..self.at + 4).ok_or(self.at)?;
        let value = std::str::from_utf8(digits).ok().and_then(|d| u32::from_str_radix(d, 16).ok()).ok_or(self.at)?;
        self.at += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, usize> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while self.at < self.bytes.len() && !matches!(self.bytes[self.at], b'"' | b'\\') {
                self.at += 1;
            }
            out.push_str(std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| start)?);
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = *self.bytes.get(self.at).ok_or(self.at)?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&high) && self.bytes[self.at..].starts_with(b"\\u") {
                                self.at += 2;
                                let low = self.hex4()?;
                                0x10000 + ((high - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF)
                            } else {
                                high
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.at - 1),
                    }
                }
                _ => return Err(self.at),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_matches_the_json_writer() {
        let value = Json::Array(vec![Json::object([("b", Json::str("x")), ("a", Json::str(""))])]);
        assert_eq!(value.to_pretty(), "[ {\n   \"a\": \"\",\n   \"b\": \"x\"\n} ]\n");
    }

    #[test]
    fn invalid_token_position() {
        assert_eq!(parse("not json"), Err(0));
        assert_eq!(parse("{\"a\": }"), Err(6));
        assert_eq!(parse(" {\"id\":1,\"method\":\"X\"} ").unwrap().get("id").and_then(Json::as_int), Some(1));
    }
}
