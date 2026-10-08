//! `fts5parse.c` (gerado pelo lemon 3.46.1 a partir de `fts5parse.y`): as tabelas LALR e o motor
//! do autômato de pilha da linguagem de consulta do FTS5, mais as ações semânticas (`yy_reduce`)
//! que chamam o construtor de árvore do `expr.rs`.
//!
//! O motor do parser principal (`crate::parse_tables`) amarra a pilha ao `YyMinor` do SQL e à
//! `Connection`, então não é reaproveitável aqui. As diferenças deste parser, resolvidas como
//! `#ifdef` do C: `YYNOERRORRECOVERY` ligado (um erro de sintaxe só avisa e descarta o token),
//! `YYFALLBACK` e `YYWILDCARD` desligados (a tabela de fallback é vazia e não há coringa, então a
//! busca da ação é só a tabela e o padrão do estado), `YYDYNSTACK` zero (a pilha NÃO cresce:
//! passar de 100 entradas é o erro `fts5: parser stack overflow`), `NDEBUG` ligado.
//!
//! Modelo de dados: o `YYMINORTYPE` do C é o enum [`YyMinor`], com um valor possuído por
//! variante (`yy0` é o token, uma fatia do texto da consulta; `yy4` é o inteiro do `star_opt`;
//! `yy11`, `yy24`, `yy46` e `yy53` são o colset, o nó, o nearset e a frase). Os `%destructor` somem:
//! nós e frases vivem na arena do `Fts5Expr` e os colsets e nearsets são valores que o `Drop`
//! libera. Uma ação que move um valor da pilha o tira com `mem::take`, deixando `Init`.

use std::mem;

use crate::printf::PrintfArg;

use super::expr::{Fts5ExprNearset, Fts5Parse, NodeId, PhraseId};
use super::int::Fts5Colset;

// ---------------------------------------------------------------------------------------------
// Códigos de token (fts5parse.h) e tipos de nó
// ---------------------------------------------------------------------------------------------

/// Fim da entrada (também o tipo de um nó que nunca casa).
pub const FTS5_EOF: i32 = 0;
/// Token `OR`.
pub const FTS5_OR: i32 = 1;
/// Token `AND`.
pub const FTS5_AND: i32 = 2;
/// Token `NOT`.
pub const FTS5_NOT: i32 = 3;
/// Tipo de nó `TERM` (um NEAR de uma frase de um termo só); como token é o `%left TERM`.
pub const FTS5_TERM: i32 = 4;
/// Token `:`.
pub const FTS5_COLON: i32 = 5;
/// Token `-`.
pub const FTS5_MINUS: i32 = 6;
/// Token `{`.
pub const FTS5_LCP: i32 = 7;
/// Token `}`.
pub const FTS5_RCP: i32 = 8;
/// Token de texto (palavra nua ou entre aspas); também o tipo de nó `STRING`.
pub const FTS5_STRING: i32 = 9;
/// Token `(`.
pub const FTS5_LP: i32 = 10;
/// Token `)`.
pub const FTS5_RP: i32 = 11;
/// Token `^`.
pub const FTS5_CARET: i32 = 12;
/// Token `,`.
pub const FTS5_COMMA: i32 = 13;
/// Token `+`.
pub const FTS5_PLUS: i32 = 14;
/// Token `*`.
pub const FTS5_STAR: i32 = 15;

// ---------------------------------------------------------------------------------------------
// Constantes de controle e tabelas
// ---------------------------------------------------------------------------------------------

/// Profundidade da pilha (`YYSTACKDEPTH`): fixa, sem `YYGROWABLESTACK`.
const YYSTACKDEPTH: usize = 100;
/// Maior ação de deslocamento (shift).
const YY_MAX_SHIFT: u8 = 34;
/// Menor ação shift-reduce.
const YY_MIN_SHIFTREDUCE: u8 = 52;
/// Maior ação shift-reduce.
const YY_MAX_SHIFTREDUCE: u8 = 79;
/// Ação de erro de sintaxe.
const YY_ERROR_ACTION: u8 = 80;
/// Ação de aceitação.
const YY_ACCEPT_ACTION: u8 = 81;
/// Menor ação reduce.
const YY_MIN_REDUCE: u8 = 83;

