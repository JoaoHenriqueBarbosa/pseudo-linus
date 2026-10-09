//! As duas entradas de `bigIntConstructorTable` (`asUintN`, `asIntN`) são reificadas no primeiro acesso. Ordens
//! medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma (`asUintN,asIntN,length,name,prototype`, os
//! nomes da tabela vêm primeiro); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: eager (`length,name,prototype`), depois os já acessados e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(BigInt).map(String).join(','); };";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), "asUintN,asIntN,length,name,prototype");
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [BigInt.asIntN, BigInt.asUintN]; typeof f[0] + '|' + k()");
    assert_eq!(run(&program), "function|asUintN,asIntN,length,name,prototype");
}

#[test]
fn own_keys_after_delete_untouched() {
    let program = format!("{KEYS} var r = delete BigInt.asUintN; r + '|' + k()");
    assert_eq!(run(&program), "true|length,name,prototype,asIntN");
}

#[test]
fn own_keys_after_delete_with_access() {
    let program = format!("{KEYS} var f = [BigInt.asIntN, BigInt.asUintN]; var r = delete BigInt.asIntN; r + '|' + k() + '|' + typeof BigInt.asIntN");
    assert_eq!(run(&program), "true|length,name,prototype,asUintN|undefined");
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(BigInt, 'asIntN'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, BigInt.asIntN.length, BigInt.asUintN.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,2,asUintN");
}
