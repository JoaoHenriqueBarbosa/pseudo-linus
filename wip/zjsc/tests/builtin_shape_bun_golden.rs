//! Golden da forma dos built-ins globais contra o JavaScriptCore do bun: `tests/golden/builtin_shape_bun.tsv` sai de
//! `scripts/gen-builtin-shape-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava: nomes próprios na ordem da engine, descritores, `name` e `length` das funções, `toStringTag`,
//! cadeia de protótipos e a mensagem exata de chamar método ou getter com receptor errado.
mod common;

use zjsc::api::eval::{evaluate_indirect_eval_with_caller, INDIRECT_EVAL_SHAPE_CALLER};
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const CHILD_FILE: &str = "/tmp/zjsc-shape/builtin_shape_case.js";

const GOLDEN: &str = include_str!("golden/builtin_shape_bun.tsv");
const PRELUDES: &str = include_str!("golden/builtin_shape.preludes.json");

#[test]
fn builtin_shape_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    // O filho do gerador é sempre este arquivo (`CHILD_FILE` de gen-builtin-shape-golden.js): o eval é o indireto de um
    // arquivo CJS, então o `stack` do programa traz `eval (unknown)` e o chamador, e o `sourceURL` é o do arquivo filho.
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| {
        evaluate_indirect_eval_with_caller(source, CHILD_FILE, INDIRECT_EVAL_SHAPE_CALLER, "R", false)
    });
}