static YY_ACTION: [u8; 105] = [
    81, 20, 96, 6, 28, 99, 98, 26, 26, 18, 96, 6, 28, 17, 98, 56, 26, 19, 96, 6, 28, 14, 98, 14,
    26, 31, 92, 96, 6, 28, 108, 98, 25, 26, 21, 96, 6, 28, 78, 98, 58, 26, 29, 96, 6, 28, 107, 98,
    22, 26, 24, 16, 12, 11, 1, 13, 13, 24, 16, 23, 11, 33, 34, 13, 97, 8, 27, 32, 98, 7, 26, 3, 4,
    5, 3, 4, 5, 3, 83, 4, 5, 3, 63, 5, 3, 62, 12, 2, 86, 13, 9, 30, 10, 10, 54, 57, 75, 78, 78,
    53, 57, 15, 82, 82, 71,
];

static YY_LOOKAHEAD: [u8; 121] = [
    16, 17, 18, 19, 20, 22, 22, 24, 24, 17, 18, 19, 20, 7, 22, 9, 24, 17, 18, 19, 20, 9, 22, 9,
    24, 13, 17, 18, 19, 20, 26, 22, 24, 24, 17, 18, 19, 20, 15, 22, 9, 24, 17, 18, 19, 20, 26, 22,
    21, 24, 6, 7, 9, 9, 10, 12, 12, 6, 7, 21, 9, 24, 25, 12, 18, 5, 20, 14, 22, 5, 24, 3, 1, 2, 3,
    1, 2, 3, 0, 1, 2, 3, 11, 2, 3, 11, 9, 10, 5, 12, 23, 24, 10, 10, 8, 9, 9, 15, 15, 8, 9, 9, 27,
    27, 11, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
];

static YY_SHIFT_OFST: [u8; 35] = [
    44, 44, 44, 44, 44, 44, 51, 77, 43, 12, 14, 83, 82, 14, 23, 23, 31, 31, 71, 74, 78, 81, 86, 91,
    6, 53, 53, 60, 64, 68, 53, 87, 92, 53, 93,
];

static YY_REDUCE_OFST: [i8; 18] =
    [-16, -8, 0, 9, 17, 25, 46, -17, -17, 37, 67, 4, 4, 8, 4, 20, 27, 38];

static YY_DEFAULT: [u8; 35] = [
    80, 80, 80, 80, 80, 80, 95, 80, 80, 105, 80, 110, 110, 80, 110, 110, 80, 80, 80, 80, 80, 91,
    80, 80, 80, 101, 100, 80, 80, 90, 103, 80, 80, 104, 80,
];

/// `yyRuleInfoLhs`: o símbolo do lado esquerdo de cada regra.
static YY_RULE_INFO_LHS: [u8; 28] = [
    16, 20, 20, 20, 20, 21, 21, 17, 17, 17, 17, 17, 17, 19, 19, 18, 18, 22, 22, 22, 23, 23, 25, 25,
    24, 24, 26, 26,
];

/// `yyRuleInfoNRhs`: o negativo do tamanho do lado direito de cada regra.
static YY_RULE_INFO_NRHS: [i8; 28] = [
    -1, -4, -3, -1, -2, -2, -1, -3, -3, -3, -5, -3, -1, -1, -2, -1, -3, -1, -2, -5, -1, -2, 0, -2,
    -4, -2, -1, 0,
];

// ---------------------------------------------------------------------------------------------
// A pilha
// ---------------------------------------------------------------------------------------------

