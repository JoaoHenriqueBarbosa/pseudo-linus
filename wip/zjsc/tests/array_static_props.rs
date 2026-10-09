//! `Array.from` é entrada da tabela estática (`arrayConstructorTable`), reificada no primeiro acesso.
//! Ordem de `Reflect.ownKeys(Array)` medida no bun 1.4.2: a lista é a mesma antes e depois do acesso (o
//! nome da tabela vem primeiro e é deduplicado), e `delete Array.from` reifica tudo e apaga o nome.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Array).map(String).join(','); };";
const WITH_FROM: &str = "from,length,name,prototype,of,isArray,fromAsync,Symbol(Symbol.species)";
const WITHOUT_FROM: &str = "length,name,prototype,of,isArray,fromAsync,Symbol(Symbol.species)";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), WITH_FROM);
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var before = typeof Array.from; before + '|' + k()");
    assert_eq!(run(&program), format!("function|{WITH_FROM}"));
}

#[test]
fn own_keys_after_delete() {
    let program = format!("{KEYS} var r = delete Array.from; r + '|' + k() + '|' + typeof Array.from");
    assert_eq!(run(&program), format!("true|{WITHOUT_FROM}|undefined"));
}

#[test]
fn from_descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Array, 'from'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Array.from.length, Array.from.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,1,from");
}
