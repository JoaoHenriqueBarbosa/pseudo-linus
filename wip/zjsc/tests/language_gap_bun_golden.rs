//! Golden dos dois maiores buracos de cobertura da linguagem contra o JavaScriptCore do bun:
//! `tests/golden/language_gap_bun.tsv` sai de `scripts/gen-language-gap-golden.js`, rodado no bun 1.4.2. Cada linha é
//! um programa (JSON) e o texto da variável global `R` que ele grava: o log de efeitos (`L`, na ordem em que
//! aconteceram) seguido de ` => ` e o resultado serializado, ou `T:Erro: mensagem` quando lançou. Cobre operadores
//! (ordem de coerção, BigInt misto, Symbol, atribuição composta e lógica em membro, update, optional chaining) e
//! destructuring (arrays e objetos em declaração, atribuição, parâmetro, for-of e catch, IteratorClose, defaults,
//! chaves computadas, rest, parâmetros default, `arguments`, escopo de parâmetros).
mod common;

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/language_gap_bun.tsv");

#[test]
fn operators_and_destructuring_match_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_golden(GOLDEN, 1000, |source| evaluate_named_script_result(source, "language_gap_case.js", "R"));
}
