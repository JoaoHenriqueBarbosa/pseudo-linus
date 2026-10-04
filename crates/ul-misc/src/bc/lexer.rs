//
// Copyright (c) 2024-2026 Hemi Labs, Inc.
//
// This file is part of the posixutils-rs project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT
//
// Modificado no pseudo-linus (2026, MIT): o léxico POSIX do upstream (letras isoladas, palavras-chave
// por casamento máximo dentro de palavras) virou o do GNU bc 1.07.1, medido em caixa preta: nomes
// `[a-z][a-z0-9_]*`, palavras-chave inteiras, strings sem escape, comentário `#`, `&&`, `||`, `!`,
// caractere ilegal com o formato do GNU, e leitura de vários arquivos e do stdin em sequência, com
// o stdin lido em blocos de até 8192 bytes por `read(2)` como o scanner do flex.

//! Análise léxica do bc, com as fontes (arquivos, depois o stdin) encadeadas como no GNU.
//!
//! - Um token nunca atravessa o fim de um arquivo: o fim do arquivo fecha o token corrente e a
//!   leitura continua no próximo, com a contagem de linhas recomeçando em 1.
//! - No fim da última fonte a linha volta a 1 (é o que o GNU mostra num erro de sintaxe no EOF).
//! - Número: dígitos `0-9A-Z`, ponto opcional, `\` + newline no meio é ignorado; o texto é
//!   normalizado como o do GNU (sem zeros à esquerda, sem ponto final solto; vazio vira `0`).
//! - Caractere fora da gramática vira "illegal character" (imprimível como está, controle como
//!   `^X`, acima de 0x7e em octal) e é descartado.

use std::collections::VecDeque;

use sysabi::{Errno, Fd, sys};

/// Símbolos terminais da gramática. A ordem não importa pra nada além de ser estável.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum T {
    Eof,
    Error,
    EndOfLine,
    And,
    Or,
    Not,
    Str,
    Name,
    Number,
    AssignOp,
    RelOp,
    IncrDecr,
    Define,
    Break,
    Quit,
    Length,
    Return,
    For,
    If,
    While,
    Sqrt,
    Else,
    Scale,
    Ibase,
    Obase,
    Auto,
    Read,
    Random,
    Warranty,
    Halt,
    Last,
    Continue,
    Print,
    Limits,
    History,
    Void,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Semicolon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Amp,
    /// Só pra `%prec` do menos unário; o léxico nunca devolve.
    UnaryMinus,
}

/// Todos os terminais, na ordem do enum.
pub const TERMINALS: &[T] = &[
    T::Eof,
    T::Error,
    T::EndOfLine,
    T::And,
    T::Or,
    T::Not,
    T::Str,
    T::Name,
    T::Number,
    T::AssignOp,
    T::RelOp,
    T::IncrDecr,
    T::Define,
    T::Break,
    T::Quit,
    T::Length,
    T::Return,
    T::For,
    T::If,
    T::While,
    T::Sqrt,
    T::Else,
    T::Scale,
    T::Ibase,
    T::Obase,
    T::Auto,
    T::Read,
    T::Random,
    T::Warranty,
    T::Halt,
    T::Last,
    T::Continue,
    T::Print,
    T::Limits,
    T::History,
    T::Void,
    T::Plus,
    T::Minus,
    T::Star,
    T::Slash,
    T::Percent,
    T::Caret,
    T::Semicolon,
    T::LParen,
    T::RParen,
    T::LBracket,
    T::RBracket,
    T::LBrace,
    T::RBrace,
    T::Comma,
    T::Amp,
    T::UnaryMinus,
];

const KEYWORDS: &[(&[u8], T)] = &[
    (b"auto", T::Auto),
    (b"break", T::Break),
    (b"continue", T::Continue),
    (b"define", T::Define),
    (b"else", T::Else),
    (b"for", T::For),
    (b"halt", T::Halt),
    (b"history", T::History),
    (b"ibase", T::Ibase),
    (b"if", T::If),
    (b"last", T::Last),
    (b"length", T::Length),
    (b"limits", T::Limits),
    (b"obase", T::Obase),
    (b"print", T::Print),
    (b"quit", T::Quit),
    (b"random", T::Random),
    (b"read", T::Read),
    (b"return", T::Return),
    (b"scale", T::Scale),
    (b"sqrt", T::Sqrt),
    (b"void", T::Void),
    (b"warranty", T::Warranty),
    (b"while", T::While),
];

