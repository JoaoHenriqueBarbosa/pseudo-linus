// Portado do PyYAML 6.0.2 (yaml/emitter.py, yaml/serializer.py e a parte segura de
// yaml/representer.py), Copyright (c) 2017-2021 Ingy döt Net, Copyright (c) 2006-2016 Kirill
// Simonov, licença MIT, e do `dumper.py` do yq 3.4.3, Copyright Andrey Kislyuk, licença Apache 2.0.
// Modificado no pseudo-linus (2026, MIT): Rust seguro; os estados do emissor viram um enum e os
// eventos de um documento ficam todos à mão (o emissor do Python espera até ter o lookahead que
// precisa, o que dá no mesmo).

//! Valores do Python em YAML, como o `yaml.dump_all` que o yq chama no `-y`/`-Y`: estilo de bloco,
//! unicode liberado, largura 80, listas indentadas dentro de mapas (o `OrderedDumper` do yq), e a
//! escolha de estilo de cada escalar pelo resolvedor YAML 1.1 do yq (o que viraria outro tipo sem
//! aspas sai entre aspas).

use resolve::resolve_implicit;

use crate::load::{MAP, SEQ, STR, hash_key};
use crate::py::{Py, float_repr};

/// Opções do `dump_all` que o yq usa.
#[derive(Clone, Debug)]
pub struct DumpOptions {
    pub width: Option<i64>,
    pub indentless: bool,
    pub explicit_start: bool,
    pub explicit_end: bool,
    pub annotations: bool,
    /// `--yaml-output-grammar-version` (1.1 é o padrão).
    pub grammar12: bool,
}

// ---- resolvedores da saída (os do yq) ----

