//! O `createNotAnObjectError` dos acessos a propriedade com base `undefined` ou `null` no LLInt, ponta a
//! ponta: o `eval` indireto lança um `TypeError` cuja mensagem leva o texto-fonte da expressão
//! (`ErrorInstance::appendSourceToMessage`), como `undefined is not an object (evaluating 'o.x')`.
use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::runtime::error_instance::ErrorInstance;
use zjsc::runtime::js_value::JSValue;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// `(name, message)` do erro lançado por `source`; falha se o programa não lançar um `ErrorInstance`.
fn thrown_error(source: &str) -> (String, String) {
    let thrown: JSValue = match evaluate_indirect_eval(source) {
        Err(thrown) => thrown,
        Ok(_) => panic!("{source}: não lançou"),
    };
    assert!(thrown.is_cell(), "{source}: o valor lançado não é objeto");
    let error = ErrorInstance::from_cell_id(thrown.as_cell()).unwrap_or_else(|| panic!("{source}: o valor lançado não é um Error"));
    let message = String::from_utf8(error.message().utf8(ConversionMode::LenientConversion)).expect("mensagem em UTF-8");
    (error.name().to_string(), message)
}

/// Confere o `TypeError` e a mensagem de `source`.
fn assert_type_error(source: &str, expected: &str) {
    let (name, message) = thrown_error(source);
    assert_eq!(name, "TypeError", "{source}");
    assert_eq!(message, expected, "{source}");
}

#[test]
fn get_by_id_on_undefined() {
    assert_type_error("var o = undefined; o.x", "undefined is not an object (evaluating 'o.x')");
}

#[test]
fn get_by_id_on_null() {
    assert_type_error("var o = null; o.x", "null is not an object (evaluating 'o.x')");
}

#[test]
fn get_length_on_undefined() {
    assert_type_error("var o = undefined; o.length", "undefined is not an object (evaluating 'o.length')");
}

#[test]
fn get_by_val_with_index_on_undefined() {
    assert_type_error("var o = undefined; o[0]", "undefined is not an object (evaluating 'o[0]')");
}

#[test]
fn get_by_val_with_key_on_null() {
    assert_type_error("var o = null, k = 'x'; o[k]", "null is not an object (evaluating 'o[k]')");
}

#[test]
fn put_by_id_on_undefined() {
    assert_type_error("var o = undefined; o.x = 1", "undefined is not an object (evaluating 'o.x = 1')");
}

#[test]
fn put_by_val_on_null() {
    assert_type_error("var o = null, k = 'x'; o[k] = 1", "null is not an object (evaluating 'o[k] = 1')");
}

#[test]
fn in_with_undefined_right_hand_side_keeps_its_own_message() {
    let (name, message) = thrown_error("var o = undefined; 'x' in o");
    assert_eq!(name, "TypeError");
    assert_eq!(message, "o is not an Object. (evaluating ''x' in o')");
}
