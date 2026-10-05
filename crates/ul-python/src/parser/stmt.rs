//! Regras de comando de `Grammar/python.gram` (3.13): `file`, `statements`, `simple_stmts`, os
//! comandos simples (atribuições, `return`, `import`, `raise`, `del`, `assert`, `global`...), os
//! compostos (`if`, `while`, `for`, `with`, `try`, `def`, `class`, `match`), os padrões do `match` e
//! os parâmetros de tipo do 3.12+. As expressões vêm de `expr`.
//!
//! Posições: o fim de um comando é o último token consumido que não seja NEWLINE, INDENT, DEDENT ou
//! ENDMARKER (`_PyPegen_get_last_nonnwhitespace_token`), de modo que um bloco termina no fim do
//! último comando dele. Funções e classes decoradas começam no `def`/`async`/`class`, não no `@`.

use super::{error_at, number_constant, token_pos, PResult, ParseError, Parser};
use crate::ast::{
    Alias, Arg, Arguments, Constant, ExceptHandler, Expr, ExprContext, ExprKind as E, FullPos, MatchCase,
    Mod, Operator, Pattern, PatternKind as P, Pos, Stmt, StmtKind as S, TypeParam, TypeParamKind,
    UnaryOp, WithItem,
};
use crate::token::TokenType as T;

use ExprContext::{Del, Load, Store};

type PatternFn = fn(&mut Parser) -> PResult<Pattern>;

/// `ast.parse(src)`: regra `file: [statements] ENDMARKER`.
pub fn parse_module(src: &str) -> Result<Mod, ParseError> {
    let mut p = Parser::new(src);
    let mut body = Vec::new();
    loop {
        if p.at_op(T::Endmarker)? {
            return Ok(Mod::Module { body, type_ignores: Vec::new() });
        }
        match p.statement()? {
            Some(stmts) => body.extend(stmts),
            None => return Err(p.invalid_syntax()),
        }
    }
}

/// Tokens que o `_PyPegen_get_last_nonnwhitespace_token` pula.
fn is_whitespace_token(kind: T) -> bool {
    matches!(kind, T::Newline | T::Indent | T::Dedent | T::Endmarker)
}

fn full(pos: Pos) -> FullPos {
    FullPos {
        lineno: pos.lineno,
        col_offset: pos.col_offset,
        end_lineno: pos.end_lineno.unwrap_or(pos.lineno),
        end_col_offset: pos.end_col_offset.unwrap_or(pos.col_offset),
    }
}

/// `augassign`.
fn aug_operator(kind: T) -> Option<Operator> {
    Some(match kind {
        T::PlusEqual => Operator::Add,
        T::MinEqual => Operator::Sub,
        T::StarEqual => Operator::Mult,
        T::AtEqual => Operator::MatMult,
        T::SlashEqual => Operator::Div,
        T::PercentEqual => Operator::Mod,
        T::AmperEqual => Operator::BitAnd,
        T::VbarEqual => Operator::BitOr,
        T::CircumflexEqual => Operator::BitXor,
        T::LeftShiftEqual => Operator::LShift,
        T::RightShiftEqual => Operator::RShift,
        T::DoubleStarEqual => Operator::Pow,
        T::DoubleSlashEqual => Operator::FloorDiv,
        _ => return None,
    })
}

impl Parser {
    // -----------------------------------------------------------------------------------------
    // Posições

    /// Posição de comando: de `tokens[start]` até o último token consumido que não é espaço.
    fn spos(&self, start: usize) -> Pos {
        let a = self.tokens[start].start;
        let mut last = self.mark.max(start + 1) - 1;
        while last > start && is_whitespace_token(self.tokens[last].kind) {
            last -= 1;
        }
        let b = self.tokens[last].end;
        Pos { lineno: a.line, col_offset: a.byte_col, end_lineno: Some(b.line), end_col_offset: Some(b.byte_col) }
    }

    fn snode(&self, kind: S, start: usize) -> Stmt {
        Stmt { kind, pos: self.spos(start) }
    }

    fn pnode(&self, kind: P, start: usize) -> Pattern {
        Pattern { kind, pos: full(self.pos_from(start)) }
    }

    // -----------------------------------------------------------------------------------------
    // Sequências de comandos

    /// `statements: statement+`.
    fn statements(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        while let Some(stmts) = self.statement()? {
            out.extend(stmts);
        }
        Ok(if out.is_empty() { None } else { Some(out) })
    }

    /// `statement: compound_stmt | simple_stmts`.
    fn statement(&mut self) -> PResult<Vec<Stmt>> {
        if let Some(s) = self.compound_stmt()? {
            return Ok(Some(vec![s]));
        }
        self.simple_stmts()
    }

    /// `simple_stmts: simple_stmt !';' NEWLINE | ';'.simple_stmt+ [';'] NEWLINE`.
    fn simple_stmts(&mut self) -> PResult<Vec<Stmt>> {
        self.attempt(|p| {
            let mut out = vec![req!(p.simple_stmt())];
            while p.eat_op(T::Semi)? {
                if p.at_op(T::Newline)? {
                    break;
                }
                out.push(req!(p.simple_stmt()));
            }
            need!(p.eat_op(T::Newline));
            Ok(Some(out))
        })
    }

