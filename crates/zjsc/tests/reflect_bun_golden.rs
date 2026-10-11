//! Golden complementar de Reflect e Proxy contra o JavaScriptCore do bun: `tests/golden/reflect_bun.tsv` sai de
//! `scripts/gen-reflect-golden.js`, rodado no bun 1.4.2. Cobre as invariantes de cada trap (alvo não configurável,
//! não gravável, não extensível) com as mensagens exatas do TypeError, `Proxy.revocable` e o uso após revoke, Proxy
//! como protótipo, Proxy em for-in/keys/JSON/spread/assign, Proxy de array, Proxy de Proxy, Proxy de função com
//! `new.target`, a ordem das traps e `Reflect.*` (receiver, argumentos inválidos, array-like, newTarget). Cada linha
//! é um programa (JSON) e o texto da variável global `R` que ele grava: o valor formatado ou `NomeDoErro: mensagem`.
mod common;

const GOLDEN: &str = include_str!("golden/reflect_bun.tsv");

const PRELUDES: &str = include_str!("golden/reflect.preludes.json");

#[test]
fn reflect_and_proxy_invariants_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1400, "reflect_case.js");
}
