//! `errorPrototypeTable` (`toString`) e `promisePrototypeTable` (`finally`) reificam no primeiro acesso.
//! Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma (o nome da tabela vem na frente do
//! que a `Structure` já tem); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da `Structure`;
//! `delete` de um nome fora da tabela não reifica (o nome da tabela segue na frente).
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const ERROR: &str = "toString,name,message,constructor";
const PROMISE: &str = "finally,then,catch,constructor,Symbol(Symbol.toStringTag)";

#[test]
fn own_keys_before_access() {
    assert_eq!(
        run(&format!("{KEYS} k(Error.prototype) + '|' + k(Promise.prototype) + '|' + k(TypeError.prototype)")),
        format!("{ERROR}|{PROMISE}|name,message,constructor")
    );
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var a = Error.prototype.toString, b = Promise.prototype.finally; k(Error.prototype) + '|' + k(Promise.prototype)");
    assert_eq!(run(&program), format!("{ERROR}|{PROMISE}"));
}

#[test]
fn error_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete Error.prototype.toString; r + '|' + k(Error.prototype) + '|' + typeof Error.prototype.toString");
    assert_eq!(run(&program), "true|name,message,constructor|function");
}

#[test]
fn error_accessed_then_delete_outside_table() {
    let program = format!("{KEYS} var a = Error.prototype.toString; var r = delete Error.prototype.message; r + '|' + k(Error.prototype)");
    assert_eq!(run(&program), "true|toString,name,constructor");
}

#[test]
fn native_error_prototype_has_no_table() {
    let program = "var r = delete TypeError.prototype.toString; r + '|' + TypeError.prototype.hasOwnProperty('toString') + '|' + Error.prototype.hasOwnProperty('toString')";
    assert_eq!(run(program), "true|false|true");
}

#[test]
fn promise_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete Promise.prototype.finally; r + '|' + k(Promise.prototype) + '|' + typeof Promise.prototype.finally");
    assert_eq!(run(&program), "true|then,catch,constructor,Symbol(Symbol.toStringTag)|undefined");
}

#[test]
fn promise_accessed_then_delete_in_table() {
    let program = format!("{KEYS} var a = Promise.prototype.finally; var r = delete Promise.prototype.finally; r + '|' + k(Promise.prototype)");
    assert_eq!(run(&program), "true|then,catch,constructor,Symbol(Symbol.toStringTag)");
}

#[test]
fn promise_accessed_then_delete_outside_table() {
    let program = format!("{KEYS} var a = Promise.prototype.finally; var r = delete Promise.prototype.catch; r + '|' + k(Promise.prototype)");
    assert_eq!(run(&program), "true|finally,then,constructor,Symbol(Symbol.toStringTag)");
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Error.prototype, 'toString'); var e = Object.getOwnPropertyDescriptor(Promise.prototype, 'finally'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, Error.prototype.toString.length, Error.prototype.toString.name, \
                    typeof e.value, e.writable, e.enumerable, e.configurable, Promise.prototype.finally.length, Promise.prototype.finally.name].join(',')";
    assert_eq!(run(program), "function,true,false,true,0,toString,function,true,false,true,1,finally");
}
