//! O logger do `lib.rs` do upstream (`Kind`, `Location`, `Data`, `Msg`, `Log`, `Source`, `range_data`), na parte
//! que o lexer e o parser usam. Os nomes e o corpo das funções são os do upstream; o que mudou está no README
//! (seção "logger").

use std::borrow::Cow;
use std::fmt;

use bun_core::strings::{CodepointIterator, Cursor};

use crate::{Loc, Range};

// ───────────────────────────────────────────────────────────────────────────
// Kind
// ───────────────────────────────────────────────────────────────────────────

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, strum::IntoStaticStr)]
pub enum Kind {
    Err = 0,
    Warn = 1,
    Note = 2,
    Debug = 3,
    Verbose = 4,
}

impl Kind {
    #[inline]
    pub fn should_print(self, other: Level) -> bool {
        match other {
            Level::Err => matches!(self, Kind::Err | Kind::Note),
            Level::Warn => matches!(self, Kind::Err | Kind::Warn | Kind::Note),
            Level::Info | Level::Debug => self != Kind::Verbose,
            Level::Verbose => true,
        }
    }

    #[inline]
    pub fn string(self) -> &'static [u8] {
        match self {
            Kind::Err => b"error",
            Kind::Warn => b"warn",
            Kind::Note => b"note",
            Kind::Debug => b"debug",
            Kind::Verbose => b"verbose",
        }
    }
}

#[repr(i8)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, strum::IntoStaticStr)]
pub enum Level {
    Verbose, // 0
    Debug,   // 1
    Info,    // 2
    Warn,    // 3
    Err,     // 4
}

impl Level {
    pub fn at_least(self, other: Level) -> bool {
        (self as i8) <= (other as i8)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Location
// ───────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Location {
    pub file: Cow<'static, [u8]>,
    pub namespace: Cow<'static, [u8]>,
    /// Text on the line, avoiding the need to refetch the source code
    pub line_text: Option<Cow<'static, [u8]>>,
    /// Number of bytes this location should highlight.
    /// 0 to just point at a single character
    pub length: usize,
    pub offset: usize,
    /// 1-based line number.
    /// Line <= 0 means there is no line and column information.
    pub line: i32,
    pub column: i32,
}

impl Default for Location {
    fn default() -> Self {
        Location {
            file: Cow::Borrowed(b""),
            namespace: Cow::Borrowed(b"file"),
            line_text: None,
            length: 0,
            offset: 0,
            line: 0,
            column: 0,
        }
    }
}

impl Location {
    pub fn init_or_null(_source: Option<&Source>, r: Range) -> Option<Location> {
        if let Some(source) = _source {
            if r.is_empty() {
                return Some(Location {
                    file: Cow::Borrowed(source.path.text),
                    namespace: Cow::Borrowed(source.path.namespace),
                    line: -1,
                    column: -1,
                    length: 0,
                    line_text: Some(Cow::Borrowed(b"")),
                    offset: 0,
                });
            }
            let data = source.init_error_position(r.loc);
            let mut full_line = &source.contents[data.line_start..data.line_end];
            // Window a long line to ~120 bytes around the error. Bounds are
            // BYTE offsets; the gate keeps the original shape (no left trim for
            // an error in the last 80 bytes) so `write_format`'s caret aligns.
            let offset_in_line = clamp_error_offset(&source.contents, r.loc)
                .saturating_sub(data.line_start)
                .min(full_line.len());
            if full_line.len() > 80 + offset_in_line {
                let mut lo = offset_in_line.saturating_sub(40);
                let mut hi = (offset_in_line + 80).min(full_line.len());
                while lo > 0 && !is_utf8_char_boundary(full_line[lo]) {
                    lo -= 1;
                }
                while hi < full_line.len() && !is_utf8_char_boundary(full_line[hi]) {
                    hi += 1;
                }
                full_line = &full_line[lo..hi];
            }

            return Some(Location {
                file: Cow::Borrowed(source.path.text),
                namespace: Cow::Borrowed(source.path.namespace),
                line: crate::usize2loc(data.line_count).start,
                column: crate::usize2loc(data.column_count).start,
                length: if r.len > -1 { u32::try_from(r.len).expect("int cast") as usize } else { 1 },
                line_text: Some(Cow::Owned(bun_core::strings::trim_left(full_line, b"\n\r").to_vec())),
                offset: usize::try_from(r.loc.start.max(0)).expect("int cast"),
            });
        }
        None
    }
}

/// `bun_core::strings::is_utf8_char_boundary`: o byte não é de continuação.
#[inline]
fn is_utf8_char_boundary(b: u8) -> bool {
    (b as i8) >= -0x40
}

// ───────────────────────────────────────────────────────────────────────────
// Data
// ───────────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct Data {
    pub text: Cow<'static, [u8]>,
    pub location: Option<Location>,
}

