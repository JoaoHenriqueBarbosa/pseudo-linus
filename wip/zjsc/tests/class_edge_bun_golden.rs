//! Golden de bordas de classes contra o JavaScriptCore do bun: `tests/golden/class_edge_bun.tsv` sai de
//! `scripts/gen-class-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre privados (#x, #m(), static #s, `#x in o`) contra receptores errados,
//! acessores privados, erros de sintaxe de classe (redeclaração, `#constructor`, `delete this.#x`, static blocks),
//! ordem de inicialização com herança e retorno de objeto no construtor base, `new.target`, `super` em métodos,
//! estáticos e literais, `extends` null/função/proxy, `Symbol.species` e `toString` de classe. Complementa
//! `class_bun_golden.rs` e `brand_bun_golden.rs`.
mod common;

const GOLDEN: &str = include_str!("golden/class_edge_bun.tsv");

#[test]
fn class_edges_match_bun() {
    common::run_golden(GOLDEN, 1200, |source| common::EvalMode::IndirectEval.evaluate(source, "class_edge_case.js", "R"));
}
