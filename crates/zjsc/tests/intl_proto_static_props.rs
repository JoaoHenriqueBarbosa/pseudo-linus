//! Os protótipos `Intl.Collator`, `Intl.NumberFormat` e `Intl.DateTimeFormat` têm tabela estática
//! (`collatorPrototypeTable`, `numberFormatPrototypeTable`, `dateTimeFormatPrototypeTable`): `format`/`compare`
//! (`DontEnum|ReadOnly|CustomAccessor`) e os métodos (`DontEnum|Function`) reificam no primeiro acesso. Eager ficam o
//! `@@toStringTag` e o `constructor`. Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a da tabela,
//! `constructor` e o `@@toStringTag`; `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: o `constructor`, os já acessados na ordem de acesso e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

fn keys(class: &str) -> String {
    format!("var P = Intl.{class}.prototype; var k = function () {{ return Reflect.ownKeys(P).map(String).join(','); }};")
}

const COLLATOR: &str = "compare,resolvedOptions,constructor,Symbol(Symbol.toStringTag)";
const FORMAT_LIST: &str = "format,formatRange,formatRangeToParts,formatToParts,resolvedOptions,constructor,Symbol(Symbol.toStringTag)";

#[test]
fn collator_own_keys_before_and_after_access() {
    assert_eq!(run(&format!("{} k()", keys("Collator"))), COLLATOR);
    let program = format!("{} var a = P.resolvedOptions; var b = Object.getOwnPropertyDescriptor(P, 'compare'); k()", keys("Collator"));
    assert_eq!(run(&program), COLLATOR);
}

#[test]
fn collator_descriptors() {
    let program = format!(
        "{} var f = Object.getOwnPropertyDescriptor(P, 'resolvedOptions'); var d = Object.getOwnPropertyDescriptor(P, 'compare'); \
         [typeof f.value, f.writable, f.enumerable, f.configurable, f.value.length, f.value.name, Object.keys(d).join('/'), d.enumerable, \
         d.configurable, d.get.name, d.get.length, typeof d.set].join(',')",
        keys("Collator")
    );
    assert_eq!(run(&program), "function,true,false,true,0,resolvedOptions,get/set/enumerable/configurable,false,true,get compare,0,undefined");
}

#[test]
fn collator_delete_reifies_all() {
    let program = format!("{} var r = delete P.compare; r + '|' + k()", keys("Collator"));
    assert_eq!(run(&program), "true|constructor,resolvedOptions,Symbol(Symbol.toStringTag)");
    let program = format!("{} var a = P.resolvedOptions; var r = delete P.resolvedOptions; r + '|' + k()", keys("Collator"));
    assert_eq!(run(&program), "true|constructor,compare,Symbol(Symbol.toStringTag)");
}

#[test]
fn collator_delete_constructor_does_not_reify() {
    let program = format!("{} var r = delete P.constructor; r + '|' + k()", keys("Collator"));
    assert_eq!(run(&program), "true|compare,resolvedOptions,Symbol(Symbol.toStringTag)");
}

#[test]
fn collator_has_and_has_own() {
    let program = format!("{} ('compare' in P) + ',' + P.hasOwnProperty('compare') + '|' + k()", keys("Collator"));
    assert_eq!(run(&program), format!("true,true|{COLLATOR}"));
}

fn format_class_checks(class: &str) {
    assert_eq!(run(&format!("{} k()", keys(class))), FORMAT_LIST);
    let program = format!("{} var a = P.resolvedOptions; var b = P.formatRange; k()", keys(class));
    assert_eq!(run(&program), FORMAT_LIST);

    let program = format!(
        "{} var f = Object.getOwnPropertyDescriptor(P, 'formatRange'); var d = Object.getOwnPropertyDescriptor(P, 'format'); \
         [typeof f.value, f.writable, f.enumerable, f.configurable, f.value.length, f.value.name, Object.keys(d).join('/'), d.enumerable, \
         d.configurable, d.get.name, d.get.length, typeof d.set].join(',')",
        keys(class)
    );
    assert_eq!(run(&program), "function,true,false,true,2,formatRange,get/set/enumerable/configurable,false,true,get format,0,undefined");

    let program = format!("{} var r = delete P.format; r + '|' + k()", keys(class));
    assert_eq!(
        run(&program),
        "true|constructor,formatRange,formatRangeToParts,formatToParts,resolvedOptions,Symbol(Symbol.toStringTag)"
    );

    let program = format!("{} var a = P.resolvedOptions; var b = P.formatRange; var r = delete P.formatRangeToParts; r + '|' + k()", keys(class));
    assert_eq!(run(&program), "true|constructor,resolvedOptions,formatRange,format,formatToParts,Symbol(Symbol.toStringTag)");

    let program = format!("{} var r = delete P.constructor; r + '|' + k()", keys(class));
    assert_eq!(
        run(&program),
        "true|format,formatRange,formatRangeToParts,formatToParts,resolvedOptions,Symbol(Symbol.toStringTag)"
    );

    let program = format!("{} ('format' in P) + ',' + P.hasOwnProperty('format') + '|' + k()", keys(class));
    assert_eq!(run(&program), format!("true,true|{FORMAT_LIST}"));
}

