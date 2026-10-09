//! As classes do `Intl` (`Segmenter`, `Locale`, `Collator`, `ListFormat`, `RelativeTimeFormat`,
//! `PluralRules`) e o `Intl` global conferidos contra o C++ do JSC (`IntlSegmenter*.cpp`, `IntlLocale*.cpp`,
//! `IntlCollator*.cpp`, `IntlListFormat*.cpp`, `IntlRelativeTimeFormat*.cpp`, `IntlObject.cpp`): mensagens
//! de erro, ordem das propriedades, `ResolveLocale` e os acessores. Cada caso é um programa que devolve uma
//! string; os programas são ASCII puro.
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

/// O programa que devolve `Nome: mensagem` do erro lançado por `expression`.
fn thrown(expression: &str) -> String {
    format!("try {{ {expression}; 'sem erro' }} catch (e) {{ e.name + ': ' + e.message }}")
}

#[test]
fn segmenter_errors_and_iterator() {
    let word = "var s = new Intl.Segmenter('en', {granularity: 'word'}).segment('Hi, you'); ";
    check(&[
        (thrown("new Intl.Segmenter('en', 'x')").as_str(), "TypeError: options argument is not an object or undefined"),
        (
            thrown("new Intl.Segmenter('en', {granularity: 'x'})").as_str(),
            "RangeError: granularity must be either \"grapheme\", \"word\", or \"sentence\"",
        ),
        (
            thrown("Intl.Segmenter.prototype.segment.call({}, 'a')").as_str(),
            "TypeError: Intl.Segmenter.prototype.segment called on value that's not a Segmenter",
        ),
        (
            thrown("Object.getPrototypeOf(new Intl.Segmenter().segment('a')[Symbol.iterator]()).next.call({})").as_str(),
            "TypeError: Intl.SegmentIterator.prototype.next called on value that's not a SegmentIterator",
        ),
        (
            "Object.prototype.toString.call(new Intl.Segmenter().segment('a')[Symbol.iterator]())",
            "[object Segment String Iterator]",
        ),
        (
            format!(
                "{word}var it = s[Symbol.iterator](); var out = []; for (var r = it.next(); !r.done; r = it.next()) out.push(r.value.segment + ':' + r.value.isWordLike); out.join('|')"
            )
            .as_str(),
            "Hi:true|,:false| :false|you:true",
        ),
        (format!("{word}s.containing(5).segment + s.containing(5).index").as_str(), "you4"),
        (format!("{word}String(s.containing(7))").as_str(), "undefined"),
    ]);
}

#[test]
fn segmenter_sentences_follow_sb_rules() {
    let sentences = |text: &str| {
        format!(
            "var out = []; var it = new Intl.Segmenter('en', {{granularity: 'sentence'}}).segment('{text}')[Symbol.iterator](); for (var r = it.next(); !r.done; r = it.next()) out.push(r.value.segment); out.join('|')"
        )
    };
    check(&[
        (sentences("Hello.World").as_str(), "Hello.World"),
        (sentences("Is it? - yes").as_str(), "Is it? - yes"),
        (sentences("Is it? Yes, fine").as_str(), "Is it? |Yes, fine"),
    ]);
}

#[test]
fn list_format_options_and_messages() {
    check(&[
        (thrown("new Intl.ListFormat('en', 'x')").as_str(), "TypeError: options argument is not an object or undefined"),
        (
            thrown("new Intl.ListFormat('en', {type: 'x'})").as_str(),
            "RangeError: type must be either \"conjunction\", \"disjunction\", or \"unit\"",
        ),
        (
            thrown("new Intl.ListFormat('en', {style: 'x'})").as_str(),
            "RangeError: style must be either \"long\", \"short\", or \"narrow\"",
        ),
        ("new Intl.ListFormat('en', {style: 'short'}).format(['a', 'b', 'c'])", "a, b, & c"),
        ("new Intl.ListFormat('en', {type: 'disjunction'}).format(['a', 'b'])", "a or b"),
    ]);
}

