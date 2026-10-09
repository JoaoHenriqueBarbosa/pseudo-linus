//! `CustomAccessor` lido por descritor precisa do realm na `Structure` do `slotBase`. O `Function.prototype`
//! nasce com a `Structure` sem realm (o global ainda não existe) e recebe o realm depois, na `Structure` viva
//! e não só na raiz. Medido no bun 1.4.2: `caller` e `arguments` de `Function.prototype` são acessores
//! `get caller`, não enumeráveis, configuráveis; `Object.getOwnPropertyNames` dá
//! `length,name,toString,apply,call,bind,arguments,caller,constructor`.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

#[test]
fn function_prototype_caller_descriptor_after_transitions() {
    let program = "Object.defineProperty(Function.prototype, 'zz', { value: 1, configurable: true });\
        delete Function.prototype.zz;\
        var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller');\
        [typeof d.get, d.get === d.set, d.enumerable, d.configurable, d.get.name, d.get.length].join();";
    assert_eq!(run(program), "function,false,false,true,get caller,0");
}

#[test]
fn function_prototype_own_names_order() {
    assert_eq!(
        run("Object.getOwnPropertyNames(Function.prototype).join();"),
        "length,name,toString,apply,call,bind,arguments,caller,constructor"
    );
}