/// Um token com o seu texto: o número normalizado, o conteúdo da string, o nome, ou o operador
/// (`=`, `+`... pro ASSIGN_OP; `==`, `<=`... pro REL_OP; `++`/`--`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: T,
    pub text: Vec<u8>,
}

impl Token {
    fn new(kind: T) -> Token {
        Token { kind, text: Vec::new() }
    }

    fn with(kind: T, text: &[u8]) -> Token {
        Token { kind, text: text.to_vec() }
    }
}

/// Uma mensagem do léxico, na ordem em que o GNU a escreveria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LexDiag {
    /// `illegal character: X` (com o prefixo de arquivo e linha).
    Illegal(String),
    /// "Non-standard base in numeric constant" (aviso no `-w`, erro no `-s`).
    NonStandardBase,
    /// `EOF encountered in a comment.` (sem prefixo; encerra a entrada).
    CommentEof,
    /// `File x is unavailable.`: fatal, sai com 1.
    Unavailable(Vec<u8>),
    /// `read() in flex scanner failed`: fatal, sai com 1.
    ReadFailed,
}

/// De onde vem o texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceName {
    File(Vec<u8>),
    Stdin,
}

/// Leitura do stdin compartilhada entre o scanner (blocos de `read(2)`) e o `read()` do bc (stdio
/// com buffer próprio de 4096 bytes): o que um buffer pegou o outro não vê, como no GNU.
pub struct StdinShare {
    /// Buffer do `getchar` usado pelo `read()`.
    stdio: VecDeque<u8>,
    stdio_eof: bool,
}

impl Default for StdinShare {
    fn default() -> Self {
        Self::new()
    }
}

impl StdinShare {
    pub fn new() -> StdinShare {
        StdinShare { stdio: VecDeque::new(), stdio_eof: false }
    }

    /// `getchar()`: devolve `None` no fim (o GNU fica em laço; quem chama decide).
    pub fn getchar(&mut self) -> Option<u8> {
        if self.stdio.is_empty() && !self.stdio_eof {
            let mut buf = [0u8; 4096];
            loop {
                match sys::read(Fd::STDIN, &mut buf) {
                    Ok(0) => {
                        self.stdio_eof = true;
                        break;
                    }
                    Ok(n) => {
                        self.stdio.extend(&buf[..n]);
                        break;
                    }
                    Err(Errno::EINTR) => {}
                    Err(_) => {
                        self.stdio_eof = true;
                        break;
                    }
                }
            }
        }
        self.stdio.pop_front()
    }
}

/// Fonte aberta.
struct Open {
    data: Vec<u8>,
    pos: usize,
    /// Pra stdin: ainda pode chegar mais (lido sob demanda).
    more: bool,
    /// Modo interativo: lê linha a linha e ecoa (o readline com entrada que não é terminal).
    interactive: bool,
}

/// O analisador léxico.
pub struct Lexer {
    pending: VecDeque<SourceName>,
    cur: Option<Open>,
    pub name: SourceName,
    pub line: u32,
    pub std_only: bool,
    /// O `-i`: stdin linha a linha, com eco.
    pub interactive: bool,
    /// A entrada acabou de vez (EOF do último arquivo ou comentário sem fim).
    finished: bool,
    /// Linhas ecoadas no modo interativo, pra quem escreve o stdout.
    pub echo: Vec<u8>,
}

/// Formata um byte ilegal como o GNU.
pub fn illegal_repr(c: u8) -> String {
    match c {
        0x20..=0x7e => (c as char).to_string(),
        0..=0x1f => format!("^{}", (c + 64) as char),
        _ => format!("\\{c:03o}"),
    }
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase()
}

impl Lexer {
    /// `files` na ordem; o stdin vem depois de todos.
    pub fn new(files: Vec<Vec<u8>>, std_only: bool, interactive: bool) -> Lexer {
        let mut pending: VecDeque<SourceName> = files.into_iter().map(SourceName::File).collect();
        pending.push_back(SourceName::Stdin);
        Lexer {
            pending,
            cur: None,
            name: SourceName::Stdin,
            line: 1,
            std_only,
            interactive,
            finished: false,
            echo: Vec::new(),
        }
    }