/// O `YYMINORTYPE` do C, um valor possuído por variante.
#[derive(Default)]
enum YyMinor<'a> {
    /// Sem valor (o `yyinit`).
    #[default]
    Init,
    /// `yy0`: um token (fatia do texto da consulta).
    Yy0(&'a [u8]),
    /// `yy4`: o inteiro do `star_opt`.
    Yy4(i32),
    /// `yy11`: um colset.
    Yy11(Option<Fts5Colset>),
    /// `yy24`: um nó da expressão.
    Yy24(Option<NodeId>),
    /// `yy46`: um nearset.
    Yy46(Option<Fts5ExprNearset>),
    /// `yy53`: uma frase.
    Yy53(Option<PhraseId>),
}

/// `yyStackEntry`.
#[derive(Default)]
struct YyStackEntry<'a> {
    /// O estado, ou a ação de redução pendente de um SHIFTREDUCE.
    stateno: u8,
    /// O símbolo (o código do terminal ou não-terminal).
    major: u8,
    /// O valor semântico.
    minor: YyMinor<'a>,
}

/// `yyParser`: o estado do autômato. `tos` é o índice do topo (o `yytos` do C); a entrada 0 é a
/// sentinela de estado 0.
pub struct YyParser<'a> {
    stack: Vec<YyStackEntry<'a>>,
    tos: usize,
}

impl<'a> YyParser<'a> {
    /// `sqlite3Fts5ParserInit`: pilha com a sentinela de estado 0.
    pub fn new() -> YyParser<'a> {
        let mut stack = Vec::with_capacity(YYSTACKDEPTH);
        stack.resize_with(YYSTACKDEPTH, YyStackEntry::default);
        YyParser { stack, tos: 0 }
    }

    /// `yy_pop_parser_stack`: desempilha uma vez (soltar o valor é o `yy_destructor`).
    fn pop(&mut self) {
        debug_assert!(self.tos > 0);
        let e = &mut self.stack[self.tos];
        self.tos -= 1;
        drop(mem::take(&mut e.minor));
    }
}

impl Default for YyParser<'_> {
    fn default() -> Self {
        YyParser::new()
    }
}

/// `yy_find_shift_action`: a ação do parser para o terminal `look` no estado `stateno`.
fn yy_find_shift_action(look: u8, stateno: u8) -> u8 {
    if stateno > YY_MAX_SHIFT {
        return stateno;
    }
    let i = YY_SHIFT_OFST[stateno as usize] as usize + look as usize;
    if YY_LOOKAHEAD[i] != look {
        YY_DEFAULT[stateno as usize]
    } else {
        YY_ACTION[i]
    }
}

/// `yy_find_reduce_action`: a ação para o não-terminal `look` no estado `stateno` (sem
/// `YYERRORSYMBOL`, valem os asserts do C: o estado e o deslocamento sempre estão na tabela).
fn yy_find_reduce_action(stateno: u8, look: u8) -> u8 {
    let i = (YY_REDUCE_OFST[stateno as usize] as isize + look as isize) as usize;
    debug_assert!(YY_LOOKAHEAD[i] == look);
    YY_ACTION[i]
}

/// `yyStackOverflow`: esvazia a pilha e grava o erro da gramática.
fn yy_stack_overflow(p: &mut YyParser<'_>, parse: &mut Fts5Parse<'_>) {
    while p.tos > 0 {
        p.pop();
    }
    parse.error(b"fts5: parser stack overflow", &[]);
}

/// `yy_shift`: empilha o token `minor` com o símbolo `major` no estado `new_state`. Devolve falso
/// se a pilha estourou (o C sai do laço do `sqlite3Fts5Parser` do mesmo jeito).
fn yy_shift<'a>(
    p: &mut YyParser<'a>,
    parse: &mut Fts5Parse<'_>,
    new_state: u8,
    major: u8,
    minor: &'a [u8],
) -> bool {
    p.tos += 1;
    if p.tos >= YYSTACKDEPTH {
        p.tos -= 1;
        yy_stack_overflow(p, parse);
        return false;
    }
    let state = if new_state > YY_MAX_SHIFT { new_state + (YY_MIN_REDUCE - YY_MIN_SHIFTREDUCE) } else { new_state };
    let e = &mut p.stack[p.tos];
    e.stateno = state;
    e.major = major;
    e.minor = YyMinor::Yy0(minor);
    true
}

