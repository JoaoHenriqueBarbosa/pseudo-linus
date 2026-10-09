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

#[test]
fn key_order_exotic_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
