//! Camada YAML "nossa": eventos do `saphyr-parser` 0.1 + resolvedor e construtor que reproduzem o
//! carregador do yq 3.4.3 (medido no oráculo, não suposto):
//!
//! - resolução implícita pelas regex do core schema do YAML 1.2 (booleanos só true/false, sem `yes`,
//!   `0b`, `_`, sexagesimal nem timestamp; hexadecimal sem sinal);
//! - construção de inteiro do PyYAML 1.1: zero à esquerda é octal (`012` = 10) e `08` dá ValueError;
//! - merge key `<<` com a ordem do `flatten_mapping` do PyYAML (lista de mapas entra invertida);
//! - tags explícitas do core (`!!str`, `!!int`, `!!float`, `!!bool`, `!!null`);
//! - BOM no começo do stream ignorado.
//!
//! O emissor do `-y` é o do saphyr (o melhor placar de `-y` entre as crates, ver README).

use saphyr_parser::{Event, Parser, ScalarStyle, Span, Tag};

use super::yaml_layers::{Node, SaphyrLayer, YamlLayer};

pub struct YqResolverLayer;

impl YamlLayer for YqResolverLayer {
    fn name(&self) -> &'static str {
        "jaq + saphyr-parser 0.1 + resolvedor yq nosso"
    }
    fn package(&self) -> &'static str {
        "saphyr-parser"
    }
    fn version(&self) -> &'static str {
        "0.1.0"
    }
    fn load(&self, text: &str) -> Result<Vec<Node>, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        Loader { parser: Parser::new_from_str(text), anchors: Vec::new() }.stream()
    }
    fn emit(&self, node: &Node) -> Result<String, String> {
        SaphyrLayer.emit(node)
    }
}

/// Chave de mapa antes do achatamento: a merge key é marcada à parte.
enum Key {
    Merge,
    Node(Node),
}

struct Loader<'a> {
    parser: Parser<'a, saphyr_parser::StrInput<'a>>,
    anchors: Vec<Option<Node>>,
}

fn at(span: Span) -> String {
    format!("line {}, column {}", span.start.line(), span.start.col() + 1)
}

impl<'a> Loader<'a> {
    fn next(&mut self) -> Result<(Event<'a>, Span), String> {
        match self.parser.next() {
            Some(Ok(ev)) => Ok(ev),
            Some(Err(e)) => Err(format!("ScannerError: {e}")),
            None => Err("fim inesperado do stream".into()),
        }
    }

    fn stream(mut self) -> Result<Vec<Node>, String> {
        let mut docs = Vec::new();
        match self.next()? {
            (Event::StreamStart, _) => {}
            (ev, _) => return Err(format!("evento inesperado {ev:?}")),
        }
        loop {
            match self.next()? {
                (Event::StreamEnd, _) => return Ok(docs),
                (Event::DocumentStart(_), _) => {
                    let ev = self.next()?;
                    docs.push(self.node(ev)?);
                    match self.next()? {
                        (Event::DocumentEnd, _) => {}
                        (ev, _) => return Err(format!("evento inesperado {ev:?}")),
                    }
                }
                (ev, _) => return Err(format!("evento inesperado {ev:?}")),
            }
        }
    }

    fn remember(&mut self, anchor: usize, node: &Node) {
        if anchor == 0 {
            return;
        }
        if self.anchors.len() < anchor {
            self.anchors.resize(anchor, None);
        }
        self.anchors[anchor - 1] = Some(node.clone());
    }

