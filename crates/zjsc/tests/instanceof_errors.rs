//! Os erros de `instanceof` no LLInt (`slow_path_instanceof`, `JSObject::hasInstance`), ponta a ponta:
//! o `eval` indireto lança, e a mensagem tem de ser a que o bun 1.4.2 mede (o texto-fonte da instrução
//! entra pelos `createInvalidInstanceofParameterError*`).
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

#[test]
fn right_hand_side_that_is_not_an_object() {
    let (name, message) = thrown_error("1 instanceof 2");
    assert_eq!(name, "TypeError");
    assert_eq!(message, "Right hand side of instanceof is not an object");
}

#[test]
fn right_hand_side_object_that_is_not_callable() {
    let (name, message) = thrown_error("({}) instanceof ({})");
    assert_eq!(name, "TypeError");
    assert_eq!(message, "({}) is not a function. (evaluating '({}) instanceof ({})')");
}
