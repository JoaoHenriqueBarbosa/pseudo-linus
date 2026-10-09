//! As três entradas de `dateConstructorTable` (`parse`, `UTC`, `now`) são reificadas no primeiro acesso. Ordens
//! medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma, com os nomes da tabela na frente de
//! `length,name,prototype`; `delete Date.parse` (nome da tabela) reifica tudo e a ordem passa a ser a da
//! `Structure`: eager, depois os reificados antes do `delete` na ordem de acesso, depois o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Date).map(String).join(','); };";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), "parse,UTC,now,length,name,prototype");
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [Date.parse, Date.UTC, Date.now, Date.prototype]; typeof f[0] + '|' + k()");
    assert_eq!(run(&program), "function|parse,UTC,now,length,name,prototype");
}

#[test]
fn own_keys_after_delete_in_table_without_access() {
    let program = format!("{KEYS} var r = delete Date.parse; r + '|' + k() + '|' + typeof Date.parse");
    assert_eq!(run(&program), "true|length,name,prototype,UTC,now|undefined");
}

#[test]
fn own_keys_after_delete_in_table_after_access() {
    let program = format!("{KEYS} var a = Date.now; var r = delete Date.UTC; r + '|' + k() + '|' + typeof Date.UTC");
    assert_eq!(run(&program), "true|length,name,prototype,now,parse|undefined");
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Date, 'UTC'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Date.UTC.length, Date.parse.length, Date.now.length, Date.now.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,7,1,0,now");
}
