// node:util/types contra o bun 1.4.2: superfície (nomes, length, descritores) e cada predicado sobre uma bateria de
// valores; cada linha é o predicado e uma string de 0/1, um caractere por valor da bateria.
const types = require("util/types");
console.log(types === require("node:util/types"), Object.keys(types).length, Object.keys(types).join(","));
const first = Object.getOwnPropertyDescriptor(types, "isDate");
console.log(typeof first.value, first.writable, first.enumerable, first.configurable, types.isDate.name, types.isDate.length);
console.log(Object.keys(types).map(k => `${types[k].name}/${types[k].length}`).join(","));
console.log(Object.getPrototypeOf(types) === Object.prototype, Object.isExtensible(types));

function args() { return arguments; }
function sloppyArgs(a) { a = 1; return arguments; }
const strictArgs = (function () { "use strict"; return arguments; })();
async function af() {}
async function* agf() {}
function* gf() {}
class Klass {}
const bound = af.bind(null);
const sab = new SharedArrayBuffer(4);
const ns = { [Symbol.toStringTag]: "Error" };
const fakeAsync = Object.setPrototypeOf(function () {}, Object.getPrototypeOf(af));
const values = [
  ["undefined", undefined], ["null", null], ["number", 1], ["string", "s"], ["boolean", true], ["symbol", Symbol()], ["bigint", 1n],
  ["object", {}], ["array", []], ["date", new Date()], ["Date.prototype", Date.prototype],
  ["args", args(1)], ["sloppyArgs", sloppyArgs(1)], ["strictArgs", strictArgs],
  ["Object(1n)", Object(1n)], ["Object(true)", Object(true)], ["Object(1)", Object(1)], ["Object('s')", Object("s")],
  ["Object(Symbol())", Object(Symbol())], ["class String", new (class extends String {})("x")],
  ["Error", new Error("e")], ["TypeError", new TypeError("e")], ["class Error", new (class extends Error {})()],
  ["errorProtoObj", Object.create(Error.prototype)], ["tagError", ns], ["DOMException", new DOMException("x")],
  ["regexp", /x/], ["RegExp.prototype", RegExp.prototype],
  ["af", af], ["agf", agf], ["gf", gf], ["bound af", bound], ["fakeAsync", fakeAsync], ["arrow", () => {}], ["class", Klass], ["native", Math.max],
  ["generator obj", gf()], ["async gen obj", agf()],
  ["promise", Promise.resolve()], ["thenable", { then() {} }],
  ["map", new Map()], ["set", new Set()], ["map iter", new Map().keys()], ["set iter", new Set().values()], ["map entries", new Map().entries()],
  ["array iter", [].values()], ["weakmap", new WeakMap()], ["weakset", new WeakSet()],
  ["arraybuffer", new ArrayBuffer(4)], ["sab", sab], ["dataview", new DataView(new ArrayBuffer(4))],
  ["proxy", new Proxy({}, {})], ["proxy fn", new Proxy(function () {}, {})], ["globalThis", globalThis],
  ["u8", new Uint8Array(1)], ["u8c", new Uint8ClampedArray(1)], ["u16", new Uint16Array(1)], ["u32", new Uint32Array(1)],
  ["i8", new Int8Array(1)], ["i16", new Int16Array(1)], ["i32", new Int32Array(1)], ["f16", new Float16Array(1)],
  ["f32", new Float32Array(1)], ["f64", new Float64Array(1)], ["bi64", new BigInt64Array(1)], ["bu64", new BigUint64Array(1)],
  ["buffer", Buffer.alloc(1)], ["u8 on sab", new Uint8Array(sab)],
  ["EventTarget", new EventTarget()], ["AbortSignal", new AbortController().signal], ["Event", new Event("x")],
  ["CryptoKey proto", Object.create(CryptoKey.prototype)],
];
console.log(values.map(([label]) => label).join(","));
for (const key of Object.keys(types)) {
  console.log(key.padEnd(24), values.map(([, v]) => (types[key](v) ? "1" : "0")).join(""));
}
console.log(types.isDate(), types.isProxy(), types.isNativeError(), types.isAsyncFunction());
console.log(types.isDate(new Date(), 1), types.isDate.call(5, new Date()));
let trapped = 0;
const watched = new Proxy({}, { getPrototypeOf() { trapped++; return Error.prototype; }, get() { trapped++; } });
console.log(types.isNativeError(watched), trapped);
(async () => {
  const cryptoKey = await crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  console.log(types.isCryptoKey(cryptoKey), types.isKeyObject(cryptoKey));
  // O `isKeyObject` de um `KeyObject` do `node:crypto` entra quando o porte instalar o `node:crypto`.
  // O namespace de um `import()` no programa CommonJS.
  const namespace = await import("node:path");
  console.log(types.isModuleNamespaceObject(namespace), types.isModuleNamespaceObject(namespace.default));
})();
