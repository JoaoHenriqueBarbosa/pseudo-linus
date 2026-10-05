//! Parser PEG do CPython 3.13 (`Grammar/python.gram`), escrito à mão, com memoização por
//! (regra, posição) como o gerador do `Parser/parser.c` faz para as regras marcadas `(memo)` e para
//! as recursivas à esquerda.
//!
//! A fatia 6 de `docs/python3-port.md` cobre as expressões (`parse_expression` equivale a
//! `ast.parse(src, mode='eval').body`) e a fatia 7 os comandos (`parse_module` equivale a
//! `ast.parse(src)`, em `stmt`). As regras recursivas à esquerda (`bitwise_or`, `sum`, `term`,
//! `primary` e as demais) viram laços que constroem a árvore associando à esquerda, o mesmo resultado
//! do "crescimento da semente" do CPython. As regras `invalid_*` (mensagens específicas de erro) são a
//! fatia 8; até lá toda falha vira `invalid syntax` no token mais distante que o parser examinou, que
//! é onde o CPython põe o erro genérico.
//!
//! Posições: como a macro `EXTRA` do CPython, o início de um nó é o primeiro token da regra e o fim é
//! o último token consumido; `col_offset` conta bytes UTF-8.

use std::collections::HashMap;
use std::fmt;

use crate::ast::{Constant, Expr, ExprContext, ExprKind, Pos};
use crate::token::TokenType;
use crate::tokenizer::{self, Mode, Token, TokenizeError, Tokenizer};

/// Resultado de uma regra: `Ok(None)` é falha da alternativa (o parser tenta outra), `Err` é erro
/// definitivo (tokenizer, literal inválido).
type PResult<T> = Result<Option<T>, ParseError>;

/// Desembrulha o resultado de uma sub-regra; na falha, a regra atual falha também.
macro_rules! req {
    ($e:expr) => {
        match $e? {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// Exige que um teste (`Result<bool, _>`) seja verdadeiro; senão a regra atual falha.
macro_rules! need {
    ($e:expr) => {
        if !($e)? {
            return Ok(None);
        }
    };
}

mod expr;
mod stmt;

pub use stmt::parse_module;

/// Palavras-chave rígidas do 3.13 (`keyword.kwlist`); as suaves (`match`, `case`, `type`, `_`)
/// continuam sendo nomes.
const KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
    "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
    "yield",
];

fn is_keyword(text: &str) -> bool {
    KEYWORDS.contains(&text)
}

/// Classe da exceção do erro de sintaxe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Syntax,
    Indentation,
    Tab,
}

/// Erro de sintaxe com a localização que o `SyntaxError` recebe (`offset` em caracteres, a partir
/// de 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ErrorKind,
    pub msg: String,
    pub lineno: usize,
    pub offset: usize,
    pub end_lineno: usize,
    pub end_offset: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl From<TokenizeError> for ParseError {
    fn from(e: TokenizeError) -> ParseError {
        let kind = match e.kind {
            tokenizer::ErrorKind::Indentation => ErrorKind::Indentation,
            tokenizer::ErrorKind::Tab => ErrorKind::Tab,
            tokenizer::ErrorKind::Syntax => ErrorKind::Syntax,
        };
        ParseError {
            kind,
            msg: e.msg,
            lineno: e.line,
            offset: e.offset,
            end_lineno: e.end_line,
            end_offset: e.end_offset,
        }
    }
}

/// `SyntaxError` localizado num token.
fn error_at(tok: &Token, msg: impl Into<String>) -> ParseError {
    ParseError {
        kind: ErrorKind::Syntax,
        msg: msg.into(),
        lineno: tok.start.line,
        offset: tok.start.col + 1,
        end_lineno: tok.end.line,
        end_offset: tok.end.col + 1,
    }
}

/// Regras memoizadas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Rule {
    Expression,
    NamedExpression,
    Disjunction,
    BitwiseOr,
    Primary,
}

/// Estado do parser: tokens lidos sob demanda (como o `_PyPegen_fill_token`, de modo que um erro de
/// sintaxe anterior vence um erro do tokenizer mais adiante), a marca atual e a memória.
struct Parser {
    tokenizer: Tokenizer,
    tokens: Vec<Token>,
    mark: usize,
    /// Maior índice de token examinado, onde cai o `invalid syntax` genérico.
    furthest: usize,
    memo: HashMap<(Rule, usize), Option<(Expr, usize)>>,
}

