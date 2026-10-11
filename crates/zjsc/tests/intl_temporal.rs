//! `Intl.DateTimeFormat` e o `toLocaleString` dos protótipos `Temporal`, e `Date.prototype.toTemporalInstant`,
//! ponta a ponta. Os valores esperados seguem `IntlDateTimeFormat.cpp` (um formatador por tipo, os plain
//! sem fuso, o `ZonedDateTime` no próprio fuso) e as mensagens de erro do C++; os programas são ASCII puro.
use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

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
    assert!(failures.is_empty(), "{} de {} divergem:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

/// O nome do erro que o programa lança (ou `"none"`).
const ERROR_NAME: &str = "(function (f) { try { f(); return 'none'; } catch (e) { return e.name + ': ' + e.message; } })";

#[test]
fn prototypes_to_locale_string() {
    check(&[
        ("Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US')", "1/5/2024"),
        ("Temporal.PlainDate.from('2024-01-05').toLocaleString('pt-BR')", "05/01/2024"),
        ("Temporal.PlainTime.from('15:04:05').toLocaleString('en-US')", "3:04:05 PM"),
        ("Temporal.PlainTime.from('15:04:05').toLocaleString('pt-BR')", "15:04:05"),
        ("Temporal.PlainDateTime.from('2024-01-05T15:04:05').toLocaleString('en-US')", "1/5/2024, 3:04:05 PM"),
        ("Temporal.PlainDateTime.from('2024-01-05T15:04:05').toLocaleString('pt-BR')", "05/01/2024, 15:04:05"),
        ("Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {month: 'long', day: 'numeric'})", "January 5"),
        ("Temporal.Instant.from('2024-01-05T15:04:05Z').toLocaleString('en-US', {timeZone: 'UTC'})", "1/5/2024, 3:04:05 PM"),
        (
            "Temporal.Instant.from('2024-01-05T15:04:05Z').toLocaleString('en-US', {timeZone: 'America/Sao_Paulo'})",
            "1/5/2024, 12:04:05 PM",
        ),
        (
            "Temporal.PlainYearMonth.from('2024-01').toLocaleString('en-US', {calendar: 'iso8601'})",
            "2024-01",
        ),
        (
            "Temporal.PlainMonthDay.from('01-05').toLocaleString('en-US', {calendar: 'iso8601'})",
            "01-05",
        ),
    ]);
}

#[test]
fn zoned_date_time_uses_its_own_time_zone() {
    check(&[
        (
            "Temporal.ZonedDateTime.from('2024-01-05T15:04:05-03:00[America/Sao_Paulo]').toLocaleString('en-US')",
            "1/5/2024, 3:04:05 PM GMT-3",
        ),
        (
            "Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]').toLocaleString('en-US')",
            "1/5/2024, 3:04:05 PM UTC",
        ),
        (
            "Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[+00:00]').toLocaleString('en-US')",
            "1/5/2024, 3:04:05 PM GMT",
        ),
        (
            "Temporal.ZonedDateTime.from('2024-01-05T15:04:05-03:00[America/Sao_Paulo]').toLocaleString('en-US', {hour: 'numeric'})",
            "3 PM",
        ),
    ]);
}

#[test]
fn date_time_format_formats_temporal_objects() {
    check(&[
        ("new Intl.DateTimeFormat('en-US').format(Temporal.PlainDate.from('2024-01-05'))", "1/5/2024"),
        (
            "new Intl.DateTimeFormat('en-US').format(Temporal.PlainDateTime.from('2024-01-05T15:04:05'))",
            "1/5/2024, 3:04:05 PM",
        ),
        ("new Intl.DateTimeFormat('en-US').format(Temporal.PlainTime.from('15:04:05'))", "3:04:05 PM"),
        (
            "new Intl.DateTimeFormat('en-US', {timeZone: 'America/Sao_Paulo'}).format(Temporal.PlainDateTime.from('2024-01-05T15:04:05'))",
            "1/5/2024, 3:04:05 PM",
        ),
        (
            "new Intl.DateTimeFormat('en-US', {timeZone: 'America/Sao_Paulo'}).format(Temporal.Instant.from('2024-01-05T15:04:05Z'))",
            "1/5/2024, 12:04:05 PM",
        ),
        (
            "new Intl.DateTimeFormat('en-US').formatToParts(Temporal.PlainDate.from('2024-01-05')).map(function (p) { return p.type + '=' + p.value; }).join('|')",
            "month=1|literal=/|day=5|literal=/|year=2024",
        ),
        (
            "new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainDate.from('2024-01-07'))",
            "1/5/2024 \u{2013} 1/7/2024",
        ),
        (
            "new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainDate.from('2024-01-05'))",
            "1/5/2024",
        ),
    ]);
}

#[test]
fn errors_follow_the_c_plus_plus_messages() {
    let program = |body: &str| format!("{ERROR_NAME}(function () {{ {body} }})");
    check(&[
        (
            program("new Intl.DateTimeFormat('en-US', {hour: 'numeric'}).format(Temporal.PlainDate.from('2024-01-05'));").as_str(),
            "TypeError: DateTimeFormat has no fields applicable to this Temporal type",
        ),
        (
            program("new Intl.DateTimeFormat('en-US', {year: 'numeric'}).format(Temporal.PlainTime.from('15:04:05'));").as_str(),
            "TypeError: DateTimeFormat has no fields applicable to this Temporal type",
        ),
        (
            program("new Intl.DateTimeFormat('en-US').format(Temporal.PlainYearMonth.from('2024-01'));").as_str(),
            "RangeError: Temporal object's calendar does not match DateTimeFormat calendar",
        ),
        (
            program("new Intl.DateTimeFormat('en-US').format(Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]'));").as_str(),
            "TypeError: Temporal.ZonedDateTime is not supported in Intl.DateTimeFormat; use toLocaleString() or convert to PlainDateTime first",
        ),
        (
            program("new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), Temporal.PlainTime.from('15:04:05'));").as_str(),
            "TypeError: formatRange requires both arguments to be the same Temporal type",
        ),
        (
            program("new Intl.DateTimeFormat('en-US').formatRange(Temporal.PlainDate.from('2024-01-05'), 0);").as_str(),
            "TypeError: formatRange requires both arguments to be the same Temporal type",
        ),
        (
            program("Temporal.ZonedDateTime.from('2024-01-05T15:04:05+00:00[UTC]').toLocaleString('en-US', {timeZone: 'UTC'});").as_str(),
            "TypeError: ZonedDateTime.toLocaleString does not accept a timeZone option; the ZonedDateTime's time zone is used",
        ),
        (
            program("Temporal.PlainDate.from('2024-01-05').toLocaleString('en-US', {timeStyle: 'short'});").as_str(),
            "TypeError: timeStyle is specified while formatting date is requested",
        ),
        (
            program("Temporal.PlainTime.prototype.toLocaleString.call({});").as_str(),
            "TypeError: Temporal.PlainTime.prototype.toLocaleString called on value that's not a PlainTime",
        ),
        (
            program("Temporal.PlainYearMonth.from('2024-01').toLocaleString('en-US');").as_str(),
            "RangeError: Temporal object's calendar does not match DateTimeFormat calendar",
        ),
    ]);
}

#[test]
fn date_to_temporal_instant() {
    check(&[
        ("new Date(Date.UTC(2024, 0, 5, 15, 4, 5)).toTemporalInstant().toString()", "2024-01-05T15:04:05Z"),
        ("new Date(Date.UTC(2024, 0, 5, 15, 4, 5, 123)).toTemporalInstant().toString()", "2024-01-05T15:04:05.123Z"),
        ("Date.prototype.toTemporalInstant.name + '/' + Date.prototype.toTemporalInstant.length", "toTemporalInstant/0"),
        (
            "Object.getOwnPropertyDescriptor(Date.prototype, 'toTemporalInstant').enumerable ? 'enumerable' : 'hidden'",
            "hidden",
        ),
        (
            format!("{ERROR_NAME}(function () {{ new Date(NaN).toTemporalInstant(); }})").as_str(),
            "RangeError: Invalid integer number of Epoch Millseconds",
        ),
        (
            "(function () { try { Date.prototype.toTemporalInstant.call({}); } catch (e) { return e.name; } return 'none'; })()",
            "TypeError",
        ),
    ]);
}
