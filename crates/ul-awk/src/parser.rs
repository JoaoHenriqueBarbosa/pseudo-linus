//! Parser do awk (descida recursiva) com a precedência, as regras de newline e as mensagens de erro do
//! gawk 5.2.1.
//!
//! A gramática segue o comportamento observado no gawk: atribuição só onde a gramática dele espera uma
//! expressão completa (`1 + a = 3` é erro, `x == y = 2` não), relacionais não associativos, `>` e `|`
//! como redirecionamento no topo da lista do `print`, operando de `getline <` sem concatenação, alvo de
//! redirecionamento de saída com concatenação.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::*;
use crate::lexer::{LexError, Lexer, Tok, Token, is_reserved};

/// Variáveis especiais, com índices fixos na tabela de globais (a ordem é o índice).
pub const SPECIALS: &[&str] = &[
    "NF", "NR", "FNR", "FS", "OFS", "ORS", "RS", "SUBSEP", "CONVFMT", "OFMT", "RSTART", "RLENGTH", "FILENAME",
    "ENVIRON", "ARGC", "ARGV", "ARGIND", "ERRNO", "FIELDWIDTHS", "FPAT", "IGNORECASE", "BINMODE", "LINT", "PROCINFO",
    "RT", "TEXTDOMAIN", "SYMTAB", "FUNCTAB", "PREC", "ROUNDMODE",
];

/// Índices das especiais (mesma ordem de [`SPECIALS`]).
pub mod sv {
    pub const NF: u32 = 0;
    pub const NR: u32 = 1;
    pub const FNR: u32 = 2;
    pub const FS: u32 = 3;
    pub const OFS: u32 = 4;
    pub const ORS: u32 = 5;
    pub const RS: u32 = 6;
    pub const SUBSEP: u32 = 7;
    pub const CONVFMT: u32 = 8;
    pub const OFMT: u32 = 9;
    pub const RSTART: u32 = 10;
    pub const RLENGTH: u32 = 11;
    pub const FILENAME: u32 = 12;
    pub const ENVIRON: u32 = 13;
    pub const ARGC: u32 = 14;
    pub const ARGV: u32 = 15;
    pub const ARGIND: u32 = 16;
    pub const ERRNO: u32 = 17;
    pub const FIELDWIDTHS: u32 = 18;
    pub const FPAT: u32 = 19;
    pub const IGNORECASE: u32 = 20;
    pub const BINMODE: u32 = 21;
    pub const LINT: u32 = 22;
    pub const PROCINFO: u32 = 23;
    pub const RT: u32 = 24;
    pub const TEXTDOMAIN: u32 = 25;
    pub const SYMTAB: u32 = 26;
    pub const FUNCTAB: u32 = 27;
    pub const PREC: u32 = 28;
    pub const ROUNDMODE: u32 = 29;
    pub const COUNT: u32 = 30;
}

/// Falha do parse: o texto pro stderr (linhas completas, com prefixo) e o código de saída.
#[derive(Debug)]
pub struct ParseFailure {
    pub stderr: String,
    pub code: i32,
}

/// Resultado de um parse bem-sucedido: o programa e os avisos (linhas completas pro stderr).
pub struct Parsed {
    pub program: Program,
    pub warnings: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Context {
    Begin,
    End,
    BeginFile,
    EndFile,
    Main,
    Function,
}

/// Erro de sintaxe: posição do token problemático e mensagem.
struct SyntaxError {
    pos: usize,
    line: u32,
    msg: String,
    /// O token problemático é newline ou fim do fonte.
    at_newline: bool,
    at_eof: bool,
}

type PResult<T> = Result<T, SyntaxError>;

struct State {
    program: Program,
    globals: HashMap<String, u32>,
    funcs: HashMap<String, u32>,
    /// Nomes usados como variável (pra detectar conflito com nome de função).
    var_uses: Vec<(String, u16, u32)>,
    /// Chamadas: (função, número de argumentos, fonte, linha).
    calls: Vec<(u32, usize, u16, u32)>,
    errors: String,
    warnings: String,
    regex_ids: HashMap<Vec<u8>, u32>,
    namespace: String,
}

struct Parser<'a, 's, 'l> {
    prog: &'a str,
    st: &'s mut State,
    lex: Lexer<'a>,
    tok: Token,
    src_idx: u16,
    src_name: String,
    text: &'a [u8],
    locals: Option<Vec<Rc<str>>>,
    ctx: Context,
    loop_depth: u32,
    switch_depth: u32,
    loader: &'s mut IncludeLoader<'l>,
}

/// Carregador de `@include`: recebe o nome como escrito e devolve a fonte (nome pra mensagens e texto),
/// ou a mensagem de erro do `open` (`No such file or directory`).
pub type IncludeLoader<'l> = dyn FnMut(&str) -> Result<Source, String> + 'l;

/// Analisa todas as fontes como um programa só.
pub fn parse(prog: &str, sources: Vec<Source>, loader: &mut IncludeLoader<'_>) -> Result<Parsed, ParseFailure> {
    let mut st = State {
        program: Program::default(),
        globals: HashMap::new(),
        funcs: HashMap::new(),
        var_uses: Vec::new(),
        calls: Vec::new(),
        errors: String::new(),
        warnings: String::new(),
        regex_ids: HashMap::new(),
        namespace: "awk".to_string(),
    };
    for (i, name) in SPECIALS.iter().enumerate() {
        st.globals.insert((*name).to_string(), i as u32);
        st.program.globals.push(Rc::from(*name));
    }
    for src in sources {
        if let Err(code) = parse_source(prog, &mut st, src, loader) {
            let mut all = std::mem::take(&mut st.warnings);
            all.push_str(&st.errors);
            return Err(ParseFailure { stderr: all, code });
        }
    }
    // Conferências do fim do parse.
    let mut st = st;
    for (name, src, line) in std::mem::take(&mut st.var_uses) {
        if let Some(&f) = st.funcs.get(&name)
            && st.program.functions[f as usize].defined {
                let loc = location(&st.program.sources, src, line);
                st.errors.push_str(&format!(
                    "{prog}: {loc}: error: function `{name}' called with space between name and `(',\nor used as a variable or an array\n"
                ));
            }
    }
    for (f, nargs, src, line) in std::mem::take(&mut st.calls) {
        let func = &st.program.functions[f as usize];
        if func.defined && nargs > func.params.len() {
            let loc = location(&st.program.sources, src, line);
            st.warnings.push_str(&format!("{prog}: {loc}: warning: function `{}' called with more arguments than declared\n", func.name));
        }
    }
    if !st.errors.is_empty() {
        let mut all = std::mem::take(&mut st.warnings);
        all.push_str(&st.errors);
        return Err(ParseFailure { stderr: all, code: 1 });
    }
    Ok(Parsed { program: st.program, warnings: st.warnings })
}

/// Analisa uma fonte (e as que ela incluir). Em erro de sintaxe, a mensagem já está em `st.errors`.
fn parse_source(prog: &str, st: &mut State, src: Source, loader: &mut IncludeLoader<'_>) -> Result<(), i32> {
    let idx = st.program.sources.len() as u16;
    st.program.sources.push(src.clone());
    st.namespace = "awk".to_string();
    let mut p = Parser {
        prog,
        lex: Lexer::new(&src.text),
        tok: Token { tok: Tok::Eof, start: 0, line: 1, space_before: false, synthetic: false },
        st,
        src_idx: idx,
        src_name: src.name.clone(),
        text: &src.text,
        locals: None,
        ctx: Context::Main,
        loop_depth: 0,
        switch_depth: 0,
        loader,
    };
    let r = p.advance().and_then(|_| p.program_items());
    p.flush_lex_warnings();
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.msg == "\0include" => Err(1),
        Err(e) => {
            let (msg, code) = p.format_syntax_error(&e);
            p.st.errors.push_str(&msg);
            Err(code)
        }
    }
}

