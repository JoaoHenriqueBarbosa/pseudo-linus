//! Golden de classes contra o JavaScriptCore do bun: `tests/golden/class_bun.tsv` sai de
//! `scripts/gen-class-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre campos públicos, privados e
//! estáticos, ordem de inicialização, blocos static, `#x in obj`, métodos e acessores privados, herança de builtins,
//! `new.target`, `super` em objetos literais e métodos estáticos, retorno de construtor derivado, `this` antes de
//! `super`, inferência de `name`, `toString` de classe e as mensagens exatas de brand check.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/class_bun.tsv");
const PRELUDES: &str = include_str!("golden/class.preludes.json");

#[test]
fn classes_match_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_mapped_golden(GOLDEN, PRELUDES, 1800, "class_case.js");
}
