//! As três entradas de `stringConstructorTable` (`fromCharCode`, `fromCodePoint`, `raw`) são reificadas no
//! primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma, com os nomes da
//! tabela logo depois de `length,name` e `prototype` por último; `delete String.raw` (nome da tabela) reifica
//! tudo e a ordem passa a ser a da `Structure`: `prototype` primeiro e o resto da tabela na ordem do `@begin`.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(String).map(String).join(','); };";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), "length,name,fromCharCode,fromCodePoint,raw,prototype");
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var a = [String.raw, String.fromCharCode]; typeof a[0] + '|' + k()");
    assert_eq!(run(&program), "function|length,name,fromCharCode,fromCodePoint,raw,prototype");
}

#[test]
fn own_keys_after_delete_raw() {
    let program = format!("{KEYS} var a = [String.raw, String.fromCharCode]; var r = delete String.raw; r + '|' + k() + '|' + typeof String.raw");
    assert_eq!(run(&program), "true|length,name,prototype,fromCharCode,fromCodePoint|undefined");
}

#[test]
fn own_keys_after_delete_from_char_code() {
    let program = format!("{KEYS} var a = [String.fromCodePoint]; var r = delete String.fromCharCode; r + '|' + k() + '|' + typeof String.fromCharCode");
    assert_eq!(run(&program), "true|length,name,prototype,fromCodePoint,raw|undefined");
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(String, 'fromCodePoint'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, String.fromCharCode.length, String.raw.length, \
                   String.fromCodePoint.name, String.raw.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,1,1,fromCodePoint,raw");
}
