//! Golden de Symbol, chaves fracas, ordem de chaves, `defineProperty` em arrays e `Symbol.species` contra o
//! JavaScriptCore do bun: `tests/golden/symbol_species_bun.tsv` sai de `scripts/gen-symbol-species-golden.js`, rodado no
//! bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre coerções de Symbol,
//! `Symbol.toPrimitive`, propriedades por símbolo, símbolos como chave de WeakMap/WeakSet/WeakRef/FinalizationRegistry,
//! ordem de `Object.keys`/`entries`/`fromEntries`, `defineProperty` em arrays, os getters `Symbol.species` e subclasses
//! com species (Array, Promise, RegExp, ArrayBuffer, TypedArray).
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/symbol_species_bun.tsv");
const PRELUDES: &str = include_str!("golden/symbol_species.preludes.json");

#[test]
fn symbol_species_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1000, |source| {
        // O golden saiu de um bun em America/Sao_Paulo; o override é da thread, e esta roda na thread de pilha grande.
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::IndirectEval.evaluate(source, "symbol_species_case.js", "R")
    });
}
