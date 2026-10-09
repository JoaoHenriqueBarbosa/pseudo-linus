//! `ordinarySetWithOwnDescriptor` (`JSObject.cpp`, `ProxyObject::performPut`) com `JSFunction` no papel de
//! receptor ou de protótipo na cadeia. Os valores esperados saíram do bun 1.4.2 (JavaScriptCore), rodando
//! exatamente os mesmos programas.
use zjsc::api::eval::{describe_exception, evaluate_script};
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// Roda o programa e devolve o valor de conclusão convertido em texto, ou `Nome: mensagem` se lançou.
/// Numa thread de pilha grande, como os goldens de proxy.
fn run(source: &str) -> String {
    let wrapped = format!(
        "(function () {{ try {{ return String((function () {{ {source} }})()); }} catch (e) {{ return e.name + ': ' + e.message; }} }})()"
    );
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            let value = evaluate_script(&wrapped).unwrap_or_else(|thrown| panic!("lançou exceção ({}): {wrapped}", describe_exception(&thrown)));
            assert!(value.is_string(), "não devolveu string: {wrapped}");
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|_| panic!("pânico no motor"))
}

#[test]
fn reflect_set_on_proxy_with_function_receiver() {
    // O proxy sem trap repassa ao alvo com o receptor original: a propriedade nasce na função.
    assert_eq!(
        run("var l = []; var p = new Proxy({}, { set(t, k, v, r) { l.push(k + ':' + (typeof r)); return Reflect.set(t, k, v, r); } }); \
             var f = function () {}; var ok = Reflect.set(p, 'x', 1, f); \
             return [ok, f.x, Object.getOwnPropertyNames(f).includes('x'), l].join('|');"),
        "true|1|true|x:function"
    );
}

#[test]
fn function_with_proxy_prototype_assignment() {
    assert_eq!(
        run("var l = []; var f = function () {}; \
             Object.setPrototypeOf(f, new Proxy({}, { set(t, k, v, r) { l.push(k + ':' + (r === f)); return Reflect.set(t, k, v, r); } })); \
             f.x = 1; return [f.x, Object.hasOwn(f, 'x'), l].join('|');"),
        "1|true|x:true"
    );
    // Trap que devolve `true` sem gravar: a função não ganha a propriedade.
    assert_eq!(
        run("var l = []; function F() {} Object.setPrototypeOf(F, new Proxy({}, { set(t, k, v, r) { l.push(k); return true; } })); \
             F.z = 3; return [F.z, Object.hasOwn(F, 'z'), l].join('|');"),
        "|false|z"
    );
}

#[test]
fn class_instance_with_proxy_on_prototype_chain() {
    assert_eq!(
        run("var l = []; class A {} Object.setPrototypeOf(A.prototype, new Proxy({}, { set(t, k, v, r) { l.push(k); return Reflect.set(t, k, v, r); } })); \
             var a = new A; a.y = 2; return [a.y, l].join('|');"),
        "2|y"
    );
}

#[test]
fn function_prototype_chain_through_function_to_proxy() {
    assert_eq!(
        run("var f = function () {}; var g = function () {}; Object.setPrototypeOf(f, g); \
             Object.setPrototypeOf(g, new Proxy({}, { set(t, k, v, r) { return Reflect.set(t, k, v, r); } })); \
             f.w = 4; return [f.w, Object.hasOwn(f, 'w'), Object.hasOwn(g, 'w')].join('|');"),
        "4|true|false"
    );
}

#[test]
fn function_receiver_with_existing_own_properties() {
    // `name` e `length` da função são não graváveis: `existingDescriptor.[[Writable]]` falso devolve false.
    assert_eq!(run("var p = new Proxy({}, {}); var f = function () {}; return Reflect.set(p, 'name', 'zz', f) + '|' + f.name;"), "false|f");
    assert_eq!(run("var p = new Proxy({}, {}); var f = function () {}; return Reflect.set(p, 'length', 5, f) + '|' + f.length;"), "false|0");
    assert_eq!(
        run("var p = new Proxy({}, {}); var f = function () {}; Object.defineProperty(f, 'ro', { value: 1, writable: false }); \
             return Reflect.set(p, 'ro', 9, f) + '|' + f.ro;"),
        "false|1"
    );
    // Acessor próprio no receptor: false, sem chamar nada.
    assert_eq!(
        run("var p = new Proxy({}, {}); var f = function () {}; Object.defineProperty(f, 'acc', { get() { return 1; }, configurable: true }); \
             return Reflect.set(p, 'acc', 2, f) + '|' + f.acc;"),
        "false|1"
    );
    // `prototype` da função é gravável: a atualização passa.
    assert_eq!(run("var p = new Proxy({}, {}); return Reflect.set(p, 'prototype', { a: 1 }, function () {});"), "true");
}

#[test]
fn strict_assignment_through_function_with_failing_proxy_trap() {
    assert_eq!(
        run("'use strict'; var f = function () {}; Object.setPrototypeOf(f, new Proxy({}, { set() { return false; } })); f.q = 1; return 'no throw';"),
        "TypeError: Proxy object's 'set' trap returned falsy value for property 'q'"
    );
}
