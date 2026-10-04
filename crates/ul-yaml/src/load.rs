// Portado do PyYAML 6.0.2 (yaml/composer.py, yaml/resolver.py e a parte segura de
// yaml/constructor.py), Copyright (c) 2017-2021 Ingy döt Net, Copyright (c) 2006-2016 Kirill
// Simonov, licença MIT, e do carregador do yq 3.4.3 (yq/loader.py), Copyright Andrey Kislyuk,
// licença Apache 2.0. Modificado no pseudo-linus (2026, MIT): Rust seguro; as mensagens do
// compositor são as do `CParser` (Cython).

//! Documentos YAML em valores do Python, como o carregador do yq: gramática 1.2 na resolução
//! implícita (só `true`/`false`, `null`/`~`/vazio, inteiros decimais, `0o` e `0x`, floats), chaves
//! de mesclagem (`<<`) expandidas, construtores do `SafeConstructor` pros tipos explícitos e o
//! `!!binary`/`!!set` tratados como etiqueta desconhecida.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use num_bigint::BigInt;
use num_traits::{Num, Zero};
use sha2::{Digest, Sha224};

use crate::parser::{Ev, Parser};
use crate::py::{Py, make_dict};
use crate::scanner::{Mark, YamlError};

pub const STR: &str = "tag:yaml.org,2002:str";
pub const SEQ: &str = "tag:yaml.org,2002:seq";
pub const MAP: &str = "tag:yaml.org,2002:map";
const MERGE: &str = "tag:yaml.org,2002:merge";

#[derive(Debug)]
pub enum Kind {
    Scalar { value: String, style: Option<char> },
    Seq { items: Vec<NodeRef>, flow: bool },
    Map { pairs: Vec<(NodeRef, NodeRef)>, flow: bool },
}

#[derive(Debug)]
pub struct Node {
    pub tag: String,
    pub kind: Kind,
    pub start: Mark,
    pub end: Mark,
}

pub type NodeRef = Rc<RefCell<Node>>;

type R<T> = Result<T, YamlError>;

// ---- resolução implícita (gramática 1.2 do yq) ----

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn strip_sign(s: &str) -> &str {
    s.strip_prefix(['-', '+']).unwrap_or(s)
}

/// O `$` do `re` do Python também casa antes de um `\n` final.
fn body(s: &str) -> &str {
    s.strip_suffix('\n').unwrap_or(s)
}

fn is_float12(s: &str) -> bool {
    let t = strip_sign(s);
    if matches!(t, ".inf" | ".Inf" | ".INF") {
        return true;
    }
    if matches!(s, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    // (?:\.[0-9]+|[0-9]+(\.[0-9]*)?)(?:[eE][-+]?[0-9]+)?
    let (mant, exp) = match t.find(['e', 'E']) {
        Some(p) => (&t[..p], Some(&t[p + 1..])),
        None => (t, None),
    };
    let mant_ok = match mant.split_once('.') {
        Some(("", f)) => is_digits(f),
        Some((i, f)) => is_digits(i) && (f.is_empty() || is_digits(f)),
        None => is_digits(mant),
    };
    let exp_ok = exp.is_none_or(|e| is_digits(strip_sign(e)));
    mant_ok && exp_ok
}

/// A etiqueta de um escalar sem etiqueta explícita.
pub fn resolve_scalar(value: &str, implicit_plain: bool) -> &'static str {
    if !implicit_plain {
        return STR;
    }
    let v = body(value);
    if matches!(v, "" | "~" | "null" | "Null" | "NULL") {
        return "tag:yaml.org,2002:null";
    }
    let first = v.chars().next().unwrap_or('\0');
    if "tTfF".contains(first) && matches!(v, "true" | "True" | "TRUE" | "false" | "False" | "FALSE") {
        return "tag:yaml.org,2002:bool";
    }
    if "-+0123456789".contains(first) {
        let is_int = v.strip_prefix("0o").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| (b'0'..=b'7').contains(&b)))
            || is_digits(strip_sign(v))
            || v.strip_prefix("0x").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_hexdigit()));
        if is_int {
            return "tag:yaml.org,2002:int";
        }
    }
    if "-+0123456789.".contains(first) && is_float12(v) {
        return "tag:yaml.org,2002:float";
    }
    if v == "<<" {
        return MERGE;
    }
    STR
}

