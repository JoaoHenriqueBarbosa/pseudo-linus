//! Roda só a parte H33 de yq e csv. Desenvolvimento; o resultado oficial vem do binário principal.

#[path = "../common.rs"]
mod common;
#[path = "../h33_yq_csv/mod.rs"]
mod h33_yq_csv;

fn main() -> anyhow::Result<()> {
    common::run_part_bin(h33_yq_csv::run)
}
