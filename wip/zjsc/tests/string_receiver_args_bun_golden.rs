//! Golden de receptores e argumentos dos métodos básicos de `String.prototype` contra o JavaScriptCore do bun:
//! `tests/golden/string_receiver_args_bun.tsv` sai de `scripts/gen-string-receiver-args-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON, várias linhas) e o texto da variável global `R` que ele grava. Cobre receptor
//! undefined/null (TypeError), números, objetos com toString/valueOf/@@toPrimitive registrando a ordem, Symbol, BigInt,
//! String boxed e arrays; e argumentos undefined/NaN/-0/Infinity/negativos/fracionários/strings numéricas/objetos/
//! Symbol/BigInt, além de RegExp passada a includes/startsWith/endsWith, inclusive com `Symbol.match` falso.
//! O prelúdio comum das linhas fica em `tests/golden/string_receiver_args.preludes.json` (ver `tests/common/mod.rs`).

mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/string_receiver_args_bun.tsv");
const PRELUDES: &str = include_str!("golden/string_receiver_args.preludes.json");

#[test]
fn string_receiver_args_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "string_receiver_args_case.js", "R")));
}
