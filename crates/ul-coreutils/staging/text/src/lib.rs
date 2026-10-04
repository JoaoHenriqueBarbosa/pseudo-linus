//! Preparação do grupo "text": compila `src/run.rs` e `src/text.rs` do `ul-coreutils` com só as
//! dependências do grupo, fora do workspace principal.

#[path = "../../../src/run.rs"]
mod run;
#[path = "../../../src/text.rs"]
mod text;

pub fn programs() -> Vec<sysabi::Program> {
    text::programs()
}
