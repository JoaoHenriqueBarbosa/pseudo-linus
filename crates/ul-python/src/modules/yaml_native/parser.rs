//! O parser do libyaml (`parser.c`): a máquina de estados que transforma tokens em eventos, com as
//! diretivas `%YAML` e `%TAG`, as âncoras, as tags e as mensagens de erro do C.

use super::reader::Parser;
use super::types::{CollectionStyle, Event, Mark, ScalarStyle, YamlError, ET, TT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseState {
    StreamStart,
    ImplicitDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    BlockNode,
    BlockNodeOrIndentlessSequence,
    FlowNode,
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
    End,
}

/// Tipo e marcas do token que está à frente.
type Head = (TT, Mark, Mark);

fn parser_error(problem: &'static str, mark: Mark) -> YamlError {
    YamlError::parser(None, Mark::default(), problem, mark)
}

fn empty_scalar(mark: Mark) -> Event {
    let mut event = Event::new(ET::Scalar, mark, mark);
    event.plain_implicit = true;
    event.style = ScalarStyle::Plain;
    event
}

impl Parser {
    /// `yaml_parser_parse`: o próximo evento; `None` depois do fim do fluxo ou de um erro.
    pub fn parse(&mut self) -> Result<Option<Event>, YamlError> {
        if self.stream_end_produced || self.error || self.state == ParseState::End {
            return Ok(None);
        }
        let result = self.state_machine();
        if result.is_err() {
            self.error = true;
        }
        result.map(Some)
    }

    fn peek(&mut self) -> Result<Head, YamlError> {
        if !self.token_available {
            self.fetch_more_tokens()?;
        }
        let token = &self.tokens[0];
        Ok((token.ty, token.start, token.end))
    }

    /// `SKIP_TOKEN`: tira o token da frente da fila e o entrega.
    fn skip_token(&mut self) -> super::types::Token {
        self.token_available = false;
        self.tokens_parsed += 1;
        let token = self.tokens.pop_front().expect("fila de tokens vazia");
        self.stream_end_produced = token.ty == TT::StreamEnd;
        token
    }

    fn pop_state(&mut self) -> ParseState {
        self.states.pop().unwrap_or(ParseState::End)
    }

    fn pop_mark(&mut self) -> Mark {
        self.marks.pop().unwrap_or_default()
    }

    fn state_machine(&mut self) -> Result<Event, YamlError> {
        match self.state {
            ParseState::StreamStart => self.parse_stream_start(),
            ParseState::ImplicitDocumentStart => self.parse_document_start(true),
            ParseState::DocumentStart => self.parse_document_start(false),
            ParseState::DocumentContent => self.parse_document_content(),
            ParseState::DocumentEnd => self.parse_document_end(),
            ParseState::BlockNode => self.parse_node(true, false),
            ParseState::BlockNodeOrIndentlessSequence => self.parse_node(true, true),
            ParseState::FlowNode => self.parse_node(false, false),
            ParseState::BlockSequenceFirstEntry => self.parse_block_sequence_entry(true),
            ParseState::BlockSequenceEntry => self.parse_block_sequence_entry(false),
            ParseState::IndentlessSequenceEntry => self.parse_indentless_sequence_entry(),
            ParseState::BlockMappingFirstKey => self.parse_block_mapping_key(true),
            ParseState::BlockMappingKey => self.parse_block_mapping_key(false),
            ParseState::BlockMappingValue => self.parse_block_mapping_value(),
            ParseState::FlowSequenceFirstEntry => self.parse_flow_sequence_entry(true),
            ParseState::FlowSequenceEntry => self.parse_flow_sequence_entry(false),
            ParseState::FlowSequenceEntryMappingKey => self.parse_flow_sequence_entry_mapping_key(),
            ParseState::FlowSequenceEntryMappingValue => self.parse_flow_sequence_entry_mapping_value(),
            ParseState::FlowSequenceEntryMappingEnd => self.parse_flow_sequence_entry_mapping_end(),
            ParseState::FlowMappingFirstKey => self.parse_flow_mapping_key(true),
            ParseState::FlowMappingKey => self.parse_flow_mapping_key(false),
            ParseState::FlowMappingValue => self.parse_flow_mapping_value(false),
            ParseState::FlowMappingEmptyValue => self.parse_flow_mapping_value(true),
            ParseState::End => Err(parser_error("expected nothing after STREAM-END", Mark::default())),
        }
    }

    fn parse_stream_start(&mut self) -> Result<Event, YamlError> {
        let (ty, start, _) = self.peek()?;
        if ty != TT::StreamStart {
            return Err(parser_error("did not find expected <stream-start>", start));
        }
        self.state = ParseState::ImplicitDocumentStart;
        let token = self.skip_token();
        let mut event = Event::new(ET::StreamStart, start, start);
        event.encoding = token.encoding;
        Ok(event)
    }

    fn parse_document_start(&mut self, implicit: bool) -> Result<Event, YamlError> {
        let (mut ty, mut start, mut end) = self.peek()?;
        if !implicit {
            while ty == TT::DocumentEnd {
                self.skip_token();
                (ty, start, end) = self.peek()?;
            }
        }
        if implicit && !matches!(ty, TT::VersionDirective | TT::TagDirective | TT::DocumentStart | TT::StreamEnd) {
            self.process_directives()?;
            self.states.push(ParseState::DocumentEnd);
            self.state = ParseState::BlockNode;
            let mut event = Event::new(ET::DocumentStart, start, start);
            event.implicit = true;
            return Ok(event);
        }
        if ty != TT::StreamEnd {
            let start_mark = start;
            let (version, tags) = self.process_directives()?;
            let (ty, token_start, token_end) = self.peek()?;
            if ty != TT::DocumentStart {
                return Err(parser_error("did not find expected <document start>", token_start));
            }
            self.states.push(ParseState::DocumentEnd);
            self.state = ParseState::DocumentContent;
            let mut event = Event::new(ET::DocumentStart, start_mark, token_end);
            event.version = version;
            event.tags = tags;
            self.skip_token();
            return Ok(event);
        }
        self.state = ParseState::End;
        self.skip_token();
        Ok(Event::new(ET::StreamEnd, start, end))
    }

    fn parse_document_content(&mut self) -> Result<Event, YamlError> {
        let (ty, start, _) = self.peek()?;
        if matches!(ty, TT::VersionDirective | TT::TagDirective | TT::DocumentStart | TT::DocumentEnd | TT::StreamEnd) {
            self.state = self.pop_state();
            return Ok(empty_scalar(start));
        }
        self.parse_node(true, false)
    }

    fn parse_document_end(&mut self) -> Result<Event, YamlError> {
        let (ty, start, end) = self.peek()?;
        let mut end_mark = start;
        let mut implicit = true;
        if ty == TT::DocumentEnd {
            end_mark = end;
            self.skip_token();
            implicit = false;
        }
        self.tag_directives.clear();
        self.state = ParseState::DocumentStart;
        let mut event = Event::new(ET::DocumentEnd, start, end_mark);
        event.implicit = implicit;
        Ok(event)
    }

    fn append_tag_directive(&mut self, value: (String, String), allow_duplicates: bool, mark: Mark) -> Result<(), YamlError> {
        if self.tag_directives.iter().any(|(handle, _)| *handle == value.0) {
            if allow_duplicates {
                return Ok(());
            }
            return Err(parser_error("found duplicate %TAG directive", mark));
        }
        self.tag_directives.push(value);
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn process_directives(&mut self) -> Result<(Option<(i64, i64)>, Vec<(String, String)>), YamlError> {
        let mut version: Option<(i64, i64)> = None;
        let mut tags: Vec<(String, String)> = Vec::new();
        let (mut ty, mut start, _) = self.peek()?;
        while ty == TT::VersionDirective || ty == TT::TagDirective {
            let token = self.skip_token();
            if ty == TT::VersionDirective {
                if version.is_some() {
                    return Err(parser_error("found duplicate %YAML directive", start));
                }
                if token.major != 1 || (token.minor != 1 && token.minor != 2) {
                    return Err(parser_error("found incompatible YAML document", start));
                }
                version = Some((token.major, token.minor));
            } else {
                let value = (token.a, token.b);
                self.append_tag_directive(value.clone(), false, start)?;
                tags.push(value);
            }
            (ty, start, _) = self.peek()?;
        }
        for (handle, prefix) in [("!", "!"), ("!!", "tag:yaml.org,2002:")] {
            self.append_tag_directive((handle.to_string(), prefix.to_string()), true, start)?;
        }
        Ok((version, tags))
    }

    /// Resolve o handle da tag contra as diretivas vigentes.
    fn resolve_tag(&self, handle: String, suffix: String, start_mark: Mark, tag_mark: Mark) -> Result<String, YamlError> {
        if handle.is_empty() {
            return Ok(suffix);
        }
        match self.tag_directives.iter().find(|(h, _)| *h == handle) {
            Some((_, prefix)) => Ok(format!("{prefix}{suffix}")),
            None => Err(YamlError::parser(Some("while parsing a node"), start_mark, "found undefined tag handle", tag_mark)),
        }
    }

    fn parse_node(&mut self, block: bool, indentless_sequence: bool) -> Result<Event, YamlError> {
        let (mut ty, start, end) = self.peek()?;
        if ty == TT::Alias {
            self.state = self.pop_state();
            let token = self.skip_token();
            let mut event = Event::new(ET::Alias, token.start, token.end);
            event.anchor = Some(token.a);
            return Ok(event);
        }
        let (mut token_start, mut token_end) = (start, end);
        let mut start_mark = start;
        let mut end_mark = start;
        let mut tag_mark = Mark::default();
        let mut anchor: Option<String> = None;
        let mut tag_parts: Option<(String, String)> = None;
        if ty == TT::Anchor || ty == TT::Tag {
            for round in 0..2 {
                if ty == TT::Anchor && anchor.is_none() {
                    let token = self.skip_token();
                    if round == 0 {
                        start_mark = token.start;
                    }
                    end_mark = token.end;
                    anchor = Some(token.a);
                } else if ty == TT::Tag && tag_parts.is_none() {
                    let token = self.skip_token();
                    if round == 0 {
                        start_mark = token.start;
                    }
                    tag_mark = token.start;
                    end_mark = token.end;
                    tag_parts = Some((token.a, token.b));
                } else {
                    break;
                }
                (ty, token_start, token_end) = self.peek()?;
            }
        }
        let tag = match tag_parts {
            Some((handle, suffix)) => Some(self.resolve_tag(handle, suffix, start_mark, tag_mark)?),
            None => None,
        };
        let implicit = tag.as_deref().is_none_or(str::is_empty);
        if indentless_sequence && ty == TT::BlockEntry {
            self.state = ParseState::IndentlessSequenceEntry;
            let mut event = Event::new(ET::SequenceStart, start_mark, token_end);
            event.anchor = anchor;
            event.tag = tag;
            event.implicit = implicit;
            event.collection_style = CollectionStyle::Block;
            return Ok(event);
        }
        let collection = match ty {
            TT::FlowSequenceStart => Some((ET::SequenceStart, ParseState::FlowSequenceFirstEntry, CollectionStyle::Flow)),
            TT::FlowMappingStart => Some((ET::MappingStart, ParseState::FlowMappingFirstKey, CollectionStyle::Flow)),
            TT::BlockSequenceStart if block => Some((ET::SequenceStart, ParseState::BlockSequenceFirstEntry, CollectionStyle::Block)),
            TT::BlockMappingStart if block => Some((ET::MappingStart, ParseState::BlockMappingFirstKey, CollectionStyle::Block)),
            _ => None,
        };
        if let Some((event_type, next_state, style)) = collection {
            self.state = next_state;
            let mut event = Event::new(event_type, start_mark, token_end);
            event.anchor = anchor;
            event.tag = tag;
            event.implicit = implicit;
            event.collection_style = style;
            return Ok(event);
        }
        if ty == TT::Scalar {
            let token = self.skip_token();
            let (mut plain_implicit, mut quoted_implicit) = (false, false);
            if (token.style == ScalarStyle::Plain && tag.is_none()) || tag.as_deref() == Some("!") {
                plain_implicit = true;
            } else if tag.is_none() {
                quoted_implicit = true;
            }
            self.state = self.pop_state();
            let mut event = Event::new(ET::Scalar, start_mark, token.end);
            event.anchor = anchor;
            event.tag = tag;
            event.value = token.a;
            event.plain_implicit = plain_implicit;
            event.quoted_implicit = quoted_implicit;
            event.style = token.style;
            return Ok(event);
        }
        if anchor.is_some() || tag.is_some() {
            self.state = self.pop_state();
            let mut event = empty_scalar(start_mark);
            event.end = end_mark;
            event.anchor = anchor;
            event.tag = tag;
            event.plain_implicit = implicit;
            return Ok(event);
        }
        let context = if block { "while parsing a block node" } else { "while parsing a flow node" };
        Err(YamlError::parser(Some(context), start_mark, "did not find expected node content", token_start))
    }

    /// Entrada de um laço de coleção: na primeira vez guarda a marca do começo e consome o
    /// indicador de abertura.
    fn open_collection(&mut self, first: bool) -> Result<Head, YamlError> {
        if first {
            let (_, start, _) = self.peek()?;
            self.marks.push(start);
            self.skip_token();
        }
        self.peek()
    }

    fn parse_block_sequence_entry(&mut self, first: bool) -> Result<Event, YamlError> {
        let (ty, start, end) = self.open_collection(first)?;
        if ty == TT::BlockEntry {
            self.skip_token();
            let (next, _, _) = self.peek()?;
            if next != TT::BlockEntry && next != TT::BlockEnd {
                self.states.push(ParseState::BlockSequenceEntry);
                return self.parse_node(true, false);
            }
            self.state = ParseState::BlockSequenceEntry;
            return Ok(empty_scalar(end));
        }
        if ty == TT::BlockEnd {
            self.state = self.pop_state();
            self.pop_mark();
            self.skip_token();
            return Ok(Event::new(ET::SequenceEnd, start, end));
        }
        let context_mark = self.pop_mark();
        Err(YamlError::parser(Some("while parsing a block collection"), context_mark, "did not find expected '-' indicator", start))
    }

    fn parse_indentless_sequence_entry(&mut self) -> Result<Event, YamlError> {
        let (ty, start, end) = self.peek()?;
        if ty != TT::BlockEntry {
            self.state = self.pop_state();
            return Ok(Event::new(ET::SequenceEnd, start, start));
        }
        self.skip_token();
        let (next, _, _) = self.peek()?;
        if !matches!(next, TT::BlockEntry | TT::Key | TT::Value | TT::BlockEnd) {
            self.states.push(ParseState::IndentlessSequenceEntry);
            return self.parse_node(true, false);
        }
        self.state = ParseState::IndentlessSequenceEntry;
        Ok(empty_scalar(end))
    }

    fn parse_block_mapping_key(&mut self, first: bool) -> Result<Event, YamlError> {
        let (ty, start, end) = self.open_collection(first)?;
        if ty == TT::Key {
            self.skip_token();
            let (next, _, _) = self.peek()?;
            if !matches!(next, TT::Key | TT::Value | TT::BlockEnd) {
                self.states.push(ParseState::BlockMappingValue);
                return self.parse_node(true, true);
            }
            self.state = ParseState::BlockMappingValue;
            return Ok(empty_scalar(end));
        }
        if ty == TT::BlockEnd {
            self.state = self.pop_state();
            self.pop_mark();
            self.skip_token();
            return Ok(Event::new(ET::MappingEnd, start, end));
        }
        let context_mark = self.pop_mark();
        Err(YamlError::parser(Some("while parsing a block mapping"), context_mark, "did not find expected key", start))
    }

    fn parse_block_mapping_value(&mut self) -> Result<Event, YamlError> {
        let (ty, start, end) = self.peek()?;
        if ty == TT::Value {
            self.skip_token();
            let (next, _, _) = self.peek()?;
            if !matches!(next, TT::Key | TT::Value | TT::BlockEnd) {
                self.states.push(ParseState::BlockMappingKey);
                return self.parse_node(true, true);
            }
            self.state = ParseState::BlockMappingKey;
            return Ok(empty_scalar(end));
        }
        self.state = ParseState::BlockMappingKey;
        Ok(empty_scalar(start))
    }

    /// Separador `,` entre os itens de uma coleção de fluxo, ou o erro de `esperado`.
    fn flow_separator(&mut self, first: bool, head: Head, context: &'static str, problem: &'static str) -> Result<Head, YamlError> {
        if first {
            return Ok(head);
        }
        if head.0 == TT::FlowEntry {
            self.skip_token();
            return self.peek();
        }
        let context_mark = self.pop_mark();
        Err(YamlError::parser(Some(context), context_mark, problem, head.1))
    }

    fn parse_flow_sequence_entry(&mut self, first: bool) -> Result<Event, YamlError> {
        let head = self.open_collection(first)?;
        let (mut ty, mut start, mut end) = head;
        if ty != TT::FlowSequenceEnd {
            (ty, start, end) = self.flow_separator(first, head, "while parsing a flow sequence", "did not find expected ',' or ']'")?;
            if ty == TT::Key {
                self.state = ParseState::FlowSequenceEntryMappingKey;
                self.skip_token();
                let mut event = Event::new(ET::MappingStart, start, end);
                event.implicit = true;
                event.collection_style = CollectionStyle::Flow;
                return Ok(event);
            }
            if ty != TT::FlowSequenceEnd {
                self.states.push(ParseState::FlowSequenceEntry);
                return self.parse_node(false, false);
            }
        }
        self.state = self.pop_state();
        self.pop_mark();
        self.skip_token();
        Ok(Event::new(ET::SequenceEnd, start, end))
    }

    fn parse_flow_sequence_entry_mapping_key(&mut self) -> Result<Event, YamlError> {
        let (ty, _, end) = self.peek()?;
        if !matches!(ty, TT::Value | TT::FlowEntry | TT::FlowSequenceEnd) {
            self.states.push(ParseState::FlowSequenceEntryMappingValue);
            return self.parse_node(false, false);
        }
        self.skip_token();
        self.state = ParseState::FlowSequenceEntryMappingValue;
        Ok(empty_scalar(end))
    }

    fn parse_flow_sequence_entry_mapping_value(&mut self) -> Result<Event, YamlError> {
        let (mut ty, mut start, _) = self.peek()?;
        if ty == TT::Value {
            self.skip_token();
            (ty, start, _) = self.peek()?;
            if ty != TT::FlowEntry && ty != TT::FlowSequenceEnd {
                self.states.push(ParseState::FlowSequenceEntryMappingEnd);
                return self.parse_node(false, false);
            }
        }
        self.state = ParseState::FlowSequenceEntryMappingEnd;
        Ok(empty_scalar(start))
    }

    fn parse_flow_sequence_entry_mapping_end(&mut self) -> Result<Event, YamlError> {
        let (_, start, _) = self.peek()?;
        self.state = ParseState::FlowSequenceEntry;
        Ok(Event::new(ET::MappingEnd, start, start))
    }

    fn parse_flow_mapping_key(&mut self, first: bool) -> Result<Event, YamlError> {
        let head = self.open_collection(first)?;
        let (mut ty, mut start, mut end) = head;
        if ty != TT::FlowMappingEnd {
            (ty, start, end) = self.flow_separator(first, head, "while parsing a flow mapping", "did not find expected ',' or '}'")?;
            if ty == TT::Key {
                self.skip_token();
                let (next, next_start, _) = self.peek()?;
                if !matches!(next, TT::Value | TT::FlowEntry | TT::FlowMappingEnd) {
                    self.states.push(ParseState::FlowMappingValue);
                    return self.parse_node(false, false);
                }
                self.state = ParseState::FlowMappingValue;
                return Ok(empty_scalar(next_start));
            }
            if ty != TT::FlowMappingEnd {
                self.states.push(ParseState::FlowMappingEmptyValue);
                return self.parse_node(false, false);
            }
        }
        self.state = self.pop_state();
        self.pop_mark();
        self.skip_token();
        Ok(Event::new(ET::MappingEnd, start, end))
    }

    fn parse_flow_mapping_value(&mut self, empty: bool) -> Result<Event, YamlError> {
        let (mut ty, mut start, _) = self.peek()?;
        if empty {
            self.state = ParseState::FlowMappingKey;
            return Ok(empty_scalar(start));
        }
        if ty == TT::Value {
            self.skip_token();
            (ty, start, _) = self.peek()?;
            if ty != TT::FlowEntry && ty != TT::FlowMappingEnd {
                self.states.push(ParseState::FlowMappingKey);
                return self.parse_node(false, false);
            }
        }
        self.state = ParseState::FlowMappingKey;
        Ok(empty_scalar(start))
    }
}
