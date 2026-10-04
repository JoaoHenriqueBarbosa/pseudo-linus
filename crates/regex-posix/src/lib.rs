//! regex-posix: o motor de regex do GNU (glibc 2.41) pro pseudo-linus.
//!
//! - Sintaxe: o `regcomp.c` do glibc, dirigido pelos bits [`Syntax`] (as sintaxes do grep, egrep,
//!   sed, gawk, awk, find e ed já vêm prontas), com as extensões GNU (`\w \W \s \S \b \B \< \>
//!   \` \'`, `\+ \? \|` em BRE), classes POSIX, intervalos e referências; mensagens de erro
//!   iguais às do glibc ([`ErrorCode::message`]).
//! - Semântica: casada leftmost-longest, submatches com as escolhas do glibc (que não são as do
//!   POSIX), `RE_ICASE` como o glibc (texto e padrão em maiúsculas), C.UTF-8.
//! - Execução: `regex-automata` pra padrão sem referência (DFA, tempo linear) e um NFA próprio pro
//!   resto. Biblioteca pura: nenhuma E/S; laços longos chamam o gancho de
//!   [`RegexBuilder::checkpoint`].
//!
//! Ver `API.md` no diretório do crate pra o guia de uso (grep, sed, awk, find).

pub mod ast;
pub mod charclass;
pub mod error;
mod hir;
pub mod nfa;
pub mod parse;
mod regex;
pub mod syntax;

pub use error::{CONFUSING_BRACKETS, Error, ErrorCode, Warning};
pub use nfa::ExecFlags;
pub use regex::{Captures, Diagnostics, Match, Matches, Regex, RegexBuilder};
pub use syntax::Syntax;