// ---- compositor ----

pub struct Loader {
    parser: Parser,
    anchors: HashMap<String, NodeRef>,
    started: bool,
    /// O `-Y` (anotações de estilo e etiqueta).
    pub annotations: bool,
}

impl Loader {
    pub fn new(text: &str) -> Loader {
        Loader { parser: Parser::new(text), anchors: HashMap::new(), started: false, annotations: false }
    }

    /// Próximo documento, ou `None` no fim.
    pub fn next_document(&mut self) -> R<Option<NodeRef>> {
        if !self.started {
            if matches!(self.parser.peek_event()?.map(|e| &e.ev), Some(Ev::StreamStart)) {
                self.parser.get_event()?;
            }
            self.started = true;
        }
        match self.parser.peek_event()? {
            None | Some(crate::parser::Event { ev: Ev::StreamEnd, .. }) => return Ok(None),
            _ => {}
        }
        self.parser.get_event()?; // DocumentStart
        let node = self.compose_node()?;
        self.parser.get_event()?; // DocumentEnd
        self.anchors.clear();
        Ok(Some(node))
    }

    fn compose_node(&mut self) -> R<NodeRef> {
        let ev = self.parser.peek_event()?.cloned().ok_or_else(|| YamlError::plain("ComposerError", "unexpected end"))?;
        if let Ev::Alias { anchor } = &ev.ev {
            self.parser.get_event()?;
            return match self.anchors.get(anchor) {
                Some(n) => Ok(n.clone()),
                None => Err(YamlError::marked("ComposerError", None, None, "found undefined alias", ev.start)),
            };
        }
        let anchor = match &ev.ev {
            Ev::Scalar { anchor, .. } | Ev::SequenceStart { anchor, .. } | Ev::MappingStart { anchor, .. } => anchor.clone(),
            _ => None,
        };
        if let Some(a) = &anchor
            && let Some(first) = self.anchors.get(a)
        {
            let fm = first.borrow().start;
            return Err(YamlError::marked("ComposerError", Some("found duplicate anchor; first occurrence"), Some(fm), "second occurrence", ev.start));
        }
        self.parser.get_event()?;
        match ev.ev {
            Ev::Scalar { tag, implicit, value, style, .. } => {
                let tag = match tag {
                    Some(t) if t != "!" => t,
                    _ => resolve_scalar(&value, implicit.0).to_string(),
                };
                let node = Rc::new(RefCell::new(Node { tag, kind: Kind::Scalar { value, style }, start: ev.start, end: ev.end }));
                if let Some(a) = anchor {
                    self.anchors.insert(a, node.clone());
                }
                Ok(node)
            }
            Ev::SequenceStart { tag, flow_style, .. } => {
                let tag = match tag {
                    Some(t) if t != "!" => t,
                    _ => SEQ.to_string(),
                };
                let node = Rc::new(RefCell::new(Node {
                    tag,
                    kind: Kind::Seq { items: Vec::new(), flow: flow_style },
                    start: ev.start,
                    end: ev.end,
                }));
                if let Some(a) = anchor {
                    self.anchors.insert(a, node.clone());
                }
                loop {
                    if matches!(self.parser.peek_event()?.map(|e| &e.ev), Some(Ev::SequenceEnd)) {
                        break;
                    }
                    let item = self.compose_node()?;
                    if let Kind::Seq { items, .. } = &mut node.borrow_mut().kind {
                        items.push(item);
                    }
                }
                let end = self.parser.get_event()?.map(|e| e.end).unwrap_or_default();
                node.borrow_mut().end = end;
                Ok(node)
            }
            Ev::MappingStart { tag, flow_style, .. } => {
                let tag = match tag {
                    Some(t) if t != "!" => t,
                    _ => MAP.to_string(),
                };
                let node = Rc::new(RefCell::new(Node {
                    tag,
                    kind: Kind::Map { pairs: Vec::new(), flow: flow_style },
                    start: ev.start,
                    end: ev.end,
                }));
                if let Some(a) = anchor {
                    self.anchors.insert(a, node.clone());
                }
                loop {
                    if matches!(self.parser.peek_event()?.map(|e| &e.ev), Some(Ev::MappingEnd)) {
                        break;
                    }
                    let k = self.compose_node()?;
                    let v = self.compose_node()?;
                    if let Kind::Map { pairs, .. } = &mut node.borrow_mut().kind {
                        pairs.push((k, v));
                    }
                }
                let end = self.parser.get_event()?.map(|e| e.end).unwrap_or_default();
                node.borrow_mut().end = end;
                Ok(node)
            }
            _ => Err(YamlError::plain("ComposerError", "unexpected event")),
        }
    }
}