mod resolve {
    /// Os resolvedores implícitos que o yq instala no dumper.
    pub fn resolve_implicit(value: &str, grammar12: bool) -> Option<&'static str> {
        let v = value.strip_suffix('\n').unwrap_or(value);
        let first = v.chars().next().unwrap_or('\0');
        if grammar12 {
            if "tTfF".contains(first) && matches!(v, "true" | "True" | "TRUE" | "false" | "False" | "FALSE") {
                return Some("tag:yaml.org,2002:bool");
            }
            if "-+0123456789".contains(first) && int12(v) {
                return Some("tag:yaml.org,2002:int");
            }
            if "-+0123456789.".contains(first) && float12(v) {
                return Some("tag:yaml.org,2002:float");
            }
            if v.is_empty() || matches!(v, "~" | "null" | "Null" | "NULL") {
                return Some("tag:yaml.org,2002:null");
            }
        } else {
            if "yYnNtTfFoO".contains(first)
                && matches!(
                    v,
                    "yes" | "Yes" | "YES" | "no" | "No" | "NO" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE" | "on"
                        | "On" | "ON" | "off" | "Off" | "OFF"
                )
            {
                return Some("tag:yaml.org,2002:bool");
            }
            if "-+0123456789.".contains(first) && float11(v) {
                return Some("tag:yaml.org,2002:float");
            }
            if "-+0123456789".contains(first) && int11(v) {
                return Some("tag:yaml.org,2002:int");
            }
            if v.is_empty() || matches!(v, "~" | "null" | "Null" | "NULL") {
                return Some("tag:yaml.org,2002:null");
            }
            if first.is_ascii_digit() && timestamp11(v) {
                return Some("tag:yaml.org,2002:timestamp");
            }
            if v == "=" {
                return Some("tag:yaml.org,2002:value");
            }
        }
        if v == "<<" {
            return Some("tag:yaml.org,2002:merge");
        }
        None
    }

    fn sign(s: &str) -> &str {
        s.strip_prefix(['-', '+']).unwrap_or(s)
    }

    fn all(s: &str, f: impl Fn(char) -> bool) -> bool {
        s.chars().all(f)
    }

    fn digits(s: &str) -> bool {
        !s.is_empty() && all(s, |c| c.is_ascii_digit())
    }

    fn int12(v: &str) -> bool {
        v.strip_prefix("0o").is_some_and(|r| !r.is_empty() && all(r, |c| ('0'..='7').contains(&c)))
            || digits(sign(v))
            || v.strip_prefix("0x").is_some_and(|r| !r.is_empty() && all(r, |c| c.is_ascii_hexdigit()))
    }

    fn float12(v: &str) -> bool {
        let t = sign(v);
        if matches!(t, ".inf" | ".Inf" | ".INF") || matches!(v, ".nan" | ".NaN" | ".NAN") {
            return true;
        }
        let (m, e) = match t.find(['e', 'E']) {
            Some(p) => (&t[..p], Some(&t[p + 1..])),
            None => (t, None),
        };
        let m_ok = match m.split_once('.') {
            Some(("", f)) => digits(f),
            Some((i, f)) => digits(i) && (f.is_empty() || digits(f)),
            None => digits(m),
        };
        m_ok && e.is_none_or(|e| digits(sign(e)))
    }

    /// `[0-9][0-9_]*` etc.: dígito seguido de dígitos e `_`.
    fn dig_us(s: &str) -> bool {
        let mut c = s.chars();
        c.next().is_some_and(|f| f.is_ascii_digit()) && all(c.as_str(), |x| x.is_ascii_digit() || x == '_')
    }

    fn us_digits(s: &str) -> bool {
        all(s, |x| x.is_ascii_digit() || x == '_')
    }

    /// `(?::[0-5]?[0-9])+`
    fn sexa_tail(s: &str) -> bool {
        if !s.starts_with(':') {
            return false;
        }
        s[1..].split(':').all(|p| match p.len() {
            1 => p.as_bytes()[0].is_ascii_digit(),
            2 => (b'0'..=b'5').contains(&p.as_bytes()[0]) && p.as_bytes()[1].is_ascii_digit(),
            _ => false,
        })
    }

    fn exp11(e: &str) -> bool {
        // [eE][-+][0-9]+
        let mut c = e.chars();
        matches!(c.next(), Some('e' | 'E')) && matches!(c.next(), Some('-' | '+')) && digits(c.as_str())
    }

    fn float11(v: &str) -> bool {
        let t = sign(v);
        if matches!(t, ".inf" | ".Inf" | ".INF") || matches!(v, ".nan" | ".NaN" | ".NAN") {
            return true;
        }
        // [-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?
        if let Some((i, rest)) = t.split_once('.') {
            let (f, e) = match rest.find(['e', 'E']) {
                Some(p) => (&rest[..p], Some(&rest[p..])),
                None => (rest, None),
            };
            if dig_us(i) && us_digits(f) && e.is_none_or(exp11) {
                return true;
            }
            // \.[0-9_]+(?:[eE][-+][0-9]+)?  (sem sinal)
            if i.is_empty() && v == t && !f.is_empty() && us_digits(f) && e.is_none_or(exp11) {
                return true;
            }
        }
        // [-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
        if let Some(colon) = t.find(':')
            && let Some(dot) = t.rfind('.')
            && dot > colon
        {
            return dig_us(&t[..colon]) && sexa_tail(&t[colon..dot]) && us_digits(&t[dot + 1..]);
        }
        false
    }

    fn int11(v: &str) -> bool {
        let t = sign(v);
        if let Some(r) = t.strip_prefix("0b") {
            return !r.is_empty() && all(r, |c| c == '0' || c == '1' || c == '_');
        }
        if let Some(r) = t.strip_prefix("0x") {
            return !r.is_empty() && all(r, |c| c.is_ascii_hexdigit() || c == '_');
        }
        if let Some(r) = t.strip_prefix('0') {
            return r.is_empty() || all(r, |c| c.is_ascii_digit() || c == '_');
        }
        if t.starts_with(|c: char| ('1'..='9').contains(&c)) {
            if let Some(colon) = t.find(':') {
                return dig_us(&t[..colon]) && sexa_tail(&t[colon..]);
            }
            return dig_us(t);
        }
        false
    }

    fn timestamp11(v: &str) -> bool {
        let b = v.as_bytes();
        let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
        if b.len() == 10 && d(0) && d(1) && d(2) && d(3) && b[4] == b'-' && d(5) && d(6) && b[7] == b'-' && d(8) && d(9) {
            return true;
        }
        // AAAA-M?M-D?D(T|t|[ \t]+)H?H:MM:SS(.frac)?([ \t]*(Z|[-+]H?H(:MM)?))?
        let mut i = 0;
        let take_digits = |i: &mut usize, min: usize, max: usize| -> bool {
            let s = *i;
            while *i < b.len() && b[*i].is_ascii_digit() && *i - s < max {
                *i += 1;
            }
            *i - s >= min
        };
        let lit = |i: &mut usize, c: u8| -> bool {
            if b.get(*i) == Some(&c) {
                *i += 1;
                true
            } else {
                false
            }
        };
        if !(take_digits(&mut i, 4, 4) && lit(&mut i, b'-') && take_digits(&mut i, 1, 2) && lit(&mut i, b'-') && take_digits(&mut i, 1, 2)) {
            return false;
        }
        if matches!(b.get(i), Some(b'T' | b't')) {
            i += 1;
        } else {
            let s = i;
            while matches!(b.get(i), Some(b' ' | b'\t')) {
                i += 1;
            }
            if i == s {
                return false;
            }
        }
        if !(take_digits(&mut i, 1, 2) && lit(&mut i, b':') && take_digits(&mut i, 2, 2) && lit(&mut i, b':') && take_digits(&mut i, 2, 2)) {
            return false;
        }
        if lit(&mut i, b'.') {
            take_digits(&mut i, 0, usize::MAX);
        }
        let save = i;
        while matches!(b.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        if lit(&mut i, b'Z') {
            return i == b.len();
        }
        if matches!(b.get(i), Some(b'-' | b'+')) {
            i += 1;
            if !take_digits(&mut i, 1, 2) {
                return false;
            }
            if lit(&mut i, b':') && !take_digits(&mut i, 2, 2) {
                return false;
            }
            return i == b.len();
        }
        save == b.len()
    }
}

// ---- nós e representador ----

#[derive(Clone, Debug)]
enum DNode {
    Scalar { tag: String, value: String, style: Option<char> },
    Seq { tag: String, items: Vec<DNode>, flow: bool },
    Map { tag: String, pairs: Vec<(DNode, DNode)>, flow: bool },
}

impl DNode {
    fn set_style(&mut self, style: &str) {
        match self {
            DNode::Scalar { style: s, .. } => *s = style.chars().next(),
            DNode::Seq { flow, .. } | DNode::Map { flow, .. } => {
                if style == "flow" {
                    *flow = true;
                }
            }
        }
    }

    fn set_tag(&mut self, t: &str) {
        match self {
            DNode::Scalar { tag, .. } | DNode::Seq { tag, .. } | DNode::Map { tag, .. } => *tag = t.to_string(),
        }
    }
}

fn scalar(tag: &str, value: String) -> DNode {
    DNode::Scalar { tag: tag.to_string(), value, style: None }
}

