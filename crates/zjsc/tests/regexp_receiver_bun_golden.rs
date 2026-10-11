//! Golden de getters e métodos de `RegExp.prototype` em receptores exóticos contra o JavaScriptCore do bun:
//! `tests/golden/regexp_receiver_bun.tsv` sai de `scripts/gen-regexp-receiver-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON, só o sufixo) e o texto da variável global `R` que ele grava. Cobre getters
//! (`flags`, `source`, `global`, ...) no próprio protótipo, em objetos comuns, subclasses e Proxy; `flags` com getters
//! individuais registrando a ordem; `toString` com `source`/`flags` exóticos; `Symbol.match`/`replace`/`search`/
//! `split`/`matchAll` com `exec` personalizado (retorno não-objeto, exceção, getter); `lastIndex` coagido, não gravável
//! e acessor; `RegExp(pattern, flags)` com pattern RegExp e `Symbol.match` falso/verdadeiro; `compile()` legado;
//! subclasses com `Symbol.species`; Proxy de RegExp com get trap.
//! O prelúdio comum das linhas fica em `tests/golden/regexp_receiver.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/regexp_receiver_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_receiver.preludes.json");

#[test]
fn regexp_receiver_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "regexp_receiver_case.js", "R")));
}
