//! Identidade dos getters `get [Symbol.species]` entre construtores, medida no bun 1.4.2: cada construtor tem o seu
//! `GetterSetter` (`Object.getOwnPropertyDescriptor(Array, Symbol.species).get !== ...(Map, ...).get`), então a
//! matriz de igualdade é a identidade. O `Uint8Array` não tem `@@species` próprio (herda de `%TypedArray%`), e o
//! `undefined === undefined` dos dois é a única igualdade fora da diagonal.
mod common;

const PROGRAM: &str = r#"
var C = { Array: Array, Map: Map, Set: Set, Promise: Promise, RegExp: RegExp, ArrayBuffer: ArrayBuffer,
  SharedArrayBuffer: SharedArrayBuffer, Int8Array: Object.getPrototypeOf(Int8Array), Uint8Array: Uint8Array,
  Float64Array: Float64Array };
var g = {};
for (var k in C) { var d = Object.getOwnPropertyDescriptor(C[k], Symbol.species); g[k] = d && d.get; }
var ks = Object.keys(g), out = [];
for (var i = 0; i < ks.length; i++) out.push(ks[i] + ":" + ks.map(function (b) { return g[ks[i]] === g[b] ? 1 : 0; }).join(""));
out.push(g.Array.name + "," + g.Array.length);
var R = out.join("\n");
"#;

const EXPECTED: &str = "Array:1000000000\nMap:0100000000\nSet:0010000000\nPromise:0001000000\nRegExp:0000100000\n\
ArrayBuffer:0000010000\nSharedArrayBuffer:0000001000\nInt8Array:0000000100\nUint8Array:0000000011\n\
Float64Array:0000000011\nget [Symbol.species],0";

#[test]
fn species_getter_identity_matches_bun() {
    let actual = common::guarded(|| common::EvalMode::IndirectEval.evaluate(PROGRAM, "species_getter_identity.js", "R"));
    assert_eq!(actual, Ok(EXPECTED.to_string()));
}
