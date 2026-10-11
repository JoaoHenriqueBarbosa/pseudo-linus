//! Golden de campos e métodos privados contra o JavaScriptCore do bun: `tests/golden/private_grid_bun.tsv` sai de
//! `scripts/gen-private-grid-golden.js`, rodado no bun. Cada linha é um programa (sufixo JSON sobre o prelúdio) e o texto
//! da variável global `R` que ele grava. Cobre `#x`, `#m()`, `get/set #a`, `static #s` e `#x in obj` em grade de operação
//! x membro x alvo (instância errada, Proxy, congelado, primitivo), return override do constructor da base (campo
//! privado em objeto alheio, em Proxy, congelado, dupla inicialização), ordem de inicialização com campos públicos,
//! privados e computados, closures, eval direto, classes aninhadas e SyntaxError de nomes privados.
mod common;

const GOLDEN: &str = include_str!("golden/private_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/private_grid.preludes.json");

#[test]
fn private_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "private_grid_case.js", "R"));
}
