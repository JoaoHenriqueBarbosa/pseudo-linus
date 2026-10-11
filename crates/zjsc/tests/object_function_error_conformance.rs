//! `Object`, `Function` e `Error`, ponta a ponta pelo `eval` indireto: as mensagens de erro, a ordem das
//! leituras e as conversões que `ObjectConstructor.cpp`, `ObjectPrototype.cpp`, `FunctionPrototype.cpp`,
//! `FunctionConstructor.cpp`, `ErrorConstructor.cpp` e `AggregateErrorConstructor.cpp` fixam. Os textos
//! esperados saem desses arquivos C++.
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

/// Confere o nome e a mensagem do erro lançado por `source`.
fn assert_throws(source: &str, expected_name: &str, expected_message: &str) {
    let (name, message) = thrown_error(source);
    assert_eq!(name, expected_name, "{source}");
    assert_eq!(message, expected_message, "{source}");
}

/// O valor de conclusão de `source`, que tem de ser string.
fn eval_string(source: &str) -> String {
    let value = match evaluate_indirect_eval(source) {
        Ok(value) => value,
        Err(_) => panic!("{source}: lançou"),
    };
    assert!(value.is_string(), "{source}: o valor não é string");
    String::from_utf8(value.as_js_string().value().utf8(ConversionMode::LenientConversion)).expect("UTF-8")
}

#[test]
fn object_entries_and_values_reject_null_and_undefined_with_their_own_message() {
    assert_throws("Object.entries(undefined)", "TypeError", "Object.entries requires that input parameter not be null or undefined");
    assert_throws("Object.entries(null)", "TypeError", "Object.entries requires that input parameter not be null or undefined");
    assert_throws("Object.values(undefined)", "TypeError", "Object.values requires that input parameter not be null or undefined");
    assert_throws("Object.values(null)", "TypeError", "Object.values requires that input parameter not be null or undefined");
}

#[test]
fn object_assign_rejects_null_target() {
    assert_throws("Object.assign(null)", "TypeError", "Object.assign requires that input parameter not be null or undefined");
}

#[test]
fn object_get_prototype_of_undefined_is_a_type_error() {
    let (name, _) = thrown_error("Object.getPrototypeOf(undefined)");
    assert_eq!(name, "TypeError");
    assert_eq!(eval_string("Object.getPrototypeOf(1) === Number.prototype ? 'ok' : 'no'"), "ok");
}

#[test]
fn object_define_property_messages() {
    assert_throws("Object.defineProperty(1, 'a', {})", "TypeError", "Properties can only be defined on Objects.");
    assert_throws("Object.defineProperty({}, 'a', 1)", "TypeError", "Property description must be an object.");
    assert_throws("Object.defineProperty({}, 'a', { get: 1 })", "TypeError", "Getter must be a function.");
    assert_throws("Object.defineProperty({}, 'a', { set: 1 })", "TypeError", "Setter must be a function.");
    assert_throws(
        "Object.defineProperty({}, 'a', { get() {}, value: 1 })",
        "TypeError",
        "Invalid property.  'value' present on property with getter or setter.",
    );
    assert_throws(
        "Object.defineProperty({}, 'a', { get() {}, writable: true })",
        "TypeError",
        "Invalid property.  'writable' present on property with getter or setter.",
    );
}

#[test]
fn object_define_properties_converts_every_descriptor_before_defining() {
    // O segundo descritor é inválido: nada do primeiro pode ter sido definido.
    assert_eq!(
        eval_string(
            "var o = {}; try { Object.defineProperties(o, { a: { value: 1 }, b: 1 }); } catch (e) {} \
             'a' in o ? 'defined' : 'untouched'"
        ),
        "untouched"
    );
}

#[test]
fn object_define_properties_reads_descriptor_fields_in_spec_order() {
    assert_eq!(
        eval_string(
            "var log = ''; var d = new Proxy({}, { has(t, k) { log += k + ','; return false; } }); \
             Object.defineProperty({}, 'a', d); log"
        ),
        "enumerable,configurable,value,writable,get,set,"
    );
}

#[test]
fn object_set_prototype_of_and_create_messages() {
    assert_throws("Object.setPrototypeOf(undefined, null)", "TypeError", "Cannot set prototype of undefined or null");
    assert_throws("Object.setPrototypeOf({}, 1)", "TypeError", "Prototype value can only be an object or null");
    assert_throws("Object.create(1)", "TypeError", "Object prototype may only be an Object or null.");
    assert_throws("var a = {}; var b = Object.create(a); Object.setPrototypeOf(a, b)", "TypeError", "cyclic __proto__ value");
    assert_throws(
        "Object.setPrototypeOf(Object.preventExtensions({}), {})",
        "TypeError",
        "Attempted to assign to readonly property.",
    );
    assert_throws(
        "Object.setPrototypeOf(Object.prototype, {})",
        "TypeError",
        "Cannot set prototype of immutable prototype object",
    );
}

