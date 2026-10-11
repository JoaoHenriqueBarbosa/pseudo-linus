//! Golden do protocolo observável de RegExp contra o JavaScriptCore do bun: `tests/golden/regexp_protocol_bun.tsv` sai
//! de `scripts/gen-regexp-protocol-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre Symbol.match/matchAll/replace/search/split em objetos regexp-like e
//! subclasses com `exec` sobrescrito, a ordem de leitura dos getters de flags, `lastIndex` não gravável e com
//! valueOf, e `replaceAll`/`matchAll` com regexp sem a flag g.
mod common;

const GOLDEN: &str = include_str!("golden/regexp_protocol_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_protocol.preludes.json");

#[test]
fn regexp_protocol_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "regexp_protocol_case.js", "R")));
}
