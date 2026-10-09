//! Golden de gerenciamento explícito de recursos contra o JavaScriptCore do bun: `tests/golden/dispose_bun.tsv` sai de
//! `scripts/gen-dispose-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava, depois de esvaziadas as microtarefas. Cobre `DisposableStack`, `AsyncDisposableStack`,
//! `SuppressedError`, `Symbol.dispose` e `Symbol.asyncDispose` (use, adopt, defer, move, dispose, ordem LIFO, erros
//! suprimidos encadeados, getter `disposed`, mensagens de TypeError e ReferenceError), a sintaxe `using` e
//! `await using`, o `Iterator.prototype[Symbol.dispose]` e os descritores de `Symbol.iterator` em Array, Map, Set,
//! String e arrays tipados.
mod common;


const GOLDEN: &str = include_str!("golden/dispose_bun.tsv");

#[test]
fn explicit_resource_management_matches_bun() {
    // O programa pode falhar na compilação (SyntaxError de `using`): o bun então deixa `R` indefinido e o golden
    // registra isso, por isso a exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 400, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
