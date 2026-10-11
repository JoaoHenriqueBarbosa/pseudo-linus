//! Golden de escopo e funções contra o JavaScriptCore do bun: `tests/golden/function_scope_bun.tsv` sai de
//! `scripts/gen-function-scope-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. Cobre hoisting de `var`/`function`/`let`/`class`, declarações duplicadas por
//! escopo, parâmetros padrão com escopo próprio, `arguments` mapeado e não mapeado, `name`/`length`, funções em blocos
//! (Annex B), `new.target`, closures em laços com `let`, IIFEs, recursão mútua, `Function.prototype.toString`, o
//! construtor `Function` e as mensagens exatas de erro via `Function`/`eval`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/function_scope_bun.tsv");
const PRELUDES: &str = include_str!("golden/function_scope.preludes.json");

#[test]
fn function_scope_matches_bun() {
    // A recursão de milhares de níveis (`f(5000)`) que o bun aceita pede a thread de pilha grande, como os outros goldens
    // de recursão funda. O golden saiu de um bun em America/Sao_Paulo; o fuso é por thread, então é fixado nela.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| {
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::IndirectEval.evaluate(source, "function_scope_case.js", "R")
    });
}
