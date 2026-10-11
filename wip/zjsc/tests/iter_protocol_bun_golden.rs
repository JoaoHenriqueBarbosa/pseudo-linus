//! Golden do protocolo de iteração observável nos built-ins consumidores contra o JavaScriptCore do bun:
//! `tests/golden/iter_protocol_bun.tsv` sai de `scripts/gen-iter-protocol-golden.js`. Cobre `Array.from`, construtores de
//! Map/Set/WeakMap/WeakSet, `Promise.all/allSettled/any/race`, `Object.fromEntries`, `TypedArray.from`, spread,
//! desestruturação, for-of e `yield*` sobre iteradores cujo `next`/`return`/`throw` lançam ou devolvem não objeto, com
//! getters de `done`/`value`, `Symbol.iterator` devolvendo primitivo e `%ArrayIteratorPrototype%.next` sobrescrito; o log
//! registra a ordem exata dos acessos. O gerador avalia o programa inteiro com `(0, eval)(fonte)` num bun filho, sem
//! arquivo nem transpilação; o teste faz o mesmo com o `eval` indireto, e o valor de completude é o `globalThis.R = ...`.
mod common;

const GOLDEN: &str = include_str!("golden/iter_protocol_bun.tsv");
const PRELUDES: &str = include_str!("golden/iter_protocol.preludes.json");

/// O erro do spread de `null` ou `undefined` (`[...X]`) no bun leva o trecho do fonte da instrução depois da mensagem
/// (`(evaluating '...X')`); no zjsc a mensagem sai sem o sufixo. Divergência de texto (a exceção, o tipo e o log de
/// acessos batem), medida no bun 1.4.2 e declarada em vez de perseguida: 9 linhas para cada um dos dois valores.
const KNOWN: &[common::KnownDivergence] = &[
    common::KnownDivergence {
        cause: "mensagem do spread de undefined sem o trecho do fonte",
        expected: "\"get iter,call iter0 => throw TypeError: undefined is not an object (evaluating '...X')\"",
        got: "\"get iter,call iter0 => throw TypeError: undefined is not an object\"",
        count: 9,
    },
    common::KnownDivergence {
        cause: "mensagem do spread de null sem o trecho do fonte",
        expected: "\"get iter,call iter0 => throw TypeError: null is not an object (evaluating '...X')\"",
        got: "\"get iter,call iter0 => throw TypeError: null is not an object\"",
        count: 9,
    },
];

#[test]
fn iter_protocol_matches_bun() {
    common::run_factored_big_stack_known(GOLDEN, PRELUDES, 3000, KNOWN, |source| common::EvalMode::IndirectEval.evaluate(source, "", "R"));
}