impl Data {
    pub fn clone_line_text(&self, should: bool) -> Data {
        if !should || self.location.is_none() || self.location.as_ref().unwrap().line_text.is_none() {
            return self.clone();
        }

        let new_line_text = self.location.as_ref().unwrap().line_text.as_deref().unwrap().to_vec();
        let mut new_location = self.location.clone().unwrap();
        new_location.line_text = Some(Cow::Owned(new_line_text));
        Data { text: self.text.clone(), location: Some(new_location) }
    }
}

/// Owned/borrowed → `Cow<'static, [u8]>` for `Data.text`.
pub trait IntoText {
    fn into_text(self) -> Cow<'static, [u8]>;
}
impl IntoText for Cow<'static, [u8]> {
    #[inline]
    fn into_text(self) -> Cow<'static, [u8]> {
        self
    }
}
impl IntoText for &'static [u8] {
    #[inline]
    fn into_text(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(self)
    }
}
impl<const N: usize> IntoText for &'static [u8; N] {
    #[inline]
    fn into_text(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(self)
    }
}
impl IntoText for &'static str {
    #[inline]
    fn into_text(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(self.as_bytes())
    }
}
impl IntoText for Vec<u8> {
    #[inline]
    fn into_text(self) -> Cow<'static, [u8]> {
        Cow::Owned(self)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Msg
// ───────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Msg {
    pub kind: Kind,
    pub data: Data,
    pub notes: Box<[Data]>,
    pub redact_sensitive_information: bool,
}

impl Default for Msg {
    fn default() -> Self {
        Msg { kind: Kind::Err, data: Data::default(), notes: Box::default(), redact_sensitive_information: false }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Log
// ───────────────────────────────────────────────────────────────────────────

pub struct Log {
    pub warnings: u32,
    pub errors: u32,
    pub msgs: Vec<Msg>,
    pub level: Level,
    pub clone_line_text: bool,
}

impl Default for Log {
    fn default() -> Self {
        Log {
            warnings: 0,
            errors: 0,
            msgs: Vec::new(),
            level: if cfg!(debug_assertions) { Level::Info } else { Level::Warn },
            clone_line_text: false,
        }
    }
}

impl Log {
    #[inline]
    pub fn has_errors(&self) -> bool {
        self.errors > 0
    }

    pub fn has_any(&self) -> bool {
        (self.warnings + self.errors) > 0
    }

    #[cold]
    #[inline(never)]
    fn add_formatted_msg(
        &mut self,
        kind: Kind,
        source: Option<&Source>,
        r: Range,
        text: Cow<'static, [u8]>,
        notes: Box<[Data]>,
        redact_sensitive_information: bool,
    ) {
        match kind {
            Kind::Err => self.errors += 1,
            Kind::Warn => self.warnings += 1,
            _ => {}
        }
        let data = range_data(source, r, text).clone_line_text(self.clone_line_text);
        self.add_msg(Msg { kind, data, notes, redact_sensitive_information })
    }

    #[inline]
    pub fn add_error_fmt_opts(&mut self, args: fmt::Arguments<'_>, opts: AddErrorOptions<'_>) {
        let text = alloc_print(args);
        self.add_formatted_msg(
            Kind::Err,
            opts.source,
            Range { loc: opts.loc, len: opts.len },
            text,
            Box::default(),
            opts.redact_sensitive_information,
        )
    }

    #[inline]
    pub fn add_range_error_fmt_with_notes(
        &mut self,
        source: Option<&Source>,
        r: Range,
        notes: Box<[Data]>,
        args: fmt::Arguments<'_>,
    ) {
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Err, source, r, text, notes, false)
    }

    #[cold]
    pub fn add_range_warning(&mut self, source: Option<&Source>, r: Range, text: &'static [u8]) {
        if !Kind::Warn.should_print(self.level) {
            return;
        }
        self.warnings += 1;
        let data = range_data(source, r, text).clone_line_text(self.clone_line_text);
        self.add_msg(Msg { kind: Kind::Warn, data, ..Default::default() })
    }

    pub fn add_msg(&mut self, msg: Msg) {
        self.msgs.push(msg);
    }

    #[inline]
    pub fn add_debug_fmt(&mut self, source: Option<&Source>, l: Loc, args: fmt::Arguments<'_>) {
        if !Kind::Debug.should_print(self.level) {
            return;
        }
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Debug, source, Range { loc: l, ..Default::default() }, text, Box::default(), false)
    }

