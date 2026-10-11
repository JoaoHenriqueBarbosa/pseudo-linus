//! Golden de Proxy na cadeia de protótipos contra o JavaScriptCore do bun: `tests/golden/proxy_chain_bun.tsv` sai de
//! `scripts/gen-proxy-chain-golden.js`, rodado no bun. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava (resultado e log das traps). Cobre Proxy como protótipo (get/set/has/in/for-in/
//! keys/delete/instanceof/JSON/spread/with), Proxy de função e classe (apply/construct/new.target), `Proxy.revocable`
//! revogado no meio das operações, Proxy sobre arrays, Proxy como alvo de `Object.assign`/`defineProperties`/`freeze`,
//! Proxy de Proxy, receiver nas traps e traps que devolvem valores inválidos (mensagens exatas de `TypeError`).
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/proxy_chain_bun.tsv");
const PRELUDES: &str = include_str!("golden/proxy_chain.preludes.json");

#[test]
fn proxy_chain_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "proxy_chain_case.js", "R"));
}
