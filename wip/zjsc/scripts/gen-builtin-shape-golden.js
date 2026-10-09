// Gera tests/golden/builtin_shape_bun.tsv: a forma dos built-ins globais de JS puro, medida no bun 1.4.2.
// Para cada construtor, protótipo e namespace: os nomes próprios na ordem da engine, os símbolos, o protótipo, o
// Symbol.toStringTag; para cada propriedade: o descritor (writable/enumerable/configurable, valor ou acessor), name e
// length da função (e do getter/setter), se é construtor, e a mensagem exata ao chamar com receptor errado.
// Um programa por built-in x propriedade, cada um num bun filho novo (o JSC reifica as tabelas estáticas por ordem de
// acesso). Fora do escopo: Bun, process, Buffer, fetch e demais APIs de host.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-builtin-shape-golden.js > tests/golden/builtin_shape_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

// O filho é sempre este arquivo, neste diretório: o `stack` e o `sourceURL` de um Error mostram o caminho do script que
// chamou o eval, e o caminho tem de ser público e determinístico (`file:///tmp/zjsc-shape/builtin_shape_case.js` e as
// frames do hospedeiro), igual ao que o porte produz para esse arquivo.
const CHILD_DIR = "/tmp/zjsc-shape";
const CHILD_FILE = path.join(CHILD_DIR, "builtin_shape_case.js");
const CHILD_TEXT = 'const fs = require("fs");\n(0, eval)(fs.readFileSync(0, "utf8"));\nprocess.exit(0);\n';
fs.mkdirSync(CHILD_DIR, { recursive: true });
fs.writeFileSync(CHILD_FILE, CHILD_TEXT);

const PRELUDE =
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="function")return "fn";if(v===null)return "null";if(t==="object")return "obj";return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v)}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function F(o,k){var d=Object.getOwnPropertyDescriptor(o,k);return d&&(d.value||d.get)}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function Q(f,r){return T(()=>Reflect.apply(f,r,[]))}\n';

