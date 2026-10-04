//! Leitura de programas: texto -> tokens (brush) -> aliases -> AST do brush -> nosso AST.
//!
//! O bash lê e executa um comando completo por vez: um erro de sintaxe na linha 5 só aparece depois
//! que as linhas 1 a 4 rodaram, e um `alias` definido numa linha vale nas seguintes. O [`Reader`]
//! reproduz isso: parseia o resto do texto de uma vez (o caso comum, barato) e, quando há erro,
//! procura o maior prefixo de linhas que parseia sozinho, devolve esses comandos pra execução e só
//! depois o erro.

use std::collections::HashMap;
use std::sync::Arc;

use brush_parser::{ParseError, ParserOptions, Token, TokenizerError};

use crate::ast::{Line, Program};
use crate::lower::Lowerer;

/// Erro de sintaxe com a mensagem do bash (sem o prefixo `bash: -c: line N: `).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub line: Line,
    pub message: String,
    /// Segunda linha que o bash imprime em "near unexpected token" (o texto da linha, sem aspas).
    pub context: Option<String>,
}

impl SyntaxError {
    pub fn new(line: Line, message: String) -> SyntaxError {
        SyntaxError { line, message, context: None }
    }
}

/// O que muda o parse e vem do estado do shell.
#[derive(Clone, Debug, Default)]
pub struct ParseEnv {
    /// Aliases ativos (só quando `expand_aliases` está ligado).
    pub aliases: Option<Arc<HashMap<String, String>>>,
    /// Modo POSIX (`sh`).
    pub posix: bool,
}

fn parser_options(env: &ParseEnv) -> ParserOptions {
    ParserOptions { enable_extended_globbing: true, posix_mode: env.posix, sh_mode: false, ..ParserOptions::default() }
}

/// Resultado de um parse completo de um trecho.
pub struct Parsed {
    pub program: Program,
    /// Linha (no texto inteiro) onde começa cada comando completo de `program.commands`.
    pub starts: Vec<Line>,
    /// Avisos de here-doc terminado pelo fim do texto: (linha do `<<`, delimitador).
    pub heredoc_eof: Vec<(Line, String)>,
}

/// Número de linhas como o bash conta pra "unexpected end of file": linhas do texto (a última sem
/// newline também conta) mais um.
fn eof_line(src: &str, offset: Line) -> Line {
    let mut n = src.bytes().filter(|b| *b == b'\n').count() as Line;
    if !src.is_empty() && !src.ends_with('\n') {
        n += 1;
    }
    offset + n + 1
}

fn line_text(src: &str, line: usize) -> String {
    src.split('\n').nth(line.saturating_sub(1)).unwrap_or("").to_string()
}

fn tokenizer_error(e: &TokenizerError, src: &str, offset: Line) -> SyntaxError {
    let at = |p: &brush_parser::SourcePosition| offset + p.line as Line;
    let eof = eof_line(src, offset);
    match e {
        TokenizerError::UnterminatedSingleQuote(p) | TokenizerError::UnterminatedAnsiCQuote(p) => {
            SyntaxError::new(at(p), "unexpected EOF while looking for matching `''".to_string())
        }
        TokenizerError::UnterminatedDoubleQuote(p) => {
            SyntaxError::new(at(p), "unexpected EOF while looking for matching `\"'".to_string())
        }
        TokenizerError::UnterminatedBackquote(p) => {
            SyntaxError::new(at(p), "unexpected EOF while looking for matching ``'".to_string())
        }
        TokenizerError::UnterminatedCommandSubstitution | TokenizerError::UnterminatedExpansion => {
            SyntaxError::new(eof, "unexpected EOF while looking for matching `)'".to_string())
        }
        TokenizerError::UnterminatedVariable => {
            SyntaxError::new(eof, "unexpected EOF while looking for matching `}'".to_string())
        }
        TokenizerError::UnterminatedExtendedGlob(_) => {
            SyntaxError::new(eof, "unexpected EOF while looking for matching `)'".to_string())
        }
        _ => SyntaxError::new(eof, "syntax error: unexpected end of file".to_string()),
    }
}

