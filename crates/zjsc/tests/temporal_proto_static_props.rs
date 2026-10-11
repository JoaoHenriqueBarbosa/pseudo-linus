//! `durationPrototypeTable`, `instantPrototypeTable` e `temporalInstantConstructorTable` são reificadas no primeiro
//! acesso. Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma (nomes da tabela na frente de
//! `constructor` e do `@@toStringTag`); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: `constructor`, os já acessados e o resto da tabela. `delete` de um nome fora da tabela (`constructor`,
//! `length`) não reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const TAG: &str = "Symbol(Symbol.toStringTag)";
const DURATION: &str = "with,negated,abs,add,subtract,round,total,toString,toJSON,toLocaleString,valueOf,years,months,weeks,days,hours,\
minutes,seconds,milliseconds,microseconds,nanoseconds,sign,blank";
const DURATION_AFTER_DELETE: &str = "constructor,round,abs,negated,add,subtract,total,toString,toJSON,toLocaleString,valueOf,years,months,\
weeks,days,hours,minutes,seconds,milliseconds,microseconds,nanoseconds,sign,blank";
const INSTANT: &str = "add,subtract,until,since,round,equals,toZonedDateTimeISO,toString,toJSON,toLocaleString,valueOf,epochMilliseconds,\
epochNanoseconds";
const INSTANT_AFTER_DELETE: &str = "constructor,round,add,subtract,until,since,equals,toZonedDateTimeISO,toJSON,toLocaleString,valueOf,\
epochMilliseconds,epochNanoseconds";
const INSTANT_CONSTRUCTOR: &str = "from,fromEpochMilliseconds,fromEpochNanoseconds,compare,length,name,prototype";

#[test]
fn own_keys_before_access() {
    assert_eq!(
        run(&format!("{KEYS} k(Temporal.Duration.prototype) + '|' + k(Temporal.Instant.prototype) + '|' + k(Temporal.Instant)")),
        format!("{DURATION},constructor,{TAG}|{INSTANT},constructor,{TAG}|{INSTANT_CONSTRUCTOR}")
    );
}

#[test]
fn own_keys_after_access() {
    let program = format!(
        "{KEYS} var a = Temporal.Duration.prototype.round, b = Temporal.Duration.prototype.abs, \
         c = Temporal.Instant.prototype.round, d = Temporal.Instant.compare; \
         k(Temporal.Duration.prototype) + '|' + k(Temporal.Instant.prototype) + '|' + k(Temporal.Instant)"
    );
    assert_eq!(
        run(&program),
        format!("{DURATION},constructor,{TAG}|{INSTANT},constructor,{TAG}|{INSTANT_CONSTRUCTOR}")
    );
}

#[test]
fn delete_in_table_reifies_all() {
    let program = format!(
        "{KEYS} var P = Temporal.Duration.prototype, a = P.round, b = P.abs; delete P.with; k(P)"
    );
    assert_eq!(run(&program), format!("{DURATION_AFTER_DELETE},{TAG}"));

    let program = format!("{KEYS} var P = Temporal.Instant.prototype, a = P.round; delete P.toString; k(P)");
    assert_eq!(run(&program), format!("{INSTANT_AFTER_DELETE},{TAG}"));

    let program = format!(
        "{KEYS} var C = Temporal.Instant, a = C.compare, b = C.fromEpochMilliseconds; delete C.from; k(C)"
    );
    assert_eq!(run(&program), "length,name,prototype,compare,fromEpochMilliseconds,fromEpochNanoseconds");
}

#[test]
fn delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var P = Temporal.Duration.prototype; delete P.constructor; k(P)");
    assert_eq!(run(&program), format!("{DURATION},{TAG}"));

    let program = format!("{KEYS} var C = Temporal.Instant; delete C.length; k(C)");
    assert_eq!(run(&program), "from,fromEpochMilliseconds,fromEpochNanoseconds,compare,name,prototype");
}

#[test]
fn descriptors() {
    assert_eq!(
        run("var d = Object.getOwnPropertyDescriptor(Temporal.Duration.prototype, 'years'); \
             [typeof d.get, d.set, d.enumerable, d.configurable, d.get.length].join()"),
        "function,,false,true,0"
    );
    assert_eq!(
        run("var d = Object.getOwnPropertyDescriptor(Temporal.Duration.prototype, 'round'); \
             [d.value.length, d.value.name, d.enumerable, d.writable].join()"),
        "1,round,false,true"
    );
    assert_eq!(
        run("var d = Object.getOwnPropertyDescriptor(Temporal.Instant, 'compare'); \
             [d.value.length, d.value.name, d.enumerable, d.writable, d.configurable].join()"),
        "2,compare,false,true,true"
    );
}