// ---- construtor ----

pub struct Constructor {
    annotations: bool,
    building: Vec<*const RefCell<Node>>,
    built: Vec<(*const RefCell<Node>, Py)>,
}

fn cerr(context: Option<&str>, context_mark: Option<Mark>, problem: &str, mark: Mark) -> YamlError {
    YamlError::marked("ConstructorError", context, context_mark, problem, mark)
}

/// `repr()` de um texto do Python (aspas simples, ou duplas se houver aspa simples).
pub fn py_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

/// `b64encode(sha224(key))`, a chave das anotações do `-Y`.
pub fn hash_key(key: &str) -> String {
    let digest = Sha224::digest(key.as_bytes());
    base64(&digest)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn style_annotation(node: &Node) -> Option<String> {
    match &node.kind {
        Kind::Scalar { style: Some(s), .. } => Some(s.to_string()),
        Kind::Seq { flow: true, .. } | Kind::Map { flow: true, .. } => Some("flow".to_string()),
        _ => None,
    }
}

fn custom_tag(node: &Node) -> Option<String> {
    let t = &node.tag;
    (t.starts_with('!') && !t.starts_with("!!") && t.len() > 1).then(|| t.clone())
}

impl Constructor {
    pub fn new(annotations: bool) -> Constructor {
        Constructor { annotations, building: Vec::new(), built: Vec::new() }
    }

    pub fn construct_document(&mut self, node: &NodeRef) -> R<Py> {
        let v = self.construct_object(node);
        self.built.clear();
        self.building.clear();
        v
    }

    fn construct_object(&mut self, node: &NodeRef) -> R<Py> {
        let ptr = Rc::as_ptr(node);
        if let Some((_, v)) = self.built.iter().find(|(p, _)| *p == ptr) {
            return Ok(v.clone());
        }
        if self.building.contains(&ptr) {
            let m = node.borrow().start;
            return Err(cerr(None, None, "found unconstructable recursive node", m));
        }
        self.building.push(ptr);
        let tag = node.borrow().tag.clone();
        let v = match tag.as_str() {
            "tag:yaml.org,2002:null" => {
                self.construct_scalar(node)?;
                Py::None
            }
            "tag:yaml.org,2002:bool" => {
                let s = self.construct_scalar(node)?;
                match s.to_lowercase().as_str() {
                    "yes" | "true" | "on" => Py::Bool(true),
                    "no" | "false" | "off" => Py::Bool(false),
                    other => return Err(YamlError::plain("KeyError", py_repr(other))),
                }
            }
            "tag:yaml.org,2002:int" => construct_int(&self.construct_scalar(node)?)?,
            "tag:yaml.org,2002:float" => construct_float(&self.construct_scalar(node)?)?,
            "tag:yaml.org,2002:str" => Py::Str(self.construct_scalar(node)?),
            "tag:yaml.org,2002:timestamp" => {
                let s = self.construct_scalar(node)?;
                construct_timestamp(&s).ok_or_else(|| YamlError::plain("ValueError", format!("invalid timestamp: {}", py_repr(&s))))?
            }
            "tag:yaml.org,2002:omap" | "tag:yaml.org,2002:pairs" => self.construct_pairs(node)?,
            SEQ => self.construct_sequence(node)?,
            MAP => self.construct_mapping(node)?,
            _ => {
                let is_scalar = matches!(node.borrow().kind, Kind::Scalar { .. });
                let is_seq = matches!(node.borrow().kind, Kind::Seq { .. });
                if is_scalar {
                    Py::Str(self.construct_scalar(node)?)
                } else if is_seq {
                    self.construct_sequence(node)?
                } else {
                    self.construct_mapping(node)?
                }
            }
        };
        self.building.retain(|p| *p != ptr);
        self.built.push((ptr, v.clone()));
        Ok(v)
    }

    fn construct_scalar(&mut self, node: &NodeRef) -> R<String> {
        let n = node.borrow();
        match &n.kind {
            Kind::Scalar { value, .. } => Ok(value.clone()),
            Kind::Seq { .. } => Err(cerr(None, None, "expected a scalar node, but found sequence", n.start)),
            Kind::Map { .. } => Err(cerr(None, None, "expected a scalar node, but found mapping", n.start)),
        }
    }

    fn construct_sequence(&mut self, node: &NodeRef) -> R<Py> {
        let items: Vec<NodeRef> = match &node.borrow().kind {
            Kind::Seq { items, .. } => items.clone(),
            Kind::Map { pairs, .. } => pairs.iter().map(|p| p.0.clone()).collect(),
            Kind::Scalar { .. } => Vec::new(),
        };
        let mut out = Vec::new();
        let mut annotations = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if self.annotations {
                let n = item.borrow();
                if let Some(t) = custom_tag(&n) {
                    annotations.push(Py::Str(format!("__yq_tag_{i}_{t}__")));
                }
                if let Some(s) = style_annotation(&n) {
                    annotations.push(Py::Str(format!("__yq_style_{i}_{s}__")));
                }
            }
        }
        for item in &items {
            out.push(self.construct_object(item)?);
        }
        out.extend(annotations);
        Ok(Py::List(out))
    }

    fn construct_pairs(&mut self, node: &NodeRef) -> R<Py> {
        let items: Vec<NodeRef> = match &node.borrow().kind {
            Kind::Seq { items, .. } => items.clone(),
            _ => {
                let m = node.borrow().start;
                return Err(cerr(Some("while constructing an ordered map"), Some(m), "expected a sequence, but found mapping", m));
            }
        };
        let mut out = Vec::new();
        for item in &items {
            let pairs = match &item.borrow().kind {
                Kind::Map { pairs, .. } if pairs.len() == 1 => pairs.clone(),
                _ => {
                    let m = item.borrow().start;
                    return Err(cerr(Some("while constructing an ordered map"), Some(m), "expected a single mapping item", m));
                }
            };
            let k = self.construct_object(&pairs[0].0)?;
            let v = self.construct_object(&pairs[0].1)?;
            out.push(Py::List(vec![k, v]));
        }
        Ok(Py::List(out))
    }

    /// `flatten_mapping` do `SafeConstructor`: expande as chaves `<<`.
    fn flatten_mapping(&mut self, node: &NodeRef) -> R<()> {
        let pairs = match &node.borrow().kind {
            Kind::Map { pairs, .. } => pairs.clone(),
            _ => return Ok(()),
        };
        let mut merge: Vec<(NodeRef, NodeRef)> = Vec::new();
        let mut rest: Vec<(NodeRef, NodeRef)> = Vec::new();
        let node_start = node.borrow().start;
        for (k, v) in pairs {
            let ktag = k.borrow().tag.clone();
            if ktag == MERGE {
                let vkind_map = matches!(v.borrow().kind, Kind::Map { .. });
                let vkind_seq = matches!(v.borrow().kind, Kind::Seq { .. });
                if vkind_map {
                    self.flatten_mapping(&v)?;
                    if let Kind::Map { pairs, .. } = &v.borrow().kind {
                        merge.extend(pairs.iter().cloned());
                    }
                } else if vkind_seq {
                    let subs: Vec<NodeRef> = match &v.borrow().kind {
                        Kind::Seq { items, .. } => items.clone(),
                        _ => Vec::new(),
                    };
                    let mut submerge = Vec::new();
                    for sub in subs {
                        if !matches!(sub.borrow().kind, Kind::Map { .. }) {
                            let (id, m) = (node_id(&sub.borrow()), sub.borrow().start);
                            return Err(cerr(
                                Some("while constructing a mapping"),
                                Some(node_start),
                                &format!("expected a mapping for merging, but found {id}"),
                                m,
                            ));
                        }
                        self.flatten_mapping(&sub)?;
                        if let Kind::Map { pairs, .. } = &sub.borrow().kind {
                            submerge.push(pairs.clone());
                        }
                    }
                    submerge.reverse();
                    for s in submerge {
                        merge.extend(s);
                    }
                } else {
                    let (id, m) = (node_id(&v.borrow()), v.borrow().start);
                    return Err(cerr(
                        Some("while constructing a mapping"),
                        Some(node_start),
                        &format!("expected a mapping or list of mappings for merging, but found {id}"),
                        m,
                    ));
                }
            } else {
                if ktag == "tag:yaml.org,2002:value" {
                    k.borrow_mut().tag = STR.to_string();
                }
                rest.push((k, v));
            }
        }
        if !merge.is_empty() {
            merge.extend(rest);
            rest = merge;
        }
        if let Kind::Map { pairs, .. } = &mut node.borrow_mut().kind {
            *pairs = rest;
        }
        Ok(())
    }

    fn construct_mapping(&mut self, node: &NodeRef) -> R<Py> {
        self.flatten_mapping(node)?;
        let pairs: Vec<(NodeRef, NodeRef)> = match &node.borrow().kind {
            Kind::Map { pairs, .. } => pairs.clone(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for (kn, vn) in &pairs {
            let key = self.construct_object(kn)?;
            let value = self.construct_object(vn)?;
            let key_str = match &key {
                Py::Str(s) => Some(s.clone()),
                _ => None,
            };
            out.push((key, value));
            if !self.annotations {
                continue;
            }
            let Some(ks) = key_str else { continue };
            let v = vn.borrow();
            if let Some(t) = custom_tag(&v) {
                out.push((Py::Str(format!("__yq_tag_{}__", hash_key(&ks))), Py::Str(t)));
            }
            if let Some(s) = style_annotation(&v) {
                out.push((Py::Str(format!("__yq_style_{}__", hash_key(&ks))), Py::Str(s)));
            }
        }
        make_dict(out)
    }
}

fn node_id(n: &Node) -> &'static str {
    match n.kind {
        Kind::Scalar { .. } => "scalar",
        Kind::Seq { .. } => "sequence",
        Kind::Map { .. } => "mapping",
    }
}

/// `int(text, base)` do Python (sem o sinal, que o construtor já tirou).
fn py_int(text: &str, base: u32) -> R<BigInt> {
    let t = text.trim();
    let digits = match base {
        8 => t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")).unwrap_or(t),
        16 => t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t),
        2 => t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")).unwrap_or(t),
        _ => t,
    };
    let ok = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c == '_' || c.is_digit(base));
    if !ok {
        return Err(YamlError::plain("ValueError", format!("invalid literal for int() with base {base}: {}", py_repr(text))));
    }
    Ok(BigInt::from_str_radix(&digits.replace('_', ""), base).unwrap_or_else(|_| BigInt::zero()))
}