/// Os primeiros `k` tokens podem começar um programa válido (parseiam, ou só falta texto)?
fn viable(tokens: &[Token], k: usize, opts: &ParserOptions) -> bool {
    match brush_parser::parse_tokens(&tokens[..k], opts) {
        Ok(_) => true,
        Err(ParseError::ParsingAtEndOfInput) => true,
        Err(ParseError::ParsingNear(pos)) => {
            // Falha marcada no último token consumido conta como "precisa de mais texto".
            tokens[..k].iter().position(|t| t.location().start.index >= pos.index).is_none_or(|i| i + 1 >= k)
        }
        Err(_) => false,
    }
}

/// Como o bash (um parser LR), o token do erro é o primeiro que não pode continuar o que veio
/// antes: o fim do maior prefixo viável, achado por busca binária.
fn parse_error(e: &ParseError, tokens: &[Token], src: &str, offset: Line, opts: &ParserOptions) -> SyntaxError {
    if let ParseError::Tokenizing { inner, .. } = e {
        return tokenizer_error(inner, src, offset);
    }
    let n = tokens.len();
    if viable(tokens, n, opts) && matches!(e, ParseError::ParsingAtEndOfInput) {
        return SyntaxError::new(eof_line(src, offset), "syntax error: unexpected end of file".to_string());
    }
    let (mut lo, mut hi) = (0usize, n);
    // Invariante: viable(lo), !viable(hi) (ou hi == n com erro).
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if viable(tokens, mid, opts) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let idx = if viable(tokens, hi, opts) { None } else { Some(hi - 1) };
    match idx.and_then(|i| tokens.get(i)) {
        Some(tok) => {
            let text = match tok {
                Token::Operator(s, _) if s == "\n" => "newline".to_string(),
                t => t.to_str().to_string(),
            };
            let pos = &tok.location().start;
            SyntaxError {
                line: offset + pos.line as Line,
                message: format!("syntax error near unexpected token `{text}'"),
                context: Some(line_text(src, pos.line)),
            }
        }
        None => SyntaxError::new(eof_line(src, offset), "syntax error: unexpected end of file".to_string()),
    }
}

/// Tokeniza, aplica aliases, parseia e converte. `offset` é somado às linhas.
pub fn parse_text(src: &str, offset: Line, source: &Arc<str>, env: &ParseEnv) -> Result<Parsed, SyntaxError> {
    let opts = parser_options(env);
    let tokens = brush_parser::tokenize_str_with_options(src, &opts.tokenizer_options())
        .map_err(|e| tokenizer_error(&e, src, offset))?;
    let tokens = match &env.aliases {
        Some(a) if !a.is_empty() => expand_aliases(tokens, a, &opts),
        _ => tokens,
    };
    let program = brush_parser::parse_tokens(&tokens, &opts).map_err(|e| parse_error(&e, &tokens, src, offset, &opts))?;
    let mut starts = Vec::with_capacity(program.complete_commands.len());
    for c in &program.complete_commands {
        use brush_parser::ast::SourceLocation;
        let l = c.location().map_or(offset + 1, |l| offset + l.start.line as Line);
        starts.push(l);
    }
    let mut lw = Lowerer::new(offset, source.clone());
    let program = lw.program(&program).map_err(|mut e| {
        // Erro dentro de `$(...)`: o bash mostra a linha do comando de fora.
        if e.context.is_none() && e.message.starts_with("syntax error near") {
            e.context = Some(line_text(src, e.line.saturating_sub(offset) as usize));
        }
        e
    })?;
    Ok(Parsed { program, starts, heredoc_eof: lw.heredoc_eof })
}

/// Parse de corpo de `$(...)`, crase ou substituição de processo: tudo ou nada.
pub fn parse_nested(src: &str, line: Line) -> Result<Program, SyntaxError> {
    let env = ParseEnv::default();
    let parsed = parse_text(src, line.saturating_sub(1), &Arc::from(""), &env)?;
    Ok(parsed.program)
}

/// Leitor incremental de um texto de programa (script, `-c`, `eval`, `source`).
pub struct Reader {
    src: String,
    /// Byte onde começa o que falta ler.
    pos: usize,
    /// Linha (1-based) de `pos`.
    line: Line,
    source: Arc<str>,
}

