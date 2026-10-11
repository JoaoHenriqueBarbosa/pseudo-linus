//! Golden de conversões e coerção por semântica contra o JavaScriptCore do bun:
//! `tests/golden/coercion_semantics_bun.tsv` sai de `scripts/gen-coercion-semantics-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre ToPrimitive com hint e
//! ordem de chamada de `valueOf`/`toString`/`Symbol.toPrimitive` em todos os operadores, ordem de avaliação dos
//! operandos com efeitos colaterais, `==` entre tipos, comparação BigInt/Number/String, `ToPropertyKey`, `ToObject`,
//! templates, `Number()`/`parseFloat`/`parseInt` exóticos, `Number.prototype.toString(radix)`, `toFixed`,
//! `toPrecision`, `toExponential`, `String(Symbol)` e mensagens exatas de `TypeError`.
//! Complementa `coercion_bun_golden.rs` (matriz de valores x operadores), sem repeti-lo.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/coercion_semantics_bun.tsv");

#[test]
fn coercion_semantics_match_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 3000, |source| {
        // O golden saiu de um bun em America/Sao_Paulo; o override é da thread, e esta roda na thread de pilha grande.
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::RunInThisContext.evaluate(source, "", "R")
    });
}