fn represent(v: &Py, ann: bool) -> DNode {
    match v {
        Py::None => scalar("tag:yaml.org,2002:null", "null".to_string()),
        Py::Bool(b) => scalar("tag:yaml.org,2002:bool", (if *b { "true" } else { "false" }).to_string()),
        Py::Int(i) => scalar("tag:yaml.org,2002:int", i.to_string()),
        Py::Float(f) => {
            let value = if f.is_nan() {
                ".nan".to_string()
            } else if f.is_infinite() {
                (if *f > 0.0 { ".inf" } else { "-.inf" }).to_string()
            } else {
                let r = float_repr(*f).to_lowercase();
                if !r.contains('.') && r.contains('e') { r.replacen('e', ".0e", 1) } else { r }
            };
            scalar("tag:yaml.org,2002:float", value)
        }
        Py::Str(s) | Py::Date(s, _) => scalar(STR, s.clone()),
        Py::List(items) => {
            let mut raw = Vec::new();
            let mut styles: Vec<(usize, String)> = Vec::new();
            let mut tags: Vec<(usize, String)> = Vec::new();
            for it in items {
                if ann && let Py::Str(s) = it && let Some((kind, idx, val)) = item_annotation(s) {
                    if kind == "style" {
                        styles.push((idx, val));
                    } else {
                        tags.push((idx, val));
                    }
                    continue;
                }
                raw.push(represent(it, ann));
            }
            for (i, node) in raw.iter_mut().enumerate() {
                if let Some((_, s)) = styles.iter().rev().find(|(k, _)| *k == i) {
                    node.set_style(s);
                }
                if let Some((_, t)) = tags.iter().rev().find(|(k, _)| *k == i) {
                    node.set_tag(t);
                }
            }
            DNode::Seq { tag: SEQ.to_string(), items: raw, flow: false }
        }
        Py::Dict(pairs) => {
            let mut out = Vec::new();
            let mut styles: Vec<(String, String)> = Vec::new();
            let mut tags: Vec<(String, String)> = Vec::new();
            for (k, x) in pairs {
                if ann && let Py::Str(ks) = k {
                    if ks == "__yq_alias__" {
                        continue;
                    }
                    if let Some((kind, key)) = value_annotation(ks) {
                        let val = match x {
                            Py::Str(s) => s.clone(),
                            other => format!("{other:?}"),
                        };
                        if kind == "style" {
                            styles.push((key, val));
                        } else {
                            tags.push((key, val));
                        }
                        continue;
                    }
                }
                out.push((represent(k, ann), represent(x, ann)));
            }
            if ann {
                for (k, v) in out.iter_mut() {
                    let DNode::Scalar { value, .. } = k else { continue };
                    let h = hash_key(value);
                    if let Some((_, s)) = styles.iter().rev().find(|(kk, _)| *kk == h) {
                        if matches!(v, DNode::Scalar { .. }) {
                            v.set_style(s);
                        } else if s == "flow" {
                            v.set_style("flow");
                        }
                    }
                    if let Some((_, t)) = tags.iter().rev().find(|(kk, _)| *kk == h) {
                        v.set_tag(t);
                    }
                }
            }
            DNode::Map { tag: MAP.to_string(), pairs: out, flow: false }
        }
    }
}

/// `^__yq_(tag|style)_(.+)__$`
fn value_annotation(s: &str) -> Option<(&'static str, String)> {
    let r = s.strip_prefix("__yq_")?.strip_suffix("__")?;
    for kind in ["tag", "style"] {
        if let Some(k) = r.strip_prefix(kind).and_then(|x| x.strip_prefix('_'))
            && !k.is_empty()
        {
            return Some((if kind == "tag" { "tag" } else { "style" }, k.to_string()));
        }
    }
    None
}

/// `^__yq_(tag|style)_(\d+)_(.+)__$`
fn item_annotation(s: &str) -> Option<(&'static str, usize, String)> {
    let r = s.strip_prefix("__yq_")?.strip_suffix("__")?;
    for kind in ["tag", "style"] {
        if let Some(k) = r.strip_prefix(kind).and_then(|x| x.strip_prefix('_')) {
            let digits: String = k.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                continue;
            }
            let rest = k[digits.len()..].strip_prefix('_')?;
            if rest.is_empty() {
                continue;
            }
            return Some((if kind == "tag" { "tag" } else { "style" }, digits.parse().ok()?, rest.to_string()));
        }
    }
    None
}

// ---- eventos (serializador) ----

#[derive(Clone, Debug)]
enum E {
    StreamStart,
    StreamEnd,
    DocStart { explicit: bool },
    DocEnd { explicit: bool },
    Scalar { tag: String, implicit: (bool, bool), value: String, style: Option<char> },
    SeqStart { tag: String, implicit: bool, flow: bool },
    SeqEnd,
    MapStart { tag: String, implicit: bool, flow: bool },
    MapEnd,
}

fn serialize(node: &DNode, grammar12: bool, out: &mut Vec<E>) {
    match node {
        DNode::Scalar { tag, value, style } => {
            let detected = resolve_implicit(value, grammar12).unwrap_or(STR);
            let implicit = (tag == detected, tag == STR);
            out.push(E::Scalar { tag: tag.clone(), implicit, value: value.clone(), style: *style });
        }
        DNode::Seq { tag, items, flow } => {
            out.push(E::SeqStart { tag: tag.clone(), implicit: tag == SEQ, flow: *flow });
            for i in items {
                serialize(i, grammar12, out);
            }
            out.push(E::SeqEnd);
        }
        DNode::Map { tag, pairs, flow } => {
            out.push(E::MapStart { tag: tag.clone(), implicit: tag == MAP, flow: *flow });
            for (k, v) in pairs {
                serialize(k, grammar12, out);
                serialize(v, grammar12, out);
            }
            out.push(E::MapEnd);
        }
    }
}

/// `yaml.dump_all(docs, ...)` com o dumper do yq.
pub fn dump_all(docs: &[Py], opts: &DumpOptions) -> String {
    let mut events = vec![E::StreamStart];
    for d in docs {
        events.push(E::DocStart { explicit: opts.explicit_start });
        serialize(&represent(d, opts.annotations), opts.grammar12, &mut events);
        events.push(E::DocEnd { explicit: opts.explicit_end });
    }
    events.push(E::StreamEnd);
    let mut em = Emitter::new(opts);
    em.run(&events);
    em.out
}