/// Um pedaço lido: comandos prontos pra executar, ou o erro de sintaxe que vem a seguir.
pub enum Chunk {
    Commands(Parsed),
    Error(SyntaxError),
}

impl Reader {
    pub fn new(src: String, first_line: Line, source: Arc<str>) -> Reader {
        Reader { src, pos: 0, line: first_line, source }
    }

    pub fn at_end(&self) -> bool {
        self.src[self.pos..].trim_matches(|c: char| c.is_ascii_whitespace()).is_empty()
    }

    /// Byte inicial da linha `line` (relativa ao começo do texto), a partir de `pos`.
    fn offset_of_line(&self, line: Line) -> usize {
        let mut cur = self.line;
        let mut idx = self.pos;
        let bytes = self.src.as_bytes();
        while cur < line && idx < bytes.len() {
            if bytes[idx] == b'\n' {
                cur += 1;
            }
            idx += 1;
        }
        idx
    }

    /// Recomeça a leitura na linha `line` (depois de executar os comandos que vieram antes dela).
    pub fn seek_line(&mut self, line: Line) {
        if line > self.line {
            self.pos = self.offset_of_line(line);
            self.line = line;
        }
    }

    /// Marca tudo como lido.
    pub fn finish(&mut self) {
        self.pos = self.src.len();
    }

    /// Lê o próximo pedaço. `None` no fim.
    pub fn next_chunk(&mut self, env: &ParseEnv) -> Option<Chunk> {
        if self.at_end() {
            self.finish();
            return None;
        }
        let rest = &self.src[self.pos..];
        let offset = self.line - 1;
        match parse_text(rest, offset, &self.source, env) {
            Ok(parsed) => {
                self.finish();
                Some(Chunk::Commands(parsed))
            }
            Err(err) => {
                // Maior prefixo de linhas que parseia sozinho e termina em comando completo.
                let starts: Vec<usize> = std::iter::once(0)
                    .chain(rest.match_indices('\n').map(|(i, _)| i + 1))
                    .filter(|i| *i <= rest.len())
                    .collect();
                let err_rel = (err.line.saturating_sub(offset)) as usize;
                let max_lines = err_rel.min(starts.len().saturating_sub(1));
                for k in (1..=max_lines).rev() {
                    let end = starts[k];
                    let prefix = &rest[..end];
                    if prefix.ends_with("\\\n") && !prefix.ends_with("\\\\\n") {
                        continue;
                    }
                    if let Ok(parsed) = parse_text(prefix, offset, &self.source, env) {
                        if parsed.program.commands.is_empty() || !parsed.heredoc_eof.is_empty() {
                            continue;
                        }
                        self.pos += end;
                        self.line += k as Line;
                        return Some(Chunk::Commands(parsed));
                    }
                }
                self.finish();
                Some(Chunk::Error(err))
            }
        }
    }
}

/// Palavras reservadas depois das quais vem posição de comando.
fn opens_command(word: &str) -> bool {
    matches!(word, "then" | "do" | "else" | "elif" | "{" | "!" | "time" | "if" | "while" | "until")
}

fn is_assignment_word(w: &str) -> bool {
    let Some(eq) = w.find('=') else { return false };
    let name = w[..eq].trim_end_matches('+');
    let name = match name.find('[') {
        Some(b) if name.ends_with(']') => &name[..b],
        _ => name,
    };
    crate::word::is_name(name.as_bytes())
}

