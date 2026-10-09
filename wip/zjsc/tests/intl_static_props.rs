//! As doze entradas de `intlObjectTable` (duas funções e dez construtores `PropertyCallback`) são reificadas no
//! primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a da tabela seguida do
//! `Symbol(Symbol.toStringTag)`; `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: o `@@toStringTag` eager, os nomes já acessados na ordem de acesso e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Intl).map(String).join(','); };";

#[test]
fn own_keys_before_access() {
    assert_eq!(
        run(&format!("{KEYS} k()")),
        "getCanonicalLocales,supportedValuesOf,Collator,DateTimeFormat,DisplayNames,DurationFormat,ListFormat,Locale,NumberFormat,PluralRules,RelativeTimeFormat,Segmenter,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [Intl.supportedValuesOf, Intl.Locale, Intl.Collator]; typeof f[0] + '|' + k()");
    assert_eq!(
        run(&program),
        "function|getCanonicalLocales,supportedValuesOf,Collator,DateTimeFormat,DisplayNames,DurationFormat,ListFormat,Locale,NumberFormat,PluralRules,RelativeTimeFormat,Segmenter,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn own_keys_after_delete_untouched() {
    let program = format!("{KEYS} var r = delete Intl.getCanonicalLocales; r + '|' + k()");
    assert_eq!(
        run(&program),
        "true|supportedValuesOf,Collator,DateTimeFormat,DisplayNames,DurationFormat,ListFormat,Locale,NumberFormat,PluralRules,RelativeTimeFormat,Segmenter,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn own_keys_after_delete_with_access() {
    let program = format!("{KEYS} var x = Intl.Segmenter, y = Intl.supportedValuesOf; var r = delete Intl.Locale; r + '|' + k()");
    assert_eq!(
        run(&program),
        "true|Segmenter,supportedValuesOf,getCanonicalLocales,Collator,DateTimeFormat,DisplayNames,DurationFormat,ListFormat,NumberFormat,PluralRules,RelativeTimeFormat,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn own_keys_after_delete_of_constructor_untouched() {
    let program = format!("{KEYS} var r = delete Intl.Collator; r + '|' + k()");
    assert_eq!(
        run(&program),
        "true|getCanonicalLocales,supportedValuesOf,DateTimeFormat,DisplayNames,DurationFormat,ListFormat,Locale,NumberFormat,PluralRules,RelativeTimeFormat,Segmenter,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Intl, 'getCanonicalLocales'); \
                   var c = Object.getOwnPropertyDescriptor(Intl, 'Collator'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Intl.getCanonicalLocales.length, Intl.supportedValuesOf.length, \
                    typeof c.value, c.writable, c.enumerable, c.configurable].join(',')";
    assert_eq!(run(program), "function,true,false,true,1,1,function,true,false,true");
}
