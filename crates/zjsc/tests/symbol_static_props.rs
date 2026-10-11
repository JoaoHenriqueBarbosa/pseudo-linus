//! As duas entradas de `symbolConstructorTable` (`for`, `keyFor`) são reificadas no primeiro acesso. Ordens
//! medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma, com os nomes da tabela na frente de
//! `length,name,prototype` e dos símbolos conhecidos; `delete Symbol.keyFor` (nome da tabela) reifica tudo e a
//! ordem passa a ser a da `Structure`: eager, depois `for` (o único reificado antes de `keyFor` sair).
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Symbol).map(String).join(','); };";
const EAGER: &str = "length,name,prototype,hasInstance,isConcatSpreadable,asyncIterator,iterator,match,matchAll,replace,search,species,split,toPrimitive,toStringTag,unscopables,dispose,asyncDispose";
const TABLE: &str = "for,keyFor";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), format!("{TABLE},{EAGER}"));
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [Symbol.for, Symbol.keyFor, Symbol.iterator]; typeof f[0] + '|' + k()");
    assert_eq!(run(&program), format!("function|{TABLE},{EAGER}"));
}

#[test]
fn own_keys_after_delete_in_table() {
    let program = format!("{KEYS} var a = Symbol.iterator; var r = delete Symbol.keyFor; r + '|' + k() + '|' + typeof Symbol.keyFor");
    assert_eq!(run(&program), format!("true|{EAGER},for|undefined"));
}

#[test]
fn well_known_symbol_is_not_deletable() {
    let program = format!("{KEYS} var r = delete Symbol.iterator; r + '|' + k()");
    assert_eq!(run(&program), format!("false|{TABLE},{EAGER}"));
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Symbol, 'keyFor'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Symbol.keyFor.length, Symbol.for.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,1,for");
}
