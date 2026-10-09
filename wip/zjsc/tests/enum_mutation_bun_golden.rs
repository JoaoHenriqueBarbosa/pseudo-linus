//! Golden de enumeração com mutação do próprio objeto contra o JavaScriptCore do bun: `tests/golden/enum_mutation_bun.tsv`
//! sai de `scripts/gen-enum-mutation-golden.js`, rodado no bun 1.4.2 (um processo por programa). Cada linha é um programa
//! (JSON) e o texto da variável global `R` que ele grava. Cobre JSON.stringify e Object.keys/values/entries/assign/
//! fromEntries/spread/for-in sobre objetos cujo getter adiciona, remove, esconde ou redefine chaves ainda não visitadas e
//! já visitadas (objeto comum, array, typed array fixo e redimensionável, String, arguments, instância de classe, função,
//! Proxy sem traps e com traps que registram a ordem), Proxies cujo ownKeys e getOwnPropertyDescriptor mentem (TypeError
//! de invariante exato) e o for-in com mutação no corpo do laço, inclusive do protótipo no meio da enumeração.
//! O prelúdio comum das linhas fica em `tests/golden/enum_mutation.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/enum_mutation_bun.tsv");
const PRELUDES: &str = include_str!("golden/enum_mutation.preludes.json");

#[test]
fn enum_mutation_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "enum_mutation_case.js", "R"));
}