fn construct_int(value: &str) -> R<Py> {
    let mut v = value.replace('_', "");
    let mut sign = 1;
    if v.starts_with('-') {
        sign = -1;
    }
    if v.starts_with(['-', '+']) {
        v.remove(0);
    }
    let n = if v == "0" {
        BigInt::zero()
    } else if let Some(r) = v.strip_prefix("0b") {
        py_int(r, 2)?
    } else if let Some(r) = v.strip_prefix("0x") {
        py_int(r, 16)?
    } else if v.starts_with('0') {
        py_int(&v, 8)?
    } else if v.contains(':') {
        let mut total = BigInt::zero();
        let mut base = BigInt::from(1);
        for part in v.split(':').rev() {
            total += py_int(part, 10)? * &base;
            base *= 60;
        }
        total
    } else {
        py_int(&v, 10)?
    };
    Ok(Py::Int(n * sign))
}

fn construct_float(value: &str) -> R<Py> {
    let mut v = value.replace('_', "").to_lowercase();
    let mut sign = 1.0;
    if v.starts_with('-') {
        sign = -1.0;
    }
    if v.starts_with(['-', '+']) {
        v.remove(0);
    }
    if v == ".inf" {
        return Ok(Py::Float(sign * f64::INFINITY));
    }
    if v == ".nan" {
        return Ok(Py::Float(f64::NAN));
    }
    if v.contains(':') {
        let mut total = 0.0;
        let mut base = 1.0;
        for part in v.split(':').rev() {
            let d: f64 = part.trim().parse().map_err(|_| YamlError::plain("ValueError", format!("could not convert string to float: {}", py_repr(part))))?;
            total += d * base;
            base *= 60.0;
        }
        return Ok(Py::Float(sign * total));
    }
    let t = v.trim();
    let valid = !t.is_empty() && !t.starts_with(['+', '-']) || t.len() > 1;
    match t.parse::<f64>() {
        Ok(f) if valid => Ok(Py::Float(sign * f)),
        _ => Err(YamlError::plain("ValueError", format!("could not convert string to float: {}", py_repr(&v)))),
    }
}