#[test]
fn number_format_prototype() {
    format_class_checks("NumberFormat");
}

#[test]
fn date_time_format_prototype() {
    format_class_checks("DateTimeFormat");
}

#[test]
fn getters_and_methods_still_work() {
    let program = "var c = new Intl.Collator(); var n = new Intl.NumberFormat('en-US'); var d = new Intl.DateTimeFormat('en-US', { timeZone: 'UTC' }); \
        [c.compare('a', 'b'), n.format(1234.5), d.format(0), typeof n.formatToParts, n.resolvedOptions().locale].join('|')";
    assert_eq!(run(program), "-1|1,234.5|1/1/1970|function|en-US");
}

/// PluralRules, RelativeTimeFormat, ListFormat e DisplayNames: só métodos `DontEnum|Function` na tabela.
/// Medido no bun 1.4.2: a ordem antes e depois de acessar é a da tabela, `constructor`, `@@toStringTag`; com
/// `resolvedOptions` acessado, `delete P.resolvedOptions` dá `constructor`, o resto da tabela, `@@toStringTag`.
fn method_class_checks(class: &str, table: &str, rest: &str, first: &str, first_length: u32) {
    let before = format!("{table},constructor,Symbol(Symbol.toStringTag)");
    assert_eq!(run(&format!("{} k()", keys(class))), before);
    let program = format!("{} var a = P.resolvedOptions; var b = Object.getOwnPropertyDescriptor(P, '{first}'); k()", keys(class));
    assert_eq!(run(&program), before);
    let program = format!(
        "{} var d = Object.getOwnPropertyDescriptor(P, 'resolvedOptions'); [typeof d.value, d.writable, d.enumerable, d.configurable, \
         d.value.length, d.value.name, P.{first}.length].join(',')",
        keys(class)
    );
    assert_eq!(run(&program), format!("function,true,false,true,0,resolvedOptions,{first_length}"));
    let program = format!("{} var a = P.resolvedOptions; var r = delete P.resolvedOptions; r + '|' + k()", keys(class));
    assert_eq!(run(&program), format!("true|constructor,{rest},Symbol(Symbol.toStringTag)"));
    let program = format!("{} var r = delete P.constructor; r + '|' + k()", keys(class));
    assert_eq!(run(&program), format!("true|{table},Symbol(Symbol.toStringTag)"));
}

#[test]
fn plural_rules_prototype() {
    method_class_checks("PluralRules", "select,selectRange,resolvedOptions", "select,selectRange", "select", 1);
}

#[test]
fn relative_time_format_prototype() {
    method_class_checks("RelativeTimeFormat", "format,formatToParts,resolvedOptions", "format,formatToParts", "format", 2);
}

#[test]
fn list_format_prototype() {
    method_class_checks("ListFormat", "format,formatToParts,resolvedOptions", "format,formatToParts", "format", 1);
}

#[test]
fn display_names_prototype() {
    method_class_checks("DisplayNames", "of,resolvedOptions", "of", "of", 1);
}

/// Locale: 10 métodos e 12 getters `CustomAccessor` (`get X`, comprimento 0, sem setter, não enumerável,
/// configurável). Medido no bun 1.4.2: a ordem antes e depois do acesso é a da tabela, `constructor`,
/// `@@toStringTag`; com `maximize` e `getWeekInfo` reificados, `delete P.baseName` dá `constructor`, `maximize`,
/// `getWeekInfo`, o resto da tabela sem `baseName`, `@@toStringTag`.
const LOCALE_TABLE: &str = "maximize,minimize,toString,getCalendars,getCollations,getHourCycles,getNumberingSystems,getTimeZones,\
    getTextInfo,getWeekInfo,baseName,calendar,caseFirst,collation,firstDayOfWeek,hourCycle,numeric,numberingSystem,language,script,\
    region,variants";

