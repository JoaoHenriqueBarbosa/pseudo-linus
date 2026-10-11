//! Golden de generators síncronos contra o JavaScriptCore do bun: `tests/golden/generator_state_bun.tsv` sai de
//! `scripts/gen-generator-state-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. Cobre `next`/`return`/`throw` em cada estado (suspendedStart, suspendedYield,
//! executing com reentrância, completed), `yield` dentro de `try`/`catch`/`finally` com `return()` e `throw()` injetados,
//! `yield*` para iteradores com e sem `return`/`throw` (o `TypeError` quando `throw` falta e o fechamento), argumentos de
//! `next`, generator methods, computed e static, generator como construtor, `prototype` de generator function, `this`,
//! `arguments`, recursão via `yield*` e as mensagens exatas ("Generator is already running" e companhia).
mod common;

const GOLDEN: &str = include_str!("golden/generator_state_bun.tsv");
const PRELUDES: &str = include_str!("golden/generator_state.preludes.json");

#[test]
fn generator_state_matches_bun() {
    // O caso do `yield*` recursivo de 1000 níveis aninha o laço nativo a cada nível: numa thread de teste comum (2 MiB)
    // estoura a pilha nativa, que no bun (8 MiB abaixo da pilha de 5 MiB do JS) não é o limite. Os goldens de profundidade
    // rodam na thread de pilha grande.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "generator_state_case.js", "R"));
}