// ---------------------------------------------------------------------------------------------
// As ações semânticas
// ---------------------------------------------------------------------------------------------

/// O token guardado na entrada (`yy0`); vazio se a entrada não guarda um.
fn tok<'a>(e: &YyStackEntry<'a>) -> &'a [u8] {
    match e.minor {
        YyMinor::Yy0(t) => t,
        _ => &[],
    }
}

/// Tira o nó (`yy24`) da entrada.
fn take_node(e: &mut YyStackEntry<'_>) -> Option<NodeId> {
    match mem::take(&mut e.minor) {
        YyMinor::Yy24(n) => n,
        _ => None,
    }
}

/// Tira o colset (`yy11`) da entrada.
fn take_colset(e: &mut YyStackEntry<'_>) -> Option<Fts5Colset> {
    match mem::take(&mut e.minor) {
        YyMinor::Yy11(c) => c,
        _ => None,
    }
}

/// Tira o nearset (`yy46`) da entrada.
fn take_near(e: &mut YyStackEntry<'_>) -> Option<Fts5ExprNearset> {
    match mem::take(&mut e.minor) {
        YyMinor::Yy46(n) => n,
        _ => None,
    }
}

/// Tira a frase (`yy53`) da entrada.
fn take_phrase(e: &mut YyStackEntry<'_>) -> Option<PhraseId> {
    match mem::take(&mut e.minor) {
        YyMinor::Yy53(ph) => ph,
        _ => None,
    }
}

