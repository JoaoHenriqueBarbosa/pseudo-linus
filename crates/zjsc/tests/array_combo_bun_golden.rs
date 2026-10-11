//! Golden de combinatórias de Array contra o JavaScriptCore do bun: `tests/golden/array_combo_bun.tsv` sai de
//! `scripts/gen-array-combo-golden.js`, rodado no bun 1.4.2, e complementa `array_bun_golden.rs`,
//! `array_edge_bun_golden.rs` e `array_more_bun_golden.rs` (os programas que já estão neles são descartados). Cada
//! linha é um programa (JSON, várias linhas, com o prelúdio `S`/`T`/`D` fatorado) e o texto da variável global `R` que ele grava.
//! Cobre `sort`/`toSorted` com comparadores inconsistentes (NaN, boolean, símbolo, mutação do array, buracos, getters),
//! estabilidade, arrays esparsos e array-likes, `concat`/`slice`/`splice`/`map`/`filter` com
//! `Symbol.isConcatSpreadable` e species, `indexOf`/`lastIndexOf`/`includes` com `-0`, `NaN` e `fromIndex` extremo,
//! `join`/`toString` com ciclos, `flat`/`flatMap` com depth extremo, `Array.from` e `Array.of` com `this` variado, o
//! setter de `length`, buracos nos métodos de iteração e arrays com 2**32-1 elementos virtuais.
//! O prelúdio comum das linhas fica em `tests/golden/array_combo.preludes.json` (ver `tests/common/mod.rs`).
mod common;

const GOLDEN: &str = include_str!("golden/array_combo_bun.tsv");

const PRELUDES: &str = include_str!("golden/array_combo.preludes.json");

#[test]
fn array_combo_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1500, "array_combo_case.js");
}
