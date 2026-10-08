//! Erros de sintaxe do CPython 3.13.5: a segunda passada com as regras `invalid_*` de
//! `Grammar/python.gram`, a escolha do erro final do `_PyPegen_run_parser`/`_PyPegen_set_syntax_error`
//! (`Parser/pegen.c` e `Parser/pegen_errors.c`) e o texto que o `python3` imprime no stderr.
//!
//! Fluxo, igual ao C:
//!
//! 1. A primeira passada roda sem as regras `invalid_*`. Erro do tokenizer é definitivo; erro
//!    levantado pelo parser (token forçado `&&':'`, literal inválido) só cede para um erro do
//!    tokenizer mais adiante (`_PyPegen_tokenize_full_source_to_check_for_errors`).
//! 2. Se ela falha sem erro, guarda o último token lido (`p->tokens[p->fill - 1]`), zera marca e
//!    memória e roda de novo com `call_invalid_rules`. As regras `invalid_*` ficam nos mesmos pontos
//!    da gramática: no começo de `expression` e de `named_expression`, no fim de `assignment`,
//!    `arguments`, `block` e `for_stmt`, e no cabeçalho de cada comando composto.
//! 3. Se nenhuma levanta erro, o último token da primeira passada decide: INDENT vira
//!    `IndentationError: unexpected indent`, DEDENT vira `unexpected unindent` e o resto vira
//!    `invalid syntax` naquele token, de novo sujeito a um erro do tokenizer mais adiante.
//!
//! Fica para o compilador (fatia 10) o que o CPython só detecta depois do parser, como
//! `'return' outside function`, `'break' outside loop` e `'await' outside function`, levantados
//! pelo `Python/compile.c` e pelo `Python/symtable.c` com a mesma formatação de `format_syntax_error`.
//!
//! Regras ainda não portadas (caem no `invalid syntax` genérico): as de `match`/`case` (que no C
//! também disparam para chamadas `match(...)` na segunda passada), `invalid_kwarg`, os genexps sem
//! parênteses em chamadas, `invalid_double_starred_kvpairs`, `invalid_import`, `invalid_group`, os
//! parâmetros (`invalid_parameters`) e as de f-string do parser (`invalid_replacement_field`); as
//! mensagens `f-string: ...` do tokenizer já saem pelo caminho do erro de tokenizer.

use super::stmt::aug_operator;
use super::{error_at, token_pos, ErrorKind, PResult, ParseError, Parser, Rule};
use crate::ast::{CmpOp, Constant, Expr, ExprContext::{Load, Store}, ExprKind as E, Pos};
use crate::token::TokenType as T;

/// Palavras-chave suaves do 3.13 (`keyword.softkwlist`).
const SOFT_KEYWORDS: [&str; 4] = ["_", "case", "match", "type"];

/// `TARGETS_TYPE` do `pegen.h` (o `DEL_TARGETS` não tem regra portada ainda).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Targets {
    Star,
    For,
}

/// `_PyPegen_run_parser`: as duas passadas e a escolha do erro.
pub(super) fn run<R>(src: &str, start: fn(&mut Parser) -> PResult<R>) -> Result<R, ParseError> {
    let mut p = Parser::new(src);
    match start(&mut p) {
        Ok(Some(value)) => return Ok(value),
        Ok(None) => {}
        Err(e) => return Err(p.settle(e)),
    }
    let last = p.tokens.len().saturating_sub(1);
    // reset_parser_state_for_error_pass.
    p.mark = 0;
    p.memo.clear();
    p.call_invalid_rules = true;
    if let Err(e) = start(&mut p) {
        return Err(p.settle(e));
    }
    Err(p.generic_error(last))
}

