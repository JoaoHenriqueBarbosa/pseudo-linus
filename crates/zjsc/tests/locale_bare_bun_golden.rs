//! Golden de métodos de locale chamados sem `Intl` explícito contra o JavaScriptCore do bun: `tests/golden/locale_bare_bun.tsv`
//! sai de `scripts/gen-locale-bare-golden.js`, rodado no bun 1.4.2 com TZ=UTC. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre `toLocaleString` de Number, BigInt, Date, Array e TypedArray,
//! `toLocaleDateString`, `toLocaleTimeString`, `localeCompare` e `toLocaleUpperCase`/`toLocaleLowerCase` em 19 locales, com
//! locale em string, array, `undefined` e inválido (RangeError com a mensagem exata) e opções (style, currency, dígitos de
//! fração, dateStyle/timeStyle, hour12, timeZone UTC/America/Sao_Paulo/Asia/Tokyo, timeZoneName, era, weekday e month).
//! O programa não lê TZ: a máquina que roda o teste precisa estar em UTC.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/locale_bare_bun.tsv");
const PRELUDES: &str = include_str!("golden/locale_bare.preludes.json");

#[test]
fn locale_bare_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| {
        set_time_zone_spec_override(Some("UTC"));
        common::EvalMode::IndirectEval.evaluate(source, "locale_bare_case.js", "R")
    });
}
