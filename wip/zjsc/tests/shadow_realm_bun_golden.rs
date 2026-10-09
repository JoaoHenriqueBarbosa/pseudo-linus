//! Golden de ShadowRealm contra o JavaScriptCore do bun: `tests/golden/shadow_realm_bun.tsv` sai de
//! `scripts/gen-shadow-realm-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre o construtor, `evaluate`,
//! `importValue`, as funções remotas (wrapped functions), o isolamento de globais e protótipos entre realms e o
//! mapeamento de erros lançados no realm para `TypeError` do chamador.
mod common;

const GOLDEN: &str = include_str!("golden/shadow_realm_bun.tsv");

const PRELUDES: &str = include_str!("golden/shadow_realm.preludes.json");

#[test]
fn shadow_realm_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 600, "shadow_realm_case.js");
}