/// `cmd. line:3` ou `prog.awk:3`.
pub fn location(sources: &[Source], src: u16, line: u32) -> String {
    let name = sources.get(src as usize).map(|s| s.name.as_str()).unwrap_or("cmd. line");
    format!("{name}:{line}")
}

fn is_assign_op(t: &Tok) -> Option<Option<BinOp>> {
    Some(match t {
        Tok::Assign => None,
        Tok::AddAssign => Some(BinOp::Add),
        Tok::SubAssign => Some(BinOp::Sub),
        Tok::MulAssign => Some(BinOp::Mul),
        Tok::DivAssign => Some(BinOp::Div),
        Tok::ModAssign => Some(BinOp::Mod),
        Tok::PowAssign => Some(BinOp::Pow),
        _ => return None,
    })
}

/// Pode começar um operando de concatenação?
fn starts_concat_operand(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Number(_)
            | Tok::Str(_)
            | Tok::Name(_)
            | Tok::FuncName(_)
            | Tok::Builtin(_)
            | Tok::Dollar
            | Tok::Not
            | Tok::LParen
            | Tok::Incr
            | Tok::Decr
            | Tok::IndirectName(_)
            | Tok::TypedRegex(_)
    )
}

impl<'a, 's, 'l> Parser<'a, 's, 'l> {
    fn advance(&mut self) -> PResult<Token> {
        let next = match self.lex.next_token() {
            Ok(t) => t,
            Err(e) => return Err(self.lex_error(e)),
        };
        Ok(std::mem::replace(&mut self.tok, next))
    }

    fn lex_error(&self, e: LexError) -> SyntaxError {
        SyntaxError { pos: e.pos, line: e.line, msg: e.message, at_newline: false, at_eof: false }
    }

    fn flush_lex_warnings(&mut self) {
        for (line, w) in std::mem::take(&mut self.lex.warnings) {
            let loc = format!("{}:{}", self.src_name, line);
            self.st.warnings.push_str(&format!("{}: {loc}: warning: {w}\n", self.prog));
        }
    }

    fn error_here(&self) -> SyntaxError {
        let t = &self.tok;
        SyntaxError {
            pos: t.start,
            line: t.line,
            msg: String::new(),
            at_newline: t.tok == Tok::Newline,
            at_eof: t.tok == Tok::Eof,
        }
    }

    fn error_at(&self, t: &Token, msg: &str) -> SyntaxError {
        SyntaxError { pos: t.start, line: t.line, msg: msg.to_string(), at_newline: false, at_eof: false }
    }

    /// Texto do erro de sintaxe no formato do gawk, e o código de saída.
    fn format_syntax_error(&self, e: &SyntaxError) -> (String, i32) {
        let prog = self.prog;
        let name = &self.src_name;
        if e.msg == "\0nul" {
            return (format!("{prog}: {name}:{}: fatal: error: invalid character '\\000' in source code\n", e.line), 2);
        }
        if let Some(rest) = e.msg.strip_prefix("\0fatal:") {
            return (format!("{prog}: {name}:{}: fatal: {rest}\n", e.line), 2);
        }
        let text = self.text;
        if e.at_eof && text.len() > 1 && text[0] == b'\n' && text.last() == Some(&b'\n') && e.msg.is_empty() {
            // Peculiaridade do gawk 5.2.1: com fonte que começa com newline, a "linha" mostrada é o
            // byte logo depois desse newline.
            let line = text.iter().filter(|b| **b == b'\n').count();
            let shown = String::from_utf8_lossy(&text[1..2]).into_owned();
            return (format!("{prog}: {name}:{line}: {shown}\n{prog}: {name}:{line}: ^ unexpected newline or end of string\n"), 1);
        }
        if e.at_eof && (text.is_empty() || text.last() == Some(&b'\n')) && e.msg.is_empty() {
            let line = text.iter().filter(|b| **b == b'\n').count().max(1);
            return (
                format!(
                    "{prog}: {name}:{line}: (END OF FILE)\n{prog}: {name}:{line}: ^ source files / command-line arguments must contain complete functions or rules\n"
                ),
                1,
            );
        }
        let pos = e.pos.min(text.len());
        let line_start = text[..pos].iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
        let line_end = text[pos..].iter().position(|b| *b == b'\n').map(|i| pos + i).unwrap_or(text.len());
        let line_text = String::from_utf8_lossy(&text[line_start..line_end]).into_owned();
        let caret: String = text[line_start..pos].iter().map(|b| if *b == b'\t' { '\t' } else { ' ' }).collect();
        let lineno = if e.at_newline { e.line + 1 } else { e.line };
        let msg = if !e.msg.is_empty() {
            e.msg.clone()
        } else if e.at_newline || e.at_eof {
            "unexpected newline or end of string".to_string()
        } else {
            "syntax error".to_string()
        };
        (format!("{prog}: {name}:{lineno}: {line_text}\n{prog}: {name}:{lineno}: {caret}^ {msg}\n"), 1)
    }

    /// Erro semântico (não para o parse): `prog: fonte:linha: error: msg`.
    fn semantic_error(&mut self, line: u32, msg: &str) {
        self.st.errors.push_str(&format!("{}: {}:{}: error: {msg}\n", self.prog, self.src_name, line));
    }

    fn plain_error(&mut self, line: u32, msg: &str) {
        self.st.errors.push_str(&format!("{}: {}:{}: {msg}\n", self.prog, self.src_name, line));
    }

    fn warning(&mut self, line: u32, msg: &str) {
        self.st.warnings.push_str(&format!("{}: {}:{}: warning: {msg}\n", self.prog, self.src_name, line));
    }

    fn is(&self, t: &Tok) -> bool {
        &self.tok.tok == t
    }

    fn expect(&mut self, t: Tok) -> PResult<Token> {
        if self.tok.tok == t { self.advance() } else { Err(self.error_here()) }
    }

    fn skip_newlines(&mut self) -> PResult<()> {
        while self.is(&Tok::Newline) {
            self.advance()?;
        }
        Ok(())
    }

