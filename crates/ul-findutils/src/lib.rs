//! ul-findutils: `find` e `xargs` do GNU findutils 4.10 no pseudo-linus.
//!
//! - `find`: fork do uutils findutils 0.10 (`vendor/findutils`) com o walkdir portado
//!   (`vendor/walkdir`), sobre o `sysio`, com as mensagens e os códigos do GNU.
//! - `xargs`: escrito aqui (`xargs/`), com as opções, os leitores, os limites e as mensagens do
//!   GNU, e o executor sobre `sysio::process::Command`.

use sysabi::Program;

mod find;

/// Tabela de programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("find", find::main)]
}
