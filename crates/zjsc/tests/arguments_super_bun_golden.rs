//! Golden do objeto `arguments`, de `super` em object literals, de getter/setter em literais e de inferência de `name`
//! contra o JavaScriptCore do bun: `tests/golden/arguments_super_bun.tsv` sai de `scripts/gen-arguments-super-golden.js`,
//! rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre a matriz
//! de parâmetros (simples, default, rest, destructuring, duplicados) contra operações em `arguments` mapeado e não
//! mapeado, `callee` em strict, `defineProperty` em índices, `apply` com array-like enorme, `super` em métodos,
//! getters e setters de literais com protótipo trocado, chaves de accessor, inferência de `name` por contexto e tipo
//! de função, `bind` com `new.target`, `length` e `name`, `Symbol.hasInstance` e `toString` de todas as formas.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/arguments_super_bun.tsv");
const PRELUDES: &str = include_str!("golden/arguments_super.preludes.json");

#[test]
fn arguments_super_matches_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1000, |source| {
        // O golden saiu de um bun em America/Sao_Paulo; o override é da thread, e esta roda na thread de pilha grande.
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::IndirectEval.evaluate(source, "arguments_super_case.js", "R")
    });
}
