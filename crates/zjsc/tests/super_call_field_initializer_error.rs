//! Mensagens medidas no bun 1.4.2 via `(0, eval)(source)`:
//!
//! 1. `super.m()` não chamável dentro de inicializador de campo de classe (frame privado) cita a própria chamada,
//!    `super.m is not a function. (In 'super.m()', 'super.m' is undefined)`, e não o construtor que o chamou.
//! 2. `get_prototype_of(undefined)` do `super.m` fora de método leva o texto-fonte: `(evaluating 'super.m')`.
use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::runtime::error_instance::ErrorInstance;
use zjsc::runtime::js_value::JSValue;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn thrown_message(source: &str) -> String {
    let thrown: JSValue = match evaluate_indirect_eval(source) {
        Err(thrown) => thrown,
        Ok(_) => panic!("{source}: não lançou"),
    };
    let error = ErrorInstance::from_cell_id(thrown.as_cell()).unwrap_or_else(|| panic!("{source}: o valor lançado não é um Error"));
    String::from_utf8(error.message().utf8(ConversionMode::LenientConversion)).expect("mensagem em UTF-8")
}

const NOT_A_FUNCTION: &str = "super.m is not a function. (In 'super.m()', 'super.m' is undefined)";

#[test]
fn super_call_in_static_field_cites_the_call() {
    assert_eq!(thrown_message("function L(x) { return x } class C { static f = L(super.m()) }"), NOT_A_FUNCTION);
}

#[test]
fn super_call_in_instance_field_cites_the_call() {
    assert_eq!(
        thrown_message("function L(x) { return x } class C { f = L(super.m()) }; new C"),
        NOT_A_FUNCTION
    );
    assert_eq!(thrown_message("class C { f = super.m() }; new C"), NOT_A_FUNCTION);
}

#[test]
fn super_property_read_in_field_still_cites_the_constructor() {
    assert_eq!(
        thrown_message("class C { f = super.x.y }; new C"),
        "undefined is not an object (near '...(function () { })...')"
    );
}

#[test]
fn super_base_of_undefined_home_object_has_source_text() {
    assert_eq!(
        thrown_message("class A { m() {} } class B extends A { [super.m()] = 1 }"),
        "undefined is not an object (evaluating 'super.m')"
    );
}
