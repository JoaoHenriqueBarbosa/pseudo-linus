//! `booleanPrototypeTable` (`toString`, `valueOf`) e `symbolPrototypeTable` (`description`, `toString`, `valueOf`)
//! são reificadas no primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma
//! (nomes da tabela na frente de `constructor` e dos símbolos); `delete` de um nome da tabela reifica tudo e a
//! ordem passa a ser a da `Structure`: `constructor`, os já acessados e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const BOOLEAN: &str = "toString,valueOf,constructor";
const SYMBOL: &str = "description,toString,valueOf,constructor,Symbol(Symbol.toPrimitive),Symbol(Symbol.toStringTag)";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k(Boolean.prototype) + '|' + k(Symbol.prototype)")), format!("{BOOLEAN}|{SYMBOL}"));
}

#[test]
fn own_keys_after_access() {
    let program = format!(
        "{KEYS} var a = Boolean.prototype.valueOf, b = Symbol.prototype.valueOf, c = Symbol.prototype.toString; \
         var s = Symbol('x'); var d = s.description; d + '|' + k(Boolean.prototype) + '|' + k(Symbol.prototype)"
    );
    assert_eq!(run(&program), format!("x|{BOOLEAN}|{SYMBOL}"));
}

#[test]
fn boolean_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete Boolean.prototype.valueOf; r + '|' + k(Boolean.prototype)");
    assert_eq!(run(&program), "true|constructor,toString");
}

#[test]
fn boolean_delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete Boolean.prototype.constructor; r + '|' + k(Boolean.prototype)");
    assert_eq!(run(&program), "true|toString,valueOf");
}

#[test]
fn boolean_accessed_then_delete() {
    let program = format!("{KEYS} var a = Boolean.prototype.toString; var r = delete Boolean.prototype.valueOf; r + '|' + k(Boolean.prototype)");
    assert_eq!(run(&program), "true|constructor,toString");
}

#[test]
fn symbol_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete Symbol.prototype.description; r + '|' + k(Symbol.prototype)");
    assert_eq!(run(&program), "true|constructor,toString,valueOf,Symbol(Symbol.toPrimitive),Symbol(Symbol.toStringTag)");
}

#[test]
fn symbol_delete_value_of_without_access() {
    let program = format!("{KEYS} var r = delete Symbol.prototype.valueOf; r + '|' + k(Symbol.prototype)");
    assert_eq!(run(&program), "true|constructor,description,toString,Symbol(Symbol.toPrimitive),Symbol(Symbol.toStringTag)");
}

#[test]
fn symbol_to_string_accessed_then_delete_value_of() {
    let program = format!("{KEYS} var a = Symbol.prototype.toString; var r = delete Symbol.prototype.valueOf; r + '|' + k(Symbol.prototype)");
    assert_eq!(run(&program), "true|constructor,toString,description,Symbol(Symbol.toPrimitive),Symbol(Symbol.toStringTag)");
}

#[test]
fn delete_symbol_constructor_does_not_reify() {
    let program = format!("{KEYS} var r = delete Symbol.prototype.constructor; r + '|' + k(Symbol.prototype)");
    assert_eq!(run(&program), "true|description,toString,valueOf,Symbol(Symbol.toPrimitive),Symbol(Symbol.toStringTag)");
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description'); \
                   var e = Object.getOwnPropertyDescriptor(Symbol.prototype, 'toString'); \
                   var f = Object.getOwnPropertyDescriptor(Boolean.prototype, 'valueOf'); \
                   [typeof d.get, typeof d.set, d.enumerable, d.configurable, \
                    typeof e.value, e.writable, e.enumerable, e.configurable, e.value.length, e.value.name, \
                    typeof f.value, f.writable, f.enumerable, f.configurable, f.value.length, f.value.name].join(',')";
    assert_eq!(run(program), "function,undefined,false,true,function,true,false,true,0,toString,function,true,false,true,0,valueOf");
}
