//! Parser de palavras: o texto cru de uma palavra (como o tokenizer entregou, com aspas e escapes)
//! vira uma sequência de [`Part`]. É o "segundo nível" do parse: `${...}` em todas as formas,
//! `$(...)` e crase (cujo corpo é parseado de novo como programa), `$((...))`, `$'...'`, aspas,
//! til e expansão de chaves.

use std::sync::Arc;

use crate::ast::*;
use crate::parse::{SyntaxError, parse_nested};

/// Contexto em que o texto é lido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Palavra de comando, fora de aspas.
    Normal,
    /// Corpo de here-doc que expande: como aspas duplas, mas `"` é literal.
    HereDoc,
    /// Texto aritmético (`$(( ))`, `(( ))`, offsets): como aspas duplas, e as `"` somem.
    Arith,
    /// Subscript de array (`a[...]`): aspas são removidas, nada de split, glob, til ou chaves.
    Subscript,
    /// Palavra de `${x:-w}` (e da substituição de `${x/p/r}`) dentro de aspas duplas.
    ParamWordInDouble,
    /// Padrão de `${x#p}`, `${x/p/}`, `${x^p}` dentro de aspas duplas: aspas valem como aspas e o
    /// resto continua ativo como padrão.
    PatternInDouble,
}

#[derive(Clone, Copy, Debug)]
pub struct WordOpts {
    pub mode: Mode,
    /// Reconhecer expansão de chaves (só argumentos de comando, listas do `for`, elementos de array).
    pub brace: bool,
    /// Til no início da palavra.
    pub tilde: bool,
    /// Valor de atribuição: til também depois de `:`.
    pub assignment: bool,
    /// Linha onde o texto começa (pra `LINENO` dentro de `$(...)`).
    pub line: Line,
}

impl WordOpts {
    pub fn normal(line: Line) -> WordOpts {
        WordOpts { mode: Mode::Normal, brace: true, tilde: true, assignment: false, line }
    }

    pub fn plain(line: Line) -> WordOpts {
        WordOpts { mode: Mode::Normal, brace: false, tilde: true, assignment: false, line }
    }

    pub fn assignment(line: Line) -> WordOpts {
        WordOpts { mode: Mode::Normal, brace: false, tilde: true, assignment: true, line }
    }

    pub fn mode(mode: Mode, line: Line) -> WordOpts {
        WordOpts { mode, brace: false, tilde: false, assignment: false, line }
    }
}

/// Parseia uma palavra inteira.
pub fn parse_word(text: &str, opts: WordOpts) -> Result<Vec<Part>, SyntaxError> {
    let mut p = WordParser { s: text.as_bytes(), src: text, i: 0, line: opts.line, opts, depth: 0 };
    let parts = p.parse_top()?;
    Ok(parts)
}

/// Monta um [`Word`] a partir do texto cru.
pub fn make_word(text: &str, opts: WordOpts) -> Result<Word, SyntaxError> {
    let parts = parse_word(text, opts)?;
    Ok(Word { raw: Arc::from(text), parts: Arc::from(parts), assign: None })
}

/// Palavra literal (sem expansão), pra palavras sintéticas.
pub fn literal_word(text: &str) -> Word {
    Word { raw: Arc::from(text), parts: Arc::from(vec![Part::Quoted(text.as_bytes().to_vec())]), assign: None }
}

/// Texto aritmético como [`ArithExp`].
pub fn parse_arith(text: &str, line: Line) -> Result<Arc<ArithExp>, SyntaxError> {
    let parts = parse_word(text, WordOpts::mode(Mode::Arith, line))?;
    Ok(Arc::new(ArithExp { parts, raw: Arc::from(text) }))
}

/// Limite de aninhamento de construções dentro de uma palavra.
const MAX_DEPTH: u32 = 200;

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// É um nome de variável válido?
pub fn is_name(s: &[u8]) -> bool {
    !s.is_empty() && is_name_start(s[0]) && s.iter().all(|c| is_name_char(*c))
}

