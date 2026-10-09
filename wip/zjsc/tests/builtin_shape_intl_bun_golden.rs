//! Golden da forma de Intl.* e Temporal.* contra o JavaScriptCore do bun: `tests/golden/builtin_shape_intl_bun.tsv` sai de
//! `scripts/gen-builtin-shape-intl-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava: nomes próprios na ordem da engine, descritores, `name` e `length` das funções, `toStringTag`,
//! cadeia de protótipos e a mensagem exata de chamar método ou getter com receptor errado.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/builtin_shape_intl_bun.tsv");
const PRELUDES: &str = include_str!("golden/builtin_shape_intl.preludes.json");

#[test]
fn builtin_shape_intl_matches_bun() {
    // O golden saiu do bun na máquina de São Paulo (`Temporal.Now.timeZoneId()` devolve o fuso do processo); o motor
    // sozinho, sem pseudo-processo, resolveria UTC, então o `TZ` entra pelo gancho, como nos goldens de Date.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "builtin_shape_case.js", "R"));
}
