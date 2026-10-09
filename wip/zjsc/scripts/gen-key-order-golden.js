// Gera tests/golden/key_order_bun.tsv: a ordem de Reflect.ownKeys e o descritor de cada chave dos globais do
// ECMAScript e do JSC e dos `.prototype` deles, medidos no bun 1.4.2. Uma linha por objeto, com duas colunas:
// a expressão do objeto (já com o filtro, no caso de globalThis) e o resultado de `ser` (tests/golden/key_order_serializer.js).
// Uso: bun scripts/gen-key-order-golden.js > tests/golden/key_order_bun.tsv
const fs = require("fs");
const path = require("path");

const serializerSource = fs.readFileSync(path.join(__dirname, "../tests/golden/key_order_serializer.js"), "utf8");
(0, eval)(serializerSource);

const ecmaGlobals = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Math", "JSON", "Reflect",
  "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "Promise", "RegExp", "Date",
  "Error", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "AggregateError",
  "SuppressedError", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray",
  "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float16Array", "Float32Array", "Float64Array",
  "BigInt64Array", "BigUint64Array", "Iterator", "Intl", "Temporal", "WebAssembly", "Atomics", "Proxy",
  "DisposableStack", "AsyncDisposableStack",
];
const globalValueNames = [
  "globalThis", "Infinity", "NaN", "undefined", "eval", "isFinite", "isNaN", "parseFloat", "parseInt",
  "decodeURI", "decodeURIComponent", "encodeURI", "encodeURIComponent", "escape", "unescape",
  ...ecmaGlobals,
];

const targets = []; // [expressão, filtro ou undefined]
const present = (name) => name in globalThis;
for (const name of ecmaGlobals) {
  if (!present(name)) continue;
  const value = globalThis[name];
  targets.push([name]);
  if (typeof value === "function" && value.prototype && typeof value.prototype === "object") targets.push([name + ".prototype"]);
}
// Subclasses de Intl e classes de Temporal (todo membro função do namespace), mais os namespaces aninhados.
for (const ns of ["Intl", "Temporal"]) {
  for (const key of Object.getOwnPropertyNames(globalThis[ns])) {
    const value = globalThis[ns][key];
    if (typeof value === "function" && value.prototype && typeof value.prototype === "object") {
      targets.push([ns + "." + key]);
      targets.push([ns + "." + key + ".prototype"]);
    } else if (value && typeof value === "object") {
      targets.push([ns + "." + key]);
    }
  }
}
targets.push(["Object.getPrototypeOf(Int8Array)"], ["Object.getPrototypeOf(Int8Array).prototype"]);
targets.push(["Object.getPrototypeOf(function* () {})"], ["Object.getPrototypeOf(async function () {})"]);
targets.push(["Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))"]);
targets.push(["Object.getPrototypeOf(Object.getPrototypeOf(function* () {}.prototype))"]);
targets.push(["globalThis", globalValueNames]);

const seen = new Set();
for (const [expr, filter] of targets) {
  const full = filter ? `ser(${expr}, ${JSON.stringify(filter)})` : `ser(${expr})`;
  if (seen.has(full)) continue;
  seen.add(full);
  let result;
  try {
    result = (0, eval)(full);
  } catch (error) {
    result = "throws " + error.name;
  }
  process.stdout.write(JSON.stringify(full) + "\t" + JSON.stringify(result) + "\n");
}
