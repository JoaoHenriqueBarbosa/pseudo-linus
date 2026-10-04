// Portado do PyYAML 6.0.2 (yaml/parser.py), Copyright (c) 2017-2021 Ingy döt Net, Copyright (c)
// 2006-2016 Kirill Simonov, licença MIT. Modificado no pseudo-linus (2026, MIT): Rust seguro, os
// estados viram um enum, e as mensagens são as do libyaml 0.2.5.

//! Parser do YAML: tokens em eventos.

use std::collections::BTreeMap;

use crate::scanner::{DirValue, Mark, Scanner, Tok, Token, YamlError};

#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    StreamStart,
    StreamEnd,
    DocumentStart { explicit: bool },
    DocumentEnd { explicit: bool },
    Alias { anchor: String },
    Scalar { anchor: Option<String>, tag: Option<String>, implicit: (bool, bool), value: String, style: Option<char> },
    SequenceStart { anchor: Option<String>, tag: Option<String>, implicit: bool, flow_style: bool },
    SequenceEnd,
    MappingStart { anchor: Option<String>, tag: Option<String>, implicit: bool, flow_style: bool },
    MappingEnd,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub ev: Ev,
    pub start: Mark,
    pub end: Mark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    StreamStart,
    ImplicitDocumentStart,
    DocumentStart,
    DocumentEnd,
    DocumentContent,
    BlockNode,
    BlockSequenceFirstEntry,
    BlockSequenceEntry,
    IndentlessSequenceEntry,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingValue,
    FlowSequenceFirstEntry,
    FlowSequenceEntry,
    FlowSequenceEntryMappingKey,
    FlowSequenceEntryMappingValue,
    FlowSequenceEntryMappingEnd,
    FlowMappingFirstKey,
    FlowMappingKey,
    FlowMappingValue,
    FlowMappingEmptyValue,
}

type R<T> = Result<T, YamlError>;

fn perr(context: Option<&str>, context_mark: Option<Mark>, problem: &str, mark: Mark) -> YamlError {
    YamlError::marked("ParserError", context, context_mark, problem, mark)
}

fn default_tags() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("!".to_string(), "!".to_string());
    m.insert("!!".to_string(), "tag:yaml.org,2002:".to_string());
    m
}

pub struct Parser {
    pub scanner: Scanner,
    current: Option<Event>,
    tag_handles: BTreeMap<String, String>,
    states: Vec<State>,
    marks: Vec<Mark>,
    state: Option<State>,
}

impl Parser {
    pub fn new(text: &str) -> Parser {
        Parser {
            scanner: Scanner::new(text),
            current: None,
            tag_handles: BTreeMap::new(),
            states: Vec::new(),
            marks: Vec::new(),
            state: Some(State::StreamStart),
        }
    }

    pub fn peek_event(&mut self) -> R<Option<&Event>> {
        if self.current.is_none()
            && let Some(s) = self.state
        {
            self.current = Some(self.run(s)?);
        }
        Ok(self.current.as_ref())
    }

    pub fn get_event(&mut self) -> R<Option<Event>> {
        self.peek_event()?;
        Ok(self.current.take())
    }

    // ---- tokens ----

    fn tok(&mut self) -> R<Tok> {
        Ok(self.scanner.check_token()?.cloned().unwrap_or(Tok::StreamEnd))
    }

    fn peek_token(&mut self) -> R<Token> {
        let m = self.scanner.mark();
        Ok(self.scanner.peek_token()?.cloned().unwrap_or(Token { tok: Tok::StreamEnd, start: m, end: m }))
    }

    fn get_token(&mut self) -> R<Token> {
        let m = self.scanner.mark();
        Ok(self.scanner.get_token()?.unwrap_or(Token { tok: Tok::StreamEnd, start: m, end: m }))
    }

    fn pop_state(&mut self) -> Option<State> {
        self.states.pop()
    }

