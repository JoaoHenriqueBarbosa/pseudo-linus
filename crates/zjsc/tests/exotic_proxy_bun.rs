//! Regressão de objetos exóticos pelo caminho do `Proxy` e do protótipo, com os valores medidos no bun 1.4.2 (os mesmos
//! programas rodados no `bun` e aqui): o typed array não segue para o protótipo com índice numérico canônico fora do alcance,
//! o alvo de um `Proxy` de typed array aceita `Reflect.set` fora do alcance sem lançar, e o `delete` pelo `Proxy` de uma
//! função apaga `length` e `name` preguiçosos.
use zjsc::api::eval::{describe_exception, evaluate_script};
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(source: &str) -> String {
    match evaluate_script(source) {
        Ok(value) if value.is_string() => String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned(),
        Ok(_) => "<não string>".to_string(),
        Err(exception) => format!("exceção: {}", describe_exception(&exception)),
    }
}

#[test]
fn typed_array_out_of_range_index_does_not_reach_the_prototype() {
    let source = r#"var o = new Uint8Array(3); Object.setPrototypeOf(o, {p:1, 4:1, "1.5":2, "-0":3}); var a=[]; for (var k in o) a.push(k); [a.join(','), 4 in o, Reflect.has(o,4), o[4], "1.5" in o, o["1.5"], "-0" in o, o["-0"], "p" in o].join('|')"#;
    assert_eq!(run(source), "0,1,2,p|false|false||false||false||true");
}

#[test]
fn proxy_of_typed_array_accepts_out_of_range_set() {
    let source = "var q = new Proxy(new Uint8Array(2), {}); Object.assign(q, {8:1, q:2, 1:3}); [Object.keys(q).join(','), q.q, Reflect.set(q, 8, 1)].join('|')";
    assert_eq!(run(source), "0,1,q|2|true");
}

#[test]
fn proxy_of_function_deletes_lazy_length_and_name() {
    let source = "var o = new Proxy(function pf(a) {}, {}); var r = [delete o.length, delete o.name]; r.push(Reflect.ownKeys(o).join(',')); r.join('|')";
    assert_eq!(run(source), "true|true|prototype");
    let source = r#"var o = new Proxy(function pf(a) {}, {}); delete o["length"]; o.length = 1; Reflect.ownKeys(o).join(',') + '|' + o.length"#;
    assert_eq!(run(source), "name,prototype|0");
}
