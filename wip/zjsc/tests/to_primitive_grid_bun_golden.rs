//! Golden de `ToPrimitive` em grade contra o JavaScriptCore do bun: `tests/golden/to_primitive_grid_bun.tsv` cruza
//! operadores binários e unários, `++`/`--` e atribuição composta em variável, propriedade e índice, com operandos que
//! registram dica e ordem (`Symbol.toPrimitive`, `valueOf`, `toString`, getters, `Proxy`), lançam, devolvem objeto,
//! `Date`, `Symbol` e `BigInt`. O prelúdio define `U`, `TP`, `Q`, `DT` e `DP`; a variável global `R` guarda o texto
//! com o registro de ordem seguido do resultado ou do `!Erro:mensagem`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/to_primitive_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/to_primitive_grid.preludes.json");

#[test]
fn to_primitive_grid_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "to_primitive_grid_case.js", "R"));
}