    fn run(&mut self, s: State) -> R<Event> {
        match s {
            State::StreamStart => {
                let t = self.get_token()?;
                self.state = Some(State::ImplicitDocumentStart);
                Ok(Event { ev: Ev::StreamStart, start: t.start, end: t.end })
            }
            State::ImplicitDocumentStart => {
                let t = self.tok()?;
                if !matches!(t, Tok::Directive { .. } | Tok::DocumentStart | Tok::StreamEnd) {
                    self.tag_handles = default_tags();
                    let token = self.peek_token()?;
                    self.states.push(State::DocumentEnd);
                    self.state = Some(State::BlockNode);
                    Ok(Event { ev: Ev::DocumentStart { explicit: false }, start: token.start, end: token.start })
                } else {
                    self.document_start()
                }
            }
            State::DocumentStart => self.document_start(),
            State::DocumentEnd => {
                let token = self.peek_token()?;
                let start = token.start;
                let mut end = start;
                let mut explicit = false;
                if token.tok == Tok::DocumentEnd {
                    let t = self.get_token()?;
                    end = t.end;
                    explicit = true;
                }
                self.state = Some(State::DocumentStart);
                Ok(Event { ev: Ev::DocumentEnd { explicit }, start, end })
            }
            State::DocumentContent => {
                let t = self.tok()?;
                if matches!(t, Tok::Directive { .. } | Tok::DocumentStart | Tok::DocumentEnd | Tok::StreamEnd) {
                    let m = self.peek_token()?.start;
                    self.state = self.pop_state();
                    Ok(empty_scalar(m))
                } else {
                    self.parse_node(true, false)
                }
            }
            State::BlockNode => self.parse_node(true, false),
            State::BlockSequenceFirstEntry => {
                let t = self.get_token()?;
                self.marks.push(t.start);
                self.block_sequence_entry()
            }
            State::BlockSequenceEntry => self.block_sequence_entry(),
            State::IndentlessSequenceEntry => {
                if self.tok()? == Tok::BlockEntry {
                    let t = self.get_token()?;
                    if !matches!(self.tok()?, Tok::BlockEntry | Tok::Key | Tok::Value | Tok::BlockEnd) {
                        self.states.push(State::IndentlessSequenceEntry);
                        return self.parse_node(true, false);
                    }
                    self.state = Some(State::IndentlessSequenceEntry);
                    return Ok(empty_scalar(t.end));
                }
                let t = self.peek_token()?;
                self.state = self.pop_state();
                Ok(Event { ev: Ev::SequenceEnd, start: t.start, end: t.start })
            }
            State::BlockMappingFirstKey => {
                let t = self.get_token()?;
                self.marks.push(t.start);
                self.block_mapping_key()
            }
            State::BlockMappingKey => self.block_mapping_key(),
            State::BlockMappingValue => {
                if self.tok()? == Tok::Value {
                    let t = self.get_token()?;
                    if !matches!(self.tok()?, Tok::Key | Tok::Value | Tok::BlockEnd) {
                        self.states.push(State::BlockMappingKey);
                        return self.parse_node(true, true);
                    }
                    self.state = Some(State::BlockMappingKey);
                    return Ok(empty_scalar(t.end));
                }
                self.state = Some(State::BlockMappingKey);
                let t = self.peek_token()?;
                Ok(empty_scalar(t.start))
            }
            State::FlowSequenceFirstEntry => {
                let t = self.get_token()?;
                self.marks.push(t.start);
                self.flow_sequence_entry(true)
            }
            State::FlowSequenceEntry => self.flow_sequence_entry(false),
            State::FlowSequenceEntryMappingKey => {
                let t = self.get_token()?;
                if !matches!(self.tok()?, Tok::Value | Tok::FlowEntry | Tok::FlowSequenceEnd) {
                    self.states.push(State::FlowSequenceEntryMappingValue);
                    return self.parse_node(false, false);
                }
                self.state = Some(State::FlowSequenceEntryMappingValue);
                Ok(empty_scalar(t.end))
            }
            State::FlowSequenceEntryMappingValue => {
                if self.tok()? == Tok::Value {
                    let t = self.get_token()?;
                    if !matches!(self.tok()?, Tok::FlowEntry | Tok::FlowSequenceEnd) {
                        self.states.push(State::FlowSequenceEntryMappingEnd);
                        return self.parse_node(false, false);
                    }
                    self.state = Some(State::FlowSequenceEntryMappingEnd);
                    return Ok(empty_scalar(t.end));
                }
                self.state = Some(State::FlowSequenceEntryMappingEnd);
                let t = self.peek_token()?;
                Ok(empty_scalar(t.start))
            }
            State::FlowSequenceEntryMappingEnd => {
                self.state = Some(State::FlowSequenceEntry);
                let t = self.peek_token()?;
                Ok(Event { ev: Ev::MappingEnd, start: t.start, end: t.start })
            }
            State::FlowMappingFirstKey => {
                let t = self.get_token()?;
                self.marks.push(t.start);
                self.flow_mapping_key(true)
            }
            State::FlowMappingKey => self.flow_mapping_key(false),
            State::FlowMappingValue => {
                if self.tok()? == Tok::Value {
                    let t = self.get_token()?;
                    if !matches!(self.tok()?, Tok::FlowEntry | Tok::FlowMappingEnd) {
                        self.states.push(State::FlowMappingKey);
                        return self.parse_node(false, false);
                    }
                    self.state = Some(State::FlowMappingKey);
                    return Ok(empty_scalar(t.end));
                }
                self.state = Some(State::FlowMappingKey);
                let t = self.peek_token()?;
                Ok(empty_scalar(t.start))
            }
            State::FlowMappingEmptyValue => {
                self.state = Some(State::FlowMappingKey);
                let t = self.peek_token()?;
                Ok(empty_scalar(t.start))
            }
        }
    }

