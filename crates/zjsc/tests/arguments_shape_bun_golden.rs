//! Golden da forma do objeto `arguments` contra o JavaScriptCore do bun: `tests/golden/arguments_shape_bun.tsv` sai de
//! `scripts/gen-arguments-shape-golden.js`, rodado no bun 1.4.2 (um processo por programa). Cada linha é um programa
//! (JSON) e o texto da variável global `R` que ele grava. Cobre `arguments` em modo sloppy (mapeado) e estrito, com e sem
//! captura do parâmetro por arrow (ScopedArguments), com 0, 1 e 3 argumentos: chaves próprias, descritores de length,
//! callee e Symbol.iterator, for-in, JSON.stringify, spread, e delete, atribuição e defineProperty em length, callee,
//! Symbol.iterator, índice 0 e índice fora, com o aliasing do parâmetro.
//! O prelúdio comum das linhas fica em `tests/golden/arguments_shape.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/arguments_shape_bun.tsv");
const PRELUDES: &str = include_str!("golden/arguments_shape.preludes.json");

#[test]
fn arguments_shape_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 700, |source| common::EvalMode::IndirectEval.evaluate(source, "arguments_shape_case.js", "R"));
}