impl Parser {
    fn new(src: &str) -> Parser {
        Parser {
            tokenizer: Tokenizer::new(src, Mode::Parser),
            tokens: Vec::new(),
            mark: 0,
            furthest: 0,
            memo: HashMap::new(),
        }
    }

    fn fill(&mut self, idx: usize) -> Result<(), ParseError> {
        // Depois do ENDMARKER o tokenizer devolve ENDMARKER de novo, então o laço sempre termina.
        while self.tokens.len() <= idx {
            let tok = self.tokenizer.next_token()?;
            self.tokens.push(tok);
        }
        self.furthest = self.furthest.max(idx);
        Ok(())
    }

    fn peek(&mut self, offset: usize) -> Result<&Token, ParseError> {
        let idx = self.mark + offset;
        self.fill(idx)?;
        Ok(&self.tokens[idx])
    }

    fn peek_kind(&mut self, offset: usize) -> Result<TokenType, ParseError> {
        Ok(self.peek(offset)?.kind)
    }

    fn at_op(&mut self, kind: TokenType) -> Result<bool, ParseError> {
        Ok(self.peek_kind(0)? == kind)
    }

    fn eat_op(&mut self, kind: TokenType) -> Result<bool, ParseError> {
        let found = self.at_op(kind)?;
        if found {
            self.mark += 1;
        }
        Ok(found)
    }

    fn kw_at(&mut self, offset: usize, kw: &str) -> Result<bool, ParseError> {
        let tok = self.peek(offset)?;
        Ok(tok.kind == TokenType::Name && tok.text == kw)
    }

    fn at_kw(&mut self, kw: &str) -> Result<bool, ParseError> {
        self.kw_at(0, kw)
    }

    fn eat_kw(&mut self, kw: &str) -> Result<bool, ParseError> {
        let found = self.at_kw(kw)?;
        if found {
            self.mark += 1;
        }
        Ok(found)
    }

    /// `NAME` que não é palavra-chave.
    fn is_name_at(&mut self, offset: usize) -> Result<bool, ParseError> {
        let tok = self.peek(offset)?;
        Ok(tok.kind == TokenType::Name && !is_keyword(&tok.text))
    }

    fn eat_name(&mut self) -> Result<Option<Token>, ParseError> {
        if self.is_name_at(0)? {
            Ok(Some(self.advance()))
        } else {
            Ok(None)
        }
    }

    /// Consome o token atual, que já precisa ter sido examinado.
    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.mark].clone();
        self.mark += 1;
        tok
    }

    /// Posição de `tokens[start]` até o último token consumido.
    fn pos_from(&self, start: usize) -> Pos {
        let a = self.tokens[start].start;
        let b = self.tokens[self.mark.max(start + 1) - 1].end;
        Pos { lineno: a.line, col_offset: a.byte_col, end_lineno: Some(b.line), end_col_offset: Some(b.byte_col) }
    }

    fn node(&self, kind: ExprKind, start: usize) -> Expr {
        Expr { kind, pos: self.pos_from(start) }
    }

    fn name_expr(tok: &Token, ctx: ExprContext) -> Expr {
        Expr { kind: ExprKind::Name { id: tok.text.clone(), ctx }, pos: token_pos(tok) }
    }

    /// Roda uma alternativa e devolve a marca ao ponto de partida se ela falhar.
    fn attempt<R>(&mut self, f: impl FnOnce(&mut Parser) -> PResult<R>) -> PResult<R> {
        let start = self.mark;
        let result = f(self)?;
        if result.is_none() {
            self.mark = start;
        }
        Ok(result)
    }

    /// Regra memoizada: o resultado (e a marca final) fica guardado por (regra, posição).
    fn memo(&mut self, rule: Rule, f: fn(&mut Parser) -> PResult<Expr>) -> PResult<Expr> {
        let start = self.mark;
        if let Some(entry) = self.memo.get(&(rule, start)) {
            return Ok(match entry {
                Some((expr, end)) => {
                    let expr = expr.clone();
                    self.mark = *end;
                    Some(expr)
                }
                None => None,
            });
        }
        let result = f(self)?;
        if result.is_none() {
            self.mark = start;
        }
        self.memo.insert((rule, start), result.as_ref().map(|e| (e.clone(), self.mark)));
        Ok(result)
    }

    fn invalid_syntax(&self) -> ParseError {
        let idx = self.furthest.min(self.tokens.len().saturating_sub(1));
        match self.tokens.get(idx) {
            Some(tok) => error_at(tok, "invalid syntax"),
            None => ParseError {
                kind: ErrorKind::Syntax,
                msg: "invalid syntax".to_string(),
                lineno: 1,
                offset: 1,
                end_lineno: 1,
                end_offset: 1,
            },
        }
    }
}