    fn skip_terminators(&mut self) -> PResult<()> {
        while matches!(self.tok.tok, Tok::Newline | Tok::Semi) {
            self.advance()?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- itens do programa

    fn program_items(&mut self) -> PResult<()> {
        loop {
            self.skip_terminators()?;
            if self.is(&Tok::Eof) {
                return Ok(());
            }
            self.item()?;
        }
    }

    fn item(&mut self) -> PResult<()> {
        let line = self.tok.line;
        match self.tok.tok.clone() {
            Tok::Begin | Tok::End | Tok::BeginFile | Tok::EndFile => {
                let which = self.tok.tok.clone();
                self.advance()?;
                let (ctx, label) = match which {
                    Tok::Begin => (Context::Begin, "BEGIN"),
                    Tok::End => (Context::End, "END"),
                    Tok::BeginFile => (Context::BeginFile, "BEGINFILE"),
                    _ => (Context::EndFile, "ENDFILE"),
                };
                if !self.is(&Tok::LBrace) {
                    if matches!(self.tok.tok, Tok::Newline | Tok::Semi | Tok::Eof) {
                        self.plain_error(line, &format!("{label} blocks must have an action part"));
                        return Ok(());
                    }
                    return Err(self.error_here());
                }
                self.ctx = ctx;
                let body = self.block()?;
                self.ctx = Context::Main;
                let p = &mut self.st.program;
                match which {
                    Tok::Begin => p.begin.push(body),
                    Tok::End => p.end.push(body),
                    Tok::BeginFile => p.beginfile.push(body),
                    _ => p.endfile.push(body),
                }
                self.after_item()
            }
            Tok::Function => self.function(),
            Tok::Directive(d) => self.directive(d),
            Tok::LBrace => {
                self.ctx = Context::Main;
                let body = self.block()?;
                self.st.program.rules.push(Rule { pattern: Pattern::All, action: Some(body), src: self.src_idx, line });
                self.after_item()
            }
            _ => {
                self.ctx = Context::Main;
                let first = self.exp()?;
                let first = self.check_expr(first)?;
                let pattern = if self.is(&Tok::Comma) {
                    self.advance()?;
                    self.skip_newlines()?;
                    let second = self.exp()?;
                    let second = self.check_expr(second)?;
                    let id = self.st.program.range_count;
                    self.st.program.range_count += 1;
                    Pattern::Range(first, second, id)
                } else {
                    Pattern::Expr(first)
                };
                let action = if self.is(&Tok::LBrace) { Some(self.block()?) } else { None };
                self.st.program.rules.push(Rule { pattern, action, src: self.src_idx, line });
                if self.st.program.rules.last().is_some_and(|r| r.action.is_none()) {
                    match self.tok.tok {
                        Tok::Newline | Tok::Semi | Tok::Eof => Ok(()),
                        _ => Err(self.error_here()),
                    }
                } else {
                    self.after_item()
                }
            }
        }
    }

    /// Depois de `}` de uma regra pode vir outra regra na mesma linha.
    fn after_item(&mut self) -> PResult<()> {
        Ok(())
    }

    fn directive(&mut self, d: &str) -> PResult<()> {
        let line = self.tok.line;
        self.advance()?;
        let Tok::Str(arg) = self.tok.tok.clone() else { return Err(self.error_here()) };
        self.advance()?;
        match d {
            "namespace" => {
                let name = String::from_utf8_lossy(&arg).into_owned();
                let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !valid {
                    self.semantic_error(line, &format!("namespace name `{name}' must meet identifier naming rules"));
                } else if is_reserved(&name) {
                    self.semantic_error(line, &format!("using reserved identifier `{name}' as a namespace is not allowed"));
                } else {
                    self.st.namespace = name;
                }
            }
            "include" => {
                let name = String::from_utf8_lossy(&arg).into_owned();
                match (self.loader)(&name) {
                    Ok(src) => {
                        // Cada arquivo entra uma vez só (o gawk ignora inclusões repetidas).
                        if !self.st.program.sources.iter().any(|s| s.name == src.name) {
                            let saved_ns = self.st.namespace.clone();
                            let r = parse_source(self.prog, self.st, src, self.loader);
                            self.st.namespace = saved_ns;
                            if r.is_err() {
                                return Err(SyntaxError { pos: 0, line, msg: "\0include".into(), at_newline: false, at_eof: false });
                            }
                        }
                    }
                    Err(e) => {
                        self.semantic_error(line, &format!("cannot open source file `{name}' for reading: {e}"));
                    }
                }
            }
            _ => {
                let name = String::from_utf8_lossy(&arg).into_owned();
                self.plain_error(line, &format!("fatal: extension: cannot open library `{name}' (cannot open shared object file)"));
            }
        }
        Ok(())
    }

    fn qualify(&self, name: &str) -> String {
        if name.contains("::") {
            if let Some(rest) = name.strip_prefix("awk::") {
                return rest.to_string();
            }
            return name.to_string();
        }
        if self.st.namespace == "awk" || name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            name.to_string()
        } else {
            format!("{}::{}", self.st.namespace, name)
        }
    }

    fn function(&mut self) -> PResult<()> {
        let line = self.tok.line;
        self.advance()?;
        let name_tok = self.tok.clone();
        let raw_name: Rc<str> = match &self.tok.tok {
            Tok::Name(n) | Tok::FuncName(n) => n.clone(),
            // Fora do namespace `awk`, extensão do gawk pode ser redefinida (vira `ns::nome`).
            Tok::Builtin(b) if self.st.namespace != "awk" && !crate::lexer::is_posix_builtin(b) => Rc::from(*b),
            Tok::Builtin(b) => {
                let msg = format!("`{b}' is a built-in function, it cannot be redefined");
                return Err(self.error_at(&name_tok, &msg));
            }
            _ => return Err(self.error_here()),
        };
        let name = self.qualify(&raw_name);
        self.advance()?;
        self.expect(Tok::LParen)?;
        let mut params: Vec<Rc<str>> = Vec::new();
        let mut param_lines = Vec::new();
        self.skip_newlines()?;
        if !self.is(&Tok::RParen) {
            loop {
                match &self.tok.tok {
                    Tok::Name(n) => {
                        params.push(n.clone());
                        param_lines.push(self.tok.line);
                    }
                    // Extensão do gawk pode ser nome de parâmetro (sombreia a função no corpo).
                    Tok::Builtin(b) if !crate::lexer::is_posix_builtin(b) => {
                        params.push(Rc::from(*b));
                        param_lines.push(self.tok.line);
                    }
                    _ => return Err(self.error_here()),
                }
                self.advance()?;
                self.skip_newlines()?;
                if self.is(&Tok::Comma) {
                    self.advance()?;
                    self.skip_newlines()?;
                    continue;
                }
                break;
            }
        }
        self.expect(Tok::RParen)?;
        // Conferências dos parâmetros.
        for (i, p) in params.iter().enumerate() {
            let pline = param_lines[i];
            if **p == *raw_name || **p == *name {
                self.semantic_error(pline, &format!("function `{name}': cannot use function name as parameter name"));
            } else if SPECIALS.contains(&&**p) {
                self.semantic_error(pline, &format!("function `{name}': cannot use special variable `{p}' as a function parameter"));
            } else if let Some(j) = params[..i].iter().position(|q| q == p) {
                self.semantic_error(
                    line,
                    &format!("function `{name}': parameter #{}, `{p}', duplicates parameter #{}", i + 1, j + 1),
                );
            }
        }
        if let Some(&f) = self.st.funcs.get(&name) {
            if self.st.program.functions[f as usize].defined {
                self.semantic_error(line, &format!("function name `{name}' previously defined"));
            }
        } else if self.st.globals.contains_key(&name) && (self.st.globals[&name] >= sv::COUNT) {
            self.semantic_error(line, &format!("function name `{name}' previously defined"));
        }
        self.skip_newlines()?;
        if !self.is(&Tok::LBrace) {
            return Err(self.error_here());
        }
        self.locals = Some(params.clone());
        self.ctx = Context::Function;
        let body = self.block()?;
        self.locals = None;
        self.ctx = Context::Main;
        let idx = self.func_index(&name);
        let f = &mut self.st.program.functions[idx as usize];
        if !f.defined {
            f.params = params;
            f.body = body;
            f.defined = true;
            f.src = self.src_idx;
            f.line = line;
        }
        Ok(())
    }

    fn func_index(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.st.funcs.get(name) {
            return i;
        }
        let i = self.st.program.functions.len() as u32;
        self.st.program.functions.push(Function {
            name: Rc::from(name),
            params: Vec::new(),
            body: Vec::new(),
            defined: false,
            src: self.src_idx,
            line: 0,
        });
        self.st.funcs.insert(name.to_string(), i);
        i
    }

    fn global_index(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.st.globals.get(name) {
            return i;
        }
        let i = self.st.program.globals.len() as u32;
        self.st.program.globals.push(Rc::from(name));
        self.st.globals.insert(name.to_string(), i);
        i
    }

    fn resolve_var(&mut self, raw: &str, line: u32) -> Var {
        if let Some(locals) = &self.locals
            && let Some(i) = locals.iter().position(|p| &**p == raw) {
                return Var::Local(i as u32);
            }
        let name = self.qualify(raw);
        self.st.var_uses.push((name.clone(), self.src_idx, line));
        Var::Global(self.global_index(&name))
    }

    fn regex_id(&mut self, body: &[u8]) -> u32 {
        if let Some(&i) = self.st.regex_ids.get(body) {
            return i;
        }
        let i = self.st.program.regexes.len() as u32;
        self.st.program.regexes.push(Rc::from(body));
        self.st.program.regex_locs.push((self.src_idx, self.tok.line));
        self.st.regex_ids.insert(body.to_vec(), i);
        i
    }

    // ---------------------------------------------------------------- comandos

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect(Tok::LBrace)?;
        let mut out = Vec::new();
        loop {
            self.skip_terminators()?;
            if self.is(&Tok::RBrace) {
                self.advance()?;
                return Ok(out);
            }
            if self.is(&Tok::Eof) {
                return Err(self.error_here());
            }
            out.push(self.statement()?);
        }
    }

