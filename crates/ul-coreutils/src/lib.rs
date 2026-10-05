//! ul-coreutils: os utilitários do GNU coreutils 9.7 no pseudo-linus, como fork por utilitário do
//! uutils 0.12 sobre o `uucore` portado (vendorizados em `vendor/src`), tudo sobre o `sysio`.
//!
//! Cada grupo de utilitários tem um módulo com a sua tabela; `programs()` junta todas.

use sysabi::Program;

mod core;
mod data;
mod files;
mod hostname;
mod run;
mod stty;
mod system;
mod text;

/// Tabela de programas deste crate.
pub fn programs() -> Vec<Program> {
    let mut out = Vec::new();
    out.extend(core::programs());
    out.extend(data::programs());
    out.extend(files::programs());
    out.extend(system::programs());
    out.extend(text::programs());
    out
}
