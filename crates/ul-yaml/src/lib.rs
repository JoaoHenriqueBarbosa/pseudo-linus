//! ul-yaml: o `yq` 3.4.3 (kislyuk) do Debian 13.
//!
//! O yq original é um programa Python: carrega YAML com o PyYAML (parser do libyaml), escreve JSON
//! pro `jq` num subprocesso e, com `-y`/`-Y`, lê a saída do jq e escreve YAML com o emissor do
//! PyYAML. Aqui o PyYAML está portado pra Rust seguro (`scanner`, `parser`, `load`, `dump`), o
//! JSON segue o módulo `json` do Python (`py`) e o jq é o do próprio sandbox.

pub mod dump;
pub mod load;
pub mod parser;
pub mod py;
pub mod scanner;
pub mod yq;

use sysabi::Program;

/// Programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("yq", yq::main)]
}
