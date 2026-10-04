//! Preparação do grupo "files": compila `src/run.rs` e `src/files.rs` do `ul-coreutils` com só as
//! dependências do grupo, fora do workspace principal.

#[path = "../../../src/run.rs"]
mod run;
#[path = "../../../src/files.rs"]
mod files;

pub fn programs() -> Vec<sysabi::Program> {
    files::programs()
}
