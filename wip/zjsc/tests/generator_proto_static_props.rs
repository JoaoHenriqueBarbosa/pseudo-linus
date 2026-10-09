//! `generatorPrototypeTable` (`next`, `return`, `throw`), `asyncGeneratorPrototypeTable` (`return`, `throw`) e
//! `jsIteratorHelperPrototypeTable` (`next`, `return`) são reificadas no primeiro acesso. Ordens medidas no bun
//! 1.4.2, cada caso num processo novo (os três protótipos são compartilhados no realm): antes e depois de acessar
//! a lista é a mesma (nomes da tabela na frente de `constructor`); `delete` de um nome da tabela reifica tudo e a
//! ordem passa a ser a da `Structure`; `delete` de um nome fora da tabela (`constructor`, `@@toStringTag`) não reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const SETUP: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); }; \
    var G = Object.getPrototypeOf(function* () {}).prototype; \
    var A = Object.getPrototypeOf(async function* () {}).prototype; \
    var I = Object.getPrototypeOf([1].values().map(function (x) { return x; }));";

fn probe(body: &str) -> String {
    run(&format!("{SETUP} {body}"))
}

#[test]
fn own_keys_before_access() {
    assert_eq!(
        probe("k(G) + '|' + k(A) + '|' + k(I)"),
        "next,return,throw,constructor,Symbol(Symbol.toStringTag)|return,throw,next,constructor,Symbol(Symbol.toStringTag)|next,return,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn own_keys_after_access() {
    let program = "var x = [G.throw, G.next, A.return, A.throw, I.return, I.next]; k(G) + '|' + k(A) + '|' + k(I)";
    assert_eq!(
        probe(program),
        "next,return,throw,constructor,Symbol(Symbol.toStringTag)|return,throw,next,constructor,Symbol(Symbol.toStringTag)|next,return,Symbol(Symbol.toStringTag)"
    );
}

#[test]
fn generator_delete_constructor_does_not_reify() {
    assert_eq!(probe("delete G.constructor + '|' + k(G)"), "true|next,return,throw,Symbol(Symbol.toStringTag)");
}

#[test]
fn async_generator_delete_constructor_does_not_reify() {
    assert_eq!(probe("delete A.constructor + '|' + k(A)"), "true|return,throw,next,Symbol(Symbol.toStringTag)");
}

#[test]
fn iterator_helper_delete_to_string_tag_does_not_reify() {
    assert_eq!(probe("delete I[Symbol.toStringTag] + '|' + k(I)"), "true|next,return");
}

#[test]
fn generator_delete_next_reifies_all() {
    assert_eq!(probe("delete G.next + '|' + k(G)"), "true|constructor,return,throw,Symbol(Symbol.toStringTag)");
}

#[test]
fn generator_throw_accessed_then_delete_next() {
    assert_eq!(probe("void G.throw; delete G.next + '|' + k(G)"), "true|constructor,throw,return,Symbol(Symbol.toStringTag)");
}

#[test]
fn async_generator_delete_throw_reifies_all() {
    assert_eq!(probe("delete A.throw + '|' + k(A)"), "true|next,constructor,return,Symbol(Symbol.toStringTag)");
}

#[test]
fn async_generator_delete_next_does_not_reify() {
    assert_eq!(probe("delete A.next + '|' + k(A)"), "true|return,throw,constructor,Symbol(Symbol.toStringTag)");
}

#[test]
fn async_generator_return_accessed_then_delete() {
    assert_eq!(probe("void A.throw; delete A.return + '|' + k(A)"), "true|next,constructor,throw,Symbol(Symbol.toStringTag)");
}

#[test]
fn iterator_helper_delete_next_reifies_all() {
    assert_eq!(probe("delete I.next + '|' + k(I)"), "true|return,Symbol(Symbol.toStringTag)");
}

#[test]
fn iterator_helper_return_accessed_then_delete_next() {
    assert_eq!(probe("void I.return; delete I.next + '|' + k(I)"), "true|return,Symbol(Symbol.toStringTag)");
}

#[test]
fn generator_descriptor_name_and_length() {
    assert_eq!(
        probe("JSON.stringify(Object.getOwnPropertyDescriptor(G, 'return')) + G.return.name + G.throw.length + G.next.length"),
        "{\"writable\":true,\"enumerable\":false,\"configurable\":true}return11"
    );
}

#[test]
fn async_generator_descriptor_name_and_length() {
    assert_eq!(
        probe("JSON.stringify(Object.getOwnPropertyDescriptor(A, 'throw')) + A.throw.name + A.return.length + A.next.length + Object.getOwnPropertyNames(A)"),
        "{\"writable\":true,\"enumerable\":false,\"configurable\":true}throw11return,throw,next,constructor"
    );
}

#[test]
fn iterator_helper_descriptor_name_and_length() {
    assert_eq!(
        probe("JSON.stringify(Object.getOwnPropertyDescriptor(I, 'next')) + I.next.name + I.return.length + I.next.length"),
        "{\"writable\":true,\"enumerable\":false,\"configurable\":true}next00"
    );
}
