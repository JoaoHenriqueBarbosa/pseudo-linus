//! Golden de BigInt, Symbol e conversões abstratas contra o JavaScriptCore do bun: `tests/golden/bigint_symbol_bun.tsv`
//! sai de `scripts/gen-bigint-symbol-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre o construtor `BigInt`, `asIntN`/`asUintN`, `toString` com radix, operadores
//! mistos com Number, comparações, shifts, expoente negativo, divisão por zero, literais, parse de string, JSON,
//! `Symbol.toPrimitive`, descrição, `for`/`keyFor`, well-knowns, conversões implícitas que lançam, `Object(sym)`, chaves
//! por símbolo em `ownKeys`/`assign`/spread e uma grade de valores por operadores de `ToNumber`/`ToString`/
//! `ToPrimitive`/`ToPropertyKey` com o log da ordem de `valueOf`/`toString`. Resultados com mais de 4000 unidades UTF-16
//! saem como `#len<N>` nos dois lados.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/bigint_symbol_bun.tsv");
const PRELUDES: &str = include_str!("golden/bigint_symbol.preludes.json");

#[test]
fn bigint_symbol_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    // Mesmo corte do gerador: acima de 4000 unidades UTF-16 só o comprimento é comparado.
    common::check(GOLDEN, PRELUDES, 3000, |source| {
        common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "bigint_symbol_case.js", "R")).map(|result| {
            let units = result.len();
            if units > 4000 { common::Units::from(format!("#len{units}")) } else { result }
        })
    });
}
