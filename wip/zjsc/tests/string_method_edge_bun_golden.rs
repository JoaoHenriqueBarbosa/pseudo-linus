//! Golden de bordas de `String.prototype` e dos wrappers Number/Boolean/String contra o JavaScriptCore do bun:
//! `tests/golden/string_method_edge_bun.tsv` sai de `scripts/gen-string-method-edge-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre índices NaN/negativos/
//! Infinity/fracionários/-0, indexOf/lastIndexOf, split com limite, repeat e padStart/padEnd, concat, trim com
//! whitespaces Unicode, localeCompare/normalize, fromCharCode/fromCodePoint, template literal e String.raw,
//! startsWith/endsWith/includes com RegExp, métodos HTML, wrappers, comparação com surrogates e radix/exponenciais.
mod common;


const GOLDEN: &str = include_str!("golden/string_method_edge_bun.tsv");

#[test]
fn string_method_edge_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 500, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