fn token_pos(tok: &Token) -> Pos {
    Pos {
        lineno: tok.start.line,
        col_offset: tok.start.byte_col,
        end_lineno: Some(tok.end.line),
        end_col_offset: Some(tok.end.byte_col),
    }
}

/// `ast.parse(src, mode='eval').body`: regra `eval: expressions NEWLINE* ENDMARKER`.
pub fn parse_expression(src: &str) -> Result<Expr, ParseError> {
    let mut p = Parser::new(src);
    if let Some(expr) = p.expressions()? {
        while p.eat_op(TokenType::Newline)? {}
        if p.at_op(TokenType::Endmarker)? {
            return Ok(expr);
        }
    }
    Err(p.invalid_syntax())
}

// ---------------------------------------------------------------------------------------------
// Literais

/// `_PY_LONG_MAX_STR_DIGITS_THRESHOLD`: limite padrão de dígitos na conversão decimal de `int`.
const MAX_STR_DIGITS: usize = 4300;

/// Valor de um token NUMBER (`parsenumber` do `Parser/pegen.c`).
fn number_constant(text: &str) -> Result<Constant, String> {
    let clean: String = text.chars().filter(|&c| c != '_').collect::<String>().to_ascii_lowercase();
    if let Some(imag) = clean.strip_suffix('j') {
        return Ok(Constant::Complex(0.0, parse_float(imag)?));
    }
    let radix = match clean.get(..2) {
        Some("0x") => 16,
        Some("0o") => 8,
        Some("0b") => 2,
        _ => 0,
    };
    if radix != 0 {
        return radix_to_decimal(&clean[2..], radix).map(Constant::Int);
    }
    if clean.contains(['.', 'e']) {
        return Ok(Constant::Float(parse_float(&clean)?));
    }
    if clean.len() > MAX_STR_DIGITS {
        return Err(format!(
            "Exceeds the limit ({MAX_STR_DIGITS} digits) for integer string conversion: value has {} \
             digits; use sys.set_int_max_str_digits() to increase the limit - Consider hexadecimal for \
             huge integer literals to avoid decimal conversions",
            clean.len()
        ));
    }
    let digits = clean.trim_start_matches('0');
    Ok(Constant::Int(if digits.is_empty() { "0" } else { digits }.to_string()))
}

fn parse_float(text: &str) -> Result<f64, String> {
    text.parse::<f64>().map_err(|_| format!("invalid float literal {text:?}"))
}

/// Dígitos na base `radix` para decimal, com limbos de base 10^9.
fn radix_to_decimal(digits: &str, radix: u32) -> Result<String, String> {
    const BASE: u64 = 1_000_000_000;
    let mut limbs: Vec<u32> = vec![0];
    for c in digits.chars() {
        let Some(d) = c.to_digit(radix) else {
            return Err(format!("invalid digit {c:?} in base {radix} literal"));
        };
        let mut carry = u64::from(d);
        for limb in limbs.iter_mut() {
            let v = u64::from(*limb) * u64::from(radix) + carry;
            *limb = (v % BASE) as u32;
            carry = v / BASE;
        }
        while carry > 0 {
            limbs.push((carry % BASE) as u32);
            carry /= BASE;
        }
    }
    let mut out = limbs.last().copied().unwrap_or(0).to_string();
    for limb in limbs.iter().rev().skip(1) {
        out.push_str(&format!("{limb:09}"));
    }
    Ok(out)
}

/// Valor de um literal de string já sem prefixo nem aspas.
enum StrValue {
    Str(String),
    Bytes(Vec<u8>),
}