// ---- emissor ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    StreamStart,
    FirstDocumentStart,
    DocumentStart,
    DocumentEnd,
    DocumentRoot,
    Nothing,
    FirstFlowSequenceItem,
    FlowSequenceItem,
    FirstFlowMappingKey,
    FlowMappingKey,
    FlowMappingSimpleValue,
    FlowMappingValue,
    FirstBlockSequenceItem,
    BlockSequenceItem,
    FirstBlockMappingKey,
    BlockMappingKey,
    BlockMappingSimpleValue,
    BlockMappingValue,
}

struct Analysis {
    scalar: Vec<char>,
    empty: bool,
    multiline: bool,
    allow_flow_plain: bool,
    allow_block_plain: bool,
    allow_single_quoted: bool,
    allow_block: bool,
}

fn is_space_or_break_z(c: char) -> bool {
    matches!(c, '\0' | ' ' | '\t' | '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_brk(c: char) -> bool {
    matches!(c, '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn analyze_scalar(s: &str) -> Analysis {
    let scalar: Vec<char> = s.chars().collect();
    if scalar.is_empty() {
        return Analysis {
            scalar,
            empty: true,
            multiline: false,
            allow_flow_plain: false,
            allow_block_plain: true,
            allow_single_quoted: true,
            allow_block: false,
        };
    }
    let mut block_indicators = false;
    let mut flow_indicators = false;
    let mut line_breaks = false;
    let mut special_characters = false;
    let mut leading_space = false;
    let mut leading_break = false;
    let mut trailing_space = false;
    let mut trailing_break = false;
    let mut break_space = false;
    let mut space_break = false;
    if s.starts_with("---") || s.starts_with("...") {
        block_indicators = true;
        flow_indicators = true;
    }
    let mut preceded_by_whitespace = true;
    let mut followed_by_whitespace = scalar.len() == 1 || is_space_or_break_z(scalar[1]);
    let mut previous_space = false;
    let mut previous_break = false;
    let n = scalar.len();
    for index in 0..n {
        let ch = scalar[index];
        if index == 0 {
            if "#,[]{}&*!|>'\"%@`".contains(ch) {
                flow_indicators = true;
                block_indicators = true;
            }
            if ch == '?' || ch == ':' {
                flow_indicators = true;
                if followed_by_whitespace {
                    block_indicators = true;
                }
            }
            if ch == '-' && followed_by_whitespace {
                flow_indicators = true;
                block_indicators = true;
            }
        } else {
            if ",?[]{}".contains(ch) {
                flow_indicators = true;
            }
            if ch == ':' {
                flow_indicators = true;
                if followed_by_whitespace {
                    block_indicators = true;
                }
            }
            if ch == '#' && preceded_by_whitespace {
                flow_indicators = true;
                block_indicators = true;
            }
        }
        if is_brk(ch) {
            line_breaks = true;
        }
        if !(ch == '\n' || (' '..='~').contains(&ch)) {
            let printable = ch == '\u{85}'
                || ('\u{a0}'..='\u{d7ff}').contains(&ch)
                || ('\u{e000}'..='\u{fffd}').contains(&ch)
                || ('\u{10000}'..'\u{10ffff}').contains(&ch);
            if !(printable && ch != '\u{feff}') {
                special_characters = true;
            }
        }
        if ch == ' ' {
            if index == 0 {
                leading_space = true;
            }
            if index == n - 1 {
                trailing_space = true;
            }
            if previous_break {
                break_space = true;
            }
            previous_space = true;
            previous_break = false;
        } else if is_brk(ch) {
            if index == 0 {
                leading_break = true;
            }
            if index == n - 1 {
                trailing_break = true;
            }
            if previous_space {
                space_break = true;
            }
            previous_space = false;
            previous_break = true;
        } else {
            previous_space = false;
            previous_break = false;
        }
        preceded_by_whitespace = is_space_or_break_z(ch);
        followed_by_whitespace = index + 2 >= n || is_space_or_break_z(scalar[index + 2]);
    }
    let mut allow_flow_plain = true;
    let mut allow_block_plain = true;
    let mut allow_single_quoted = true;
    let mut allow_block = true;
    if leading_space || leading_break || trailing_space || trailing_break {
        allow_flow_plain = false;
        allow_block_plain = false;
    }
    if trailing_space {
        allow_block = false;
    }
    if break_space {
        allow_flow_plain = false;
        allow_block_plain = false;
        allow_single_quoted = false;
    }
    if space_break || special_characters {
        allow_flow_plain = false;
        allow_block_plain = false;
        allow_single_quoted = false;
        allow_block = false;
    }
    if line_breaks {
        allow_flow_plain = false;
        allow_block_plain = false;
    }
    if flow_indicators {
        allow_flow_plain = false;
    }
    if block_indicators {
        allow_block_plain = false;
    }
    Analysis { scalar, empty: false, multiline: line_breaks, allow_flow_plain, allow_block_plain, allow_single_quoted, allow_block }
}

struct Emitter {
    out: String,
    states: Vec<St>,
    state: St,
    events: Vec<E>,
    pos: usize,
    indents: Vec<Option<usize>>,
    indent: Option<usize>,
    flow_level: usize,
    root_context: bool,
    mapping_context: bool,
    simple_key_context: bool,
    column: usize,
    whitespace: bool,
    indention: bool,
    open_ended: bool,
    best_indent: usize,
    best_width: usize,
    force_indent: bool,
    analysis: Option<Analysis>,
    style: Option<char>,
    /// Estilo escolhido = plain (o `''` do Python).
    style_plain: bool,
    style_chosen: bool,
}

impl Emitter {
    fn new(opts: &DumpOptions) -> Emitter {
        let best_indent = 2;
        let best_width = match opts.width {
            Some(w) if w > (best_indent * 2) as i64 => w as usize,
            _ => 80,
        };
        Emitter {
            out: String::new(),
            states: Vec::new(),
            state: St::StreamStart,
            events: Vec::new(),
            pos: 0,
            indents: Vec::new(),
            indent: None,
            flow_level: 0,
            root_context: false,
            mapping_context: false,
            simple_key_context: false,
            column: 0,
            whitespace: true,
            indention: true,
            open_ended: false,
            best_indent,
            best_width,
            force_indent: !opts.indentless,
            analysis: None,
            style: None,
            style_plain: false,
            style_chosen: false,
        }
    }

    fn run(&mut self, events: &[E]) {
        self.events = events.to_vec();
        while self.pos < self.events.len() {
            self.step();
            self.pos += 1;
        }
    }

    fn ev(&self) -> &E {
        &self.events[self.pos]
    }

    fn next_ev(&self) -> Option<&E> {
        self.events.get(self.pos + 1)
    }

    fn pop_state(&mut self) -> St {
        self.states.pop().unwrap_or(St::Nothing)
    }

    fn increase_indent(&mut self, flow: bool, indentless: bool) {
        let indentless = indentless && !self.force_indent;
        self.indents.push(self.indent);
        match self.indent {
            None => self.indent = Some(if flow { self.best_indent } else { 0 }),
            Some(i) if !indentless => self.indent = Some(i + self.best_indent),
            _ => {}
        }
    }

    fn step(&mut self) {
        match self.state {
            St::StreamStart => self.state = St::FirstDocumentStart,
            St::FirstDocumentStart => self.expect_document_start(true),
            St::DocumentStart => self.expect_document_start(false),
            St::DocumentEnd => {
                if let E::DocEnd { explicit } = *self.ev() {
                    self.write_indent();
                    if explicit {
                        self.write_indicator("...", true, false, false);
                        self.write_indent();
                    }
                }
                self.state = St::DocumentStart;
            }
            St::DocumentRoot => {
                self.states.push(St::DocumentEnd);
                self.expect_node(true, false, false);
            }
            St::Nothing => {}
            St::FirstFlowSequenceItem | St::FlowSequenceItem => {
                let first = self.state == St::FirstFlowSequenceItem;
                if matches!(self.ev(), E::SeqEnd) {
                    self.indent = self.indents.pop().unwrap_or(None);
                    self.flow_level -= 1;
                    self.write_indicator("]", false, false, false);
                    self.state = self.pop_state();
                } else {
                    if !first {
                        self.write_indicator(",", false, false, false);
                    }
                    if self.column > self.best_width {
                        self.write_indent();
                    }
                    self.states.push(St::FlowSequenceItem);
                    self.expect_node(false, false, false);
                }
            }
            St::FirstFlowMappingKey | St::FlowMappingKey => {
                let first = self.state == St::FirstFlowMappingKey;
                if matches!(self.ev(), E::MapEnd) {
                    self.indent = self.indents.pop().unwrap_or(None);
                    self.flow_level -= 1;
                    self.write_indicator("}", false, false, false);
                    self.state = self.pop_state();
                } else {
                    if !first {
                        self.write_indicator(",", false, false, false);
                    }
                    if self.column > self.best_width {
                        self.write_indent();
                    }
                    if self.check_simple_key() {
                        self.states.push(St::FlowMappingSimpleValue);
                        self.expect_node(false, true, true);
                    } else {
                        self.write_indicator("?", true, false, false);
                        self.states.push(St::FlowMappingValue);
                        self.expect_node(false, true, false);
                    }
                }
            }
            St::FlowMappingSimpleValue => {
                self.write_indicator(":", false, false, false);
                self.states.push(St::FlowMappingKey);
                self.expect_node(false, true, false);
            }
            St::FlowMappingValue => {
                if self.column > self.best_width {
                    self.write_indent();
                }
                self.write_indicator(":", true, false, false);
                self.states.push(St::FlowMappingKey);
                self.expect_node(false, true, false);
            }
            St::FirstBlockSequenceItem | St::BlockSequenceItem => {
                let first = self.state == St::FirstBlockSequenceItem;
                if !first && matches!(self.ev(), E::SeqEnd) {
                    self.indent = self.indents.pop().unwrap_or(None);
                    self.state = self.pop_state();
                } else {
                    self.write_indent();
                    self.write_indicator("-", true, false, true);
                    self.states.push(St::BlockSequenceItem);
                    self.expect_node(false, false, false);
                }
            }
            St::FirstBlockMappingKey | St::BlockMappingKey => {
                let first = self.state == St::FirstBlockMappingKey;
                if !first && matches!(self.ev(), E::MapEnd) {
                    self.indent = self.indents.pop().unwrap_or(None);
                    self.state = self.pop_state();
                } else {
                    self.write_indent();
                    if self.check_simple_key() {
                        self.states.push(St::BlockMappingSimpleValue);
                        self.expect_node(false, true, true);
                    } else {
                        self.write_indicator("?", true, false, true);
                        self.states.push(St::BlockMappingValue);
                        self.expect_node(false, true, false);
                    }
                }
            }
            St::BlockMappingSimpleValue => {
                self.write_indicator(":", false, false, false);
                self.states.push(St::BlockMappingKey);
                self.expect_node(false, true, false);
            }
            St::BlockMappingValue => {
                self.write_indent();
                self.write_indicator(":", true, false, true);
                self.states.push(St::BlockMappingKey);
                self.expect_node(false, true, false);
            }
        }
    }

    fn expect_document_start(&mut self, first: bool) {
        match self.ev().clone() {
            E::DocStart { explicit } => {
                let implicit = first && !explicit && !self.check_empty_document();
                if !implicit {
                    self.write_indent();
                    self.write_indicator("---", true, false, false);
                }
                self.state = St::DocumentRoot;
            }
            E::StreamEnd => {
                if self.open_ended {
                    self.write_indicator("...", true, false, false);
                    self.write_indent();
                }
                self.state = St::Nothing;
            }
            _ => {}
        }
    }

    /// O documento vazio do emissor exige escalar sem etiqueta, e o representador sempre põe uma.
    fn check_empty_document(&self) -> bool {
        false
    }

    fn check_empty_sequence(&self) -> bool {
        matches!(self.ev(), E::SeqStart { .. }) && matches!(self.next_ev(), Some(E::SeqEnd))
    }

    fn check_empty_mapping(&self) -> bool {
        matches!(self.ev(), E::MapStart { .. }) && matches!(self.next_ev(), Some(E::MapEnd))
    }

    fn prepared_tag_len(&self) -> usize {
        match self.ev() {
            E::Scalar { tag, .. } | E::SeqStart { tag, .. } | E::MapStart { tag, .. } => prepare_tag(tag).chars().count(),
            _ => 0,
        }
    }

    fn check_simple_key(&mut self) -> bool {
        let mut length = 0;
        if matches!(self.ev(), E::Scalar { .. } | E::SeqStart { .. } | E::MapStart { .. }) {
            length += self.prepared_tag_len();
        }
        if let E::Scalar { value, .. } = self.ev() {
            if self.analysis.is_none() {
                self.analysis = Some(analyze_scalar(value));
            }
            if let Some(a) = &self.analysis {
                length += a.scalar.len();
            }
        }
        let scalar_ok = matches!(self.ev(), E::Scalar { .. }) && self.analysis.as_ref().is_some_and(|a| !a.empty && !a.multiline);
        length < 128 && (scalar_ok || self.check_empty_sequence() || self.check_empty_mapping())
    }

    fn expect_node(&mut self, root: bool, mapping: bool, simple_key: bool) {
        self.root_context = root;
        self.mapping_context = mapping;
        self.simple_key_context = simple_key;
        match self.ev().clone() {
            E::Scalar { .. } => {
                self.process_tag();
                self.increase_indent(true, false);
                self.process_scalar();
                self.indent = self.indents.pop().unwrap_or(None);
                self.state = self.pop_state();
            }
            E::SeqStart { flow, .. } => {
                self.process_tag();
                if self.flow_level > 0 || flow || self.check_empty_sequence() {
                    self.write_indicator("[", true, true, false);
                    self.flow_level += 1;
                    self.increase_indent(true, false);
                    self.state = St::FirstFlowSequenceItem;
                } else {
                    let indentless = self.mapping_context && !self.indention;
                    self.increase_indent(false, indentless);
                    self.state = St::FirstBlockSequenceItem;
                }
            }
            E::MapStart { flow, .. } => {
                self.process_tag();
                if self.flow_level > 0 || flow || self.check_empty_mapping() {
                    self.write_indicator("{", true, true, false);
                    self.flow_level += 1;
                    self.increase_indent(true, false);
                    self.state = St::FirstFlowMappingKey;
                } else {
                    self.increase_indent(false, false);
                    self.state = St::FirstBlockMappingKey;
                }
            }
            _ => {}
        }
    }

    fn process_tag(&mut self) {
        let (tag, implicit_scalar, implicit_coll, is_scalar) = match self.ev() {
            E::Scalar { tag, implicit, .. } => (tag.clone(), *implicit, false, true),
            E::SeqStart { tag, implicit, .. } | E::MapStart { tag, implicit, .. } => (tag.clone(), (false, false), *implicit, false),
            _ => return,
        };
        if is_scalar {
            if !self.style_chosen {
                self.choose_scalar_style();
            }
            if (self.style_plain && implicit_scalar.0) || (!self.style_plain && implicit_scalar.1) {
                return;
            }
        } else if implicit_coll {
            return;
        }
        let t = prepare_tag(&tag);
        if !t.is_empty() {
            self.write_indicator(&t, true, false, false);
        }
    }

    fn choose_scalar_style(&mut self) {
        let E::Scalar { value, implicit, style, .. } = self.ev().clone() else { return };
        if self.analysis.is_none() {
            self.analysis = Some(analyze_scalar(&value));
        }
        let a = self.analysis.as_ref().expect("análise");
        self.style_chosen = true;
        self.style_plain = false;
        if style == Some('"') {
            self.style = Some('"');
            return;
        }
        if style.is_none() && implicit.0 {
            let ok = !(self.simple_key_context && (a.empty || a.multiline))
                && ((self.flow_level > 0 && a.allow_flow_plain) || (self.flow_level == 0 && a.allow_block_plain));
            if ok {
                self.style = None;
                self.style_plain = true;
                return;
            }
        }
        if let Some(s @ ('|' | '>')) = style
            && self.flow_level == 0
            && !self.simple_key_context
            && a.allow_block
        {
            self.style = Some(s);
            return;
        }
        if (style.is_none() || style == Some('\'')) && a.allow_single_quoted && !(self.simple_key_context && a.multiline) {
            self.style = Some('\'');
            return;
        }
        self.style = Some('"');
    }

    fn process_scalar(&mut self) {
        if !self.style_chosen {
            self.choose_scalar_style();
        }
        let split = !self.simple_key_context;
        let text = self.analysis.take().map(|a| a.scalar).unwrap_or_default();
        if self.style_plain {
            self.write_plain(&text, split);
        } else {
            match self.style {
                Some('"') => self.write_double_quoted(&text, split),
                Some('\'') => self.write_single_quoted(&text, split),
                Some('>') => self.write_folded(&text),
                Some('|') => self.write_literal(&text),
                _ => self.write_plain(&text, split),
            }
        }
        self.style = None;
        self.style_plain = false;
        self.style_chosen = false;
    }

    // ---- escrita ----

    fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn write_indicator(&mut self, indicator: &str, need_whitespace: bool, whitespace: bool, indention: bool) {
        let data = if self.whitespace || !need_whitespace { indicator.to_string() } else { format!(" {indicator}") };
        self.whitespace = whitespace;
        self.indention = self.indention && indention;
        self.column += data.chars().count();
        self.open_ended = false;
        self.write(&data);
    }

    fn write_indent(&mut self) {
        let indent = self.indent.unwrap_or(0);
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.write_line_break(None);
        }
        if self.column < indent {
            self.whitespace = true;
            let data = " ".repeat(indent - self.column);
            self.column = indent;
            self.write(&data);
        }
    }

    fn write_line_break(&mut self, data: Option<char>) {
        self.whitespace = true;
        self.indention = true;
        self.column = 0;
        match data {
            Some(c) => self.out.push(c),
            None => self.out.push('\n'),
        }
    }

    fn write_chunk(&mut self, text: &[char]) {
        self.column += text.len();
        let s: String = text.iter().collect();
        self.write(&s);
    }

    fn write_single_quoted(&mut self, text: &[char], split: bool) {
        self.write_indicator("'", true, false, false);
        let mut spaces = false;
        let mut breaks = false;
        let (mut start, mut end) = (0, 0);
        while end <= text.len() {
            let ch = text.get(end).copied();
            if spaces {
                if ch != Some(' ') {
                    if start + 1 == end && self.column > self.best_width && split && start != 0 && end != text.len() {
                        self.write_indent();
                    } else {
                        self.write_chunk(&text[start..end]);
                    }
                    start = end;
                }
            } else if breaks {
                if ch.is_none_or(|c| !is_brk(c)) {
                    if text[start] == '\n' {
                        self.write_line_break(None);
                    }
                    for &br in &text[start..end] {
                        if br == '\n' {
                            self.write_line_break(None);
                        } else {
                            self.write_line_break(Some(br));
                        }
                    }
                    self.write_indent();
                    start = end;
                }
            } else if (ch.is_none() || ch.is_some_and(|c| c == ' ' || is_brk(c) || c == '\'')) && start < end {
                self.write_chunk(&text[start..end]);
                start = end;
            }
            if ch == Some('\'') {
                self.column += 2;
                self.write("''");
                start = end + 1;
            }
            if let Some(c) = ch {
                spaces = c == ' ';
                breaks = is_brk(c);
            }
            end += 1;
        }
        self.write_indicator("'", false, false, false);
    }

    fn write_double_quoted(&mut self, text: &[char], split: bool) {
        self.write_indicator("\"", true, false, false);
        let (mut start, mut end) = (0, 0);
        while end <= text.len() {
            let ch = text.get(end).copied();
            let needs_escape = match ch {
                None => true,
                Some(c) => {
                    matches!(c, '"' | '\\' | '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}')
                        || !((' '..='~').contains(&c) || ('\u{a0}'..='\u{d7ff}').contains(&c) || ('\u{e000}'..='\u{fffd}').contains(&c))
                }
            };
            if needs_escape {
                if start < end {
                    self.write_chunk(&text[start..end]);
                    start = end;
                }
                if let Some(c) = ch {
                    let rep = match c {
                        '\0' => Some('0'),
                        '\u{7}' => Some('a'),
                        '\u{8}' => Some('b'),
                        '\t' => Some('t'),
                        '\n' => Some('n'),
                        '\u{b}' => Some('v'),
                        '\u{c}' => Some('f'),
                        '\r' => Some('r'),
                        '\u{1b}' => Some('e'),
                        '"' => Some('"'),
                        '\\' => Some('\\'),
                        '\u{85}' => Some('N'),
                        '\u{a0}' => Some('_'),
                        '\u{2028}' => Some('L'),
                        '\u{2029}' => Some('P'),
                        _ => None,
                    };
                    let data = match rep {
                        Some(r) => format!("\\{r}"),
                        None if (c as u32) <= 0xff => format!("\\x{:02X}", c as u32),
                        None if (c as u32) <= 0xffff => format!("\\u{:04X}", c as u32),
                        None => format!("\\U{:08X}", c as u32),
                    };
                    self.column += data.chars().count();
                    self.write(&data);
                    start = end + 1;
                }
            }
            if 0 < end
                && end + 1 < text.len()
                && (ch == Some(' ') || start >= end)
                && self.column as isize + end as isize - start as isize > self.best_width as isize
                && split
            {
                let mut data: String = text[start.min(end)..end].iter().collect();
                data.push('\\');
                if start < end {
                    start = end;
                }
                self.column += data.chars().count();
                self.write(&data);
                self.write_indent();
                self.whitespace = false;
                self.indention = false;
                if text.get(start) == Some(&' ') {
                    self.column += 1;
                    self.write("\\");
                }
            }
            end += 1;
        }
        self.write_indicator("\"", false, false, false);
    }

    fn determine_block_hints(&self, text: &[char]) -> String {
        let mut hints = String::new();
        if let (Some(&first), Some(&last)) = (text.first(), text.last()) {
            if first == ' ' || is_brk(first) {
                hints.push_str(&self.best_indent.to_string());
            }
            if !is_brk(last) {
                hints.push('-');
            } else if text.len() == 1 || is_brk(text[text.len() - 2]) {
                hints.push('+');
            }
        }
        hints
    }

    fn write_folded(&mut self, text: &[char]) {
        let hints = self.determine_block_hints(text);
        self.write_indicator(&format!(">{hints}"), true, false, false);
        if hints.ends_with('+') {
            self.open_ended = true;
        }
        self.write_line_break(None);
        let mut leading_space = true;
        let mut spaces = false;
        let mut breaks = true;
        let (mut start, mut end) = (0, 0);
        while end <= text.len() {
            let ch = text.get(end).copied();
            if breaks {
                if ch.is_none_or(|c| !is_brk(c)) {
                    if !leading_space && ch.is_some_and(|c| c != ' ') && text[start] == '\n' {
                        self.write_line_break(None);
                    }
                    leading_space = ch == Some(' ');
                    for &br in &text[start..end] {
                        if br == '\n' {
                            self.write_line_break(None);
                        } else {
                            self.write_line_break(Some(br));
                        }
                    }
                    if ch.is_some() {
                        self.write_indent();
                    }
                    start = end;
                }
            } else if spaces {
                if ch != Some(' ') {
                    if start + 1 == end && self.column > self.best_width {
                        self.write_indent();
                    } else {
                        self.write_chunk(&text[start..end]);
                    }
                    start = end;
                }
            } else if ch.is_none_or(|c| c == ' ' || is_brk(c)) {
                self.write_chunk(&text[start..end]);
                if ch.is_none() {
                    self.write_line_break(None);
                }
                start = end;
            }
            if let Some(c) = ch {
                breaks = is_brk(c);
                spaces = c == ' ';
            }
            end += 1;
        }
    }

    fn write_literal(&mut self, text: &[char]) {
        let hints = self.determine_block_hints(text);
        self.write_indicator(&format!("|{hints}"), true, false, false);
        if hints.ends_with('+') {
            self.open_ended = true;
        }
        self.write_line_break(None);
        let mut breaks = true;
        let (mut start, mut end) = (0, 0);
        while end <= text.len() {
            let ch = text.get(end).copied();
            if breaks {
                if ch.is_none_or(|c| !is_brk(c)) {
                    for &br in &text[start..end] {
                        if br == '\n' {
                            self.write_line_break(None);
                        } else {
                            self.write_line_break(Some(br));
                        }
                    }
                    if ch.is_some() {
                        self.write_indent();
                    }
                    start = end;
                }
            } else if ch.is_none_or(is_brk) {
                let s: String = text[start..end].iter().collect();
                self.write(&s);
                if ch.is_none() {
                    self.write_line_break(None);
                }
                start = end;
            }
            if let Some(c) = ch {
                breaks = is_brk(c);
            }
            end += 1;
        }
    }

    fn write_plain(&mut self, text: &[char], split: bool) {
        if self.root_context {
            self.open_ended = true;
        }
        if text.is_empty() {
            return;
        }
        if !self.whitespace {
            self.column += 1;
            self.write(" ");
        }
        self.whitespace = false;
        self.indention = false;
        let mut spaces = false;
        let mut breaks = false;
        let (mut start, mut end) = (0, 0);
        while end <= text.len() {
            let ch = text.get(end).copied();
            if spaces {
                if ch != Some(' ') {
                    if start + 1 == end && self.column > self.best_width && split {
                        self.write_indent();
                        self.whitespace = false;
                        self.indention = false;
                    } else {
                        self.write_chunk(&text[start..end]);
                    }
                    start = end;
                }
            } else if breaks {
                if ch.is_none_or(|c| !is_brk(c)) {
                    if text[start] == '\n' {
                        self.write_line_break(None);
                    }
                    for &br in &text[start..end] {
                        if br == '\n' {
                            self.write_line_break(None);
                        } else {
                            self.write_line_break(Some(br));
                        }
                    }
                    self.write_indent();
                    self.whitespace = false;
                    self.indention = false;
                    start = end;
                }
            } else if ch.is_none_or(|c| c == ' ' || is_brk(c)) {
                self.write_chunk(&text[start..end]);
                start = end;
            }
            if let Some(c) = ch {
                spaces = c == ' ';
                breaks = is_brk(c);
            }
            end += 1;
        }
    }
}

/// `prepare_tag` com os prefixos padrão (`!` e `!!`).
fn prepare_tag(tag: &str) -> String {
    if tag == "!" {
        return tag.to_string();
    }
    let mut handle: Option<&str> = None;
    let mut suffix = tag;
    for (prefix, h) in [("!", "!"), ("tag:yaml.org,2002:", "!!")] {
        if tag.starts_with(prefix) && (prefix == "!" || prefix.len() < tag.len()) {
            handle = Some(h);
            suffix = &tag[prefix.len()..];
        }
    }
    let mut out = String::new();
    for ch in suffix.chars() {
        if ch.is_ascii_alphanumeric() || "-;/?:@&=+$,_.~*'()[]".contains(ch) || (ch == '!' && handle != Some("!")) {
            out.push(ch);
        } else {
            let mut b = [0u8; 4];
            for byte in ch.encode_utf8(&mut b).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    match handle {
        Some(h) => format!("{h}{out}"),
        None => format!("!<{out}>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    fn opts() -> DumpOptions {
        DumpOptions { width: None, indentless: false, explicit_start: false, explicit_end: false, annotations: false, grammar12: false }
    }

    fn s(x: &str) -> Py {
        Py::Str(x.to_string())
    }

    #[test]
    fn block_style_like_yq() {
        let doc = Py::Dict(vec![
            (s("name"), s("web")),
            (s("n"), Py::Int(BigInt::from(3))),
            (s("ports"), Py::List(vec![Py::Int(BigInt::from(80))])),
            (s("e"), Py::List(vec![])),
            (s("q"), s("yes")),
        ]);
        assert_eq!(dump_all(&[doc], &opts()), "name: web\nn: 3\nports:\n  - 80\ne: []\nq: 'yes'\n");
        assert_eq!(dump_all(&[Py::Int(BigInt::from(5))], &opts()), "5\n...\n");
        assert_eq!(dump_all(&[s("a"), s("b")], &opts()), "a\n--- b\n...\n");
    }
}
