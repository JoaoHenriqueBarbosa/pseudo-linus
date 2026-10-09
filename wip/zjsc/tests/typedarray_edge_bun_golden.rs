//! Golden de borda de TypedArray e DataView contra o JavaScriptCore do bun: `tests/golden/typedarray_edge_bun.tsv` sai
//! de `scripts/gen-typedarray-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre `set` com overlap, `subarray`, `slice`, `fill`, `copyWithin`,
//! `sort` com NaN e -0, `toSorted`/`toReversed`/`with`, `findLast`, `includes`/`indexOf` com NaN, `join`, `from`/`of`,
//! conversões entre tipos (Uint8Clamped, Float16Array, BigInt64), `DataView` com endianness e bordas, `Math.f16round`,
//! buffers redimensionáveis e destacados, e as mensagens de erro exatas.
mod common;

const GOLDEN: &str = include_str!("golden/typedarray_edge_bun.tsv");

#[test]
fn typedarray_edge_matches_bun() {
    common::run_golden(GOLDEN, 500, |source| common::EvalMode::IndirectEval.evaluate(source, "typedarray_edge_case.js", "R"));
}
