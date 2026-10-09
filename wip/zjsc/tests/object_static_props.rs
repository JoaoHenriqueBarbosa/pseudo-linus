//! As funções de `Object` são entradas da tabela estática (`objectConstructorTable`), reificadas no
//! primeiro acesso. Ordens de `Reflect.ownKeys(Object)` medidas no bun 1.4.2.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Object).map(String).join(','); };";
const TABLE_ORDER: &str = "getPrototypeOf,setPrototypeOf,getOwnPropertyDescriptor,getOwnPropertyDescriptors,getOwnPropertyNames,\
getOwnPropertySymbols,keys,defineProperty,defineProperties,create,seal,freeze,preventExtensions,isSealed,isFrozen,isExtensible,\
is,assign,values,entries,fromEntries";
const STRUCTURE_HEAD: &str = "length,name,prototype,hasOwn,groupBy";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), format!("{TABLE_ORDER},{STRUCTURE_HEAD}"));
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [Object.is, Object.hasOwn, Object.entries]; k()");
    assert_eq!(run(&program), format!("{TABLE_ORDER},{STRUCTURE_HEAD}"));
}

#[test]
fn own_keys_after_delete_assign() {
    let program = format!("{KEYS} var f = [Object.is, Object.hasOwn, Object.entries]; var r = delete Object.assign; r + '|' + k()");
    let rest = "getPrototypeOf,setPrototypeOf,getOwnPropertyDescriptor,getOwnPropertyDescriptors,getOwnPropertyNames,\
getOwnPropertySymbols,keys,defineProperty,defineProperties,create,seal,freeze,preventExtensions,isSealed,isFrozen,isExtensible,\
values,fromEntries";
    assert_eq!(run(&program), format!("true|{STRUCTURE_HEAD},is,entries,{rest}"));
}
