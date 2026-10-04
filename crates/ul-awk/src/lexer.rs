//! Analisador léxico do awk com as regras do gawk 5.2.1.
//!
//! O lexer é puxado pelo parser token a token. A ambiguidade entre divisão e regex (`/`) é resolvida
//! pelo parser: quando ele espera um operando e recebe `/` ou `/=`, pede [`Lexer::read_regex`], que
//! relê a partir da barra. Cada token guarda o deslocamento em bytes e a linha, pra mensagem de erro
//! com circunflexo no formato do gawk.

use std::rc::Rc;

/// Tipo de token.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Newline,
    Eof,
    Semi,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Not,
    Gt,
    Lt,
    Pipe,
    PipeAmp,
    Question,
    Colon,
    Tilde,
    NoMatch,
    Dollar,
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    Eq,
    Le,
    Ge,
    Ne,
    Incr,
    Decr,
    And,
    Or,
    Append,
    Number(f64),
    Str(Rc<[u8]>),
    /// Corpo de uma regex constante, como está no fonte (sem as barras).
    Regex(Rc<[u8]>),
    /// `@/.../`: regex tipada.
    TypedRegex(Rc<[u8]>),
    /// Nome seguido imediatamente de `(` (chamada de função do usuário).
    FuncName(Rc<str>),
    Name(Rc<str>),
    Builtin(&'static str),
    /// `@nome` (chamada indireta).
    IndirectName(Rc<str>),
    /// `@include`, `@load`, `@namespace`.
    Directive(&'static str),
    /// Nome qualificado inválido: a mensagem de erro (o parser acusa erro de sintaxe no token seguinte).
    BadName(String),
    // palavras reservadas
    Begin,
    End,
    BeginFile,
    EndFile,
    Function,
    If,
    Else,
    While,
    For,
    Do,
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    Return,
    Delete,
    Getline,
    Print,
    Printf,
    In,
    Switch,
    Case,
    Default,
}

/// Nomes das funções embutidas do gawk 5.2.1.
pub const BUILTINS: &[&str] = &[
    "and", "asort", "asorti", "atan2", "bindtextdomain", "close", "compl", "cos", "dcgettext", "dcngettext", "exp",
    "fflush", "gensub", "gsub", "index", "int", "isarray", "length", "log", "lshift", "match", "mkbool", "mktime",
    "or", "patsplit", "rand", "rshift", "sin", "split", "sprintf", "sqrt", "srand", "strftime", "strtonum", "sub",
    "substr", "system", "systime", "tolower", "toupper", "typeof", "xor",
];

fn keyword(name: &str) -> Option<Tok> {
    Some(match name {
        "BEGIN" => Tok::Begin,
        "END" => Tok::End,
        "BEGINFILE" => Tok::BeginFile,
        "ENDFILE" => Tok::EndFile,
        "function" | "func" => Tok::Function,
        "if" => Tok::If,
        "else" => Tok::Else,
        "while" => Tok::While,
        "for" => Tok::For,
        "do" => Tok::Do,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "next" => Tok::Next,
        "nextfile" => Tok::NextFile,
        "exit" => Tok::Exit,
        "return" => Tok::Return,
        "delete" => Tok::Delete,
        "getline" => Tok::Getline,
        "print" => Tok::Print,
        "printf" => Tok::Printf,
        "in" => Tok::In,
        "switch" => Tok::Switch,
        "case" => Tok::Case,
        "default" => Tok::Default,
        _ => return None,
    })
}

/// Palavras reservadas (pra mensagens de erro de nome inválido).
pub fn is_reserved(name: &str) -> bool {
    keyword(name).is_some() || BUILTINS.contains(&name)
}

/// Funções embutidas do POSIX: nunca podem ser redefinidas nem usadas como nome, em namespace nenhum.
/// As extensões do gawk podem ser sombreadas fora do namespace `awk` (e como nome de parâmetro).
pub fn is_posix_builtin(name: &str) -> bool {
    matches!(
        name,
        "atan2" | "close" | "cos" | "exp" | "fflush" | "gsub" | "index" | "int" | "length" | "log" | "match"
            | "rand" | "sin" | "split" | "sprintf" | "sqrt" | "srand" | "sub" | "substr" | "system" | "tolower"
            | "toupper"
    )
}

/// Um token com a posição.
#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    /// Deslocamento em bytes do início do token no fonte.
    pub start: usize,
    /// Linha (a partir de 1) onde o token começa.
    pub line: u32,
    /// Verdadeiro se havia espaço (ou comentário) entre o token anterior e este.
    pub space_before: bool,
    /// Newline sintético do fim do fonte.
    pub synthetic: bool,
}