    /// `block: NEWLINE INDENT statements DEDENT | simple_stmts`.
    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.attempt(|p| {
            if p.eat_op(T::Newline)? {
                need!(p.eat_op(T::Indent));
                let body = req!(p.statements());
                need!(p.eat_op(T::Dedent));
                return Ok(Some(body));
            }
            p.simple_stmts()
        })
    }

    /// `else_block: 'else' ':' block` e `finally_block: 'finally' ':' block`.
    fn keyword_block(&mut self, kw: &str) -> PResult<Vec<Stmt>> {
        self.attempt(|p| {
            need!(p.eat_kw(kw));
            need!(p.eat_op(T::Colon));
            p.block()
        })
    }

    /// `[else_block]`.
    fn opt_else(&mut self) -> Result<Vec<Stmt>, ParseError> {
        Ok(self.keyword_block("else")?.unwrap_or_default())
    }

    // -----------------------------------------------------------------------------------------
    // Comandos simples

    /// `simple_stmt`, na ordem das alternativas da gramática.
    fn simple_stmt(&mut self) -> PResult<Stmt> {
        let start = self.mark;
        if let Some(s) = self.assignment()? {
            return Ok(Some(s));
        }
        if self.at_kw("type")?
            && let Some(s) = self.type_alias()?
        {
            return Ok(Some(s));
        }
        if let Some(value) = self.star_expressions()? {
            return Ok(Some(self.snode(S::Expr { value: Box::new(value) }, start)));
        }
        let tok = self.peek(0)?;
        if tok.kind != T::Name {
            return Ok(None);
        }
        let text = tok.text.clone();
        match text.as_str() {
            "return" => self.return_stmt(),
            "import" => self.import_name(),
            "from" => self.import_from(),
            "raise" => self.raise_stmt(),
            "pass" => Ok(Some(self.keyword_stmt(S::Pass))),
            "break" => Ok(Some(self.keyword_stmt(S::Break))),
            "continue" => Ok(Some(self.keyword_stmt(S::Continue))),
            "del" => self.del_stmt(),
            "yield" => self.yield_stmt(),
            "assert" => self.assert_stmt(),
            "global" => self.names_stmt(true),
            "nonlocal" => self.names_stmt(false),
            _ => Ok(None),
        }
    }

    /// `'pass'`, `'break'`, `'continue'`.
    fn keyword_stmt(&mut self, kind: S) -> Stmt {
        let start = self.mark;
        self.mark += 1;
        self.snode(kind, start)
    }

    /// `assignment`: as duas formas anotadas, a atribuição (com alvos múltiplos) e a aumentada.
    fn assignment(&mut self) -> PResult<Stmt> {
        let start = self.mark;
        // NAME ':' expression ['=' annotated_rhs]
        if self.is_name_at(0)? && self.peek_kind(1)? == T::Colon {
            let r = self.attempt(|p| {
                let tok = p.advance();
                let target = Parser::name_expr(&tok, Store);
                p.annotated_assign_rest(target, 1, start)
            })?;
            if r.is_some() {
                return Ok(r);
            }
        }
        // ('(' single_target ')' | single_subscript_attribute_target) ':' expression ['=' annotated_rhs]
        if self.at_op(T::Lpar)? {
            let r = self.attempt(|p| {
                p.mark += 1;
                let target = req!(p.single_target());
                need!(p.eat_op(T::Rpar));
                p.annotated_assign_rest(target, 0, start)
            })?;
            if r.is_some() {
                return Ok(r);
            }
        }
        let r = self.attempt(|p| {
            let target = req!(p.subscript_attribute_target(Store));
            p.annotated_assign_rest(target, 0, start)
        })?;
        if r.is_some() {
            return Ok(r);
        }
        // (star_targets '=')+ (yield_expr | star_expressions) !'='
        let r = self.attempt(|p| {
            let mut targets = Vec::new();
            loop {
                let save = p.mark;
                if let Some(t) = p.star_targets()?
                    && p.eat_op(T::Equal)?
                {
                    targets.push(t);
                    continue;
                }
                p.mark = save;
                break;
            }
            if targets.is_empty() {
                return Ok(None);
            }
            let value = req!(p.annotated_rhs());
            if p.at_op(T::Equal)? {
                return Ok(None);
            }
            Ok(Some(p.snode(S::Assign { targets, value: Box::new(value), type_comment: None }, start)))
        })?;
        if r.is_some() {
            return Ok(r);
        }
        // single_target augassign ~ (yield_expr | star_expressions)
        self.attempt(|p| {
            let target = req!(p.single_target());
            let Some(op) = aug_operator(p.peek_kind(0)?) else { return Ok(None) };
            p.mark += 1;
            let value = req!(p.annotated_rhs());
            Ok(Some(p.snode(S::AugAssign { target: Box::new(target), op, value: Box::new(value) }, start)))
        })
    }

    /// `':' expression ['=' annotated_rhs]` depois do alvo de um `AnnAssign`.
    fn annotated_assign_rest(&mut self, target: Expr, simple: i64, start: usize) -> PResult<Stmt> {
        need!(self.eat_op(T::Colon));
        let annotation = req!(self.expression());
        let value = if self.eat_op(T::Equal)? { Some(Box::new(req!(self.annotated_rhs()))) } else { None };
        let kind = S::AnnAssign { target: Box::new(target), annotation: Box::new(annotation), value, simple };
        Ok(Some(self.snode(kind, start)))
    }

    /// `annotated_rhs: yield_expr | star_expressions`.
    fn annotated_rhs(&mut self) -> PResult<Expr> {
        if let Some(e) = self.yield_expr()? {
            return Ok(Some(e));
        }
        self.star_expressions()
    }

    /// `single_subscript_attribute_target` (e o `del_target` equivalente): `t_primary '.' NAME
    /// !t_lookahead | t_primary '[' slices ']' !t_lookahead`, com o contexto pedido.
    fn subscript_attribute_target(&mut self, ctx: ExprContext) -> PResult<Expr> {
        let start = self.mark;
        if let Some(mut e) = self.primary()?
            && let E::Attribute { ctx: c, .. } | E::Subscript { ctx: c, .. } = &mut e.kind
        {
            *c = ctx;
            return Ok(Some(e));
        }
        self.mark = start;
        Ok(None)
    }

    /// `single_target: single_subscript_attribute_target | NAME | '(' single_target ')'`.
    fn single_target(&mut self) -> PResult<Expr> {
        if let Some(e) = self.subscript_attribute_target(Store)? {
            return Ok(Some(e));
        }
        if let Some(tok) = self.eat_name()? {
            return Ok(Some(Parser::name_expr(&tok, Store)));
        }
        self.attempt(|p| {
            need!(p.eat_op(T::Lpar));
            let e = req!(p.single_target());
            need!(p.eat_op(T::Rpar));
            Ok(Some(e))
        })
    }

    /// `type_alias: "type" NAME [type_params] '=' expression`.
    fn type_alias(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("type"));
            let tok = req!(p.eat_name());
            let type_params = if p.at_op(T::Lsqb)? { req!(p.type_params()) } else { Vec::new() };
            need!(p.eat_op(T::Equal));
            let value = req!(p.expression());
            let name = Box::new(Parser::name_expr(&tok, Store));
            Ok(Some(p.snode(S::TypeAlias { name, type_params, value: Box::new(value) }, start)))
        })
    }

    /// `return_stmt: 'return' [star_expressions]`.
    fn return_stmt(&mut self) -> PResult<Stmt> {
        let start = self.mark;
        need!(self.eat_kw("return"));
        let value = self.star_expressions()?.map(Box::new);
        Ok(Some(self.snode(S::Return { value }, start)))
    }

    /// `raise_stmt: 'raise' expression ['from' expression] | 'raise'`.
    fn raise_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("raise"));
            let Some(exc) = p.expression()? else {
                return Ok(Some(p.snode(S::Raise { exc: None, cause: None }, start)));
            };
            let after = p.mark;
            let mut cause = None;
            if p.eat_kw("from")? {
                match p.expression()? {
                    Some(c) => cause = Some(Box::new(c)),
                    None => p.mark = after,
                }
            }
            Ok(Some(p.snode(S::Raise { exc: Some(Box::new(exc)), cause }, start)))
        })
    }

    /// `global_stmt: 'global' ','.NAME+` e `nonlocal_stmt: 'nonlocal' ','.NAME+`.
    fn names_stmt(&mut self, global: bool) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            p.mark += 1;
            let mut names = vec![req!(p.eat_name()).text];
            while p.eat_op(T::Comma)? {
                names.push(req!(p.eat_name()).text);
            }
            let kind = if global { S::Global { names } } else { S::Nonlocal { names } };
            Ok(Some(p.snode(kind, start)))
        })
    }

    /// `del_stmt: 'del' del_targets &(';' | NEWLINE)`.
    fn del_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("del"));
            let targets = req!(p.del_targets());
            if !matches!(p.peek_kind(0)?, T::Semi | T::Newline) {
                return Ok(None);
            }
            Ok(Some(p.snode(S::Delete { targets }, start)))
        })
    }

    /// `del_targets: ','.del_target+ [',']`.
    fn del_targets(&mut self) -> PResult<Vec<Expr>> {
        let mut out = vec![req!(self.del_target())];
        while self.eat_op(T::Comma)? {
            match self.del_target()? {
                Some(e) => out.push(e),
                None => break,
            }
        }
        Ok(Some(out))
    }

    /// `del_target` e `del_t_atom: NAME | '(' del_target ')' | '(' [del_targets] ')' |
    /// '[' [del_targets] ']'`.
    fn del_target(&mut self) -> PResult<Expr> {
        if let Some(e) = self.subscript_attribute_target(Del)? {
            return Ok(Some(e));
        }
        if let Some(tok) = self.eat_name()? {
            return Ok(Some(Parser::name_expr(&tok, Del)));
        }
        self.attempt(|p| {
            let start = p.mark;
            if p.eat_op(T::Lpar)? {
                let inner = p.mark;
                if let Some(e) = p.del_target()?
                    && p.eat_op(T::Rpar)?
                {
                    return Ok(Some(e));
                }
                p.mark = inner;
                let elts = p.del_targets()?.unwrap_or_default();
                need!(p.eat_op(T::Rpar));
                return Ok(Some(p.node(E::Tuple { elts, ctx: Del }, start)));
            }
            need!(p.eat_op(T::Lsqb));
            let elts = p.del_targets()?.unwrap_or_default();
            need!(p.eat_op(T::Rsqb));
            Ok(Some(p.node(E::List { elts, ctx: Del }, start)))
        })
    }

    /// `yield_stmt: yield_expr`.
    fn yield_stmt(&mut self) -> PResult<Stmt> {
        let start = self.mark;
        let value = req!(self.yield_expr());
        Ok(Some(self.snode(S::Expr { value: Box::new(value) }, start)))
    }

    /// `assert_stmt: 'assert' expression [',' expression]`.
    fn assert_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("assert"));
            let test = req!(p.expression());
            let after = p.mark;
            let mut msg = None;
            if p.eat_op(T::Comma)? {
                match p.expression()? {
                    Some(m) => msg = Some(Box::new(m)),
                    None => p.mark = after,
                }
            }
            Ok(Some(p.snode(S::Assert { test: Box::new(test), msg }, start)))
        })
    }

    // -----------------------------------------------------------------------------------------
    // import

    /// `import_name: 'import' dotted_as_names`.
    fn import_name(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("import"));
            let mut names = vec![req!(p.dotted_as_name())];
            while p.eat_op(T::Comma)? {
                names.push(req!(p.dotted_as_name()));
            }
            Ok(Some(p.snode(S::Import { names }, start)))
        })
    }

    /// `['as' NAME]`.
    fn as_name(&mut self) -> Result<Option<String>, ParseError> {
        if self.at_kw("as")? && self.is_name_at(1)? {
            self.mark += 1;
            return Ok(Some(self.advance().text));
        }
        Ok(None)
    }

    /// `dotted_as_name: dotted_name ['as' NAME]`.
    fn dotted_as_name(&mut self) -> PResult<Alias> {
        let start = self.mark;
        let name = req!(self.dotted_name());
        let asname = self.as_name()?;
        Ok(Some(Alias { name, asname, pos: self.pos_from(start) }))
    }

    /// `dotted_name: dotted_name '.' NAME | NAME`.
    fn dotted_name(&mut self) -> PResult<String> {
        let mut name = req!(self.eat_name()).text;
        while self.at_op(T::Dot)? && self.is_name_at(1)? {
            self.mark += 1;
            name.push('.');
            name.push_str(&self.advance().text);
        }
        Ok(Some(name))
    }

    /// `import_from: 'from' ('.' | '...')* dotted_name 'import' import_from_targets |
    /// 'from' ('.' | '...')+ 'import' import_from_targets`.
    fn import_from(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("from"));
            let mut level = 0i64;
            loop {
                if p.eat_op(T::Dot)? {
                    level += 1;
                } else if p.eat_op(T::Ellipsis)? {
                    level += 3;
                } else {
                    break;
                }
            }
            let module = p.dotted_name()?;
            if module.is_none() && level == 0 {
                return Ok(None);
            }
            need!(p.eat_kw("import"));
            let names = req!(p.import_from_targets());
            Ok(Some(p.snode(S::ImportFrom { module, names, level: Some(level) }, start)))
        })
    }

    /// `import_from_targets: '(' import_from_as_names [','] ')' | import_from_as_names !',' | '*'`.
    fn import_from_targets(&mut self) -> PResult<Vec<Alias>> {
        self.attempt(|p| {
            if p.at_op(T::Star)? {
                let tok = p.advance();
                return Ok(Some(vec![Alias { name: "*".to_string(), asname: None, pos: token_pos(&tok) }]));
            }
            let paren = p.eat_op(T::Lpar)?;
            let mut names = vec![req!(p.import_from_as_name())];
            while p.eat_op(T::Comma)? {
                match p.import_from_as_name()? {
                    Some(a) => names.push(a),
                    None if paren => break,
                    None => return Ok(None),
                }
            }
            if paren {
                need!(p.eat_op(T::Rpar));
            }
            Ok(Some(names))
        })
    }

    /// `import_from_as_name: NAME ['as' NAME]`.
    fn import_from_as_name(&mut self) -> PResult<Alias> {
        let start = self.mark;
        let name = req!(self.eat_name()).text;
        let asname = self.as_name()?;
        Ok(Some(Alias { name, asname, pos: self.pos_from(start) }))
    }

    // -----------------------------------------------------------------------------------------
    // Comandos compostos

    /// `compound_stmt`, escolhido pelo primeiro token.
    fn compound_stmt(&mut self) -> PResult<Stmt> {
        let tok = self.peek(0)?;
        let kind = tok.kind;
        let text = tok.text.clone();
        if kind == T::At {
            return self.decorated();
        }
        if kind != T::Name {
            return Ok(None);
        }
        match text.as_str() {
            "def" => self.function_def(Vec::new()),
            "async" => {
                if self.kw_at(1, "def")? {
                    self.function_def(Vec::new())
                } else if self.kw_at(1, "for")? {
                    self.for_stmt()
                } else if self.kw_at(1, "with")? {
                    self.with_stmt()
                } else {
                    Ok(None)
                }
            }
            "if" => self.if_stmt("if"),
            "class" => self.class_def(Vec::new()),
            "with" => self.with_stmt(),
            "for" => self.for_stmt(),
            "try" => self.try_stmt(),
            "while" => self.while_stmt(),
            "match" => self.match_stmt(),
            _ => Ok(None),
        }
    }

    /// `decorators: ('@' named_expression NEWLINE)+` seguido de `function_def_raw` ou
    /// `class_def_raw`.
    fn decorated(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let mut decorators = Vec::new();
            while p.eat_op(T::At)? {
                decorators.push(req!(p.named_expression()));
                need!(p.eat_op(T::Newline));
            }
            if p.at_kw("class")? { p.class_def(decorators) } else { p.function_def(decorators) }
        })
    }

    /// `if_stmt` e `elif_stmt`: `kw named_expression ':' block (elif_stmt | [else_block])`.
    fn if_stmt(&mut self, kw: &'static str) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw(kw));
            let test = req!(p.named_expression());
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            let orelse = if p.at_kw("elif")? { vec![req!(p.if_stmt("elif"))] } else { p.opt_else()? };
            Ok(Some(p.snode(S::If { test: Box::new(test), body, orelse }, start)))
        })
    }

    /// `while_stmt: 'while' named_expression ':' block [else_block]`.
    fn while_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("while"));
            let test = req!(p.named_expression());
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            let orelse = p.opt_else()?;
            Ok(Some(p.snode(S::While { test: Box::new(test), body, orelse }, start)))
        })
    }

    /// `for_stmt: ['async'] 'for' star_targets 'in' ~ star_expressions ':' [TYPE_COMMENT] block
    /// [else_block]`.
    fn for_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            let is_async = p.eat_kw("async")?;
            need!(p.eat_kw("for"));
            let target = Box::new(req!(p.star_targets()));
            need!(p.eat_kw("in"));
            let iter = Box::new(req!(p.star_expressions()));
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            let orelse = p.opt_else()?;
            let kind = if is_async {
                S::AsyncFor { target, iter, body, orelse, type_comment: None }
            } else {
                S::For { target, iter, body, orelse, type_comment: None }
            };
            Ok(Some(p.snode(kind, start)))
        })
    }

    /// `with_stmt: ['async'] 'with' '(' ','.with_item+ ','? ')' ':' block |
    /// ['async'] 'with' ','.with_item+ ':' [TYPE_COMMENT] block`.
    fn with_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            let is_async = p.eat_kw("async")?;
            need!(p.eat_kw("with"));
            let items = match p.paren_with_items()? {
                Some(items) => items,
                None => req!(p.with_items()),
            };
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            let kind = if is_async {
                S::AsyncWith { items, body, type_comment: None }
            } else {
                S::With { items, body, type_comment: None }
            };
            Ok(Some(p.snode(kind, start)))
        })
    }

    /// `'(' ','.with_item+ ','? ')' &':'`.
    fn paren_with_items(&mut self) -> PResult<Vec<WithItem>> {
        self.attempt(|p| {
            need!(p.eat_op(T::Lpar));
            let mut items = vec![req!(p.with_item())];
            while p.eat_op(T::Comma)? {
                match p.with_item()? {
                    Some(i) => items.push(i),
                    None => break,
                }
            }
            need!(p.eat_op(T::Rpar));
            need!(p.at_op(T::Colon));
            Ok(Some(items))
        })
    }

    /// `','.with_item+`.
    fn with_items(&mut self) -> PResult<Vec<WithItem>> {
        self.attempt(|p| {
            let mut items = vec![req!(p.with_item())];
            while p.eat_op(T::Comma)? {
                items.push(req!(p.with_item()));
            }
            Ok(Some(items))
        })
    }

    /// `with_item: expression 'as' star_target &(',' | ')' | ':') | expression`.
    fn with_item(&mut self) -> PResult<WithItem> {
        let context_expr = req!(self.expression());
        let after = self.mark;
        if self.eat_kw("as")? {
            if let Some(t) = self.star_target()?
                && matches!(self.peek_kind(0)?, T::Comma | T::Rpar | T::Colon)
            {
                return Ok(Some(WithItem { context_expr, optional_vars: Some(Box::new(t)) }));
            }
            self.mark = after;
        }
        Ok(Some(WithItem { context_expr, optional_vars: None }))
    }

    /// `try_stmt: 'try' ':' block finally_block | 'try' ':' block except_block+ [else_block]
    /// [finally_block] | 'try' ':' block except_star_block+ [else_block] [finally_block]`.
    fn try_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("try"));
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            let star = p.at_kw("except")? && p.peek_kind(1)? == T::Star;
            let mut handlers = Vec::new();
            while let Some(h) = p.except_block(star)? {
                handlers.push(h);
            }
            let orelse = if handlers.is_empty() { Vec::new() } else { p.opt_else()? };
            let finalbody = p.keyword_block("finally")?;
            if handlers.is_empty() && finalbody.is_none() {
                return Ok(None);
            }
            let finalbody = finalbody.unwrap_or_default();
            let kind = if star {
                S::TryStar { body, handlers, orelse, finalbody }
            } else {
                S::Try { body, handlers, orelse, finalbody }
            };
            Ok(Some(p.snode(kind, start)))
        })
    }

    /// `except_block: 'except' expression ['as' NAME] ':' block | 'except' ':' block` e
    /// `except_star_block: 'except' '*' expression ['as' NAME] ':' block`.
    fn except_block(&mut self, star: bool) -> PResult<ExceptHandler> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("except"));
            if star {
                need!(p.eat_op(T::Star));
            } else if p.at_op(T::Star)? {
                return Ok(None);
            }
            let (ty, name) = if !star && p.eat_op(T::Colon)? {
                (None, None)
            } else {
                let ty = req!(p.expression());
                let name = if p.eat_kw("as")? { Some(req!(p.eat_name()).text) } else { None };
                need!(p.eat_op(T::Colon));
                (Some(Box::new(ty)), name)
            };
            let body = req!(p.block());
            Ok(Some(ExceptHandler { r#type: ty, name, body, pos: p.spos(start) }))
        })
    }

    // -----------------------------------------------------------------------------------------
    // def e class

    /// `function_def_raw: ['async'] 'def' NAME [type_params] '(' [params] ')' ['->' expression]
    /// ':' [func_type_comment] block`.
    fn function_def(&mut self, decorator_list: Vec<Expr>) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            let is_async = p.eat_kw("async")?;
            need!(p.eat_kw("def"));
            let name = req!(p.eat_name()).text;
            let type_params = if p.at_op(T::Lsqb)? { req!(p.type_params()) } else { Vec::new() };
            need!(p.eat_op(T::Lpar));
            let args = if p.at_op(T::Rpar)? { Arguments::default() } else { req!(p.params()) };
            need!(p.eat_op(T::Rpar));
            let returns = if p.eat_op(T::Rarrow)? { Some(Box::new(req!(p.expression()))) } else { None };
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            let args = Box::new(args);
            let kind = if is_async {
                S::AsyncFunctionDef { name, args, body, decorator_list, returns, type_comment: None, type_params }
            } else {
                S::FunctionDef { name, args, body, decorator_list, returns, type_comment: None, type_params }
            };
            Ok(Some(p.snode(kind, start)))
        })
    }

    /// `class_def_raw: 'class' NAME [type_params] ['(' [arguments] ')'] ':' block`.
    fn class_def(&mut self, decorator_list: Vec<Expr>) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("class"));
            let name = req!(p.eat_name()).text;
            let type_params = if p.at_op(T::Lsqb)? { req!(p.type_params()) } else { Vec::new() };
            let (bases, keywords) = if p.eat_op(T::Lpar)? {
                let args = req!(p.call_arguments());
                need!(p.eat_op(T::Rpar));
                args
            } else {
                (Vec::new(), Vec::new())
            };
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            Ok(Some(p.snode(S::ClassDef { name, bases, keywords, body, decorator_list, type_params }, start)))
        })
    }

    /// Fim de um parâmetro: `','` ou `&')'`.
    fn param_end(&mut self) -> Result<bool, ParseError> {
        if self.eat_op(T::Comma)? {
            return Ok(true);
        }
        self.at_op(T::Rpar)
    }

    /// `param: NAME annotation?`, ou `NAME star_annotation?` no `*args`.
    fn param(&mut self, star_annotation: bool) -> PResult<Arg> {
        self.attempt(|p| {
            let start = p.mark;
            let tok = req!(p.eat_name());
            let annotation = if p.eat_op(T::Colon)? {
                let ann = if star_annotation { req!(p.star_expression()) } else { req!(p.expression()) };
                Some(Box::new(ann))
            } else {
                None
            };
            Ok(Some(Arg { arg: tok.text, annotation, type_comment: None, pos: p.pos_from(start) }))
        })
    }

    /// `param_maybe_default: param default? ',' | param default? &')'`.
    fn param_maybe_default(&mut self) -> PResult<(Arg, Option<Expr>)> {
        self.attempt(|p| {
            let arg = req!(p.param(false));
            let default = if p.eat_op(T::Equal)? { Some(req!(p.expression())) } else { None };
            need!(p.param_end());
            Ok(Some((arg, default)))
        })
    }

    /// `params`: as alternativas de `parameters` (com `/` e padrões só no fim da parte posicional)
    /// seguidas de `star_etc` (`*args`, ou `*` puro com ao menos um só-nomeado, e `**kwargs` por
    /// último).
    fn params(&mut self) -> PResult<Arguments> {
        self.attempt(|p| {
            let mut a = Arguments::default();
            let mut positional: Vec<(Arg, Option<Expr>)> = Vec::new();
            let mut posonly_count = None;
            loop {
                if posonly_count.is_none() && !positional.is_empty() && p.at_op(T::Slash)? {
                    p.mark += 1;
                    need!(p.param_end());
                    posonly_count = Some(positional.len());
                    continue;
                }
                let Some((arg, default)) = p.param_maybe_default()? else { break };
                if default.is_none() && positional.iter().any(|(_, d)| d.is_some()) {
                    return Ok(None);
                }
                positional.push((arg, default));
            }
            if p.eat_op(T::Star)? {
                if p.eat_op(T::Comma)? {
                    if !p.is_name_at(0)? {
                        return Ok(None);
                    }
                } else {
                    let arg = req!(p.param(true));
                    need!(p.param_end());
                    a.vararg = Some(Box::new(arg));
                }
                while let Some((arg, default)) = p.param_maybe_default()? {
                    a.kwonlyargs.push(arg);
                    a.kw_defaults.push(default);
                }
            }
            if p.eat_op(T::DoubleStar)? {
                let arg = req!(p.param(false));
                need!(p.param_end());
                a.kwarg = Some(Box::new(arg));
            }
            let split = posonly_count.unwrap_or(0);
            for (i, (arg, default)) in positional.into_iter().enumerate() {
                if let Some(d) = default {
                    a.defaults.push(d);
                }
                if i < split {
                    a.posonlyargs.push(arg);
                } else {
                    a.args.push(arg);
                }
            }
            Ok(Some(a))
        })
    }

    /// `type_params: '[' type_param_seq ']'`.
    fn type_params(&mut self) -> PResult<Vec<TypeParam>> {
        self.attempt(|p| {
            need!(p.eat_op(T::Lsqb));
            let mut out = vec![req!(p.type_param())];
            while p.eat_op(T::Comma)? {
                match p.type_param()? {
                    Some(t) => out.push(t),
                    None => break,
                }
            }
            need!(p.eat_op(T::Rsqb));
            Ok(Some(out))
        })
    }

    /// `type_param: NAME [type_param_bound] [type_param_default] | '*' NAME
    /// [type_param_starred_default] | '**' NAME [type_param_default]`.
    fn type_param(&mut self) -> PResult<TypeParam> {
        self.attempt(|p| {
            let start = p.mark;
            let kind = if p.eat_op(T::Star)? {
                let name = req!(p.eat_name()).text;
                let default_value =
                    if p.eat_op(T::Equal)? { Some(Box::new(req!(p.star_expression()))) } else { None };
                TypeParamKind::TypeVarTuple { name, default_value }
            } else if p.eat_op(T::DoubleStar)? {
                let name = req!(p.eat_name()).text;
                let default_value = if p.eat_op(T::Equal)? { Some(Box::new(req!(p.expression()))) } else { None };
                TypeParamKind::ParamSpec { name, default_value }
            } else {
                let name = req!(p.eat_name()).text;
                let bound = if p.eat_op(T::Colon)? { Some(Box::new(req!(p.expression()))) } else { None };
                let default_value = if p.eat_op(T::Equal)? { Some(Box::new(req!(p.expression()))) } else { None };
                TypeParamKind::TypeVar { name, bound, default_value }
            };
            Ok(Some(TypeParam { kind, pos: full(p.pos_from(start)) }))
        })
    }

    // -----------------------------------------------------------------------------------------
    // match

    /// `match_stmt: "match" subject_expr ':' NEWLINE INDENT case_block+ DEDENT`.
    fn match_stmt(&mut self) -> PResult<Stmt> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("match"));
            let subject = req!(p.subject_expr());
            need!(p.eat_op(T::Colon));
            need!(p.eat_op(T::Newline));
            need!(p.eat_op(T::Indent));
            let mut cases = Vec::new();
            while let Some(c) = p.case_block()? {
                cases.push(c);
            }
            if cases.is_empty() {
                return Ok(None);
            }
            need!(p.eat_op(T::Dedent));
            Ok(Some(p.snode(S::Match { subject: Box::new(subject), cases }, start)))
        })
    }

    /// `subject_expr: star_named_expression ',' star_named_expressions? | named_expression`.
    fn subject_expr(&mut self) -> PResult<Expr> {
        let tuple = self.attempt(|p| {
            let start = p.mark;
            let first = req!(p.star_named_expression());
            need!(p.eat_op(T::Comma));
            let mut elts = vec![first];
            if let Some(rest) = p.comma_list(Parser::star_named_expression)? {
                elts.extend(rest);
            }
            Ok(Some(p.node(E::Tuple { elts, ctx: Load }, start)))
        })?;
        if tuple.is_some() {
            return Ok(tuple);
        }
        self.named_expression()
    }

    /// `case_block: "case" patterns guard? ':' block`.
    fn case_block(&mut self) -> PResult<MatchCase> {
        self.attempt(|p| {
            need!(p.eat_kw("case"));
            let pattern = req!(p.patterns());
            let guard = if p.eat_kw("if")? { Some(Box::new(req!(p.named_expression()))) } else { None };
            need!(p.eat_op(T::Colon));
            let body = req!(p.block());
            Ok(Some(MatchCase { pattern, guard, body }))
        })
    }

    /// `patterns: open_sequence_pattern | pattern`.
    fn patterns(&mut self) -> PResult<Pattern> {
        let seq = self.attempt(|p| {
            let start = p.mark;
            let patterns = req!(p.open_sequence_pattern());
            Ok(Some(p.pnode(P::MatchSequence { patterns }, start)))
        })?;
        if seq.is_some() {
            return Ok(seq);
        }
        self.pattern()
    }

    /// `open_sequence_pattern: maybe_star_pattern ',' maybe_sequence_pattern?`.
    fn open_sequence_pattern(&mut self) -> PResult<Vec<Pattern>> {
        self.attempt(|p| {
            let first = req!(p.maybe_star_pattern());
            need!(p.eat_op(T::Comma));
            let mut out = vec![first];
            if let Some(rest) = p.maybe_sequence_pattern()? {
                out.extend(rest);
            }
            Ok(Some(out))
        })
    }

    /// `maybe_sequence_pattern: ','.maybe_star_pattern+ ','?`.
    fn maybe_sequence_pattern(&mut self) -> PResult<Vec<Pattern>> {
        let mut out = vec![req!(self.maybe_star_pattern())];
        while self.eat_op(T::Comma)? {
            match self.maybe_star_pattern()? {
                Some(p) => out.push(p),
                None => break,
            }
        }
        Ok(Some(out))
    }

    /// `maybe_star_pattern: star_pattern | pattern`, com `star_pattern: '*' pattern_capture_target
    /// | '*' wildcard_pattern`.
    fn maybe_star_pattern(&mut self) -> PResult<Pattern> {
        let start = self.mark;
        if self.eat_op(T::Star)? {
            if let Some(name) = self.capture_target()? {
                return Ok(Some(self.pnode(P::MatchStar { name: Some(name) }, start)));
            }
            if self.eat_kw("_")? {
                return Ok(Some(self.pnode(P::MatchStar { name: None }, start)));
            }
            self.mark = start;
            return Ok(None);
        }
        self.pattern()
    }

    /// `pattern_capture_target: !"_" NAME !('.' | '(' | '=')`.
    fn capture_target(&mut self) -> Result<Option<String>, ParseError> {
        if self.is_name_at(0)?
            && self.peek(0)?.text != "_"
            && !matches!(self.peek_kind(1)?, T::Dot | T::Lpar | T::Equal)
        {
            return Ok(Some(self.advance().text));
        }
        Ok(None)
    }

    /// `pattern: as_pattern | or_pattern`, com `as_pattern: or_pattern 'as' pattern_capture_target`.
    fn pattern(&mut self) -> PResult<Pattern> {
        let start = self.mark;
        let inner = req!(self.or_pattern());
        let after = self.mark;
        if self.eat_kw("as")? {
            if let Some(name) = self.capture_target()? {
                let kind = P::MatchAs { pattern: Some(Box::new(inner)), name: Some(name) };
                return Ok(Some(self.pnode(kind, start)));
            }
            self.mark = after;
        }
        Ok(Some(inner))
    }

    /// `or_pattern: '|'.closed_pattern+`.
    fn or_pattern(&mut self) -> PResult<Pattern> {
        let start = self.mark;
        let mut patterns = vec![req!(self.closed_pattern())];
        loop {
            let save = self.mark;
            if !self.eat_op(T::Vbar)? {
                break;
            }
            match self.closed_pattern()? {
                Some(p) => patterns.push(p),
                None => {
                    self.mark = save;
                    break;
                }
            }
        }
        if patterns.len() == 1 {
            return Ok(patterns.pop());
        }
        Ok(Some(self.pnode(P::MatchOr { patterns }, start)))
    }

    /// `closed_pattern: literal_pattern | capture_pattern | wildcard_pattern | value_pattern |
    /// group_pattern | sequence_pattern | mapping_pattern | class_pattern`.
    fn closed_pattern(&mut self) -> PResult<Pattern> {
        let start = self.mark;
        if let Some((value, singleton)) = self.literal_expr()? {
            let kind = match singleton {
                Some(value) => P::MatchSingleton { value },
                None => P::MatchValue { value: Box::new(value) },
            };
            return Ok(Some(self.pnode(kind, start)));
        }
        if let Some(name) = self.capture_target()? {
            return Ok(Some(self.pnode(P::MatchAs { pattern: None, name: Some(name) }, start)));
        }
        if self.eat_kw("_")? {
            return Ok(Some(self.pnode(P::MatchAs { pattern: None, name: None }, start)));
        }
        let alts: [PatternFn; 5] = [
            Parser::value_pattern,
            Parser::group_pattern,
            Parser::sequence_pattern,
            Parser::mapping_pattern,
            Parser::class_pattern,
        ];
        for alt in alts {
            if let Some(p) = alt(self)? {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    /// `literal_expr` (e `literal_pattern`): `signed_number !('+' | '-') | complex_number | strings |
    /// 'None' | 'True' | 'False'`. O segundo valor é a constante das três últimas, que viram
    /// `MatchSingleton`.
    fn literal_expr(&mut self) -> PResult<(Expr, Option<Constant>)> {
        let start = self.mark;
        let number = self.attempt(|p| {
            let e = req!(p.signed_number());
            if matches!(p.peek_kind(0)?, T::Plus | T::Minus) {
                return Ok(None);
            }
            Ok(Some(e))
        })?;
        if let Some(e) = number {
            return Ok(Some((e, None)));
        }
        if let Some(e) = self.complex_number()? {
            return Ok(Some((e, None)));
        }
        if self.at_op(T::String)? {
            let e = req!(self.strings());
            return Ok(Some((e, None)));
        }
        let tok = self.peek(0)?;
        if tok.kind != T::Name {
            return Ok(None);
        }
        let value = match tok.text.as_str() {
            "None" => Constant::None,
            "True" => Constant::Bool(true),
            "False" => Constant::Bool(false),
            _ => return Ok(None),
        };
        self.mark += 1;
        let e = self.node(E::Constant { value: value.clone(), kind: None }, start);
        Ok(Some((e, Some(value))))
    }

    /// `signed_number: NUMBER | '-' NUMBER`.
    fn signed_number(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            let negative = p.eat_op(T::Minus)?;
            let num = req!(p.number_token());
            if !negative {
                return Ok(Some(num));
            }
            Ok(Some(p.node(E::UnaryOp { op: UnaryOp::USub, operand: Box::new(num) }, start)))
        })
    }

    /// Um token NUMBER como `Constant`.
    fn number_token(&mut self) -> PResult<Expr> {
        let tok = self.peek(0)?.clone();
        if tok.kind != T::Number {
            return Ok(None);
        }
        let value = number_constant(&tok.text).map_err(|msg| error_at(&tok, msg))?;
        self.mark += 1;
        Ok(Some(Expr { kind: E::Constant { value, kind: None }, pos: token_pos(&tok) }))
    }

    /// `complex_number: signed_real_number ('+' | '-') imaginary_number`.
    fn complex_number(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            let real = req!(p.signed_number());
            let op = match p.peek_kind(0)? {
                T::Plus => Operator::Add,
                T::Minus => Operator::Sub,
                _ => return Ok(None),
            };
            p.mark += 1;
            let imag = req!(p.number_token());
            if !matches!(imag.kind, E::Constant { value: Constant::Complex(..), .. }) {
                return Ok(None);
            }
            Ok(Some(p.node(E::BinOp { left: Box::new(real), op, right: Box::new(imag) }, start)))
        })
    }

    /// `name_or_attr: attr | NAME`, com `attr: name_or_attr '.' NAME`.
    fn name_or_attr(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let tok = req!(self.eat_name());
        let mut e = Parser::name_expr(&tok, Load);
        while self.at_op(T::Dot)? && self.is_name_at(1)? {
            self.mark += 1;
            let attr = self.advance().text;
            e = self.node(E::Attribute { value: Box::new(e), attr, ctx: Load }, start);
        }
        Ok(Some(e))
    }

    /// `attr`: `name_or_attr` com ao menos um `.`.
    fn attr(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let e = req!(p.name_or_attr());
            Ok(if matches!(e.kind, E::Attribute { .. }) { Some(e) } else { None })
        })
    }

    /// `value_pattern: attr !('.' | '(' | '=')`.
    fn value_pattern(&mut self) -> PResult<Pattern> {
        self.attempt(|p| {
            let start = p.mark;
            let value = req!(p.attr());
            if matches!(p.peek_kind(0)?, T::Dot | T::Lpar | T::Equal) {
                return Ok(None);
            }
            Ok(Some(p.pnode(P::MatchValue { value: Box::new(value) }, start)))
        })
    }

    /// `group_pattern: '(' pattern ')'`.
    fn group_pattern(&mut self) -> PResult<Pattern> {
        self.attempt(|p| {
            need!(p.eat_op(T::Lpar));
            let inner = req!(p.pattern());
            need!(p.eat_op(T::Rpar));
            Ok(Some(inner))
        })
    }

    /// `sequence_pattern: '[' maybe_sequence_pattern? ']' | '(' open_sequence_pattern? ')'`.
    fn sequence_pattern(&mut self) -> PResult<Pattern> {
        self.attempt(|p| {
            let start = p.mark;
            let patterns = if p.eat_op(T::Lsqb)? {
                let pats = p.maybe_sequence_pattern()?.unwrap_or_default();
                need!(p.eat_op(T::Rsqb));
                pats
            } else {
                need!(p.eat_op(T::Lpar));
                let pats = p.open_sequence_pattern()?.unwrap_or_default();
                need!(p.eat_op(T::Rpar));
                pats
            };
            Ok(Some(p.pnode(P::MatchSequence { patterns }, start)))
        })
    }

    /// `mapping_pattern: '{' [items_pattern ','] [double_star_pattern] ','? '}'`, com
    /// `key_value_pattern: (literal_expr | attr) ':' pattern` e `double_star_pattern: '**'
    /// pattern_capture_target`.
    fn mapping_pattern(&mut self) -> PResult<Pattern> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lbrace));
            let (mut keys, mut patterns, mut rest) = (Vec::new(), Vec::new(), None);
            loop {
                if p.eat_op(T::DoubleStar)? {
                    rest = Some(req!(p.capture_target()));
                    p.eat_op(T::Comma)?;
                    break;
                }
                let key = match p.literal_expr()? {
                    Some((e, _)) => e,
                    None => match p.attr()? {
                        Some(e) => e,
                        None => break,
                    },
                };
                need!(p.eat_op(T::Colon));
                patterns.push(req!(p.pattern()));
                keys.push(key);
                if !p.eat_op(T::Comma)? {
                    break;
                }
            }
            need!(p.eat_op(T::Rbrace));
            Ok(Some(p.pnode(P::MatchMapping { keys, patterns, rest }, start)))
        })
    }

    /// `class_pattern: name_or_attr '(' [positional_patterns ','?] [keyword_patterns ','?] ')'`.
    fn class_pattern(&mut self) -> PResult<Pattern> {
        self.attempt(|p| {
            let start = p.mark;
            let cls = req!(p.name_or_attr());
            need!(p.eat_op(T::Lpar));
            let (mut patterns, mut kwd_attrs, mut kwd_patterns) = (Vec::new(), Vec::new(), Vec::new());
            while !p.at_op(T::Rpar)? {
                if p.is_name_at(0)? && p.peek_kind(1)? == T::Equal {
                    kwd_attrs.push(p.advance().text);
                    p.mark += 1;
                    kwd_patterns.push(req!(p.pattern()));
                } else {
                    if !kwd_attrs.is_empty() {
                        return Ok(None);
                    }
                    patterns.push(req!(p.pattern()));
                }
                if !p.eat_op(T::Comma)? {
                    break;
                }
            }
            need!(p.eat_op(T::Rpar));
            let kind = P::MatchClass { cls: Box::new(cls), patterns, kwd_attrs, kwd_patterns };
            Ok(Some(p.pnode(kind, start)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::dump;

    /// `ast.dump(ast.parse(src))`.
    fn d(src: &str) -> String {
        dump(&parse_module(src).unwrap())
    }

    fn module(body: &str) -> String {
        format!("Module(body=[{body}], type_ignores=[])")
    }

    fn first(src: &str) -> Stmt {
        match parse_module(src).unwrap() {
            Mod::Module { mut body, .. } => body.remove(0),
            _ => unreachable!(),
        }
    }

    fn pos(p: &Pos) -> (usize, usize, Option<usize>, Option<usize>) {
        (p.lineno, p.col_offset, p.end_lineno, p.end_col_offset)
    }

    #[test]
    fn assignments() {
        assert_eq!(
            d("x = 1\n"),
            module("Assign(targets=[Name(id='x', ctx=Store())], value=Constant(value=1))")
        );
        assert_eq!(
            d("a, *b = c = d"),
            module(
                "Assign(targets=[Tuple(elts=[Name(id='a', ctx=Store()), Starred(value=Name(id='b', \
                 ctx=Store()), ctx=Store())], ctx=Store()), Name(id='c', ctx=Store())], \
                 value=Name(id='d', ctx=Load()))"
            )
        );
        assert_eq!(
            d("x: int = 1\n(y): int\n"),
            module(
                "AnnAssign(target=Name(id='x', ctx=Store()), annotation=Name(id='int', ctx=Load()), \
                 value=Constant(value=1), simple=1), AnnAssign(target=Name(id='y', ctx=Store()), \
                 annotation=Name(id='int', ctx=Load()), simple=0)"
            )
        );
        assert_eq!(
            d("a.b += 1\n"),
            module(
                "AugAssign(target=Attribute(value=Name(id='a', ctx=Load()), attr='b', ctx=Store()), \
                 op=Add(), value=Constant(value=1))"
            )
        );
    }

    #[test]
    fn simple_statements() {
        assert_eq!(
            d("global a, b; nonlocal c\ndel x, y[0]\nassert t, m\nraise E from c\nreturn\n"),
            module(
                "Global(names=['a', 'b']), Nonlocal(names=['c']), Delete(targets=[Name(id='x', \
                 ctx=Del()), Subscript(value=Name(id='y', ctx=Load()), slice=Constant(value=0), \
                 ctx=Del())]), Assert(test=Name(id='t', ctx=Load()), msg=Name(id='m', ctx=Load())), \
                 Raise(exc=Name(id='E', ctx=Load()), cause=Name(id='c', ctx=Load())), Return()"
            )
        );
        assert_eq!(
            d("match = 1\nmatch(x)\ntype = 2\n"),
            module(
                "Assign(targets=[Name(id='match', ctx=Store())], value=Constant(value=1)), \
                 Expr(value=Call(func=Name(id='match', ctx=Load()), args=[Name(id='x', ctx=Load())], \
                 keywords=[])), Assign(targets=[Name(id='type', ctx=Store())], value=Constant(value=2))"
            )
        );
    }

    #[test]
    fn imports() {
        assert_eq!(
            d("import a.b as c, d\nfrom ..m import (x as y, z,)\nfrom . import *\n"),
            module(
                "Import(names=[alias(name='a.b', asname='c'), alias(name='d')]), \
                 ImportFrom(module='m', names=[alias(name='x', asname='y'), alias(name='z')], level=2), \
                 ImportFrom(names=[alias(name='*')], level=1)"
            )
        );
    }

    #[test]
    fn if_elif_else_and_loops() {
        assert_eq!(
            d("if a:\n    pass\nelif b:\n    pass\nelse:\n    x\n"),
            module(
                "If(test=Name(id='a', ctx=Load()), body=[Pass()], orelse=[If(test=Name(id='b', \
                 ctx=Load()), body=[Pass()], orelse=[Expr(value=Name(id='x', ctx=Load()))])])"
            )
        );
        assert_eq!(
            d("for i in x:\n    break\nelse:\n    continue\nwhile t: pass\n"),
            module(
                "For(target=Name(id='i', ctx=Store()), iter=Name(id='x', ctx=Load()), body=[Break()], \
                 orelse=[Continue()]), While(test=Name(id='t', ctx=Load()), body=[Pass()], orelse=[])"
            )
        );
    }

    #[test]
    fn try_and_with() {
        assert_eq!(
            d("try:\n    pass\nexcept E as e:\n    pass\nelse:\n    pass\nfinally:\n    pass\n"),
            module(
                "Try(body=[Pass()], handlers=[ExceptHandler(type=Name(id='E', ctx=Load()), name='e', \
                 body=[Pass()])], orelse=[Pass()], finalbody=[Pass()])"
            )
        );
        assert_eq!(
            d("try:\n    pass\nexcept* E:\n    pass\n"),
            module(
                "TryStar(body=[Pass()], handlers=[ExceptHandler(type=Name(id='E', ctx=Load()), \
                 body=[Pass()])], orelse=[], finalbody=[])"
            )
        );
        assert_eq!(
            d("with (a as b, c):\n    pass\n"),
            module(
                "With(items=[withitem(context_expr=Name(id='a', ctx=Load()), optional_vars=Name(id='b', \
                 ctx=Store())), withitem(context_expr=Name(id='c', ctx=Load()))], body=[Pass()])"
            )
        );
    }

    #[test]
    fn function_with_all_parameter_kinds() {
        assert_eq!(
            d("def f(a, /, b: int = 1, *c, d, e=2, **g) -> r: pass\n"),
            module(
                "FunctionDef(name='f', args=arguments(posonlyargs=[arg(arg='a')], args=[arg(arg='b', \
                 annotation=Name(id='int', ctx=Load()))], vararg=arg(arg='c'), kwonlyargs=[arg(arg='d'), \
                 arg(arg='e')], kw_defaults=[None, Constant(value=2)], kwarg=arg(arg='g'), \
                 defaults=[Constant(value=1)]), body=[Pass()], decorator_list=[], \
                 returns=Name(id='r', ctx=Load()), type_params=[])"
            )
        );
    }

    #[test]
    fn decorated_class_and_async() {
        assert_eq!(
            d("@dec\nclass C(B, metaclass=M):\n    x = 1\n"),
            module(
                "ClassDef(name='C', bases=[Name(id='B', ctx=Load())], keywords=[keyword(arg='metaclass', \
                 value=Name(id='M', ctx=Load()))], body=[Assign(targets=[Name(id='x', ctx=Store())], \
                 value=Constant(value=1))], decorator_list=[Name(id='dec', ctx=Load())], type_params=[])"
            )
        );
        assert_eq!(
            d("async def f():\n    async for x in y:\n        pass\n    async with a:\n        pass\n"),
            module(
                "AsyncFunctionDef(name='f', args=arguments(posonlyargs=[], args=[], kwonlyargs=[], \
                 kw_defaults=[], defaults=[]), body=[AsyncFor(target=Name(id='x', ctx=Store()), \
                 iter=Name(id='y', ctx=Load()), body=[Pass()], orelse=[]), \
                 AsyncWith(items=[withitem(context_expr=Name(id='a', ctx=Load()))], body=[Pass()])], \
                 decorator_list=[], type_params=[])"
            )
        );
    }

    #[test]
    fn match_statement() {
        let src = "match p:\n    case [1, *rest] | {\"k\": _, **kw} if x:\n        pass\n    \
                   case Point(0, y=-1):\n        pass\n    case _:\n        pass\n";
        assert_eq!(
            d(src),
            module(
                "Match(subject=Name(id='p', ctx=Load()), cases=[match_case(pattern=MatchOr(\
                 patterns=[MatchSequence(patterns=[MatchValue(value=Constant(value=1)), \
                 MatchStar(name='rest')]), MatchMapping(keys=[Constant(value='k')], \
                 patterns=[MatchAs()], rest='kw')]), guard=Name(id='x', ctx=Load()), body=[Pass()]), \
                 match_case(pattern=MatchClass(cls=Name(id='Point', ctx=Load()), \
                 patterns=[MatchValue(value=Constant(value=0))], kwd_attrs=['y'], \
                 kwd_patterns=[MatchValue(value=UnaryOp(op=USub(), operand=Constant(value=1)))]), \
                 body=[Pass()]), match_case(pattern=MatchAs(), body=[Pass()])])"
            )
        );
    }

    #[test]
    fn type_alias_and_params() {
        assert_eq!(
            d("type L[T: int, *Ts, **P] = list[T]\n"),
            module(
                "TypeAlias(name=Name(id='L', ctx=Store()), type_params=[TypeVar(name='T', \
                 bound=Name(id='int', ctx=Load())), TypeVarTuple(name='Ts'), ParamSpec(name='P')], \
                 value=Subscript(value=Name(id='list', ctx=Load()), slice=Name(id='T', ctx=Load()), \
                 ctx=Load()))"
            )
        );
    }

    #[test]
    fn positions_like_cpython() {
        let def = first("@d\ndef f(a: int):\n    pass\n");
        assert_eq!(pos(&def.pos), (2, 0, Some(3), Some(8)));
        let S::FunctionDef { args, .. } = &def.kind else { panic!("esperava FunctionDef") };
        assert_eq!(pos(&args.args[0].pos), (2, 6, Some(2), Some(12)));

        assert_eq!(pos(&first("if a:\n    pass\n\n").pos), (1, 0, Some(2), Some(8)));

        let Mod::Module { body, .. } = parse_module("x = 1; y = 2\n").unwrap() else { unreachable!() };
        assert_eq!(pos(&body[1].pos), (1, 7, Some(1), Some(12)));
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(parse_module("x = = 1\n").unwrap_err().msg, "invalid syntax");
        assert_eq!(parse_module("if x\n    pass\n").unwrap_err().msg, "invalid syntax");
    }
}