/// `_PyPegen_get_expr_name`.
fn expr_name(e: &Expr) -> &'static str {
    match &e.kind {
        E::Attribute { .. } => "attribute",
        E::Subscript { .. } => "subscript",
        E::Starred { .. } => "starred",
        E::Name { .. } => "name",
        E::List { .. } => "list",
        E::Tuple { .. } => "tuple",
        E::Lambda { .. } => "lambda",
        E::Call { .. } => "function call",
        E::BoolOp { .. } | E::BinOp { .. } | E::UnaryOp { .. } => "expression",
        E::GeneratorExp { .. } => "generator expression",
        E::Yield { .. } | E::YieldFrom { .. } => "yield expression",
        E::Await { .. } => "await expression",
        E::ListComp { .. } => "list comprehension",
        E::SetComp { .. } => "set comprehension",
        E::DictComp { .. } => "dict comprehension",
        E::Dict { .. } => "dict literal",
        E::Set { .. } => "set display",
        E::JoinedStr { .. } | E::FormattedValue { .. } => "f-string expression",
        E::Constant { value, .. } => match value {
            Constant::None => "None",
            Constant::Bool(false) => "False",
            Constant::Bool(true) => "True",
            Constant::Ellipsis => "ellipsis",
            _ => "literal",
        },
        E::Compare { .. } => "comparison",
        E::IfExp { .. } => "conditional expression",
        E::NamedExpr { .. } => "named expression",
        // O C não tem nome para Slice (não chega aqui como alvo).
        E::Slice { .. } => "slice",
    }
}

/// `_PyPegen_get_invalid_target`: a primeira subexpressão que não pode ser alvo.
fn invalid_target(e: &Expr, targets: Targets) -> Option<&Expr> {
    match &e.kind {
        E::List { elts, .. } | E::Tuple { elts, .. } => elts.iter().find_map(|x| invalid_target(x, targets)),
        E::Starred { value, .. } => invalid_target(value, targets),
        E::Compare { left, ops, .. } => {
            if targets == Targets::For {
                // `for x in y` lido como comparação: só o lado esquerdo do primeiro `in` é alvo.
                if matches!(ops.first(), Some(CmpOp::In)) {
                    return invalid_target(left, targets);
                }
                return None;
            }
            Some(e)
        }
        E::Name { .. } | E::Subscript { .. } | E::Attribute { .. } => None,
        _ => Some(e),
    }
}

/// `_PyPegen_check_legacy_stmt`.
fn is_legacy_name(e: &Expr) -> bool {
    matches!(&e.kind, E::Name { id, .. } if id == "print" || id == "exec")
}

impl Parser {
    // -----------------------------------------------------------------------------------------
    // Escolha do erro final

    /// Erro que já existe ao fim de uma passada: o do tokenizer fica; o do parser pode ceder a um
    /// erro do tokenizer mais adiante (`_PyPegen_set_syntax_error` com `PyErr_Occurred`).
    fn settle(&mut self, e: ParseError) -> ParseError {
        if self.tok_failed {
            return e;
        }
        self.check_full_source(e)
    }

    /// `_PyPegen_tokenize_full_source_to_check_for_errors`: tokeniza o resto do fonte; um erro do
    /// tokenizer substitui o atual, e um parêntese nunca fechado só o substitui se abriu numa linha
    /// anterior à do último token lido.
    fn check_full_source(&mut self, e: ParseError) -> ParseError {
        let current_line = self.tokens.last().map_or(1, |t| self.c_lineno(t.kind, t.start.line));
        loop {
            match self.tokenizer.next_token() {
                Ok(tok) if tok.kind == T::Endmarker => return e,
                Ok(_) => {}
                Err(te) => {
                    if te.msg.ends_with(" was never closed") && current_line <= te.line {
                        return e;
                    }
                    return te.into();
                }
            }
        }
    }

    /// Linha que o tokenizer do C dá ao token: o ENDMARKER (e o DEDENT do fim) ficam na última
    /// linha lida, não na seguinte.
    fn c_lineno(&self, kind: T, line: usize) -> usize {
        if matches!(kind, T::Endmarker | T::Dedent) && line > self.real_lines() {
            return line.saturating_sub(1).max(1);
        }
        line
    }

    /// Linhas do fonte, sem contar a vazia depois da nova linha final.
    fn real_lines(&self) -> usize {
        match self.lines.last() {
            Some(last) if last.is_empty() => self.lines.len() - 1,
            _ => self.lines.len(),
        }
    }