fn utf8_len(c: u8) -> usize {
    match c {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

fn push_lit(parts: &mut Vec<Part>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if let Some(Part::Lit(v)) = parts.last_mut() {
        v.extend_from_slice(bytes);
    } else {
        parts.push(Part::Lit(bytes.to_vec()));
    }
}

fn push_quoted(parts: &mut Vec<Part>, bytes: &[u8]) {
    if let Some(Part::Quoted(v)) = parts.last_mut() {
        v.extend_from_slice(bytes);
    } else {
        parts.push(Part::Quoted(bytes.to_vec()));
    }
}

struct WordParser<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
    line: Line,
    opts: WordOpts,
    depth: u32,
}

impl WordParser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn at(&self, off: usize) -> Option<u8> {
        self.s.get(self.i + off).copied()
    }

    fn count_lines(&mut self, from: usize, to: usize) {
        self.line += self.s[from..to].iter().filter(|c| **c == b'\n').count() as Line;
    }

    fn err(&self, what: &str) -> SyntaxError {
        SyntaxError::new(self.line, format!("unexpected EOF while looking for matching `{what}'"))
    }

    fn enter(&mut self) -> Result<(), SyntaxError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(SyntaxError::new(self.line, "expression nesting too deep".to_string()));
        }
        Ok(())
    }

    fn parse_top(&mut self) -> Result<Vec<Part>, SyntaxError> {
        match self.opts.mode {
            Mode::Normal | Mode::Subscript | Mode::PatternInDouble => self.parse_normal(false, self.opts.tilde, false),
            Mode::HereDoc | Mode::Arith | Mode::ParamWordInDouble => self.parse_double_like(None),
        }
    }

    /// Laço do modo normal. `in_brace` faz `,` e `}` terminarem (alternativa de chaves).
    fn parse_normal(&mut self, in_brace: bool, mut tilde_ok: bool, _alt_start: bool) -> Result<Vec<Part>, SyntaxError> {
        let mut parts: Vec<Part> = Vec::new();
        let mode = self.opts.mode;
        let patternish = mode == Mode::PatternInDouble;
        while let Some(c) = self.peek() {
            if in_brace && (c == b',' || c == b'}') {
                break;
            }
            if tilde_ok && c == b'~' && mode == Mode::Normal {
                if let Some(t) = self.try_tilde() {
                    parts.push(t);
                    tilde_ok = false;
                    continue;
                }
            }
            tilde_ok = false;
            match c {
                b'\\' => {
                    if self.i + 1 < self.s.len() {
                        let n = utf8_len(self.s[self.i + 1]).min(self.s.len() - self.i - 1);
                        let ch = &self.s[self.i + 1..self.i + 1 + n];
                        if ch == b"\n" {
                            self.line += 1;
                        } else {
                            push_quoted(&mut parts, ch);
                        }
                        self.i += 1 + n;
                    } else {
                        push_lit(&mut parts, b"\\");
                        self.i += 1;
                    }
                }
                b'\'' => {
                    let start = self.i + 1;
                    let end = self.s[start..].iter().position(|b| *b == b'\'').map(|p| start + p);
                    match end {
                        Some(e) => {
                            self.count_lines(start, e);
                            push_quoted(&mut parts, &self.s[start..e]);
                            if e == start {
                                // `''` sozinho ainda é um argumento vazio.
                                push_quoted(&mut parts, b"");
                            }
                            self.i = e + 1;
                        }
                        None => return Err(self.err("'")),
                    }
                }
                b'"' => {
                    self.i += 1;
                    let inner = self.parse_double_like(Some(b'"'))?;
                    parts.push(Part::Double(inner));
                }
                b'$' => {
                    let part = self.parse_dollar(false)?;
                    match part {
                        DollarResult::Part(p) => parts.push(p),
                        DollarResult::Text(t) => {
                            if patternish {
                                push_lit(&mut parts, &t);
                            } else {
                                push_lit(&mut parts, &t);
                            }
                        }
                    }
                }
                b'`' => {
                    let p = self.parse_backquote(false)?;
                    parts.push(p);
                }
                b'{' if self.opts.brace && mode == Mode::Normal => {
                    if let Some(p) = self.try_brace()? {
                        parts.push(p);
                    } else {
                        push_lit(&mut parts, b"{");
                        self.i += 1;
                    }
                }
                b':' if self.opts.assignment => {
                    push_lit(&mut parts, b":");
                    self.i += 1;
                    tilde_ok = true;
                }
                b'\n' => {
                    self.line += 1;
                    push_lit(&mut parts, b"\n");
                    self.i += 1;
                }
                _ => {
                    // Corrida de bytes sem significado especial.
                    let start = self.i;
                    while let Some(d) = self.peek() {
                        if matches!(d, b'\\' | b'\'' | b'"' | b'$' | b'`' | b'\n')
                            || (d == b'{' && self.opts.brace)
                            || (d == b':' && self.opts.assignment)
                            || (in_brace && (d == b',' || d == b'}'))
                        {
                            break;
                        }
                        self.i += 1;
                    }
                    if self.i == start {
                        self.i += 1;
                    }
                    push_lit(&mut parts, &self.s[start..self.i]);
                }
            }
        }
        Ok(parts)
    }

    /// Til no início: `~`, `~nome`, `~+`, `~-`, `~+2`...; só se o prefixo for todo sem aspas.
    fn try_tilde(&mut self) -> Option<Part> {
        let start = self.i + 1;
        let mut j = start;
        while j < self.s.len() {
            let c = self.s[j];
            if c == b'/' || (self.opts.assignment && c == b':') {
                break;
            }
            if matches!(c, b'\\' | b'\'' | b'"' | b'$' | b'`' | b'{' | b'}' | b'*' | b'?' | b'[' | b'\n') {
                return None;
            }
            j += 1;
        }
        let prefix = self.s[start..j].to_vec();
        self.i = j;
        Some(Part::Tilde(prefix))
    }

    /// Laço de aspas duplas e afins. Com `close = Some(b'"')` termina no `"`; senão vai até o fim.
    fn parse_double_like(&mut self, close: Option<u8>) -> Result<Vec<Part>, SyntaxError> {
        self.enter()?;
        let mode = if close.is_some() { Mode::Normal } else { self.opts.mode };
        let mut parts: Vec<Part> = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                if close.is_some() {
                    return Err(self.err("\""));
                }
                break;
            };
            match c {
                b'"' if close.is_some() => {
                    self.i += 1;
                    break;
                }
                b'"' if mode == Mode::Arith => {
                    // Remoção de aspas no texto aritmético.
                    self.i += 1;
                }
                b'"' if mode == Mode::ParamWordInDouble => {
                    self.i += 1;
                    let inner = self.parse_double_like(Some(b'"'))?;
                    parts.extend(inner);
                }
                b'\\' => {
                    let next = self.at(1);
                    let special = match next {
                        Some(b'$') | Some(b'`') | Some(b'\\') => true,
                        Some(b'"') => mode != Mode::HereDoc,
                        Some(b'}') => mode == Mode::ParamWordInDouble,
                        _ => false,
                    };
                    if next == Some(b'\n') && mode != Mode::HereDoc {
                        self.i += 2;
                        self.line += 1;
                    } else if special {
                        push_quoted(&mut parts, &[next.unwrap_or(b'\\')]);
                        self.i += 2;
                    } else {
                        push_quoted(&mut parts, b"\\");
                        self.i += 1;
                    }
                }
                b'$' => match self.parse_dollar(true)? {
                    DollarResult::Part(p) => parts.push(p),
                    DollarResult::Text(t) => push_quoted(&mut parts, &t),
                },
                b'`' => {
                    let p = self.parse_backquote(close.is_some() || mode != Mode::HereDoc)?;
                    parts.push(p);
                }
                b'\n' => {
                    self.line += 1;
                    push_quoted(&mut parts, b"\n");
                    self.i += 1;
                }
                _ => {
                    let start = self.i;
                    while let Some(d) = self.peek() {
                        if matches!(d, b'"' | b'\\' | b'$' | b'`' | b'\n') {
                            break;
                        }
                        self.i += 1;
                    }
                    push_quoted(&mut parts, &self.s[start..self.i]);
                }
            }
        }
        self.depth -= 1;
        if close.is_some() && parts.is_empty() {
            parts.push(Part::Quoted(Vec::new()));
        }
        Ok(parts)
    }

    /// `$...` a partir de `self.i` (que aponta pro `$`).
    fn parse_dollar(&mut self, in_double: bool) -> Result<DollarResult, SyntaxError> {
        let next = self.at(1);
        let quotes_special = !in_double && matches!(self.opts.mode, Mode::Normal | Mode::Subscript | Mode::PatternInDouble);
        match next {
            Some(b'(') => {
                if self.at(2) == Some(b'(') {
                    if let Some(p) = self.try_arith()? {
                        return Ok(DollarResult::Part(p));
                    }
                }
                self.parse_comsub().map(DollarResult::Part)
            }
            Some(b'[') => {
                // `$[expr]`, forma antiga da aritmética.
                let start = self.i + 2;
                let mut depth = 1;
                let mut j = start;
                while j < self.s.len() {
                    match self.s[j] {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if j >= self.s.len() {
                    return Err(self.err("]"));
                }
                let text = &self.src[start..j];
                let line = self.line;
                self.count_lines(start, j);
                self.i = j + 1;
                Ok(DollarResult::Part(Part::Arith(parse_arith(text, line)?)))
            }
            Some(b'{') => self.parse_braced_param(in_double).map(DollarResult::Part),
            Some(b'\'') if quotes_special => {
                let start = self.i + 2;
                let mut j = start;
                while j < self.s.len() {
                    match self.s[j] {
                        b'\\' => j += 2,
                        b'\'' => break,
                        _ => j += 1,
                    }
                }
                if j >= self.s.len() {
                    return Err(self.err("'"));
                }
                let body = self.s[start..j].to_vec();
                self.count_lines(start, j);
                self.i = j + 1;
                Ok(DollarResult::Part(Part::AnsiC(body)))
            }
            Some(b'"') if quotes_special => {
                self.i += 2;
                let inner = self.parse_double_like(Some(b'"'))?;
                Ok(DollarResult::Part(Part::Double(inner)))
            }
            Some(c) if is_name_start(c) => {
                let start = self.i + 1;
                let mut j = start;
                while j < self.s.len() && is_name_char(self.s[j]) {
                    j += 1;
                }
                let name = self.src[start..j].to_string();
                self.i = j;
                Ok(DollarResult::Part(Part::Param(Box::new(ParamExp {
                    raw: Arc::from(format!("${name}")),
                    name: ParamName::Var(name),
                    index: None,
                    indirect: false,
                    op: ParamOp::None,
                    braced: false,
                }))))
            }
            Some(c) if c.is_ascii_digit() => {
                self.i += 2;
                Ok(DollarResult::Part(Part::Param(Box::new(ParamExp {
                    raw: Arc::from(format!("${}", c as char)),
                    name: ParamName::Positional((c - b'0') as u32),
                    index: None,
                    indirect: false,
                    op: ParamOp::None,
                    braced: false,
                }))))
            }
            Some(c) if matches!(c, b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!') => {
                self.i += 2;
                Ok(DollarResult::Part(Part::Param(Box::new(ParamExp {
                    raw: Arc::from(format!("${}", c as char)),
                    name: ParamName::Special(c),
                    index: None,
                    indirect: false,
                    op: ParamOp::None,
                    braced: false,
                }))))
            }
            _ => {
                self.i += 1;
                Ok(DollarResult::Text(b"$".to_vec()))
            }
        }
    }

    /// `$((...))`. Devolve `None` se não fecha com `))` (aí é `$( (...) )`).
    fn try_arith(&mut self) -> Result<Option<Part>, SyntaxError> {
        let start = self.i + 3;
        let mut depth: i32 = 2;
        let mut j = start;
        let mut last_close_2_to_1: Option<usize> = None;
        while j < self.s.len() {
            match self.s[j] {
                b'\\' => {
                    j += 2;
                    continue;
                }
                b'\'' => {
                    // Aspas simples são literais na aritmética, mas pulamos pra não contar parênteses.
                    match self.s[j + 1..].iter().position(|b| *b == b'\'') {
                        Some(p) => j += p + 2,
                        None => j = self.s.len(),
                    }
                    continue;
                }
                b'"' => {
                    let mut k = j + 1;
                    while k < self.s.len() && self.s[k] != b'"' {
                        if self.s[k] == b'\\' {
                            k += 1;
                        }
                        k += 1;
                    }
                    j = k + 1;
                    continue;
                }
                b'$' if self.s.get(j + 1) == Some(&b'(') && self.s.get(j + 2) != Some(&b'(') => {
                    let rest = &self.src[j + 2..];
                    match brush_parser::scan_command_substitution(rest) {
                        Ok(n) => {
                            j += 2 + n;
                            continue;
                        }
                        Err(_) => return Err(self.err(")")),
                    }
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 1 {
                        last_close_2_to_1 = Some(j);
                    }
                    if depth == 0 {
                        if last_close_2_to_1 == Some(j - 1) {
                            let text = &self.src[start..j - 1];
                            let line = self.line;
                            self.count_lines(self.i, j);
                            self.i = j + 1;
                            return Ok(Some(Part::Arith(parse_arith(text, line)?)));
                        }
                        return Ok(None);
                    }
                }
                _ => {}
            }
            j += 1;
        }
        Ok(None)
    }

    /// `$(...)`.
    fn parse_comsub(&mut self) -> Result<Part, SyntaxError> {
        self.enter()?;
        let body_start = self.i + 2;
        let rest = &self.src[body_start..];
        let n = brush_parser::scan_command_substitution(rest).map_err(|_| self.err(")"))?;
        if n == 0 || body_start + n > self.s.len() || self.s[body_start + n - 1] != b')' {
            return Err(self.err(")"));
        }
        let body = &self.src[body_start..body_start + n - 1];
        let line = self.line;
        let close_line = line + body.bytes().filter(|b| *b == b'\n').count() as Line;
        let program = parse_nested(body, line).map_err(|mut e| {
            // O bash parseia o corpo junto com o `)`: faltando texto, o token inesperado é o `)`.
            if e.message == "syntax error: unexpected end of file" {
                e.message = "syntax error near unexpected token `)'".to_string();
                e.line = close_line;
            }
            e.context = None;
            e
        })?;
        self.count_lines(self.i, body_start + n);
        self.i = body_start + n;
        self.depth -= 1;
        Ok(Part::CmdSub(Arc::new(CmdSub { src: Arc::from(body), program, backquote: false })))
    }

    /// Crase. `in_double` liga o desescape de `\"`.
    fn parse_backquote(&mut self, in_double: bool) -> Result<Part, SyntaxError> {
        self.enter()?;
        let start = self.i + 1;
        let mut j = start;
        let mut body = String::new();
        loop {
            if j >= self.s.len() {
                return Err(self.err("`"));
            }
            let c = self.s[j];
            if c == b'\\' && j + 1 < self.s.len() {
                let n = self.s[j + 1];
                if n == b'$' || n == b'`' || n == b'\\' || (in_double && n == b'"') {
                    body.push(n as char);
                    j += 2;
                    continue;
                }
                body.push('\\');
                j += 1;
                continue;
            }
            if c == b'`' {
                break;
            }
            let len = utf8_len(c).min(self.s.len() - j);
            body.push_str(&self.src[j..j + len]);
            j += len;
        }
        let line = self.line;
        let program = parse_nested(&body, line)?;
        self.count_lines(self.i, j);
        self.i = j + 1;
        self.depth -= 1;
        Ok(Part::CmdSub(Arc::new(CmdSub { src: Arc::from(body.as_str()), program, backquote: true })))
    }

    /// `${...}`.
    fn parse_braced_param(&mut self, in_double: bool) -> Result<Part, SyntaxError> {
        self.enter()?;
        let start = self.i + 2;
        let end = find_brace_end(self.s, self.src, start).ok_or_else(|| self.err("}"))?;
        let inner = &self.src[start..end];
        let line = self.line;
        let in_dq = in_double || matches!(self.opts.mode, Mode::HereDoc | Mode::ParamWordInDouble | Mode::Arith);
        let p = parse_param_inner(inner, in_dq, line)?;
        self.count_lines(self.i, end);
        self.i = end + 1;
        self.depth -= 1;
        Ok(Part::Param(Box::new(p)))
    }

    /// Tenta ler uma expressão de chaves a partir de `{`. `None` deixa a posição como estava.
    fn try_brace(&mut self) -> Result<Option<Part>, SyntaxError> {
        let save_i = self.i;
        let save_line = self.line;
        self.enter()?;
        self.i += 1;
        let mut alts: Vec<Vec<Part>> = Vec::new();
        let ok = loop {
            let alt = self.parse_normal(true, false, true)?;
            match self.peek() {
                Some(b',') => {
                    alts.push(alt);
                    self.i += 1;
                }
                Some(b'}') => {
                    alts.push(alt);
                    self.i += 1;
                    break true;
                }
                _ => break false,
            }
        };
        self.depth -= 1;
        if ok && alts.len() >= 2 {
            return Ok(Some(Part::Brace(alts)));
        }
        if ok && alts.len() == 1 {
            if let [Part::Lit(text)] = alts[0].as_slice() {
                if let Some(seq) = parse_brace_seq(text) {
                    return Ok(Some(Part::BraceSeq(seq)));
                }
            }
        }
        self.i = save_i;
        self.line = save_line;
        Ok(None)
    }
}

enum DollarResult {
    Part(Part),
    Text(Vec<u8>),
}

/// Acha o `}` que fecha um `${` cujo conteúdo começa em `start`. Respeita aspas, escapes, `${`/`$(`
/// aninhados, crase e chaves aninhadas.
pub fn find_brace_end(s: &[u8], src: &str, start: usize) -> Option<usize> {
    let mut depth = 1;
    let mut j = start;
    while j < s.len() {
        match s[j] {
            b'\\' => j += 2,
            b'\'' => {
                // Dentro de `${...}` as aspas simples protegem `}` (bash).
                match s[j + 1..].iter().position(|b| *b == b'\'') {
                    Some(p) => j += p + 2,
                    None => return None,
                }
            }
            b'"' => {
                j += 1;
                while j < s.len() && s[j] != b'"' {
                    if s[j] == b'\\' {
                        j += 1;
                    } else if s[j] == b'$' && s.get(j + 1) == Some(&b'{') {
                        let e = find_brace_end(s, src, j + 2)?;
                        j = e;
                    } else if s[j] == b'$' && s.get(j + 1) == Some(&b'(') {
                        let n = brush_parser::scan_command_substitution(&src[j + 2..]).ok()?;
                        j += 1 + n;
                    }
                    j += 1;
                }
                j += 1;
            }
            b'`' => {
                j += 1;
                while j < s.len() && s[j] != b'`' {
                    if s[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                j += 1;
            }
            b'$' if s.get(j + 1) == Some(&b'(') => {
                let n = brush_parser::scan_command_substitution(&src[j + 2..]).ok()?;
                j += 2 + n;
            }
            b'$' if s.get(j + 1) == Some(&b'{') => {
                depth += 1;
                j += 2;
            }
            b'{' => {
                depth += 1;
                j += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
                j += 1;
            }
            _ => j += 1,
        }
    }
    None
}

/// `{1..5}`, `{01..10..3}`, `{a..e}`, `{e..a..2}`.
fn parse_brace_seq(text: &[u8]) -> Option<BraceSeq> {
    let s = std::str::from_utf8(text).ok()?;
    let pieces: Vec<&str> = s.split("..").collect();
    if pieces.len() != 2 && pieces.len() != 3 {
        return None;
    }
    let step: i64 = if pieces.len() == 3 { parse_seq_int(pieces[2])? } else { 1 };
    let (a, b) = (pieces[0], pieces[1]);
    if let (Some(x), Some(y)) = (parse_seq_int(a), parse_seq_int(b)) {
        let padded = |t: &str| {
            let d = t.trim_start_matches(['-', '+']);
            d.len() > 1 && d.starts_with('0')
        };
        let width = if padded(a) || padded(b) { a.len().max(b.len()) } else { 0 };
        return Some(BraceSeq::Num { start: x, end: y, step, width });
    }
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    if ab.len() == 1 && bb.len() == 1 && ab[0].is_ascii_alphabetic() && bb[0].is_ascii_alphabetic() {
        return Some(BraceSeq::Char { start: ab[0], end: bb[0], step });
    }
    None
}

fn parse_seq_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parâmetro "nu" no começo de `t`: nome, dígitos ou especial. Devolve o nome e o tamanho.
fn scan_param_name(t: &[u8]) -> Option<(ParamName, usize)> {
    let c = *t.first()?;
    if is_name_start(c) {
        let n = t.iter().take_while(|b| is_name_char(**b)).count();
        return Some((ParamName::Var(String::from_utf8_lossy(&t[..n]).into_owned()), n));
    }
    if c.is_ascii_digit() {
        let n = t.iter().take_while(|b| b.is_ascii_digit()).count();
        let v: u32 = std::str::from_utf8(&t[..n]).ok()?.parse().unwrap_or(u32::MAX);
        return Some((ParamName::Positional(v), n));
    }
    if matches!(c, b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!') {
        return Some((ParamName::Special(c), 1));
    }
    None
}

/// Fim do subscript que começa em `t[0] == '['`: índice do `]` correspondente.
fn scan_subscript(t: &[u8]) -> Option<usize> {
    let mut depth = 0;
    let mut j = 0;
    while j < t.len() {
        match t[j] {
            b'\\' => j += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            b'\'' => {
                if let Some(p) = t[j + 1..].iter().position(|b| *b == b'\'') {
                    j += p + 1;
                }
            }
            b'"' => {
                if let Some(p) = t[j + 1..].iter().position(|b| *b == b'"') {
                    j += p + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn index_of(text: &str, line: Line) -> Result<Index, SyntaxError> {
    match text {
        "@" => Ok(Index::At),
        "*" => Ok(Index::Star),
        _ => {
            let parts = parse_word(text, WordOpts::mode(Mode::Subscript, line))?;
            Ok(Index::Expr(Word { raw: Arc::from(text), parts: Arc::from(parts), assign: None }))
        }
    }
}

/// Conteúdo de `${...}` (sem as chaves).
pub fn parse_param_inner(inner: &str, in_dq: bool, line: Line) -> Result<ParamExp, SyntaxError> {
    let t = inner.as_bytes();
    let raw: Arc<str> = Arc::from(format!("${{{inner}}}"));
    let bad = |raw: Arc<str>| ParamExp {
        name: ParamName::Var(String::new()),
        index: None,
        indirect: false,
        op: ParamOp::Bad,
        braced: true,
        raw,
    };
    if t.is_empty() {
        return Ok(bad(raw));
    }

    // `${#parametro}` (comprimento), a não ser que o resto não seja só um parâmetro.
    if t[0] == b'#' && t.len() > 1 {
        if let Some((name, n)) = scan_param_name(&t[1..]) {
            let mut k = 1 + n;
            let mut index = None;
            if matches!(name, ParamName::Var(_)) && t.get(k) == Some(&b'[') {
                if let Some(e) = scan_subscript(&t[k..]) {
                    index = Some(index_of(&inner[k + 1..k + e], line)?);
                    k += e + 1;
                }
            }
            if k == t.len() {
                return Ok(ParamExp { name, index, indirect: false, op: ParamOp::Length, braced: true, raw });
            }
        }
    }

    let mut j = 0;
    let mut indirect = false;
    if t[0] == b'!' && t.len() > 1 {
        let rest = &t[1..];
        if let Some((ParamName::Var(name), n)) = scan_param_name(rest) {
            let after = &rest[n..];
            if after == b"*" || after == b"@" {
                return Ok(ParamExp {
                    name: ParamName::Var(name.clone()),
                    index: None,
                    indirect: false,
                    op: ParamOp::Names { prefix: name, star: after == b"*" },
                    braced: true,
                    raw,
                });
            }
            if after == b"[@]" || after == b"[*]" {
                return Ok(ParamExp {
                    name: ParamName::Var(name),
                    index: None,
                    indirect: false,
                    op: ParamOp::Keys { star: after == b"[*]" },
                    braced: true,
                    raw,
                });
            }
        }
        indirect = true;
        j = 1;
    }

    let Some((name, n)) = scan_param_name(&t[j..]) else {
        return Ok(bad(raw));
    };
    j += n;
    let mut index = None;
    if matches!(name, ParamName::Var(_)) && t.get(j) == Some(&b'[') {
        match scan_subscript(&t[j..]) {
            Some(e) => {
                index = Some(index_of(&inner[j + 1..j + e], line)?);
                j += e + 1;
            }
            None => return Ok(bad(raw)),
        }
    }
    let rest = &inner[j..];
    let op = parse_param_op(rest, in_dq, line)?;
    Ok(ParamExp { name, index, indirect, op, braced: true, raw })
}

fn word_parts(text: &str, in_dq: bool, pattern: bool, line: Line) -> Result<Vec<Part>, SyntaxError> {
    let opts = if in_dq {
        WordOpts::mode(if pattern { Mode::PatternInDouble } else { Mode::ParamWordInDouble }, line)
    } else {
        WordOpts::plain(line)
    };
    parse_word(text, opts)
}

/// Acha o primeiro `/` não escapado e fora de aspas e de expansões em `s`.
fn find_unquoted(s: &[u8], src: &str, target: u8) -> Option<usize> {
    let mut j = 0;
    while j < s.len() {
        match s[j] {
            b'\\' => j += 2,
            b'\'' => match s[j + 1..].iter().position(|b| *b == b'\'') {
                Some(p) => j += p + 2,
                None => return None,
            },
            b'"' => {
                j += 1;
                while j < s.len() && s[j] != b'"' {
                    if s[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                j += 1;
            }
            b'$' if s.get(j + 1) == Some(&b'{') => {
                j = find_brace_end(s, src, j + 2)? + 1;
            }
            b'$' if s.get(j + 1) == Some(&b'(') => {
                let n = brush_parser::scan_command_substitution(&src[j + 2..]).ok()?;
                j += 2 + n;
            }
            b'`' => {
                j += 1;
                while j < s.len() && s[j] != b'`' {
                    if s[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                j += 1;
            }
            c if c == target => return Some(j),
            _ => j += 1,
        }
    }
    None
}

/// Fim do offset de `${x:off:len}`: o `:` que não pertence a um `?:` aninhado.
fn find_substring_colon(s: &[u8]) -> Option<usize> {
    let mut pending_q = 0;
    let mut depth = 0;
    for (j, c) in s.iter().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'?' if depth == 0 => pending_q += 1,
            b':' if depth == 0 => {
                if pending_q > 0 {
                    pending_q -= 1;
                } else {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// `\/` vira `/`; outros pares com barra ficam como estão (`\\` inclusive, sem olhar o seguinte).
fn unescape_slash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('/') => out.push('/'),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_param_op(rest: &str, in_dq: bool, line: Line) -> Result<ParamOp, SyntaxError> {
    let r = rest.as_bytes();
    if r.is_empty() {
        return Ok(ParamOp::None);
    }
    let default = |colon: bool, kind: DefaultKind, w: &str| -> Result<ParamOp, SyntaxError> {
        Ok(ParamOp::Default { colon, kind, word: word_parts(w, in_dq, false, line)? })
    };
    match r[0] {
        b':' if r.len() >= 2 && matches!(r[1], b'-' | b'=' | b'?' | b'+') => {
            let kind = match r[1] {
                b'-' => DefaultKind::Use,
                b'=' => DefaultKind::Assign,
                b'?' => DefaultKind::Error,
                _ => DefaultKind::Alt,
            };
            default(true, kind, &rest[2..])
        }
        b'-' => default(false, DefaultKind::Use, &rest[1..]),
        b'=' => default(false, DefaultKind::Assign, &rest[1..]),
        b'?' => default(false, DefaultKind::Error, &rest[1..]),
        b'+' => default(false, DefaultKind::Alt, &rest[1..]),
        b'#' => {
            let longest = r.get(1) == Some(&b'#');
            let p = if longest { &rest[2..] } else { &rest[1..] };
            Ok(ParamOp::RemovePrefix { longest, pattern: word_parts(p, in_dq, true, line)? })
        }
        b'%' => {
            let longest = r.get(1) == Some(&b'%');
            let p = if longest { &rest[2..] } else { &rest[1..] };
            Ok(ParamOp::RemoveSuffix { longest, pattern: word_parts(p, in_dq, true, line)? })
        }
        b'/' => {
            let (kind, body) = match r.get(1) {
                Some(b'/') => (ReplaceKind::All, &rest[2..]),
                Some(b'#') => (ReplaceKind::Prefix, &rest[2..]),
                Some(b'%') => (ReplaceKind::Suffix, &rest[2..]),
                _ => (ReplaceKind::First, &rest[1..]),
            };
            let (pat, rep) = match find_unquoted(body.as_bytes(), body, b'/') {
                Some(k) => (&body[..k], Some(&body[k + 1..])),
                None => (body, None),
            };
            let pattern = word_parts(pat, in_dq, true, line)?;
            let replacement = match rep {
                // Entre aspas duplas o bash ainda tira a barra de `\/` na substituição (a barra
                // escapada é o delimitador protegido); fora delas a remoção de aspas já faz isso.
                Some(rp) if in_dq => Some(word_parts(&unescape_slash(rp), in_dq, false, line)?),
                Some(rp) => Some(word_parts(rp, in_dq, false, line)?),
                None => None,
            };
            Ok(ParamOp::Replace { kind, pattern, replacement })
        }
        b':' => {
            let body = &rest[1..];
            let (off, len) = match find_substring_colon(body.as_bytes()) {
                Some(k) => (&body[..k], Some(&body[k + 1..])),
                None => (body, None),
            };
            let offset = parse_word(off, WordOpts::mode(Mode::Arith, line))?;
            let length = match len {
                Some(l) => Some(parse_word(l, WordOpts::mode(Mode::Arith, line))?),
                None => None,
            };
            Ok(ParamOp::Substring { offset, length })
        }
        b'^' | b',' | b'~' => {
            let op = match r[0] {
                b'^' => CaseOp::Upper,
                b',' => CaseOp::Lower,
                _ => CaseOp::Toggle,
            };
            let all = r.get(1) == Some(&r[0]);
            let p = if all { &rest[2..] } else { &rest[1..] };
            let pattern = if p.is_empty() { None } else { Some(word_parts(p, in_dq, true, line)?) };
            Ok(ParamOp::Case { op, all, pattern })
        }
        b'@' if r.len() == 2 && b"QEPAKaUuLk".contains(&r[1]) => Ok(ParamOp::Transform(r[1])),
        _ => Ok(ParamOp::Bad),
    }
}

/// Corpo de here-doc que expande.
pub fn parse_heredoc(body: &str, line: Line) -> Result<Vec<Part>, SyntaxError> {
    parse_word(body, WordOpts::mode(Mode::HereDoc, line))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(s: &str) -> Vec<Part> {
        parse_word(s, WordOpts::normal(1)).expect("parse")
    }

    #[test]
    fn literals_quotes_and_escapes() {
        let p = parts(r#"a'b c'"d $x"\e"#);
        assert!(matches!(&p[0], Part::Lit(v) if v == b"a"));
        assert!(matches!(&p[1], Part::Quoted(v) if v == b"b c"));
        match &p[2] {
            Part::Double(inner) => {
                assert!(matches!(&inner[0], Part::Quoted(v) if v == b"d "));
                assert!(matches!(&inner[1], Part::Param(pe) if pe.name == ParamName::Var("x".into())));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&p[3], Part::Quoted(v) if v == b"e"));
    }

    #[test]
    fn param_forms() {
        let p = parts("${x:-a b}${#y}${!z}${a[@]}${s//o/0}${v:2:3}${!p@}${!m[@]}${x@Q}${u^^}");
        assert_eq!(p.len(), 10);
        let ops: Vec<String> = p
            .iter()
            .map(|x| match x {
                Part::Param(pe) => format!("{:?}", std::mem::discriminant(&pe.op)),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(ops.len(), 10);
        match &p[1] {
            Part::Param(pe) => assert!(matches!(pe.op, ParamOp::Length)),
            _ => panic!(),
        }
        match &p[2] {
            Part::Param(pe) => assert!(pe.indirect),
            _ => panic!(),
        }
        match &p[3] {
            Part::Param(pe) => assert!(matches!(pe.index, Some(Index::At))),
            _ => panic!(),
        }
        match &p[6] {
            Part::Param(pe) => assert!(matches!(&pe.op, ParamOp::Names { prefix, star: false } if prefix == "p")),
            _ => panic!(),
        }
        match &p[7] {
            Part::Param(pe) => assert!(matches!(pe.op, ParamOp::Keys { star: false })),
            _ => panic!(),
        }
    }

    #[test]
    fn braces_and_sequences() {
        let p = parts("{a,b}.txt");
        assert!(matches!(&p[0], Part::Brace(alts) if alts.len() == 2));
        let p = parts("{01..10..3}");
        assert_eq!(p.len(), 1);
        assert!(matches!(&p[0], Part::BraceSeq(BraceSeq::Num { start: 1, end: 10, step: 3, width: 2 })));
        let p = parts("{x}");
        assert!(matches!(&p[0], Part::Lit(v) if v == b"{x}"));
        let p = parts("{a..}");
        assert!(matches!(&p[0], Part::Lit(v) if v == b"{a..}"));
        let p = parts("{a,{b,c}d}");
        assert!(matches!(&p[0], Part::Brace(alts) if alts.len() == 2));
    }

    #[test]
    fn tilde_only_at_start_or_after_colon_in_assignment() {
        let p = parts("~/x");
        assert!(matches!(&p[0], Part::Tilde(v) if v.is_empty()));
        let p = parts("a:~/b");
        assert!(matches!(&p[0], Part::Lit(v) if v == b"a:~/b"));
        let p = parse_word("~/a:~/b", WordOpts::assignment(1)).unwrap();
        assert_eq!(p.iter().filter(|x| matches!(x, Part::Tilde(_))).count(), 2);
        let p = parts("\"~\"");
        assert!(matches!(&p[0], Part::Double(_)));
    }
}
