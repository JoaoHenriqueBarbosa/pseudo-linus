//! O emissor do libyaml (`emitter.c`): fila de eventos com antecipação, máquina de estados, análise de
//! escalares, escolha de estilo e a escrita com quebra de linha na largura configurada. A saída sai
//! em blocos de até 16384 bytes, nas mesmas fronteiras do `yaml_emitter_flush`.

use std::collections::VecDeque;

use super::types::{is_alpha, is_blank, is_blankz, is_break, CollectionStyle, Encoding, Event, ScalarStyle, ET};

/// `OUTPUT_BUFFER_SIZE` do libyaml.
const OUTPUT_BUFFER_SIZE: usize = 16384;

pub type EmitResult = Result<(), &'static str>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBreak {
    Any,
    Cr,
    Ln,
    CrLn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmitState {
    StreamStart,
    FirstDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    FlowSequenceFirstItem,
    FlowSequenceItem,
    FlowMappingFirstKey,
    FlowMappingKey,
    FlowMappingSimpleValue,
    FlowMappingValue,
    BlockSequenceFirstItem,
    BlockSequenceItem,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingSimpleValue,
    BlockMappingValue,
    End,
}

#[derive(Default)]
struct ScalarData {
    value: Vec<char>,
    multiline: bool,
    flow_plain_allowed: bool,
    block_plain_allowed: bool,
    single_quoted_allowed: bool,
    block_allowed: bool,
    style: Option<ScalarStyle>,
}

pub struct Emitter {
    pub canonical: bool,
    pub best_indent: i64,
    pub best_width: i64,
    pub unicode: bool,
    pub line_break: LineBreak,
    encoding: Option<Encoding>,
    buffer: Vec<u8>,
    chunks: Vec<Vec<u8>>,
    states: Vec<EmitState>,
    state: EmitState,
    events: VecDeque<Event>,
    indents: Vec<i64>,
    tag_directives: Vec<(String, String)>,
    indent: i64,
    flow_level: usize,
    root_context: bool,
    sequence_context: bool,
    mapping_context: bool,
    simple_key_context: bool,
    column: usize,
    whitespace: bool,
    indention: bool,
    open_ended: u8,
    anchor: Option<String>,
    anchor_is_alias: bool,
    tag_handle: Option<String>,
    tag_suffix: Option<String>,
    scalar: ScalarData,
}

/// `IS_PRINTABLE` do libyaml: LF, ASCII imprimível e o BMP sem controles C1, sem U+FEFF e sem
/// U+FFFE/U+FFFF. Nada acima de U+FFFF (emoji) e nem o TAB são imprimíveis.
fn is_printable(c: char) -> bool {
    matches!(c, '\n' | '\u{20}'..='\u{7E}' | '\u{A0}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}') && c != '\u{FEFF}'
}

impl Emitter {
    pub fn new() -> Emitter {
        Emitter {
            canonical: false,
            best_indent: 0,
            best_width: 0,
            unicode: false,
            line_break: LineBreak::Any,
            encoding: None,
            buffer: Vec::new(),
            chunks: Vec::new(),
            states: Vec::new(),
            state: EmitState::StreamStart,
            events: VecDeque::new(),
            indents: Vec::new(),
            tag_directives: Vec::new(),
            indent: 0,
            flow_level: 0,
            root_context: false,
            sequence_context: false,
            mapping_context: false,
            simple_key_context: false,
            column: 0,
            whitespace: false,
            indention: false,
            open_ended: 0,
            anchor: None,
            anchor_is_alias: false,
            tag_handle: None,
            tag_suffix: None,
            scalar: ScalarData::default(),
        }
    }

