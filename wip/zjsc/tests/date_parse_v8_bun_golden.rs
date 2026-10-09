//! Golden do parser de data do V8 que o bun usa (`js_date_math_v8`) contra o bun 1.4.2: `tests/golden/date_parse_v8_bun.tsv`
//! sai de `scripts/gen-date-parse-v8-golden.js`, rodado com `TZ` fixa por subprocesso (UTC e America/Sao_Paulo, esta
//! para cobrir os formatos sem fuso, que são hora local). Cada linha é `fuso`, o literal JSON da cadeia (só ASCII) e o
//! resultado `P=<Date.parse>|N=<new Date(s).getTime()>|I=<toISOString, ou - se inválida>`. Cobre ISO completo e parcial
//! (ano estendido, frações de 1 a 12 dígitos, offsets), legado (nomes de mês e dia, ordens, AM/PM, ano de 2 dígitos,
//! fusos nomeados, parênteses, espaços, lixo no fim), números soltos e cadeias com caracteres não ASCII.
mod common;

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/date_parse_v8_bun.tsv");
const ZONES: [&str; 2] = ["UTC", "America/Sao_Paulo"];

/// O programa mede os três valores e grava o texto na global `R`; o literal da cadeia é o JSON do gerador.
fn program(literal: &str) -> String {
    format!(
        "var s = {literal};\nvar p = Date.parse(s);\nvar d = new Date(s);\nvar n = d.getTime();\n\
         globalThis.R = 'P=' + p + '|N=' + n + '|I=' + (Number.isNaN(n) ? '-' : d.toISOString());"
    )
}

#[test]
fn date_parse_v8_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for zone in ZONES {
        set_time_zone_spec_override(Some(zone));
        for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
            let mut columns = line.splitn(3, '\t');
            if columns.next().expect("fuso") != zone {
                continue;
            }
            let literal = columns.next().expect("cadeia");
            let expected = columns.next().expect("resultado");
            total += 1;
            let source = program(literal);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| evaluate_named_script_result(&source, "date_parse_v8_case.js", "R")));
            match outcome {
                Ok(Ok(value)) => {
                    let actual = common::guarded(|| Ok(value)).unwrap_or_default();
                    if actual != expected {
                        failures.push(format!("[{zone}] {literal}\n    esperado {expected}\n    veio     {actual}"));
                    }
                }
                Ok(Err(_)) => failures.push(format!("[{zone}] {literal}\n    esperado {expected}\n    o programa lançou exceção")),
                Err(_) => failures.push(format!("[{zone}] {literal}\n    esperado {expected}\n    pânico")),
            }
        }
    }
    set_time_zone_spec_override(None);
    assert!(total >= 8000, "golden pequeno demais: {total} linhas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
