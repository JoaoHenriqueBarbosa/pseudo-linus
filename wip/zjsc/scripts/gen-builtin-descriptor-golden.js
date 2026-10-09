// Gera tests/golden/builtin_descriptor_bun.tsv: descritores de propriedades dos builtins do JavaScriptCore, medidos
// no bun 1.4.2 com `vm.runInThisContext` (JSC puro, sem o transpilador do bun). Para cada construtor, protótipo e
// namespace global do JSC (APIs de host ficam de fora), um programa por objeto e por visão devolve uma string compacta
// em `globalThis.R`. Visões:
//   full   : por chave de `Reflect.ownKeys`, tipo (data/accessor), writable/enumerable/configurable, `length`/`name`
//            das funções, getter.name/length e se o setter existe, valores primitivos;
//   meta   : protótipo, `Symbol.toStringTag`, extensibilidade, congelamento, `Object.prototype.toString`, typeof;
//   flags  : contagem de combinações de atributos por tipo;
//   plus   : visões extras só de alguns objetos (construtores: `length`/`name`/`prototype`, descritor de `constructor`).
// Objeto ausente no motor fica registrado como "absent". `Error.appendStackTrace` e `Error.prepareStackTrace` (só no
// bun) saem das visões.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-builtin-descriptor-golden.js > tests/golden/builtin_descriptor_bun.tsv
const vm = require("node:vm");
const { emitRow } = require("./golden-prelude.js");

const PRELUDE = `
var R;
function DS(k) { return typeof k === "symbol" ? "@@" + (k.description === undefined ? "" : k.description) : k; }
function PV(v) {
  var t = typeof v;
  if (t === "function") return "f(" + v.length + "," + JSON.stringify(v.name) + ")";
  if (t === "object") return v === null ? "null" : "object";
  if (t === "string") return "s" + JSON.stringify(v);
  if (t === "symbol") return "sym" + DS(v);
  if (t === "number") return Object.is(v, -0) ? "n-0" : "n" + v;
  if (t === "bigint") return "b" + v;
  return t + ":" + String(v);
}
function FN(f) { return f === undefined ? "-" : "(" + f.length + "," + JSON.stringify(f.name) + ")"; }
function KEYS(o) {
  return Reflect.ownKeys(o).filter(function (k) { return !(typeof k === "string" && (k === "appendStackTrace" || k === "prepareStackTrace")); });
}
function FULL(o) {
  return KEYS(o).map(function (k) {
    var d = Object.getOwnPropertyDescriptor(o, k);
    var a = (d.enumerable ? "e" : "-") + (d.configurable ? "c" : "-");
    if ("value" in d) return DS(k) + "=D" + (d.writable ? "w" : "-") + a + ":" + PV(d.value);
    return DS(k) + "=A" + a + ":g" + FN(d.get) + "s" + FN(d.set);
  }).join("\\n");
}
function FNMETA(o) {
  var out = [];
  KEYS(o).forEach(function (k) {
    var d = Object.getOwnPropertyDescriptor(o, k);
    if ("value" in d) { if (typeof d.value === "function") out.push(DS(k) + ":" + FN(d.value)); }
    else out.push(DS(k) + ":" + FN(d.get) + FN(d.set));
  });
  return out.join("\\n");
}
function PROTO(o) {
  var p = Object.getPrototypeOf(o);
  if (p === null) return "null";
  var tag = p[Symbol.toStringTag];
  var c = Object.getOwnPropertyDescriptor(p, "constructor");
  return typeof p + ":" + (typeof tag === "string" ? tag : "-") + ":" + (c && typeof c.value === "function" ? c.value.name : "-");
}
function META(o) {
  var tag = Object.getOwnPropertyDescriptor(o, Symbol.toStringTag);
  var t = Object.prototype.toString.call(o);
  return [
    "typeof=" + typeof o, "proto=" + PROTO(o),
    "tag=" + (tag ? ("value" in tag ? "D" + (tag.writable ? "w" : "-") + (tag.enumerable ? "e" : "-") + (tag.configurable ? "c" : "-") + ":" + PV(tag.value) : "A") : "none"),
    "toString=" + t, "ext=" + Object.isExtensible(o), "frozen=" + Object.isFrozen(o), "sealed=" + Object.isSealed(o),
    "count=" + KEYS(o).length, "strings=" + Object.getOwnPropertyNames(o).length, "symbols=" + Object.getOwnPropertySymbols(o).length,
    "enumKeys=" + Object.keys(o).length
  ].join("\\n");
}
function FLAGS(o) {
  var m = {};
  KEYS(o).forEach(function (k) {
    var d = Object.getOwnPropertyDescriptor(o, k);
    var kind = "value" in d ? (typeof d.value === "function" ? "fn" : "data") : "acc";
    var key = kind + ":" + ("value" in d ? (d.writable ? "w" : "-") : "") + (d.enumerable ? "e" : "-") + (d.configurable ? "c" : "-");
    m[key] = (m[key] || 0) + 1;
  });
  return Object.keys(m).sort().map(function (k) { return k + "=" + m[k]; }).join("\\n");
}
function PLUS(o) {
  if (typeof o !== "function") return "notfunction";
  var out = ["length=" + JSON.stringify(Object.getOwnPropertyDescriptor(o, "length")), "name=" + JSON.stringify(Object.getOwnPropertyDescriptor(o, "name"))];
  var p = Object.getOwnPropertyDescriptor(o, "prototype");
  out.push("prototype=" + (p ? "D" + (p.writable ? "w" : "-") + (p.enumerable ? "e" : "-") + (p.configurable ? "c" : "-") : "none"));
  if (p && p.value) {
    var c = Object.getOwnPropertyDescriptor(p.value, "constructor");
    out.push("ctor=" + (c ? "D" + (c.writable ? "w" : "-") + (c.enumerable ? "e" : "-") + (c.configurable ? "c" : "-") + ":" + (c.value === o) : "none"));
  }
  out.push("fnproto=" + (Object.getPrototypeOf(o) === Function.prototype));
  out.push("ownNames=" + Object.getOwnPropertyNames(o).slice(0, 3).join(","));
  return out.join("\\n");
}
`;