    fn mk(&self, kind: StmtKind, line: u32) -> Stmt {
        Stmt { kind, src: self.src_idx, line }
    }

    /// Fim de comando simples: `;`, newline, ou `}` (que fica pro bloco).
    fn end_simple(&mut self) -> PResult<()> {
        match self.tok.tok {
            Tok::Semi | Tok::Newline => {
                self.advance()?;
                Ok(())
            }
            Tok::RBrace | Tok::Eof => Ok(()),
            _ => Err(self.error_here()),
        }
    }

    /// Corpo de `if`/`while`/`for`: newline opcional e um comando (ou `;` vazio).
    fn body(&mut self) -> PResult<Stmt> {
        self.skip_newlines()?;
        if self.is(&Tok::Semi) {
            let line = self.tok.line;
            self.advance()?;
            return Ok(self.mk(StmtKind::Nop, line));
        }
        self.statement()
    }

    fn statement(&mut self) -> PResult<Stmt> {
        let line = self.tok.line;
        let start_tok = self.tok.clone();
        match self.tok.tok.clone() {
            Tok::LBrace => {
                let b = self.block()?;
                Ok(self.mk(StmtKind::Block(b), line))
            }
            Tok::If => {
                self.advance()?;
                self.expect(Tok::LParen)?;
                let cond = self.exp()?;
                let cond = self.check_expr(cond)?;
                self.expect(Tok::RParen)?;
                let then = self.body()?;
                // `else` pode vir depois de newlines (e de um `;` que o comando simples já consumiu).
                let save_nl = self.is(&Tok::Newline);
                if save_nl || self.is(&Tok::Else) {
                    while self.is(&Tok::Newline) {
                        self.advance()?;
                    }
                }
                let els = if self.is(&Tok::Else) {
                    self.advance()?;
                    Some(Box::new(self.body()?))
                } else {
                    None
                };
                Ok(self.mk(StmtKind::If(cond, Box::new(then), els), line))
            }
            Tok::While => {
                self.advance()?;
                self.expect(Tok::LParen)?;
                let cond = self.exp()?;
                let cond = self.check_expr(cond)?;
                self.expect(Tok::RParen)?;
                if self.is(&Tok::Semi) {
                    self.advance()?;
                    let nop = self.mk(StmtKind::Nop, line);
                    return Ok(self.mk(StmtKind::While(cond, Box::new(nop)), line));
                }
                self.loop_depth += 1;
                let body = self.body();
                self.loop_depth -= 1;
                Ok(self.mk(StmtKind::While(cond, Box::new(body?)), line))
            }
            Tok::Do => {
                self.advance()?;
                self.loop_depth += 1;
                let body = self.body();
                self.loop_depth -= 1;
                let body = body?;
                self.skip_terminators()?;
                self.expect(Tok::While)?;
                self.expect(Tok::LParen)?;
                let cond = self.exp()?;
                let cond = self.check_expr(cond)?;
                self.expect(Tok::RParen)?;
                self.end_simple()?;
                Ok(self.mk(StmtKind::DoWhile(Box::new(body), cond), line))
            }
            Tok::For => self.for_stmt(),
            Tok::Switch => self.switch_stmt(),
            Tok::Semi => {
                self.advance()?;
                Ok(self.mk(StmtKind::Nop, line))
            }
            Tok::Next | Tok::NextFile => {
                let is_next = self.is(&Tok::Next);
                self.advance()?;
                let word = if is_next { "next" } else { "nextfile" };
                match self.ctx {
                    Context::Begin => self.semantic_error(line, &format!("`{word}' used in BEGIN action")),
                    Context::End => self.semantic_error(line, &format!("`{word}' used in END action")),
                    Context::BeginFile if is_next => self.semantic_error(line, "`next' used in BEGINFILE action"),
                    Context::EndFile => self.semantic_error(line, &format!("`{word}' used in ENDFILE action")),
                    _ => {}
                }
                self.end_simple()?;
                Ok(self.mk(if is_next { StmtKind::Next } else { StmtKind::NextFile }, line))
            }
            Tok::Exit => {
                self.advance()?;
                let e = if matches!(self.tok.tok, Tok::Semi | Tok::Newline | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    let e = self.exp()?;
                    Some(self.check_expr(e)?)
                };
                self.end_simple()?;
                Ok(self.mk(StmtKind::Exit(e), line))
            }
            Tok::Return => {
                if self.ctx != Context::Function {
                    return Err(self.error_at(&start_tok, "`return' used outside function context"));
                }
                self.advance()?;
                let e = if matches!(self.tok.tok, Tok::Semi | Tok::Newline | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    let e = self.exp()?;
                    Some(self.check_expr(e)?)
                };
                self.end_simple()?;
                Ok(self.mk(StmtKind::Return(e), line))
            }
            Tok::Break | Tok::Continue => {
                let is_break = self.is(&Tok::Break);
                self.advance()?;
                if is_break && self.loop_depth == 0 && self.switch_depth == 0 {
                    // O gawk 5.2.1 imprime esta mensagem duas vezes.
                    self.semantic_error(line, "`break' is not allowed outside a loop or switch");
                    self.semantic_error(line, "`break' is not allowed outside a loop or switch");
                } else if !is_break && self.loop_depth == 0 {
                    self.semantic_error(line, "`continue' is not allowed outside a loop");
                    self.semantic_error(line, "`continue' is not allowed outside a loop");
                }
                self.end_simple()?;
                Ok(self.mk(if is_break { StmtKind::Break } else { StmtKind::Continue }, line))
            }
            Tok::Delete => {
                self.advance()?;
                let paren = self.is(&Tok::LParen);
                if paren {
                    self.advance()?;
                }
                let name = match &self.tok.tok {
                    Tok::Name(n) => n.clone(),
                    _ => return Err(self.error_here()),
                };
                let nline = self.tok.line;
                self.advance()?;
                let var = self.resolve_var(&name, nline);
                let subs = self.subscript_groups()?;
                if paren {
                    self.expect(Tok::RParen)?;
                }
                self.end_simple()?;
                Ok(self.mk(StmtKind::Delete(var, subs), line))
            }
            Tok::Print | Tok::Printf => self.print_stmt(),
            _ => {
                let e = self.exp()?;
                let e = self.check_expr(e)?;
                self.end_simple()?;
                Ok(self.mk(StmtKind::Expr(e), line))
            }
        }
    }

    fn for_stmt(&mut self) -> PResult<Stmt> {
        let line = self.tok.line;
        self.advance()?;
        self.expect(Tok::LParen)?;
        // `for (x in arr)`: nome, `in`, nome, `)`.
        let init = if self.is(&Tok::Semi) {
            None
        } else if matches!(self.tok.tok, Tok::Print | Tok::Printf) {
            // O gawk aceita comando simples (até print) na inicialização do for.
            Some(Box::new(self.print_core()?))
        } else {
            let e = self.exp()?;
            if self.is(&Tok::RParen) {
                if let Expr::In(keys, arr, path) = e
                    && keys.len() == 1
                        && let Expr::Var(v) = &keys[0] {
                            let v = *v;
                            self.advance()?;
                            self.loop_depth += 1;
                            let body = self.body();
                            self.loop_depth -= 1;
                            return Ok(self.mk(StmtKind::ForIn(v, arr, path, Box::new(body?)), line));
                        }
                return Err(self.error_here());
            }
            let e = self.check_expr(e)?;
            Some(Box::new(self.mk(StmtKind::Expr(e), line)))
        };
        self.expect(Tok::Semi)?;
        self.skip_newlines()?;
        let cond = if self.is(&Tok::Semi) {
            None
        } else {
            let e = self.exp()?;
            Some(self.check_expr(e)?)
        };
        self.expect(Tok::Semi)?;
        self.skip_newlines()?;
        let incr = if self.is(&Tok::RParen) {
            None
        } else if matches!(self.tok.tok, Tok::Print | Tok::Printf) {
            Some(Box::new(self.print_core()?))
        } else {
            let e = self.exp()?;
            let e = self.check_expr(e)?;
            Some(Box::new(self.mk(StmtKind::Expr(e), line)))
        };
        self.expect(Tok::RParen)?;
        if self.is(&Tok::Semi) {
            self.advance()?;
            let nop = self.mk(StmtKind::Nop, line);
            return Ok(self.mk(StmtKind::For(init, cond, incr, Box::new(nop)), line));
        }
        self.loop_depth += 1;
        let body = self.body();
        self.loop_depth -= 1;
        Ok(self.mk(StmtKind::For(init, cond, incr, Box::new(body?)), line))
    }

