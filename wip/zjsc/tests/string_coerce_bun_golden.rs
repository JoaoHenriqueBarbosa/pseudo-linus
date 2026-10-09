//! Golden de coerção de argumentos dos métodos de `String.prototype` contra o JavaScriptCore do bun:
//! `tests/golden/string_coerce_bun.tsv` sai de `scripts/gen-string-coerce-golden.js`. Cada linha é um programa
//! (sufixo JSON) e o texto da variável global `R` que ele grava. Cobre at, charAt, charCodeAt, codePointAt, slice,
//! substring, substr, padStart, padEnd, repeat, indexOf, lastIndexOf, includes, startsWith, endsWith, normalize,
//! localeCompare, isWellFormed e toWellFormed, com receptores (surrogates soltos, vazios, longos, null/undefined,
//! números, objetos com toString) e argumentos (-0, NaN, infinitos, 2**53, fracionários, strings numéricas, objetos
//! com valueOf que registram a ordem ou lançam, Symbol, BigInt).
//! O prelúdio comum fica em `tests/golden/string_coerce.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/string_coerce_bun.tsv");
const PRELUDES: &str = include_str!("golden/string_coerce.preludes.json");

#[test]
fn string_coerce_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "string_coerce_case.js", "R"));
}