    /// Os blocos que o emissor já descarregou, na ordem.
    pub fn take_output(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.chunks)
    }

    /// `yaml_emitter_emit`: enfileira o evento e processa o que já tem contexto suficiente.
    pub fn emit(&mut self, event: Event) -> EmitResult {
        self.events.push_back(event);
        while !self.need_more_events() {
            let head = self.events[0].clone();
            self.analyze_event(&head)?;
            self.state_machine(&head)?;
            self.events.pop_front();
        }
        Ok(())
    }

    fn need_more_events(&self) -> bool {
        let Some(head) = self.events.front() else { return true };
        let accumulate = match head.ty {
            ET::DocumentStart => 1,
            ET::SequenceStart => 2,
            ET::MappingStart => 3,
            _ => return false,
        };
        if self.events.len() > accumulate {
            return false;
        }
        let mut level = 0i32;
        for event in &self.events {
            match event.ty {
                ET::StreamStart | ET::DocumentStart | ET::SequenceStart | ET::MappingStart => level += 1,
                ET::StreamEnd | ET::DocumentEnd | ET::SequenceEnd | ET::MappingEnd => level -= 1,
                _ => {}
            }
            if level == 0 {
                return false;
            }
        }
        true
    }

    fn append_tag_directive(&mut self, value: &(String, String), allow_duplicates: bool) -> EmitResult {
        if self.tag_directives.iter().any(|(handle, _)| *handle == value.0) {
            return if allow_duplicates { Ok(()) } else { Err("duplicate %TAG directive") };
        }
        self.tag_directives.push(value.clone());
        Ok(())
    }

    fn increase_indent(&mut self, flow: bool, indentless: bool) {
        self.indents.push(self.indent);
        if self.indent < 0 {
            self.indent = if flow { self.best_indent } else { 0 };
        } else if !indentless {
            self.indent += self.best_indent;
        }
    }

    fn pop_state(&mut self) -> EmitState {
        self.states.pop().unwrap_or(EmitState::End)
    }

    fn pop_indent(&mut self) {
        self.indent = self.indents.pop().unwrap_or(-1);
    }

    fn state_machine(&mut self, event: &Event) -> EmitResult {
        match self.state {
            EmitState::StreamStart => self.emit_stream_start(event),
            EmitState::FirstDocumentStart => self.emit_document_start(event, true),
            EmitState::DocumentStart => self.emit_document_start(event, false),
            EmitState::DocumentContent => {
                self.states.push(EmitState::DocumentEnd);
                self.emit_node(event, true, false, false, false)
            }
            EmitState::DocumentEnd => self.emit_document_end(event),
            EmitState::FlowSequenceFirstItem => self.emit_flow_sequence_item(event, true),
            EmitState::FlowSequenceItem => self.emit_flow_sequence_item(event, false),
            EmitState::FlowMappingFirstKey => self.emit_flow_mapping_key(event, true),
            EmitState::FlowMappingKey => self.emit_flow_mapping_key(event, false),
            EmitState::FlowMappingSimpleValue => self.emit_flow_mapping_value(event, true),
            EmitState::FlowMappingValue => self.emit_flow_mapping_value(event, false),
            EmitState::BlockSequenceFirstItem => self.emit_block_sequence_item(event, true),
            EmitState::BlockSequenceItem => self.emit_block_sequence_item(event, false),
            EmitState::BlockMappingFirstKey => self.emit_block_mapping_key(event, true),
            EmitState::BlockMappingKey => self.emit_block_mapping_key(event, false),
            EmitState::BlockMappingSimpleValue => self.emit_block_mapping_value(event, true),
            EmitState::BlockMappingValue => self.emit_block_mapping_value(event, false),
            EmitState::End => Err("expected nothing after STREAM-END"),
        }
    }

    fn emit_stream_start(&mut self, event: &Event) -> EmitResult {
        if event.ty != ET::StreamStart {
            return Err("expected STREAM-START");
        }
        if self.encoding.is_none() {
            self.encoding = Some(event.encoding);
        }
        if self.best_indent < 2 || self.best_indent > 9 {
            self.best_indent = 2;
        }
        if self.best_width >= 0 && self.best_width <= self.best_indent * 2 {
            self.best_width = 80;
        }
        if self.best_width < 0 {
            self.best_width = i64::from(i32::MAX);
        }
        if self.line_break == LineBreak::Any {
            self.line_break = LineBreak::Ln;
        }
        self.indent = -1;
        self.column = 0;
        self.whitespace = true;
        self.indention = true;
        if self.encoding != Some(Encoding::Utf8) {
            self.flush_check();
            self.buffer.extend_from_slice("\u{FEFF}".as_bytes());
        }
        self.state = EmitState::FirstDocumentStart;
        Ok(())
    }

    fn emit_document_start(&mut self, event: &Event, first: bool) -> EmitResult {
        if event.ty == ET::DocumentStart {
            if let Some((major, minor)) = event.version {
                if major != 1 || (minor != 1 && minor != 2) {
                    return Err("incompatible %YAML directive");
                }
            }
            for directive in &event.tags {
                Self::analyze_tag_directive(directive)?;
                self.append_tag_directive(directive, false)?;
            }
            for (handle, prefix) in [("!", "!"), ("!!", "tag:yaml.org,2002:")] {
                self.append_tag_directive(&(handle.to_string(), prefix.to_string()), true)?;
            }
            let mut implicit = event.implicit;
            if !first || self.canonical {
                implicit = false;
            }
            if (event.version.is_some() || !event.tags.is_empty()) && self.open_ended != 0 {
                self.write_indicator("...", true, false, false);
                self.write_indent();
            }
            self.open_ended = 0;
            if let Some((_, minor)) = event.version {
                implicit = false;
                self.write_indicator("%YAML", true, false, false);
                self.write_indicator(if minor == 1 { "1.1" } else { "1.2" }, true, false, false);
                self.write_indent();
            }
            if !event.tags.is_empty() {
                implicit = false;
                for (handle, prefix) in &event.tags {
                    self.write_indicator("%TAG", true, false, false);
                    self.write_tag_handle(handle);
                    self.write_tag_content(prefix, true);
                    self.write_indent();
                }
            }
            if !implicit {
                self.write_indent();
                self.write_indicator("---", true, false, false);
                if self.canonical {
                    self.write_indent();
                }
            }
            self.state = EmitState::DocumentContent;
            self.open_ended = 0;
            return Ok(());
        }
        if event.ty == ET::StreamEnd {
            if self.open_ended == 2 {
                self.write_indicator("...", true, false, false);
                self.open_ended = 0;
                self.write_indent();
            }
            self.flush();
            self.state = EmitState::End;
            return Ok(());
        }
        Err("expected DOCUMENT-START or STREAM-END")
    }

    fn analyze_tag_directive(directive: &(String, String)) -> EmitResult {
        let (handle, prefix) = directive;
        if handle.is_empty() {
            return Err("tag handle must not be empty");
        }
        if !handle.starts_with('!') {
            return Err("tag handle must start with '!'");
        }
        if !handle.ends_with('!') {
            return Err("tag handle must end with '!'");
        }
        let inner: Vec<char> = handle.chars().collect();
        if inner.len() > 2 && !inner[1..inner.len() - 1].iter().all(|c| is_alpha(*c)) {
            return Err("tag handle must contain alphanumerical characters only");
        }
        if prefix.is_empty() {
            return Err("tag prefix must not be empty");
        }
        Ok(())
    }

    fn emit_document_end(&mut self, event: &Event) -> EmitResult {
        if event.ty != ET::DocumentEnd {
            return Err("expected DOCUMENT-END");
        }
        self.write_indent();
        if !event.implicit {
            self.write_indicator("...", true, false, false);
            self.open_ended = 0;
            self.write_indent();
        } else if self.open_ended == 0 {
            // libyaml 0.2.5: o fim implícito deixa o documento "aberto", e um `%YAML`/`%TAG` do
            // próximo documento então vem precedido de `...`.
            self.open_ended = 1;
        }
        self.flush();
        self.state = EmitState::DocumentStart;
        self.tag_directives.clear();
        Ok(())
    }

    fn emit_flow_sequence_item(&mut self, event: &Event, first: bool) -> EmitResult {
        if first {
            self.write_indicator("[", true, true, false);
            self.increase_indent(true, false);
            self.flow_level += 1;
        }
        if event.ty == ET::SequenceEnd {
            return self.emit_flow_end(first, "]");
        }
        if !first {
            self.write_indicator(",", false, false, false);
        }
        if self.canonical || self.column as i64 > self.best_width {
            self.write_indent();
        }
        self.states.push(EmitState::FlowSequenceItem);
        self.emit_node(event, false, true, false, false)
    }

    /// Fecha uma coleção de fluxo: `]` ou `}`.
    fn emit_flow_end(&mut self, first: bool, indicator: &str) -> EmitResult {
        self.flow_level -= 1;
        self.pop_indent();
        if self.canonical && !first {
            self.write_indicator(",", false, false, false);
            self.write_indent();
        }
        self.write_indicator(indicator, false, false, false);
        self.state = self.pop_state();
        Ok(())
    }

    fn emit_flow_mapping_key(&mut self, event: &Event, first: bool) -> EmitResult {
        if first {
            self.write_indicator("{", true, true, false);
            self.increase_indent(true, false);
            self.flow_level += 1;
        }
        if event.ty == ET::MappingEnd {
            return self.emit_flow_end(first, "}");
        }
        if !first {
            self.write_indicator(",", false, false, false);
        }
        if self.canonical || self.column as i64 > self.best_width {
            self.write_indent();
        }
        if !self.canonical && self.check_simple_key(event) {
            self.states.push(EmitState::FlowMappingSimpleValue);
            self.emit_node(event, false, false, true, true)
        } else {
            self.write_indicator("?", true, false, false);
            self.states.push(EmitState::FlowMappingValue);
            self.emit_node(event, false, false, true, false)
        }
    }

    fn emit_flow_mapping_value(&mut self, event: &Event, simple: bool) -> EmitResult {
        if simple {
            self.write_indicator(":", false, false, false);
        } else {
            if self.canonical || self.column as i64 > self.best_width {
                self.write_indent();
            }
            self.write_indicator(":", true, false, false);
        }
        self.states.push(EmitState::FlowMappingKey);
        self.emit_node(event, false, false, true, false)
    }

    fn emit_block_sequence_item(&mut self, event: &Event, first: bool) -> EmitResult {
        if first {
            self.increase_indent(false, self.mapping_context && !self.indention);
        }
        if event.ty == ET::SequenceEnd {
            self.pop_indent();
            self.state = self.pop_state();
            return Ok(());
        }
        self.write_indent();
        self.write_indicator("-", true, false, true);
        self.states.push(EmitState::BlockSequenceItem);
        self.emit_node(event, false, true, false, false)
    }

    fn emit_block_mapping_key(&mut self, event: &Event, first: bool) -> EmitResult {
        if first {
            self.increase_indent(false, false);
        }
        if event.ty == ET::MappingEnd {
            self.pop_indent();
            self.state = self.pop_state();
            return Ok(());
        }
        self.write_indent();
        if self.check_simple_key(event) {
            self.states.push(EmitState::BlockMappingSimpleValue);
            self.emit_node(event, false, false, true, true)
        } else {
            self.write_indicator("?", true, false, true);
            self.states.push(EmitState::BlockMappingValue);
            self.emit_node(event, false, false, true, false)
        }
    }

    fn emit_block_mapping_value(&mut self, event: &Event, simple: bool) -> EmitResult {
        if simple {
            self.write_indicator(":", false, false, false);
        } else {
            self.write_indent();
            self.write_indicator(":", true, false, true);
        }
        self.states.push(EmitState::BlockMappingKey);
        self.emit_node(event, false, false, true, false)
    }

    fn emit_node(&mut self, event: &Event, root: bool, sequence: bool, mapping: bool, simple_key: bool) -> EmitResult {
        self.root_context = root;
        self.sequence_context = sequence;
        self.mapping_context = mapping;
        self.simple_key_context = simple_key;
        match event.ty {
            ET::Alias => {
                self.process_anchor();
                if self.simple_key_context {
                    self.put(' ');
                }
                self.state = self.pop_state();
                Ok(())
            }
            ET::Scalar => self.emit_scalar(event),
            ET::SequenceStart | ET::MappingStart => {
                self.process_anchor();
                self.process_tag();
                let flow = self.flow_level > 0
                    || self.canonical
                    || event.collection_style == CollectionStyle::Flow
                    || self.check_empty_collection(event.ty);
                self.state = match (event.ty == ET::SequenceStart, flow) {
                    (true, true) => EmitState::FlowSequenceFirstItem,
                    (true, false) => EmitState::BlockSequenceFirstItem,
                    (false, true) => EmitState::FlowMappingFirstKey,
                    (false, false) => EmitState::BlockMappingFirstKey,
                };
                Ok(())
            }
            _ => Err("expected SCALAR, SEQUENCE-START, MAPPING-START, or ALIAS"),
        }
    }

    fn emit_scalar(&mut self, event: &Event) -> EmitResult {
        self.select_scalar_style(event)?;
        self.process_anchor();
        self.process_tag();
        self.increase_indent(true, false);
        self.process_scalar();
        self.pop_indent();
        self.state = self.pop_state();
        Ok(())
    }

    /// O evento da frente abre uma coleção que fecha logo em seguida (`[]` ou `{}`).
    fn check_empty_collection(&self, start: ET) -> bool {
        let end = if start == ET::SequenceStart { ET::SequenceEnd } else { ET::MappingEnd };
        self.events.len() >= 2 && self.events[0].ty == start && self.events[1].ty == end
    }

    fn check_simple_key(&self, event: &Event) -> bool {
        let anchor_len = self.anchor.as_ref().map_or(0, String::len);
        let tag_len = self.tag_handle.as_ref().map_or(0, String::len) + self.tag_suffix.as_ref().map_or(0, String::len);
        let length = match event.ty {
            ET::Alias => anchor_len,
            ET::Scalar => {
                if self.scalar.multiline {
                    return false;
                }
                anchor_len + tag_len + self.scalar.value.iter().map(|c| c.len_utf8()).sum::<usize>()
            }
            ET::SequenceStart | ET::MappingStart => {
                if !self.check_empty_collection(event.ty) {
                    return false;
                }
                anchor_len + tag_len
            }
            _ => return false,
        };
        length <= 128
    }

    fn select_scalar_style(&mut self, event: &Event) -> EmitResult {
        let mut style = event.style;
        let no_tag = self.tag_handle.is_none() && self.tag_suffix.is_none();
        if no_tag && !event.plain_implicit && !event.quoted_implicit {
            return Err("neither tag nor implicit flags are specified");
        }
        if style == ScalarStyle::Any {
            style = ScalarStyle::Plain;
        }
        if self.canonical {
            style = ScalarStyle::DoubleQuoted;
        }
        if self.simple_key_context && self.scalar.multiline {
            style = ScalarStyle::DoubleQuoted;
        }
        if style == ScalarStyle::Plain {
            let plain_allowed = if self.flow_level > 0 { self.scalar.flow_plain_allowed } else { self.scalar.block_plain_allowed };
            if !plain_allowed {
                style = ScalarStyle::SingleQuoted;
            }
            if self.scalar.value.is_empty() && (self.flow_level > 0 || self.simple_key_context) {
                style = ScalarStyle::SingleQuoted;
            }
            if no_tag && !event.plain_implicit {
                style = ScalarStyle::SingleQuoted;
            }
        }
        if style == ScalarStyle::SingleQuoted && !self.scalar.single_quoted_allowed {
            style = ScalarStyle::DoubleQuoted;
        }
        if (style == ScalarStyle::Literal || style == ScalarStyle::Folded)
            && (!self.scalar.block_allowed || self.flow_level > 0 || self.simple_key_context)
        {
            style = ScalarStyle::DoubleQuoted;
        }
        if no_tag && !event.quoted_implicit && style != ScalarStyle::Plain {
            self.tag_handle = Some("!".to_string());
        }
        self.scalar.style = Some(style);
        Ok(())
    }

    fn process_anchor(&mut self) {
        if let Some(anchor) = self.anchor.clone() {
            self.write_indicator(if self.anchor_is_alias { "*" } else { "&" }, true, false, false);
            self.write_anchor(&anchor);
        }
    }

    fn process_tag(&mut self) {
        if self.tag_handle.is_none() && self.tag_suffix.is_none() {
            return;
        }
        if let Some(handle) = self.tag_handle.clone() {
            self.write_tag_handle(&handle);
            if let Some(suffix) = self.tag_suffix.clone() {
                self.write_tag_content(&suffix, false);
            }
        } else if let Some(suffix) = self.tag_suffix.clone() {
            self.write_indicator("!<", true, false, false);
            self.write_tag_content(&suffix, false);
            self.write_indicator(">", false, false, false);
        }
    }

    fn process_scalar(&mut self) {
        let value = std::mem::take(&mut self.scalar.value);
        let allow_breaks = !self.simple_key_context;
        match self.scalar.style.unwrap_or(ScalarStyle::Plain) {
            ScalarStyle::SingleQuoted => self.write_single_quoted_scalar(&value, allow_breaks),
            ScalarStyle::DoubleQuoted => self.write_double_quoted_scalar(&value, allow_breaks),
            ScalarStyle::Literal => self.write_literal_scalar(&value),
            ScalarStyle::Folded => self.write_folded_scalar(&value),
            _ => self.write_plain_scalar(&value, allow_breaks),
        }
        self.scalar.value = value;
    }

    fn analyze_event(&mut self, event: &Event) -> EmitResult {
        self.anchor = None;
        self.anchor_is_alias = false;
        self.tag_handle = None;
        self.tag_suffix = None;
        self.scalar.value.clear();
        let (anchor, tag_wanted) = match event.ty {
            ET::Alias => {
                self.analyze_anchor(event.anchor.as_deref().unwrap_or(""), true)?;
                return Ok(());
            }
            ET::Scalar => (event.anchor.as_deref(), !event.plain_implicit && !event.quoted_implicit),
            ET::SequenceStart | ET::MappingStart => (event.anchor.as_deref(), !event.implicit),
            _ => return Ok(()),
        };
        if let Some(anchor) = anchor {
            self.analyze_anchor(anchor, false)?;
        }
        if let Some(tag) = event.tag.as_deref() {
            if self.canonical || tag_wanted {
                self.analyze_tag(tag)?;
            }
        }
        if event.ty == ET::Scalar {
            self.analyze_scalar(&event.value);
        }
        Ok(())
    }

    fn analyze_anchor(&mut self, anchor: &str, alias: bool) -> EmitResult {
        if anchor.is_empty() {
            return Err(if alias { "alias value must not be empty" } else { "anchor value must not be empty" });
        }
        if !anchor.chars().all(is_alpha) {
            return Err(if alias {
                "alias value must contain alphanumerical characters only"
            } else {
                "anchor value must contain alphanumerical characters only"
            });
        }
        self.anchor = Some(anchor.to_string());
        self.anchor_is_alias = alias;
        Ok(())
    }

    fn analyze_tag(&mut self, tag: &str) -> EmitResult {
        if tag.is_empty() {
            return Err("tag value must not be empty");
        }
        for (handle, prefix) in &self.tag_directives {
            if prefix.len() < tag.len() && tag.starts_with(prefix.as_str()) {
                self.tag_handle = Some(handle.clone());
                self.tag_suffix = Some(tag[prefix.len()..].to_string());
                return Ok(());
            }
        }
        self.tag_suffix = Some(tag.to_string());
        Ok(())
    }

    fn analyze_scalar(&mut self, value: &str) {
        let chars: Vec<char> = value.chars().collect();
        let data = &mut self.scalar;
        data.value = chars.clone();
        if chars.is_empty() {
            data.multiline = false;
            data.flow_plain_allowed = false;
            data.block_plain_allowed = true;
            data.single_quoted_allowed = true;
            data.block_allowed = false;
            return;
        }
        let len = chars.len();
        let (mut block_indicators, mut flow_indicators) = (false, false);
        let (mut line_breaks, mut special_characters) = (false, false);
        let (mut leading_space, mut leading_break, mut trailing_space, mut trailing_break) = (false, false, false, false);
        let (mut break_space, mut space_break) = (false, false);
        let (mut previous_space, mut previous_break) = (false, false);
        if len >= 3 && ((chars[0] == '-' && chars[1] == '-' && chars[2] == '-') || (chars[0] == '.' && chars[1] == '.' && chars[2] == '.')) {
            block_indicators = true;
            flow_indicators = true;
        }
        let mut preceded_by_whitespace = true;
        let mut followed_by_whitespace = chars.get(1).is_none_or(|c| is_blankz(*c));
        for (i, &c) in chars.iter().enumerate() {
            if i == 0 {
                if "#,[]{}&*!|>'\"%@`".contains(c) {
                    flow_indicators = true;
                    block_indicators = true;
                }
                if c == '?' || c == ':' {
                    flow_indicators = true;
                    if followed_by_whitespace {
                        block_indicators = true;
                    }
                }
                if c == '-' && followed_by_whitespace {
                    flow_indicators = true;
                    block_indicators = true;
                }
            } else {
                if ",?[]{}".contains(c) {
                    flow_indicators = true;
                }
                if c == ':' {
                    flow_indicators = true;
                    if followed_by_whitespace {
                        block_indicators = true;
                    }
                }
                if c == '#' && preceded_by_whitespace {
                    flow_indicators = true;
                    block_indicators = true;
                }
            }
            if !is_printable(c) || (!c.is_ascii() && !self.unicode) {
                special_characters = true;
            }
            if is_break(c) {
                line_breaks = true;
            }
            if c == ' ' {
                leading_space |= i == 0;
                trailing_space |= i + 1 == len;
                break_space |= previous_break;
                previous_space = true;
                previous_break = false;
            } else if is_break(c) {
                leading_break |= i == 0;
                trailing_break |= i + 1 == len;
                space_break |= previous_space;
                previous_space = false;
                previous_break = true;
            } else {
                previous_space = false;
                previous_break = false;
            }
            preceded_by_whitespace = is_blankz(c);
            if i + 1 < len {
                followed_by_whitespace = chars.get(i + 2).is_none_or(|c| is_blankz(*c));
            }
        }
        let data = &mut self.scalar;
        data.multiline = line_breaks;
        data.flow_plain_allowed = true;
        data.block_plain_allowed = true;
        data.single_quoted_allowed = true;
        data.block_allowed = true;
        if leading_space || leading_break || trailing_space || trailing_break {
            data.flow_plain_allowed = false;
            data.block_plain_allowed = false;
        }
        if trailing_space {
            data.block_allowed = false;
        }
        if break_space {
            data.flow_plain_allowed = false;
            data.block_plain_allowed = false;
            data.single_quoted_allowed = false;
        }
        if space_break || special_characters {
            data.flow_plain_allowed = false;
            data.block_plain_allowed = false;
            data.single_quoted_allowed = false;
            data.block_allowed = false;
        }
        if line_breaks {
            data.flow_plain_allowed = false;
            data.block_plain_allowed = false;
        }
        if flow_indicators {
            data.flow_plain_allowed = false;
        }
        if block_indicators {
            data.block_plain_allowed = false;
        }
    }

    // ---- escrita ----

    fn flush_check(&mut self) {
        if self.buffer.len() + 5 >= OUTPUT_BUFFER_SIZE {
            self.flush();
        }
    }

    /// `yaml_emitter_flush`: entrega o bloco corrente, recodificado para a saída.
    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.buffer);
        let chunk = match self.encoding {
            Some(encoding @ (Encoding::Utf16Le | Encoding::Utf16Be)) => {
                let text = String::from_utf8_lossy(&data);
                text.encode_utf16()
                    .flat_map(|unit| if encoding == Encoding::Utf16Le { unit.to_le_bytes() } else { unit.to_be_bytes() })
                    .collect()
            }
            _ => data,
        };
        self.chunks.push(chunk);
    }

    fn put(&mut self, c: char) {
        self.flush_check();
        self.buffer.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        self.column += 1;
    }

    fn put_break(&mut self) {
        self.flush_check();
        let bytes: &[u8] = match self.line_break {
            LineBreak::Cr => b"\r",
            LineBreak::CrLn => b"\r\n",
            _ => b"\n",
        };
        self.buffer.extend_from_slice(bytes);
        self.column = 0;
    }

    /// `WRITE_BREAK`: LF vira a quebra configurada; as demais saem como são.
    fn write_break(&mut self, c: char) {
        self.flush_check();
        if c == '\n' {
            self.put_break();
        } else {
            self.buffer.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            self.column = 0;
        }
    }

    fn write_indicator(&mut self, indicator: &str, need_whitespace: bool, is_whitespace: bool, is_indention: bool) {
        if need_whitespace && !self.whitespace {
            self.put(' ');
        }
        for c in indicator.chars() {
            self.put(c);
        }
        self.whitespace = is_whitespace;
        self.indention = self.indention && is_indention;
    }

    fn write_indent(&mut self) {
        let indent = self.indent.max(0) as usize;
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.put_break();
        }
        while self.column < indent {
            self.put(' ');
        }
        self.whitespace = true;
        self.indention = true;
    }

    fn write_anchor(&mut self, anchor: &str) {
        for c in anchor.chars() {
            self.put(c);
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn write_tag_handle(&mut self, handle: &str) {
        if !self.whitespace {
            self.put(' ');
        }
        for c in handle.chars() {
            self.put(c);
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn write_tag_content(&mut self, value: &str, need_whitespace: bool) {
        if need_whitespace && !self.whitespace {
            self.put(' ');
        }
        for c in value.chars() {
            if is_alpha(c) || ";/?:@&=+$,_.~*'()[]".contains(c) {
                self.put(c);
            } else {
                for byte in c.encode_utf8(&mut [0; 4]).bytes() {
                    self.put('%');
                    self.put(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0').to_ascii_uppercase());
                    self.put(char::from_digit(u32::from(byte & 0x0F), 16).unwrap_or('0').to_ascii_uppercase());
                }
            }
        }
        self.whitespace = false;
        self.indention = false;
    }

    /// Fim de um escalar escrito: o que vem depois não está em branco nem recuado.
    fn finish_scalar(&mut self) {
        self.whitespace = false;
        self.indention = false;
    }

    fn write_plain_scalar(&mut self, value: &[char], allow_breaks: bool) {
        let (mut spaces, mut breaks) = (false, false);
        if !self.whitespace && (!value.is_empty() || self.flow_level > 0) {
            self.put(' ');
        }
        let mut i = 0;
        while i < value.len() {
            let c = value[i];
            if c == ' ' {
                if allow_breaks && !spaces && self.column as i64 > self.best_width && value.get(i + 1) != Some(&' ') {
                    self.write_indent();
                } else {
                    self.put(c);
                }
                spaces = true;
            } else if is_break(c) {
                if !breaks && c == '\n' {
                    self.put_break();
                }
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                self.put(c);
                self.indention = false;
                spaces = false;
                breaks = false;
            }
            i += 1;
        }
        self.finish_scalar();
        if self.root_context {
            self.open_ended = 1;
        }
    }

    fn write_single_quoted_scalar(&mut self, value: &[char], allow_breaks: bool) {
        let (mut spaces, mut breaks) = (false, false);
        self.write_indicator("'", true, false, false);
        for (i, &c) in value.iter().enumerate() {
            if c == ' ' {
                if allow_breaks && !spaces && self.column as i64 > self.best_width && i != 0 && i + 1 != value.len() && value.get(i + 1) != Some(&' ') {
                    self.write_indent();
                } else {
                    self.put(c);
                }
                spaces = true;
            } else if is_break(c) {
                if !breaks && c == '\n' {
                    self.put_break();
                }
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                if c == '\'' {
                    self.put('\'');
                }
                self.put(c);
                self.indention = false;
                spaces = false;
                breaks = false;
            }
        }
        if breaks {
            self.write_indent();
        }
        self.write_indicator("'", false, false, false);
        self.finish_scalar();
    }

    fn write_double_quoted_scalar(&mut self, value: &[char], allow_breaks: bool) {
        let mut spaces = false;
        self.write_indicator("\"", true, false, false);
        for (i, &c) in value.iter().enumerate() {
            if !is_printable(c) || (!self.unicode && !c.is_ascii()) || c == '\u{FEFF}' || is_break(c) || c == '"' || c == '\\' {
                self.put('\\');
                let simple = match c {
                    '\0' => Some('0'),
                    '\x07' => Some('a'),
                    '\x08' => Some('b'),
                    '\t' => Some('t'),
                    '\n' => Some('n'),
                    '\x0B' => Some('v'),
                    '\x0C' => Some('f'),
                    '\r' => Some('r'),
                    '\x1B' => Some('e'),
                    '"' => Some('"'),
                    '\\' => Some('\\'),
                    '\u{85}' => Some('N'),
                    '\u{A0}' => Some('_'),
                    '\u{2028}' => Some('L'),
                    '\u{2029}' => Some('P'),
                    _ => None,
                };
                match simple {
                    Some(letter) => self.put(letter),
                    None => {
                        let code = u32::from(c);
                        let (marker, digits) = if code <= 0xFF {
                            ('x', 2)
                        } else if code <= 0xFFFF {
                            ('u', 4)
                        } else {
                            ('U', 8)
                        };
                        self.put(marker);
                        for shift in (0..digits).rev() {
                            let digit = (code >> (shift * 4)) & 0x0F;
                            self.put(char::from_digit(digit, 16).unwrap_or('0').to_ascii_uppercase());
                        }
                    }
                }
                spaces = false;
            } else if c == ' ' {
                if allow_breaks && !spaces && self.column as i64 > self.best_width && i != 0 && i + 1 != value.len() {
                    self.write_indent();
                    if value.get(i + 1) == Some(&' ') {
                        self.put('\\');
                    }
                } else {
                    self.put(c);
                }
                spaces = true;
            } else {
                self.put(c);
                spaces = false;
            }
        }
        self.write_indicator("\"", false, false, false);
        self.finish_scalar();
    }

    fn write_block_scalar_hints(&mut self, value: &[char]) {
        if value.first().is_some_and(|c| *c == ' ' || is_break(*c)) {
            let hint = char::from_digit(self.best_indent as u32, 10).unwrap_or('2').to_string();
            self.write_indicator(&hint, false, false, false);
        }
        self.open_ended = 0;
        let chomp_hint = match value {
            [] => Some("-"),
            [.., last] if !is_break(*last) => Some("-"),
            [only] if is_break(*only) => {
                self.open_ended = 2;
                Some("+")
            }
            [.., before, _] if is_break(*before) => {
                self.open_ended = 2;
                Some("+")
            }
            _ => None,
        };
        if let Some(hint) = chomp_hint {
            self.write_indicator(hint, false, false, false);
        }
    }

    /// Cabeçalho de um escalar em bloco (`|` ou `>`, as dicas e a quebra de linha).
    fn write_block_header(&mut self, indicator: &str, value: &[char]) {
        self.write_indicator(indicator, true, false, false);
        self.write_block_scalar_hints(value);
        self.put_break();
        self.indention = true;
        self.whitespace = true;
    }

    fn write_literal_scalar(&mut self, value: &[char]) {
        let mut breaks = true;
        self.write_block_header("|", value);
        for &c in value {
            if is_break(c) {
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                self.put(c);
                self.indention = false;
                breaks = false;
            }
        }
    }

    fn write_folded_scalar(&mut self, value: &[char]) {
        let mut breaks = true;
        let mut leading_spaces = true;
        self.write_block_header(">", value);
        for (i, &c) in value.iter().enumerate() {
            if is_break(c) {
                if !breaks && !leading_spaces && c == '\n' {
                    let mut k = 0;
                    while value.get(i + k).is_some_and(|c| is_break(*c)) {
                        k += 1;
                    }
                    if !value.get(i + k).is_none_or(|c| is_blankz(*c)) {
                        self.put_break();
                    }
                }
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                    leading_spaces = is_blank(c);
                }
                if !breaks && c == ' ' && value.get(i + 1) != Some(&' ') && self.column as i64 > self.best_width {
                    self.write_indent();
                } else {
                    self.put(c);
                }
                self.indention = false;
                breaks = false;
            }
        }
    }
}

impl Default for Emitter {
    fn default() -> Emitter {
        Emitter::new()
    }
}
