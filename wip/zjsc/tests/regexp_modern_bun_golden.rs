//! Golden de RegExp moderno contra o JavaScriptCore do bun: `tests/golden/regexp_modern_bun.tsv` sai de
//! `scripts/gen-regexp-modern-golden.js`. Cada linha é um programa (JSON, várias linhas) e o texto da variável global
//! `R` que ele grava. Cobre a flag `v` (notação de conjuntos, `\q{}`, propriedades de string, subtração e interseção,
//! erros de sintaxe), a flag `d` (indices e groups em indices), grupos nomeados duplicados em alternativas, lookbehind
//! com referências de volta, `\p{}` com Script/Script_Extensions/General_Category em grade astral, case-insensitive com
//! `u` e `v`, sticky com `lastIndex` em grade, `Symbol.replace` com `$<name>`, `$&`, `` $` `` e `$'`, `RegExp.escape` e
//! modificadores `(?i:)`.
//! O prelúdio comum das linhas fica em `tests/golden/regexp_modern.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/regexp_modern_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_modern.preludes.json");

#[test]
fn regexp_modern_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "regexp_modern_case.js", "R")));
}