    fn switch_stmt(&mut self) -> PResult<Stmt> {
        let line = self.tok.line;
        self.advance()?;
        self.expect(Tok::LParen)?;
        let e = self.exp()?;
        let e = self.check_expr(e)?;
        self.expect(Tok::RParen)?;
        self.skip_newlines()?;
        self.expect(Tok::LBrace)?;
        let mut cases: Vec<(Option<CaseLabel>, Vec<Stmt>)> = Vec::new();
        let mut seen_default = false;
        self.switch_depth += 1;
        loop {
            self.skip_terminators()?;
            match self.tok.tok.clone() {
                Tok::RBrace => {
                    self.advance()?;
                    break;
                }
                Tok::Case => {
                    let cline = self.tok.line;
                    self.advance()?;
                    let label = match self.tok.tok.clone() {
                        Tok::Number(n) => {
                            self.advance()?;
                            CaseLabel::Num(n)
                        }
                        Tok::Minus | Tok::Plus => {
                            let neg = self.is(&Tok::Minus);
                            self.advance()?;
                            let Tok::Number(n) = self.tok.tok else { return Err(self.error_here()) };
                            self.advance()?;
                            CaseLabel::Num(if neg { -n } else { n })
                        }
                        Tok::Str(s) => {
                            self.advance()?;
                            CaseLabel::Str(s)
                        }
                        Tok::Slash | Tok::DivAssign => {
                            let t = self.tok.clone();
                            let r = self.lex.read_regex(&t).map_err(|e| self.lex_error(e))?;
                            self.tok = r;
                            let Tok::Regex(body) = self.tok.tok.clone() else { unreachable!() };
                            self.advance()?;
                            CaseLabel::Regex(self.regex_id(&body))
                        }
                        Tok::TypedRegex(body) => {
                            self.advance()?;
                            CaseLabel::Regex(self.regex_id(&body))
                        }
                        _ => return Err(self.error_here()),
                    };
                    let _ = cline;
                    self.expect(Tok::Colon)?;
                    cases.push((Some(label), Vec::new()));
                }
                Tok::Default => {
                    let dline = self.tok.line;
                    self.advance()?;
                    self.expect(Tok::Colon)?;
                    if seen_default {
                        self.semantic_error(dline, "duplicate `default' detected in switch body");
                    }
                    seen_default = true;
                    cases.push((None, Vec::new()));
                }
                Tok::Eof => return Err(self.error_here()),
                _ => {
                    if cases.is_empty() {
                        return Err(self.error_here());
                    }
                    let s = self.statement()?;
                    cases.last_mut().expect("case").1.push(s);
                }
            }
        }
        self.switch_depth -= 1;
        Ok(self.mk(StmtKind::Switch(e, cases), line))
    }

    fn print_stmt(&mut self) -> PResult<Stmt> {
        let s = self.print_core()?;
        self.end_simple()?;
        Ok(s)
    }

    /// `print`/`printf` sem o terminador.
    fn print_core(&mut self) -> PResult<Stmt> {
        let line = self.tok.line;
        let is_printf = self.is(&Tok::Printf);
        self.advance()?;
        let mut args = Vec::new();
        if !matches!(self.tok.tok, Tok::Semi | Tok::Newline | Tok::RBrace | Tok::Gt | Tok::Append | Tok::Pipe | Tok::PipeAmp | Tok::Eof) {
            let first = self.exp_ctx(true)?;
            match first {
                Expr::List(items) if !self.is(&Tok::Comma) => args.extend(items),
                other => {
                    args.push(other);
                    while self.is(&Tok::Comma) {
                        self.advance()?;
                        self.skip_newlines()?;
                        args.push(self.exp_ctx(true)?);
                    }
                }
            }
        }
        let mut checked = Vec::with_capacity(args.len());
        for a in args {
            checked.push(self.check_expr(a)?);
        }
        let args = checked;
        let redirect = match self.tok.tok {
            Tok::Gt | Tok::Append | Tok::Pipe | Tok::PipeAmp => {
                let kind = self.tok.tok.clone();
                self.advance()?;
                let target = self.concat_level(true)?;
                let target = self.check_expr(target)?;
                Some(match kind {
                    Tok::Gt => Redirect::File(target),
                    Tok::Append => Redirect::Append(target),
                    Tok::Pipe => Redirect::Pipe(target),
                    _ => Redirect::Coproc(target),
                })
            }
            _ => None,
        };
        if is_printf {
            Ok(self.mk(StmtKind::Printf(args, redirect), line))
        } else {
            Ok(self.mk(StmtKind::Print(args, redirect), line))
        }
    }

    /// Zero ou mais grupos `[i, j]` seguidos (arrays de arrays).
    fn subscript_groups(&mut self) -> PResult<Vec<Vec<Expr>>> {
        let mut groups = Vec::new();
        while self.is(&Tok::LBracket) {
            self.advance()?;
            let subs = self.expr_list_until(Tok::RBracket)?;
            if subs.is_empty() {
                return Err(self.error_here());
            }
            self.expect(Tok::RBracket)?;
            groups.push(subs);
        }
        Ok(groups)
    }

    /// Lista de expressões separadas por vírgula até `end` (sem consumir `end`).
    fn expr_list_until(&mut self, end: Tok) -> PResult<Vec<Expr>> {
        let mut out = Vec::new();
        if self.is(&end) {
            return Ok(out);
        }
        loop {
            let e = self.exp()?;
            out.push(self.check_expr(e)?);
            if self.is(&Tok::Comma) {
                self.advance()?;
                self.skip_newlines()?;
                continue;
            }
            return Ok(out);
        }
    }

    /// Rejeita uma lista entre parênteses fora do lugar.
    fn check_expr(&self, e: Expr) -> PResult<Expr> {
        if let Expr::List(_) = e {
            return Err(self.error_here());
        }
        Ok(e)
    }

    // ---------------------------------------------------------------- expressões

    fn exp(&mut self) -> PResult<Expr> {
        self.exp_ctx(false)
    }

    /// Expressão completa (o `exp` da gramática do gawk). `no_gt`: no topo da lista do `print`, onde
    /// `>` e `|` são redirecionamento.
    fn exp_ctx(&mut self, no_gt: bool) -> PResult<Expr> {
        let e = self.ternary(no_gt)?;
        self.maybe_assign(e, no_gt)
    }

    /// Se vier operador de atribuição depois de um alvo simples, é atribuição.
    fn maybe_assign(&mut self, e: Expr, no_gt: bool) -> PResult<Expr> {
        let Some(op) = is_assign_op(&self.tok.tok) else { return Ok(e) };
        if let Expr::IncDec(target, false, _) = &e
            && let LValue::Field(_) = **target {
                // O gawk só percebe ao reduzir a atribuição: o circunflexo fica depois do lado direito.
                self.advance()?;
                let _ = self.exp_ctx(no_gt)?;
                let t = self.tok.clone();
                return Err(self.error_at(&t, "cannot assign a value to the result of a field post-increment expression"));
            }
        if !e.is_lvalue() {
            return Err(self.error_here());
        }
        self.advance()?;
        self.skip_newlines_after_assign()?;
        let rhs = self.exp_ctx(no_gt)?;
        let rhs = self.check_expr(rhs)?;
        let lv = Box::new(e.into_lvalue().map_err(|_| self.error_here())?);
        Ok(match op {
            None => Expr::Assign(lv, Box::new(rhs)),
            Some(op) => Expr::AugAssign(op, lv, Box::new(rhs)),
        })
    }

