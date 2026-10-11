//! `TypedArray.prototype.sort` e `toSorted` conferem o `this` antes do comparador, como o despacho por tipo do
//! `typedArrayViewProtoFuncSort` do JSC; mensagens medidas no bun 1.4.2.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

#[test]
fn receiver_is_checked_before_the_comparator() {
    let program = "var P = Object.getPrototypeOf(Int8Array.prototype); \
                   ['sort', 'toSorted'].map(function (m) { return [[undefined, 1], [1, 1], [{}, 1], [new Int8Array(2), 1]] \
                   .map(function (a) { try { P[m].call(a[0], a[1]); return 'ok' } catch (e) { return e.message } }).join(';') }).join('|')";
    let expected = ["sort", "toSorted"]
        .map(|m| {
            format!(
                "Receiver should be a typed array view but was not an object;\
                 Receiver should be a typed array view but was not an object;\
                 Receiver should be a typed array view;\
                 TypedArray.prototype.{m} requires the comparator argument to be a function or undefined"
            )
        })
        .join("|");
    assert_eq!(run(program), expected);
}