    /// Falha da segunda passada sem erro (`_PyPegen_set_syntax_error`).
    fn generic_error(&mut self, last: usize) -> ParseError {
        let Some(tok) = self.tokens.get(last).cloned() else {
            return ParseError {
                kind: ErrorKind::Syntax,
                msg: "error at start before reading any input".to_string(),
                lineno: 1,
                offset: 0,
                end_lineno: 1,
                end_offset: 0,
            };
        };
        match tok.kind {
            T::Indent => return self.raise_no_location(ErrorKind::Indentation, "unexpected indent"),
            T::Dedent => return self.raise_no_location(ErrorKind::Indentation, "unexpected unindent"),
            _ => {}
        }
        let e = if tok.kind == T::Endmarker {
            // RAISE_SYNTAX_ERROR_KNOWN_LOCATION com col_offset -1: coluna 0, sem circunflexo.
            let line = self.c_lineno(tok.kind, tok.start.line);
            ParseError { kind: ErrorKind::Syntax, msg: "invalid syntax".to_string(), lineno: line, offset: 0, end_lineno: line, end_offset: 0 }
        } else {
            error_at(&tok, "invalid syntax")
        };
        self.check_full_source(e)
    }

    // -----------------------------------------------------------------------------------------
    // Localização (as macros RAISE_* do `pegen.h`)

    /// Colunas em caracteres a partir de uma coluna em bytes da linha `line`.
    fn char_col(&self, line: usize, byte_col: usize) -> usize {
        let Some(text) = self.lines.get(line.wrapping_sub(1)) else { return byte_col };
        let chars = text.char_indices().take_while(|&(i, _)| i < byte_col).count();
        chars + byte_col.saturating_sub(text.len())
    }

    /// `RAISE_SYNTAX_ERROR`/`RAISE_INDENTATION_ERROR` sem localização: o último token lido
    /// (`p->tokens[p->fill - 1]`). INDENT, DEDENT e ENDMARKER não têm coluna no C (`col_offset ==
    /// -1`), e a posição vira a do cursor do tokenizer naquela linha.
    fn raise_no_location(&self, kind: ErrorKind, msg: impl Into<String>) -> ParseError {
        let msg = msg.into();
        let Some(tok) = self.tokens.last() else {
            return ParseError { kind, msg, lineno: 1, offset: 0, end_lineno: 1, end_offset: 0 };
        };
        let at_eof = matches!(tok.kind, T::Endmarker | T::Dedent) && tok.start.line > self.real_lines();
        if at_eof {
            let line = self.c_lineno(tok.kind, tok.start.line);
            // O cursor está depois da nova linha final: a linha inteira mais um.
            let offset = self.lines.get(line - 1).map_or(0, |l| l.chars().count()) + 1;
            return ParseError { kind, msg, lineno: line, offset, end_lineno: line, end_offset: 0 };
        }
        if matches!(tok.kind, T::Indent | T::Dedent) {
            // O cursor parou no primeiro caractere depois da indentação.
            let line = tok.end.line;
            return ParseError { kind, msg, lineno: line, offset: tok.end.col, end_lineno: line, end_offset: 0 };
        }
        let mut e = error_at(tok, msg);
        e.kind = kind;
        e
    }

    /// `RAISE_SYNTAX_ERROR_KNOWN_RANGE(a, b, ...)`: do início de `a` ao fim de `b`.
    fn raise_range(&self, a: &Pos, b: &Pos, msg: impl Into<String>) -> ParseError {
        let end_lineno = b.end_lineno.unwrap_or(b.lineno);
        let end_col = b.end_col_offset.unwrap_or(b.col_offset);
        ParseError {
            kind: ErrorKind::Syntax,
            msg: msg.into(),
            lineno: a.lineno,
            offset: self.char_col(a.lineno, a.col_offset) + 1,
            end_lineno,
            end_offset: self.char_col(end_lineno, end_col) + 1,
        }
    }

    /// `RAISE_SYNTAX_ERROR_INVALID_TARGET`.
    fn raise_invalid_target(&self, targets: Targets, e: &Expr) -> ParseError {
        match invalid_target(e, targets) {
            Some(t) => self.raise_range(&t.pos, &t.pos, format!("cannot assign to {}", expr_name(t))),
            None => self.raise_no_location(ErrorKind::Syntax, "invalid syntax"),
        }
    }

