//! As duas entradas de `jsonTable` (`parse`, `stringify`) são reificadas no primeiro acesso. Ordens medidas no
//! bun 1.4.2: antes e depois de acessar a lista é a mesma (`parse,stringify,isRawJSON,rawJSON,Symbol(Symbol.toStringTag)`,
//! os nomes da tabela vêm primeiro); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: eager (`isRawJSON,rawJSON`, `@@toStringTag`), depois os já acessados e o resto da tabela.
//! `delete JSON.rawJSON` (fora da tabela) não reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(JSON).map(String).join(','); };";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), "parse,stringify,isRawJSON,rawJSON,Symbol(Symbol.toStringTag)");
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [JSON.stringify, JSON.parse]; typeof f[0] + '|' + k()");
    assert_eq!(run(&program), "function|parse,stringify,isRawJSON,rawJSON,Symbol(Symbol.toStringTag)");
}

#[test]
fn own_keys_after_delete_untouched() {
    let program = format!("{KEYS} var r = delete JSON.parse; r + '|' + k()");
    assert_eq!(run(&program), "true|isRawJSON,rawJSON,stringify,Symbol(Symbol.toStringTag)");
}

#[test]
fn own_keys_after_delete_with_access() {
    let program = format!("{KEYS} var a = JSON.stringify; var r = delete JSON.stringify; r + '|' + k() + '|' + typeof JSON.stringify");
    assert_eq!(run(&program), "true|isRawJSON,rawJSON,parse,Symbol(Symbol.toStringTag)|undefined");
}

#[test]
fn delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete JSON.rawJSON; r + '|' + k()");
    assert_eq!(run(&program), "true|parse,stringify,isRawJSON,Symbol(Symbol.toStringTag)");
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(JSON, 'parse'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, JSON.parse.length, JSON.stringify.length, JSON.stringify.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,2,3,stringify");
}