#[test]
fn underscore_proto_accessor() {
    assert_eq!(eval_string("var o = {}; o.__proto__ = 5; Object.getPrototypeOf(o) === Object.prototype ? 'ok' : 'no'"), "ok");
    assert_throws(
        "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.call(undefined, {})",
        "TypeError",
        "Object.prototype.__proto__ called on null or undefined",
    );
}

#[test]
fn define_accessor_legacy_messages() {
    assert_throws("({}).__defineGetter__('a', 1)", "TypeError", "invalid getter usage");
    assert_throws("({}).__defineSetter__('a', 1)", "TypeError", "invalid setter usage");
    assert_eq!(
        eval_string("var o = {}; o.__defineGetter__('a', function () { return 'g'; }); o.a + o.__lookupGetter__('a')()"),
        "gg"
    );
}

#[test]
fn get_own_property_descriptors_does_not_call_prototype_index_setters() {
    assert_eq!(
        eval_string(
            "Object.defineProperty(Object.prototype, '0', { set(v) { throw new Error('setter'); }, configurable: true }); \
             try { var d = Object.getOwnPropertyDescriptors({ 0: 'x' }); return_value = typeof d[0].value + d[0].value; } \
             finally { delete Object.prototype[0]; } return_value"
        ),
        "stringx"
    );
}

#[test]
fn integrity_level_messages_and_results() {
    assert_throws(
        "Object.preventExtensions(new Proxy({}, { preventExtensions() { return false; } }))",
        "TypeError",
        "Unable to prevent extension in Object.preventExtensions",
    );
    assert_throws(
        "Object.freeze(new Proxy({}, { preventExtensions() { return false; } }))",
        "TypeError",
        "Unable to prevent extension in Object.freeze",
    );
    assert_throws(
        "Object.seal(new Proxy({}, { preventExtensions() { return false; } }))",
        "TypeError",
        "Unable to prevent extension in Object.seal",
    );
    assert_eq!(eval_string("var a = Object.freeze([1, 2]); (Object.isFrozen(a) && !Object.isExtensible(a)) ? 'ok' : 'no'"), "ok");
}

#[test]
fn function_prototype_to_string_native_and_user() {
    assert_eq!(eval_string("Math.max.toString()"), "function max() { [native code] }");
    assert_eq!(eval_string("Object.toString()"), "function Object() { [native code] }");
    assert_eq!(eval_string("(function f(a, b) { return a + b; }).toString()"), "function f(a, b) { return a + b; }");
    assert_eq!(eval_string("(class A { m() {} }).toString()"), "class A { m() {} }");
    assert_eq!(eval_string("(function f() {}).bind().toString()"), "function bound f() { [native code] }");
    assert_throws("Function.prototype.toString.call({})", "TypeError", "Type error");
}

#[test]
fn function_constructor_builds_the_source_like_the_cpp() {
    assert_eq!(eval_string("Function('a', 'b', 'return a + b').toString()"), "function anonymous(a,b\n) {\nreturn a + b\n}");
    assert_eq!(eval_string("Function('return 1').toString()"), "function anonymous(\n) {\nreturn 1\n}");
    assert_eq!(eval_string("Function().toString()"), "function anonymous(\n) {\n\n}");
    assert_eq!(eval_string("String(Function('a', 'b', 'return a + b')(1, 2))"), "3");
}

#[test]
fn function_constructor_stops_at_the_first_throwing_conversion() {
    assert_eq!(
        eval_string(
            "var log = ''; \
             try { Function({ toString() { log += 'a'; throw new RangeError('x'); } }, { toString() { log += 'b'; return ''; } }, ''); } \
             catch (e) { log += e.name; } log"
        ),
        "aRangeError"
    );
}

#[test]
fn error_constructor_propagates_message_conversion_errors() {
    assert_throws("new Error({ toString() { throw new RangeError('boom'); } })", "RangeError", "boom");
    assert_throws("Error(Symbol())", "TypeError", "Cannot convert a symbol to a string");
    assert_eq!(eval_string("new Error('m', { cause: 7 }).cause + ''"), "7");
    assert_eq!(eval_string("'cause' in new Error('m', {}) ? 'yes' : 'no'"), "no");
}

#[test]
fn error_prototype_to_string_stops_at_the_first_throwing_conversion() {
    assert_eq!(
        eval_string(
            "var log = ''; var e = { name: { toString() { log += 'n'; throw new RangeError('x'); } }, \
             get message() { log += 'm'; return ''; } }; \
             try { Error.prototype.toString.call(e); } catch (x) { log += x.name; } log"
        ),
        "nRangeError"
    );
    assert_eq!(eval_string("Error.prototype.toString.call({ name: 'A', message: 'b' })"), "A: b");
    assert_eq!(eval_string("Error.prototype.toString.call({})"), "Error");
}

#[test]
fn aggregate_error_message_conversion_and_errors() {
    assert_throws("new AggregateError([], { toString() { throw new RangeError('boom'); } })", "RangeError", "boom");
    assert_eq!(eval_string("new AggregateError([1, 2], 'm').errors.length + ''"), "2");
    assert_eq!(eval_string("AggregateError.length + ''"), "2");
}
