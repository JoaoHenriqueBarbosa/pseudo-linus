//! Escrita em `globalThis` quando o protótipo do global é exótico (`Object.prototype.__proto__` com
//! `this = globalThis`). A escrita chega ao receptor `JSGlobalProxy` por `definePropertyOnReceiver` (Array e
//! função na cadeia) ou por `[[Set]]` do Proxy (receptor = o proxy global); nos dois casos tem de cair no
//! `JSGlobalObject`. Medido no bun 1.4.2: o protótipo do global muda e `globalThis.R = r` segue normal.
use zjsc::api::eval::evaluate_script;

fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    value.is_true()
}

fn assert_global_write_survives(proto_expression: &str) {
    let source = format!(
        "var setter = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set; \
         var proto = {proto_expression}; \
         setter.call(globalThis, proto); \
         globalThis.R = 42; \
         Object.getPrototypeOf(globalThis) === proto && R === 42 && globalThis.R === 42 \
         && Object.prototype.hasOwnProperty.call(globalThis, 'R')"
    );
    assert!(is_true(&source), "{proto_expression}");
}

#[test]
fn array_prototype_as_global_prototype() {
    assert_global_write_survives("Array.prototype");
}

#[test]
fn function_as_global_prototype() {
    assert_global_write_survives("function () {}");
}

#[test]
fn proxy_as_global_prototype() {
    assert_global_write_survives("new Proxy({}, {})");
}

/// A leitura de `R` depois do script não monta um segundo `Program`: o `ProgramExecutable` recusaria o `Proxy`
/// na cadeia do global (`Proxy is not allowed in the global prototype chain.`), erro que o bun, rodando o
/// arquivo uma vez só, nunca mostra (caso do golden `annexb_methods`).
#[test]
fn named_script_result_survives_proxy_in_global_chain() {
    let source = "var gp = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); \
                  gp.set.call(globalThis, new Proxy({}, {})); globalThis.R = 'ok';";
    let value = zjsc::api::eval::evaluate_named_script_result(source, "proxy_chain.js", "R")
        .unwrap_or_else(|thrown| panic!("lançou exceção ({})", zjsc::api::eval::describe_exception(&thrown)));
    assert_eq!(&value.to_wtf_string().latin1()[..], b"ok");
}