#[test]
fn locale_prototype() {
    let before = format!("{LOCALE_TABLE},constructor,Symbol(Symbol.toStringTag)");
    assert_eq!(run(&format!("{} k()", keys("Locale"))), before);
    let program = format!("{} var a = P.maximize; k()", keys("Locale"));
    assert_eq!(run(&program), before);
    let program = format!(
        "{} var d = Object.getOwnPropertyDescriptor(P, 'baseName'); [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length].join(',')",
        keys("Locale")
    );
    assert_eq!(run(&program), "function,,false,true,get baseName,0");
    let program = format!(
        "{} var m = Object.getOwnPropertyDescriptor(P, 'getWeekInfo'); [typeof m.value, m.writable, m.enumerable, m.configurable, m.value.length].join(',')",
        keys("Locale")
    );
    assert_eq!(run(&program), "function,true,false,true,0");
    let program = format!(
        "{} var a = P.maximize; var b = Object.getOwnPropertyDescriptor(P, 'getWeekInfo'); var r = delete P.baseName; r + '|' + k()",
        keys("Locale")
    );
    let rest = LOCALE_TABLE.replace(",baseName", "").replace("maximize,", "").replace(",getWeekInfo", "");
    assert_eq!(run(&program), format!("true|constructor,maximize,getWeekInfo,{rest},Symbol(Symbol.toStringTag)"));
    let program = "var l = new Intl.Locale('en-Latn-US-u-hc-h23-kn'); [l.baseName, l.language, l.script, l.region, l.hourCycle, l.numeric, l.maximize().toString()].join('|')";
    assert_eq!(run(program), "en-Latn-US|en|Latn|US|h23|true|en-Latn-US-u-hc-h23-kn");
}

#[test]
fn duration_format_prototype() {
    method_class_checks("DurationFormat", "format,formatToParts,resolvedOptions", "format,formatToParts", "format", 1);
}

/// Segmenter, `%Segments%` e `%SegmentIteratorPrototype%`. Medido no bun 1.4.2: Segmenter
/// `segment,resolvedOptions,constructor,@@toStringTag`; `%Segments%` `containing,@@iterator` (sem `constructor`);
/// iterador `next,@@toStringTag`; métodos `function,true,false,true`; `delete` do membro da tabela deixa o resto.
#[test]
fn segmenter_prototypes() {
    method_class_checks("Segmenter", "segment,resolvedOptions", "segment", "segment", 1);
    let setup = "var segs = new Intl.Segmenter('en', {granularity:'word'}).segment('ab cd'); var SP = Object.getPrototypeOf(segs); \
         var it = segs[Symbol.iterator](); var IP = Object.getPrototypeOf(it); \
         var k = function (P) { return Reflect.ownKeys(P).map(String).join(','); }; \
         var d = function (P, n) { var x = Object.getOwnPropertyDescriptor(P, n); \
         return [typeof x.value, x.writable, x.enumerable, x.configurable, x.value.length, x.value.name].join('/'); };";
    assert_eq!(run(&format!("{setup} k(SP) + '|' + k(IP)")), "containing,Symbol(Symbol.iterator)|next,Symbol(Symbol.toStringTag)");
    assert_eq!(
        run(&format!("{setup} [d(SP, 'containing'), d(SP, Symbol.iterator), d(IP, 'next')].join(' ')")),
        "function/true/false/true/1/containing function/true/false/true/0/[Symbol.iterator] function/true/false/true/0/next"
    );
    assert_eq!(
        run(&format!(
            "{setup} [Object.getPrototypeOf(IP) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())), \
             Object.getPrototypeOf(SP) === Object.prototype, IP[Symbol.toStringTag]].join(',')"
        )),
        "true,true,Segment String Iterator"
    );
    assert_eq!(
        run(&format!("{setup} [Reflect.ownKeys(segs).length, segs.containing(1).segment, [...segs].map(function (x) {{ return x.segment; }}).join('|'), it.next().value.segment].join(',')")),
        "0,ab,ab| |cd,ab"
    );
    assert_eq!(
        run(&format!("{setup} var a = delete SP.containing; var b = delete IP.next; a + ',' + b + '|' + k(SP) + '|' + k(IP)")),
        "true,true|Symbol(Symbol.iterator)|Symbol(Symbol.toStringTag)"
    );
}
