//! Golden de borda de Array contra o JavaScriptCore do bun: `tests/golden/array_edge_bun.tsv` sai de
//! `scripts/gen-array-edge-golden.js`, rodado no bun 1.4.2, e complementa `array_bun_golden.rs` (os programas que já
//! estão em `array_bun.tsv` são descartados). Cada linha é um programa (JSON, várias linhas, com o prelúdio
//! `S`/`T`/`D`) e o texto da variável global `R` que ele grava.
//! Cobre `splice` e `copyWithin` em array-likes de `length` gigante, `flat`/`flatMap` com proxies e buracos, `sort`
//! estável com comparadores inconsistentes, `toSorted`/`toSpliced`/`toReversed`/`with` (limites e `RangeError`),
//! `find`/`findLast`, `at`, `includes` com `NaN` e `-0`, species, buracos com índices herdados do protótipo,
//! `Symbol.isConcatSpreadable` e `Array.from`/`Array.of` com iterables, `mapFn` e fechamento do iterador.
//! `Array.fromAsync` fica de fora.
mod common;

const GOLDEN: &str = include_str!("golden/array_edge_bun.tsv");

const PRELUDES: &str = include_str!("golden/array_edge.preludes.json");

#[test]
fn array_edge_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 580, "array_edge_case.js");
}