    fn document_start(&mut self) -> R<Event> {
        while self.tok()? == Tok::DocumentEnd {
            self.get_token()?;
        }
        if self.tok()? != Tok::StreamEnd {
            let start = self.peek_token()?.start;
            self.process_directives()?;
            if self.tok()? != Tok::DocumentStart {
                let t = self.peek_token()?;
                return Err(perr(None, None, "did not find expected <document start>", t.start));
            }
            let t = self.get_token()?;
            self.states.push(State::DocumentEnd);
            self.state = Some(State::DocumentContent);
            Ok(Event { ev: Ev::DocumentStart { explicit: true }, start, end: t.end })
        } else {
            let t = self.get_token()?;
            self.state = None;
            Ok(Event { ev: Ev::StreamEnd, start: t.start, end: t.end })
        }
    }

    fn process_directives(&mut self) -> R<()> {
        let mut version_seen = false;
        self.tag_handles = BTreeMap::new();
        while let Tok::Directive { name, value } = self.tok()? {
            let t = self.get_token()?;
            if name == "YAML" {
                if version_seen {
                    return Err(perr(None, None, "found duplicate %YAML directive", t.start));
                }
                if let DirValue::Yaml(major, _) = value
                    && major != 1
                {
                    return Err(perr(None, None, "found incompatible YAML document", t.start));
                }
                version_seen = true;
            } else if let DirValue::Tag(handle, prefix) = value {
                if self.tag_handles.contains_key(&handle) {
                    return Err(perr(None, None, "found duplicate %TAG directive", t.start));
                }
                self.tag_handles.insert(handle, prefix);
            }
        }
        for (k, v) in default_tags() {
            self.tag_handles.entry(k).or_insert(v);
        }
        Ok(())
    }

