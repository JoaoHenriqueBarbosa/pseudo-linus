//! O bash do pseudo-linus: programas `bash` e `sh`.
//!
//! Interpretador nosso sobre um fork do brush-parser (`vendor/brush-parser`, MIT). Camadas:
//! [`parse`] (leitura comando a comando, aliases, mensagens de erro de sintaxe) e [`lower`] +
//! [`word`] (AST próprio em [`ast`]); [`expand`] (expansões), [`exec`] (execução), [`redir`]
//! (redireções), [`builtins`]. Todo I/O passa pelo `sysabi`.

pub mod ast;
pub mod builtins;
pub mod cond;
pub mod entry;
pub mod exec;
pub mod expand;
pub mod glob;
pub mod lower;
pub mod options;
pub mod parse;
pub mod print;
pub mod redir;
pub mod shell;
pub mod timefmt;
pub mod vars;
pub mod word;

// Provisórios até os módulos de sala limpa chegarem (ver STATUS.md).
#[path = "interim/arith.rs"]
pub mod arith;
#[path = "interim/pattern.rs"]
pub mod pattern;
#[path = "interim/printf.rs"]
pub mod printf;
#[path = "interim/quote.rs"]
pub mod quote;

pub use brush_parser;

use sysabi::Program;

/// Programas deste crate: `bash` e `sh` em `/usr/bin` (com `/bin -> usr/bin` como no Debian).
pub fn programs() -> Vec<Program> {
    // No Debian o `sh` é um symlink para o `dash` (`/usr/bin/sh -> dash`): o programa de verdade é o
    // `dash`, e o link sai da tabela de links do Debian.
    vec![Program::bin("bash", entry::bash_main), Program::bin("sh", entry::sh_main), Program::bin("dash", entry::sh_main)]
}
