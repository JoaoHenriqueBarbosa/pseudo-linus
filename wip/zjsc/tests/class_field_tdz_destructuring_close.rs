//! Duas provas ponta a ponta, com as mensagens medidas no bun 1.4.2 via `(0, eval)(source)`:
//!
//! 1. TDZ em inicializador de campo de classe: a leitura direta (frame privado do inicializador) dá
//!    `Cannot access '' before initialization.` (`create_tdz_error_from_source_range` sem trecho do fonte);
//!    a leitura dentro de uma arrow no inicializador dá o nome (`'y'`); a chave computada `[k]` também.
//! 2. `[o.x] = iter` com setter que lança: o iterador é fechado (`return` chamado) depois do setter e a
//!    exceção do setter propaga (`GetterSetter::call_setter` devolve `PutError::Pending`).
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

fn assert_tdz(source: &str, name: &str) {
    let (kind, message) = thrown_error(source);
    assert_eq!(kind, "ReferenceError", "{source}");
    assert_eq!(message, format!("Cannot access '{name}' before initialization."), "{source}");
}

#[test]
fn static_field_direct_read_has_empty_name() {
    assert_tdz("class A { static x = y } let y", "");
    assert_tdz("class A { static x = y; } let y; 1", "");
}

#[test]
fn instance_field_direct_read_has_empty_name() {
    assert_tdz("class A { x = y } new A; let y", "");
}

#[test]
fn static_field_arrow_read_has_variable_name() {
    assert_tdz("class A { static x = () => y; static z = A.x() } let y", "y");
    assert_tdz("class A { static x = (() => y)() } let y", "y");
}

#[test]
fn instance_field_arrow_read_has_variable_name() {
    assert_tdz("class A { x = () => y } new A().x(); let y", "y");
}

#[test]
fn computed_field_key_has_variable_name() {
    assert_tdz("class A { static [k] = 1 } let k", "k");
    assert_tdz("class A { [k] = 1 } let k", "k");
}

#[test]
fn array_destructuring_closes_iterator_when_setter_throws() {
    let source = "var log = [];
        var it = { [Symbol.iterator]() { return { next() { log.push('next'); return { value: 1, done: false }; },
                                                  return() { log.push('return'); return {}; } }; } };
        var o = { set x(v) { log.push('setter'); throw new Error('boom'); } };
        var caught;
        try { [o.x] = it; } catch (e) { caught = e.message; }
        if (log.join() !== 'next,setter,return' || caught !== 'boom') throw new Error(log.join() + '|' + caught);";
    if let Err(thrown) = evaluate_indirect_eval(source) {
        let error = ErrorInstance::from_cell_id(thrown.as_cell()).expect("Error");
        let message = String::from_utf8(error.message().utf8(ConversionMode::LenientConversion)).expect("UTF-8");
        panic!("ordem inesperada (esperado 'next,setter,return|boom'): {message}");
    }
}
