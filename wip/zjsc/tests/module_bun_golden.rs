//! Golden de módulos ES contra o JavaScriptCore real: `tests/golden/module_bun.tsv` sai de
//! `scripts/gen-module-golden.js`, rodado no bun (formato em `tests/common/module_golden.rs`): o array global `log`
//! (alimentado por `L(x)`) e o `Nome: mensagem` do erro que rejeitou o import, ou `null`. Os módulos rodam num loader
//! em memória (`zjsc::api::module::evaluate_module_map_until`), cada caso num realm novo.
mod common;

use common::module_golden::check_module_golden;
use zjsc::api::module::LogPoint;

/// O gerador espera um `setTimeout(0)` depois do `await import(main)` antes de imprimir o `log`.
#[test]
fn module_graphs_match_bun() {
    check_module_golden(include_str!("golden/module_bun.tsv"), 200, LogPoint::NextTimeout);
}
