//! Roda só a parte H32 (date). Desenvolvimento; o resultado oficial vem do binário principal.

#[path = "../common.rs"]
mod common;
#[path = "../h32_date/mod.rs"]
mod h32_date;

fn main() -> anyhow::Result<()> {
    common::run_part_bin(h32_date::run)
}