// Alvos: [expressão, rótulo]. Os que não existem no bun ou lançam são descartados pelo próprio gerador.
const targets = [];
const ctorNames = ["Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Date", "RegExp",
  "Error", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "AggregateError",
  "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "Promise", "ArrayBuffer", "SharedArrayBuffer",
  "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array",
  "Float16Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "Proxy", "Iterator"];
for (const n of ctorNames) {
  targets.push([n, n]);
  targets.push([n + ".prototype", n + ".prototype"]);
}
for (const n of ["Reflect", "JSON", "Math", "Atomics", "WebAssembly"]) targets.push([n, n]);
const hidden = {
  "TypedArray": "Object.getPrototypeOf(Int8Array)",
  "TypedArray.prototype": "Object.getPrototypeOf(Int8Array.prototype)",
  "ArrayIteratorPrototype": "Object.getPrototypeOf([][Symbol.iterator]())",
  "StringIteratorPrototype": "Object.getPrototypeOf(''[Symbol.iterator]())",
  "MapIteratorPrototype": "Object.getPrototypeOf(new Map()[Symbol.iterator]())",
  "SetIteratorPrototype": "Object.getPrototypeOf(new Set()[Symbol.iterator]())",
  "RegExpStringIteratorPrototype": "Object.getPrototypeOf(/a/[Symbol.matchAll](''))",
  "IteratorHelperPrototype": "Object.getPrototypeOf([].values().map(x=>x))",
  "WrapForValidIteratorPrototype": "Object.getPrototypeOf(Iterator.from({next(){}}))",
  "GeneratorFunction": "Object.getPrototypeOf(function*(){}).constructor",
  "GeneratorFunction.prototype": "Object.getPrototypeOf(function*(){})",
  "Generator.prototype": "Object.getPrototypeOf(function*(){}).prototype",
  "AsyncFunction": "Object.getPrototypeOf(async function(){}).constructor",
  "AsyncFunction.prototype": "Object.getPrototypeOf(async function(){})",
  "AsyncGeneratorFunction": "Object.getPrototypeOf(async function*(){}).constructor",
  "AsyncGeneratorFunction.prototype": "Object.getPrototypeOf(async function*(){})",
  "AsyncGenerator.prototype": "Object.getPrototypeOf(async function*(){}).prototype",
  "AsyncIteratorPrototype": "Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype.__proto__)",
  "Function.prototype.bound": "(function(){}).bind()",
  "ThrowTypeError": "Object.getOwnPropertyDescriptor(Function.prototype,'caller').get",
  "ArrayLiteral": "[]",
  "ObjectLiteral": "{}",
  "ArgumentsObject": "(function(){return arguments})()",
  "ErrorInstance": "new Error('x')",
  "RegExpInstance": "/a/g",
  "PromiseInstance": "Promise.resolve(1)",
  "MapInstance": "new Map()",
  "DateInstance": "new Date(0)",
  "FunctionInstance": "function f(a,b){}",
  "ArrowInstance": "(a)=>a",
  "ClassInstance": "class C{static m(){}}",
  "StringObject": "new String('ab')",
  "ArrayBufferInstance": "new ArrayBuffer(4)",
  "Uint8ArrayInstance": "new Uint8Array(2)",
};
for (const [label, expr] of Object.entries(hidden)) targets.push([expr, label]);
// Funções e valores globais, como propriedades do objeto global.
const globalKeys = ["globalThis", "NaN", "Infinity", "undefined", "parseInt", "parseFloat", "isNaN", "isFinite",
  "encodeURI", "encodeURIComponent", "decodeURI", "decodeURIComponent", "escape", "unescape", "eval"];

const wellKnown = ["iterator", "asyncIterator", "hasInstance", "isConcatSpreadable", "match", "matchAll", "replace",
  "search", "species", "split", "toPrimitive", "toStringTag", "unscopables", "dispose", "asyncDispose"];

function keyExpr(key) {
  if (typeof key === "string") return JSON.stringify(key);
  const m = /^Symbol\.(\w+)$/.exec(key.description || "");
  return m && wellKnown.includes(m[1]) && Symbol[m[1]] === key ? "Symbol." + m[1] : null;
}

const exprs = [];
const add = (e) => exprs.push(e);
const receivers = ["undefined", "{}", "1"];

function describeKey(E, label, key, obj) {
  const k = keyExpr(key);
  if (k === null) return;
  const d = Object.getOwnPropertyDescriptor(obj, key);
  add(`T(()=>D(${E},${k}))`);
  const hasValueFn = typeof d.value === "function";
  const fns = [];
  if (hasValueFn) fns.push(["F(" + E + "," + k + ")", "value"]);
  if (d.get) fns.push(["Object.getOwnPropertyDescriptor(" + E + "," + k + ").get", "get"]);
  if (d.set) fns.push(["Object.getOwnPropertyDescriptor(" + E + "," + k + ").set", "set"]);
  for (const [fe, kind] of fns) {
    add(`T(()=>S((${fe}).name)+" "+S((${fe}).length))`);
    add(`T(()=>D(${fe},"name")+" "+D(${fe},"length"))`);
    add(`T(()=>Object.getOwnPropertyNames(${fe}).join()+" "+(Object.getPrototypeOf(${fe})===Function.prototype))`);
    add(`T(()=>{try{Reflect.construct(function(){},[],${fe});return "ctor"}catch(e){return "noctor"}})`);
    if (kind !== "set") for (const r of receivers) add(`Q(${fe},${r})`);
    else add(`T(()=>Reflect.apply(${fe},undefined,[1]))`);
  }
  if (hasValueFn && typeof d.value.prototype === "object") add(`T(()=>D(F(${E},${k}),"prototype"))`);
}

function describeObject(E, label, obj) {
  add(`T(()=>Object.getOwnPropertyNames(${E}).join())`);
  add(`T(()=>Object.getOwnPropertySymbols(${E}).map(String).join())`);
  add(`T(()=>Reflect.ownKeys(${E}).map(String).join())`);
  add(`T(()=>S(Object.getOwnPropertyNames(${E}).length))`);
  add(`T(()=>D(${E},Symbol.toStringTag))`);
  add(`T(()=>Object.prototype.toString.call(${E}))`);
  add(`T(()=>{var o=${E},r=[];while(o=Object.getPrototypeOf(o))r.push(Object.prototype.hasOwnProperty.call(o,"constructor")&&typeof o.constructor==="function"?o.constructor.name:Object.prototype.toString.call(o));return r.join()})`);
  add(`T(()=>Object.isExtensible(${E})+" "+Object.isFrozen(${E})+" "+Object.isSealed(${E})+" "+typeof ${E})`);
  add(`T(()=>S(Object.getPrototypeOf(${E})===null)+" "+S(Object.getPrototypeOf(${E})===Object.prototype)+" "+S(Object.getPrototypeOf(${E})===Function.prototype)`+`)`);
  if (typeof obj === "function") {
    add(`T(()=>S((${E}).name)+" "+S((${E}).length))`);
    add(`T(()=>D(${E},"name")+" "+D(${E},"length")+" "+D(${E},"prototype"))`);
    add(`T(()=>String(Function.prototype.toString.call(${E})).replace(/\\s+/g," "))`);
    add(`T(()=>{try{${E}.call(undefined);return "ok"}catch(e){return e.name+": "+e.message}})`);
    add(`T(()=>{try{new (${E})();return "ok"}catch(e){return e.name+": "+e.message}})`);
  }
  for (const key of Reflect.ownKeys(obj)) describeKey(E, label, key, obj);
}

for (const [expr] of targets) {
  let obj;
  try { obj = (0, eval)(expr); } catch (e) { continue; }
  if (obj === null || (typeof obj !== "object" && typeof obj !== "function")) continue;
  describeObject(expr, expr, obj);
}
// Objeto global: só as chaves de JS puro, uma a uma.
for (const key of globalKeys) {
  if (!(key in globalThis)) continue;
  describeKey("globalThis", "globalThis", key, globalThis);
  add(`T(()=>"typeof "+typeof globalThis[${JSON.stringify(key)}])`);
}
add(`T(()=>Object.getOwnPropertyDescriptor(globalThis,Symbol.toStringTag)===undefined)`);

// ---- Execução.
const baseSources = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "builtin_shape_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.add(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}

const seen = new Set();
const programs = [];
let dup = 0;
for (const expr of exprs) {
  // Chamar `Date.now`, `Date()` (o `Date.prototype.constructor` sem `new`) e `Math.random` devolve o relógio ou o acaso
  // da hora da geração: o valor nunca se repete numa corrida do teste, então não há resultado a comparar.
  if (/^Q\(F\((Date,"now"|Date\.prototype,"constructor"|Math,"random")\),/.test(expr)) continue;
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source)) continue;
  seen.add(source);
  if (baseSources.has(source)) { dup++; continue; }
  programs.push({ expr, source });
}

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, CHILD_FILE], { cwd: CHILD_DIR, stdio: ["pipe", "pipe", "pipe"], timeout: 5000, killSignal: "SIGKILL", env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      const result = decodeResult(out);
      resolve({ code: code === 0 && result === null ? -1 : code, out: result === null ? "" : result, err });
    });
    child.on("error", (e) => resolve({ code: -1, out: "", err: String(e) }));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const MAX_PARALLEL = 8;
  const results = new Array(programs.length);
  let next = 0;
  async function worker() {
    while (next < programs.length) {
      const i = next++;
      results[i] = await runChild(programs[i].source);
    }
  }
  await Promise.all(Array.from({ length: MAX_PARALLEL }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  // Programa que nem compila (SyntaxError do eval): o teste não aceita exceção, então o golden grava o programa dentro
  // de um `eval` direto sob `T`, que devolve "throw SyntaxError: <mensagem>", como os demais casos que lançam.
  for (let i = 0; i < programs.length; i++) {
    if (results[i].code === 0 || !/SyntaxError/.test(results[i].err)) continue;
    const wrapped = '"use strict";\n' + PRELUDE + `globalThis.R = T(()=>eval(${JSON.stringify(programs[i].expr)}))`;
    const r = await runChild(wrapped);
    if (r.code === 0) {
      programs[i].source = wrapped;
      results[i] = r;
    }
  }
  programs.forEach(({ expr, source }, i) => {
    const r = results[i];
    if (r.code !== 0) {
      dropped++;
      process.stderr.write("filho falhou: " + JSON.stringify(expr).slice(0, 160) + " " + r.err.slice(0, 80) + "\n");
      return;
    }
    // Só o caminho fixo e público do filho pode aparecer; qualquer outro caminho ou a marca do runtime é vazamento.
    if (/\/home\/|\/Users\/|bun/i.test(r.out) || /\/tmp\/(?!zjsc-shape\/builtin_shape_case\.js)/.test(r.out)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(JSON.stringify(source) + "\t" + JSON.stringify(r.out));
  });
  process.stdout.write(emitFactoredLines("builtin_shape", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
