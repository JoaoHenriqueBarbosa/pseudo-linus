//! `webAssemblyTable` (JSWebAssembly.cpp: dez classes `PropertyCallback` e `compile`, `instantiate`, `validate`) e
//! `constructorTableWebAssemblyModule` (`customSections`, `imports`, `exports`) são reificadas no primeiro acesso.
//! Ordens medidas no bun 1.4.2: antes e depois de acessar a lista é a mesma (os nomes da tabela vêm primeiro e são
//! deduplicados); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da `Structure`: as eager
//! (`compileStreaming`, `instantiateStreaming`, `JSTag`, `promising`, `Suspending`, `SuspendError`), os já
//! acessados na ordem de acesso e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const INITIAL: &str = "CompileError,Exception,Global,Instance,LinkError,Memory,Module,RuntimeError,Table,Tag,compile,instantiate,validate,\
compileStreaming,instantiateStreaming,JSTag,promising,Suspending,SuspendError,Symbol(Symbol.toStringTag)";
const AFTER_DELETE: &str = "compileStreaming,instantiateStreaming,JSTag,promising,Suspending,SuspendError,Table,validate,\
CompileError,Exception,Global,Instance,LinkError,Module,RuntimeError,Tag,compile,instantiate,Symbol(Symbol.toStringTag)";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k(WebAssembly)")), INITIAL);
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var f = [WebAssembly.Table, WebAssembly.validate]; typeof f[0] + '|' + k(WebAssembly)");
    assert_eq!(run(&program), format!("function|{INITIAL}"));
}

#[test]
fn own_keys_after_delete() {
    let program = format!(
        "{KEYS} var f = [WebAssembly.Table, WebAssembly.validate]; var r = delete WebAssembly.Memory; \
         r + '|' + k(WebAssembly) + '|' + typeof WebAssembly.Memory"
    );
    assert_eq!(run(&program), format!("true|{AFTER_DELETE}|undefined"));
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(WebAssembly, 'Module'); \
                   var v = Object.getOwnPropertyDescriptor(WebAssembly, 'validate'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, typeof v.value, v.writable, v.enumerable, v.configurable, \
                    WebAssembly.validate.length, WebAssembly.compile.name, WebAssembly.instantiate.length].join(',')";
    assert_eq!(run(program), "function,true,false,true,function,true,true,true,1,compile,1");
}

#[test]
fn module_own_keys_before_and_after_access() {
    let before = format!("{KEYS} k(WebAssembly.Module)");
    assert_eq!(run(&before), "customSections,imports,exports,length,name,prototype");
    let after = format!("{KEYS} var f = WebAssembly.Module.imports; k(WebAssembly.Module)");
    assert_eq!(run(&after), "customSections,imports,exports,length,name,prototype");
}

#[test]
fn module_own_keys_after_delete() {
    let program = format!("{KEYS} var f = WebAssembly.Module.imports; var r = delete WebAssembly.Module.exports; r + '|' + k(WebAssembly.Module)");
    assert_eq!(run(&program), "true|length,name,prototype,imports,customSections");
}

#[test]
fn module_descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(WebAssembly.Module, 'customSections'); \
                   [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.length, WebAssembly.Module.imports.length, WebAssembly.Module.exports.name].join(',')";
    assert_eq!(run(program), "function,true,true,true,2,1,exports");
}