    /// Nome pra prefixo de mensagem.
    pub fn display_name(&self) -> String {
        match &self.name {
            SourceName::File(f) => String::from_utf8_lossy(f).into_owned(),
            SourceName::Stdin => "(standard_in)".to_string(),
        }
    }

    /// Prefixo `arquivo linha: ` das mensagens do compilador.
    pub fn prefix(&self) -> String {
        format!("{} {}: ", self.display_name(), self.line)
    }

    /// Abre a próxima fonte. `Err` com o diagnóstico fatal quando o arquivo não abre.
    fn open_next(&mut self) -> Result<bool, LexDiag> {
        let Some(next) = self.pending.pop_front() else { return Ok(false) };
        self.line = 1;
        match &next {
            SourceName::File(path) => {
                self.name = next.clone();
                let fd = match sys::open(path, sysabi::OFlags::RDONLY | sysabi::OFlags::CLOEXEC, 0) {
                    Ok(fd) => fd,
                    Err(_) => return Err(LexDiag::Unavailable(path.clone())),
                };
                let mut data = Vec::new();
                let mut buf = vec![0u8; 65536];
                loop {
                    match sys::read(fd, &mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if data.try_reserve(n).is_err() {
                                let _ = sys::close(fd);
                                return Err(LexDiag::ReadFailed);
                            }
                            data.extend_from_slice(&buf[..n]);
                        }
                        Err(Errno::EINTR) => {}
                        Err(_) => {
                            let _ = sys::close(fd);
                            return Err(LexDiag::ReadFailed);
                        }
                    }
                }
                let _ = sys::close(fd);
                self.cur = Some(Open { data, pos: 0, more: false, interactive: false });
            }
            SourceName::Stdin => {
                self.name = SourceName::Stdin;
                self.cur = Some(Open { data: Vec::new(), pos: 0, more: true, interactive: self.interactive });
            }
        }
        Ok(true)
    }

    /// Lê mais do stdin pro buffer do scanner: um `read(2)` de até 8192 bytes (ou uma linha no modo
    /// interativo). Devolve `false` no fim.
    fn refill(&mut self) -> Result<bool, LexDiag> {
        let Some(open) = self.cur.as_mut() else { return Ok(false) };
        if !open.more {
            return Ok(false);
        }
        // Descarta o que já foi consumido pra o buffer não crescer sem limite.
        if open.pos > 0 && open.pos == open.data.len() {
            open.data.clear();
            open.pos = 0;
        }
        if open.interactive {
            // readline: lê byte a byte até o newline e ecoa a linha.
            let mut line = Vec::new();
            let mut got_any = false;
            loop {
                let mut b = [0u8; 1];
                match sys::read(Fd::STDIN, &mut b) {
                    Ok(0) => break,
                    Ok(_) => {
                        got_any = true;
                        if b[0] == b'\n' {
                            break;
                        }
                        line.push(b[0]);
                    }
                    Err(Errno::EINTR) => {}
                    Err(_) => return Err(LexDiag::ReadFailed),
                }
            }
            if !got_any {
                open.more = false;
                return Ok(false);
            }
            self.echo.extend_from_slice(&line);
            self.echo.push(b'\n');
            open.data.extend_from_slice(&line);
            open.data.push(b'\n');
            return Ok(true);
        }
        let mut buf = vec![0u8; 8192];
        loop {
            match sys::read(Fd::STDIN, &mut buf) {
                Ok(0) => {
                    open.more = false;
                    return Ok(false);
                }
                Ok(n) => {
                    open.data.extend_from_slice(&buf[..n]);
                    return Ok(true);
                }
                Err(Errno::EINTR) => {}
                Err(_) => return Err(LexDiag::ReadFailed),
            }
        }
    }

    /// O byte `k` posições à frente na fonte corrente (lendo mais do stdin se preciso).
    fn peek(&mut self, k: usize) -> Result<Option<u8>, LexDiag> {
        loop {
            let Some(open) = self.cur.as_ref() else { return Ok(None) };
            if open.pos + k < open.data.len() {
                return Ok(Some(open.data[open.pos + k]));
            }
            if !self.refill()? {
                return Ok(None);
            }
        }
    }

    fn bump(&mut self, n: usize) {
        if let Some(open) = self.cur.as_mut() {
            open.pos += n;
        }
    }

    /// O próximo token, com os diagnósticos que o léxico emitiu no caminho (na ordem). Um
    /// diagnóstico fatal (`Unavailable`, `ReadFailed`) vem sozinho em `Err`.
    pub fn next(&mut self, diags: &mut Vec<(String, LexDiag)>) -> Result<Token, LexDiag> {
        loop {
            if self.finished {
                return Ok(Token::new(T::Eof));
            }
            if self.cur.is_none() {
                if !self.open_next()? {
                    // Fim de tudo: o GNU mostra linha 1 nos erros a partir daqui.
                    self.finished = true;
                    self.line = 1;
                    return Ok(Token::new(T::Eof));
                }
                continue;
            }
            let Some(c) = self.peek(0)? else {
                self.cur = None;
                continue;
            };
            sys::checkpoint();
            match c {
                b' ' | b'\t' => self.bump(1),
                b'\\' => {
                    if self.peek(1)? == Some(b'\n') {
                        self.bump(2);
                        self.line += 1;
                    } else {
                        self.bump(1);
                        diags.push((self.prefix(), LexDiag::Illegal(illegal_repr(c))));
                    }
                }
                b'\n' => {
                    self.bump(1);
                    self.line += 1;
                    return Ok(Token::new(T::EndOfLine));
                }
                b'#' if !self.std_only => {
                    // Até o fim da linha, sem consumir o newline.
                    let mut k = 1;
                    while let Some(d) = self.peek(k)? {
                        if d == b'\n' {
                            break;
                        }
                        k += 1;
                    }
                    self.bump(k);
                }
                b'/' if self.peek(1)? == Some(b'*') => {
                    let mut k = 2;
                    let mut lines = 0;
                    loop {
                        match self.peek(k)? {
                            None => {
                                diags.push((String::new(), LexDiag::CommentEof));
                                self.finished = true;
                                self.cur = None;
                                self.pending.clear();
                                return Ok(Token::new(T::Eof));
                            }
                            Some(b'*') if self.peek(k + 1)? == Some(b'/') => {
                                k += 2;
                                break;
                            }
                            Some(b'\n') => {
                                lines += 1;
                                k += 1;
                            }
                            Some(_) => k += 1,
                        }
                    }
                    self.bump(k);
                    self.line += lines;
                }
                b'"' => {
                    let mut k = 1;
                    let mut closed = false;
                    while let Some(d) = self.peek(k)? {
                        if d == b'"' {
                            closed = true;
                            break;
                        }
                        k += 1;
                    }
                    if !closed {
                        self.bump(1);
                        diags.push((self.prefix(), LexDiag::Illegal("\"".to_string())));
                        continue;
                    }
                    let open = self.cur.as_ref().expect("fonte aberta");
                    let text = open.data[open.pos + 1..open.pos + k].to_vec();
                    self.line += text.iter().filter(|&&b| b == b'\n').count() as u32;
                    self.bump(k + 1);
                    return Ok(Token { kind: T::Str, text });
                }
                b'a'..=b'z' => {
                    let mut k = 1;
                    while let Some(d) = self.peek(k)? {
                        if d.is_ascii_lowercase() || d.is_ascii_digit() || d == b'_' {
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    let open = self.cur.as_ref().expect("fonte aberta");
                    let word = open.data[open.pos..open.pos + k].to_vec();
                    self.bump(k);
                    if let Some((_, kw)) = KEYWORDS.iter().find(|(w, _)| *w == word.as_slice()) {
                        return Ok(Token::with(*kw, &word));
                    }
                    return Ok(Token { kind: T::Name, text: word });
                }
                b'.' if !self.number_follows_dot()? => {
                    self.bump(1);
                    return Ok(Token::with(T::Last, b"."));
                }
                c if is_digit(c) || c == b'.' => return self.number(diags),
                _ => {
                    let two = [c, self.peek(1)?.unwrap_or(0)];
                    let tok = match &two {
                        b"+=" | b"-=" | b"*=" | b"/=" | b"%=" | b"^=" => Some((Token::with(T::AssignOp, &two[..1]), 2)),
                        b"==" | b"<=" | b">=" | b"!=" => Some((Token::with(T::RelOp, &two), 2)),
                        b"++" | b"--" => Some((Token::with(T::IncrDecr, &two), 2)),
                        b"&&" => Some((Token::new(T::And), 2)),
                        b"||" => Some((Token::new(T::Or), 2)),
                        _ => None,
                    };
                    if let Some((t, n)) = tok {
                        self.bump(n);
                        return Ok(t);
                    }
                    let single = match c {
                        b'=' => Some(Token::with(T::AssignOp, b"=")),
                        b'<' | b'>' => Some(Token::with(T::RelOp, &[c])),
                        b'!' => Some(Token::new(T::Not)),
                        b'+' => Some(Token::new(T::Plus)),
                        b'-' => Some(Token::new(T::Minus)),
                        b'*' => Some(Token::new(T::Star)),
                        b'/' => Some(Token::new(T::Slash)),
                        b'%' => Some(Token::new(T::Percent)),
                        b'^' => Some(Token::new(T::Caret)),
                        b';' => Some(Token::new(T::Semicolon)),
                        b'(' => Some(Token::new(T::LParen)),
                        b')' => Some(Token::new(T::RParen)),
                        b'[' => Some(Token::new(T::LBracket)),
                        b']' => Some(Token::new(T::RBracket)),
                        b'{' => Some(Token::new(T::LBrace)),
                        b'}' => Some(Token::new(T::RBrace)),
                        b',' => Some(Token::new(T::Comma)),
                        b'&' => Some(Token::new(T::Amp)),
                        _ => None,
                    };
                    self.bump(1);
                    match single {
                        Some(t) => return Ok(t),
                        None => diags.push((self.prefix(), LexDiag::Illegal(illegal_repr(c)))),
                    }
                }
            }
        }
    }

    /// Depois de um `.`: vem dígito (é número) ou não (é o `last`)?
    fn number_follows_dot(&mut self) -> Result<bool, LexDiag> {
        let mut k = 1;
        loop {
            match self.peek(k)? {
                Some(b'\\') if self.peek(k + 1)? == Some(b'\n') => k += 2,
                Some(d) => return Ok(is_digit(d)),
                None => return Ok(false),
            }
        }
    }

    fn number(&mut self, diags: &mut Vec<(String, LexDiag)>) -> Result<Token, LexDiag> {
        let mut raw = Vec::new();
        let mut k = 0;
        let mut lines = 0;
        let mut seen_dot = false;
        loop {
            match self.peek(k)? {
                Some(d) if is_digit(d) => {
                    raw.push(d);
                    k += 1;
                }
                Some(b'.') if !seen_dot => {
                    seen_dot = true;
                    raw.push(b'.');
                    k += 1;
                }
                Some(b'\\') if self.peek(k + 1)? == Some(b'\n') => {
                    lines += 1;
                    k += 2;
                }
                _ => break,
            }
        }
        self.bump(k);
        let warn_line = self.prefix();
        self.line += lines;
        if raw.iter().any(|&c| (b'G'..=b'Z').contains(&c)) {
            diags.push((warn_line, LexDiag::NonStandardBase));
        }
        Ok(Token { kind: T::Number, text: normalize_number(&raw) })
    }
}

/// Normaliza o texto de um número como o léxico do GNU: tira os zeros à esquerda da parte inteira e
/// o ponto final sem fração; o que sobrar vazio vira `0`.
pub fn normalize_number(raw: &[u8]) -> Vec<u8> {
    let mut start = 0;
    while start < raw.len() && raw[start] == b'0' {
        start += 1;
    }
    let mut s = raw[start..].to_vec();
    if s.last() == Some(&b'.') {
        s.pop();
    }
    if s.is_empty() {
        s.push(b'0');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_normalization() {
        assert_eq!(normalize_number(b"000.000"), b".000");
        assert_eq!(normalize_number(b"0."), b"0");
        assert_eq!(normalize_number(b"00"), b"0");
        assert_eq!(normalize_number(b"0010"), b"10");
        assert_eq!(normalize_number(b"1.0"), b"1.0");
        assert_eq!(normalize_number(b"0A"), b"A");
        assert_eq!(normalize_number(b"00.50"), b".50");
        assert_eq!(normalize_number(b"10."), b"10");
    }

    #[test]
    fn illegal_characters() {
        assert_eq!(illegal_repr(b'\r'), "^M");
        assert_eq!(illegal_repr(0x1b), "^[");
        assert_eq!(illegal_repr(0x7f), "\\177");
        assert_eq!(illegal_repr(0xc3), "\\303");
        assert_eq!(illegal_repr(b'@'), "@");
    }
}