#[test]
fn locale_info_and_options() {
    check(&[
        (
            thrown("Intl.Locale.prototype.maximize.call({})").as_str(),
            "TypeError: Intl.Locale.prototype.maximize called on value that's not a Locale",
        ),
        (
            thrown("Object.getOwnPropertyDescriptor(Intl.Locale.prototype, 'variants').get.call({})").as_str(),
            "TypeError: Intl.Locale.prototype.variants called on value that's not a Locale",
        ),
        ("new Intl.Locale('en', {firstDayOfWeek: 'mon'}).firstDayOfWeek", "mon"),
        ("new Intl.Locale('en', {firstDayOfWeek: '0'}).toString()", "en-u-fw-sun"),
        (
            thrown("new Intl.Locale('en', {firstDayOfWeek: '8'})").as_str(),
            "RangeError: firstDayOfWeek is not a well-formed firstDayOfWeek value",
        ),
        ("String(new Intl.Locale('en').firstDayOfWeek)", "undefined"),
        ("new Intl.Locale('sl-rozaj').variants", "rozaj"),
        ("String(new Intl.Locale('en').variants)", "undefined"),
        ("new Intl.Locale('en', {variants: 'posix'}).toString()", "en-u-va-posix"),
        (
            thrown("new Intl.Locale('en', {variants: 'posix-POSIX'})").as_str(),
            "RangeError: variants is not a well-formed variants value",
        ),
        ("JSON.stringify(new Intl.Locale('en-US').getWeekInfo())", "{\"firstDay\":7,\"weekend\":[6,7]}"),
        ("String(new Intl.Locale('en-GB').getWeekInfo().firstDay)", "1"),
        ("String(new Intl.Locale('pt').getWeekInfo().firstDay)", "7"),
        ("String(new Intl.Locale('en-US-u-fw-mon').getWeekInfo().firstDay)", "1"),
        ("new Intl.Locale('ar').getTextInfo().direction", "rtl"),
        ("new Intl.Locale('en').getTextInfo().direction", "ltr"),
        ("new Intl.Locale('th-TH').getCalendars().join()", "buddhist,gregory"),
        ("new Intl.Locale('en-US').getCalendars().join()", "gregory"),
        ("new Intl.Locale('en-u-ca-japanese').getCalendars().join()", "japanese"),
        ("new Intl.Locale('de').getCollations().join()", "emoji,eor,phonebk"),
        ("new Intl.Locale('en-u-co-emoji').getCollations().join()", "emoji"),
        ("String(new Intl.Locale('en').getTimeZones())", "undefined"),
        ("new Intl.Locale('en-PT').getTimeZones().join()", "Atlantic/Azores,Atlantic/Madeira,Europe/Lisbon"),
        ("new Intl.Locale('zh-Hant').maximize().toString()", "zh-Hant-TW"),
        ("new Intl.Locale('und-DE').maximize().toString()", "de-Latn-DE"),
        ("new Intl.Locale('zh-Hant-TW').minimize().toString()", "zh-TW"),
    ]);
}

