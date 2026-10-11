//! `RegExp.prototype[Symbol.replace]` com um resultado de `exec` de `length` 2^32: o `captures.tryReserveCapacity` do
//! JSC recusa o buffer de mais de `UINT_MAX` bytes e lança `RangeError: Out of memory` antes de ler qualquer captura,
//! medido no bun 1.4.2. Sem o limite do `WTF::Vector`, o porte reservava 32 GiB virtuais e lia 2^32 capturas.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

#[test]
fn replace_with_huge_result_length_throws_out_of_memory() {
    let program = "var o = { exec: function () { return { length: 2 ** 32, 0: 'a', index: 0 } }, flags: '', lastIndex: 0 }; \
                   ['x', function () { return 'R' }].map(function (r) { try { RegExp.prototype[Symbol.replace].call(o, 'aXbX', r); return 'ok' } \
                   catch (e) { return e.name + ': ' + e.message } }).join('|')";
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    let text = String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned();
    assert_eq!(text, "RangeError: Out of memory|RangeError: Out of memory");
}
