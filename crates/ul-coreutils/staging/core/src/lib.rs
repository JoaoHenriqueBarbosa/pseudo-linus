//! Preparação do grupo "core": compila `src/run.rs` e `src/core.rs` do `ul-coreutils` com só as
//! dependências do grupo, fora do workspace principal.

#[path = "../../../src/run.rs"]
mod run;
#[path = "../../../src/core.rs"]
mod core;

pub fn programs() -> Vec<sysabi::Program> {
    core::programs()
}
