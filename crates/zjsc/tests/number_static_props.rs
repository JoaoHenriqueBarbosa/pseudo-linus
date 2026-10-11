//! As três entradas de `numberConstructorTable` (`isFinite`, `isNaN`, `isSafeInteger`) são reificadas no
//! primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois de acessar (inclusive `isInteger` e `parseFloat`,
//! que são eager) a lista é a mesma, com os nomes da tabela logo depois de `length,name`; `delete Number.isInteger`
//! (fora da tabela) não reifica; `delete Number.isFinite` (nome da tabela) reifica tudo e a ordem passa a ser a da
//! `Structure`: eager, depois os já acessados e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Number).map(String).join(','); };";
const EAGER: &str = "prototype,EPSILON,MAX_VALUE,MIN_VALUE,MAX_SAFE_INTEGER,MIN_SAFE_INTEGER,NEGATIVE_INFINITY,POSITIVE_INFINITY,NaN,parseInt,parseFloat,isInteger";
const TABLE: &str = "isFinite,isNaN,isSafeInteger";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), format!("length,name,{TABLE},{EAGER}"));
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [Number.isInteger, Number.parseFloat]; var g = [Number.isNaN, Number.isFinite]; typeof g[0] + '|' + k()");
    assert_eq!(run(&program), format!("function|length,name,{TABLE},{EAGER}"));
}

#[test]
fn own_keys_after_delete_outside_table() {
    let program = format!("{KEYS} var f = [Number.isInteger, Number.parseFloat]; var r = delete Number.isInteger; r + '|' + k()");
    let eager = EAGER.strip_suffix(",isInteger").unwrap();
    assert_eq!(run(&program), format!("true|length,name,{TABLE},{eager}"));
}

#[test]
fn own_keys_after_delete_in_table() {
    let program = format!("{KEYS} var f = [Number.isFinite, Number.isNaN]; var r = delete Number.isFinite; r + '|' + k() + '|' + typeof Number.isFinite");
    assert_eq!(run(&program), format!("true|length,name,{EAGER},isNaN,isSafeInteger|undefined"));
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Number, 'isSafeInteger'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Number.isSafeInteger.length, Number.isNaN.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,1,isNaN");
}
