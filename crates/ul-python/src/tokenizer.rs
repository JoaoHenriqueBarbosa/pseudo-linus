//! Tokenizer do CPython 3.13.5, port do `Parser/tokenizer.c` (e do `Parser/lexer/lexer.c`, para onde
//! o 3.13 moveu o `tok_get_normal_mode`).
//!
//! Cobre nomes (ASCII e Unicode), números (inteiros decimal, hexadecimal, octal e binário com `_`,
//! float, imaginário e os erros "invalid ... literal"), operadores, NEWLINE/NL, INDENT/DEDENT com as
//! pilhas `indstack`/`altindstack`, comentários, continuação com `\`, a pilha de parênteses e o
//! ENDMARKER. Os literais de string entram por `string_literal`; a fatia 4 do plano
//! (`docs/python3-port.md`) completa esse ponto (f-strings e o restante).
//!
//! Há dois modos, como no C: `Mode::Tokenize` equivale ao `tok_extra_tokens` do módulo `tokenize`
//! (emite COMMENT e NL, não reclama de `1if` nem de parêntese desbalanceado) e `Mode::Parser` é o que o
//! parser consome (sem COMMENT/NL, com as verificações de fim de número e de parênteses).
//!
//! Posições: `line` começa em 1; `col` conta caracteres desde o início da linha (o que o módulo
//! `tokenize` imprime) e `byte_col` conta bytes UTF-8 (o `col_offset` do AST). Nos erros, `offset` e
//! `end_offset` são os valores que o `SyntaxError` recebe do tokenizer.

use std::fmt;

use crate::token::{exact_type, TokenType};

/// `TABSIZE` do tokenizer: tab avança até o próximo múltiplo de 8.
const TABSIZE: usize = 8;
/// `MAXINDENT`: profundidade máxima da pilha de indentação.
const MAXINDENT: usize = 100;
/// `MAXLEVEL`: parênteses aninhados no máximo.
const MAXLEVEL: usize = 200;

/// Modo de operação (ver o doc do módulo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Como o módulo `tokenize` (`extra_tokens`).
    Tokenize,
    /// Como o parser do interpretador.
    Parser,
}

/// Posição de um token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    /// Linha, a partir de 1.
    pub line: usize,
    /// Coluna em caracteres.
    pub col: usize,
    /// Coluna em bytes UTF-8.
    pub byte_col: usize,
}

/// Um token, com o texto exato do fonte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// Tipo exato (operadores já com o tipo de `EXACT_TOKEN_TYPES`).
    pub kind: TokenType,
    pub text: String,
    pub start: Pos,
    pub end: Pos,
}

impl Token {
    /// Tipo como o `tokenize` imprime sem `-e`: operadores viram `OP`.
    pub fn generic_kind(&self) -> TokenType {
        if self.kind.exact_text().is_some() || self.text == "<>" {
            TokenType::Op
        } else {
            self.kind
        }
    }
}

/// Classe da exceção que o erro vira.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// `SyntaxError`.
    Syntax,
    /// `IndentationError`.
    Indentation,
    /// `TabError`.
    Tab,
    /// Construção que este tokenizer ainda não reconhece (f-strings, fatia 4 do plano).
    Unsupported,
}

/// Erro do tokenizer, com a localização que o CPython põe no `SyntaxError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizeError {
    pub kind: ErrorKind,
    pub msg: String,
    pub line: usize,
    pub offset: usize,
    pub end_line: usize,
    pub end_offset: usize,
}

impl fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

/// `SyntaxWarning` emitido pelo tokenizer (por exemplo `1if x else y`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub msg: String,
    pub line: usize,
    pub offset: usize,
}

/// Parêntese aberto (`parenstack`, `parenlinenostack`, `parencolstack`).
struct Paren {
    ch: char,
    line: usize,
    col: usize,
}

/// Estágios do `tok_get` para números, no lugar dos `goto fraction/exponent/imaginary` do C.
enum NumberStage {
    /// `c` é o caractere depois do ponto.
    Fraction,
    /// Depois da parte inteira ou fracionária: confere expoente, `j` e o fim.
    Suffix,
    /// `c` é o `e`/`E`.
    Exponent,
    /// `c` é o `j`/`J`.
    Imaginary,
    /// Número completo; `c` é o primeiro caractere depois dele.
    End,
}

/// Estado do tokenizer (`struct tok_state`).
pub struct Tokenizer {
    src: Vec<char>,
    pos: usize,
    /// O fonte não terminava em nova linha e o tokenizer acrescentou uma (o NEWLINE dela tem texto vazio).
    implicit_newline: bool,
    line: usize,
    line_start: usize,
    atbol: bool,
    indstack: Vec<usize>,
    altindstack: Vec<usize>,
    pendin: isize,
    parens: Vec<Paren>,
    comment_newline: bool,
    finished: bool,
    mode: Mode,
    warnings: Vec<Warning>,
}

fn is_digit(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_digit())
}

fn is_potential_identifier_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_potential_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()
}

/// Marcas combinantes (Mn/Mc) dos blocos mais usados: são `XID_Continue` mas não `XID_Start`, e o
/// `char::is_alphabetic` aceita várias delas por causa de `Other_Alphabetic`.
const MARK_RANGES: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06ED),
    (0x0900, 0x0903),
    (0x093A, 0x093C),
    (0x093E, 0x094F),
    (0x0951, 0x0957),
    (0x0962, 0x0963),
    (0x0981, 0x0983),
    (0x09BC, 0x09BC),
    (0x09BE, 0x09CD),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x1AB0, 0x1AFF),
    (0x1DC0, 0x1DFF),
    (0x20D0, 0x20FF),
    (0x302A, 0x302F),
    (0x3099, 0x309A),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
];

