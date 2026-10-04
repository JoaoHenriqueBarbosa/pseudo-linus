//! A gramática do bc, montada a partir da gramática do POSIX (XCU bc, "Grammar") com as extensões
//! do GNU bc 1.07.1 e com a forma (produções com `error`, ações no meio de regras, precedências)
//! deduzida em caixa preta: linha em que sai cada aviso, quantos erros de sintaxe aparecem e onde a
//! recuperação retoma. Código nosso, MIT.
//!
//! Observações que fixaram a forma:
//!
//! - `input_item: error ENDOFLINE` com `yyerrok` (um segundo erro logo na linha seguinte é
//!   reportado) e `statement_or_error: error statement` dentro das listas (dentro de `{ }` o newline
//!   depois de um erro é descartado, e o `}` também);
//! - `else` só na mesma linha do comando do `if`; um newline opcional depois do `)` de `if`,
//!   `while` e `for`, depois do `else` e entre o `)` e o `{` do `define`;
//! - ações no meio da regra logo depois de `print`, `else`, `&&`, `||`, `while` e do `)` do `if`, que
//!   é onde o GNU emite código e avisos sem ler o token seguinte;
//! - precedência do manual do GNU: `||` < `&&` < `!` < relacionais < atribuição < `+ -` < `* / %` <
//!   `^` < menos unário < `++ --`.

use std::sync::OnceLock;

use super::lalr::{Assoc, Grammar, Rule, Tables};
use super::lexer::{T, TERMINALS};

/// Não terminais.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum N {
    Accept,
    Program,
    InputItem,
    OptNewline,
    SemicolonList,
    StatementList,
    StatementOrError,
    Statement,
    ForM1,
    ForM2,
    ForM3,
    ForM4,
    IfM1,
    OptElse,
    ElseM1,
    WhileM1,
    WhileM2,
    PrintM1,
    PrintList,
    PrintElement,
    Function,
    FuncM1,
    OptVoid,
    OptParameterList,
    OptAutoDefineList,
    DefineList,
    OptArgumentList,
    ArgumentList,
    RequiredEol,
    OptExpression,
    ReturnExpression,
    Expression,
    AssignM1,
    AndM1,
    OrM1,
    NamedExpression,
}

const N_NONTERMS: usize = N::NamedExpression as usize + 1;

/// Ação semântica de cada regra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A {
    None,
    /// `$$ = $1`.
    Pass1,
    RunCode,
    ErrorLine,
    Warranty,
    Limits,
    ExprStmt,
    StringStmt,
    Break,
    Continue,
    Quit,
    Halt,
    Return,
    ForM1,
    ForM2,
    ForM3,
    ForM4,
    ForEnd,
    IfM1,
    IfNoElse,
    ElseM1,
    ElseEnd,
    WhileM1,
    WhileM2,
    WhileEnd,
    PrintM1,
    PrintStr,
    PrintExpr,
    FuncM1,
    FuncEnd,
    OptVoid,
    DefVar,
    DefArray,
    DefRef,
    DefListVar,
    DefListArray,
    DefListRef,
    AutoList,
    ArgsEmpty,
    ArgExpr,
    ArgArray,
    ArgListExpr,
    ArgListArray,
    RequiredEolEmpty,
    OptExprEmpty,
    RetEmpty,
    RetExpr,
    AssignM1,
    Assign,
    AndM1,
    And,
    OrM1,
    Or,
    Not,
    Rel,
    Binary(u8),
    Neg,
    LoadNamed,
    Number,
    Paren,
    Call,
    PreIncr,
    PostIncr,
    Length,
    Sqrt,
    ScaleFn,
    Read,
    Random,
    NamedVar,
    NamedArray,
    NamedSpecial(u8),
    NamedHistory,
    NamedLast,
}

/// Símbolo do corpo de uma regra.
#[derive(Clone, Copy)]
enum S {
    T(T),
    N(N),
}

fn sym(s: S) -> u16 {
    match s {
        S::T(t) => t as u16,
        S::N(n) => (TERMINALS.len() + n as usize) as u16,
    }
}

/// A gramática pronta: tabelas e ação de cada regra.
pub struct Bc {
    pub tables: Tables,
    pub actions: Vec<A>,
    pub eof: u16,
    pub error: u16,
}