/// `yy_reduce`: executa a ação da regra `yyruleno`, acha o estado de destino pelo goto do
/// não-terminal da regra, recolhe a pilha e empilha o lado esquerdo. Devolve a ação seguinte
/// (o `yyact` do C).
///
/// O C deixa o resultado numa entrada `yymsp[k]` que depois vira o topo; aqui a ação devolve o
/// valor e o fim da função o grava na entrada que passa a ser o topo (`yymsp[yysize+1]`).
fn yy_reduce(p: &mut YyParser<'_>, yyruleno: usize, parse: &mut Fts5Parse<'_>) -> u8 {
    let old_tos = p.tos;
    // `yymsp[k]` do C, com k <= 0 (ou 1 nas regras de lado direito vazio).
    let at = |k: isize| (old_tos as isize + k) as usize;
    let stack = &mut p.stack;

    let result: YyMinor<'_> = match yyruleno {
        0 => {
            /* input ::= expr */
            let x = take_node(&mut stack[at(0)]);
            parse.finished(x);
            YyMinor::Init
        }
        1 => {
            /* colset ::= MINUS LCP colsetlist RCP */
            let x = take_colset(&mut stack[at(-1)]);
            YyMinor::Yy11(parse.colset_invert(x))
        }
        2 => {
            /* colset ::= LCP colsetlist RCP */
            YyMinor::Yy11(take_colset(&mut stack[at(-1)]))
        }
        3 => {
            /* colset ::= STRING */
            let x = tok(&stack[at(0)]);
            YyMinor::Yy11(parse.colset(None, x))
        }
        4 => {
            /* colset ::= MINUS STRING */
            let x = tok(&stack[at(0)]);
            let a = parse.colset(None, x);
            YyMinor::Yy11(parse.colset_invert(a))
        }
        5 => {
            /* colsetlist ::= colsetlist STRING */
            let y = take_colset(&mut stack[at(-1)]);
            let x = tok(&stack[at(0)]);
            YyMinor::Yy11(parse.colset(y, x))
        }
        6 => {
            /* colsetlist ::= STRING */
            let x = tok(&stack[at(0)]);
            YyMinor::Yy11(parse.colset(None, x))
        }
        7 | 8 | 9 => {
            /* expr ::= expr AND expr, expr ::= expr OR expr, expr ::= expr NOT expr */
            let e_type = match yyruleno {
                7 => FTS5_AND,
                8 => FTS5_OR,
                _ => FTS5_NOT,
            };
            let x = take_node(&mut stack[at(-2)]);
            let y = take_node(&mut stack[at(0)]);
            YyMinor::Yy24(parse.node(e_type, x, y, None))
        }
        10 => {
            /* expr ::= colset COLON LP expr RP */
            let y = take_node(&mut stack[at(-1)]);
            let x = take_colset(&mut stack[at(-4)]);
            parse.set_colset(y, x);
            YyMinor::Yy24(y)
        }
        11 => {
            /* expr ::= LP expr RP */
            YyMinor::Yy24(take_node(&mut stack[at(-1)]))
        }
        12 | 13 => {
            /* expr ::= exprlist, exprlist ::= cnearset */
            YyMinor::Yy24(take_node(&mut stack[at(0)]))
        }
        14 => {
            /* exprlist ::= exprlist cnearset */
            let x = take_node(&mut stack[at(-1)]);
            let y = take_node(&mut stack[at(0)]);
            YyMinor::Yy24(parse.implicit_and(x, y))
        }
        15 => {
            /* cnearset ::= nearset */
            let x = take_near(&mut stack[at(0)]);
            YyMinor::Yy24(parse.node(FTS5_STRING, None, None, x))
        }
        16 => {
            /* cnearset ::= colset COLON nearset */
            let y = take_near(&mut stack[at(0)]);
            let x = take_colset(&mut stack[at(-2)]);
            let a = parse.node(FTS5_STRING, None, None, y);
            parse.set_colset(a, x);
            YyMinor::Yy24(a)
        }
        17 => {
            /* nearset ::= phrase */
            let y = take_phrase(&mut stack[at(0)]);
            YyMinor::Yy46(parse.nearset(None, y))
        }
        18 => {
            /* nearset ::= CARET phrase */
            let y = take_phrase(&mut stack[at(0)]);
            parse.set_caret(y);
            YyMinor::Yy46(parse.nearset(None, y))
        }
        19 => {
            /* nearset ::= STRING LP nearphrases neardist_opt RP */
            let near_tok = tok(&stack[at(-4)]);
            let dist_tok = tok(&stack[at(-1)]);
            let mut y = take_near(&mut stack[at(-2)]);
            parse.near(near_tok);
            parse.set_distance(y.as_mut(), dist_tok);
            YyMinor::Yy46(y)
        }
        20 => {
            /* nearphrases ::= phrase */
            let x = take_phrase(&mut stack[at(0)]);
            YyMinor::Yy46(parse.nearset(None, x))
        }
        21 => {
            /* nearphrases ::= nearphrases phrase */
            let x = take_near(&mut stack[at(-1)]);
            let y = take_phrase(&mut stack[at(0)]);
            YyMinor::Yy46(parse.nearset(x, y))
        }
        22 => {
            /* neardist_opt ::= */
            YyMinor::Yy0(&[])
        }
        23 => {
            /* neardist_opt ::= COMMA STRING */
            YyMinor::Yy0(tok(&stack[at(0)]))
        }
        24 => {
            /* phrase ::= phrase PLUS STRING star_opt */
            let x = take_phrase(&mut stack[at(-3)]);
            let y = tok(&stack[at(-1)]);
            let z = matches!(stack[at(0)].minor, YyMinor::Yy4(v) if v != 0);
            YyMinor::Yy53(parse.term(x, y, z))
        }
        25 => {
            /* phrase ::= STRING star_opt */
            let y = tok(&stack[at(-1)]);
            let z = matches!(stack[at(0)].minor, YyMinor::Yy4(v) if v != 0);
            YyMinor::Yy53(parse.term(None, y, z))
        }
        26 => {
            /* star_opt ::= STAR */
            YyMinor::Yy4(1)
        }
        _ => {
            /* star_opt ::= */
            debug_assert!(yyruleno == 27);
            YyMinor::Yy4(0)
        }
    };

    debug_assert!(yyruleno < YY_RULE_INFO_LHS.len());
    let yygoto = YY_RULE_INFO_LHS[yyruleno];
    let yysize = YY_RULE_INFO_NRHS[yyruleno] as isize;
    let base = (old_tos as isize + yysize) as usize;
    let yyact = yy_find_reduce_action(stack[base].stateno, yygoto);

    // Não há SHIFTREDUCE em não-terminais (o gerador os simplificou em REDUCE puros), e um
    // REDUCE nunca é seguido de erro.
    debug_assert!(!(yyact > YY_MAX_SHIFT && yyact <= YY_MAX_SHIFTREDUCE));
    debug_assert!(yyact != YY_ERROR_ACTION);

    let new_tos = base + 1;
    // Os valores que a ação não moveu, acima do novo topo, são soltos (os `yy_destructor`).
    for k in (new_tos + 1)..=old_tos {
        drop(mem::take(&mut stack[k].minor));
    }
    p.tos = new_tos;
    let e = &mut stack[new_tos];
    e.minor = result;
    e.stateno = yyact;
    e.major = yygoto;
    yyact
}

