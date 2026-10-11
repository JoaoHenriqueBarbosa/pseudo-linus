//! Golden de módulos ES de borda contra o JavaScriptCore real: `tests/golden/module_edge_bun.tsv` sai de
//! `scripts/gen-module-edge-golden.js`, rodado no bun (formato em `tests/common/module_golden.rs`). Cobre ciclos com
//! TDZ em export, export * conflitante e ambíguo, export default anônimo e name, import.meta, import() dinâmico com
//! rejeição, top-level await com ordem entre irmãos, namespace objects (toStringTag, extensibilidade, descritores),
//! re-export de namespace e import attributes.
//!
//! Os erros da camada do bun (`BuildMessage`, `ResolveMessage`, `AggregateError`) também são comparados
//! pelo texto exato `Nome: mensagem`: o porte imita essa camada em `src/runtime/js_module_loader.rs`.
mod common;
mod zbun_transpiler;

use common::module_golden::check_module_golden;
use zjsc::api::module::LogPoint;

#[test]
fn edge_module_graphs_match_bun() {
    zbun_transpiler::install();
    check_module_golden(include_str!("golden/module_edge_bun.tsv"), 600, LogPoint::EntrySettled);
}

/// `import.meta.resolve` e `import.meta.resolveSync` (`scripts/gen-module-meta-resolve-golden.js`).
#[test]
fn import_meta_resolve_matches_bun() {
    zbun_transpiler::install();
    check_module_golden(include_str!("golden/module_meta_resolve_bun.tsv"), 200, LogPoint::EntrySettled);
}