    fn node(&mut self, (ev, span): (Event<'a>, Span)) -> Result<Node, String> {
        match ev {
            Event::Scalar(text, style, anchor, tag) => {
                let node = construct_scalar(&text, style, tag.as_deref())?;
                self.remember(anchor, &node);
                Ok(node)
            }
            Event::SequenceStart(anchor, _) => {
                let mut items = Vec::new();
                loop {
                    let ev = self.next()?;
                    if matches!(ev.0, Event::SequenceEnd) {
                        break;
                    }
                    items.push(self.node(ev)?);
                }
                let node = Node::Seq(items);
                self.remember(anchor, &node);
                Ok(node)
            }
            Event::MappingStart(anchor, _) => {
                let mut pairs: Vec<(Key, Node)> = Vec::new();
                loop {
                    let ev = self.next()?;
                    if matches!(ev.0, Event::MappingEnd) {
                        break;
                    }
                    let key = match &ev.0 {
                        Event::Scalar(t, ScalarStyle::Plain, _, None) if t == "<<" => Key::Merge,
                        Event::Scalar(t, _, _, Some(tag)) if is_core(tag, "merge") && t == "<<" => Key::Merge,
                        _ => Key::Node(self.node(ev)?),
                    };
                    let value_ev = self.next()?;
                    let value = self.node(value_ev)?;
                    pairs.push((key, value));
                }
                let node = Node::Map(flatten_merge(pairs, span)?);
                self.remember(anchor, &node);
                Ok(node)
            }
            Event::Alias(id) => self
                .anchors
                .get(id.wrapping_sub(1))
                .and_then(Option::clone)
                .ok_or_else(|| format!("ComposerError: found undefined alias\n  in {}", at(span))),
            other => Err(format!("evento inesperado {other:?}")),
        }
    }
}

/// `SafeConstructor.flatten_mapping` do PyYAML: os pares vindos de merge entram antes dos pares do
/// próprio mapa; numa lista de mapas, a lista entra de trás pra frente.
fn flatten_merge(pairs: Vec<(Key, Node)>, span: Span) -> Result<Vec<(Node, Node)>, String> {
    let mut merged: Vec<(Node, Node)> = Vec::new();
    let mut own: Vec<(Node, Node)> = Vec::new();
    let err = || format!("ConstructorError: expected a mapping or list of mappings for merging\n  in {}", at(span));
    for (key, value) in pairs {
        match key {
            Key::Node(k) => own.push((k, value)),
            Key::Merge => match value {
                Node::Map(p) => merged.extend(p),
                Node::Seq(items) => {
                    let mut sub = Vec::new();
                    for item in items {
                        match item {
                            Node::Map(p) => sub.push(p),
                            _ => return Err(err()),
                        }
                    }
                    for p in sub.into_iter().rev() {
                        merged.extend(p);
                    }
                }
                _ => return Err(err()),
            },
        }
    }
    merged.extend(own);
    Ok(merged)
}

fn is_core(tag: &Tag, suffix: &str) -> bool {
    tag.is_yaml_core_schema() && tag.suffix == suffix
}

fn construct_scalar(s: &str, style: ScalarStyle, tag: Option<&Tag>) -> Result<Node, String> {
    if let Some(tag) = tag.filter(|t| t.is_yaml_core_schema()) {
        return match tag.suffix.as_str() {
            "str" => Ok(Node::Str(s.to_string())),
            "int" => construct_int(s),
            "float" => construct_float(s).ok_or_else(|| format!("ValueError: could not convert string to float: '{s}'")),
            "bool" => Ok(Node::Bool(s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes") || s.eq_ignore_ascii_case("on"))),
            "null" => Ok(Node::Null),
            _ => Ok(Node::Str(s.to_string())),
        };
    }
    if style != ScalarStyle::Plain {
        return Ok(Node::Str(s.to_string()));
    }
    Ok(match s {
        "" | "~" | "null" | "Null" | "NULL" => Node::Null,
        "true" | "True" | "TRUE" => Node::Bool(true),
        "false" | "False" | "FALSE" => Node::Bool(false),
        _ if is_int_12(s) => construct_int(s)?,
        _ if is_float_12(s) => construct_float(s).unwrap_or_else(|| Node::Str(s.to_string())),
        _ => Node::Str(s.to_string()),
    })
}

/// Regex de inteiro do core schema 1.2: `[-+]?[0-9]+ | 0o[0-7]+ | 0x[0-9a-fA-F]+`.
fn is_int_12(s: &str) -> bool {
    let digits = |t: &str, ok: fn(char) -> bool| !t.is_empty() && t.chars().all(ok);
    if let Some(rest) = s.strip_prefix("0o") {
        return digits(rest, |c| ('0'..='7').contains(&c));
    }
    if let Some(rest) = s.strip_prefix("0x") {
        return digits(rest, |c| c.is_ascii_hexdigit());
    }
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    digits(unsigned, |c| c.is_ascii_digit())
}

/// Regex de float do core schema 1.2 (inclui `.inf`/`.nan`).
fn is_float_12(s: &str) -> bool {
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(s, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let (mant, exp) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    let (int, frac) = match mant.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mant, None),
    };
    let all_digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
    let mant_ok = match frac {
        None => !int.is_empty() && all_digits(int),
        Some(f) => all_digits(int) && all_digits(f) && (!int.is_empty() || !f.is_empty()) && !(int.is_empty() && f.is_empty()),
    } && (frac.is_some() || !int.is_empty());
    // `.5` exige fração; `1.` é aceito.
    let mant_ok = mant_ok && !(int.is_empty() && frac.is_none_or(str::is_empty));
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && all_digits(e)
    });
    mant_ok && exp_ok
}

