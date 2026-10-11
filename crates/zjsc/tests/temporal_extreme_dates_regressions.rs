//! Regressões das datas nos limites do Temporal (-271821-04-19 a +275760-09-13) em calendários não ISO. O icu4x só constrói
//! `Date::try_new_iso` de -9999 a 9999: fora disso `calendar_fields` entrava em pânico (`data dentro dos limites de Temporal é
//! representável no icu4x`), nos testes `intl402/Temporal/*/extreme-dates.js` do test262.
use zjsc::api::eval::evaluate_named_script_reporting_uncaught;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(source: &str) -> String {
    let source = format!("var R; try {{ R = {source} }} catch (e) {{ R = e.name + ': ' + e.message }}");
    let read = evaluate_named_script_reporting_uncaught(&source, "temporal_extreme_dates.js", "R").expect("sem exceção solta");
    let value = read.unwrap_or_else(|_| panic!("leitura de R"));
    String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
}

fn fields(calendar: &str, date: &str) -> String {
    run(&format!(
        "(function () {{ var d = Temporal.PlainDate.from({date}).withCalendar('{calendar}'); \
         return [d.year, d.month, d.monthCode, d.day, d.era, d.eraYear].join(','); }})()"
    ))
}

/// Os valores são os do bun 1.4.2 (os mesmos das tabelas do test262).
#[test]
fn minimum_and_maximum_dates_read_back_in_gregory_and_buddhist() {
    assert_eq!(fields("gregory", "'-271821-04-19'"), "-271821,4,M04,19,bce,271822");
    assert_eq!(fields("gregory", "'+275760-09-13'"), "275760,9,M09,13,ce,275760");
    assert_eq!(fields("buddhist", "'-271821-04-19'"), "-271278,4,M04,19,be,-271278");
    assert_eq!(fields("buddhist", "'+275760-09-13'"), "276303,9,M09,13,be,276303");
}

#[test]
fn plain_year_month_with_minimum_year_in_gregory() {
    assert_eq!(
        run("(function () { var d = new Temporal.PlainYearMonth(2000, 4, 'gregory').with({ year: -271821 }); return [d.year, d.month, d.monthCode, d.era, d.eraYear].join(','); })()"),
        "-271821,4,M04,bce,271822"
    );
}
