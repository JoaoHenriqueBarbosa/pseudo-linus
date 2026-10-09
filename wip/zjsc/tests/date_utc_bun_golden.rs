//! Golden de Date com fuso fixo UTC contra o JavaScriptCore do bun: `tests/golden/date_utc_bun.tsv` sai de
//! `scripts/gen-date-utc-golden.js`, rodado no bun 1.4.2 com TZ=UTC. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre Date.parse de formatos não ISO (RFC 2822, "Mon Jan 01 2024", AM/PM,
//! GMT+0100, anos de dois dígitos, strings inválidas), toString/toUTCString/toISOString/toLocale*String, setters com
//! vários argumentos e NaN, `Date.UTC` com 1 a 7 argumentos, `@@toPrimitive`, limites de ±8.64e15,
//! getYear/setYear/toGMTString e subclasses. O programa não lê TZ: a máquina que roda o teste precisa estar em UTC.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/date_utc_bun.tsv");

#[test]
fn date_utc_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 800, |source| {
        set_time_zone_spec_override(Some("UTC"));
        common::EvalMode::RunInThisContext.evaluate(source, "", "R")
    });
}
