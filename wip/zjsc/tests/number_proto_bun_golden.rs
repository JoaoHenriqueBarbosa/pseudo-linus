//! Golden de `Number.prototype` contra o JavaScriptCore do bun: `tests/golden/number_proto_bun.tsv` sai de
//! `scripts/gen-number-proto-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `toFixed`, `toPrecision`, `toExponential` e `toString(radix)` em grades de valor por
//! argumento (metades, expoentes, limites, `RangeError`), receptor inválido, coerção do argumento e constantes de `Number`.
mod common;

const GOLDEN: &str = include_str!("golden/number_proto_bun.tsv");
const PRELUDES: &str = include_str!("golden/number_proto.preludes.json");

#[test]
fn number_proto_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 800, |source| common::EvalMode::IndirectEval.evaluate(source, "number_proto_case.js", "R"));
}