/// Números `No` (sobrescritos, frações, numerais circulados): `is_numeric` aceita, `XID_Continue` não.
const OTHER_NUMBER_RANGES: &[(u32, u32)] = &[
    (0x00B2, 0x00B3),
    (0x00B9, 0x00B9),
    (0x00BC, 0x00BE),
    (0x2070, 0x209F),
    (0x2150, 0x215F),
    (0x2189, 0x2189),
    (0x2460, 0x24FF),
    (0x2776, 0x2793),
    (0x3192, 0x3195),
    (0x3220, 0x3229),
    (0x3248, 0x325F),
    (0x3280, 0x3289),
    (0x32B1, 0x32BF),
];

fn in_ranges(c: char, ranges: &[(u32, u32)]) -> bool {
    let n = c as u32;
    ranges.iter().any(|&(a, b)| (a..=b).contains(&n))
}

/// `XID_Start`, aproximado pelas propriedades que a biblioteca padrão expõe.
fn is_xid_start(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic();
    }
    // Other_ID_Start.
    if matches!(c, '\u{2118}' | '\u{212E}' | '\u{309B}' | '\u{309C}') {
        return true;
    }
    c.is_alphabetic() && !in_ranges(c, MARK_RANGES)
}

/// `XID_Continue`, aproximado como `is_xid_start`.
fn is_xid_continue(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || c == '_';
    }
    if is_xid_start(c) || in_ranges(c, MARK_RANGES) || c.is_alphabetic() {
        return true;
    }
    // Other_ID_Continue e pontuação conectora.
    if matches!(
        c,
        '\u{00B7}'
            | '\u{0387}'
            | '\u{1369}'..='\u{1371}'
            | '\u{19DA}'
            | '\u{203F}'
            | '\u{2040}'
            | '\u{2054}'
            | '\u{FE33}'
            | '\u{FE34}'
            | '\u{FE4D}'..='\u{FE4F}'
            | '\u{FF3F}'
    ) {
        return true;
    }
    c.is_numeric() && !in_ranges(c, OTHER_NUMBER_RANGES)
}

/// `Py_UNICODE_ISPRINTABLE`, aproximado: exclui controles, separadores que não são o espaço,
/// caracteres de formatação conhecidos e uso privado.
fn is_printable(c: char) -> bool {
    if c.is_ascii() {
        return !(c < ' ' || c == '\x7f');
    }
    if c.is_control() || c.is_whitespace() {
        return false;
    }
    let n = c as u32;
    let format = matches!(
        n,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
    );
    let private = matches!(n, 0xE000..=0xF8FF | 0xF0000..=0x10FFFF);
    !format && !private
}

impl Tokenizer {
    /// Tokenizer sobre `source`. Como o `_PyTokenizer_translate_newlines`, `\r\n` e `\r` viram `\n` e
    /// uma nova linha final é acrescentada se faltar.
    pub fn new(source: &str, mode: Mode) -> Tokenizer {
        let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
        let mut src: Vec<char> = normalized.chars().collect();
        let implicit_newline = !src.is_empty() && src.last() != Some(&'\n');
        if implicit_newline {
            src.push('\n');
        }
        Tokenizer {
            src,
            pos: 0,
            implicit_newline,
            line: 1,
            line_start: 0,
            atbol: true,
            indstack: vec![0],
            altindstack: vec![0],
            pendin: 0,
            parens: Vec::new(),
            comment_newline: false,
            finished: false,
            mode,
            warnings: Vec::new(),
        }
    }

    /// `SyntaxWarning`s emitidos até aqui.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Próximo token. Depois do ENDMARKER, devolve ENDMARKER de novo.
    pub fn next_token(&mut self) -> Result<Token, TokenizeError> {
        loop {
            let tok = self.next_raw()?;
            if self.mode == Mode::Parser && matches!(tok.kind, TokenType::Comment | TokenType::Nl) {
                continue;
            }
            return Ok(tok);
        }
    }

