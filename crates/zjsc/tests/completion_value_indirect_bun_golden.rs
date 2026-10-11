//! Golden do valor de completude de eval indireto `(0, eval)(fonte)` contra o JavaScriptCore do bun:
//! `tests/golden/completion_value_indirect_bun.tsv` sai de `scripts/gen-completion-value-indirect-golden.js`, rodado no
//! bun 1.4.2. Cada fonte combina de 2 a 4 statements (if/else, laços com break/continue rotulados, switch com
//! fallthrough, try/catch/finally com e sem break no finally, blocos vazios, declarações, with, labels aninhados).
//! O prelúdio comum das linhas fica em `tests/golden/completion_value_indirect.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/completion_value_indirect_bun.tsv");
const PRELUDES: &str = include_str!("golden/completion_value_indirect.preludes.json");

#[test]
fn completion_value_indirect_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "completion_value_indirect_case.js", "R")));
}
