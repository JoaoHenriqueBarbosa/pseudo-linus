//! Roda só a parte H31 (compressão e arquivos). Desenvolvimento; o resultado oficial vem do binário principal.

#[path = "../common.rs"]
mod common;
#[path = "../h31_archive/mod.rs"]
mod h31_archive;

fn main() -> anyhow::Result<()> {
    common::run_part_bin(h31_archive::run)
}