/// Erro léxico: mensagem e posição do circunflexo.
#[derive(Clone, Debug)]
pub struct LexError {
    pub message: String,
    pub pos: usize,
    pub line: u32,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    /// Já devolveu o newline sintético do fim.
    eof_newline: bool,
    /// Avisos (escape desconhecido etc.), com a linha.
    pub warnings: Vec<(u32, String)>,
    last_significant: Option<Tok>,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a [u8]) -> Lexer<'a> {
        Lexer { src, pos: 0, line: 1, eof_newline: false, warnings: Vec::new(), last_significant: None }
    }

    pub fn source(&self) -> &'a [u8] {
        self.src
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn byte_at(&self, i: usize) -> Option<u8> {
        self.src.get(i).copied()
    }

    /// Pula espaços, comentários e continuações de linha. Devolve se pulou algo.
    fn skip_space(&mut self) -> bool {
        let start = self.pos;
        loop {
            match self.peek_byte() {
                Some(b' ' | b'\t' | b'\r') => self.pos += 1,
                Some(b'\\') => {
                    if self.byte_at(self.pos + 1) == Some(b'\n') {
                        self.pos += 2;
                        self.line += 1;
                    } else if self.byte_at(self.pos + 1) == Some(b'\r') && self.byte_at(self.pos + 2) == Some(b'\n') {
                        self.pos += 3;
                        self.line += 1;
                    } else {
                        break;
                    }
                }
                Some(b'#') => {
                    while let Some(c) = self.peek_byte() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
        self.pos != start
    }

    /// Próximo token.
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        let space_before = self.skip_space();
        let start = self.pos;
        let line = self.line;
        let mk = |tok: Tok| Token { tok, start, line, space_before, synthetic: false };
        let Some(c) = self.peek_byte() else {
            // Fonte sem newline final ganha um newline sintético (o gawk faz o mesmo).
            if !self.eof_newline && self.src.last().is_some_and(|b| *b != b'\n') {
                self.eof_newline = true;
                let t = Token { tok: Tok::Newline, start, line, space_before, synthetic: true };
                self.last_significant = Some(Tok::Newline);
                return Ok(t);
            }
            return Ok(mk(Tok::Eof));
        };
        if c == 0 {
            return Err(LexError { message: "\0nul".into(), pos: start, line });
        }
        self.pos += 1;
        let two = |s: &mut Self, next: u8, yes: Tok, no: Tok| -> Tok {
            if s.peek_byte() == Some(next) {
                s.pos += 1;
                yes
            } else {
                no
            }
        };
        let tok = match c {
            b'\n' => {
                self.line += 1;
                Tok::Newline
            }
            b';' => Tok::Semi,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b',' => Tok::Comma,
            b'?' => Tok::Question,
            b':' => Tok::Colon,
            b'~' => Tok::Tilde,
            b'$' => Tok::Dollar,
            b'+' => match self.peek_byte() {
                Some(b'+') => {
                    self.pos += 1;
                    Tok::Incr
                }
                Some(b'=') => {
                    self.pos += 1;
                    Tok::AddAssign
                }
                _ => Tok::Plus,
            },
            b'-' => match self.peek_byte() {
                Some(b'-') => {
                    self.pos += 1;
                    Tok::Decr
                }
                Some(b'=') => {
                    self.pos += 1;
                    Tok::SubAssign
                }
                _ => Tok::Minus,
            },
            b'*' => {
                if self.peek_byte() == Some(b'*') {
                    self.pos += 1;
                    two(self, b'=', Tok::PowAssign, Tok::Caret)
                } else {
                    two(self, b'=', Tok::MulAssign, Tok::Star)
                }
            }
            b'/' => two(self, b'=', Tok::DivAssign, Tok::Slash),
            b'%' => two(self, b'=', Tok::ModAssign, Tok::Percent),
            b'^' => two(self, b'=', Tok::PowAssign, Tok::Caret),
            b'=' => two(self, b'=', Tok::Eq, Tok::Assign),
            b'!' => match self.peek_byte() {
                Some(b'=') => {
                    self.pos += 1;
                    Tok::Ne
                }
                Some(b'~') => {
                    self.pos += 1;
                    Tok::NoMatch
                }
                _ => Tok::Not,
            },
            b'<' => two(self, b'=', Tok::Le, Tok::Lt),
            b'>' => match self.peek_byte() {
                Some(b'=') => {
                    self.pos += 1;
                    Tok::Ge
                }
                Some(b'>') => {
                    self.pos += 1;
                    Tok::Append
                }
                _ => Tok::Gt,
            },
            b'|' => match self.peek_byte() {
                Some(b'|') => {
                    self.pos += 1;
                    Tok::Or
                }
                Some(b'&') => {
                    self.pos += 1;
                    Tok::PipeAmp
                }
                _ => Tok::Pipe,
            },
            b'&' => {
                if self.peek_byte() == Some(b'&') {
                    self.pos += 1;
                    Tok::And
                } else {
                    self.pos = start;
                    return Err(LexError { message: "syntax error".into(), pos: start, line });
                }
            }
            b'"' => {
                let s = self.read_string(start, line)?;
                Tok::Str(s)
            }
            b'0'..=b'9' | b'.' => {
                if c == b'.' && !self.peek_byte().is_some_and(|d| d.is_ascii_digit()) {
                    self.pos = start + 1;
                    return Err(LexError { message: "syntax error".into(), pos: start, line });
                }
                self.pos = start;
                Tok::Number(self.read_number())
            }
            b'@' => self.read_at(start, line)?,
            c if c.is_ascii_alphabetic() || c == b'_' => {
                self.pos = start;
                let name = self.read_name();
                if let Some((ns, rest)) = name.split_once("::") {
                    // Nome qualificado: nenhum dos dois componentes pode ser palavra reservada.
                    if is_reserved(ns) {
                        let msg = format!("using reserved identifier `{ns}' as a namespace is not allowed");
                        return Ok(Token { tok: Tok::BadName(msg), start, line, space_before, synthetic: false });
                    }
                    if is_reserved(rest) {
                        let msg = format!("using reserved identifier `{rest}' as second component of a qualified name is not allowed");
                        return Ok(Token { tok: Tok::BadName(msg), start, line, space_before, synthetic: false });
                    }
                }
                if let Some(k) = keyword(&name) {
                    k
                } else if let Some(b) = BUILTINS.iter().find(|b| **b == name) {
                    Tok::Builtin(b)
                } else if self.peek_byte() == Some(b'(') {
                    Tok::FuncName(Rc::from(name))
                } else {
                    Tok::Name(Rc::from(name))
                }
            }
            _ => {
                // Caractere inválido no fonte (o gawk diz "invalid char 'x' in expression").
                let ch = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                return Err(LexError { message: format!("invalid char '{ch}' in expression"), pos: start, line });
            }
        };
        self.last_significant = Some(tok.clone());
        Ok(mk(tok))
    }

    fn read_name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek_byte() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.pos += 1;
            } else if c == b':' && self.byte_at(self.pos + 1) == Some(b':') {
                // Nome qualificado de namespace (`ns::nome`).
                if self.byte_at(self.pos + 2).is_some_and(|d| d.is_ascii_alphabetic() || d == b'_') {
                    self.pos += 2;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn read_at(&mut self, start: usize, line: u32) -> Result<Tok, LexError> {
        match self.peek_byte() {
            Some(b'/') => {
                self.pos += 1;
                let body = self.scan_regex_body(start + 1, line)?;
                Ok(Tok::TypedRegex(body))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let name = self.read_name();
                match name.as_str() {
                    "include" => Ok(Tok::Directive("include")),
                    "load" => Ok(Tok::Directive("load")),
                    "namespace" => Ok(Tok::Directive("namespace")),
                    _ => Ok(Tok::IndirectName(Rc::from(name))),
                }
            }
            _ => Err(LexError { message: "syntax error".into(), pos: start, line }),
        }
    }

    /// Número no fonte: decimal, com fração e expoente; hexadecimal `0x` e octal com zero à esquerda,
    /// como o gawk fora do modo POSIX.
    fn read_number(&mut self) -> f64 {
        let start = self.pos;
        let s = self.src;
        if s[start] == b'0' && matches!(s.get(start + 1), Some(b'x' | b'X')) && s.get(start + 2).is_some_and(|c| c.is_ascii_hexdigit()) {
            let mut i = start + 2;
            let mut v = 0f64;
            while let Some(c) = s.get(i).filter(|c| c.is_ascii_hexdigit()) {
                v = v * 16.0 + (*c as char).to_digit(16).unwrap_or(0) as f64;
                i += 1;
            }
            self.pos = i;
            return v;
        }
        let mut i = start;
        while s.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        let int_end = i;
        let mut is_float = false;
        if s.get(i) == Some(&b'.') {
            is_float = true;
            i += 1;
            while s.get(i).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
            }
        }
        if matches!(s.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(s.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if s.get(j).is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                while s.get(j).is_some_and(|c| c.is_ascii_digit()) {
                    j += 1;
                }
                i = j;
            }
        }
        self.pos = i;
        let text = std::str::from_utf8(&s[start..i]).unwrap_or("0");
        // Octal: zero à esquerda, só dígitos 0-7, sem ponto nem expoente.
        if !is_float && int_end - start > 1 && s[start] == b'0' && s[start..int_end].iter().all(|c| (b'0'..=b'7').contains(c)) {
            let mut v = 0f64;
            for c in &s[start..int_end] {
                v = v * 8.0 + (c - b'0') as f64;
            }
            return v;
        }
        text.parse::<f64>().unwrap_or(0.0)
    }

    /// Lê uma string entre aspas (a aspa de abertura já foi consumida), processando os escapes.
    fn read_string(&mut self, start: usize, line: u32) -> Result<Rc<[u8]>, LexError> {
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek_byte() else {
                return Err(LexError { message: "unterminated string".into(), pos: start, line });
            };
            self.pos += 1;
            match c {
                0 => return Err(LexError { message: "\0nul".into(), pos: start, line }),
                b'"' => break,
                b'\n' => {
                    self.pos -= 1;
                    return Err(LexError { message: "unterminated string".into(), pos: start, line });
                }
                b'\\' => {
                    let Some(e) = self.peek_byte() else {
                        return Err(LexError { message: "unterminated string".into(), pos: start, line });
                    };
                    self.pos += 1;
                    match e {
                        b'\n' => {
                            self.line += 1;
                        }
                        b'\r' if self.peek_byte() == Some(b'\n') => {
                            self.pos += 1;
                            self.line += 1;
                        }
                        _ => {
                            let lineno = self.line;
                            // Volta pra barra: `escape_sequence` começa nela.
                            self.pos -= 2;
                            let (bytes, warn) = escape_sequence(self.src, &mut self.pos, false);
                            out.extend_from_slice(&bytes);
                            if let Some(w) = warn {
                                self.warnings.push((lineno, w));
                            }
                        }
                    }
                }
                _ => out.push(c),
            }
        }
        Ok(Rc::from(out))
    }

    /// Relê a partir da barra de um token `/` ou `/=` como regex constante.
    pub fn read_regex(&mut self, slash: &Token) -> Result<Token, LexError> {
        self.pos = slash.start + 1;
        self.line = slash.line;
        let body = self.scan_regex_body(slash.start + 1, slash.line)?;
        let tok = Tok::Regex(body);
        self.last_significant = Some(tok.clone());
        Ok(Token { tok, start: slash.start + 1, line: slash.line, space_before: slash.space_before, synthetic: false })
    }

    /// Lê o corpo da regex até a barra de fechamento (fora de colchetes), mantendo os escapes.
    fn scan_regex_body(&mut self, err_pos: usize, line: u32) -> Result<Rc<[u8]>, LexError> {
        let mut out = Vec::new();
        let mut in_bracket = false;
        let mut bracket_start = 0usize;
        loop {
            let Some(c) = self.peek_byte() else {
                return Err(LexError { message: "unterminated regexp".into(), pos: err_pos, line });
            };
            match c {
                0 => return Err(LexError { message: "\0nul".into(), pos: err_pos, line }),
                b'\n' => {
                    return Err(LexError { message: "unterminated regexp".into(), pos: err_pos, line });
                }
                b'\\' => {
                    if self.byte_at(self.pos + 1) == Some(0) {
                        return Err(LexError { message: "\0nul".into(), pos: err_pos, line });
                    }
                    match self.byte_at(self.pos + 1) {
                        Some(b'\n') => {
                            // Continuação de linha dentro da regex.
                            self.pos += 2;
                            self.line += 1;
                            continue;
                        }
                        Some(b'\r') if self.byte_at(self.pos + 2) == Some(b'\n') => {
                            self.pos += 3;
                            self.line += 1;
                            continue;
                        }
                        Some(n) => {
                            out.push(b'\\');
                            out.push(n);
                            self.pos += 2;
                        }
                        None => {
                            self.pos += 1;
                            return Err(LexError { message: "unterminated regexp".into(), pos: err_pos, line });
                        }
                    }
                    continue;
                }
                b'[' if !in_bracket => {
                    in_bracket = true;
                    bracket_start = out.len();
                    out.push(c);
                    self.pos += 1;
                    // `]` logo depois de `[` ou `[^` é literal.
                    if self.peek_byte() == Some(b'^') {
                        out.push(b'^');
                        self.pos += 1;
                    }
                    if self.peek_byte() == Some(b']') {
                        out.push(b']');
                        self.pos += 1;
                    }
                    continue;
                }
                b'[' if in_bracket => {
                    // Classe `[:alpha:]`, `[.x.]`, `[=x=]` dentro do colchete.
                    if let Some(k @ (b':' | b'.' | b'=')) = self.byte_at(self.pos + 1) {
                        let mut j = self.pos + 2;
                        let mut found = None;
                        while let Some(d) = self.byte_at(j) {
                            if d == b'\n' {
                                break;
                            }
                            if d == k && self.byte_at(j + 1) == Some(b']') {
                                found = Some(j + 2);
                                break;
                            }
                            j += 1;
                        }
                        if let Some(end) = found {
                            out.extend_from_slice(&self.src[self.pos..end]);
                            self.pos = end;
                            continue;
                        }
                    }
                    out.push(c);
                    self.pos += 1;
                    continue;
                }
                b']' if in_bracket => {
                    let _ = bracket_start;
                    in_bracket = false;
                    out.push(c);
                    self.pos += 1;
                    continue;
                }
                b'/' if !in_bracket => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        Ok(Rc::from(out))
    }

    /// Reposiciona (pra sondagens com um lexer descartável sobre o mesmo fonte).
    pub fn set_position(&mut self, pos: usize, line: u32) {
        self.pos = pos;
        self.line = line;
    }

    /// Posição atual (pra mensagens).
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn line(&self) -> u32 {
        self.line
    }
}

/// Processa um escape começando em `src[*pos] == b'\\'`, como o gawk faz em strings. Avança `pos`.
/// Devolve os bytes e um aviso opcional. `in_regex` mantém operadores de regex intactos.
pub fn escape_sequence(src: &[u8], pos: &mut usize, in_regex: bool) -> (Vec<u8>, Option<String>) {
    debug_assert_eq!(src.get(*pos), Some(&b'\\'));
    *pos += 1;
    let Some(&e) = src.get(*pos) else {
        return (vec![b'\\'], None);
    };
    *pos += 1;
    let simple = |b: u8| (vec![b], None);
    match e {
        b'a' => simple(7),
        b'b' => simple(8),
        b'f' => simple(12),
        b'n' => simple(b'\n'),
        b'r' => simple(b'\r'),
        b't' => simple(b'\t'),
        b'v' => simple(11),
        b'\\' => simple(b'\\'),
        b'"' => simple(b'"'),
        b'0'..=b'7' => {
            let mut v: u32 = (e - b'0') as u32;
            let mut n = 1;
            while n < 3 {
                match src.get(*pos) {
                    Some(&d @ b'0'..=b'7') => {
                        v = v * 8 + (d - b'0') as u32;
                        *pos += 1;
                        n += 1;
                    }
                    _ => break,
                }
            }
            simple((v & 0xff) as u8)
        }
        b'x' => {
            let mut v: u32 = 0;
            let mut n = 0;
            while n < 2 {
                match src.get(*pos) {
                    Some(d) if d.is_ascii_hexdigit() => {
                        v = v * 16 + (*d as char).to_digit(16).unwrap_or(0);
                        *pos += 1;
                        n += 1;
                    }
                    _ => break,
                }
            }
            if n == 0 {
                return (b"x".to_vec(), Some("no hex digits in `\\x' escape sequence".to_string()));
            }
            simple(v as u8)
        }
        b'/' if !in_regex => (vec![b'/'], Some("escape sequence `\\/' treated as plain `/'".to_string())),
        _ => {
            let len = utf8_len(e);
            let end = (*pos - 1 + len).min(src.len());
            let ch = src[*pos - 1..end].to_vec();
            *pos = end;
            let shown = String::from_utf8_lossy(&ch).into_owned();
            (ch, Some(format!("escape sequence `\\{shown}' treated as plain `{shown}'")))
        }
    }
}

fn utf8_len(b: u8) -> usize {
    match b {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        let mut l = Lexer::new(src.as_bytes());
        let mut out = Vec::new();
        loop {
            let t = l.next_token().unwrap();
            if t.tok == Tok::Eof {
                break;
            }
            out.push(t.tok);
        }
        out
    }

    #[test]
    fn numbers_and_names() {
        assert_eq!(toks("011 0x11 018 1e3 .5"), vec![Tok::Number(9.0), Tok::Number(17.0), Tok::Number(18.0), Tok::Number(1000.0), Tok::Number(0.5), Tok::Newline]);
        assert_eq!(toks("foo(x) bar (y)")[0], Tok::FuncName(Rc::from("foo")));
        assert_eq!(toks("a ** b **= c")[1], Tok::Caret);
    }

    #[test]
    fn strings_with_escapes() {
        assert_eq!(toks(r#""a\tb\101\x41""#)[0], Tok::Str(Rc::from(&b"a\tbAA"[..])));
    }
}
