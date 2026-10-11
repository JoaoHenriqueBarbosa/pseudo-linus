//! Golden dos padrões de data e hora do `Intl.DateTimeFormat` contra o JavaScriptCore real:
//! `tests/golden/date_pattern_bun.tsv` sai de `scripts/gen-date-pattern-golden.js`, rodado no bun.
//! Colunas: tag pedida, instante (ms), opções (JSON), `resolvedOptions().locale`, `numberingSystem` e todas as
//! partes de `formatToParts` na ordem, com os literais, como `tipo=valor` separados por `|`.
//!
//! Ao contrário de `calendar_bun_golden`, a ordem e a pontuação do padrão do locale são conferidas por inteiro.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/date_pattern_bun.tsv");

/// O que o gerador roda no bun, com as mesmas opções e o mesmo formato de saída.
const PROGRAM: &str = r#"(function (tag, epoch, options) {
  options.timeZone = "UTC";
  var format = new Intl.DateTimeFormat(tag, options);
  var resolved = format.resolvedOptions();
  var parts = format.formatToParts(epoch).map(function (p) { return p.type + "=" + p.value; }).join("|");
  return [resolved.locale, resolved.numberingSystem, parts].join("\t");
})"#;

fn unescape(cell: &str) -> String {
    let mut out = String::new();
    let mut chars = cell.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn run(tag: &str, epoch: &str, options: &str) -> Result<String, String> {
    let program = format!("{PROGRAM}({:?}, {epoch}, ({options}))", tag);
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o programa não devolveu string".to_string()),
        Ok(Err(_)) => Err("o programa lançou exceção".to_string()),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

#[test]
fn date_patterns_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let cells: Vec<String> = line.split('\t').map(unescape).collect();
        let [tag, epoch, options, locale, numbering, parts] = cells.as_slice() else {
            panic!("linha sem as seis colunas: {line}");
        };
        total += 1;
        let expected = format!("{locale}\t{numbering}\t{parts}");
        match run(tag, epoch, options) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{tag} {epoch} {options}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{tag} {epoch} {options}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