    fn nextc(&mut self) -> Option<char> {
        let c = self.src.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// `tok_backup`: devolver EOF não faz nada.
    fn backup(&mut self, c: Option<char>) {
        if c.is_some() {
            self.pos -= 1;
        }
    }

    /// Chamado logo depois de consumir um `\n`.
    fn new_line(&mut self) {
        self.line += 1;
        self.line_start = self.pos;
    }

    fn text(&self, a: usize, b: usize) -> String {
        self.src[a..b].iter().collect()
    }

    fn byte_len(&self, a: usize, b: usize) -> usize {
        self.src[a..b].iter().map(|c| c.len_utf8()).sum()
    }

    fn here(&self, idx: usize) -> Pos {
        Pos { line: self.line, col: idx - self.line_start, byte_col: self.byte_len(self.line_start, idx) }
    }

    fn token(&self, kind: TokenType, start: usize) -> Token {
        Token { kind, text: self.text(start, self.pos), start: self.here(start), end: self.here(self.pos) }
    }

    fn error(&self, kind: ErrorKind, msg: String, offset: usize) -> TokenizeError {
        TokenizeError { kind, msg, line: self.line, offset, end_line: self.line, end_offset: offset }
    }

    /// `syntaxerror`: a posição é o ponto atual de leitura.
    fn syntax_error(&self, msg: impl Into<String>) -> TokenizeError {
        self.error(ErrorKind::Syntax, msg.into(), self.pos - self.line_start)
    }

    /// Erros de indentação: o C faz `tok->cur = tok->inp`, o fim da linha.
    fn indentation_error(&self, kind: ErrorKind, msg: &str) -> TokenizeError {
        let eol = self.src[self.pos..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.src.len(), |i| self.pos + i + 1);
        self.error(kind, msg.to_string(), eol - self.line_start)
    }

    fn tab_error(&self) -> TokenizeError {
        self.indentation_error(ErrorKind::Tab, "inconsistent use of tabs and spaces in indentation")
    }

    /// `tok_continuation_line`: a barra já foi lida.
    fn continuation_line(&mut self) -> Result<(), TokenizeError> {
        let c = self.nextc();
        if c != Some('\n') {
            let offset = (self.pos - self.line_start).max(1);
            return Err(self.error(
                ErrorKind::Syntax,
                "unexpected character after line continuation character".to_string(),
                offset,
            ));
        }
        self.new_line();
        let c = self.nextc();
        if c.is_none() {
            return Err(self.syntax_error("unexpected EOF while parsing"));
        }
        self.backup(c);
        Ok(())
    }

    /// Cálculo da indentação no início de uma linha lógica. Devolve se a linha é em branco.
    fn indentation(&mut self) -> Result<bool, TokenizeError> {
        let (mut col, mut altcol, mut cont_col) = (0usize, 0usize, 0usize);
        let c = loop {
            match self.nextc() {
                Some(' ') => {
                    col += 1;
                    altcol += 1;
                }
                Some('\t') => {
                    col = (col / TABSIZE + 1) * TABSIZE;
                    altcol += 1;
                }
                Some('\x0c') => {
                    col = 0;
                    altcol = 0;
                }
                Some('\\') => {
                    // A primeira barra precedida de espaço decide a indentação do que vier depois.
                    if cont_col == 0 {
                        cont_col = col;
                    }
                    self.continuation_line()?;
                }
                other => break other,
            }
        };
        self.backup(c);
        let blankline = matches!(c, Some('#' | '\n'));
        if blankline || !self.parens.is_empty() {
            return Ok(blankline);
        }
        if cont_col != 0 {
            col = cont_col;
            altcol = cont_col;
        }
        let top = *self.indstack.last().unwrap_or(&0);
        let alttop = *self.altindstack.last().unwrap_or(&0);
        if col == top {
            if altcol != alttop {
                return Err(self.tab_error());
            }
        } else if col > top {
            if self.indstack.len() >= MAXINDENT {
                return Err(self.indentation_error(ErrorKind::Indentation, "too many levels of indentation"));
            }
            if altcol <= alttop {
                return Err(self.tab_error());
            }
            self.pendin += 1;
            self.indstack.push(col);
            self.altindstack.push(altcol);
        } else {
            while self.indstack.len() > 1 && col < *self.indstack.last().unwrap_or(&0) {
                self.pendin -= 1;
                self.indstack.pop();
                self.altindstack.pop();
            }
            if col != *self.indstack.last().unwrap_or(&0) {
                return Err(self.indentation_error(
                    ErrorKind::Indentation,
                    "unindent does not match any outer indentation level",
                ));
            }
            if altcol != *self.altindstack.last().unwrap_or(&0) {
                return Err(self.tab_error());
            }
        }
        Ok(false)
    }

    /// `tok_get_normal_mode`, com COMMENT e NL sempre emitidos (o modo parser os filtra).
    fn next_raw(&mut self) -> Result<Token, TokenizeError> {
        if self.finished {
            return Ok(self.token(TokenType::Endmarker, self.pos));
        }
        let mut blankline = false;
        if self.atbol {
            self.atbol = false;
            blankline = self.indentation()?;
        }
        if self.pendin < 0 {
            self.pendin += 1;
            return Ok(self.token(TokenType::Dedent, self.pos));
        }
        if self.pendin > 0 {
            self.pendin -= 1;
            return Ok(self.token(TokenType::Indent, self.line_start));
        }
        loop {
            let mut c = self.nextc();
            while matches!(c, Some(' ' | '\t' | '\x0c')) {
                c = self.nextc();
            }
            let start = if c.is_some() { self.pos - 1 } else { self.pos };
            let Some(ch) = c else {
                if let Some(open) = self.parens.last() {
                    let offset = open.col + 1;
                    return Err(TokenizeError {
                        kind: ErrorKind::Syntax,
                        msg: format!("'{}' was never closed", open.ch),
                        line: open.line,
                        offset,
                        end_line: open.line,
                        end_offset: offset,
                    });
                }
                self.finished = true;
                return Ok(self.token(TokenType::Endmarker, start));
            };
            if ch == '#' {
                let mut c = c;
                while !matches!(c, None | Some('\n')) {
                    c = self.nextc();
                }
                self.backup(c);
                self.comment_newline = blankline;
                return Ok(self.token(TokenType::Comment, start));
            }
            if is_potential_identifier_start(ch) {
                return self.name(start, ch);
            }
            if ch == '\n' {
                let start_pos = self.here(start);
                let implicit = self.implicit_newline && self.pos == self.src.len();
                let text = if implicit { String::new() } else { "\n".to_string() };
                let end = Pos { line: start_pos.line, col: start_pos.col + 1, byte_col: start_pos.byte_col + 1 };
                self.atbol = true;
                self.new_line();
                let comment_newline = std::mem::take(&mut self.comment_newline);
                let kind = if blankline || !self.parens.is_empty() || comment_newline {
                    TokenType::Nl
                } else {
                    TokenType::Newline
                };
                return Ok(Token { kind, text, start: start_pos, end });
            }
            if ch == '.' {
                let c = self.nextc();
                if is_digit(c) {
                    return self.number(start, c, NumberStage::Fraction);
                }
                if c == Some('.') {
                    let c3 = self.nextc();
                    if c3 == Some('.') {
                        return Ok(self.token(TokenType::Ellipsis, start));
                    }
                    self.backup(c3);
                }
                self.backup(c);
                return Ok(self.token(TokenType::Dot, start));
            }
            if ch.is_ascii_digit() {
                return self.number_start(start, ch);
            }
            if ch == '\'' || ch == '"' {
                return self.string_literal(start, ch);
            }
            if ch == '\\' {
                self.continuation_line()?;
                continue;
            }
            return self.operator(start, ch);
        }
    }

    /// Operadores de dois e três caracteres, pilha de parênteses e operador de um caractere.
    fn operator(&mut self, start: usize, ch: char) -> Result<Token, TokenizeError> {
        let c2 = self.nextc();
        if let Some(c2) = c2 {
            let two: String = [ch, c2].iter().collect();
            let two_type = if two == "<>" { Some(TokenType::NotEqual) } else { exact_type(&two) };
            if let Some(two_type) = two_type {
                let c3 = self.nextc();
                if let Some(c3) = c3 {
                    let three: String = [ch, c2, c3].iter().collect();
                    if let Some(three_type) = exact_type(&three) {
                        return Ok(self.token(three_type, start));
                    }
                }
                self.backup(c3);
                return Ok(self.token(two_type, start));
            }
        }
        self.backup(c2);
        match ch {
            '(' | '[' | '{' => {
                if self.parens.len() >= MAXLEVEL {
                    return Err(self.syntax_error("too many nested parentheses"));
                }
                self.parens.push(Paren { ch, line: self.line, col: start - self.line_start });
            }
            ')' | ']' | '}' => {
                if self.mode == Mode::Parser && self.parens.is_empty() {
                    return Err(self.syntax_error(format!("unmatched '{ch}'")));
                }
                if let Some(open) = self.parens.pop() {
                    let matches = matches!((open.ch, ch), ('(', ')') | ('[', ']') | ('{', '}'));
                    if self.mode == Mode::Parser && !matches {
                        let msg = if open.line != self.line {
                            format!(
                                "closing parenthesis '{ch}' does not match opening parenthesis '{}' on line {}",
                                open.ch, open.line
                            )
                        } else {
                            format!("closing parenthesis '{ch}' does not match opening parenthesis '{}'", open.ch)
                        };
                        return Err(self.syntax_error(msg));
                    }
                }
            }
            _ => {}
        }
        if !is_printable(ch) {
            return Err(self.syntax_error(format!("invalid non-printable character U+{:04X}", ch as u32)));
        }
        let one: String = ch.to_string();
        Ok(self.token(exact_type(&one).unwrap_or(TokenType::Op), start))
    }

    /// Nome, ou prefixo de string seguido de aspas.
    fn name(&mut self, start: usize, first: char) -> Result<Token, TokenizeError> {
        let mut c = Some(first);
        let (mut saw_b, mut saw_r, mut saw_u, mut saw_f) = (false, false, false, false);
        loop {
            match c {
                Some('b' | 'B') if !saw_b && !saw_u && !saw_f => saw_b = true,
                Some('u' | 'U') if !saw_b && !saw_u && !saw_r && !saw_f => saw_u = true,
                Some('r' | 'R') if !saw_r && !saw_u => saw_r = true,
                Some('f' | 'F') if !saw_f && !saw_b && !saw_u => saw_f = true,
                _ => break,
            }
            c = self.nextc();
            if let Some(q @ ('"' | '\'')) = c {
                return self.string_literal(start, q);
            }
        }
        let mut nonascii = false;
        while let Some(ch) = c.filter(|&ch| is_potential_identifier_char(ch)) {
            nonascii |= !ch.is_ascii();
            c = self.nextc();
        }
        self.backup(c);
        if nonascii {
            self.verify_identifier(start)?;
        }
        Ok(self.token(TokenType::Name, start))
    }

    /// `verify_identifier`: o primeiro caractere fora de `XID_Start`/`XID_Continue` é o erro.
    fn verify_identifier(&mut self, start: usize) -> Result<(), TokenizeError> {
        let bad = self.src[start..self.pos].iter().enumerate().find(|&(i, &c)| {
            if i == 0 {
                !(c == '_' || is_xid_start(c))
            } else {
                !is_xid_continue(c)
            }
        });
        let Some((i, &ch)) = bad else { return Ok(()) };
        self.pos = start + i + 1;
        let msg = if is_printable(ch) {
            format!("invalid character '{ch}' (U+{:04X})", ch as u32)
        } else {
            format!("invalid non-printable character U+{:04X}", ch as u32)
        };
        Err(self.syntax_error(msg))
    }

    /// `lookahead`: o fonte continua com `test` seguido de algo que não pode estar num nome.
    fn lookahead(&self, test: &str) -> bool {
        let mut i = self.pos;
        for t in test.chars() {
            if self.src.get(i) != Some(&t) {
                return false;
            }
            i += 1;
        }
        !self.src.get(i).is_some_and(|&c| is_potential_identifier_char(c))
    }

    /// `verify_end_of_number`: `c` é o caractere lido depois do número.
    fn verify_end_of_number(&mut self, c: Option<char>, kind: &str) -> Result<(), TokenizeError> {
        if self.mode == Mode::Tokenize {
            return Ok(());
        }
        let Some(ch) = c else { return Ok(()) };
        // Palavras-chave que podem vir logo depois de um número em código válido viram aviso.
        let keyword = match ch {
            'a' => self.lookahead("nd"),
            'e' => self.lookahead("lse"),
            'f' => self.lookahead("or"),
            'i' => matches!(self.src.get(self.pos), Some('f' | 'n' | 's')),
            'o' => self.lookahead("r"),
            'n' => self.lookahead("ot"),
            _ => false,
        };
        if keyword {
            self.backup(c);
            self.warnings.push(Warning {
                msg: format!("invalid {kind} literal"),
                line: self.line,
                offset: self.pos - self.line_start,
            });
            self.nextc();
        } else if ch.is_ascii() && is_potential_identifier_char(ch) {
            self.backup(c);
            return Err(self.syntax_error(format!("invalid {kind} literal")));
        }
        Ok(())
    }

    /// `tok_decimal_tail`: dígitos com `_` entre grupos; devolve o primeiro caractere depois.
    fn decimal_tail(&mut self) -> Result<Option<char>, TokenizeError> {
        loop {
            let mut c = self.nextc();
            while is_digit(c) {
                c = self.nextc();
            }
            if c != Some('_') {
                return Ok(c);
            }
            c = self.nextc();
            if !is_digit(c) {
                self.backup(c);
                return Err(self.syntax_error("invalid decimal literal"));
            }
        }
    }

    /// Dígitos de um inteiro com prefixo (`0x`, `0o`, `0b`); `c` é o caractere depois do prefixo.
    /// `bad_digit` liga o erro "invalid digit 'N' in ... literal" do octal e do binário.
    fn radix_digits(
        &mut self,
        mut c: Option<char>,
        valid: fn(char) -> bool,
        kind: &str,
        bad_digit: bool,
    ) -> Result<Option<char>, TokenizeError> {
        loop {
            if c == Some('_') {
                c = self.nextc();
            }
            if !c.is_some_and(valid) {
                if let Some(d) = c.filter(|d| bad_digit && d.is_ascii_digit()) {
                    return Err(self.syntax_error(format!("invalid digit '{d}' in {kind} literal")));
                }
                self.backup(c);
                return Err(self.syntax_error(format!("invalid {kind} literal")));
            }
            loop {
                c = self.nextc();
                if !c.is_some_and(valid) {
                    break;
                }
            }
            if c != Some('_') {
                break;
            }
        }
        if let Some(d) = c.filter(|d| bad_digit && d.is_ascii_digit()) {
            return Err(self.syntax_error(format!("invalid digit '{d}' in {kind} literal")));
        }
        Ok(c)
    }

    /// Número começando por dígito (`first` já lido).
    fn number_start(&mut self, start: usize, first: char) -> Result<Token, TokenizeError> {
        if first != '0' {
            let c = self.decimal_tail()?;
            if c == Some('.') {
                let c = self.nextc();
                return self.number(start, c, NumberStage::Fraction);
            }
            return self.number(start, c, NumberStage::Suffix);
        }
        let mut c = self.nextc();
        match c {
            Some('x' | 'X') => {
                let c = self.nextc();
                let c = self.radix_digits(c, |d| d.is_ascii_hexdigit(), "hexadecimal", false)?;
                self.verify_end_of_number(c, "hexadecimal")?;
                self.number(start, c, NumberStage::End)
            }
            Some('o' | 'O') => {
                let c = self.nextc();
                let c = self.radix_digits(c, |d| ('0'..='7').contains(&d), "octal", true)?;
                self.verify_end_of_number(c, "octal")?;
                self.number(start, c, NumberStage::End)
            }
            Some('b' | 'B') => {
                let c = self.nextc();
                let c = self.radix_digits(c, |d| d == '0' || d == '1', "binary", true)?;
                self.verify_end_of_number(c, "binary")?;
                self.number(start, c, NumberStage::End)
            }
            _ => {
                // Talvez octal no estilo antigo; `0` sozinho (ou `00`, `0_0`) é válido.
                loop {
                    if c == Some('_') {
                        c = self.nextc();
                        if !is_digit(c) {
                            self.backup(c);
                            return Err(self.syntax_error("invalid decimal literal"));
                        }
                    }
                    if c != Some('0') {
                        break;
                    }
                    c = self.nextc();
                }
                let zeros_end = self.pos;
                let mut nonzero = false;
                if is_digit(c) {
                    nonzero = true;
                    c = self.decimal_tail()?;
                }
                match c {
                    Some('.') => {
                        let c = self.nextc();
                        self.number(start, c, NumberStage::Fraction)
                    }
                    Some('e' | 'E') => self.number(start, c, NumberStage::Exponent),
                    Some('j' | 'J') => self.number(start, c, NumberStage::Imaginary),
                    _ => {
                        if nonzero && self.mode == Mode::Parser {
                            self.backup(c);
                            // `syntaxerror_known_range`: deslocamentos em bytes, como o C os passa.
                            let offset = self.byte_len(self.line_start, start + 1);
                            let end_offset = self.byte_len(self.line_start, zeros_end);
                            return Err(TokenizeError {
                                kind: ErrorKind::Syntax,
                                msg: "leading zeros in decimal integer literals are not permitted; \
                                      use an 0o prefix for octal integers"
                                    .to_string(),
                                line: self.line,
                                offset,
                                end_line: self.line,
                                end_offset,
                            });
                        }
                        self.verify_end_of_number(c, "decimal")?;
                        self.number(start, c, NumberStage::End)
                    }
                }
            }
        }
    }

    /// Parte fracionária, expoente e sufixo imaginário (os `goto` do C).
    fn number(&mut self, start: usize, mut c: Option<char>, mut stage: NumberStage) -> Result<Token, TokenizeError> {
        loop {
            stage = match stage {
                NumberStage::Fraction => {
                    if is_digit(c) {
                        c = self.decimal_tail()?;
                    }
                    NumberStage::Suffix
                }
                NumberStage::Suffix => match c {
                    Some('e' | 'E') => NumberStage::Exponent,
                    Some('j' | 'J') => NumberStage::Imaginary,
                    _ => {
                        self.verify_end_of_number(c, "decimal")?;
                        NumberStage::End
                    }
                },
                NumberStage::Exponent => {
                    let e = c;
                    c = self.nextc();
                    if matches!(c, Some('+' | '-')) {
                        c = self.nextc();
                        if !is_digit(c) {
                            self.backup(c);
                            return Err(self.syntax_error("invalid decimal literal"));
                        }
                    } else if !is_digit(c) {
                        // `1e` seguido de outra coisa: o número termina antes do `e`.
                        self.backup(c);
                        self.verify_end_of_number(e, "decimal")?;
                        self.backup(e);
                        return Ok(self.token(TokenType::Number, start));
                    }
                    c = self.decimal_tail()?;
                    if matches!(c, Some('j' | 'J')) {
                        NumberStage::Imaginary
                    } else {
                        self.verify_end_of_number(c, "decimal")?;
                        NumberStage::End
                    }
                }
                NumberStage::Imaginary => {
                    c = self.nextc();
                    self.verify_end_of_number(c, "imaginary")?;
                    NumberStage::End
                }
                NumberStage::End => {
                    self.backup(c);
                    return Ok(self.token(TokenType::Number, start));
                }
            };
        }
    }

    /// Literal de string; `start` aponta para o início do prefixo (ou para a aspa) e a aspa de
    /// abertura `quote` já foi lida. Encontra o fim do literal como o C: aspas simples ou triplas,
    /// barra invertida pulando o caractere seguinte (inclusive a nova linha) e os erros de literal
    /// não terminado. f-strings (FSTRING_START/MIDDLE/END) são a fatia 4 do plano.
    fn string_literal(&mut self, start: usize, quote: char) -> Result<Token, TokenizeError> {
        if self.src[start..self.pos].iter().any(|&c| c == 'f' || c == 'F') {
            let offset = start - self.line_start + 1;
            return Err(self.error(
                ErrorKind::Unsupported,
                "f-string literals are not tokenized yet".to_string(),
                offset,
            ));
        }
        let start_pos = self.here(start);
        let first_line = self.line;
        let multi_line_start = self.line_start;
        let mut quote_size = 1;
        let mut end_quote_size = 0;
        let mut c = self.nextc();
        if c == Some(quote) {
            c = self.nextc();
            if c == Some(quote) {
                quote_size = 3;
            } else {
                end_quote_size = 1;
            }
        }
        if c != Some(quote) {
            self.backup(c);
        }
        while end_quote_size != quote_size {
            let c = self.nextc();
            if c.is_none() || (quote_size == 1 && c == Some('\n')) {
                // Com EOF, a última nova linha já foi contada.
                let detected = if c.is_none() { self.line - 1 } else { self.line };
                let offset = start - multi_line_start + 1;
                let msg = if quote_size == 3 {
                    format!("unterminated triple-quoted string literal (detected at line {detected})")
                } else {
                    format!("unterminated string literal (detected at line {detected})")
                };
                return Err(TokenizeError {
                    kind: ErrorKind::Syntax,
                    msg,
                    line: first_line,
                    offset,
                    end_line: first_line,
                    end_offset: offset,
                });
            }
            if c == Some(quote) {
                end_quote_size += 1;
                continue;
            }
            end_quote_size = 0;
            let mut c = c;
            if c == Some('\\') {
                c = self.nextc();
            }
            if c == Some('\n') {
                self.new_line();
            }
        }
        Ok(Token { kind: TokenType::String, text: self.text(start, self.pos), start: start_pos, end: self.here(self.pos) })
    }
}

/// Todos os tokens de `source`, até o ENDMARKER inclusive.
pub fn tokenize(source: &str, mode: Mode) -> Result<Vec<Token>, TokenizeError> {
    let mut tokenizer = Tokenizer::new(source, mode);
    let mut out = Vec::new();
    loop {
        let tok = tokenizer.next_token()?;
        let end = tok.kind == TokenType::Endmarker;
        out.push(tok);
        if end {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(src: &str, mode: Mode) -> Vec<String> {
        tokenize(src, mode)
            .unwrap()
            .iter()
            .map(|t| {
                format!(
                    "{},{}-{},{}:{}:'{}'",
                    t.start.line,
                    t.start.col,
                    t.end.line,
                    t.end.col,
                    t.kind.name(),
                    t.text.replace('\n', "\\n")
                )
            })
            .collect()
    }

    fn err(src: &str, mode: Mode) -> TokenizeError {
        tokenize(src, mode).unwrap_err()
    }

    #[test]
    fn assignment() {
        assert_eq!(
            dump("x = 1\n", Mode::Tokenize),
            [
                "1,0-1,1:NAME:'x'",
                "1,2-1,3:EQUAL:'='",
                "1,4-1,5:NUMBER:'1'",
                "1,5-1,6:NEWLINE:'\\n'",
                "2,0-2,0:ENDMARKER:''",
            ]
        );
    }

    #[test]
    fn indent_and_dedent() {
        assert_eq!(
            dump("if x:\n    y = 2\nz\n", Mode::Tokenize),
            [
                "1,0-1,2:NAME:'if'",
                "1,3-1,4:NAME:'x'",
                "1,4-1,5:COLON:':'",
                "1,5-1,6:NEWLINE:'\\n'",
                "2,0-2,4:INDENT:'    '",
                "2,4-2,5:NAME:'y'",
                "2,6-2,7:EQUAL:'='",
                "2,8-2,9:NUMBER:'2'",
                "2,9-2,10:NEWLINE:'\\n'",
                "3,0-3,0:DEDENT:''",
                "3,0-3,1:NAME:'z'",
                "3,1-3,2:NEWLINE:'\\n'",
                "4,0-4,0:ENDMARKER:''",
            ]
        );
    }

    #[test]
    fn comments_and_blank_lines() {
        assert_eq!(
            dump("# c\n\nx  # t\n", Mode::Tokenize),
            [
                "1,0-1,3:COMMENT:'# c'",
                "1,3-1,4:NL:'\\n'",
                "2,0-2,1:NL:'\\n'",
                "3,0-3,1:NAME:'x'",
                "3,3-3,6:COMMENT:'# t'",
                "3,6-3,7:NEWLINE:'\\n'",
                "4,0-4,0:ENDMARKER:''",
            ]
        );
        assert_eq!(
            dump("# c\n\nx  # t\n", Mode::Parser),
            ["3,0-3,1:NAME:'x'", "3,6-3,7:NEWLINE:'\\n'", "4,0-4,0:ENDMARKER:''"]
        );
    }

    #[test]
    fn open_parenthesis_spans_lines() {
        assert_eq!(
            dump("f(a,\n  b)\n", Mode::Tokenize),
            [
                "1,0-1,1:NAME:'f'",
                "1,1-1,2:LPAR:'('",
                "1,2-1,3:NAME:'a'",
                "1,3-1,4:COMMA:','",
                "1,4-1,5:NL:'\\n'",
                "2,2-2,3:NAME:'b'",
                "2,3-2,4:RPAR:')'",
                "2,4-2,5:NEWLINE:'\\n'",
                "3,0-3,0:ENDMARKER:''",
            ]
        );
    }

    #[test]
    fn backslash_continuation() {
        assert_eq!(
            dump("x = 1 + \\\n    2\n", Mode::Tokenize),
            [
                "1,0-1,1:NAME:'x'",
                "1,2-1,3:EQUAL:'='",
                "1,4-1,5:NUMBER:'1'",
                "1,6-1,7:PLUS:'+'",
                "2,4-2,5:NUMBER:'2'",
                "2,5-2,6:NEWLINE:'\\n'",
                "3,0-3,0:ENDMARKER:''",
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(
            dump("0x_1f 0o17 0b1_0 1_000 3.14 .5 1e-3 2j 1.5J 0 00 1if\n", Mode::Tokenize),
            [
                "1,0-1,5:NUMBER:'0x_1f'",
                "1,6-1,10:NUMBER:'0o17'",
                "1,11-1,16:NUMBER:'0b1_0'",
                "1,17-1,22:NUMBER:'1_000'",
                "1,23-1,27:NUMBER:'3.14'",
                "1,28-1,30:NUMBER:'.5'",
                "1,31-1,35:NUMBER:'1e-3'",
                "1,36-1,38:NUMBER:'2j'",
                "1,39-1,43:NUMBER:'1.5J'",
                "1,44-1,45:NUMBER:'0'",
                "1,46-1,48:NUMBER:'00'",
                "1,49-1,50:NUMBER:'1'",
                "1,50-1,52:NAME:'if'",
                "1,52-1,53:NEWLINE:'\\n'",
                "2,0-2,0:ENDMARKER:''",
            ]
        );
    }

    #[test]
    fn operators() {
        assert_eq!(
            dump("a **= b // c -> d != e <<= f ... g := h\n", Mode::Tokenize),
            [
                "1,0-1,1:NAME:'a'",
                "1,2-1,5:DOUBLESTAREQUAL:'**='",
                "1,6-1,7:NAME:'b'",
                "1,8-1,10:DOUBLESLASH:'//'",
                "1,11-1,12:NAME:'c'",
                "1,13-1,15:RARROW:'->'",
                "1,16-1,17:NAME:'d'",
                "1,18-1,20:NOTEQUAL:'!='",
                "1,21-1,22:NAME:'e'",
                "1,23-1,26:LEFTSHIFTEQUAL:'<<='",
                "1,27-1,28:NAME:'f'",
                "1,29-1,32:ELLIPSIS:'...'",
                "1,33-1,34:NAME:'g'",
                "1,35-1,37:COLONEQUAL:':='",
                "1,38-1,39:NAME:'h'",
                "1,39-1,40:NEWLINE:'\\n'",
                "2,0-2,0:ENDMARKER:''",
            ]
        );
        let toks = tokenize("a<>b\n", Mode::Tokenize).unwrap();
        assert_eq!(toks[1].kind, TokenType::NotEqual);
        assert_eq!(toks[1].generic_kind(), TokenType::Op);
    }

    #[test]
    fn nested_dedent_without_final_newline() {
        assert_eq!(
            dump("def f():\n  if x:\n    return\nx", Mode::Tokenize),
            [
                "1,0-1,3:NAME:'def'",
                "1,4-1,5:NAME:'f'",
                "1,5-1,6:LPAR:'('",
                "1,6-1,7:RPAR:')'",
                "1,7-1,8:COLON:':'",
                "1,8-1,9:NEWLINE:'\\n'",
                "2,0-2,2:INDENT:'  '",
                "2,2-2,4:NAME:'if'",
                "2,5-2,6:NAME:'x'",
                "2,6-2,7:COLON:':'",
                "2,7-2,8:NEWLINE:'\\n'",
                "3,0-3,4:INDENT:'    '",
                "3,4-3,10:NAME:'return'",
                "3,10-3,11:NEWLINE:'\\n'",
                "4,0-4,0:DEDENT:''",
                "4,0-4,0:DEDENT:''",
                "4,0-4,1:NAME:'x'",
                "4,1-4,2:NEWLINE:''",
                "5,0-5,0:ENDMARKER:''",
            ]
        );
    }

    #[test]
    fn simple_strings() {
        assert_eq!(
            dump("s = 'a\\'b' + \"\"\n", Mode::Tokenize),
            [
                "1,0-1,1:NAME:'s'",
                "1,2-1,3:EQUAL:'='",
                "1,4-1,10:STRING:''a\\'b''",
                "1,11-1,12:PLUS:'+'",
                "1,13-1,15:STRING:'\"\"'",
                "1,15-1,16:NEWLINE:'\\n'",
                "2,0-2,0:ENDMARKER:''",
            ]
        );
        assert_eq!(
            dump("'''a\nb'''\n", Mode::Tokenize),
            ["1,0-2,4:STRING:''''a\\nb''''", "2,4-2,5:NEWLINE:'\\n'", "3,0-3,0:ENDMARKER:''"]
        );
    }

    #[test]
    fn unicode_identifier() {
        let toks = tokenize("ação = 1\n", Mode::Tokenize).unwrap();
        assert_eq!(toks[0].text, "ação");
        assert_eq!((toks[0].end.col, toks[0].end.byte_col), (4, 6));
        assert_eq!((toks[1].start.col, toks[1].start.byte_col), (5, 7));
        let e = err("a € b\n", Mode::Parser);
        assert_eq!(e.msg, "invalid character '€' (U+20AC)");
        assert_eq!((e.line, e.offset), (1, 3));
    }

    #[test]
    fn indentation_errors() {
        let e = err("if x:\n    a\n  b\n", Mode::Parser);
        assert_eq!(e.kind, ErrorKind::Indentation);
        assert_eq!(e.msg, "unindent does not match any outer indentation level");
        assert_eq!(e.line, 3);
        let e = err("if x:\n\ta\n        b\n", Mode::Parser);
        assert_eq!(e.kind, ErrorKind::Tab);
        assert_eq!(e.msg, "inconsistent use of tabs and spaces in indentation");
        assert_eq!(e.line, 3);
    }

    #[test]
    fn number_errors() {
        let e = err("1_\n", Mode::Parser);
        assert_eq!((e.msg.as_str(), e.offset), ("invalid decimal literal", 2));
        let e = err("0o8\n", Mode::Parser);
        assert_eq!((e.msg.as_str(), e.offset), ("invalid digit '8' in octal literal", 3));
        let e = err("012\n", Mode::Parser);
        assert!(e.msg.starts_with("leading zeros in decimal integer literals"));
        assert_eq!((e.offset, e.end_offset), (1, 2));
        let e = err("1abc\n", Mode::Parser);
        assert_eq!((e.msg.as_str(), e.offset), ("invalid decimal literal", 1));
        assert_eq!(err("0x\n", Mode::Tokenize).msg, "invalid hexadecimal literal");
        let mut t = Tokenizer::new("1if x else y\n", Mode::Parser);
        while t.next_token().unwrap().kind != TokenType::Endmarker {}
        assert_eq!(t.warnings().len(), 1);
        assert_eq!(t.warnings()[0].msg, "invalid decimal literal");
    }

    #[test]
    fn string_and_bracket_errors() {
        let e = err("'abc\n", Mode::Parser);
        assert_eq!(e.msg, "unterminated string literal (detected at line 1)");
        assert_eq!((e.line, e.offset), (1, 1));
        let e = err("(1\n", Mode::Parser);
        assert_eq!((e.msg.as_str(), e.line, e.offset), ("'(' was never closed", 1, 1));
        let e = err(")", Mode::Parser);
        assert_eq!((e.msg.as_str(), e.offset), ("unmatched ')'", 1));
        let e = err("(]", Mode::Parser);
        assert_eq!(e.msg, "closing parenthesis ']' does not match opening parenthesis '('");
        assert_eq!(e.offset, 2);
        assert!(tokenize(")", Mode::Tokenize).is_ok());
        let e = err("x \\ y\n", Mode::Parser);
        assert_eq!(e.msg, "unexpected character after line continuation character");
    }
}
