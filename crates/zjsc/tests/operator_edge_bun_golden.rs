//! Golden de operadores de borda contra o JavaScriptCore do bun: `tests/golden/operator_edge_bun.tsv` sai de
//! `scripts/gen-operator-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `==`/`===` entre pares de tipos (null, undefined, BigInt, Symbol, wrappers),
//! relacionais com BigInt e string numérica, `+` com objetos e Date, typeof/void/in/instanceof com
//! `Symbol.hasInstance`, ++/-- em string/BigInt/objeto, ordem de avaliação em `a[b()] = c()`, compound assignment
//! com getters/setters e Proxy, exponenciação, shifts e bitwise com BigInt, Object.is, SameValueZero, coerção de
//! chave de propriedade e ToPrimitive com hints e erros (os hooks registram a ordem de chamada).
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/operator_edge_bun.tsv");

#[test]
fn operator_edge_cases_match_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    // O programa pode falhar na compilação (early error): o bun então deixa `R` indefinido e o golden registra isso,
    // por isso a exceção do script não encerra a medição.
    common::run_golden(GOLDEN, 1000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
