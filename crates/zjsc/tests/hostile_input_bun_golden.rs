//! Golden de entradas JS hostis contra o JavaScriptCore do bun: `tests/golden/hostile_input_bun.tsv` sai de
//! `scripts/gen-hostile-input-golden.js`, rodado no bun 1.4.2. Cada linha é um sufixo de programa (JSON) e o texto da
//! variável global `R` que ele grava (`ok:<valor>` ou `throw:<name>:<message>`, mais o log das traps de um array-like
//! gigante); o prelúdio fatorado fica em `tests/golden/hostile_input.preludes.json`. Cobre comprimentos 2^32-1, 2^32, 2^53,
//! `NaN` e `-0` em `Array`, `TypedArray`, `ArrayBuffer` e `String`, métodos genéricos de `Array.prototype` sobre array-likes
//! com `length` gigante, `@@species` hostil, `Proxy` com traps que lançam, `toString`/`valueOf` que lançam ou mutam o
//! receptor, recursão profunda e infinita e `RegExp` com retrocesso pesado. O que importa: onde o JSC lança um erro, o porte
//! lança o mesmo erro e nunca entra em `panic!`.
mod common;

const GOLDEN: &str = include_str!("golden/hostile_input_bun.tsv");
const PRELUDES: &str = include_str!("golden/hostile_input.preludes.json");

#[test]
fn hostile_input_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 250, |source| common::EvalMode::IndirectEval.evaluate(source, "hostile_input_case.js", "R"));
}