// Caminho a partir de globalThis; `%Nome%` são intrínsecos sem global.
const specials = {
  "%ArrayIteratorPrototype%": "Object.getPrototypeOf([][Symbol.iterator]())",
  "%MapIteratorPrototype%": "Object.getPrototypeOf(new Map()[Symbol.iterator]())",
  "%SetIteratorPrototype%": "Object.getPrototypeOf(new Set()[Symbol.iterator]())",
  "%StringIteratorPrototype%": "Object.getPrototypeOf(''[Symbol.iterator]())",
  "%RegExpStringIteratorPrototype%": "Object.getPrototypeOf(/a/[Symbol.matchAll](''))",
  "%GeneratorFunction%": "Object.getPrototypeOf(function* () {}).constructor",
  "%GeneratorFunctionPrototype%": "Object.getPrototypeOf(function* () {})",
  "%GeneratorPrototype%": "Object.getPrototypeOf(function* () {}).prototype",
  "%AsyncGeneratorFunction%": "Object.getPrototypeOf(async function* () {}).constructor",
  "%AsyncGeneratorFunctionPrototype%": "Object.getPrototypeOf(async function* () {})",
  "%AsyncGeneratorPrototype%": "Object.getPrototypeOf(async function* () {}).prototype",
  "%AsyncFunction%": "Object.getPrototypeOf(async function () {}).constructor",
  "%AsyncFunctionPrototype%": "Object.getPrototypeOf(async function () {})",
  "%AsyncIteratorPrototype%": "Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype)",
  "%IteratorPrototype%": "Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))",
  "%TypedArray%": "Object.getPrototypeOf(Int8Array)",
  "%TypedArrayPrototype%": "Object.getPrototypeOf(Int8Array.prototype)",
  "%ThrowTypeError%": "Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get",
  "%ArgumentsObject%": "(function () { return arguments; })(1, 2)",
  "%StrictArgumentsObject%": "(function () { 'use strict'; return arguments; })(1, 2)",
};

