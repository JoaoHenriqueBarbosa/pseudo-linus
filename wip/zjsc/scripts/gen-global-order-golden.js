// Gera tests/golden/global_order_bun.tsv: a ordem relativa de `Object.keys(globalThis)` e de
// `Object.getOwnPropertyNames(globalThis)` medida no bun 1.4.2, restrita aos nomes que o porte tem (lista
// `PORT_NAMES` abaixo, mantida à mão: nome novo no porte entra aqui e o golden é regerado). Os globais do JSC puro
// e os do bun (atob, btoa, timers, queueMicrotask, reportError, structuredClone, BuildMessage, ResolveMessage,
// DOMException, TextEncoder) ficam misturados exatamente como o bun os instala; quem decide a posição no porte é
// a tabela `ORDER` de `reorder_standard_globals` em src/runtime/js_global_object_init.rs.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). O programa filtra pela mesma lista
// no bun e no porte, então nome ausente num dos dois aparece como divergência.
// Uso: bun scripts/gen-global-order-golden.js > tests/golden/global_order_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const PORT_NAMES = [
  "Infinity", "undefined", "NaN",
  "atob", "btoa", "clearImmediate", "clearInterval", "clearTimeout", "queueMicrotask", "reportError",
  "setImmediate", "setInterval", "setTimeout", "structuredClone",
  "BuildMessage", "ResolveMessage", "DOMException", "TextEncoder",
  "isNaN", "isFinite", "escape", "unescape", "decodeURI", "decodeURIComponent", "encodeURI", "encodeURIComponent",
  "eval", "globalThis", "parseInt", "parseFloat", "ArrayBuffer", "EvalError", "RangeError", "ReferenceError",
  "SyntaxError", "TypeError", "URIError", "AggregateError", "SuppressedError", "Proxy", "Reflect", "JSON", "Math",
  "Atomics", "WebAssembly", "console", "Int8Array", "Int16Array", "Int32Array", "Uint8Array", "Uint8ClampedArray",
  "Uint16Array", "Uint32Array", "Float16Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array",
  "DataView", "Date", "Error", "Boolean", "Map", "Number", "Set", "WeakMap", "WeakSet", "WeakRef",
  "FinalizationRegistry", "Object", "Function", "Array", "RegExp", "Iterator", "SharedArrayBuffer",
  "DisposableStack", "AsyncDisposableStack", "String", "Promise", "BigInt", "Symbol", "Intl", "Temporal",
  "ShadowRealm",
];

const list = JSON.stringify(PORT_NAMES);
const programs = [];
for (const fn of ["Object.keys", "Object.getOwnPropertyNames"]) {
  programs.push(`var L = ${list};\nR = ${fn}(globalThis).filter(function (k) { return L.indexOf(k) >= 0 }).join()`);
}

for (const source of programs) {
  (0, eval)("var R");
  (0, eval)(source);
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(String(globalThis.R)));
}
