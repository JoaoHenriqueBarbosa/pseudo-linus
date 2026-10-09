//! O texto-fonte das mensagens de `TypeError` de chamada, construção e acesso, ponta a ponta no `eval`
//! indireto (`ExceptionHelpers.cpp`: `defaultSourceAppender`, `notAFunctionSourceAppender`,
//! `invalidParameterInSourceAppender`). Os valores esperados foram medidos no bun 1.4.2.
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

fn assert_error(source: &str, name: &str, expected: &str) {
    let (actual_name, message) = thrown_error(source);
    assert_eq!(actual_name, name, "{source}");
    assert_eq!(message, expected, "{source}");
}

#[test]
fn call_of_undefined_variable() {
    assert_error("var a3; a3()", "TypeError", "a3 is not a function. (In 'a3()', 'a3' is undefined)");
}

#[test]
fn call_of_missing_method() {
    assert_error("var a6 = {}; a6.f()", "TypeError", "a6.f is not a function. (In 'a6.f()', 'a6.f' is undefined)");
}

#[test]
fn call_of_number() {
    assert_error("var a11 = 1; a11()", "TypeError", "a11 is not a function. (In 'a11()', 'a11' is 1)");
}

#[test]
fn construct_of_undefined_variable() {
    assert_error("var a4; new a4()", "TypeError", "undefined is not a constructor (evaluating 'new a4()')");
}

#[test]
fn construct_through_undefined_base() {
    assert_error("var a10; new a10.B()", "TypeError", "undefined is not an object (evaluating 'new a10.B')");
}

#[test]
fn chained_get_by_id() {
    assert_error("var a2; a2.b.c", "TypeError", "undefined is not an object (evaluating 'a2.b')");
}

#[test]
fn method_call_through_undefined_property() {
    assert_error("var o = {}; o.x.y()", "TypeError", "undefined is not an object (evaluating 'o.x.y')");
}

#[test]
fn in_with_non_object_right_hand_side() {
    assert_error("var a8; 'x' in a8", "TypeError", "a8 is not an Object. (evaluating ''x' in a8')");
}

#[test]
fn instanceof_with_undefined_right_hand_side() {
    assert_error("var a7; 1 instanceof a7", "TypeError", "Right hand side of instanceof is not an object");
}