    fn parse_node(&mut self, block: bool, indentless_sequence: bool) -> R<Event> {
        if let Tok::Alias(name) = self.tok()? {
            let t = self.get_token()?;
            self.state = self.pop_state();
            return Ok(Event { ev: Ev::Alias { anchor: name }, start: t.start, end: t.end });
        }
        let mut anchor = None;
        let mut tag: Option<(Option<String>, String)> = None;
        let mut start = None;
        let mut end = None;
        let mut tag_mark = None;
        if let Tok::Anchor(a) = self.tok()? {
            let t = self.get_token()?;
            start = Some(t.start);
            end = Some(t.end);
            anchor = Some(a);
            if let Tok::Tag(h, s) = self.tok()? {
                let t = self.get_token()?;
                tag_mark = Some(t.start);
                end = Some(t.end);
                tag = Some((h, s));
            }
        } else if let Tok::Tag(h, s) = self.tok()? {
            let t = self.get_token()?;
            start = Some(t.start);
            tag_mark = Some(t.start);
            end = Some(t.end);
            tag = Some((h, s));
            if let Tok::Anchor(a) = self.tok()? {
                let t = self.get_token()?;
                end = Some(t.end);
                anchor = Some(a);
            }
        }
        let tag: Option<String> = match tag {
            Some((Some(handle), suffix)) => match self.tag_handles.get(&handle) {
                Some(p) => Some(format!("{p}{suffix}")),
                None => {
                    return Err(perr(Some("while parsing a node"), start, "found undefined tag handle", tag_mark.unwrap_or_default()));
                }
            },
            Some((None, suffix)) => Some(suffix),
            None => None,
        };
        let (start, mut end) = match start {
            Some(s) => (s, end.unwrap_or(s)),
            None => {
                let m = self.peek_token()?.start;
                (m, m)
            }
        };
        let implicit = tag.is_none() || tag.as_deref() == Some("!");
        let t = self.tok()?;
        if indentless_sequence && t == Tok::BlockEntry {
            end = self.peek_token()?.end;
            self.state = Some(State::IndentlessSequenceEntry);
            return Ok(Event { ev: Ev::SequenceStart { anchor, tag, implicit, flow_style: false }, start, end });
        }
        match t {
            Tok::Scalar { value, plain, style } => {
                let token = self.get_token()?;
                end = token.end;
                let imp = if (plain && tag.is_none()) || tag.as_deref() == Some("!") {
                    (true, false)
                } else if tag.is_none() {
                    (false, true)
                } else {
                    (false, false)
                };
                self.state = self.pop_state();
                Ok(Event { ev: Ev::Scalar { anchor, tag, implicit: imp, value, style }, start, end })
            }
            Tok::FlowSequenceStart => {
                end = self.peek_token()?.end;
                self.state = Some(State::FlowSequenceFirstEntry);
                Ok(Event { ev: Ev::SequenceStart { anchor, tag, implicit, flow_style: true }, start, end })
            }
            Tok::FlowMappingStart => {
                end = self.peek_token()?.end;
                self.state = Some(State::FlowMappingFirstKey);
                Ok(Event { ev: Ev::MappingStart { anchor, tag, implicit, flow_style: true }, start, end })
            }
            Tok::BlockSequenceStart if block => {
                end = self.peek_token()?.start;
                self.state = Some(State::BlockSequenceFirstEntry);
                Ok(Event { ev: Ev::SequenceStart { anchor, tag, implicit, flow_style: false }, start, end })
            }
            Tok::BlockMappingStart if block => {
                end = self.peek_token()?.start;
                self.state = Some(State::BlockMappingFirstKey);
                Ok(Event { ev: Ev::MappingStart { anchor, tag, implicit, flow_style: false }, start, end })
            }
            _ if anchor.is_some() || tag.is_some() => {
                self.state = self.pop_state();
                Ok(Event {
                    ev: Ev::Scalar { anchor, tag, implicit: (implicit, false), value: String::new(), style: None },
                    start,
                    end,
                })
            }
            _ => {
                let ctx = if block { "while parsing a block node" } else { "while parsing a flow node" };
                let t = self.peek_token()?;
                Err(perr(Some(ctx), Some(start), "did not find expected node content", t.start))
            }
        }
    }