    fn skip_newlines_after_assign(&mut self) -> PResult<()> {
        Ok(())
    }

    fn ternary(&mut self, no_gt: bool) -> PResult<Expr> {
        let cond = self.or_expr(no_gt)?;
        if !self.is(&Tok::Question) {
            return Ok(cond);
        }
        self.advance()?;
        self.skip_newlines()?;
        let a = self.ternary(no_gt)?;
        let a = self.maybe_assign(a, no_gt)?;
        self.skip_newlines()?;
        self.expect(Tok::Colon)?;
        self.skip_newlines()?;
        let b = self.ternary(no_gt)?;
        let b = self.maybe_assign(b, no_gt)?;
        Ok(Expr::Cond(Box::new(self.check_expr(cond)?), Box::new(self.check_expr(a)?), Box::new(self.check_expr(b)?)))
    }

    fn or_expr(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.and_expr(no_gt)?;
        while self.is(&Tok::Or) {
            let l = self.check_expr(left)?;
            self.advance()?;
            self.skip_newlines()?;
            let r = self.and_expr(no_gt)?;
            let r = self.maybe_assign(r, no_gt)?;
            left = Expr::Or(Box::new(l), Box::new(self.check_expr(r)?));
        }
        Ok(left)
    }

    fn and_expr(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.in_expr(no_gt)?;
        while self.is(&Tok::And) {
            let l = self.check_expr(left)?;
            self.advance()?;
            self.skip_newlines()?;
            let r = self.in_expr(no_gt)?;
            let r = self.maybe_assign(r, no_gt)?;
            left = Expr::And(Box::new(l), Box::new(self.check_expr(r)?));
        }
        Ok(left)
    }

    fn in_expr(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.match_expr(no_gt)?;
        while self.is(&Tok::In) {
            self.advance()?;
            let name = match &self.tok.tok {
                Tok::Name(n) => n.clone(),
                _ => return Err(self.error_here()),
            };
            let line = self.tok.line;
            self.advance()?;
            let arr = self.resolve_var(&name, line);
            let path = self.subscript_groups()?;
            let keys = match left {
                Expr::List(items) => items,
                Expr::Group(inner) => vec![*inner],
                other => vec![other],
            };
            left = Expr::In(keys, arr, path);
        }
        Ok(left)
    }

    fn match_expr(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.rel_expr(no_gt)?;
        while matches!(self.tok.tok, Tok::Tilde | Tok::NoMatch) {
            let neg = self.is(&Tok::NoMatch);
            let l = self.check_expr(left)?;
            self.advance()?;
            let r = self.rel_expr(no_gt)?;
            let r = self.maybe_assign(r, no_gt)?;
            left = Expr::Match(neg, Box::new(l), Box::new(self.check_expr(r)?));
        }
        Ok(left)
    }

    fn rel_expr(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.concat_level(no_gt)?;
        loop {
            // `cmd | getline [var]` e `cmd |& getline [var]`.
            if matches!(self.tok.tok, Tok::Pipe | Tok::PipeAmp) {
                let coproc = self.is(&Tok::PipeAmp);
                let save = self.tok.clone();
                // Só é getline se o próximo token for `getline`; no print, `|` é redirecionamento.
                let next_is_getline = self.peek_is_getline();
                if !next_is_getline {
                    if no_gt {
                        return Ok(left);
                    }
                    return Err(self.error_at(&save, ""));
                }
                self.advance()?; // `|`
                self.advance()?; // `getline`
                let target = self.opt_getline_target()?;
                let cmd = Box::new(self.check_expr(left)?);
                let src = if coproc { GetlineSrc::Coproc(cmd) } else { GetlineSrc::Cmd(cmd) };
                left = Expr::Getline(src, target.map(Box::new));
                // O resultado ainda pode entrar em conta e concatenação: `cmd | getline x + 1`,
                // `cmd | getline x y`.
                left = self.continue_operand(left, no_gt)?;
                continue;
            }
            let op = match self.tok.tok {
                Tok::Lt => CmpOp::Lt,
                Tok::Le => CmpOp::Le,
                Tok::Ne => CmpOp::Ne,
                Tok::Eq => CmpOp::Eq,
                Tok::Ge => CmpOp::Ge,
                Tok::Gt if !no_gt => CmpOp::Gt,
                _ => return Ok(left),
            };
            let l = self.check_expr(left)?;
            self.advance()?;
            let r = self.concat_level(no_gt)?;
            let r = self.maybe_assign(r, no_gt)?;
            left = Expr::Cmp(op, Box::new(l), Box::new(self.check_expr(r)?));
            // Relacionais não são associativos.
            if matches!(self.tok.tok, Tok::Lt | Tok::Le | Tok::Ne | Tok::Eq | Tok::Ge) || (self.is(&Tok::Gt) && !no_gt) {
                return Err(self.error_here());
            }
        }
    }

