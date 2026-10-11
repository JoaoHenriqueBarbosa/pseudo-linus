//! Segundo golden de módulos ES contra o JavaScriptCore real: `tests/golden/module_more_bun.tsv` sai de
//! `scripts/gen-module-more-golden.js`, rodado no bun (formato em `tests/common/module_golden.rs`). Cobre export
//! default anônimo, export * (as), ciclos com TDZ e hoisting, namespace objects, re-export ambíguo, binding
//! inexistente, live bindings, import.meta, import() dinâmico, top-level await, strict, import attributes e erros de
//! parse.
mod common;
mod zbun_transpiler;

use common::module_golden::check_module_golden;
use zjsc::api::module::LogPoint;

#[test]
fn more_module_graphs_match_bun() {
    zbun_transpiler::install();
    check_module_golden(include_str!("golden/module_more_bun.tsv"), 1000, LogPoint::EntrySettled);
}
