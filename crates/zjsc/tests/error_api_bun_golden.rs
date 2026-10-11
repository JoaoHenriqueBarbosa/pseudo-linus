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

/// Frames de função nativa no `stack`, medidos no bun 1.4.2: `Function.prototype.call` aparece como `at call (unknown:1:11)` (o motor
/// diz `native:1:11`, que é como o bun mostra `Array.prototype.map`), e o `super()` de uma classe que estende `null` deixa o frame
/// `at unknown` que o motor não tem. Texto de frame de pilha, sem vazamento nem comportamento: backlog no `PLAN.md`.
const CAUSE: &str = "texto do frame de função nativa no stack (unknown contra native, frame sem nome do super())";
const KNOWN: &[common::KnownCase] = &[common::KnownCase { label: "454", cause: CAUSE }, common::KnownCase { label: "465", cause: CAUSE }];

#[test]
fn error_api_matches_bun() {
    // Um programa que não grava `R` (ou falha na compilação) deixa `R` indefinido, e o golden registra isso.
    common::run_factored_big_stack_known_lines(GOLDEN, common::NO_PRELUDES, 800, KNOWN, |source| {
        // O golden saiu de um bun em America/Sao_Paulo; o override é da thread, e esta roda na thread de pilha grande.
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::RunInThisContext.evaluate(source, "x.js", "R")
    });
}
