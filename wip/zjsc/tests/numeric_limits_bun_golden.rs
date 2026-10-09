//! Golden de limites numéricos contra o JavaScriptCore do bun: `tests/golden/numeric_limits_bun.tsv` sai de
//! `scripts/gen-numeric-limits-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `Number.prototype.toString(radix)` com fracionários em radix 2..36,
//! `toFixed`/`toPrecision`/`toExponential` nos limites (1e21, 1e-7, 0.5, -0), `parseFloat`/`parseInt` com lixo, hex,
//! octal e separadores, `BigInt.asIntN`/`asUintN` com bits grandes, `BigInt` toString(radix) e de strings, aritmética
//! BigInt (`**`, `%`, `>>`, divisão por zero, mistura com Number) e `Math.round`/`fround`/`clz32`/`hypot`/`expm1`/
//! `log1p`/`cbrt`/`sumPrecise` nas bordas.
mod common;

const GOLDEN: &str = include_str!("golden/numeric_limits_bun.tsv");
const PRELUDES: &str = include_str!("golden/numeric_limits.preludes.json");

#[test]
fn numeric_limits_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1000, |source| common::EvalMode::IndirectEval.evaluate(source, "numeric_limits_case.js", "R"));
}