// Os protótipos e construtores restantes de Temporal (ordens medidas no bun 1.4.2). Todos listam a tabela, depois
// `constructor` e `@@toStringTag` (protótipos) ou `length`, `name`, `prototype` (construtores).
const PLAIN_DATE_TABLE: &str = "toPlainMonthDay,toPlainYearMonth,withCalendar,add,subtract,with,until,since,equals,toPlainDateTime,\
toZonedDateTime,toString,toJSON,toLocaleString,valueOf,calendarId,year,month,monthCode,day,dayOfWeek,dayOfYear,weekOfYear,yearOfWeek,\
daysInWeek,daysInMonth,daysInYear,monthsInYear,inLeapYear,era,eraYear";
const PLAIN_DATE_TIME_TABLE: &str = "add,subtract,until,since,with,withCalendar,withPlainTime,round,equals,toPlainDate,toPlainTime,\
toZonedDateTime,toString,toJSON,toLocaleString,valueOf,calendarId,year,month,monthCode,day,hour,minute,second,millisecond,microsecond,\
nanosecond,dayOfWeek,dayOfYear,weekOfYear,yearOfWeek,daysInWeek,daysInMonth,daysInYear,monthsInYear,inLeapYear,era,eraYear";
const PLAIN_MONTH_DAY_TABLE: &str = "toPlainDate,toString,toJSON,toLocaleString,with,equals,valueOf,calendarId,day,monthCode";
const PLAIN_TIME_TABLE: &str = "add,subtract,with,until,since,round,equals,toString,toJSON,toLocaleString,valueOf,hour,minute,second,\
millisecond,microsecond,nanosecond";
const PLAIN_YEAR_MONTH_TABLE: &str = "add,subtract,until,since,toPlainDate,toString,toJSON,toLocaleString,with,equals,valueOf,calendarId,\
year,month,monthCode,daysInMonth,daysInYear,monthsInYear,inLeapYear,era,eraYear";
const ZONED_TABLE: &str = "with,withPlainTime,withTimeZone,withCalendar,add,subtract,until,since,round,startOfDay,getTimeZoneTransition,\
equals,toInstant,toPlainDateTime,toPlainDate,toPlainTime,toString,toJSON,toLocaleString,valueOf,epochNanoseconds,timeZoneId,calendarId,\
year,month,monthCode,day,hour,minute,second,millisecond,microsecond,nanosecond,offset,offsetNanoseconds,dayOfWeek,dayOfYear,weekOfYear,\
yearOfWeek,hoursInDay,daysInWeek,daysInMonth,daysInYear,monthsInYear,inLeapYear,era,eraYear,epochMilliseconds";

#[test]
fn remaining_prototypes_own_keys() {
    for (class, table) in [
        ("PlainDate", PLAIN_DATE_TABLE),
        ("PlainDateTime", PLAIN_DATE_TIME_TABLE),
        ("PlainMonthDay", PLAIN_MONTH_DAY_TABLE),
        ("PlainTime", PLAIN_TIME_TABLE),
        ("PlainYearMonth", PLAIN_YEAR_MONTH_TABLE),
        ("ZonedDateTime", ZONED_TABLE),
    ] {
        assert_eq!(run(&format!("{KEYS} k(Temporal.{class}.prototype)")), format!("{table},constructor,{TAG}"), "{class}");
    }
}

#[test]
fn remaining_constructors_own_keys() {
    for (class, keys) in [
        ("Duration", "from,compare,length,name,prototype"),
        ("PlainDate", "from,compare,length,name,prototype"),
        ("PlainDateTime", "from,compare,length,name,prototype"),
        ("PlainMonthDay", "from,length,name,prototype"),
        ("PlainTime", "from,compare,length,name,prototype"),
        ("PlainYearMonth", "from,compare,length,name,prototype"),
        ("ZonedDateTime", "from,compare,length,name,prototype"),
    ] {
        assert_eq!(run(&format!("{KEYS} k(Temporal.{class})")), keys, "{class}");
    }
}

#[test]
fn remaining_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var P = Temporal.PlainDate.prototype, a = P.add; delete P.with; k(P)");
    assert_eq!(
        run(&program),
        format!(
            "constructor,add,toPlainMonthDay,toPlainYearMonth,withCalendar,subtract,until,since,equals,toPlainDateTime,toZonedDateTime,\
             toString,toJSON,toLocaleString,valueOf,calendarId,year,month,monthCode,day,dayOfWeek,dayOfYear,weekOfYear,yearOfWeek,\
             daysInWeek,daysInMonth,daysInYear,monthsInYear,inLeapYear,era,eraYear,{TAG}"
        )
    );

    let program = format!("{KEYS} var C = Temporal.PlainTime, a = C.compare; delete C.from; k(C)");
    assert_eq!(run(&program), "length,name,prototype,compare");

    let program = format!("{KEYS} var C = Temporal.Duration, a = C.from; delete C.compare; k(C)");
    assert_eq!(run(&program), "length,name,prototype,from");

    let program = format!("{KEYS} var P = Temporal.ZonedDateTime.prototype, a = P.round; delete P.epochMilliseconds; k(P)");
    assert_eq!(
        run(&program),
        format!(
            "constructor,round,with,withPlainTime,withTimeZone,withCalendar,add,subtract,until,since,startOfDay,getTimeZoneTransition,\
             equals,toInstant,toPlainDateTime,toPlainDate,toPlainTime,toString,toJSON,toLocaleString,valueOf,epochNanoseconds,timeZoneId,\
             calendarId,year,month,monthCode,day,hour,minute,second,millisecond,microsecond,nanosecond,offset,offsetNanoseconds,\
             dayOfWeek,dayOfYear,weekOfYear,yearOfWeek,hoursInDay,daysInWeek,daysInMonth,daysInYear,monthsInYear,inLeapYear,era,eraYear,\
             {TAG}"
        )
    );
}

#[test]
fn remaining_delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var C = Temporal.PlainMonthDay; delete C.length; k(C)");
    assert_eq!(run(&program), "from,name,prototype");
}

#[test]
fn remaining_descriptors() {
    assert_eq!(
        run("var d = Object.getOwnPropertyDescriptor(Temporal.PlainDate.prototype, 'year'); \
             [typeof d.get, d.set, d.enumerable, d.configurable, d.get.length].join()"),
        "function,,false,true,0"
    );
    assert_eq!(
        run("var d = Object.getOwnPropertyDescriptor(Temporal.PlainTime, 'compare'); \
             [d.value.length, d.value.name, d.enumerable, d.writable, d.configurable].join()"),
        "2,compare,false,true,true"
    );
}