    #[cold]
    pub fn add_range_error(&mut self, source: Option<&Source>, r: Range, text: &'static [u8]) {
        self.errors += 1;
        let data = range_data(source, r, text);
        self.add_msg(Msg { kind: Kind::Err, data, ..Default::default() })
    }

    #[inline]
    pub fn add_range_error_fmt(&mut self, source: Option<&Source>, r: Range, args: fmt::Arguments<'_>) {
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Err, source, r, text, Box::default(), false)
    }

    #[inline]
    pub fn add_error_fmt<'a>(&mut self, source: impl Into<Option<&'a Source>>, l: Loc, args: fmt::Arguments<'_>) {
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Err, source.into(), Range { loc: l, ..Default::default() }, text, Box::default(), false)
    }

    #[inline]
    pub fn add_warning_fmt(&mut self, source: Option<&Source>, l: Loc, args: fmt::Arguments<'_>) {
        if !Kind::Warn.should_print(self.level) {
            return;
        }
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Warn, source, Range { loc: l, ..Default::default() }, text, Box::default(), false)
    }

    #[inline]
    pub fn add_range_warning_fmt(&mut self, source: Option<&Source>, r: Range, args: fmt::Arguments<'_>) {
        if !Kind::Warn.should_print(self.level) {
            return;
        }
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Warn, source, r, text, Box::default(), false)
    }

    #[cold]
    pub fn add_range_warning_fmt_with_note(
        &mut self,
        source: Option<&Source>,
        r: Range,
        args: fmt::Arguments<'_>,
        note_args: fmt::Arguments<'_>,
        note_range: Range,
    ) {
        if !Kind::Warn.should_print(self.level) {
            return;
        }
        self.warnings += 1;
        let notes: Box<[Data]> = Box::new([range_data(source, note_range, alloc_print(note_args))]);
        let data = range_data(source, r, alloc_print(args));
        self.add_msg(Msg { kind: Kind::Warn, data, notes, ..Default::default() })
    }

    #[inline]
    pub fn add_range_warning_fmt_with_notes(
        &mut self,
        source: Option<&Source>,
        r: Range,
        notes: Box<[Data]>,
        args: fmt::Arguments<'_>,
    ) {
        let text = alloc_print(args);
        self.add_formatted_msg(Kind::Warn, source, r, text, notes, false)
    }

    #[cold]
    pub fn add_range_error_fmt_with_note(
        &mut self,
        source: Option<&Source>,
        r: Range,
        args: fmt::Arguments<'_>,
        note_args: fmt::Arguments<'_>,
        note_range: Range,
    ) {
        if !Kind::Err.should_print(self.level) {
            return;
        }
        self.errors += 1;
        let notes: Box<[Data]> = Box::new([range_data(source, note_range, alloc_print(note_args))]);
        let data = range_data(source, r, alloc_print(args));
        self.add_msg(Msg { kind: Kind::Err, data, notes, ..Default::default() })
    }

    #[cold]
    pub fn add_warning(&mut self, source: Option<&Source>, l: Loc, text: &'static [u8]) {
        if !Kind::Warn.should_print(self.level) {
            return;
        }
        self.warnings += 1;
        let data = range_data(source, Range { loc: l, ..Default::default() }, text);
        self.add_msg(Msg { kind: Kind::Warn, data, ..Default::default() })
    }

    #[cold]
    pub fn add_warning_with_note(&mut self, source: Option<&Source>, l: Loc, warn: &'static [u8], note_args: fmt::Arguments<'_>) {
        if !Kind::Warn.should_print(self.level) {
            return;
        }
        self.warnings += 1;
        let notes: Box<[Data]> =
            Box::new([range_data(source, Range { loc: l, ..Default::default() }, alloc_print(note_args))]);
        let data = range_data(None, Range::NONE, warn);
        self.add_msg(Msg { kind: Kind::Warn, data, notes, ..Default::default() })
    }

    #[cold]
    pub fn add_range_debug(&mut self, source: Option<&Source>, r: Range, text: &'static [u8]) {
        if !Kind::Debug.should_print(self.level) {
            return;
        }
        let data = range_data(source, r, text);
        self.add_msg(Msg { kind: Kind::Debug, data, ..Default::default() })
    }

    #[inline]
    pub fn add_range_error_with_notes(&mut self, source: Option<&Source>, r: Range, text: impl IntoText, notes: Box<[Data]>) {
        self.add_range_error_with_notes_text(source, r, text.into_text(), notes)
    }

    #[cold]
    fn add_range_error_with_notes_text(&mut self, source: Option<&Source>, r: Range, text: Cow<'static, [u8]>, notes: Box<[Data]>) {
        self.errors += 1;
        let data = range_data(source, r, text);
        self.add_msg(Msg { kind: Kind::Err, data, notes, ..Default::default() })
    }

    /// Generic only over the text conversion; the body is shared so each `&[u8; N]` literal length does not get its
    /// own copy.
    #[inline]
    pub fn add_error(&mut self, source: Option<&Source>, loc: Loc, text: impl IntoText) {
        self.add_error_text(source, loc, text.into_text());
    }

    #[cold]
    fn add_error_text(&mut self, source: Option<&Source>, loc: Loc, text: Cow<'static, [u8]>) {
        self.errors += 1;
        let data = range_data(source, Range { loc, ..Default::default() }, text);
        self.add_msg(Msg { kind: Kind::Err, data, ..Default::default() })
    }

    pub fn add_symbol_already_declared_error(&mut self, source: &Source, name: &[u8], new_loc: Loc, old_loc: Loc) {
        let note_text = alloc_print(format_args!("\"{}\" was originally declared here", bstr::BStr::new(name)));
        let notes: Box<[Data]> = Box::new([range_data(Some(source), source.range_of_identifier(old_loc), note_text)]);
        self.add_range_error_fmt_with_notes(
            Some(source),
            source.range_of_identifier(new_loc),
            notes,
            format_args!("\"{}\" has already been declared", bstr::BStr::new(name)),
        )
    }
}

