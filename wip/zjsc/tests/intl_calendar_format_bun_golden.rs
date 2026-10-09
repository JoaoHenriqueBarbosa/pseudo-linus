//! Golden da saída formatada de `Intl.DateTimeFormat` em calendários não gregorianos contra o JavaScriptCore do bun:
//! `tests/golden/intl_calendar_format_bun.tsv` sai de `scripts/gen-intl-calendar-format-golden.js`, rodado no bun 1.4.2
//! com `TZ=America/Sao_Paulo`. Cada linha é uma chamada `G(locale, calendário, opções, instante)` sobre o prelúdio comum
//! e o texto da variável global `R` (`format`, ` # ` e o `formatToParts` serializado, ou `Nome: mensagem` quando lança).
//! Cobre 14 calendários × 6 locales (en, pt, de, ja, es, fr) com skeletons de data, hora 12/24, `timeZoneName`,
//! `dateStyle` e `timeStyle`, em datas de virada de ano, ano bissexto hebraico, mês bissexto chinês e troca de era
//! japonesa, o que exercita os padrões de `intl_calendar_patterns.rs` consumidos por `native_pattern_parts`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/intl_calendar_format_bun.tsv");
const PRELUDES: &str = include_str!("golden/intl_calendar_format.preludes.json");

#[test]
fn intl_calendar_format_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| {
        set_time_zone_spec_override(Some("America/Sao_Paulo"));
        common::EvalMode::IndirectEval.evaluate(source, "intl_calendar_format_case.js", "R")
    });
}
