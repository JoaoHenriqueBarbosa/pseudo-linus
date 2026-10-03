//! E06: as três camadas de isolamento do design v2, cada uma posta à prova.
//!
//! - [`h19`]: `clippy::disallowed_methods` no crate de userland não enxerga dependências; o depscan
//!   enxerga.
//! - [`h20`]: o que `forbid(unsafe_code)` pega quando o `unsafe` vem de macro de outra crate.
//! - [`h21`]: Landlock e seccomp por thread. Tudo que restringe roda num subprocesso ([`child`]),
//!   nunca no processo principal.
//!
//! As crates-sonda de H19 e H20 ficam em `probes/` (workspace próprio) e são compiladas pelo binário
//! via [`cargo_probe`].

pub mod cargo_probe;
pub mod child;
pub mod h19;
pub mod h20;
pub mod h21;
pub mod layout;
pub mod probe;
pub mod sandbox;