/// As tabelas são construídas uma vez por processo hospedeiro (são puras e iguais pra todos).
pub fn get() -> &'static Bc {
    static G: OnceLock<Bc> = OnceLock::new();
    G.get_or_init(build)
}

fn build() -> Bc {
    use N::*;
    use S::N as n;
    use S::T as t;
    let mut rules: Vec<(N, Vec<S>, Option<T>, A)> = Vec::new();
    let mut r = |lhs: N, rhs: &[S], act: A| rules.push((lhs, rhs.to_vec(), None, act));
    r(Accept, &[n(Program), t(T::Eof)], A::None);
    r(Program, &[], A::None);
    r(Program, &[n(Program), n(InputItem)], A::None);
    r(InputItem, &[n(SemicolonList), t(T::EndOfLine)], A::RunCode);
    r(InputItem, &[n(Function)], A::RunCode);
    r(InputItem, &[t(T::Error), t(T::EndOfLine)], A::ErrorLine);
    r(OptNewline, &[], A::None);
    r(OptNewline, &[t(T::EndOfLine)], A::None);
    r(SemicolonList, &[], A::None);
    r(SemicolonList, &[n(StatementOrError)], A::None);
    r(SemicolonList, &[n(SemicolonList), t(T::Semicolon), n(StatementOrError)], A::None);
    r(SemicolonList, &[n(SemicolonList), t(T::Semicolon)], A::None);
    r(StatementList, &[], A::None);
    r(StatementList, &[n(StatementOrError)], A::None);
    r(StatementList, &[n(StatementList), t(T::EndOfLine)], A::None);
    r(StatementList, &[n(StatementList), t(T::EndOfLine), n(StatementOrError)], A::None);
    r(StatementList, &[n(StatementList), t(T::Semicolon)], A::None);
    r(StatementList, &[n(StatementList), t(T::Semicolon), n(StatementOrError)], A::None);
    r(StatementOrError, &[n(Statement)], A::None);
    r(StatementOrError, &[t(T::Error), n(Statement)], A::None);
    r(Statement, &[t(T::Warranty)], A::Warranty);
    r(Statement, &[t(T::Limits)], A::Limits);
    r(Statement, &[n(Expression)], A::ExprStmt);
    r(Statement, &[t(T::Str)], A::StringStmt);
    r(Statement, &[t(T::Break)], A::Break);
    r(Statement, &[t(T::Continue)], A::Continue);
    r(Statement, &[t(T::Quit)], A::Quit);
    r(Statement, &[t(T::Halt)], A::Halt);
    r(Statement, &[t(T::Return), n(ReturnExpression)], A::Return);
    r(
        Statement,
        &[
            t(T::For),
            t(T::LParen),
            n(ForM1),
            n(OptExpression),
            t(T::Semicolon),
            n(ForM2),
            n(OptExpression),
            t(T::Semicolon),
            n(ForM3),
            n(OptExpression),
            t(T::RParen),
            n(ForM4),
            n(OptNewline),
            n(Statement),
        ],
        A::ForEnd,
    );
    r(ForM1, &[], A::ForM1);
    r(ForM2, &[], A::ForM2);
    r(ForM3, &[], A::ForM3);
    r(ForM4, &[], A::ForM4);
    r(
        Statement,
        &[t(T::If), t(T::LParen), n(Expression), t(T::RParen), n(IfM1), n(OptNewline), n(Statement), n(OptElse)],
        A::None,
    );
    r(IfM1, &[], A::IfM1);
    r(OptElse, &[], A::IfNoElse);
    r(OptElse, &[t(T::Else), n(ElseM1), n(OptNewline), n(Statement)], A::ElseEnd);
    r(ElseM1, &[], A::ElseM1);
    r(
        Statement,
        &[t(T::While), n(WhileM1), t(T::LParen), n(Expression), t(T::RParen), n(WhileM2), n(OptNewline), n(Statement)],
        A::WhileEnd,
    );
    r(WhileM1, &[], A::WhileM1);
    r(WhileM2, &[], A::WhileM2);
    r(Statement, &[t(T::LBrace), n(StatementList), t(T::RBrace)], A::None);
    r(Statement, &[t(T::Print), n(PrintM1), n(PrintList)], A::None);
    r(PrintM1, &[], A::PrintM1);
    r(PrintList, &[n(PrintElement)], A::None);
    r(PrintList, &[n(PrintList), t(T::Comma), n(PrintElement)], A::None);
    r(PrintElement, &[t(T::Str)], A::PrintStr);
    r(PrintElement, &[n(Expression)], A::PrintExpr);
    r(
        Function,
        &[
            t(T::Define),
            n(OptVoid),
            t(T::Name),
            t(T::LParen),
            n(OptParameterList),
            t(T::RParen),
            n(OptNewline),
            t(T::LBrace),
            n(RequiredEol),
            n(OptAutoDefineList),
            n(FuncM1),
            n(StatementList),
            t(T::RBrace),
        ],
        A::FuncEnd,
    );
    r(FuncM1, &[], A::FuncM1);
    r(OptVoid, &[], A::None);
    r(OptVoid, &[t(T::Void)], A::OptVoid);
    r(OptParameterList, &[], A::None);
    r(OptParameterList, &[n(DefineList)], A::Pass1);
    r(OptAutoDefineList, &[], A::None);
    r(OptAutoDefineList, &[t(T::Auto), n(DefineList), t(T::EndOfLine)], A::AutoList);
    r(OptAutoDefineList, &[t(T::Auto), n(DefineList), t(T::Semicolon)], A::AutoList);
    r(DefineList, &[t(T::Name)], A::DefVar);
    r(DefineList, &[t(T::Name), t(T::LBracket), t(T::RBracket)], A::DefArray);
    r(DefineList, &[t(T::Star), t(T::Name), t(T::LBracket), t(T::RBracket)], A::DefRef);
    r(DefineList, &[t(T::Amp), t(T::Name), t(T::LBracket), t(T::RBracket)], A::DefRef);
    r(DefineList, &[n(DefineList), t(T::Comma), t(T::Name)], A::DefListVar);
    r(DefineList, &[n(DefineList), t(T::Comma), t(T::Name), t(T::LBracket), t(T::RBracket)], A::DefListArray);
    r(DefineList, &[n(DefineList), t(T::Comma), t(T::Star), t(T::Name), t(T::LBracket), t(T::RBracket)], A::DefListRef);
    r(DefineList, &[n(DefineList), t(T::Comma), t(T::Amp), t(T::Name), t(T::LBracket), t(T::RBracket)], A::DefListRef);
    r(OptArgumentList, &[], A::ArgsEmpty);
    r(OptArgumentList, &[n(ArgumentList)], A::Pass1);
    r(ArgumentList, &[n(Expression)], A::ArgExpr);
    r(ArgumentList, &[t(T::Name), t(T::LBracket), t(T::RBracket)], A::ArgArray);
    r(ArgumentList, &[n(ArgumentList), t(T::Comma), n(Expression)], A::ArgListExpr);
    r(ArgumentList, &[n(ArgumentList), t(T::Comma), t(T::Name), t(T::LBracket), t(T::RBracket)], A::ArgListArray);
    r(RequiredEol, &[], A::RequiredEolEmpty);
    r(RequiredEol, &[t(T::EndOfLine)], A::None);
    r(OptExpression, &[], A::OptExprEmpty);
    r(OptExpression, &[n(Expression)], A::Pass1);
    r(ReturnExpression, &[], A::RetEmpty);
    r(ReturnExpression, &[n(Expression)], A::RetExpr);
    r(Expression, &[n(NamedExpression), t(T::AssignOp), n(AssignM1), n(Expression)], A::Assign);
    r(AssignM1, &[], A::AssignM1);
    r(Expression, &[n(Expression), t(T::And), n(AndM1), n(Expression)], A::And);
    r(AndM1, &[], A::AndM1);
    r(Expression, &[n(Expression), t(T::Or), n(OrM1), n(Expression)], A::Or);
    r(OrM1, &[], A::OrM1);
    r(Expression, &[t(T::Not), n(Expression)], A::Not);
    r(Expression, &[n(Expression), t(T::RelOp), n(Expression)], A::Rel);
    for (tok, op) in [(T::Plus, b'+'), (T::Minus, b'-'), (T::Star, b'*'), (T::Slash, b'/'), (T::Percent, b'%'), (T::Caret, b'^')] {
        r(Expression, &[n(Expression), t(tok), n(Expression)], A::Binary(op));
    }
    // '-' expression %prec UNARY_MINUS
    rules.push((Expression, vec![t(T::Minus), n(Expression)], Some(T::UnaryMinus), A::Neg));
    let mut r = |lhs: N, rhs: &[S], act: A| rules.push((lhs, rhs.to_vec(), None, act));
    r(Expression, &[n(NamedExpression)], A::LoadNamed);
    r(Expression, &[t(T::Number)], A::Number);
    r(Expression, &[t(T::LParen), n(Expression), t(T::RParen)], A::Paren);
    r(Expression, &[t(T::Name), t(T::LParen), n(OptArgumentList), t(T::RParen)], A::Call);
    r(Expression, &[t(T::IncrDecr), n(NamedExpression)], A::PreIncr);
    r(Expression, &[n(NamedExpression), t(T::IncrDecr)], A::PostIncr);
    r(Expression, &[t(T::Length), t(T::LParen), n(Expression), t(T::RParen)], A::Length);
    r(Expression, &[t(T::Sqrt), t(T::LParen), n(Expression), t(T::RParen)], A::Sqrt);
    r(Expression, &[t(T::Scale), t(T::LParen), n(Expression), t(T::RParen)], A::ScaleFn);
    r(Expression, &[t(T::Read), t(T::LParen), t(T::RParen)], A::Read);
    r(Expression, &[t(T::Random), t(T::LParen), t(T::RParen)], A::Random);
    r(NamedExpression, &[t(T::Name)], A::NamedVar);
    r(NamedExpression, &[t(T::Name), t(T::LBracket), n(Expression), t(T::RBracket)], A::NamedArray);
    r(NamedExpression, &[t(T::Ibase)], A::NamedSpecial(0));
    r(NamedExpression, &[t(T::Obase)], A::NamedSpecial(1));
    r(NamedExpression, &[t(T::Scale)], A::NamedSpecial(2));
    r(NamedExpression, &[t(T::History)], A::NamedHistory);
    r(NamedExpression, &[t(T::Last)], A::NamedLast);

    let mut term_prec = vec![(0u8, Assoc::Left); TERMINALS.len()];
    let levels: &[(&[T], Assoc)] = &[
        (&[T::Or], Assoc::Left),
        (&[T::And], Assoc::Left),
        (&[T::Not], Assoc::NonAssoc),
        (&[T::RelOp], Assoc::Left),
        (&[T::AssignOp], Assoc::Right),
        (&[T::Plus, T::Minus], Assoc::Left),
        (&[T::Star, T::Slash, T::Percent], Assoc::Left),
        (&[T::Caret], Assoc::Right),
        (&[T::UnaryMinus], Assoc::NonAssoc),
        (&[T::IncrDecr], Assoc::NonAssoc),
    ];
    for (i, (toks, assoc)) in levels.iter().enumerate() {
        for &tk in *toks {
            term_prec[tk as usize] = (i as u8 + 1, *assoc);
        }
    }
    let n_terms = TERMINALS.len();
    let g = Grammar {
        n_terms,
        n_symbols: n_terms + N_NONTERMS,
        eof: T::Eof as u16,
        error: T::Error as u16,
        rules: rules
            .iter()
            .map(|(lhs, rhs, prec, _)| Rule {
                lhs: sym(S::N(*lhs)),
                rhs: rhs.iter().map(|s| sym(*s)).collect(),
                prec: prec.map(|p| p as u16),
            })
            .collect(),
        term_prec,
    };
    let tables = g.build();
    Bc { tables, actions: rules.iter().map(|r| r.3).collect(), eof: T::Eof as u16, error: T::Error as u16 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_expected_conflicts() {
        let g = get();
        // Dois conflitos deslocar/reduzir sem precedência, os dois resolvidos por "desloca" como no
        // bison: o `else` pendente e o fim de linha depois do `{` do `define` (vira o
        // `required_eol` em vez de uma linha vazia no corpo; sem ele sai "End of line required").
        assert_eq!(g.tables.sr_conflicts, 2);
        assert_eq!(g.tables.rr_conflicts, 0);
    }
}
