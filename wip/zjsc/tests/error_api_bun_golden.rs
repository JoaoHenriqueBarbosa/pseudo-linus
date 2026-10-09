//! Golden da API avançada de Error contra o JavaScriptCore do bun: `tests/golden/error_api_bun.tsv` sai de
//! `scripts/gen-error-api-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `Error.captureStackTrace` (objetos variados, constructorOpt, congelados),
//! `Error.stackTraceLimit`, `Error.prepareStackTrace` com todos os métodos de CallSite, a propriedade `stack`,
//! o formato de frames por contexto, `cause`, AggregateError, SuppressedError, `Error.prototype.toString` exótico,
//! coerção da mensagem, Symbol em message, subclasses e name. Os programas normalizam o texto de stack sozinhos
//! (`x.js:L:C`, frames do hospedeiro descartados), então o nome do arquivo do script é `x.js`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/error_api_bun.tsv");

#[test]
fn error_api_matches_bun() {
    // Um programa que não grava `R` (ou falha na compilação) deixa `R` indefinido, e o golden registra isso.
    common::run_golden_big_stack(GOLDEN, 800, |source| {
        // O golden saiu de um bun em America/Sao_Paulo; o override é da thread, e esta roda na thread de pilha grande.
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::RunInThisContext.evaluate(source, "x.js", "R")
    });
}