    /// Continua uma expressão já lida como operando esquerdo de `* / %`, `+ -` e concatenação.
    fn continue_operand(&mut self, mut left: Expr, no_gt: bool) -> PResult<Expr> {
        loop {
            let op = match self.tok.tok {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance()?;
            let r = self.unary(no_gt)?;
            left = self.fold(op, left, self.check_expr(r)?);
        }
        loop {
            let op = match self.tok.tok {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance()?;
            let r = self.multiplicative(no_gt)?;
            left = self.fold(op, left, self.check_expr(r)?);
        }
        while starts_concat_operand(&self.tok.tok) {
            let r = self.additive(no_gt)?;
            left = Expr::Concat(Box::new(left), Box::new(self.check_expr(r)?));
        }
        Ok(left)
    }

    /// O token depois do atual é `getline`? Olha sem consumir (relendo o lexer a partir da posição).
    fn peek_is_getline(&mut self) -> bool {
        let save_pos = self.lex.pos();
        let save_line = self.lex.line();
        let mut probe = Lexer::new(self.text);
        probe.set_position(save_pos, save_line);
        matches!(probe.next_token().map(|t| t.tok), Ok(Tok::Getline))
    }

    fn concat_level(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.additive(no_gt)?;
        let mut concatenated = false;
        loop {
            // `/=` depois de algo que não pode ser alvo de atribuição começa uma regex (o gawk lê a
            // barra antes do `=` e a gramática decide): `print $/= b/ c /= d/`.
            let regex_slash = self.is(&Tok::DivAssign) && (concatenated || !left.is_lvalue());
            if !starts_concat_operand(&self.tok.tok) && !regex_slash {
                return Ok(left);
            }
            let l = self.check_expr(left)?;
            let r = self.additive(no_gt)?;
            left = Expr::Concat(Box::new(l), Box::new(self.check_expr(r)?));
            concatenated = true;
        }
    }

    fn additive(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.multiplicative(no_gt)?;
        loop {
            let op = match self.tok.tok {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(left),
            };
            let l = self.check_expr(left)?;
            self.advance()?;
            let r = self.multiplicative(no_gt)?;
            left = self.fold(op, l, self.check_expr(r)?);
        }
    }

    fn multiplicative(&mut self, no_gt: bool) -> PResult<Expr> {
        let mut left = self.unary(no_gt)?;
        loop {
            let op = match self.tok.tok {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => return Ok(left),
            };
            let l = self.check_expr(left)?;
            self.advance()?;
            let r = self.unary(no_gt)?;
            left = self.fold(op, l, self.check_expr(r)?);
        }
    }

    /// Dobra de constantes como o gawk (e o erro de divisão por zero em tempo de parse).
    fn fold(&mut self, op: BinOp, l: Expr, r: Expr) -> Expr {
        // Parênteses não criam nó no gawk: `(4)/0` também é dobrado.
        fn peel(e: &Expr) -> &Expr {
            match e {
                Expr::Group(inner) => peel(inner),
                other => other,
            }
        }
        if let (Expr::Num(a), Expr::Num(b)) = (peel(&l), peel(&r)) {
            let (a, b) = (*a, *b);
            let line = self.tok.line;
            let v = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => {
                    if b == 0.0 {
                        self.semantic_error(line, "division by zero attempted");
                        return Expr::Binary(op, Box::new(l), Box::new(r));
                    }
                    a / b
                }
                BinOp::Mod => {
                    if b == 0.0 {
                        self.semantic_error(line, "division by zero attempted in `%'");
                        return Expr::Binary(op, Box::new(l), Box::new(r));
                    }
                    a % b
                }
                BinOp::Pow => crate::value::pow(a, b),
            };
            return Expr::Num(v);
        }
        Expr::Binary(op, Box::new(l), Box::new(r))
    }

    fn unary(&mut self, no_gt: bool) -> PResult<Expr> {
        match self.tok.tok {
            Tok::Not => {
                self.advance()?;
                let e = self.unary(no_gt)?;
                Ok(Expr::Not(Box::new(self.check_expr(e)?)))
            }
            Tok::Minus => {
                self.advance()?;
                let e = self.unary(no_gt)?;
                Ok(match self.check_expr(e)? {
                    Expr::Num(n) => Expr::Num(-n),
                    e => Expr::Neg(Box::new(e)),
                })
            }
            Tok::Plus => {
                self.advance()?;
                let e = self.unary(no_gt)?;
                Ok(match self.check_expr(e)? {
                    Expr::Num(n) => Expr::Num(n),
                    e => Expr::Plus(Box::new(e)),
                })
            }
            _ => self.power(no_gt),
        }
    }

    fn power(&mut self, no_gt: bool) -> PResult<Expr> {
        let base = self.postfix(no_gt)?;
        if self.is(&Tok::Caret) {
            self.advance()?;
            // O expoente é associativo à direita e aceita sinal: `2^-1`, `2^3^2`.
            let exp = self.power_operand(no_gt)?;
            return Ok(self.fold(BinOp::Pow, self.check_expr(base)?, self.check_expr(exp)?));
        }
        Ok(base)
    }

    fn power_operand(&mut self, no_gt: bool) -> PResult<Expr> {
        match self.tok.tok {
            Tok::Minus => {
                self.advance()?;
                let e = self.power_operand(no_gt)?;
                Ok(match self.check_expr(e)? {
                    Expr::Num(n) => Expr::Num(-n),
                    e => Expr::Neg(Box::new(e)),
                })
            }
            Tok::Plus => {
                self.advance()?;
                let e = self.power_operand(no_gt)?;
                Ok(Expr::Plus(Box::new(self.check_expr(e)?)))
            }
            Tok::Not => {
                self.advance()?;
                let e = self.power_operand(no_gt)?;
                Ok(Expr::Not(Box::new(self.check_expr(e)?)))
            }
            _ => self.power(no_gt),
        }
    }

    fn postfix(&mut self, no_gt: bool) -> PResult<Expr> {
        let e = self.primary(no_gt)?;
        if matches!(self.tok.tok, Tok::Incr | Tok::Decr) && e.is_lvalue() {
            let delta = if self.is(&Tok::Incr) { 1.0 } else { -1.0 };
            self.advance()?;
            let lv = e.into_lvalue().map_err(|_| self.error_here())?;
            return Ok(Expr::IncDec(Box::new(lv), false, delta));
        }
        Ok(e)
    }

    /// Operando de `$`: sem pós-incremento (que se aplica ao campo) e sem operadores binários.
    fn dollar_operand(&mut self) -> PResult<Expr> {
        match self.tok.tok {
            Tok::Incr | Tok::Decr => {
                let delta = if self.is(&Tok::Incr) { 1.0 } else { -1.0 };
                self.advance()?;
                if !matches!(self.tok.tok, Tok::Name(_) | Tok::Dollar) {
                    return Err(self.error_here());
                }
                let target = self.primary(false)?;
                let lv = target.into_lvalue().map_err(|_| self.error_here())?;
                Ok(Expr::IncDec(Box::new(lv), true, delta))
            }
            // Unário dentro do `$`: o operando aceita pós-incremento e `^` (`$-i++` é `$(-(i++))`).
            Tok::Minus => {
                self.advance()?;
                let e = self.dollar_unary_operand()?;
                Ok(match e {
                    Expr::Num(n) => Expr::Num(-n),
                    e => Expr::Neg(Box::new(e)),
                })
            }
            Tok::Plus => {
                self.advance()?;
                let e = self.dollar_unary_operand()?;
                Ok(Expr::Plus(Box::new(e)))
            }
            Tok::Not => {
                self.advance()?;
                let e = self.dollar_unary_operand()?;
                Ok(Expr::Not(Box::new(e)))
            }
            Tok::Dollar => {
                // `$$a++`: o `++` vale pro campo de dentro.
                self.advance()?;
                let inner = Expr::Field(Box::new(self.dollar_operand()?));
                if matches!(self.tok.tok, Tok::Incr | Tok::Decr) {
                    let delta = if self.is(&Tok::Incr) { 1.0 } else { -1.0 };
                    self.advance()?;
                    let lv = inner.into_lvalue().map_err(|_| self.error_here())?;
                    return Ok(Expr::IncDec(Box::new(lv), false, delta));
                }
                Ok(inner)
            }
            _ => self.primary(false),
        }
    }

    fn dollar_unary_operand(&mut self) -> PResult<Expr> {
        match self.tok.tok {
            Tok::Minus | Tok::Plus | Tok::Not => self.dollar_operand(),
            _ => self.power(false),
        }
    }

    fn opt_getline_target(&mut self) -> PResult<Option<LValue>> {
        match self.tok.tok.clone() {
            Tok::Name(n) => {
                let line = self.tok.line;
                self.advance()?;
                let var = self.resolve_var(&n, line);
                let subs = self.subscript_groups()?;
                if subs.is_empty() { Ok(Some(LValue::Var(var))) } else { Ok(Some(LValue::Index(var, subs))) }
            }
            Tok::Dollar => {
                self.advance()?;
                let e = self.dollar_operand()?;
                // A gramática do gawk aceita `$e++` como alvo (o incremento fica sem efeito útil).
                if matches!(self.tok.tok, Tok::Incr | Tok::Decr) {
                    self.advance()?;
                }
                Ok(Some(LValue::Field(Box::new(e))))
            }
            _ => Ok(None),
        }
    }

    fn call_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(Tok::LParen)?;
        self.skip_newlines()?;
        let mut args = Vec::new();
        if self.is(&Tok::RParen) {
            self.advance()?;
            return Ok(args);
        }
        loop {
            let e = self.exp()?;
            args.push(self.check_expr(e)?);
            if self.is(&Tok::Comma) {
                self.advance()?;
                self.skip_newlines()?;
                continue;
            }
            break;
        }
        self.expect(Tok::RParen)?;
        Ok(args)
    }