/// `yy_syntax_error`: a ação `%syntax_error` do fts5parse.y. O `TOKEN.n` com o `%.*s` do C
/// imprime os bytes do token até o primeiro NUL; o token de fim de entrada é vazio.
fn yy_syntax_error(parse: &mut Fts5Parse<'_>, token: &[u8]) {
    parse.error(
        b"fts5: syntax error near \"%s\"",
        &[PrintfArg::Text(Some(token.to_vec()))],
    );
}

/// `sqlite3Fts5Parser`: alimenta o autômato com um token (`yymajor` é o código `FTS5_*`,
/// `yyminor` o texto). `parse` é o `%extra_argument`.
///
/// Sem recuperação de erro (`YYNOERRORRECOVERY`): num erro de sintaxe chama `yy_syntax_error`,
/// descarta o token e devolve o controle; o chamador decide parar por `parse.rc`.
pub fn fts5_parser<'a>(
    p: &mut YyParser<'a>,
    yymajor: i32,
    yyminor: &'a [u8],
    parse: &mut Fts5Parse<'_>,
) {
    debug_assert!(p.tos < YYSTACKDEPTH);
    let major = yymajor as u8;
    let mut yyact = p.stack[p.tos].stateno;
    loop {
        debug_assert!(yyact == p.stack[p.tos].stateno);
        yyact = yy_find_shift_action(major, yyact);
        if yyact >= YY_MIN_REDUCE {
            let yyruleno = (yyact - YY_MIN_REDUCE) as usize;
            // Garante espaço para empilhar o lado esquerdo de uma regra de lado direito vazio.
            if YY_RULE_INFO_NRHS[yyruleno] == 0 && p.tos >= YYSTACKDEPTH - 1 {
                yy_stack_overflow(p, parse);
                break;
            }
            yyact = yy_reduce(p, yyruleno, parse);
        } else if yyact <= YY_MAX_SHIFTREDUCE {
            yy_shift(p, parse, yyact, major, yyminor);
            break;
        } else if yyact == YY_ACCEPT_ACTION {
            p.tos -= 1;
            debug_assert!(p.tos == 0);
            return;
        } else {
            debug_assert!(yyact == YY_ERROR_ACTION);
            yy_syntax_error(parse, yyminor);
            break;
        }
    }
}