const paths = [];
const addPath = (...list) => paths.push(...list);
const ctorsWithProto = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Promise", "Map", "Set", "WeakMap", "WeakSet",
  "WeakRef", "FinalizationRegistry", "RegExp", "Date", "Error", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Proxy",
  "Iterator", "ShadowRealm", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "AggregateError",
  "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array",
  "Float64Array", "BigInt64Array", "BigUint64Array", "Float16Array", "SuppressedError", "DisposableStack", "AsyncDisposableStack",
  "Intl.Collator", "Intl.DateTimeFormat", "Intl.DisplayNames", "Intl.DurationFormat", "Intl.ListFormat", "Intl.Locale",
  "Intl.NumberFormat", "Intl.PluralRules", "Intl.RelativeTimeFormat", "Intl.Segmenter",
  "WebAssembly.Module", "WebAssembly.Instance", "WebAssembly.Memory", "WebAssembly.Table", "WebAssembly.Global",
  "WebAssembly.Tag", "WebAssembly.Exception", "WebAssembly.CompileError", "WebAssembly.LinkError", "WebAssembly.RuntimeError",
];
for (const c of ctorsWithProto) addPath(c, c + ".prototype");
addPath("Math", "JSON", "Reflect", "Atomics", "Intl", "WebAssembly", "Temporal", "Iterator.prototype", "Intl.Segmenter.prototype");
addPath(...Object.keys(specials));

const exprOf = p => specials[p] || "globalThis." + p;

// ---- Visões.
const views = ["full", "meta", "flags"];
const programs = [];
for (const p of paths) {
  for (const v of views) {
    // `flags` só para construtores e namespaces: nos protótipos e intrínsecos a visão `full` já diz tudo.
    if (v === "flags" && (/prototype$/.test(p) || p.startsWith("%"))) continue;
    programs.push(`${PRELUDE}try { var o = ${exprOf(p)}; R = o === undefined || o === null ? "absent" : ${v.toUpperCase()}(o); } catch (e) { R = "throw " + e.name; }`);
  }
}
// PLUS só para funções (os `.prototype` e namespaces dariam "notfunction", sem informação).
for (const p of paths) {
  if (/prototype$/.test(p) || ["Math", "JSON", "Reflect", "Atomics", "Intl", "WebAssembly", "Temporal"].includes(p)) continue;
  programs.push(`${PRELUDE}try { var o = ${exprOf(p)}; R = o === undefined || o === null ? "absent" : PLUS(o); } catch (e) { R = "throw " + e.name; }`);
}
// Variações por flag: forma de acesso e herança que muda a leitura dos mesmos descritores.
for (const p of ["Object.prototype", "Function.prototype", "Array.prototype", "String.prototype", "Symbol", "Math", "Reflect", "Promise"]) {
  programs.push(`${PRELUDE}try { var o = ${exprOf(p)}; R = Object.entries(Object.getOwnPropertyDescriptors(o)).map(function (e) { var d = e[1]; return e[0] + ":" + Object.keys(d).join("/"); }).join("\\n"); } catch (e) { R = "throw " + e.name; }`);
  programs.push(`${PRELUDE}try { var o = ${exprOf(p)}; R = Object.getOwnPropertySymbols(o).map(function (s) { var d = Object.getOwnPropertyDescriptor(o, s); return DS(s) + ":" + ("value" in d ? PV(d.value) : "accessor"); }).join("\\n"); } catch (e) { R = "throw " + e.name; }`);
  programs.push(`${PRELUDE}try { var o = ${exprOf(p)}; R = Object.getOwnPropertyNames(o).filter(function (k) { return Object.getOwnPropertyDescriptor(o, k).enumerable; }).join(","); } catch (e) { R = "throw " + e.name; }`);
}

// ---- Execução.
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const source of programs) {
  if (seen.has(source)) continue;
  seen.add(source);
  let result;
  try {
    globalThis.R = undefined;
    vm.runInThisContext(source);
    result = globalThis.R === undefined ? "<undefined>" : String(globalThis.R);
  } catch (e) {
    dropped++;
    process.stderr.write("erro de programa: " + JSON.stringify(source.slice(PRELUDE.length, PRELUDE.length + 120)) + " " + e + "\n");
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
