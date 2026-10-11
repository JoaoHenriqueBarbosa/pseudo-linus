//! Golden de conversão exata entre string e número contra o JavaScriptCore do bun: `tests/golden/number_convert_bun.tsv`
//! sai de `scripts/gen-number-convert-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `Number()`/`parseFloat`/unário `+` em strings de fronteira (meios-termos exatos entre
//! doubles, 17+ dígitos, expoentes enormes, subnormais, `Infinity` com espaços Unicode, prefixos 0x/0b/0o, separadores
//! `_`), `parseInt` com radix 2..36 e inválidos, `toString(radix)` de frações em todos os radix e
//! `toFixed`/`toExponential`/`toPrecision` com dígitos 0..100 e o RangeError fora da faixa.
mod common;

const GOLDEN: &str = include_str!("golden/number_convert_bun.tsv");
const PRELUDES: &str = include_str!("golden/number_convert.preludes.json");

#[test]
fn number_convert_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "number_convert_case.js", "R"));
}