    fn block_sequence_entry(&mut self) -> R<Event> {
        if self.tok()? == Tok::BlockEntry {
            let t = self.get_token()?;
            if !matches!(self.tok()?, Tok::BlockEntry | Tok::BlockEnd) {
                self.states.push(State::BlockSequenceEntry);
                return self.parse_node(true, false);
            }
            self.state = Some(State::BlockSequenceEntry);
            return Ok(empty_scalar(t.end));
        }
        if self.tok()? != Tok::BlockEnd {
            let t = self.peek_token()?;
            return Err(perr(Some("while parsing a block collection"), self.marks.last().copied(), "did not find expected '-' indicator", t.start));
        }
        let t = self.get_token()?;
        self.state = self.pop_state();
        self.marks.pop();
        Ok(Event { ev: Ev::SequenceEnd, start: t.start, end: t.end })
    }

    fn block_mapping_key(&mut self) -> R<Event> {
        if self.tok()? == Tok::Key {
            let t = self.get_token()?;
            if !matches!(self.tok()?, Tok::Key | Tok::Value | Tok::BlockEnd) {
                self.states.push(State::BlockMappingValue);
                return self.parse_node(true, true);
            }
            self.state = Some(State::BlockMappingValue);
            return Ok(empty_scalar(t.end));
        }
        if self.tok()? != Tok::BlockEnd {
            let t = self.peek_token()?;
            return Err(perr(Some("while parsing a block mapping"), self.marks.last().copied(), "did not find expected key", t.start));
        }
        let t = self.get_token()?;
        self.state = self.pop_state();
        self.marks.pop();
        Ok(Event { ev: Ev::MappingEnd, start: t.start, end: t.end })
    }

    fn flow_sequence_entry(&mut self, first: bool) -> R<Event> {
        if self.tok()? != Tok::FlowSequenceEnd {
            if !first {
                if self.tok()? == Tok::FlowEntry {
                    self.get_token()?;
                } else {
                    let t = self.peek_token()?;
                    return Err(perr(Some("while parsing a flow sequence"), self.marks.last().copied(), "did not find expected ',' or ']'", t.start));
                }
            }
            if self.tok()? == Tok::Key {
                let t = self.peek_token()?;
                self.state = Some(State::FlowSequenceEntryMappingKey);
                return Ok(Event {
                    ev: Ev::MappingStart { anchor: None, tag: None, implicit: true, flow_style: true },
                    start: t.start,
                    end: t.end,
                });
            } else if self.tok()? != Tok::FlowSequenceEnd {
                self.states.push(State::FlowSequenceEntry);
                return self.parse_node(false, false);
            }
        }
        let t = self.get_token()?;
        self.state = self.pop_state();
        self.marks.pop();
        Ok(Event { ev: Ev::SequenceEnd, start: t.start, end: t.end })
    }

    fn flow_mapping_key(&mut self, first: bool) -> R<Event> {
        if self.tok()? != Tok::FlowMappingEnd {
            if !first {
                if self.tok()? == Tok::FlowEntry {
                    self.get_token()?;
                } else {
                    let t = self.peek_token()?;
                    return Err(perr(Some("while parsing a flow mapping"), self.marks.last().copied(), "did not find expected ',' or '}'", t.start));
                }
            }
            if self.tok()? == Tok::Key {
                let t = self.get_token()?;
                if !matches!(self.tok()?, Tok::Value | Tok::FlowEntry | Tok::FlowMappingEnd) {
                    self.states.push(State::FlowMappingValue);
                    return self.parse_node(false, false);
                }
                self.state = Some(State::FlowMappingValue);
                return Ok(empty_scalar(t.end));
            } else if self.tok()? != Tok::FlowMappingEnd {
                self.states.push(State::FlowMappingEmptyValue);
                return self.parse_node(false, false);
            }
        }
        let t = self.get_token()?;
        self.state = self.pop_state();
        self.marks.pop();
        Ok(Event { ev: Ev::MappingEnd, start: t.start, end: t.end })
    }
}

fn empty_scalar(m: Mark) -> Event {
    Event {
        ev: Ev::Scalar { anchor: None, tag: None, implicit: (true, false), value: String::new(), style: None },
        start: m,
        end: m,
    }
}