#[test]
fn collator_resolve_locale_keys() {
    check(&[
        ("new Intl.Collator('en-u-co-emoji').resolvedOptions().collation", "emoji"),
        ("new Intl.Collator('en-u-co-emoji').resolvedOptions().locale", "en-u-co-emoji"),
        ("new Intl.Collator('en-u-co-phonebk').resolvedOptions().collation", "default"),
        ("new Intl.Collator('en-u-co-phonebk').resolvedOptions().locale", "en"),
        ("new Intl.Collator('en', {collation: 'eor'}).resolvedOptions().collation", "eor"),
        ("new Intl.Collator('en', {collation: 'eor', usage: 'search'}).resolvedOptions().collation", "default"),
        // A opção igual à extensão não a desloca (`resolveLocale`: `*optionsValue != value`).
        ("new Intl.Collator('en-u-kn', {numeric: true}).resolvedOptions().locale", "en-u-kn"),
        ("new Intl.Collator('en-u-kn', {numeric: false}).resolvedOptions().locale", "en"),
        ("String(new Intl.Collator('en-u-kn', {numeric: false}).resolvedOptions().numeric)", "false"),
        ("new Intl.Collator('en-u-kf-upper').resolvedOptions().caseFirst", "upper"),
        ("new Intl.Collator('en-u-kf-upper').resolvedOptions().locale", "en-u-kf-upper"),
        ("new Intl.Collator('en-u-kf-upper', {caseFirst: 'lower'}).resolvedOptions().locale", "en"),
        ("['a', 'A'].sort(new Intl.Collator('en-u-kf-upper').compare).join('')", "Aa"),
    ]);
}

#[test]
fn relative_time_format_numbering_system_key() {
    check(&[
        ("new Intl.RelativeTimeFormat('en-u-nu-latn').resolvedOptions().locale", "en-u-nu-latn"),
        ("new Intl.RelativeTimeFormat('en', {numberingSystem: 'latn'}).resolvedOptions().locale", "en"),
        ("new Intl.RelativeTimeFormat('en-u-nu-latn', {numberingSystem: 'latn'}).resolvedOptions().locale", "en-u-nu-latn"),
        ("new Intl.RelativeTimeFormat('en').resolvedOptions().numberingSystem", "latn"),
        (
            thrown("new Intl.RelativeTimeFormat('en').format(Infinity, 'day')").as_str(),
            "RangeError: number argument must be finite",
        ),
        (
            thrown("new Intl.RelativeTimeFormat('en').format(1, 'fortnight')").as_str(),
            "RangeError: unit argument is not a recognized unit type",
        ),
    ]);
}

#[test]
fn plural_rules_resolved_options_order() {
    check(&[
        (
            "Object.keys(new Intl.PluralRules('en').resolvedOptions()).join()",
            "locale,type,notation,minimumIntegerDigits,minimumFractionDigits,maximumFractionDigits,pluralCategories,roundingIncrement,roundingMode,roundingPriority,trailingZeroDisplay",
        ),
        ("new Intl.PluralRules('en', {type: 'ordinal'}).resolvedOptions().pluralCategories.join()", "one,two,few,other"),
        ("new Intl.PluralRules('pt-BR').resolvedOptions().pluralCategories.join()", "one,many,other"),
        (
            thrown("new Intl.PluralRules('en').selectRange(undefined, 1)").as_str(),
            "TypeError: start or end is undefined",
        ),
        (
            thrown("new Intl.PluralRules('en').selectRange(NaN, 1)").as_str(),
            "RangeError: Passed numbers are out of range",
        ),
    ]);
}

#[test]
fn intl_global_functions() {
    check(&[
        ("Intl.getCanonicalLocales(['EN-us', 'pt-br', 'en-US']).join()", "en-US,pt-BR"),
        ("Intl.getCanonicalLocales('zh-hant-tw').join()", "zh-Hant-TW"),
        (thrown("Intl.getCanonicalLocales('en_US')").as_str(), "RangeError: invalid language tag: en_US"),
        (thrown("Intl.getCanonicalLocales([1])").as_str(), "TypeError: locale value must be a string or object"),
        (
            "Intl.supportedValuesOf('collation').join()",
            "compat,dict,emoji,eor,phonebk,phonetic,pinyin,searchjl,stroke,trad,unihan,zhuyin",
        ),
        ("Intl.supportedValuesOf('calendar').length + ''", "16"),
        (thrown("Intl.supportedValuesOf('x')").as_str(), "RangeError: Unknown key for Intl.supportedValuesOf"),
        ("String(Intl.supportedValuesOf.length) + String(Intl.getCanonicalLocales.length)", "11"),
    ]);
}