/// Substitui aliases em posição de comando, como o bash faz ao ler.
fn expand_aliases(tokens: Vec<Token>, aliases: &HashMap<String, String>, opts: &ParserOptions) -> Vec<Token> {
    /// Token na fila, com os aliases em expansão (pra não recursar) e se a palavra seguinte também
    /// deve ser checada (alias cujo valor termina em espaço).
    struct Item {
        tok: Token,
        active: Vec<String>,
        check_next: bool,
    }
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut cmd_pos = true;
    let mut queue: std::collections::VecDeque<Item> =
        tokens.into_iter().map(|tok| Item { tok, active: Vec::new(), check_next: false }).collect();
    let mut guard = 0;
    while let Some(item) = queue.pop_front() {
        guard += 1;
        match &item.tok {
            Token::Operator(op, _) => {
                cmd_pos = matches!(op.as_str(), ";" | "&" | "&&" | "||" | "|" | "|&" | "(" | ")" | "\n" | ";;" | ";&" | ";;&");
                out.push(item.tok);
            }
            Token::Word(w, loc) => {
                let is_plain = !w.contains(['\'', '"', '\\', '$', '`']);
                if cmd_pos && is_plain && guard < 10_000 && !item.active.contains(w) {
                    if let Some(value) = aliases.get(w) {
                        let mut chain = item.active.clone();
                        chain.push(w.clone());
                        let trailing_space = value.ends_with([' ', '\t']);
                        if let Ok(mut repl) = brush_parser::tokenize_str_with_options(value, &opts.tokenizer_options()) {
                            for t in &mut repl {
                                let span = brush_parser::SourceSpan { start: loc.start.clone(), end: loc.end.clone() };
                                *t = match t {
                                    Token::Word(s, _) => Token::Word(s.clone(), span),
                                    Token::Operator(s, _) => Token::Operator(s.clone(), span),
                                };
                            }
                            let n = repl.len();
                            for (i, tok) in repl.into_iter().enumerate().rev() {
                                queue.push_front(Item { tok, active: chain.clone(), check_next: i + 1 == n && trailing_space });
                            }
                            cmd_pos = true;
                            continue;
                        }
                    }
                }
                cmd_pos = opens_command(w) || (cmd_pos && is_assignment_word(w)) || item.check_next;
                out.push(item.tok);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<Parsed, SyntaxError> {
        parse_text(src, 0, &Arc::from(""), &ParseEnv::default())
    }

    #[test]
    fn simple_program_and_lines() {
        let p = parse("echo a\nif true; then\n  echo b\nfi\necho c\n").expect("parse");
        assert_eq!(p.program.commands.len(), 3);
        assert_eq!(p.starts, vec![1, 2, 5]);
    }

    #[test]
    fn syntax_error_messages() {
        let e = parse("echo a; fi").err().expect("erro");
        assert_eq!(e.message, "syntax error near unexpected token `fi'");
        assert_eq!(e.context.as_deref(), Some("echo a; fi"));
        let e = parse("for x in; do").err().expect("erro");
        assert_eq!((e.line, e.message.as_str()), (2, "syntax error: unexpected end of file"));
        let e = parse("echo 'abc").err().expect("erro");
        assert_eq!(e.message, "unexpected EOF while looking for matching `''");
    }

    #[test]
    fn reader_runs_prefix_before_error() {
        let mut r = Reader::new("echo a\necho b\nfi\necho c\n".to_string(), 1, Arc::from(""));
        let env = ParseEnv::default();
        match r.next_chunk(&env) {
            Some(Chunk::Commands(p)) => assert_eq!(p.program.commands.len(), 2),
            _ => panic!("esperava comandos"),
        }
        match r.next_chunk(&env) {
            Some(Chunk::Error(e)) => assert_eq!((e.line, e.message.as_str()), (3, "syntax error near unexpected token `fi'")),
            _ => panic!("esperava erro"),
        }
        assert!(r.next_chunk(&env).is_none());
    }

    #[test]
    fn aliases_expand_in_command_position() {
        let mut a = HashMap::new();
        a.insert("ll".to_string(), "echo listando".to_string());
        let env = ParseEnv { aliases: Some(Arc::new(a)), posix: false };
        let p = parse_text("ll x; echo ll", 0, &Arc::from(""), &env).expect("parse");
        let list = &p.program.commands[0];
        let first = &list.items[0].and_or.first.commands[0];
        match first {
            crate::ast::Command::Simple(s) => {
                assert_eq!(&*s.words[0].raw, "echo");
                assert_eq!(&*s.words[1].raw, "listando");
                assert_eq!(&*s.words[2].raw, "x");
            }
            _ => panic!(),
        }
        match &list.items[1].and_or.first.commands[0] {
            crate::ast::Command::Simple(s) => assert_eq!(&*s.words[1].raw, "ll"),
            _ => panic!(),
        }
    }
}
