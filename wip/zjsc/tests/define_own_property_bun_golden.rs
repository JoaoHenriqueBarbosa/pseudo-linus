//! Golden de `[[DefineOwnProperty]]` contra o JavaScriptCore do bun: `tests/golden/define_own_property_bun.tsv` sai de
//! `scripts/gen-define-own-property-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre `ValidateAndApplyPropertyDescriptor` (transições dado e acessor,
//! `configurable` falso, `writable` verdadeiro para falso, `SameValue` com `NaN` e `-0`, descritor parcial e com campos
//! herdados), arrays (índice, `length`, 2**32-2), `arguments` mapeado, objetos `String`, typed arrays, funções, `globalThis`,
//! `Proxy` com a trap `defineProperty` e seus invariantes, `freeze`/`seal`/`isFrozen` em exóticos e a ordem de efeitos
//! de `Object.defineProperties`.
mod common;

const GOLDEN: &str = include_str!("golden/define_own_property_bun.tsv");
const PRELUDES: &str = include_str!("golden/define_own_property.preludes.json");

#[test]
fn define_own_property_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "define_own_property_case.js", "R"));
}
