//! Preparação do grupo "system": compila `src/run.rs` e `src/system.rs` do `ul-coreutils` com só as
//! dependências do grupo, fora do workspace principal.

#[path = "../../../src/run.rs"]
mod run;
#[path = "../../../src/system.rs"]
mod system;

pub fn programs() -> Vec<sysabi::Program> {
    system::programs()
}
