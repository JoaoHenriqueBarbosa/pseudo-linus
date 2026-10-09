//! Golden de Proxy e Reflect contra o JavaScriptCore do bun: `tests/golden/proxy_bun.tsv` sai de
//! `scripts/gen-proxy-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre cada trap (resultado normal, trap que lança, trap que não é função, getter
//! de trap que lança, violação de cada invariante com a mensagem exata), Proxy.revocable e operações sobre proxy
//! revogado, proxy de proxy, de função, de array e de classe, a ordem de chamada das traps, Reflect.* com receiver,
//! `with` com Proxy e Symbol.unscopables, `in`, JSON, for-in, spread e `class extends` de Proxy. O arquivo se chama
//! `proxy_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/proxy_bun.tsv");

const PRELUDES: &str = include_str!("golden/proxy.preludes.json");

#[test]
fn proxy_and_reflect_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1500, "proxy_case.js");
}
