//! Golden dos construtores de erro contra o JavaScriptCore do bun: `tests/golden/error_ctor_bun.tsv` sai de
//! `scripts/gen-error-ctor-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre `Error`, `TypeError`, `RangeError`, `SyntaxError`, `ReferenceError`,
//! `EvalError`, `URIError` e `AggregateError` com e sem `new`, `message` de tipos variados, `options.cause` (getter,
//! `has` contra `undefined`, `Proxy`), iteráveis exóticos no `AggregateError`, subclasses, `newTarget` exótico,
//! `Error.prototype.toString` com `name` e `message` exóticos e receptores inválidos, e os descritores. O resultado nunca
//! inclui `stack`.
mod common;

const GOLDEN: &str = include_str!("golden/error_ctor_bun.tsv");
const PRELUDES: &str = include_str!("golden/error_ctor.preludes.json");

#[test]
fn error_ctor_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "error_ctor_case.js", "R"));
}
