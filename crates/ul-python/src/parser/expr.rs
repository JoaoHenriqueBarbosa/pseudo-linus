//! Regras de expressão de `Grammar/python.gram` (3.13), na ordem do arquivo: `expressions`,
//! `expression`, `yield_expr`, `star_expressions`, `named_expression`, os operadores do
//! `disjunction` ao `primary`, `slices`, `atom`, os displays e comprehensions, os argumentos de
//! chamada, os alvos de `for` e o `lambdef`. Cada função cita a regra que implementa.

use super::{decode_string, error_at, is_keyword, number_constant, PResult, ParseError, Parser, Rule, StrValue};
use crate::ast::{
    Arg, Arguments, BoolOp, CmpOp, Comprehension, Constant, Expr, ExprContext, ExprKind as E, Keyword,
    Operator, UnaryOp,
};
use crate::token::TokenType as T;
use crate::tokenizer::Token;

use ExprContext::{Load, Store};

type RuleFn = fn(&mut Parser) -> PResult<Expr>;
/// Posicionais e nomeados de uma chamada.
type CallArgs = (Vec<Expr>, Vec<Keyword>);

impl Parser {
    // -----------------------------------------------------------------------------------------
    // Sequências

    /// `item (',' item)+ [','] | item ',' | item`: tupla sem parênteses (`expressions`,
    /// `star_expressions`).
    fn tuple_of(&mut self, item: RuleFn) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            let first = req!(item(p));
            if !p.at_op(T::Comma)? {
                return Ok(Some(first));
            }
            let mut elts = vec![first];
            while p.eat_op(T::Comma)? {
                match item(p)? {
                    Some(e) => elts.push(e),
                    None => break,
                }
            }
            Ok(Some(p.node(E::Tuple { elts, ctx: Load }, start)))
        })
    }

    /// `','.item+ [',']`.
    fn comma_list(&mut self, item: RuleFn) -> PResult<Vec<Expr>> {
        self.attempt(|p| {
            let mut out = vec![req!(item(p))];
            while p.eat_op(T::Comma)? {
                match item(p)? {
                    Some(e) => out.push(e),
                    None => break,
                }
            }
            Ok(Some(out))
        })
    }

    /// `expressions`.
    pub(super) fn expressions(&mut self) -> PResult<Expr> {
        self.tuple_of(Parser::expression)
    }

    /// `star_expressions`.
    fn star_expressions(&mut self) -> PResult<Expr> {
        self.tuple_of(Parser::star_expression)
    }

    // -----------------------------------------------------------------------------------------
    // expression, yield, starred, named

    /// `expression (memo): disjunction 'if' disjunction 'else' expression | disjunction | lambdef`.
    fn expression(&mut self) -> PResult<Expr> {
        self.memo(Rule::Expression, Parser::expression_raw)
    }

    fn expression_raw(&mut self) -> PResult<Expr> {
        let start = self.mark;
        if let Some(body) = self.disjunction()? {
            let after = self.mark;
            if self.eat_kw("if")?
                && let Some(test) = self.disjunction()?
                    && self.eat_kw("else")?
                        && let Some(orelse) = self.expression()? {
                            let kind = E::IfExp { test: Box::new(test), body: Box::new(body), orelse: Box::new(orelse) };
                            return Ok(Some(self.node(kind, start)));
                        }
            self.mark = after;
            return Ok(Some(body));
        }
        self.lambdef()
    }

    /// `yield_expr: 'yield' 'from' expression | 'yield' [star_expressions]`.
    fn yield_expr(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("yield"));
            let after = p.mark;
            if p.eat_kw("from")? {
                if let Some(value) = p.expression()? {
                    return Ok(Some(p.node(E::YieldFrom { value: Box::new(value) }, start)));
                }
                p.mark = after;
            }
            let value = p.star_expressions()?.map(Box::new);
            Ok(Some(p.node(E::Yield { value }, start)))
        })
    }

    /// `'*' inner` como `Starred` de leitura.
    fn starred_with(&mut self, inner: RuleFn) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Star));
            let value = req!(inner(p));
            Ok(Some(p.node(E::Starred { value: Box::new(value), ctx: Load }, start)))
        })
    }

    /// `star_expression: '*' bitwise_or | expression`.
    fn star_expression(&mut self) -> PResult<Expr> {
        if let Some(e) = self.starred_with(Parser::bitwise_or)? {
            return Ok(Some(e));
        }
        self.expression()
    }

    /// `star_named_expression: '*' bitwise_or | named_expression`.
    fn star_named_expression(&mut self) -> PResult<Expr> {
        if let Some(e) = self.starred_with(Parser::bitwise_or)? {
            return Ok(Some(e));
        }
        self.named_expression()
    }

    /// `starred_expression: '*' expression`.
    fn starred_expression(&mut self) -> PResult<Expr> {
        self.starred_with(Parser::expression)
    }

    /// `assignment_expression: NAME ':=' ~ expression`.
    fn assignment_expression(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            if !(p.is_name_at(0)? && p.peek_kind(1)? == T::ColonEqual) {
                return Ok(None);
            }
            let name = p.advance();
            p.mark += 1;
            let value = req!(p.expression());
            let target = Parser::name_expr(&name, Store);
            Ok(Some(p.node(E::NamedExpr { target: Box::new(target), value: Box::new(value) }, start)))
        })
    }

    /// `expression !':='`.
    fn expression_not_walrus(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let e = req!(p.expression());
            if p.at_op(T::ColonEqual)? {
                return Ok(None);
            }
            Ok(Some(e))
        })
    }

    /// `assignment_expression | expression !':='` (argumentos e genexp).
    fn walrus_or_expression(&mut self) -> PResult<Expr> {
        if let Some(e) = self.assignment_expression()? {
            return Ok(Some(e));
        }
        self.expression_not_walrus()
    }

    /// `named_expression: assignment_expression | expression !':='`.
    fn named_expression(&mut self) -> PResult<Expr> {
        self.memo(Rule::NamedExpression, Parser::walrus_or_expression)
    }

    // -----------------------------------------------------------------------------------------
    // Operadores

    /// `kw sub (kw sub)+ | sub` como `BoolOp`.
    fn bool_chain(&mut self, kw: &str, op: BoolOp, sub: RuleFn) -> PResult<Expr> {
        let start = self.mark;
        let first = req!(sub(self));
        let mut values = vec![first];
        loop {
            let save = self.mark;
            if !self.eat_kw(kw)? {
                break;
            }
            match sub(self)? {
                Some(v) => values.push(v),
                None => {
                    self.mark = save;
                    break;
                }
            }
        }
        if values.len() == 1 {
            return Ok(values.pop());
        }
        Ok(Some(self.node(E::BoolOp { op, values }, start)))
    }

    /// `disjunction (memo): conjunction ('or' conjunction)+ | conjunction`.
    fn disjunction(&mut self) -> PResult<Expr> {
        self.memo(Rule::Disjunction, |p| p.bool_chain("or", BoolOp::Or, Parser::conjunction))
    }

    /// `conjunction (memo): inversion ('and' inversion)+ | inversion`.
    fn conjunction(&mut self) -> PResult<Expr> {
        self.bool_chain("and", BoolOp::And, Parser::inversion)
    }

    /// `inversion: 'not' inversion | comparison`.
    fn inversion(&mut self) -> PResult<Expr> {
        let start = self.mark;
        if self.eat_kw("not")? {
            if let Some(operand) = self.inversion()? {
                return Ok(Some(self.node(E::UnaryOp { op: UnaryOp::Not, operand: Box::new(operand) }, start)));
            }
            self.mark = start;
            return Ok(None);
        }
        self.comparison()
    }

    /// `comparison: bitwise_or compare_op_bitwise_or_pair+ | bitwise_or`.
    fn comparison(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let left = req!(self.bitwise_or());
        let (mut ops, mut comparators) = (Vec::new(), Vec::new());
        loop {
            let save = self.mark;
            let Some(op) = self.compare_op()? else { break };
            match self.bitwise_or()? {
                Some(right) => {
                    ops.push(op);
                    comparators.push(right);
                }
                None => {
                    self.mark = save;
                    break;
                }
            }
        }
        if ops.is_empty() {
            return Ok(Some(left));
        }
        Ok(Some(self.node(E::Compare { left: Box::new(left), ops, comparators }, start)))
    }

    /// Os operadores de `compare_op_bitwise_or_pair` (`eq_bitwise_or` ... `is_bitwise_or`).
    fn compare_op(&mut self) -> Result<Option<CmpOp>, ParseError> {
        let op = match self.peek_kind(0)? {
            T::EqEqual => Some(CmpOp::Eq),
            T::NotEqual => Some(CmpOp::NotEq),
            T::LessEqual => Some(CmpOp::LtE),
            T::Less => Some(CmpOp::Lt),
            T::GreaterEqual => Some(CmpOp::GtE),
            T::Greater => Some(CmpOp::Gt),
            _ => None,
        };
        if op.is_some() {
            self.mark += 1;
            return Ok(op);
        }
        if self.at_kw("not")? && self.kw_at(1, "in")? {
            self.mark += 2;
            return Ok(Some(CmpOp::NotIn));
        }
        if self.eat_kw("in")? {
            return Ok(Some(CmpOp::In));
        }
        if self.eat_kw("is")? {
            return Ok(Some(if self.eat_kw("not")? { CmpOp::IsNot } else { CmpOp::Is }));
        }
        Ok(None)
    }

    /// Regra binária recursiva à esquerda (`rule: rule op sub | sub`) como laço.
    fn binary_chain(&mut self, sub: RuleFn, ops: &[(T, Operator)]) -> PResult<Expr> {
        let start = self.mark;
        let mut left = req!(sub(self));
        loop {
            let save = self.mark;
            let kind = self.peek_kind(0)?;
            let Some(&(_, op)) = ops.iter().find(|(t, _)| *t == kind) else { break };
            self.mark += 1;
            match sub(self)? {
                Some(right) => {
                    left = self.node(E::BinOp { left: Box::new(left), op, right: Box::new(right) }, start);
                }
                None => {
                    self.mark = save;
                    break;
                }
            }
        }
        Ok(Some(left))
    }

    /// `bitwise_or: bitwise_or '|' bitwise_xor | bitwise_xor`.
    fn bitwise_or(&mut self) -> PResult<Expr> {
        self.memo(Rule::BitwiseOr, |p| p.binary_chain(Parser::bitwise_xor, &[(T::Vbar, Operator::BitOr)]))
    }

    /// `bitwise_xor: bitwise_xor '^' bitwise_and | bitwise_and`.
    fn bitwise_xor(&mut self) -> PResult<Expr> {
        self.binary_chain(Parser::bitwise_and, &[(T::Circumflex, Operator::BitXor)])
    }

    /// `bitwise_and: bitwise_and '&' shift_expr | shift_expr`.
    fn bitwise_and(&mut self) -> PResult<Expr> {
        self.binary_chain(Parser::shift_expr, &[(T::Amper, Operator::BitAnd)])
    }

    /// `shift_expr: shift_expr ('<<' | '>>') sum | sum`.
    fn shift_expr(&mut self) -> PResult<Expr> {
        self.binary_chain(Parser::sum, &[(T::LeftShift, Operator::LShift), (T::RightShift, Operator::RShift)])
    }

    /// `sum: sum ('+' | '-') term | term`.
    fn sum(&mut self) -> PResult<Expr> {
        self.binary_chain(Parser::term, &[(T::Plus, Operator::Add), (T::Minus, Operator::Sub)])
    }

    /// `term: term ('*' | '/' | '//' | '%' | '@') factor | factor`.
    fn term(&mut self) -> PResult<Expr> {
        self.binary_chain(
            Parser::factor,
            &[
                (T::Star, Operator::Mult),
                (T::Slash, Operator::Div),
                (T::DoubleSlash, Operator::FloorDiv),
                (T::Percent, Operator::Mod),
                (T::At, Operator::MatMult),
            ],
        )
    }

    /// `factor (memo): '+' factor | '-' factor | '~' factor | power`.
    fn factor(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let op = match self.peek_kind(0)? {
            T::Plus => Some(UnaryOp::UAdd),
            T::Minus => Some(UnaryOp::USub),
            T::Tilde => Some(UnaryOp::Invert),
            _ => None,
        };
        if let Some(op) = op {
            self.mark += 1;
            if let Some(operand) = self.factor()? {
                return Ok(Some(self.node(E::UnaryOp { op, operand: Box::new(operand) }, start)));
            }
            self.mark = start;
            return Ok(None);
        }
        self.power()
    }

    /// `power: await_primary '**' factor | await_primary`.
    fn power(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let base = req!(self.await_primary());
        let save = self.mark;
        if self.eat_op(T::DoubleStar)? {
            if let Some(exp) = self.factor()? {
                let kind = E::BinOp { left: Box::new(base), op: Operator::Pow, right: Box::new(exp) };
                return Ok(Some(self.node(kind, start)));
            }
            self.mark = save;
        }
        Ok(Some(base))
    }

    /// `await_primary (memo): 'await' primary | primary`.
    fn await_primary(&mut self) -> PResult<Expr> {
        let start = self.mark;
        if self.eat_kw("await")? {
            if let Some(value) = self.primary()? {
                return Ok(Some(self.node(E::Await { value: Box::new(value) }, start)));
            }
            self.mark = start;
            return Ok(None);
        }
        self.primary()
    }

    /// `primary: primary '.' NAME | primary genexp | primary '(' [arguments] ')' |
    /// primary '[' slices ']' | atom`.
    fn primary(&mut self) -> PResult<Expr> {
        self.memo(Rule::Primary, Parser::primary_raw)
    }

    fn primary_raw(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let mut value = req!(self.atom());
        loop {
            let save = self.mark;
            match self.peek_kind(0)? {
                T::Dot => {
                    self.mark += 1;
                    let Some(name) = self.eat_name()? else {
                        self.mark = save;
                        break;
                    };
                    value = self.node(E::Attribute { value: Box::new(value), attr: name.text, ctx: Load }, start);
                }
                T::Lpar => {
                    if let Some(genexp) = self.genexp()? {
                        let kind = E::Call { func: Box::new(value), args: vec![genexp], keywords: Vec::new() };
                        value = self.node(kind, start);
                        continue;
                    }
                    self.mark += 1;
                    let Some((args, keywords)) = self.call_arguments()? else {
                        self.mark = save;
                        break;
                    };
                    if !self.eat_op(T::Rpar)? {
                        self.mark = save;
                        break;
                    }
                    value = self.node(E::Call { func: Box::new(value), args, keywords }, start);
                }
                T::Lsqb => {
                    self.mark += 1;
                    let Some(slice) = self.slices()? else {
                        self.mark = save;
                        break;
                    };
                    if !self.eat_op(T::Rsqb)? {
                        self.mark = save;
                        break;
                    }
                    value = self.node(E::Subscript { value: Box::new(value), slice: Box::new(slice), ctx: Load }, start);
                }
                _ => break,
            }
        }
        Ok(Some(value))
    }

    /// `slices: slice !',' | ','.(slice | starred_expression)+ [',']`.
    fn slices(&mut self) -> PResult<Expr> {
        let start = self.mark;
        if let Some(s) = self.slice()?
            && !self.at_op(T::Comma)? {
                return Ok(Some(s));
            }
        self.mark = start;
        let elts = req!(self.comma_list(Parser::slice_or_starred));
        Ok(Some(self.node(E::Tuple { elts, ctx: Load }, start)))
    }

    fn slice_or_starred(&mut self) -> PResult<Expr> {
        if let Some(s) = self.slice()? {
            return Ok(Some(s));
        }
        self.starred_expression()
    }

    /// `slice: [expression] ':' [expression] [':' [expression]] | named_expression`.
    fn slice(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let lower = self.expression()?;
        if self.eat_op(T::Colon)? {
            let upper = self.expression()?;
            let step = if self.eat_op(T::Colon)? { self.expression()? } else { None };
            let kind = E::Slice { lower: lower.map(Box::new), upper: upper.map(Box::new), step: step.map(Box::new) };
            return Ok(Some(self.node(kind, start)));
        }
        self.mark = start;
        self.named_expression()
    }

    // -----------------------------------------------------------------------------------------
    // Átomos

    /// `atom: NAME | 'True' | 'False' | 'None' | strings | NUMBER | (tuple | group | genexp) |
    /// (list | listcomp) | (dict | set | dictcomp | setcomp) | '...'`.
    fn atom(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let tok = self.peek(0)?.clone();
        match tok.kind {
            T::Name => {
                let value = match tok.text.as_str() {
                    "True" => Constant::Bool(true),
                    "False" => Constant::Bool(false),
                    "None" => Constant::None,
                    text if is_keyword(text) => return Ok(None),
                    _ => {
                        self.mark += 1;
                        return Ok(Some(Parser::name_expr(&tok, Load)));
                    }
                };
                self.mark += 1;
                Ok(Some(self.node(E::Constant { value, kind: None }, start)))
            }
            T::String => self.strings(),
            T::Number => {
                let value = number_constant(&tok.text).map_err(|msg| error_at(&tok, msg))?;
                self.mark += 1;
                Ok(Some(self.node(E::Constant { value, kind: None }, start)))
            }
            T::Ellipsis => {
                self.mark += 1;
                Ok(Some(self.node(E::Constant { value: Constant::Ellipsis, kind: None }, start)))
            }
            T::Lpar => self.first_of(&[Parser::tuple as RuleFn, Parser::group, Parser::genexp]),
            T::Lsqb => self.first_of(&[Parser::list as RuleFn, Parser::listcomp]),
            T::Lbrace => {
                self.first_of(&[Parser::dict as RuleFn, Parser::set, Parser::dictcomp, Parser::setcomp])
            }
            _ => Ok(None),
        }
    }

    /// Primeira alternativa que casar.
    fn first_of(&mut self, alts: &[RuleFn]) -> PResult<Expr> {
        for alt in alts {
            if let Some(e) = alt(self)? {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }

    /// `strings (memo): (fstring | string)+`, concatenados (`_PyPegen_concatenate_strings`).
    fn strings(&mut self) -> PResult<Expr> {
        let start = self.mark;
        let mut parts: Vec<Token> = Vec::new();
        while self.peek_kind(0)? == T::String {
            parts.push(self.advance());
        }
        let kind = parts.first().filter(|t| t.text.starts_with('u')).map(|_| "u".to_string());
        let mut text = String::new();
        let mut bytes = Vec::new();
        let mut is_bytes = None;
        for tok in &parts {
            match (decode_string(&tok.text).map_err(|msg| error_at(tok, msg))?, is_bytes) {
                (StrValue::Str(s), None | Some(false)) => {
                    is_bytes = Some(false);
                    text.push_str(&s);
                }
                (StrValue::Bytes(b), None | Some(true)) => {
                    is_bytes = Some(true);
                    bytes.extend(b);
                }
                _ => return Err(error_at(tok, "cannot mix bytes and nonbytes literals")),
            }
        }
        let value = if is_bytes == Some(true) { Constant::Bytes(bytes) } else { Constant::Str(text) };
        Ok(Some(self.node(E::Constant { value, kind }, start)))
    }

    /// `tuple: '(' [star_named_expression ',' [star_named_expressions]] ')'`.
    fn tuple(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lpar));
            let mut elts = Vec::new();
            if !p.at_op(T::Rpar)? {
                elts.push(req!(p.star_named_expression()));
                need!(p.eat_op(T::Comma));
                if let Some(rest) = p.comma_list(Parser::star_named_expression)? {
                    elts.extend(rest);
                }
            }
            need!(p.eat_op(T::Rpar));
            Ok(Some(p.node(E::Tuple { elts, ctx: Load }, start)))
        })
    }

    /// `group: '(' (yield_expr | named_expression) ')'`; devolve a expressão interna.
    fn group(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            need!(p.eat_op(T::Lpar));
            let inner = match p.yield_expr()? {
                Some(e) => e,
                None => req!(p.named_expression()),
            };
            need!(p.eat_op(T::Rpar));
            Ok(Some(inner))
        })
    }

    /// `genexp: '(' (assignment_expression | expression !':=') for_if_clauses ')'`.
    fn genexp(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lpar));
            let elt = req!(p.walrus_or_expression());
            let generators = req!(p.for_if_clauses());
            need!(p.eat_op(T::Rpar));
            Ok(Some(p.node(E::GeneratorExp { elt: Box::new(elt), generators }, start)))
        })
    }

    /// `list: '[' [star_named_expressions] ']'`.
    fn list(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lsqb));
            let elts = p.comma_list(Parser::star_named_expression)?.unwrap_or_default();
            need!(p.eat_op(T::Rsqb));
            Ok(Some(p.node(E::List { elts, ctx: Load }, start)))
        })
    }

    /// `listcomp: '[' named_expression for_if_clauses ']'`.
    fn listcomp(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lsqb));
            let elt = req!(p.named_expression());
            let generators = req!(p.for_if_clauses());
            need!(p.eat_op(T::Rsqb));
            Ok(Some(p.node(E::ListComp { elt: Box::new(elt), generators }, start)))
        })
    }

    /// `set: '{' star_named_expressions '}'`.
    fn set(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lbrace));
            let elts = req!(p.comma_list(Parser::star_named_expression));
            need!(p.eat_op(T::Rbrace));
            Ok(Some(p.node(E::Set { elts }, start)))
        })
    }

    /// `setcomp: '{' named_expression for_if_clauses '}'`.
    fn setcomp(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lbrace));
            let elt = req!(p.named_expression());
            let generators = req!(p.for_if_clauses());
            need!(p.eat_op(T::Rbrace));
            Ok(Some(p.node(E::SetComp { elt: Box::new(elt), generators }, start)))
        })
    }

    /// `dict: '{' [double_starred_kvpairs] '}'`.
    fn dict(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lbrace));
            let (mut keys, mut values) = (Vec::new(), Vec::new());
            while let Some((k, v)) = p.double_starred_kvpair()? {
                keys.push(k);
                values.push(v);
                if !p.eat_op(T::Comma)? {
                    break;
                }
            }
            need!(p.eat_op(T::Rbrace));
            Ok(Some(p.node(E::Dict { keys, values }, start)))
        })
    }

    /// `dictcomp: '{' kvpair for_if_clauses '}'`.
    fn dictcomp(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_op(T::Lbrace));
            let (key, value) = req!(p.kvpair());
            let generators = req!(p.for_if_clauses());
            need!(p.eat_op(T::Rbrace));
            Ok(Some(p.node(E::DictComp { key: Box::new(key), value: Box::new(value), generators }, start)))
        })
    }

    /// `double_starred_kvpair: '**' bitwise_or | kvpair`.
    fn double_starred_kvpair(&mut self) -> PResult<(Option<Expr>, Expr)> {
        self.attempt(|p| {
            if p.eat_op(T::DoubleStar)? {
                let value = req!(p.bitwise_or());
                return Ok(Some((None, value)));
            }
            let (key, value) = req!(p.kvpair());
            Ok(Some((Some(key), value)))
        })
    }

    /// `kvpair: expression ':' expression`.
    fn kvpair(&mut self) -> PResult<(Expr, Expr)> {
        self.attempt(|p| {
            let key = req!(p.expression());
            need!(p.eat_op(T::Colon));
            let value = req!(p.expression());
            Ok(Some((key, value)))
        })
    }

    /// `for_if_clauses: for_if_clause+`.
    fn for_if_clauses(&mut self) -> PResult<Vec<Comprehension>> {
        let mut out = Vec::new();
        while let Some(c) = self.for_if_clause()? {
            out.push(c);
        }
        Ok(if out.is_empty() { None } else { Some(out) })
    }

    /// `for_if_clause: ['async'] 'for' star_targets 'in' ~ disjunction ('if' disjunction)*`.
    fn for_if_clause(&mut self) -> PResult<Comprehension> {
        self.attempt(|p| {
            let is_async = i64::from(p.eat_kw("async")?);
            need!(p.eat_kw("for"));
            let target = req!(p.star_targets());
            need!(p.eat_kw("in"));
            let iter = req!(p.disjunction());
            let mut ifs = Vec::new();
            loop {
                let save = p.mark;
                if !p.eat_kw("if")? {
                    break;
                }
                match p.disjunction()? {
                    Some(e) => ifs.push(e),
                    None => {
                        p.mark = save;
                        break;
                    }
                }
            }
            Ok(Some(Comprehension { target, iter, ifs, is_async }))
        })
    }

    // -----------------------------------------------------------------------------------------
    // Argumentos de chamada

    /// `arguments: args [','] &')'`, com `args` e `kwargs`. Os `*x` vão para `args` na ordem em que
    /// aparecem, como o `_PyPegen_collect_call_seqs`. `None` se a lista não casar.
    fn call_arguments(&mut self) -> Result<Option<CallArgs>, ParseError> {
        self.attempt(|p| {
            let (mut args, mut keywords) = (Vec::new(), Vec::new());
            // 0: posicionais; 1: depois de `nome=`; 2: depois de `**`.
            let mut phase = 0u8;
            loop {
                let item_start = p.mark;
                if p.is_name_at(0)? && p.peek_kind(1)? == T::Equal {
                    let name = p.advance();
                    p.mark += 1;
                    let value = req!(p.expression());
                    keywords.push(Keyword { arg: Some(name.text), value, pos: p.pos_from(item_start) });
                    phase = phase.max(1);
                } else if p.eat_op(T::DoubleStar)? {
                    let value = req!(p.expression());
                    keywords.push(Keyword { arg: None, value, pos: p.pos_from(item_start) });
                    phase = 2;
                } else if p.at_op(T::Star)? {
                    if phase == 2 {
                        return Ok(None);
                    }
                    args.push(req!(p.starred_expression()));
                } else {
                    let Some(e) = p.walrus_or_expression()? else { break };
                    if phase > 0 || p.at_op(T::Equal)? {
                        return Ok(None);
                    }
                    args.push(e);
                }
                if !p.eat_op(T::Comma)? {
                    break;
                }
            }
            need!(p.at_op(T::Rpar));
            Ok(Some((args, keywords)))
        })
    }

    // -----------------------------------------------------------------------------------------
    // Alvos (comprehensions)

    /// `star_targets: star_target !',' | star_target (',' star_target)* [',']`.
    fn star_targets(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            let first = req!(p.star_target());
            if !p.at_op(T::Comma)? {
                return Ok(Some(first));
            }
            let mut elts = vec![first];
            while p.eat_op(T::Comma)? {
                match p.star_target()? {
                    Some(e) => elts.push(e),
                    None => break,
                }
            }
            Ok(Some(p.node(E::Tuple { elts, ctx: Store }, start)))
        })
    }

    /// `star_target: '*' (!'*' star_target) | target_with_star_atom`.
    fn star_target(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            if p.eat_op(T::Star)? {
                if p.at_op(T::Star)? {
                    return Ok(None);
                }
                let inner = req!(p.star_target());
                return Ok(Some(p.node(E::Starred { value: Box::new(inner), ctx: Store }, start)));
            }
            p.target_with_star_atom()
        })
    }

    /// `target_with_star_atom: t_primary '.' NAME !t_lookahead | t_primary '[' slices ']'
    /// !t_lookahead | star_atom`. O `primary` para no primeiro sufixo que não casa, então um
    /// atributo ou subscrição final já satisfaz o `!t_lookahead`.
    fn target_with_star_atom(&mut self) -> PResult<Expr> {
        let start = self.mark;
        if let Some(mut e) = self.primary()?
            && let E::Attribute { ctx, .. } | E::Subscript { ctx, .. } = &mut e.kind {
                *ctx = Store;
                return Ok(Some(e));
            }
        self.mark = start;
        self.star_atom()
    }

    /// `star_atom: NAME | '(' target_with_star_atom ')' | '(' [star_targets_tuple_seq] ')' |
    /// '[' [star_targets_list_seq] ']'`.
    fn star_atom(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            if let Some(tok) = p.eat_name()? {
                return Ok(Some(Parser::name_expr(&tok, Store)));
            }
            if p.eat_op(T::Lpar)? {
                let inner_start = p.mark;
                if let Some(e) = p.target_with_star_atom()?
                    && p.eat_op(T::Rpar)? {
                        return Ok(Some(e));
                    }
                p.mark = inner_start;
                let mut elts = Vec::new();
                if let Some(first) = p.star_target()? {
                    elts.push(first);
                    need!(p.eat_op(T::Comma));
                    while let Some(e) = p.star_target()? {
                        elts.push(e);
                        if !p.eat_op(T::Comma)? {
                            break;
                        }
                    }
                }
                need!(p.eat_op(T::Rpar));
                return Ok(Some(p.node(E::Tuple { elts, ctx: Store }, start)));
            }
            if p.eat_op(T::Lsqb)? {
                let mut elts = Vec::new();
                while let Some(e) = p.star_target()? {
                    elts.push(e);
                    if !p.eat_op(T::Comma)? {
                        break;
                    }
                }
                need!(p.eat_op(T::Rsqb));
                return Ok(Some(p.node(E::List { elts, ctx: Store }, start)));
            }
            Ok(None)
        })
    }

    // -----------------------------------------------------------------------------------------
    // lambda

    /// `lambdef: 'lambda' [lambda_params] ':' expression`.
    fn lambdef(&mut self) -> PResult<Expr> {
        self.attempt(|p| {
            let start = p.mark;
            need!(p.eat_kw("lambda"));
            let args = req!(p.lambda_params());
            need!(p.eat_op(T::Colon));
            let body = req!(p.expression());
            Ok(Some(p.node(E::Lambda { args: Box::new(args), body: Box::new(body) }, start)))
        })
    }

    /// Fim de um parâmetro: `','` ou `&':'`.
    fn lambda_param_end(&mut self) -> Result<bool, ParseError> {
        if self.eat_op(T::Comma)? {
            return Ok(true);
        }
        self.at_op(T::Colon)
    }

    /// `lambda_param_maybe_default: lambda_param default? ',' | lambda_param default? &':'`.
    fn lambda_param(&mut self) -> PResult<(Arg, Option<Expr>)> {
        self.attempt(|p| {
            let Some(tok) = p.eat_name()? else { return Ok(None) };
            let default = if p.eat_op(T::Equal)? { Some(req!(p.expression())) } else { None };
            need!(p.lambda_param_end());
            let arg = Arg { arg: tok.text.clone(), annotation: None, type_comment: None, pos: super::token_pos(&tok) };
            Ok(Some((arg, default)))
        })
    }

    /// `lambda_params`: as cinco alternativas de `lambda_parameters` (com `/`, padrões só no fim
    /// da parte posicional) seguidas de `lambda_star_etc` (`*args` ou `*` puro com ao menos um
    /// só-nomeado, e `**kwargs` por último).
    fn lambda_params(&mut self) -> PResult<Arguments> {
        self.attempt(|p| {
            let mut a = Arguments::default();
            let mut positional: Vec<(Arg, Option<Expr>)> = Vec::new();
            let mut posonly_count = None;
            loop {
                if posonly_count.is_none() && !positional.is_empty() && p.at_op(T::Slash)? {
                    p.mark += 1;
                    need!(p.lambda_param_end());
                    posonly_count = Some(positional.len());
                    continue;
                }
                let Some((arg, default)) = p.lambda_param()? else { break };
                if default.is_none() && positional.iter().any(|(_, d)| d.is_some()) {
                    return Ok(None);
                }
                positional.push((arg, default));
            }
            if p.eat_op(T::Star)? {
                if !p.eat_op(T::Comma)? {
                    let (arg, default) = req!(p.lambda_param());
                    if default.is_some() {
                        return Ok(None);
                    }
                    a.vararg = Some(Box::new(arg));
                } else if !p.is_name_at(0)? {
                    return Ok(None);
                }
                while let Some((arg, default)) = p.lambda_param()? {
                    a.kwonlyargs.push(arg);
                    a.kw_defaults.push(default);
                }
            }
            if p.eat_op(T::DoubleStar)? {
                let (arg, default) = req!(p.lambda_param());
                if default.is_some() {
                    return Ok(None);
                }
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
}
