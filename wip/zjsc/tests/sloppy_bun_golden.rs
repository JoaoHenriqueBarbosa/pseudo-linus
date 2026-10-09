//! Golden de modo sloppy legado e hoisting contra o JavaScriptCore do bun: `tests/golden/sloppy_bun.tsv` sai de
//! `scripts/gen-sloppy-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava. Cobre `with` (Symbol.unscopables, proxies, delete, typeof, chamadas com this), labels e
//! break/continue rotulados, function declarations em blocos (Annex B), var vs let em switch, arguments e var-scoping
//! de eval, `this` em sloppy e strict, getters em globalThis, delete de binding, octais legados e escapes, HTML
//! comments, `__proto__` e nomes reservados em sloppy.
mod common;


const GOLDEN: &str = include_str!("golden/sloppy_bun.tsv");

#[test]
fn sloppy_legacy_and_hoisting_match_bun() {
    // O programa pode falhar na compilação (early error): o bun então deixa `R` indefinido e o golden registra isso,
    // por isso a exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 1500, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
