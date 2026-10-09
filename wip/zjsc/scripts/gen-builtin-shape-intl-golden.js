// Gera tests/golden/builtin_shape_intl_bun.tsv: a forma de Intl.* e Temporal.*, medida no bun 1.4.2.
// Complementa gen-builtin-shape-golden.js (que deixou os dois namespaces de fora). Para cada construtor, protótipo e
// namespace: os nomes próprios na ordem da engine, os símbolos, o protótipo, o Symbol.toStringTag; para cada
// propriedade: o descritor, name e length da função (e do getter/setter), se é construtor, e a mensagem exata ao
// chamar com receptor errado. Também: construtor chamado sem new. Um programa por built-in x propriedade, cada um
// num bun filho novo (o JSC reifica as tabelas estáticas por ordem de acesso). Fora do escopo: APIs de host.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-builtin-shape-golden.js.
// Uso: bun scripts/gen-builtin-shape-intl-golden.js > tests/golden/builtin_shape_intl_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const SELF_TSV = "builtin_shape_intl_bun.tsv";

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="function")return "fn";if(v===null)return "null";if(t==="object")return "obj";return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v)}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function F(o,k){var d=Object.getOwnPropertyDescriptor(o,k);return d&&(d.value||d.get)}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function Q(f,r){return T(()=>Reflect.apply(f,r,[]))}\n';

// Alvos: [expressão, rótulo]. Os que não existem no bun ou lançam são descartados pelo próprio gerador.
const targets = [];
const intlCtors = ["Collator", "DateTimeFormat", "DisplayNames", "DurationFormat", "ListFormat", "Locale", "NumberFormat",
  "PluralRules", "RelativeTimeFormat", "Segmenter"];
const temporalCtors = ["Duration", "Instant", "PlainDate", "PlainDateTime", "PlainTime", "PlainMonthDay",
  "PlainYearMonth", "ZonedDateTime"];
targets.push(["Intl", "Intl"]);
for (const n of intlCtors) {
  targets.push(["Intl." + n, "Intl." + n]);
  targets.push(["Intl." + n + ".prototype", "Intl." + n + ".prototype"]);
}
targets.push(["Temporal", "Temporal"]);
targets.push(["Temporal.Now", "Temporal.Now"]);
for (const n of temporalCtors) {
  targets.push(["Temporal." + n, "Temporal." + n]);
  targets.push(["Temporal." + n + ".prototype", "Temporal." + n + ".prototype"]);
}
const hidden = {
  "SegmentsPrototype": "Object.getPrototypeOf(new Intl.Segmenter().segment('ab'))",
  "SegmentIteratorPrototype": "Object.getPrototypeOf(new Intl.Segmenter().segment('ab')[Symbol.iterator]())",
  "SegmentsInstance": "new Intl.Segmenter().segment('ab')",
  "SegmentIteratorInstance": "new Intl.Segmenter().segment('ab')[Symbol.iterator]()",
  "SegmentDataObject": "new Intl.Segmenter().segment('ab').containing(0)",
  "CollatorInstance": "new Intl.Collator()",
  "DateTimeFormatInstance": "new Intl.DateTimeFormat()",
  "DisplayNamesInstance": "new Intl.DisplayNames('en',{type:'region'})",
  "DurationFormatInstance": "new Intl.DurationFormat()",
  "ListFormatInstance": "new Intl.ListFormat()",
  "LocaleInstance": "new Intl.Locale('en-US')",
  "NumberFormatInstance": "new Intl.NumberFormat()",
  "PluralRulesInstance": "new Intl.PluralRules()",
  "RelativeTimeFormatInstance": "new Intl.RelativeTimeFormat()",
  "SegmenterInstance": "new Intl.Segmenter()",
  "DurationInstance": "new Temporal.Duration(1)",
  "InstantInstance": "new Temporal.Instant(0n)",
  "PlainDateInstance": "new Temporal.PlainDate(2020,1,2)",
  "PlainDateTimeInstance": "new Temporal.PlainDateTime(2020,1,2)",
  "PlainTimeInstance": "new Temporal.PlainTime(1,2,3)",
  "PlainMonthDayInstance": "new Temporal.PlainMonthDay(1,2)",
  "PlainYearMonthInstance": "new Temporal.PlainYearMonth(2020,1)",
  "ZonedDateTimeInstance": "new Temporal.ZonedDateTime(0n,'UTC')",
  "CollatorCompare": "new Intl.Collator().compare",
  "DateTimeFormatFormat": "new Intl.DateTimeFormat().format",
  "NumberFormatFormat": "new Intl.NumberFormat().format",
};
for (const [label, expr] of Object.entries(hidden)) targets.push([expr, label]);

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
    add(`T(()=>{try{${E}();return "ok"}catch(e){return e.name+": "+e.message}})`);
    add(`T(()=>{try{new (${E})();return "ok"}catch(e){return e.name+": "+e.message}})`);
    add(`T(()=>{try{new (${E})(undefined);return "ok"}catch(e){return e.name+": "+e.message}})`);
    add(`T(()=>{try{Reflect.apply(${E},{},[]);return "ok"}catch(e){return e.name+": "+e.message}})`);
  }
  for (const key of Reflect.ownKeys(obj)) describeKey(E, label, key, obj);
}

for (const [expr] of targets) {
  let obj;
  try { obj = (0, eval)(expr); } catch (e) { continue; }
  if (obj === null || (typeof obj !== "object" && typeof obj !== "function")) continue;
  describeObject(expr, expr, obj);
}
// Funções estáticas de Intl e Temporal.Now chamadas como método de outro objeto e sem argumentos.
for (const ns of ["Intl", "Temporal.Now"]) {
  const obj = (0, eval)(ns);
  for (const key of Object.getOwnPropertyNames(obj)) {
    if (typeof obj[key] !== "function" || /^[A-Z]/.test(key)) continue;
    add(`T(()=>${ns}[${JSON.stringify(key)}].call({}))`);
    add(`T(()=>new (${ns}[${JSON.stringify(key)}])())`);
  }
}

// ---- Execução.
const baseSources = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === SELF_TSV) continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.add(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}

const seen = new Set();
const programs = [];
let dup = 0;
for (const expr of exprs) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source)) continue;
  seen.add(source);
  if (baseSources.has(source)) { dup++; continue; }
  programs.push({ expr, source });
}

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 5000, killSignal: "SIGKILL" });
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
  programs.forEach(({ expr, source }, i) => {
    const r = results[i];
    if (r.code !== 0) {
      dropped++;
      process.stderr.write("filho falhou: " + JSON.stringify(expr).slice(0, 160) + " " + r.err.slice(0, 80) + "\n");
      return;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(JSON.stringify(source) + "\t" + JSON.stringify(r.out));
  });
  process.stdout.write(emitFactoredLines("builtin_shape_intl", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
