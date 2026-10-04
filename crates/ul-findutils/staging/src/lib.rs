//! Preparação do ul-findutils: compila o `src/` do crate (via `#[path]`) com o findutils vendorizado,
//! fora do workspace principal.

#[path = "../../src/lib.rs"]
mod ul_findutils;

pub use ul_findutils::programs;
