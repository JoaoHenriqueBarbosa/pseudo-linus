//! Golden de `import()` num script avulso, sem host de módulos, contra o bun 1.4.2:
//! `tests/golden/import_no_host_bun.tsv` sai de `scripts/gen-import-no-host-golden.js`. A rejeição é um
//! `ResolveMessage` (`Cannot find module './x.js' imported from /main.js`, `Cannot find package 'x' ...`,
//! `No such built-in module: node:x`), nunca o texto de um carregador ausente. O referrer é o caminho neutro
//! `/main.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/import_no_host_bun.tsv");

#[test]
fn import_without_a_module_host_rejects_like_bun() {
    common::run_golden_big_stack(GOLDEN, 25, |source| common::EvalMode::Script.evaluate(source, "main.js", "R"));
}
