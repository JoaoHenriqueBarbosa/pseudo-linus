//! Golden de Proxy e Reflect avançados contra o JavaScriptCore do bun: `tests/golden/proxy_reflect_bun.tsv` sai de
//! `scripts/gen-proxy-reflect-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre invariantes de cada trap contra alvos não configuráveis, não graváveis, não
//! extensíveis e congelados, Proxy.revocable e o uso depois da revogação, Proxy como protótipo, Proxy em for-in,
//! Object.keys, JSON.stringify, Array.isArray, spread, instanceof e nos métodos de Array, Reflect.construct com
//! newTarget, a ordem de Reflect.ownKeys e as mensagens exatas de TypeError.
mod common;


const GOLDEN: &str = include_str!("golden/proxy_reflect_bun.tsv");
const PRELUDES: &str = include_str!("golden/proxy_reflect.preludes.json");

#[test]
fn proxy_and_reflect_advanced_match_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 500, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
