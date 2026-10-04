//! Preparação do grupo "data": compila `src/run.rs` e `src/data.rs` do `ul-coreutils` com só as
//! dependências do grupo, fora do workspace principal.

#[path = "../../../src/run.rs"]
mod run;
#[path = "../../../src/data.rs"]
mod data;

pub fn programs() -> Vec<sysabi::Program> {
    data::programs()
}