    fn primary(&mut self, no_gt: bool) -> PResult<Expr> {
        let _ = no_gt;
        let tok = self.tok.clone();
        match tok.tok {
            Tok::Number(n) => {
                self.advance()?;
                Ok(Expr::Num(n))
            }
            Tok::Str(s) => {
                self.advance()?;
                Ok(Expr::Str(s))
            }
            Tok::Slash | Tok::DivAssign => {
                let r = self.lex.read_regex(&tok).map_err(|e| self.lex_error(e))?;
                self.tok = r;
                let Tok::Regex(body) = self.tok.tok.clone() else { unreachable!() };
                self.advance()?;
                Ok(Expr::Regex(self.regex_id(&body)))
            }
            Tok::TypedRegex(body) => {
                self.advance()?;
                Ok(Expr::TypedRegex(self.regex_id(&body)))
            }
            Tok::Dollar => {
                self.advance()?;
                let e = self.dollar_operand()?;
                Ok(Expr::Field(Box::new(e)))
            }
            Tok::Not | Tok::Minus | Tok::Plus => self.unary(false),
            Tok::Incr | Tok::Decr => {
                let delta = if self.is(&Tok::Incr) { 1.0 } else { -1.0 };
                self.advance()?;
                // O operando do pré-incremento é uma variável (nome ou campo), como na gramática do gawk.
                if !matches!(self.tok.tok, Tok::Name(_) | Tok::Dollar) {
                    return Err(self.error_here());
                }
                let target = self.primary(false)?;
                match target.into_lvalue() {
                    Ok(lv) => Ok(Expr::IncDec(Box::new(lv), true, delta)),
                    Err(_) => Err(self.error_here()),
                }
            }
            Tok::LParen => {
                self.advance()?;
                let first = self.exp()?;
                let first = self.check_expr(first)?;
                if self.is(&Tok::Comma) {
                    let mut items = vec![first];
                    while self.is(&Tok::Comma) {
                        self.advance()?;
                        self.skip_newlines()?;
                        let e = self.exp()?;
                        items.push(self.check_expr(e)?);
                    }
                    self.expect(Tok::RParen)?;
                    // Só vale seguido de `in` ou como lista do print.
                    return Ok(Expr::List(items));
                }
                self.expect(Tok::RParen)?;
                Ok(Expr::Group(Box::new(first)))
            }
            Tok::Name(n) => {
                self.advance()?;
                let var = self.resolve_var(&n, tok.line);
                let subs = self.subscript_groups()?;
                if subs.is_empty() { Ok(Expr::Var(var)) } else { Ok(Expr::Index(var, subs)) }
            }
            Tok::FuncName(n) => {
                let name = self.qualify(&n);
                if let Some(locals) = &self.locals
                    && locals.iter().any(|p| **p == *n) {
                        self.semantic_error(tok.line, &format!("attempt to use non-function `{n}' in function call"));
                    }
                if let Some(&g) = self.st.globals.get(&name)
                    && g < sv::COUNT {
                        self.semantic_error(tok.line, &format!("attempt to use non-function `{name}' in function call"));
                    }
                self.advance()?;
                let args = self.call_args()?;
                let idx = self.func_index(&name);
                self.st.calls.push((idx, args.len(), self.src_idx, tok.line));
                Ok(Expr::Call(idx, args))
            }
            Tok::IndirectName(n) => {
                self.advance()?;
                let var = self.resolve_var(&n, tok.line);
                let args = self.call_args()?;
                Ok(Expr::IndirectCall(var, args))
            }
            Tok::BadName(msg) => {
                self.semantic_error(tok.line, &msg);
                self.advance()?;
                Err(self.error_here())
            }
            Tok::Builtin(name) if !crate::lexer::is_posix_builtin(name) && self.shadowed_builtin(name) => {
                // Extensão do gawk sombreada por parâmetro, variável ou função de outro namespace.
                self.advance()?;
                if let Some(locals) = &self.locals
                    && let Some(i) = locals.iter().position(|p| &**p == name) {
                        return Ok(Expr::Var(Var::Local(i as u32)));
                    }
                let q = self.qualify(name);
                if self.is(&Tok::LParen) && !tok.space_before {
                    let args = self.call_args()?;
                    let idx = self.func_index(&q);
                    self.st.calls.push((idx, args.len(), self.src_idx, tok.line));
                    return Ok(Expr::Call(idx, args));
                }
                let var = self.resolve_var(name, tok.line);
                let subs = self.subscript_groups()?;
                if subs.is_empty() { Ok(Expr::Var(var)) } else { Ok(Expr::Index(var, subs)) }
            }
            Tok::Builtin(name) => {
                self.advance()?;
                let b = Builtin::from_name(name).expect("builtin conhecido");
                if !self.is(&Tok::LParen) {
                    if b == Builtin::Length {
                        return Ok(Expr::Builtin(b, Vec::new()));
                    }
                    return Err(self.error_here());
                }
                self.advance()?;
                self.skip_newlines()?;
                let mut args = Vec::new();
                if !self.is(&Tok::RParen) {
                    loop {
                        let e = self.exp()?;
                        args.push(self.check_expr(e)?);
                        if self.is(&Tok::Comma) {
                            self.advance()?;
                            self.skip_newlines()?;
                            continue;
                        }
                        break;
                    }
                }
                if !self.is(&Tok::RParen) {
                    return Err(self.error_here());
                }
                self.check_builtin_args(b, &args)?;
                self.advance()?;
                Ok(Expr::Builtin(b, args))
            }
            Tok::Getline => {
                self.advance()?;
                let target = self.opt_getline_target()?;
                if self.is(&Tok::Lt) {
                    self.advance()?;
                    // O operando de `<` não tem concatenação (é o `simp_exp` do gawk).
                    let file = self.additive(true)?;
                    let file = self.check_expr(file)?;
                    return Ok(Expr::Getline(GetlineSrc::File(Box::new(file)), target.map(Box::new)));
                }
                Ok(Expr::Getline(GetlineSrc::Main, target.map(Box::new)))
            }
            _ => Err(self.error_here()),
        }
    }

    /// O nome de uma extensão do gawk está sombreado aqui? (parâmetro da função corrente, ou namespace
    /// fora do `awk` em que o nome não é seguido de `(` ou já existe a função `ns::nome`).
    fn shadowed_builtin(&self, name: &str) -> bool {
        if let Some(locals) = &self.locals
            && locals.iter().any(|p| &**p == name) {
                return true;
            }
        if self.st.namespace == "awk" {
            return false;
        }
        let q = format!("{}::{}", self.st.namespace, name);
        if self.st.funcs.contains_key(&q) {
            return true;
        }
        // Sem `(` logo depois, é variável do namespace.
        !self.lex.source().get(self.lex.pos()).is_some_and(|c| *c == b'(') || self.st.globals.contains_key(&q)
    }

    fn check_builtin_args(&mut self, b: Builtin, args: &[Expr]) -> PResult<()> {
        use Builtin::*;
        let n = args.len();
        let (min, max): (usize, usize) = match b {
            Length => (0, 1),
            Substr => (2, 3),
            Index => (2, 2),
            Split => (2, 4),
            Patsplit => (2, 4),
            Sub | Gsub => (2, 3),
            Gensub => (3, 4),
            Match => (2, 3),
            Sprintf => (1, usize::MAX),
            Sin | Cos | Exp | Log | Sqrt | Int => (1, 1),
            Atan2 => (2, 2),
            Rand => (0, 0),
            Srand => (0, 1),
            Tolower | Toupper => (1, 1),
            System => (1, 1),
            Close => (1, 2),
            Fflush => (0, 1),
            Systime => (0, 0),
            Strftime => (0, 3),
            Mktime => (1, 2),
            Asort | Asorti => (1, 3),
            Isarray => (1, 1),
            Typeof => (1, 2),
            Strtonum => (1, 1),
            And | Or | Xor => (2, usize::MAX),
            Lshift | Rshift => (2, 2),
            Compl => (1, 1),
            Bindtextdomain => (1, 2),
            Dcgettext => (1, 3),
            Dcngettext => (3, 5),
            Mkbool => (1, 1),
        };
        if n < min || n > max {
            let name = b.name();
            if matches!(b, And | Or | Xor) && n < 2 {
                let mut e = self.error_here();
                e.msg = format!("\0fatal:{name}: called with less than two arguments");
                return Err(e);
            }
            let mut e = self.error_here();
            e.msg = format!("{n} is invalid as number of arguments for {name}");
            return Err(e);
        }
        Ok(())
    }
}

