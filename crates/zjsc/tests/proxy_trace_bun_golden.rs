//! Golden da sequência de traps de Proxy contra o JavaScriptCore do bun: `tests/golden/proxy_trace_bun.tsv` sai de
//! `scripts/gen-proxy-trace-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava: as traps disparadas (nome e chave) na ordem, seguidas do resultado ou da
//! exceção. Cobre spread, for-in, `Object.keys/values/entries`, `JSON.stringify`, `Array.prototype.*` em proxy de
//! array, `instanceof`, `in`, `with`, `delete`, `class extends` de Proxy, `Symbol.toPrimitive`, `Object.assign`,
//! destructuring e as invariantes violadas (TypeError com a mensagem exata).
mod common;

const GOLDEN: &str = include_str!("golden/proxy_trace_bun.tsv");

#[test]
fn proxy_trace_matches_bun() {
    common::run_golden(GOLDEN, 700, |source| common::EvalMode::IndirectEval.evaluate(source, "proxy_trace_case.js", "R"));
}
