//! `numberPrototypeTable` (`toLocaleString`, `valueOf`, `toFixed`, `toExponential`, `toPrecision`) e
//! `bigIntPrototypeTable` (`toString`, `toLocaleString`, `valueOf`) reificam no primeiro acesso. Ordens medidas
//! no bun 1.4.2: antes e depois de acessar a lista é a mesma (nomes da tabela na frente do que a `Structure`
//! já tem); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da `Structure`.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const NUMBER: &str = "toLocaleString,valueOf,toFixed,toExponential,toPrecision,toString,constructor";
const BIGINT: &str = "toString,toLocaleString,valueOf,constructor,Symbol(Symbol.toStringTag)";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k(Number.prototype) + '|' + k(BigInt.prototype)")), format!("{NUMBER}|{BIGINT}"));
}

#[test]
fn own_keys_after_access() {
    let program = format!(
        "{KEYS} var a = Number.prototype.toFixed, b = Number.prototype.valueOf, c = BigInt.prototype.valueOf; \
         k(Number.prototype) + '|' + k(BigInt.prototype)"
    );
    assert_eq!(run(&program), format!("{NUMBER}|{BIGINT}"));
}

#[test]
fn number_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete Number.prototype.toFixed; r + '|' + k(Number.prototype)");
    assert_eq!(run(&program), "true|toString,constructor,toLocaleString,valueOf,toExponential,toPrecision");
}

#[test]
fn number_accessed_then_delete() {
    let program = format!("{KEYS} var a = Number.prototype.toPrecision; var r = delete Number.prototype.valueOf; r + '|' + k(Number.prototype)");
    assert_eq!(run(&program), "true|toString,constructor,toPrecision,toLocaleString,toFixed,toExponential");
}

#[test]
fn number_delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete Number.prototype.toString; r + '|' + k(Number.prototype)");
    assert_eq!(run(&program), "true|toLocaleString,valueOf,toFixed,toExponential,toPrecision,constructor");
}

#[test]
fn bigint_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete BigInt.prototype.valueOf; r + '|' + k(BigInt.prototype)");
    assert_eq!(run(&program), "true|constructor,toString,toLocaleString,Symbol(Symbol.toStringTag)");
}

#[test]
fn bigint_accessed_then_delete() {
    let program = format!("{KEYS} var a = BigInt.prototype.toLocaleString; var r = delete BigInt.prototype.toString; r + '|' + k(BigInt.prototype)");
    assert_eq!(run(&program), "true|constructor,toLocaleString,valueOf,Symbol(Symbol.toStringTag)");
}

#[test]
fn bigint_delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete BigInt.prototype.constructor; r + '|' + k(BigInt.prototype)");
    assert_eq!(run(&program), "true|toString,toLocaleString,valueOf,Symbol(Symbol.toStringTag)");
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var f = function (o, n) { var d = Object.getOwnPropertyDescriptor(o, n); \
                   return [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.length, d.value.name].join(','); }; \
                   [f(Number.prototype, 'toFixed'), f(Number.prototype, 'toLocaleString'), \
                    f(BigInt.prototype, 'toString'), f(BigInt.prototype, 'valueOf')].join('|')";
    assert_eq!(
        run(program),
        "function,true,false,true,1,toFixed|function,true,false,true,0,toLocaleString|function,true,false,true,0,toString|function,true,false,true,0,valueOf"
    );
}