/// `SafeConstructor.construct_yaml_int` do PyYAML.
fn construct_int(s: &str) -> Result<Node, String> {
    let clean: String = s.chars().filter(|c| *c != '_').collect();
    let (neg, body) = match clean.strip_prefix('-') {
        Some(b) => (true, b.to_string()),
        None => (false, clean.strip_prefix('+').unwrap_or(&clean).to_string()),
    };
    let parse = |digits: &str, radix: u32| -> Option<i128> { i128::from_str_radix(digits, radix).ok() };
    let value = if body == "0" {
        Some(0)
    } else if let Some(h) = body.strip_prefix("0x") {
        parse(h, 16)
    } else if let Some(o) = body.strip_prefix("0o") {
        parse(o, 8)
    } else if let Some(b) = body.strip_prefix("0b") {
        parse(b, 2)
    } else if body.starts_with('0') {
        match parse(&body, 8) {
            Some(v) => Some(v),
            // O PyYAML não anexa posição a esse ValueError (vem do `int()` do Python).
            None => return Err(format!("ValueError: invalid literal for int() with base 8: '{body}'")),
        }
    } else if body.chars().all(|c| c.is_ascii_digit()) {
        // Decimal: mantém o texto (pode passar de 128 bits).
        let digits = body.trim_start_matches('0');
        let digits = if digits.is_empty() { "0" } else { digits };
        return Ok(Node::Int(if neg && digits != "0" { format!("-{digits}") } else { digits.to_string() }));
    } else {
        None
    };
    let v = value.ok_or_else(|| format!("ValueError: invalid literal for int(): '{s}'"))?;
    Ok(Node::Int((if neg { -v } else { v }).to_string()))
}

/// `SafeConstructor.construct_yaml_float` do PyYAML.
fn construct_float(s: &str) -> Option<Node> {
    let clean: String = s.chars().filter(|c| *c != '_').collect::<String>().to_lowercase();
    let (sign, body) = match clean.strip_prefix('-') {
        Some(b) => (-1.0, b.to_string()),
        None => (1.0, clean.strip_prefix('+').unwrap_or(&clean).to_string()),
    };
    let v = match body.as_str() {
        ".inf" => f64::INFINITY,
        ".nan" => f64::NAN,
        b => b.parse::<f64>().ok()?,
    };
    Some(Node::Float(sign * v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(s: &str) -> Vec<Node> {
        YqResolverLayer.load(s).unwrap()
    }

    #[test]
    fn scalars_follow_yq_loader() {
        let doc = &load("a: 012\nb: 0o12\nc: 0x1F\nd: -0x10\ne: 1_000\nf: 1e3\ng: yes\nh: 1:20\ni: 0b101\nj: .5\nk: 1.\n")[0];
        let Node::Map(pairs) = doc else { panic!("mapa") };
        let vals: Vec<&Node> = pairs.iter().map(|(_, v)| v).collect();
        assert_eq!(vals[0], &Node::Int("10".into()));
        assert_eq!(vals[1], &Node::Int("10".into()));
        assert_eq!(vals[2], &Node::Int("31".into()));
        assert_eq!(vals[3], &Node::Str("-0x10".into()));
        assert_eq!(vals[4], &Node::Str("1_000".into()));
        assert_eq!(vals[5], &Node::Float(1000.0));
        assert_eq!(vals[6], &Node::Str("yes".into()));
        assert_eq!(vals[7], &Node::Str("1:20".into()));
        assert_eq!(vals[8], &Node::Str("0b101".into()));
        assert_eq!(vals[9], &Node::Float(0.5));
        assert_eq!(vals[10], &Node::Float(1.0));
        assert!(YqResolverLayer.load("a: 08\n").unwrap_err().contains("base 8: '08'"));
    }

    #[test]
    fn merge_order_matches_pyyaml() {
        let doc = &load("a: &a {x: 1, y: 1}\nb: &b {y: 2, z: 2}\nc:\n  <<: [*a, *b]\n  w: 0\n")[0];
        let Node::Map(pairs) = doc else { panic!("mapa") };
        let Node::Map(c) = &pairs[2].1 else { panic!("mapa c") };
        let keys: Vec<&Node> = c.iter().map(|(k, _)| k).collect();
        let names: Vec<String> = keys.iter().map(|k| format!("{k:?}")).collect();
        assert_eq!(names, ["Str(\"y\")", "Str(\"z\")", "Str(\"x\")", "Str(\"y\")", "Str(\"w\")"]);
    }
}
