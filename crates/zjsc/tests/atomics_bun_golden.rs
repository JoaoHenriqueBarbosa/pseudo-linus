//! Golden de Atomics e SharedArrayBuffer contra o JavaScriptCore do bun: `tests/golden/atomics_bun.tsv` sai de
//! `scripts/gen-atomics-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre todas as operações de `Atomics` em todos os tipos inteiros (inclusive BigInt64 e
//! BigUint64), em SharedArrayBuffer e ArrayBuffer comum, índices e valores que pedem coerção, wrap-around,
//! `wait`/`waitAsync`/`notify` sem bloquear, `pause`, `isLockFree`, as mensagens exatas de erro e o
//! SharedArrayBuffer (construtor, `slice`, `grow`, species, `structuredClone`).
mod common;

const GOLDEN: &str = include_str!("golden/atomics_bun.tsv");
const PRELUDES: &str = include_str!("golden/atomics.preludes.json");

#[test]
fn atomics_match_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 4000, |source| common::EvalMode::IndirectEval.evaluate(source, "atomics_case.js", "R"));
}
