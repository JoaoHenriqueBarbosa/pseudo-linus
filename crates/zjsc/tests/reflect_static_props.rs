//! As treze entradas de `reflectObjectTable` são reificadas no primeiro acesso. Ordens medidas no bun 1.4.2:
//! antes e depois de acessar a lista é a da tabela (`apply,construct,...,setPrototypeOf`, depois o
//! `Symbol(Symbol.toStringTag)`); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da
//! `Structure`: o `@@toStringTag` eager, os nomes já acessados na ordem de acesso e o resto da tabela.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const TABLE: &str = "apply,construct,defineProperty,deleteProperty,get,getOwnPropertyDescriptor,getPrototypeOf,has,isExtensible,ownKeys,preventExtensions,set,setPrototypeOf";

#[test]
fn own_keys_before_access() {
    let program = "Reflect.ownKeys(Reflect).map(String).join(',')";
    assert_eq!(run(program), format!("{TABLE},Symbol(Symbol.toStringTag)"));
}

#[test]
fn own_keys_after_access() {
    let program = "var f = [Reflect.has, Reflect.get]; Reflect.ownKeys(Reflect).map(String).join(',')";
    assert_eq!(run(program), format!("{TABLE},Symbol(Symbol.toStringTag)"));
}

#[test]
fn own_keys_after_delete_with_access() {
    let program = "var f = [Reflect.has, Reflect.get]; var r = delete Reflect.apply; r + '|' + Reflect.ownKeys(Reflect).map(String).join(',')";
    assert_eq!(
        run(program),
        "true|has,get,construct,defineProperty,deleteProperty,getOwnPropertyDescriptor,getPrototypeOf,isExtensible,ownKeys,preventExtensions,set,setPrototypeOf,Symbol(Symbol.toStringTag)"
    );
}
