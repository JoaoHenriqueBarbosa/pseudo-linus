//! Roda só a parte H30 (diff/patch). Desenvolvimento; o resultado oficial vem do binário principal.

#[path = "../common.rs"]
mod common;
#[path = "../h30_diff/mod.rs"]
mod h30_diff;

fn main() -> anyhow::Result<()> {
    common::run_part_bin(h30_diff::run)
}
