//! Golden de `DataView` e `ArrayBuffer` em grade contra o JavaScriptCore do bun: `tests/golden/dataview_bun.tsv` sai de
//! `scripts/gen-dataview-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre get/set de todos os tipos (com `Float16` e `BigInt`) contra offsets e
//! `littleEndian` esquisitos e valores extremos (NaN com payload, `-0`, estouro), o construtor com offset e length
//! inválidos, `DataView` sobre buffer detached ou redimensionado (length-tracking), getters em receptores inválidos,
//! `ArrayBuffer` com `maxByteLength`, `isView`, species de `slice`, limites de `resize`/`transfer` e `@@toStringTag`.
//! Cada programa roda num filho novo do bun por `(0, eval)(fonte)`, e o teste usa `EvalMode::IndirectEval` e lê `R` logo depois.
mod common;

const GOLDEN: &str = include_str!("golden/dataview_bun.tsv");
const PRELUDES: &str = include_str!("golden/dataview.preludes.json");

#[test]
fn dataview_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "dataview_case.js", "R"));
}
