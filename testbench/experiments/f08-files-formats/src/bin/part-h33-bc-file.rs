//! Roda só a parte H33 de bc e file. Desenvolvimento; o resultado oficial vem do binário principal.

#[path = "../common.rs"]
mod common;
#[path = "../h33_bc_file/mod.rs"]
mod h33_bc_file;

fn main() -> anyhow::Result<()> {
    common::run_part_bin(h33_bc_file::run)
}