/// Decodifica um token STRING (`_PyPegen_parse_string` e `decode_unicode_with_escapes` /
/// `decode_bytes_with_escapes` do `Parser/string_parser.c`).
fn decode_string(text: &str) -> Result<StrValue, String> {
    let quote_at = text.find(['\'', '"']).ok_or_else(|| "malformed string literal".to_string())?;
    let prefix = &text[..quote_at];
    let raw = prefix.contains(['r', 'R']);
    let bytes = prefix.contains(['b', 'B']);
    let rest = &text[quote_at..];
    let rb = rest.as_bytes();
    let qlen = if rb.len() >= 6 && rb[1] == rb[0] && rb[2] == rb[0] { 3 } else { 1 };
    let body = &rest[qlen..rest.len() - qlen];
    if bytes {
        if !body.is_ascii() {
            return Err("bytes can only contain ASCII literal characters".to_string());
        }
        if raw {
            return Ok(StrValue::Bytes(body.as_bytes().to_vec()));
        }
        return decode_bytes_escapes(body).map(StrValue::Bytes);
    }
    if raw {
        return Ok(StrValue::Str(body.to_string()));
    }
    decode_str_escapes(body).map(StrValue::Str)
}

/// Lê até `max` dígitos na base `radix` a partir de `*i`; devolve o valor e quantos leu.
fn read_digits(b: &[u8], i: &mut usize, max: usize, radix: u32) -> (u32, usize) {
    let (mut value, mut count) = (0u32, 0usize);
    while count < max {
        let Some(d) = b.get(*i).and_then(|&c| char::from(c).to_digit(radix)) else { break };
        value = value * radix + d;
        *i += 1;
        count += 1;
    }
    (value, count)
}

