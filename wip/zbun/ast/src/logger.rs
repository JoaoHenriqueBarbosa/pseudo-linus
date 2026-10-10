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
                    file: source.path.text.clone(),
                    namespace: source.path.namespace.clone(),
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
                file: source.path.text.clone(),
                namespace: source.path.namespace.clone(),
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

// ───────────────────────────────────────────────────────────────────────────
// Source
// ───────────────────────────────────────────────────────────────────────────

/// O `bun_paths::fs::Path` na parte que o logger lê.
#[derive(Clone)]
pub struct Path {
    pub text: Cow<'static, [u8]>,
    pub namespace: Cow<'static, [u8]>,
}

impl Default for Path {
    fn default() -> Self {
        Path { text: Cow::Borrowed(b""), namespace: Cow::Borrowed(b"file") }
    }
}

#[derive(Clone)]
pub struct Source {
    pub path: Path,
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
    /// `Source::init_path_string`, sem o `bun_paths`: o caminho é o texto dado, no namespace `file`.
    pub fn init_path_string(path: impl Into<Cow<'static, [u8]>>, contents: impl Into<Cow<'static, [u8]>>) -> Source {
        Source { path: Path { text: path.into(), ..Path::default() }, contents: contents.into(), ..Source::default() }
    }

    #[inline]
    pub fn contents(&self) -> &[u8] {
        &self.contents
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
