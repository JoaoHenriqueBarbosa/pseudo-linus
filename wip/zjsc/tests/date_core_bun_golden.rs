//! Golden do núcleo de Date contra o JavaScriptCore do bun: `tests/golden/date_core_bun.tsv` sai de
//! `scripts/gen-date-core-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava. Cobre o construtor com 0..7 argumentos, valores extremos e NaN, strings ISO, `Date.UTC`,
//! setters locais e UTC, getters UTC, limites de ±8.64e15, anos negativos e acima de 9999, serialização,
//! `Symbol.toPrimitive` com hints inválidos, `Date.now` e aritmética. Só entram programas com resultado idêntico em
//! cinco fusos, então o teste não depende do fuso da máquina.
mod common;


const GOLDEN: &str = include_str!("golden/date_core_bun.tsv");

#[test]
fn date_core_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 1500, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
