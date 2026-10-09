//! `temporalObjectTable` (nove `PropertyCallback`) e `temporalNowTable` (seis funções) são reificadas no
//! primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a da tabela seguida do
//! `Symbol(Symbol.toStringTag)`; `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: os nomes já acessados na ordem de acesso e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const TEMPORAL_KEYS: &str =
    "Duration,Instant,Now,PlainDate,PlainDateTime,PlainTime,PlainMonthDay,PlainYearMonth,ZonedDateTime,Symbol(Symbol.toStringTag)";
const NOW_KEYS: &str = "instant,timeZoneId,plainDateISO,plainDateTimeISO,plainTimeISO,zonedDateTimeISO,Symbol(Symbol.toStringTag)";

#[test]
fn temporal_own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k(Temporal)")), TEMPORAL_KEYS);
}

#[test]
fn temporal_own_keys_after_access() {
    let program = format!("{KEYS} var x = [Temporal.Now, Temporal.Duration, Temporal.PlainTime]; k(Temporal)");
    assert_eq!(run(&program), TEMPORAL_KEYS);
}

#[test]
fn temporal_own_keys_after_delete_with_access() {
    let program = format!("{KEYS} var x = [Temporal.Now, Temporal.Duration, Temporal.PlainTime]; var r = delete Temporal.PlainDate; r + '|' + k(Temporal)");
    assert_eq!(
        run(&program),
        "true|Now,Duration,PlainTime,Instant,PlainDateTime,PlainMonthDay,PlainYearMonth,ZonedDateTime,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn temporal_own_keys_after_delete_untouched() {
    let program = format!("{KEYS} var y = Temporal.PlainTime; var r = delete Temporal.Instant; r + '|' + k(Temporal)");
    assert_eq!(
        run(&program),
        "true|PlainTime,Duration,Now,PlainDate,PlainDateTime,PlainMonthDay,PlainYearMonth,ZonedDateTime,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn temporal_descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Temporal, 'Instant'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Temporal.Duration.length].join(',')";
    assert_eq!(run(program), "function,true,false,true,0");
}

#[test]
fn now_own_keys_before_and_after_access() {
    let program = format!("{KEYS} var a = k(Temporal.Now); var f = [Temporal.Now.instant, Temporal.Now.plainDateISO]; a + '|' + k(Temporal.Now)");
    assert_eq!(run(&program), format!("{NOW_KEYS}|{NOW_KEYS}"));
}

#[test]
fn now_own_keys_after_delete_with_access() {
    let program = format!("{KEYS} var f = [Temporal.Now.instant, Temporal.Now.plainDateISO]; var r = delete Temporal.Now.timeZoneId; r + '|' + k(Temporal.Now)");
    assert_eq!(
        run(&program),
        "true|instant,plainDateISO,plainDateTimeISO,plainTimeISO,zonedDateTimeISO,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn now_descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Temporal.Now, 'instant'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Temporal.Now.instant.length, Temporal.Now.plainDateISO.length].join(',')";
    assert_eq!(run(program), "function,true,false,true,0,0");
}

#[test]
fn now_creates_classes_on_demand() {
    let program = "var d = Temporal.Now.plainDateISO(); d instanceof Temporal.PlainDate";
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_boolean() && value.as_boolean());
}
