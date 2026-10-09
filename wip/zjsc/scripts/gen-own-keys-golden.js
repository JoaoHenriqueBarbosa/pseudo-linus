// Gera tests/golden/own_keys_bun.json: as chaves próprias de cada construtor, protótipo e global solto
// na ORDEM em que o bun 1.4.2 (o oráculo) as instala, sem ordenar. Uso:
//   bun scripts/gen-own-keys-golden.js > tests/golden/own_keys_bun.json
//
// Formato: um objeto com uma linha por caminho (`Array.prototype`, `globalThis`...). Cada caminho tem
//   "keys":  `Reflect.ownKeys` em ordem; string vira ela mesma, Symbol vira "@@sym:" + descrição
//            (`@@sym:Symbol.iterator`);
//   "props": por chave, os atributos: `w`/`e`/`c` (writable, enumerable, configurable; acessor traz
//            `get`/`set` como booleano em vez de `w`) e, se o valor é função, `length` e `name`;
//   "only":  (só em `globalThis`) as chaves medidas são filtradas por esta lista, porque o bun acrescenta
//            globais próprios (`Bun`, `process`, `fetch`...) que não são do JavaScriptCore.
// `Error.appendStackTrace`/`prepareStackTrace` e afins do bun ficam de fora pelo teste, não aqui.

const ES_GLOBAL_VALUES = ["globalThis", "NaN", "Infinity", "undefined"];
const ES_GLOBAL_FUNCTIONS = ["eval", "parseInt", "parseFloat", "isNaN", "isFinite", "decodeURI",
  "decodeURIComponent", "encodeURI", "encodeURIComponent", "escape", "unescape"];
const ES_CONSTRUCTORS = ["Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt",
  "Date", "RegExp", "Error", "AggregateError", "EvalError", "RangeError", "ReferenceError", "SyntaxError",
  "TypeError", "URIError", "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "Promise",
  "Proxy", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray",
  "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float16Array", "Float32Array", "Float64Array",
  "BigInt64Array", "BigUint64Array", "Iterator"];
const ES_NAMESPACES = ["Math", "JSON", "Reflect", "Atomics"];

const root = globalThis;
const esGlobals = [...ES_GLOBAL_VALUES, ...ES_GLOBAL_FUNCTIONS, ...ES_CONSTRUCTORS, ...ES_NAMESPACES]
  .filter((name) => Object.prototype.hasOwnProperty.call(root, name));

// Os 34 caminhos da medição antiga primeiro, na mesma ordem, depois o resto.
const paths = ["globalThis"];
const seen = new Set(paths);
function add(path) { if (!seen.has(path)) { seen.add(path); paths.push(path); } }
const old = require("../wip-notes/bun-builtin-props.json");
for (const path of Object.keys(old)) add(path);
for (const name of [...ES_CONSTRUCTORS, ...ES_NAMESPACES]) {
  if (!esGlobals.includes(name)) continue;
  add(name);
  if (typeof root[name] === "function" && root[name].prototype && typeof root[name].prototype === "object") add(name + ".prototype");
}

// Objetos sem nome global: o primeiro segmento do caminho pode ser uma destas chaves (a mesma tabela vive
// em `ORDERED_PROGRAM` de tests/builtin_own_keys_golden.rs); `@proto` num segmento é `Object.getPrototypeOf`.
const SPECIALS = {
  "%ArrayIteratorPrototype%": () => Object.getPrototypeOf([][Symbol.iterator]()),
  "%MapIteratorPrototype%": () => Object.getPrototypeOf(new Map()[Symbol.iterator]()),
  "%SetIteratorPrototype%": () => Object.getPrototypeOf(new Set()[Symbol.iterator]()),
  "%StringIteratorPrototype%": () => Object.getPrototypeOf(""[Symbol.iterator]()),
  "%RegExpStringIteratorPrototype%": () => Object.getPrototypeOf(/a/[Symbol.matchAll]("")),
  "%GeneratorFunction%": () => Object.getPrototypeOf(function* () {}).constructor,
  "%GeneratorFunctionPrototype%": () => Object.getPrototypeOf(function* () {}),
  "%GeneratorPrototype%": () => Object.getPrototypeOf(function* () {}).prototype,
  "%AsyncGeneratorFunction%": () => Object.getPrototypeOf(async function* () {}).constructor,
  "%AsyncGeneratorFunctionPrototype%": () => Object.getPrototypeOf(async function* () {}),
  "%AsyncGeneratorPrototype%": () => Object.getPrototypeOf(async function* () {}).prototype,
  "%AsyncFunction%": () => Object.getPrototypeOf(async function () {}).constructor,
  "%AsyncFunctionPrototype%": () => Object.getPrototypeOf(async function () {}),
  "%AsyncIteratorPrototype%": () => Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype),
  "%TypedArray%": () => Object.getPrototypeOf(Int8Array),
  "%TypedArrayPrototype%": () => Object.getPrototypeOf(Int8Array.prototype),
};
for (const name of Object.keys(SPECIALS)) add(name);
add("Function.prototype");
add("Symbol.prototype");
for (const ns of ["Intl", "Temporal", "WebAssembly"]) {
  if (!Object.prototype.hasOwnProperty.call(root, ns)) continue;
  add(ns);
  for (const member of Object.getOwnPropertyNames(root[ns])) {
    if (member === "Now") add(ns + ".Now");
    if (typeof root[ns][member] === "function") {
      add(ns + "." + member);
      const proto = root[ns][member].prototype;
      if (proto && typeof proto === "object") add(ns + "." + member + ".prototype");
    }
  }
}
for (const name of ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array",
  "Uint32Array", "Float16Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"]) {
  if (Object.prototype.hasOwnProperty.call(root, name)) { add(name); add(name + ".prototype"); }
}

function resolve(path) {
  let target = root;
  const parts = path.split(".");
  for (let i = 0; i < parts.length; i++) {
    if (target == null) return undefined;
    target = i === 0 && parts[i] in SPECIALS ? SPECIALS[parts[i]]() : target[parts[i]];
  }
  return target;
}

function encodeKey(key) {
  if (typeof key === "symbol") return "@@sym:" + (key.description ?? "");
  return key;
}

function describeProps(target, keys) {
  const props = {};
  for (const key of keys) {
    const d = Object.getOwnPropertyDescriptor(target, key);
    const entry = {};
    if ("value" in d) {
      entry.w = d.writable;
      if (typeof d.value === "function") {
        entry.length = d.value.length;
        entry.name = d.value.name;
      }
    } else {
      entry.get = d.get !== undefined;
      entry.set = d.set !== undefined;
    }
    entry.e = d.enumerable;
    entry.c = d.configurable;
    // defineProperty: `props["__proto__"] = ...` trocaria o protótipo em vez de criar a chave.
    Object.defineProperty(props, encodeKey(key), { value: entry, enumerable: true, writable: true, configurable: true });
  }
  return props;
}

const lines = [];
for (const path of paths) {
  const target = resolve(path);
  if (target == null || (typeof target !== "object" && typeof target !== "function")) continue;
  let keys = Reflect.ownKeys(target);
  const record = {};
  if (path === "globalThis") {
    record.only = esGlobals;
    keys = keys.filter((key) => typeof key === "string" && esGlobals.includes(key));
  }
  record.keys = keys.map(encodeKey);
  record.props = describeProps(target, keys);
  lines.push(JSON.stringify(path) + ":" + JSON.stringify(record));
}
console.log("{\n" + lines.join(",\n") + "\n}");
