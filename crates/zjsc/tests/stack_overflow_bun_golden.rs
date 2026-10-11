//! Golden de estouro de pilha contra o JavaScriptCore do bun: `tests/golden/stack_overflow_bun.tsv` sai de
//! `scripts/gen-stack-overflow-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava, depois de esvaziadas as microtarefas. O programa captura o erro e devolve só
//! `name: message` (`RangeError: Maximum call stack size exceeded.`), nunca a profundidade. Cobre recursão infinita e
//! profundidades seguras (5 e 1000) via getters, setters, toString, valueOf, Symbol.toPrimitive, traps de Proxy, JSON
//! (toJSON, replacer, reviver), comparador de sort, callbacks, apply/call/bind encadeados, new e classes, geradores e
//! async recursivos, RegExp com callback e exec customizado, cadeias de closures e estruturas aninhadas grandes.
mod common;

const GOLDEN: &str = include_str!("golden/stack_overflow_bun.tsv");

#[test]
fn stack_overflow_matches_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 800, |source| common::EvalMode::IndirectEvalDrained.evaluate(source, "stack_overflow_case.js", "R"));
}