#[derive(Clone, Copy, Default)]
pub struct AddErrorOptions<'a> {
    pub source: Option<&'a Source>,
    pub loc: Loc,
    pub len: i32,
    pub redact_sensitive_information: bool,
}

#[inline]
pub fn alloc_print(args: fmt::Arguments<'_>) -> Cow<'static, [u8]> {
    use std::io::Write;
    let mut v = Vec::new();
    let _ = write!(&mut v, "{}", args);
    Cow::Owned(v)
}

#[inline]
pub fn range_data(source: Option<&Source>, r: Range, text: impl IntoText) -> Data {
    Data { text: text.into_text(), location: Location::init_or_null(source, r) }
}

/// O identificador que começa em `loc`, nos dois laços que o upstream tem: o do logger (`FIXED`, que pula o escape
/// `\u{...}` também no começo, zera a largura depois dele e inclui o último ponto de código no fim do arquivo) e o do
/// lexer (`!FIXED`), que o parser chama. A diferença é de comportamento medido no upstream, então vira parâmetro em vez
/// de segunda cópia (README, "logger").
pub fn range_of_identifier<const FIXED: bool>(contents: &[u8], loc: Loc) -> Range {
    if loc.start < 0 || (loc.start as usize) >= contents.len() {
        return Range::NONE;
    }
    let text = &contents[loc.start as usize..];
    let mut r = Range { loc, len: 0 };
    if text.is_empty() {
        return r;
    }
    let end = text.len() as u32;

    let iter = CodepointIterator::init(text);
    let mut cursor = Cursor::default();
    if !iter.next(&mut cursor) {
        return r;
    }

    // Handle private names
    if cursor.c == '#' as i32 && !iter.next(&mut cursor) {
        r.len = 1;
        return r;
    }

    // Skip over bracketed unicode escapes such as "\u{10000}".
    let skip_bracketed_escape = |cursor: &mut Cursor| {
        if cursor.i + 2 < end && text[cursor.i as usize + 1] == b'u' && text[cursor.i as usize + 2] == b'{' {
            cursor.i += 2;
            while cursor.i < end {
                if text[cursor.i as usize] == b'}' {
                    cursor.i += 1;
                    break;
                }
                cursor.i += 1;
            }
            if FIXED {
                cursor.width = 0;
            }
        }
    };

    if bun_core::identifier::is_identifier_start(cursor.c) || cursor.c == '\\' as i32 {
        if FIXED && cursor.c == '\\' as i32 {
            skip_bracketed_escape(&mut cursor);
        }
        while iter.next(&mut cursor) {
            if cursor.c == '\\' as i32 {
                skip_bracketed_escape(&mut cursor);
            } else if !crate::lexer_tables::is_identifier_continue(cursor.c) {
                r.len = i32::try_from(cursor.i).expect("int cast");
                return r;
            }
        }

        // EOF dentro do identificador: o cursor guarda o deslocamento e a largura do último ponto de código lido.
        r.len = i32::try_from(if FIXED { cursor.i + cursor.width as u32 } else { cursor.i }).expect("int cast");
    }

    r
}

// ───────────────────────────────────────────────────────────────────────────
// Source
// ───────────────────────────────────────────────────────────────────────────

/// O caminho lógico do arquivo, o mesmo `bun_paths::fs::Path` do upstream (`name()`, `is_node_module()`).
pub use bun_paths::fs::Path;

/// Returned by [`Source::check_parseable_len`] once it has logged the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTooLarge;

#[derive(Clone)]
pub struct Source {
    pub path: Path<'static>,
    pub contents: Cow<'static, [u8]>,
    pub contents_is_recycled: bool,
    pub identifier_name: Cow<'static, [u8]>,
    pub index: crate::Index,
}

impl Default for Source {
    fn default() -> Self {
        Source {
            path: Path::default(),
            contents: Cow::Borrowed(b""),
            contents_is_recycled: false,
            identifier_name: Cow::Borrowed(b""),
            index: crate::Index::source(0),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ErrorPosition {
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) column_count: usize,
    pub(crate) line_count: usize,
}

#[derive(Clone, Copy)]
struct ErrorPositionState {
    line_start: usize,
    line_count: usize,
    column_number: usize,
    prev_code_point: i32,
}

impl Default for ErrorPositionState {
    fn default() -> Self {
        ErrorPositionState { line_start: 0, line_count: 1, column_number: 1, prev_code_point: 0 }
    }
}

impl ErrorPositionState {
    /// Consume the codepoints of `contents[from..to]`, updating line/column
    /// exactly the way `Source::init_error_position` always has (`\r` resets
    /// the column to 0, `\r\n` counts as a single line break, U+2028/U+2029
    /// are line breaks). Returns `true` if a line break was crossed.
    fn advance(&mut self, contents: &[u8], from: usize, to: usize) -> bool {
        let iter_ = CodepointIterator::init(&contents[from..to]);
        let mut iter = Cursor::default();
        let mut crossed_line_break = false;

        while iter_.next(&mut iter) {
            match iter.c {
                0x0A => {
                    // '\n'
                    self.column_number = 1;
                    self.line_start = from + iter.width as usize + iter.i as usize;
                    if self.prev_code_point != ('\r' as i32) {
                        self.line_count += 1;
                    }
                    crossed_line_break = true;
                }
                0x0D => {
                    // '\r'
                    self.column_number = 0;
                    self.line_start = from + iter.width as usize + iter.i as usize;
                    self.line_count += 1;
                    crossed_line_break = true;
                }
                0x2028 | 0x2029 => {
                    // These take three bytes to encode in UTF-8
                    self.line_start = from + iter.width as usize + iter.i as usize;
                    self.line_count += 1;
                    self.column_number = 1;
                    crossed_line_break = true;
                }
                _ => {
                    // Columns count UTF-16 code units (JSC/V8 stack traces, the
                    // source-map spec, and the CSS logger all agree on this).
                    self.column_number += 1 + (iter.c > 0xFFFF) as usize;
                }
            }

            self.prev_code_point = iter.c;
        }

        crossed_line_break
    }

    fn to_error_position(self, line_end: usize) -> ErrorPosition {
        ErrorPosition {
            line_start: if self.line_start > 0 { self.line_start - 1 } else { self.line_start },
            line_end,
            line_count: self.line_count,
            column_count: self.column_number,
        }
    }
}

/// Byte offset of the line break at or after `offset` (the end of the line
/// containing `offset`), or the end of the file if this is the last line.
fn scan_line_end(contents: &[u8], offset: usize) -> usize {
    let iter_ = CodepointIterator::init(&contents[offset..]);
    let mut iter = Cursor::default();

    while iter_.next(&mut iter) {
        match iter.c {
            0x0D | 0x0A | 0x2028 | 0x2029 => return offset + iter.i as usize,
            _ => {}
        }
    }

    contents.len()
}

/// Clamp a diagnostic `Loc` into `contents` the way `init_error_position`
/// always has.
#[inline]
fn clamp_error_offset(contents: &[u8], offset_loc: Loc) -> usize {
    (usize::try_from(offset_loc.start).expect("int cast")).min(contents.len().max(1) - 1)
}

impl Source {
    /// `Source::init_path_string`: o caminho é o texto dado, no namespace `file`.
    pub fn init_path_string(path: &'static [u8], contents: impl Into<Cow<'static, [u8]>>) -> Source {
        Source { path: Path::init(path), contents: contents.into(), ..Source::default() }
    }

    #[inline]
    pub fn contents(&self) -> &[u8] {
        &self.contents
    }

    /// Positions are `i32` [`Loc`]s, so no parser can take a longer source.
    pub const MAX_PARSEABLE_LEN: usize = i32::MAX as usize;

    /// Parsers call this before reading a source, whichever way it reached them.
    pub fn check_parseable_len(&self, log: &mut Log, what: &str) -> Result<(), SourceTooLarge> {
        if self.contents.len() <= Self::MAX_PARSEABLE_LEN {
            return Ok(());
        }
        // Without a position: finding the line of one would scan the oversized source.
        log.add_error_fmt_opts(
            format_args!("{what} is too large to parse (2 GiB maximum)"),
            AddErrorOptions { source: Some(self), loc: Loc::EMPTY, len: 0, redact_sensitive_information: false },
        );
        Err(SourceTooLarge)
    }

    /// O identificador que começa em `loc`. É o `range_of_identifier` do logger do upstream, com a correção dos
    /// escapes `\u{...}` (README, "logger"); o do lexer, que o parser chama, é o mesmo laço sem ela.
    pub fn range_of_identifier(&self, loc: Loc) -> Range {
        range_of_identifier::<true>(&self.contents, loc)
    }

    pub fn text_for_range(&self, r: Range) -> &[u8] {
        &self.contents[r.loc.i()..r.end_i()]
    }

    pub fn range_of_operator_before(&self, loc: Loc, op: &[u8]) -> Range {
        let text = &self.contents[0..loc.i()];
        if let Some(index) = bun_core::strings::last_index_of(text, op) {
            return Range { loc: crate::usize2loc(index), len: i32::try_from(op.len()).expect("int cast") };
        }

        Range { loc, ..Default::default() }
    }

    pub fn range_of_string(&self, loc: Loc) -> Range {
        if loc.start < 0 {
            return Range::NONE;
        }

        let text = &self.contents[loc.i()..];

        if text.is_empty() {
            return Range::NONE;
        }

        let quote = text[0];

        if quote == b'"' || quote == b'\'' {
            let mut i: usize = 1;
            let mut c: u8;
            while i < text.len() {
                c = text[i];

                if c == quote {
                    return Range { loc, len: i32::try_from(i + 1).expect("int cast") };
                } else if c == b'\\' {
                    i += 1;
                }
                i += 1;
            }
        }

        Range { loc, len: 0 }
    }

    pub(crate) fn init_error_position(&self, offset_loc: Loc) -> ErrorPosition {
        debug_assert!(!offset_loc.is_empty());
        let contents: &[u8] = &self.contents;
        let offset = clamp_error_offset(contents, offset_loc);

        let mut state = ErrorPositionState::default();
        state.advance(contents, 0, offset);

        // Scan to the end of the line (or end of file if this is the last line)
        state.to_error_position(scan_line_end(contents, offset))
    }
}
