//! Golden de Date legado e conversão contra o JavaScriptCore do bun: `tests/golden/date_legacy_bun.tsv` sai de
//! `scripts/gen-date-legacy-golden.js`, rodado no bun 1.4.2 com TZ=UTC. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre setters e getters locais e UTC em grade, `new Date` com 0 a 7 argumentos e
//! tipos (BigInt e Symbol lançam TypeError), `@@toPrimitive` com hints inválidos, toJSON genérico, subclasses,
//! aritmética e comparação, "Invalid time value", anos de ±271821 e anos 0 a 99 contra `Date.UTC`.
//! O programa não lê TZ: a máquina que roda o teste precisa estar em UTC.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/date_legacy_bun.tsv");
const PRELUDES: &str = include_str!("golden/date_legacy.preludes.json");

#[test]
fn date_legacy_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 2000, |source| {
        set_time_zone_spec_override(Some("UTC"));
        common::EvalMode::IndirectEval.evaluate(source, "date_legacy_case.js", "R")
    });
}
