//! As sete entradas de `promiseConstructorTable` são reificadas no primeiro acesso.
//! Ordens medidas no bun 1.4.2: antes e depois de acessar `all`/`race` a lista é a mesma (os nomes da tabela vêm
//! primeiro e são deduplicados); depois de `delete Promise.all` (nome da tabela) tudo é reificado e a ordem passa
//! a ser a da `Structure`: eager, `resolve` (reificado na criação do global), `race` (acessado) e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Promise).map(String).join(','); };";
const INITIAL: &str = "length,name,resolve,reject,race,all,allSettled,any,withResolvers,prototype,try,Symbol(Symbol.species)";
const AFTER_DELETE: &str = "length,name,prototype,try,resolve,race,reject,allSettled,any,withResolvers,Symbol(Symbol.species)";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), INITIAL);
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [Promise.all, Promise.race]; typeof f[0] + '|' + k()");
    assert_eq!(run(&program), format!("function|{INITIAL}"));
}

#[test]
fn own_keys_after_delete() {
    let program = format!("{KEYS} var f = [Promise.all, Promise.race]; var r = delete Promise.all; r + '|' + k() + '|' + typeof Promise.all");
    assert_eq!(run(&program), format!("true|{AFTER_DELETE}|undefined"));
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Promise, 'withResolvers'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Promise.withResolvers.length, Promise.all.length, Promise.all.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,0,1,all");
}
