//! Módulos embutidos do interpretador (fatias 13 a 16 de `docs/python3-port.md`): escritos em Rust,
//! sem depender de `Lib/` em Python. Aqui ficam as partes puras (JSON, CSV); a ligação com a VM
//! (`import`, `sys`, `open`, métodos de arquivo) está em `vm.rs`.

pub mod csv;
pub mod json;
