//! Golden dos calendários e sistemas numéricos do `Intl.DateTimeFormat` contra o JavaScriptCore real:
//! `tests/golden/calendar_bun.tsv` sai de `scripts/gen-calendar-golden.js golden`, rodado no bun.
//! Colunas: tag pedida, instante, opções (`A`: era curta, ano, mês por extenso, dia; `B`: tudo numérico), e o que o
//! bun devolveu: `resolvedOptions().locale`, `calendar`, `numberingSystem` e as partes de componente.
//!
//! O locale, o calendário e o sistema numérico resolvidos são conferidos em toda linha. As partes de componente
//! são conferidas como conjunto de `tipo=valor` (a ordem e a pontuação dependem do padrão do locale para cada
//! calendário, que o porte não tem); nas opções `B` fora do `en` o padrão muda até a largura do mês, do dia e da
//! era, então só `year` e `relatedYear` são conferidos.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/calendar_bun.tsv");

/// O que o gerador roda no bun, com as mesmas opções e o mesmo filtro de partes.
const PROGRAM: &str = r#"(function (tag, epoch, id) {
  var options = id === "A"
    ? { era: "short", year: "numeric", month: "long", day: "numeric", timeZone: "UTC" }
    : { year: "numeric", month: "numeric", day: "numeric", timeZone: "UTC" };
  var format = new Intl.DateTimeFormat(tag, options);
  var resolved = format.resolvedOptions();
  var keep = { era: 1, year: 1, relatedYear: 1, yearName: 1, month: 1, day: 1 };
  var parts = format.formatToParts(epoch).filter(function (p) { return keep[p.type]; })
    .map(function (p) { return p.type + "=" + p.value; }).join("|");
  return [resolved.locale, resolved.calendar, resolved.numberingSystem, parts].join("\t");
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

fn run(tag: &str, epoch: &str, id: &str) -> Result<String, String> {
    let program = format!("{PROGRAM}({:?}, {epoch}, {id:?})", tag);
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

/// As partes a conferir: sorteadas, e só `year`/`relatedYear` nas opções `B` fora do `en`.
fn compared_parts(parts: &str, id: &str, tag: &str) -> Vec<String> {
    let narrow = id == "B" && !tag.starts_with("en-");
    let mut kept: Vec<String> = parts
        .split('|')
        .filter(|part| !part.is_empty())
        .filter(|part| !narrow || part.starts_with("year=") || part.starts_with("relatedYear="))
        .map(str::to_string)
        .collect();
    kept.sort();
    kept
}

#[test]
fn calendars_and_numbering_systems_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let cells: Vec<String> = line.split('\t').map(unescape).collect();
        let [tag, epoch, id, locale, calendar, numbering, parts] = cells.as_slice() else {
            panic!("linha sem as sete colunas: {line}");
        };
        total += 1;
        let expected_head = format!("{locale}\t{calendar}\t{numbering}");
        match run(tag, epoch, id) {
            Ok(actual) => {
                let mut fields = actual.splitn(4, '\t');
                let head = [fields.next(), fields.next(), fields.next()].map(|cell| cell.unwrap_or("")).join("\t");
                let actual_parts = fields.next().unwrap_or("");
                if head != expected_head || compared_parts(actual_parts, id, tag) != compared_parts(parts, id, tag) {
                    failures.push(format!("{tag} {epoch} {id}\n    esperado {expected_head} {parts}\n    veio     {actual}"));
                }
            }
            Err(reason) => failures.push(format!("{tag} {epoch} {id}\n    esperado {expected_head} {parts}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
