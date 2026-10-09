//! Mensagens de erro dos métodos que escrevem em array (ou array-like) com `length` não gravável, congelado e no
//! próprio `Array.prototype`, medidas no bun 1.4.2. O `Array.prototype` é `DerivedArrayType` (`isJSArray` falso): o
//! `pop` dele toma o caminho genérico e termina no `JSArray::put` de `length` ("Array length is not writable"), enquanto
//! um `JSArray` comum passa por `setLengthWithArrayStorage` ("Attempted to assign to readonly property.").
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const PROGRAM: &str = r#"
var out = [];
function t(n, f) { try { f(); out.push(n + '=ok'); } catch (e) { out.push(n + '=' + e.constructor.name + ': ' + e.message); } }
var A = Array.prototype;
function ro() { var a = [1, 2, 3]; Object.defineProperty(a, 'length', { writable: false }); return a; }
function fz() { return Object.freeze([1, 2, 3]); }
function al() { var o = { 0: 1, 1: 2, 2: 3, length: 3 }; Object.defineProperty(o, 'length', { writable: false }); return o; }
var kinds = [['ro', ro], ['frozen', fz], ['arraylike', al]];
kinds.forEach(function (k) {
  var n = k[0], m = k[1];
  t(n + ' pop', function () { A.pop.call(m()); });
  t(n + ' push', function () { A.push.call(m(), 9); });
  t(n + ' push0', function () { A.push.call(m()); });
  t(n + ' shift', function () { A.shift.call(m()); });
  t(n + ' unshift', function () { A.unshift.call(m(), 9); });
  t(n + ' splice', function () { A.splice.call(m(), 0, 1); });
  t(n + ' splice-ins', function () { A.splice.call(m(), 0, 0, 7); });
  t(n + ' fill', function () { A.fill.call(m(), 0); });
  t(n + ' length=0 sloppy', function () { m().length = 0; });
  t(n + ' length=0 strict', function () { 'use strict'; m().length = 0; });
  t(n + ' a[len]=1 strict', function () { 'use strict'; var a = m(); a[a.length] = 1; });
  t(n + ' delete strict', function () { 'use strict'; delete m()[0]; });
});
t('proto pop', function () { Object.defineProperty(Array.prototype, 'length', { value: 4, writable: false }); Array.prototype.pop(); });
t('proto push', function () { Array.prototype.push(1); });
t('proto length=0', function () { 'use strict'; Array.prototype.length = 0; });
t('isArray proto', function () { out.push('isArray=' + Array.isArray(Array.prototype) + ',' + Object.prototype.toString.call(Array.prototype)); });
t('big push', function () { var a = []; a.length = 4294967295; a.push(1); });
t('big push2', function () { A.push.call({ length: Math.pow(2, 53) - 1 }, 1); });
t('big unshift', function () { A.unshift.call({ length: Math.pow(2, 53) - 1 }, 1); });
t('big splice', function () { A.splice.call({ length: Math.pow(2, 53) - 1 }, 0, 0, 1); });
out.join('\n')
"#;

const EXPECTED: &str = "ro pop=TypeError: Attempted to assign to readonly property.
ro push=TypeError: Attempted to assign to readonly property.
ro push0=TypeError: Attempted to assign to readonly property.
ro shift=TypeError: Attempted to assign to readonly property.
ro unshift=TypeError: Attempted to assign to readonly property.
ro splice=TypeError: Attempted to assign to readonly property.
ro splice-ins=TypeError: Attempted to assign to readonly property.
ro fill=ok
ro length=0 sloppy=ok
ro length=0 strict=TypeError: Array length is not writable
ro a[len]=1 strict=TypeError: Attempted to assign to readonly property.
ro delete strict=ok
frozen pop=TypeError: Unable to delete property.
frozen push=TypeError: Attempted to assign to readonly property.
frozen push0=TypeError: Attempted to assign to readonly property.
frozen shift=TypeError: Attempted to assign to readonly property.
frozen unshift=TypeError: Attempted to assign to readonly property.
frozen splice=TypeError: Attempted to assign to readonly property.
frozen splice-ins=TypeError: Attempted to assign to readonly property.
frozen fill=TypeError: Attempted to assign to readonly property.
frozen length=0 sloppy=ok
frozen length=0 strict=TypeError: Array length is not writable
frozen a[len]=1 strict=TypeError: Attempted to assign to readonly property.
frozen delete strict=TypeError: Unable to delete property.
arraylike pop=TypeError: Attempted to assign to readonly property.
arraylike push=TypeError: Attempted to assign to readonly property.
arraylike push0=TypeError: Attempted to assign to readonly property.
arraylike shift=TypeError: Attempted to assign to readonly property.
arraylike unshift=TypeError: Attempted to assign to readonly property.
arraylike splice=TypeError: Attempted to assign to readonly property.
arraylike splice-ins=TypeError: Attempted to assign to readonly property.
arraylike fill=ok
arraylike length=0 sloppy=ok
arraylike length=0 strict=TypeError: Attempted to assign to readonly property.
arraylike a[len]=1 strict=ok
arraylike delete strict=ok
proto pop=TypeError: Array length is not writable
proto push=TypeError: Attempted to assign to readonly property.
proto length=0=TypeError: Array length is not writable
isArray=true,[object Array]
isArray proto=ok
big push=RangeError: Length exceeded the maximum array length
big push2=TypeError: push cannot produce an array of length larger than (2 ** 53) - 1
big unshift=TypeError: unshift cannot produce an array of length larger than (2 ** 53) - 1
big splice=TypeError: Splice cannot produce an array of length larger than (2 ** 53) - 1";

#[test]
fn readonly_length_messages_match_bun() {
    assert_eq!(run(PROGRAM), EXPECTED);
}