    /// Profundidade de parênteses depois de `tokens[end - 1]` (o `level` do token no C).
    fn level_before(&self, end: usize) -> isize {
        self.tokens[..end].iter().fold(0, |level, t| match t.kind {
            T::Lpar | T::Lsqb | T::Lbrace => level + 1,
            T::Rpar | T::Rsqb | T::Rbrace => level - 1,
            _ => level,
        })
    }

    fn is_soft_keyword_at(&mut self, offset: usize) -> Result<bool, ParseError> {
        let tok = self.peek(offset)?;
        Ok(tok.kind == T::Name && !tok.normalized && SOFT_KEYWORDS.contains(&tok.text.as_str()))
    }

    // -----------------------------------------------------------------------------------------
    // Tokens forçados e blocos

    /// Token forçado `&&'x'` (`_PyPegen_expect_forced_token`): vale nas duas passadas e levanta
    /// `expected 'x'` no token encontrado.
    pub(super) fn expect_forced(&mut self, kind: T, text: &str) -> Result<(), ParseError> {
        let tok = self.peek(0)?.clone();
        if tok.kind != kind {
            return Err(error_at(&tok, format!("expected '{text}'")));
        }
        self.mark += 1;
        Ok(())
    }

    /// Fim do cabeçalho de um comando composto, já lido até antes do `:`: as alternativas
    /// `... NEWLINE { "expected ':'" }` (quando `newline_alt`) e `... ':' NEWLINE !INDENT {
    /// "expected an indented block after ..." }` das regras `invalid_<comando>_stmt`.
    pub(super) fn invalid_block_header(&mut self, what: &str, line: usize, newline_alt: bool) -> Result<(), ParseError> {
        if !self.call_invalid_rules {
            return Ok(());
        }
        if newline_alt && self.at_op(T::Newline)? {
            return Err(self.raise_no_location(ErrorKind::Syntax, "expected ':'"));
        }
        if self.at_op(T::Colon)? && self.peek_kind(1)? == T::Newline && self.peek_kind(2)? != T::Indent {
            let msg = format!("expected an indented block after {what} on line {line}");
            return Err(self.raise_no_location(ErrorKind::Indentation, msg));
        }
        Ok(())
    }

