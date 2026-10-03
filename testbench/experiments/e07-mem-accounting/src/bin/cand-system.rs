//! Linha de base: o System allocator (malloc da glibc), sem contabilidade.
//!
//! Também hospeda as demonstrações que independem de candidato: o pedido gigante que aborta o host
//! mesmo sem limite nenhum, e a contabilidade explícita do kernel sem escopo de exclusão.

use e07_mem_accounting::accounting::NoAccounting;
use e07_mem_accounting::{cli, demos};

fn main() {
    cli::run(&NoAccounting { name: "system" }, |a| match a.cmd.as_str() {
        "huge-alloc" => Some(demos::huge_alloc(false)),
        "huge-alloc-try" => Some(demos::huge_alloc(true)),
        _ => None,
    });
}
