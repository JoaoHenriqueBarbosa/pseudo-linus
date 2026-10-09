//! Golden de invariantes de Proxy contra o JavaScriptCore do bun: `tests/golden/proxy_invariants_bun.tsv` sai de
//! `scripts/gen-proxy-invariants-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. Cobre as 13 traps com retorno válido e cada violação de invariante contra alvos
//! não configuráveis, não graváveis, accessors sem getter ou setter e não extensíveis (mensagens exatas), a ordem das
//! traps por operação de alto nível, a busca da trap no handler, revogação no meio da operação, Proxy como protótipo e
//! Proxy de Proxy.
//! O prelúdio comum das linhas fica em `tests/golden/proxy_invariants.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/proxy_invariants_bun.tsv");
const PRELUDES: &str = include_str!("golden/proxy_invariants.preludes.json");

#[test]
fn proxy_invariants_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "proxy_invariants_case.js", "R")));
}