fn decode_str_escapes(body: &str) -> Result<String, String> {
    let b = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while let Some(c) = body[i..].chars().next() {
        i += c.len_utf8();
        if c != '\\' {
            out.push(c);
            continue;
        }
        let esc_start = i - 1;
        let Some(e) = body[i..].chars().next() else {
            out.push('\\');
            break;
        };
        i += e.len_utf8();
        match e {
            '\n' => {}
            '\\' | '\'' | '"' => out.push(e),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\x0b'),
            '0'..='7' => {
                let (rest, _) = read_digits(b, &mut i, 2, 8);
                let digits = (i - esc_start - 2) as u32;
                let value = (e as u32 - '0' as u32) * 8u32.pow(digits) + rest;
                // Até 0o777, sempre um código-ponto válido.
                out.extend(char::from_u32(value));
            }
            'x' | 'u' | 'U' => {
                let width = match e {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let (value, count) = read_digits(b, &mut i, width, 16);
                if count < width {
                    return Err(format!(
                        "(unicode error) 'unicodeescape' codec can't decode bytes in position {}-{}: \
                         truncated \\{e}{} escape",
                        esc_start,
                        i - 1,
                        "X".repeat(width)
                    ));
                }
                match char::from_u32(value) {
                    Some(ch) => out.push(ch),
                    None if value > 0x10ffff => {
                        return Err(format!(
                            "(unicode error) 'unicodeescape' codec can't decode bytes in position {}-{}: \
                             illegal Unicode character",
                            esc_start,
                            i - 1
                        ))
                    }
                    None => {
                        return Err(format!(
                            "surrogate escape \\{e}{value:04x} is not representable: str here holds \
                             only Unicode scalar values"
                        ))
                    }
                }
            }
            'N' => {
                return Err("\\N{...} escapes need the unicodedata name table, which is not ported".to_string())
            }
            other => {
                // Escape inválido: o CPython mantém a barra (e emite SyntaxWarning).
                out.push('\\');
                out.push(other);
            }
        }
    }
    Ok(out)
}

fn decode_bytes_escapes(body: &str) -> Result<Vec<u8>, String> {
    let b = body.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let esc_start = i - 1;
        let Some(&e) = b.get(i) else {
            out.push(b'\\');
            break;
        };
        i += 1;
        match e {
            b'\n' => {}
            b'\\' | b'\'' | b'"' => out.push(e),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'0'..=b'7' => {
                let (rest, _) = read_digits(b, &mut i, 2, 8);
                let digits = (i - esc_start - 2) as u32;
                let value = u32::from(e - b'0') * 8u32.pow(digits) + rest;
                // Acima de 0o377 o C trunca para um byte (com SyntaxWarning).
                out.push((value & 0xff) as u8);
            }
            b'x' => {
                let (value, count) = read_digits(b, &mut i, 2, 16);
                if count < 2 {
                    return Err(format!("(value error) invalid \\x escape at position {esc_start}"));
                }
                out.push(value as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::dump_expr;

    fn d(src: &str) -> String {
        dump_expr(&parse_expression(src).unwrap())
    }

    #[test]
    fn arithmetic_precedence() {
        assert_eq!(
            d("1 + 2 * 3"),
            "BinOp(left=Constant(value=1), op=Add(), right=BinOp(left=Constant(value=2), op=Mult(), \
             right=Constant(value=3)))"
        );
        assert_eq!(
            d("-x ** 2"),
            "UnaryOp(op=USub(), operand=BinOp(left=Name(id='x', ctx=Load()), op=Pow(), \
             right=Constant(value=2)))"
        );
    }

    #[test]
    fn bitwise_shift_await_bytes() {
        assert_eq!(
            d("await f() >> 1 | b'\\x00' ^ ~c"),
            "BinOp(left=BinOp(left=Await(value=Call(func=Name(id='f', ctx=Load()), args=[], \
             keywords=[])), op=RShift(), right=Constant(value=1)), op=BitOr(), \
             right=BinOp(left=Constant(value=b'\\x00'), op=BitXor(), right=UnaryOp(op=Invert(), \
             operand=Name(id='c', ctx=Load()))))"
        );
    }

    #[test]
    fn chained_comparisons() {
        assert_eq!(
            d("a < b <= c"),
            "Compare(left=Name(id='a', ctx=Load()), ops=[Lt(), LtE()], \
             comparators=[Name(id='b', ctx=Load()), Name(id='c', ctx=Load())])"
        );
        assert_eq!(
            d("x not in y is not z"),
            "Compare(left=Name(id='x', ctx=Load()), ops=[NotIn(), IsNot()], \
             comparators=[Name(id='y', ctx=Load()), Name(id='z', ctx=Load())])"
        );
    }

    #[test]
    fn boolean_operators() {
        assert_eq!(
            d("not a and b or c"),
            "BoolOp(op=Or(), values=[BoolOp(op=And(), values=[UnaryOp(op=Not(), \
             operand=Name(id='a', ctx=Load())), Name(id='b', ctx=Load())]), Name(id='c', ctx=Load())])"
        );
        assert_eq!(
            d("x if y else z"),
            "IfExp(test=Name(id='y', ctx=Load()), body=Name(id='x', ctx=Load()), \
             orelse=Name(id='z', ctx=Load()))"
        );
    }

    #[test]
    fn call_arguments() {
        assert_eq!(
            d("f(a, *b, c=1, **d)"),
            "Call(func=Name(id='f', ctx=Load()), args=[Name(id='a', ctx=Load()), \
             Starred(value=Name(id='b', ctx=Load()), ctx=Load())], keywords=[keyword(arg='c', \
             value=Constant(value=1)), keyword(value=Name(id='d', ctx=Load()))])"
        );
        assert_eq!(
            d("f(x for x in y)"),
            "Call(func=Name(id='f', ctx=Load()), args=[GeneratorExp(elt=Name(id='x', ctx=Load()), \
             generators=[comprehension(target=Name(id='x', ctx=Store()), iter=Name(id='y', ctx=Load()), \
             ifs=[], is_async=0)])], keywords=[])"
        );
    }

    #[test]
    fn attribute_and_slices() {
        assert_eq!(
            d("a.b[1:2, ::3]"),
            "Subscript(value=Attribute(value=Name(id='a', ctx=Load()), attr='b', ctx=Load()), \
             slice=Tuple(elts=[Slice(lower=Constant(value=1), upper=Constant(value=2)), \
             Slice(step=Constant(value=3))], ctx=Load()), ctx=Load())"
        );
    }

    #[test]
    fn strings_and_numbers() {
        assert_eq!(d("'a' 'b'"), "Constant(value='ab')");
        assert_eq!(d("u'x'"), "Constant(value='x', kind='u')");
        assert_eq!(d("r'\\n' '\\t\\x41\\101'"), "Constant(value='\\\\n\\tAA')");
        assert_eq!(
            d("{1, 2.5, 3j}"),
            "Set(elts=[Constant(value=1), Constant(value=2.5), Constant(value=3j)])"
        );
        assert_eq!(
            d("(y := 0x1f)"),
            "NamedExpr(target=Name(id='y', ctx=Store()), value=Constant(value=31))"
        );
        assert_eq!(
            d("[1, *a, ...]"),
            "List(elts=[Constant(value=1), Starred(value=Name(id='a', ctx=Load()), ctx=Load()), \
             Constant(value=Ellipsis)], ctx=Load())"
        );
        assert_eq!(
            d("1, None,"),
            "Tuple(elts=[Constant(value=1), Constant(value=None)], ctx=Load())"
        );
    }

    #[test]
    fn lambda_with_all_parameter_kinds() {
        assert_eq!(
            d("lambda a, /, b=1, *c, d, e=2, **f: 0"),
            "Lambda(args=arguments(posonlyargs=[arg(arg='a')], args=[arg(arg='b')], \
             vararg=arg(arg='c'), kwonlyargs=[arg(arg='d'), arg(arg='e')], \
             kw_defaults=[None, Constant(value=2)], kwarg=arg(arg='f'), \
             defaults=[Constant(value=1)]), body=Constant(value=0))"
        );
    }

    #[test]
    fn comprehensions_and_dicts() {
        assert_eq!(
            d("[x for x in y if x async for z in w]"),
            "ListComp(elt=Name(id='x', ctx=Load()), generators=[comprehension(target=Name(id='x', \
             ctx=Store()), iter=Name(id='y', ctx=Load()), ifs=[Name(id='x', ctx=Load())], \
             is_async=0), comprehension(target=Name(id='z', ctx=Store()), iter=Name(id='w', \
             ctx=Load()), ifs=[], is_async=1)])"
        );
        assert_eq!(
            d("{1: 2, **d}"),
            "Dict(keys=[Constant(value=1), None], values=[Constant(value=2), Name(id='d', ctx=Load())])"
        );
        assert_eq!(
            d("{k: v for k, v in x}"),
            "DictComp(key=Name(id='k', ctx=Load()), value=Name(id='v', ctx=Load()), \
             generators=[comprehension(target=Tuple(elts=[Name(id='k', ctx=Store()), \
             Name(id='v', ctx=Store())], ctx=Store()), iter=Name(id='x', ctx=Load()), ifs=[], \
             is_async=0)])"
        );
    }

    fn pos(e: &Expr) -> (usize, usize, Option<usize>, Option<usize>) {
        (e.pos.lineno, e.pos.col_offset, e.pos.end_lineno, e.pos.end_col_offset)
    }

    #[test]
    fn positions_like_cpython() {
        let call = parse_expression("f(a, b=1)").unwrap();
        assert_eq!(pos(&call), (1, 0, Some(1), Some(9)));
        let ExprKind::Call { keywords, .. } = &call.kind else { panic!("esperava Call") };
        assert_eq!(keywords[0].pos.col_offset, 5);
        assert_eq!(keywords[0].pos.end_col_offset, Some(8));

        // col_offset em bytes: 'é' ocupa dois.
        let sum = parse_expression("'é' + x").unwrap();
        assert_eq!(pos(&sum), (1, 0, Some(1), Some(8)));
        let ExprKind::BinOp { right, .. } = &sum.kind else { panic!("esperava BinOp") };
        assert_eq!(pos(right), (1, 7, Some(1), Some(8)));

        // O GeneratorExp único de uma chamada abrange os parênteses dela.
        let genexp = parse_expression("f(x for x in y)").unwrap();
        let ExprKind::Call { args, .. } = &genexp.kind else { panic!("esperava Call") };
        assert_eq!(pos(&args[0]), (1, 1, Some(1), Some(15)));

        // Parênteses de agrupamento não entram na posição; os de tupla entram.
        assert_eq!(pos(&parse_expression("(a)").unwrap()), (1, 1, Some(1), Some(2)));
        assert_eq!(pos(&parse_expression("(a,)").unwrap()), (1, 0, Some(1), Some(4)));
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(parse_expression("1 +").unwrap_err().msg, "invalid syntax");
        assert_eq!(parse_expression("f(a=1, b)").unwrap_err().msg, "invalid syntax");
        assert_eq!(
            parse_expression("'a' b'c'").unwrap_err().msg,
            "cannot mix bytes and nonbytes literals"
        );
    }
}
