//! Golden da ordem das chaves próprias de objetos exóticos contra o JavaScriptCore do bun:
//! `tests/golden/key_order_exotic_bun.tsv` sai de `scripts/gen-key-order-exotic-golden.js`, rodado no bun 1.4.2. Cada
//! linha é um programa (sufixo JSON sobre o prelúdio fatorado) e o texto da variável global `R` que ele grava. Cobre
//! Reflect.ownKeys, Object.keys, for-in, JSON.stringify, Object.assign, spread, Object.entries e getOwnPropertyNames em
//! arrays com índices além de length (inclusive 2**32-2 e 2**32-1), strings boxed, typed arrays com chaves numéricas
//! canônicas e não canônicas, arguments, funções e classes depois de delete e redefinição, Error, RegExp, objetos com
//! muitas chaves inteiras, defineProperty de índice em ordem decrescente e Proxy sem trap ownKeys.
mod common;


const GOLDEN: &str = include_str!("golden/key_order_exotic_bun.tsv");
const PRELUDES: &str = include_str!("golden/key_order_exotic.preludes.json");

/// Quirks do JavaScriptCore do bun que a especificação não tem (medidos no bun 1.4.2, o motor segue a especificação):
/// - `Object.keys` de uma classe com `static length` depois de `delete name` ainda lista `name`, que o `Reflect.ownKeys` já não tem
///   (3 casos mais 1 com `Object.assign` de array);
/// - o `for-in` de uma função (`async function`, gerador) não lista a chave de índice `0` que o `Object.keys` lista (2 casos).
const KNOWN: &[common::KnownDivergence] = &[
    common::KnownDivergence {
        cause: "Object.keys de classe com static length lista o name já apagado (quirk do JSC)",
        expected: "[length,prototype] | [length,name] | [length] | undefined | [length] | [length] | length=3 | [length,prototype]",
        got: "[length,prototype] | [length] | [length] | undefined | [length] | [length] | length=3 | [length,prototype]",
        count: 3,
    },
    common::KnownDivergence {
        cause: "Object.keys de classe com static length lista o name já apagado (quirk do JSC), com índices do Object.assign",
        expected: "[0,1,2,length,prototype,4294967296] | [0,1,2,length,name,4294967296] | [0,1,2,length,4294967296] | undefined | [0,1,2,length,4294967296] | [0,1,2,length,4294967296] | 0=9,1=8,2=7,length=3,4294967296=1 | [0,1,2,length,prototype,4294967296]",
        got: "[0,1,2,length,prototype,4294967296] | [0,1,2,length,4294967296] | [0,1,2,length,4294967296] | undefined | [0,1,2,length,4294967296] | [0,1,2,length,4294967296] | 0=9,1=8,2=7,length=3,4294967296=1 | [0,1,2,length,prototype,4294967296]",
        count: 1,
    },
    common::KnownDivergence {
        cause: "for-in de função não lista o índice 0 (quirk do JSC), com a chave 01 definida",
        expected: "[01]",
        got: "[0,01]",
        count: 1,
    },
    common::KnownDivergence {
        cause: "for-in de gerador não lista o índice 0 (quirk do JSC)",
        expected: "[]",
        got: "[0]",
        count: 1,
    },
];

#[test]
fn key_order_exotic_matches_bun() {
    common::run_factored_big_stack_known(GOLDEN, PRELUDES, 3000, KNOWN, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