/// `!!timestamp`: data (`date`) ou data e hora (`datetime`), no formato do `isoformat()`.
fn construct_timestamp(s: &str) -> Option<Py> {
    let b = s.as_bytes();
    let num = |r: &[u8]| -> Option<u32> { std::str::from_utf8(r).ok()?.parse().ok() };
    if b.len() == 10 && b[4] == b'-' && b[7] == b'-' {
        let (y, m, d) = (num(&b[0..4])?, num(&b[5..7])?, num(&b[8..10])?);
        return Some(Py::Date(format!("{y:04}-{m:02}-{d:02}"), "date"));
    }
    // AAAA-M-D[Tt ]H:MM:SS[.frac][ ][Z|±H[:MM]]
    let (date, rest) = s.split_at(s.find(['T', 't', ' ', '\t'])?);
    let mut parts = date.split('-');
    let y = num(parts.next()?.as_bytes())?;
    let mo = num(parts.next()?.as_bytes())?;
    let d = num(parts.next()?.as_bytes())?;
    let rest = rest.trim_start_matches(['T', 't', ' ', '\t']);
    let tz_at = rest.find(['Z', '+', '-']).or_else(|| rest.find(' '));
    let (time, tz) = match tz_at {
        Some(p) => (rest[..p].trim_end(), rest[p..].trim()),
        None => (rest, ""),
    };
    let (hms, frac) = match time.split_once('.') {
        Some((a, f)) => (a, f),
        None => (time, ""),
    };
    let mut t = hms.split(':');
    let h = num(t.next()?.as_bytes())?;
    let mi = num(t.next()?.as_bytes())?;
    let se = num(t.next()?.as_bytes())?;
    let mut out = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}");
    if !frac.is_empty() {
        let mut f: String = frac.chars().take(6).collect();
        while f.len() < 6 {
            f.push('0');
        }
        if f != "000000" {
            out.push('.');
            out.push_str(&f);
        }
    }
    if tz == "Z" {
        out.push_str("+00:00");
    } else if !tz.is_empty() {
        let sign = &tz[..1];
        let body = &tz[1..];
        let (th, tm) = match body.split_once(':') {
            Some((a, b)) => (num(a.as_bytes())?, num(b.as_bytes())?),
            None => (num(body.as_bytes())?, 0),
        };
        out.push_str(&format!("{sign}{th:02}:{tm:02}"));
    }
    Some(Py::Date(out, "datetime"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(s: &str) -> Vec<String> {
        let mut l = Loader::new(s);
        let mut out = Vec::new();
        while let Some(n) = l.next_document().map_err(|e| e.message("t")).unwrap() {
            let v = Constructor::new(false).construct_document(&n).unwrap();
            let mut j = String::new();
            crate::py::to_json(&v, &mut j).unwrap();
            out.push(j);
        }
        out
    }

    #[test]
    fn scalars_like_yq() {
        assert_eq!(load("a: yes\nb: 012\nc: 0x1F\nd: 1e3\ne: ~\nf: true\n"), vec![
            r#"{"a": "yes", "b": 10, "c": 31, "d": 1000.0, "e": null, "f": true}"#
        ]);
        assert_eq!(load("1: one\ntrue: yes\n"), vec![r#"{"1": "yes"}"#]);
        assert_eq!(load("base: &b {x: 1}\nc:\n  <<: *b\n  y: 2\n"), vec![r#"{"base": {"x": 1}, "c": {"x": 1, "y": 2}}"#]);
    }

    #[test]
    fn hash_key_matches_python() {
        // b64encode(sha224(b"a").digest())
        assert_eq!(hash_key("a"), "q9N1NMfZou+5Rl3pMc1wVf/biHlWOumAeNbW1Q==");
    }
}
