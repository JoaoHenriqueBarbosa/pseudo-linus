//! Golden da conferência de calendário (`validateCalendar` do `handleDateTimeValue`) entre objetos Temporal e
//! `Intl.DateTimeFormat` contra o JavaScriptCore do bun: `tests/golden/temporal_calendar_format_bun.tsv` sai de
//! `scripts/gen-temporal-calendar-format-golden.js`, rodado no bun 1.4.2 com `TZ=America/Sao_Paulo`. Cada linha é um
//! programa e o texto da variável global `R` (o resultado, ou `Nome: mensagem` quando lança). Cobre PlainDate,
//! PlainDateTime, PlainYearMonth e PlainMonthDay em calendários ISO, gregoriano, japonês, budista, hebraico,
//! islâmicos, chinês, `roc`, persa e outros, contra formatadores do mesmo calendário e de calendários diferentes,
//! mais `toLocaleString`.
mod common;

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/temporal_calendar_format_bun.tsv");

#[test]
fn temporal_calendar_format_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_golden(GOLDEN, 200, |source| evaluate_named_script_result(source, "temporal_calendar_format_case.js", "R"));
    set_time_zone_spec_override(None);
}
