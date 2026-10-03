//! Linha de base do allocator interno alternativo: mimalloc puro, sem contabilidade.

use e07_mem_accounting::accounting::NoAccounting;
use e07_mem_accounting::cli;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    cli::run(&NoAccounting { name: "mimalloc" }, |_| None);
}
