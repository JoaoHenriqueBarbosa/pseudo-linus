//! Golden de strings, template literals e operadores contra o JavaScriptCore do bun: `tests/golden/template_ops_bun.tsv`
//! sai de `scripts/gen-template-ops-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre tagged templates (`strings.raw`, cache por site, escapes inválidos
//! em tagged e untagged), `String.raw`, concatenação, comparação relacional, `in`, `instanceof` com `Symbol.hasInstance`,
//! `typeof`, `void`/`delete`/vírgula, `**`, atribuição composta com getters que registram a ordem, optional chaining,
//! nullish com atribuição, spread em chamadas/arrays/objetos e destructuring em parâmetros com defaults com efeito.
//! O prelúdio comum das linhas fica em `tests/golden/template_ops.preludes.json` (ver `tests/common/mod.rs`).

mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/template_ops_bun.tsv");
const PRELUDES: &str = include_str!("golden/template_ops.preludes.json");

#[test]
fn template_ops_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "template_ops_case.js", "R")));
}