    /// `invalid_block: NEWLINE !INDENT { RAISE_INDENTATION_ERROR("expected an indented block") }`.
    pub(super) fn invalid_block(&mut self) -> Result<(), ParseError> {
        if self.at_op(T::Newline)? && self.peek_kind(1)? != T::Indent {
            return Err(self.raise_no_location(ErrorKind::Indentation, "expected an indented block"));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Expressões

    /// `invalid_expression`: as alternativas da vírgula esquecida e do `if` sem `else`.
    pub(super) fn invalid_expression(&mut self) -> Result<(), ParseError> {
        let start = self.mark;
        // !(NAME STRING | SOFT_KEYWORD) a=disjunction b=expression_without_invalid
        let blocked = (self.is_name_at(0)? && self.peek_kind(1)? == T::String) || self.is_soft_keyword_at(0)?;
        if !blocked {
            if let Some(a) = self.memo(Rule::Disjunction)? {
                let saved = self.call_invalid_rules;
                self.call_invalid_rules = false;
                let b = self.expression_raw();
                self.call_invalid_rules = saved;
                if let Some(b) = b?
                    && !is_legacy_name(&a)
                    && self.level_before(self.mark) != 0
                {
                    return Err(self.raise_range(&a.pos, &b.pos, "invalid syntax. Perhaps you forgot a comma?"));
                }
            }
            self.mark = start;
        }
        // a=disjunction 'if' b=disjunction !('else'|':')
        if let Some(a) = self.memo(Rule::Disjunction)?
            && self.eat_kw("if")?
            && let Some(b) = self.memo(Rule::Disjunction)?
            && !self.kw_at(0, "else")?
            && !self.at_op(T::Colon)?
        {
            return Err(self.raise_range(&a.pos, &b.pos, "expected 'else' after 'if' expression"));
        }
        self.mark = start;
        Ok(())
    }

    /// `invalid_legacy_expression: a=NAME !'(' b=star_expressions`, para `print` e `exec`.
    pub(super) fn invalid_legacy_expression(&mut self) -> Result<(), ParseError> {
        let start = self.mark;
        if self.is_name_at(0)? && self.peek_kind(1)? != T::Lpar {
            let a = self.advance();
            if let Some(b) = self.tuple_of(Parser::star_expression, Load)?
                && (a.is_word("print") || a.is_word("exec"))
            {
                let msg = format!("Missing parentheses in call to '{0}'. Did you mean {0}(...)?", a.text);
                return Err(self.raise_range(&token_pos(&a), &b.pos, msg));
            }
        }
        self.mark = start;
        Ok(())
    }

    /// `invalid_named_expression`.
    pub(super) fn invalid_named_expression(&mut self) -> Result<(), ParseError> {
        let start = self.mark;
        // a=expression ':=' expression
        if let Some(a) = self.memo(Rule::Expression)?
            && self.eat_op(T::ColonEqual)?
            && self.memo(Rule::Expression)?.is_some()
        {
            return Err(self.raise_range(&a.pos, &a.pos, format!("cannot use assignment expressions with {}", expr_name(&a))));
        }
        self.mark = start;
        // a=NAME '=' b=bitwise_or !('='|':=')
        if self.is_name_at(0)? && self.peek_kind(1)? == T::Equal {
            let a = self.advance();
            self.mark += 1;
            if let Some(b) = self.binary(0)?
                && !matches!(self.peek_kind(0)?, T::Equal | T::ColonEqual)
            {
                return Err(self.raise_range(
                    &token_pos(&a),
                    &b.pos,
                    "invalid syntax. Maybe you meant '==' or ':=' instead of '='?",
                ));
            }
        }
        self.mark = start;
        // !(list|tuple|genexp|'True'|'None'|'False') a=bitwise_or b='=' bitwise_or !('='|':=')
        let blocked = match self.peek_kind(0)? {
            T::Lsqb => self.list()?.is_some(),
            T::Lpar => self.tuple()?.is_some() || self.genexp()?.is_some(),
            _ => false,
        } || self.kw_at(0, "True")?
            || self.kw_at(0, "None")?
            || self.kw_at(0, "False")?;
        self.mark = start;
        if !blocked
            && let Some(a) = self.binary(0)?
            && self.eat_op(T::Equal)?
            && self.binary(0)?.is_some()
            && !matches!(self.peek_kind(0)?, T::Equal | T::ColonEqual)
        {
            let msg = format!("cannot assign to {} here. Maybe you meant '==' instead of '='?", expr_name(&a));
            return Err(self.raise_range(&a.pos, &a.pos, msg));
        }
        self.mark = start;
        Ok(())
    }

    /// `invalid_arguments`, nas alternativas de argumento sem valor e de posicional depois de
    /// nomeado (`_PyPegen_arguments_parsing_error`). A marca está logo depois do `(`.
    pub(super) fn invalid_arguments(&mut self) -> Result<(), ParseError> {
        let start = self.mark;
        let (mut keyword, mut unpacking) = (false, false);
        loop {
            if self.is_name_at(0)? && self.peek_kind(1)? == T::Equal {
                let a = self.advance();
                let eq = self.advance();
                if matches!(self.peek_kind(0)?, T::Comma | T::Rpar) {
                    return Err(self.raise_range(&token_pos(&a), &token_pos(&eq), "expected argument value expression"));
                }
                if self.memo(Rule::Expression)?.is_none() {
                    break;
                }
                keyword = true;
            } else if self.eat_op(T::DoubleStar)? {
                if self.memo(Rule::Expression)?.is_none() {
                    break;
                }
                keyword = true;
                unpacking = true;
            } else if self.at_op(T::Star)? {
                if self.starred_with(|p| p.memo(Rule::Expression))?.is_none() {
                    break;
                }
            } else {
                if self.walrus_or_expression()?.is_none() {
                    break;
                }
                if keyword {
                    let msg = if unpacking {
                        "positional argument follows keyword argument unpacking"
                    } else {
                        "positional argument follows keyword argument"
                    };
                    return Err(self.raise_no_location(ErrorKind::Syntax, msg));
                }
            }
            if !self.eat_op(T::Comma)? {
                break;
            }
        }
        self.mark = start;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Comandos

    /// `invalid_ann_assign_target: list | tuple | '(' invalid_ann_assign_target ')'`.
    fn invalid_ann_assign_target(&mut self) -> PResult<Expr> {
        match self.peek_kind(0)? {
            T::Lsqb => self.list(),
            T::Lpar => {
                if let Some(t) = self.tuple()? {
                    return Ok(Some(t));
                }
                self.attempt(|p| {
                    p.mark += 1;
                    let inner = req!(p.invalid_ann_assign_target());
                    need!(p.eat_op(T::Rpar));
                    Ok(Some(inner))
                })
            }
            _ => Ok(None),
        }
    }

    /// `(star_targets '=')*`, guloso como o laço do PEG.
    fn star_targets_eq_loop(&mut self) -> Result<(), ParseError> {
        loop {
            let save = self.mark;
            if self.tuple_of(Parser::star_target, Store)?.is_some() && self.eat_op(T::Equal)? {
                continue;
            }
            self.mark = save;
            return Ok(());
        }
    }

    /// `invalid_assignment`, última alternativa de `assignment`.
    pub(super) fn invalid_assignment(&mut self) -> Result<(), ParseError> {
        let start = self.mark;
        // a=invalid_ann_assign_target ':' expression
        if let Some(a) = self.invalid_ann_assign_target()?
            && self.eat_op(T::Colon)?
            && self.memo(Rule::Expression)?.is_some()
        {
            let msg = format!("only single target (not {}) can be annotated", expr_name(&a));
            return Err(self.raise_range(&a.pos, &a.pos, msg));
        }
        self.mark = start;
        // a=star_named_expression ',' star_named_expressions* ':' expression
        if let Some(a) = self.star_named_expression()?
            && self.eat_op(T::Comma)?
        {
            while self.separated(true, Parser::star_named_expression)?.is_some() {}
            if self.eat_op(T::Colon)? && self.memo(Rule::Expression)?.is_some() {
                return Err(self.raise_range(&a.pos, &a.pos, "only single target (not tuple) can be annotated"));
            }
        }
        self.mark = start;
        // a=expression ':' expression
        if let Some(a) = self.memo(Rule::Expression)?
            && self.eat_op(T::Colon)?
            && self.memo(Rule::Expression)?.is_some()
        {
            return Err(self.raise_range(&a.pos, &a.pos, "illegal target for annotation"));
        }
        self.mark = start;
        // (star_targets '=')* a=star_expressions '='
        self.star_targets_eq_loop()?;
        if let Some(a) = self.tuple_of(Parser::star_expression, Load)?
            && self.eat_op(T::Equal)?
        {
            return Err(self.raise_invalid_target(Targets::Star, &a));
        }
        self.mark = start;
        // (star_targets '=')* a=yield_expr '='
        self.star_targets_eq_loop()?;
        if let Some(a) = self.yield_expr()?
            && self.eat_op(T::Equal)?
        {
            return Err(self.raise_range(&a.pos, &a.pos, "assignment to yield expression not possible"));
        }
        self.mark = start;
        // a=star_expressions augassign annotated_rhs
        if let Some(a) = self.tuple_of(Parser::star_expression, Load)?
            && aug_operator(self.peek_kind(0)?).is_some()
        {
            self.mark += 1;
            if self.annotated_rhs()?.is_some() {
                let msg = format!("'{}' is an illegal expression for augmented assignment", expr_name(&a));
                return Err(self.raise_range(&a.pos, &a.pos, msg));
            }
        }
        self.mark = start;
        Ok(())
    }

    /// `invalid_for_target: 'async'? 'for' a=star_expressions`, última alternativa de `for_stmt`.
    pub(super) fn invalid_for_target(&mut self) -> Result<(), ParseError> {
        let start = self.mark;
        self.eat_kw("async")?;
        if self.eat_kw("for")?
            && let Some(a) = self.tuple_of(Parser::star_expression, Load)?
        {
            return Err(self.raise_invalid_target(Targets::For, &a));
        }
        self.mark = start;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Saída no stderr

/// Texto que o `python3` 3.13 imprime para um `SyntaxError` sem traceback (erro de compilação do
/// programa principal). O 3.13 formata pelo `traceback._print_exception_bltin`, isto é, pelo
/// `TracebackException._format_syntax_error` do `Lib/traceback.py`, e só cai no
/// `print_error_text` do C se aquilo falhar.
///
/// `filename` é o que o CPython põe no erro: `<string>` para `-c`, `<stdin>` para a entrada padrão e
/// o caminho como foi passado para um arquivo. `source` é o programa inteiro, de onde sai a linha
/// do erro (o `text` do `SyntaxError`, com a nova linha final).
pub fn format_syntax_error(err: &ParseError, filename: &str, source: &str) -> String {
    let mut out = format!("  File \"{filename}\", line {}\n", err.lineno);
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let real = if normalized.ends_with('\n') { lines.len() - 1 } else { lines.len() };
    if err.lineno >= 1 && err.lineno <= real {
        out.push_str(&source_and_carets(lines[err.lineno - 1], err));
    }
    let name = match err.kind {
        ErrorKind::Syntax => "SyntaxError",
        ErrorKind::Indentation => "IndentationError",
        ErrorKind::Tab => "TabError",
    };
    let msg = if err.msg.is_empty() { "<no detail available>" } else { err.msg.as_str() };
    out.push_str(&format!("{name}: {msg}\n"));
    out
}

/// A linha do fonte e os circunflexos, com o recorte do `_format_syntax_error`: espaço, nova linha
/// e form feed à esquerda saem (tab fica e é mantido no alinhamento), deslocamentos além da linha
/// são presos ao fim dela, faixa vazia vira um caractere e erro de várias linhas vai até o fim da
/// primeira. Coluna à esquerda do texto recortado não imprime circunflexo.
fn source_and_carets(rtext: &str, err: &ParseError) -> String {
    let ltext = rtext.trim_start_matches([' ', '\n', '\x0c']);
    let rlen = rtext.chars().count() as isize;
    let spaces = rlen - ltext.chars().count() as isize;
    // len(self.text), que inclui a nova linha final.
    let text_len = rlen + 1;
    let mut offset = err.offset as isize;
    let mut end_offset = if err.lineno == err.end_lineno {
        if err.end_offset != 0 { err.end_offset as isize } else { offset }
    } else {
        rlen + 1
    };
    if offset > text_len {
        offset = rlen + 1;
    }
    if end_offset > text_len {
        end_offset = rlen + 1;
    }
    if offset >= end_offset {
        end_offset = offset + 1;
    }
    let colno = offset - 1 - spaces;
    let end_colno = end_offset - 1 - spaces;
    let mut out = format!("    {ltext}\n");
    if colno >= 0 {
        let caretspace: String =
            ltext.chars().take(colno as usize).map(|c| if c.is_whitespace() { c } else { ' ' }).collect();
        let carets = "^".repeat((end_colno - colno).max(0) as usize);
        out.push_str(&format!("    {caretspace}{carets}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse_expression, parse_module};

    fn report(src: &str) -> String {
        format_syntax_error(&parse_module(src).unwrap_err(), "<string>", src)
    }

    #[test]
    fn generic_invalid_syntax() {
        assert_eq!(report("x y\n"), "  File \"<string>\", line 1\n    x y\n      ^\nSyntaxError: invalid syntax\n");
        assert_eq!(parse_expression("1 +").unwrap_err().msg, "invalid syntax");
    }

    #[test]
    fn filename_of_stdin_and_file() {
        let src = "x y\n";
        let err = parse_module(src).unwrap_err();
        assert_eq!(
            format_syntax_error(&err, "<stdin>", src),
            "  File \"<stdin>\", line 1\n    x y\n      ^\nSyntaxError: invalid syntax\n"
        );
        assert_eq!(
            format_syntax_error(&err, "/tmp/t.py", src),
            "  File \"/tmp/t.py\", line 1\n    x y\n      ^\nSyntaxError: invalid syntax\n"
        );
    }

    #[test]
    fn forgot_comma() {
        assert_eq!(
            report("print(1 2)"),
            "  File \"<string>\", line 1\n    print(1 2)\n          ^^^\n\
             SyntaxError: invalid syntax. Perhaps you forgot a comma?\n"
        );
        // Fora de parênteses não há sugestão.
        assert_eq!(parse_module("a b").unwrap_err().msg, "invalid syntax");
    }

    #[test]
    fn missing_colon() {
        assert_eq!(
            report("if x\n    pass\n"),
            "  File \"<string>\", line 1\n    if x\n        ^\nSyntaxError: expected ':'\n"
        );
        // Token forçado `&&':'` do def: vale já na primeira passada.
        assert_eq!(
            report("def f()\n    pass\n"),
            "  File \"<string>\", line 1\n    def f()\n           ^\nSyntaxError: expected ':'\n"
        );
    }

    #[test]
    fn parentheses_from_tokenizer() {
        assert_eq!(
            report("print((1)"),
            "  File \"<string>\", line 1\n    print((1)\n         ^\nSyntaxError: '(' was never closed\n"
        );
        assert_eq!(
            report("x = 1)"),
            "  File \"<string>\", line 1\n    x = 1)\n         ^\nSyntaxError: unmatched ')'\n"
        );
        assert_eq!(
            report("(1]"),
            "  File \"<string>\", line 1\n    (1]\n      ^\n\
             SyntaxError: closing parenthesis ']' does not match opening parenthesis '('\n"
        );
    }

    #[test]
    fn invalid_assignment_targets() {
        assert_eq!(
            report("f() = 1"),
            "  File \"<string>\", line 1\n    f() = 1\n    ^^^\n\
             SyntaxError: cannot assign to function call here. Maybe you meant '==' instead of '='?\n"
        );
        assert_eq!(
            report("for 1 in x: pass"),
            "  File \"<string>\", line 1\n    for 1 in x: pass\n        ^\nSyntaxError: cannot assign to literal\n"
        );
        assert_eq!(
            report("if x = 1:\n    pass\n"),
            "  File \"<string>\", line 1\n    if x = 1:\n       ^^^^^\n\
             SyntaxError: invalid syntax. Maybe you meant '==' or ':=' instead of '='?\n"
        );
    }

    #[test]
    fn indentation() {
        assert_eq!(
            report("if x:\npass\n"),
            "  File \"<string>\", line 2\n    pass\n    ^^^^\n\
             IndentationError: expected an indented block after 'if' statement on line 1\n"
        );
        assert_eq!(
            report("if x:\n    a\n  b\n"),
            "  File \"<string>\", line 3\n    b\n     ^\n\
             IndentationError: unindent does not match any outer indentation level\n"
        );
        assert_eq!(
            report("x = 1\n  y = 2\n"),
            "  File \"<string>\", line 2\n    y = 2\nIndentationError: unexpected indent\n"
        );
    }

    #[test]
    fn legacy_print() {
        assert_eq!(
            report("print \"hello\""),
            "  File \"<string>\", line 1\n    print \"hello\"\n    ^^^^^^^^^^^^^\n\
             SyntaxError: Missing parentheses in call to 'print'. Did you mean print(...)?\n"
        );
    }

    #[test]
    fn positional_after_keyword() {
        assert_eq!(
            report("f(a=1, b)"),
            "  File \"<string>\", line 1\n    f(a=1, b)\n            ^\n\
             SyntaxError: positional argument follows keyword argument\n"
        );
    }

    #[test]
    fn tokenizer_string_errors() {
        assert_eq!(
            report("x = 'abc"),
            "  File \"<string>\", line 1\n    x = 'abc\n        ^\n\
             SyntaxError: unterminated string literal (detected at line 1)\n"
        );
        assert_eq!(parse_module("f\"a}\"").unwrap_err().msg, "f-string: single '}' is not allowed");
    }

    #[test]
    fn caret_clipping() {
        // Erro de várias linhas vai até o fim da primeira; deslocamento além da linha é preso.
        let err = ParseError {
            kind: ErrorKind::Syntax,
            msg: "m".to_string(),
            lineno: 1,
            offset: 3,
            end_lineno: 2,
            end_offset: 2,
        };
        assert_eq!(format_syntax_error(&err, "<string>", "  abc\nd\n"), "  File \"<string>\", line 1\n    abc\n    ^^^\nSyntaxError: m\n");
        let err = ParseError { offset: 40, end_lineno: 1, end_offset: 0, ..err };
        assert_eq!(format_syntax_error(&err, "<string>", "\tab\n"), "  File \"<string>\", line 1\n    \tab\n    \t  ^\nSyntaxError: m\n");
    }
}
