//! O `Intl` do porte contra o bun 1.4.2 (`TZ=UTC`), ponta a ponta: cada caso é um programa que devolve
//! a string medida no bun. Os programas são ASCII puro (o `eval` indireto lê a fonte como Latin-1), os
//! caracteres fora do ASCII entram por `\u` no JavaScript e no valor esperado.
use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// O instante dos casos de data: `2024-01-05T15:04:05Z`, uma sexta-feira.
const DATE: &str = "var d = new Date(Date.UTC(2024,0,5,15,4,5)); ";

/// O valor de `source` como texto; falha se o programa lança ou devolve algo que não é string.
fn string_of(source: &str) -> String {
    match evaluate_indirect_eval(source) {
        Ok(value) => {
            assert!(value.is_string(), "{source}: o valor não é string");
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            String::from_utf8(bytes).expect("texto em UTF-8")
        }
        Err(_) => panic!("{source}: lançou exceção"),
    }
}

/// Confere uma lista de `(programa, esperado)` e junta todas as divergências numa só falha.
fn check(cases: &[(&str, &str)]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(source, expected)| {
            let actual = std::panic::catch_unwind(|| string_of(source)).unwrap_or_else(|_| "<falhou ao avaliar>".to_string());
            (actual != *expected).then(|| format!("{source}\n    esperado {expected:?}\n    veio     {actual:?}"))
        })
        .collect();
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

#[test]
fn number_to_locale_string() {
    check(&[
        ("(1234567.891).toLocaleString()", "1,234,567.891"),
        ("(1234567.891).toLocaleString('pt-BR')", "1.234.567,891"),
        ("(0.256).toLocaleString('en-US', {style: 'percent'})", "26%"),
        ("(1234.5).toLocaleString('en-US', {style: 'currency', currency: 'USD'})", "$1,234.50"),
        ("(1234.5).toLocaleString('pt-BR', {style: 'currency', currency: 'BRL'})", "R$\u{a0}1.234,50"),
        ("(1234567).toLocaleString('en-US', {notation: 'compact'})", "1.2M"),
        ("(12345n).toLocaleString()", "12,345"),
    ]);
}

#[test]
fn date_to_locale_string() {
    let program = |call: &str| format!("{DATE}{call}");
    check(&[
        (program("d.toLocaleString('en-US', {timeZone: 'UTC'})").as_str(), "1/5/2024, 3:04:05 PM"),
        (program("d.toLocaleDateString('pt-BR', {timeZone: 'UTC'})").as_str(), "05/01/2024"),
        (program("d.toLocaleTimeString('en-US', {timeZone: 'UTC'})").as_str(), "3:04:05 PM"),
    ]);
}

#[test]
fn date_time_format() {
    let program = |call: &str| format!("{DATE}{call}");
    check(&[
        (
            program("new Intl.DateTimeFormat('en-US', {dateStyle: 'full', timeStyle: 'long', timeZone: 'UTC'}).format(d)").as_str(),
            "Friday, January 5, 2024 at 3:04:05 PM UTC",
        ),
        (
            program("new Intl.DateTimeFormat('pt-BR', {dateStyle: 'full', timeStyle: 'short', timeZone: 'UTC'}).format(d)").as_str(),
            "sexta-feira, 5 de janeiro de 2024 \u{e0}s 15:04",
        ),
        ("Intl.DateTimeFormat().resolvedOptions().locale", "en-US"),
    ]);
}

#[test]
fn relative_time_format() {
    check(&[
        ("new Intl.RelativeTimeFormat('en', {numeric: 'auto'}).format(-1, 'day')", "yesterday"),
        ("new Intl.RelativeTimeFormat('pt-BR').format(3, 'hour')", "em 3 horas"),
    ]);
}

#[test]
fn list_format() {
    check(&[
        ("new Intl.ListFormat('en', {type: 'conjunction'}).format(['a', 'b', 'c'])", "a, b, and c"),
        ("new Intl.ListFormat('pt-BR', {type: 'conjunction'}).format(['a', 'b', 'c'])", "a, b e c"),
    ]);
}

#[test]
fn plural_rules() {
    check(&[
        ("new Intl.PluralRules('en').select(1)", "one"),
        ("new Intl.PluralRules('en', {type: 'ordinal'}).select(2)", "two"),
    ]);
}

#[test]
fn collator_and_case_mapping() {
    check(&[
        ("['b', 'a', 'B', '\\u00e1'].sort(new Intl.Collator('en').compare).join('')", "a\u{e1}bB"),
        ("'I'.toLocaleLowerCase('tr')", "\u{131}"),
    ]);
}

#[test]
fn supported_values_of_calendar() {
    check(&[("String(Intl.supportedValuesOf('calendar').length)", "16")]);
}
