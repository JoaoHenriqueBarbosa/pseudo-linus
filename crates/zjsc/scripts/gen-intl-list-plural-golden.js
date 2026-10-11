// Gera tests/golden/intl_list_plural_bun.tsv: Intl.ListFormat (type, style, 15 locales, listas de 0 a 5 itens,
// formatToParts, iteráveis exóticos e itens não string), Intl.PluralRules (select e selectRange, cardinal e ordinal em
// 20 locales, números de fronteira, minimumFractionDigits) e Intl.Segmenter (grapheme, word e sentence em 10 locales,
// containing() em índices de fronteira, isWordLike), medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda num bun filho novo,
// sem APIs de host, com timeout de 8 s e no máximo 6 filhos ao mesmo tempo.
// Os programas já presentes nos outros goldens (knownPrograms) são descartados.
// Uso: bun scripts/gen-intl-list-plural-golden.js > tests/golden/intl_list_plural_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = JSON.stringify;
const T = (body) => `T(()=>${body})`;

// ---- 1. Intl.ListFormat.
const listLocales = ["en", "pt", "es", "fr", "de", "it", "ja", "zh", "ko", "ru", "ar", "hi", "tr", "pl", "nl"];
const listTypes = ["conjunction", "disjunction", "unit"];
const listStyles = ["long", "short", "narrow"];
const items = ["Ω1", "bé2", "γ3", "dü4", "ø5"];
for (const loc of listLocales) for (const type of listTypes) for (const style of listStyles) {
  const lf = `new Intl.ListFormat(${q(loc)},{type:${q(type)},style:${q(style)}})`;
  for (let n = 0; n <= 5; n++) add(T(`${lf}.format(${q(items.slice(0, n))})`));
  for (const n of [2, 3, 5]) add(T(`${lf}.formatToParts(${q(items.slice(0, n))}).map(p=>p.type[0]+":"+p.value)`));
}
const exotic = [
  `function*(){yield "x1";yield "y2";yield "z3"}()`,
  `new Set(["s1","s2"])`,
  `"abc"`,
  `new Map([["k","v"]])`,
  `{length:2,0:"a",1:"b"}`,
  `[1,"a"]`,
  `["a",null]`,
  `["a",undefined]`,
  `["a",{}]`,
  `["a",Symbol("s")]`,
  `[new String("a")]`,
  `["a",2n]`,
  `["a",,"c"]`,
  `42`,
  `null`,
  `undefined`,
  `true`,
  `{[Symbol.iterator](){return {next(){throw new RangeError("boom")}}}}`,
  `{[Symbol.iterator](){let i=0;return {next(){return i++<2?{value:"q"+i,done:false}:{done:true}},return(){globalThis.RET=(globalThis.RET||0)+1;return {}}}}}`,
  `{[Symbol.iterator](){let i=0;return {next(){return i++<2?{value:i,done:false}:{done:true}},return(){globalThis.RET=(globalThis.RET||0)+1;return {}}}}}`,
  `{[Symbol.iterator]:1}`,
  `{[Symbol.iterator](){return 1}}`,
  `{[Symbol.iterator](){return {next(){return 1}}}}`,
  `Object.create(null)`,
  `[]`,
  `new Proxy(["p","q"],{})`,
  `Object.assign(["a","b"],{extra:"c"})`,
  `["a\\u0000b","\\ud800"]`,
];
for (const loc of ["en", "pt", "ja", "ar"]) for (const type of listTypes) for (const e of exotic) {
  const lf = `new Intl.ListFormat(${q(loc)},{type:${q(type)}})`;
  add(T(`${lf}.format(${e})`));
  if (loc === "en") add(T(`${lf}.formatToParts(${e}).length`));
}
for (const e of exotic) add(T(`(globalThis.RET=0,new Intl.ListFormat("en").format(${e}),globalThis.RET)`));
for (const bad of [`{type:"x"}`, `{style:"x"}`, `{type:null}`, `{style:null}`, `{type:"Conjunction"}`, `{localeMatcher:"x"}`, `1`, `"str"`, `null`, `{get type(){throw new TypeError("g")}}`]) {
  add(T(`new Intl.ListFormat("en",${bad}).resolvedOptions().type`));
}
for (const loc of listLocales) add(T(`new Intl.ListFormat(${q(loc)}).resolvedOptions().locale`));
for (const loc of ["en-US-u-nu-arab", "pt-BR", "es-419", "zh-Hant", "de-AT", "fr-CA", "en-GB", "sr-Latn", "xx", "und"]) {
  add(T(`new Intl.ListFormat(${q(loc)}).format(["A1","B2","C3"])+"|"+new Intl.ListFormat(${q(loc)}).resolvedOptions().locale`));
}

// ---- 2. Intl.PluralRules.
const plLocales = ["en", "pt", "es", "fr", "de", "ru", "ar", "pl", "cs", "ja", "zh", "ko", "he", "lt", "lv", "ro", "sl", "cy", "ga", "gd"];
const plNumbers = ["0", "1", "2", "3", "4", "5", "6", "7", "10", "11", "12", "13", "14", "20", "21", "22", "23", "100", "101", "102", "103", "111", "112",
  "1000", "1000000", "1e21", "1.5", "0.5", "2.5", "-1", "-0", "0.1", "1.0", "21.5", "0.0001", "NaN", "Infinity", "-Infinity", "12345678901234567890"];
for (const loc of plLocales) for (const type of ["cardinal", "ordinal"]) for (const n of plNumbers) {
  add(T(`new Intl.PluralRules(${q(loc)},{type:${q(type)}}).select(${n})`));
}
for (const loc of plLocales) for (const n of ["0", "1", "1.5", "2", "11", "21"]) for (const mfd of [0, 1, 2, 3]) {
  add(T(`new Intl.PluralRules(${q(loc)},{minimumFractionDigits:${mfd}}).select(${n})`));
}
for (const loc of plLocales) for (const n of ["1", "1.20", "2.00"]) for (const msd of [1, 2, 3]) {
  add(T(`new Intl.PluralRules(${q(loc)},{maximumSignificantDigits:${msd}}).select(${n})+","+new Intl.PluralRules(${q(loc)},{maximumFractionDigits:${msd - 1}}).select(${n})`));
}
const ranges = [[0, 1], [1, 2], [1, 5], [2, 2], [0, 0], [21, 22], [1.5, 2], [11, 101], [0, 2], [3, 11], [5, 1], [-1, 1], [1, 21]];
for (const loc of plLocales) for (const type of ["cardinal", "ordinal"]) for (const [a, b] of ranges) {
  add(T(`new Intl.PluralRules(${q(loc)},{type:${q(type)}}).selectRange(${a},${b})`));
}
for (const loc of ["en", "ar", "ru", "ja"]) for (const args of ["", "1", "undefined,1", "1,undefined", "NaN,1", "1,NaN", "Infinity,1", "'1','2'", "1n,2n", "null,null", "{},1", "1,{valueOf(){return 3}}"]) {
  add(T(`new Intl.PluralRules(${q(loc)}).selectRange(${args})`));
}
for (const loc of ["en", "ar", "ru", "pl", "ja"]) for (const type of ["cardinal", "ordinal"]) {
  add(T(`new Intl.PluralRules(${q(loc)},{type:${q(type)}}).resolvedOptions().pluralCategories.slice().sort().join()`));
  add(T(`new Intl.PluralRules(${q(loc)},{type:${q(type)},minimumFractionDigits:2}).resolvedOptions().minimumFractionDigits`));
}
for (const bad of [`{type:"x"}`, `{type:null}`, `{minimumFractionDigits:-1}`, `{minimumFractionDigits:101}`, `{maximumFractionDigits:0,minimumFractionDigits:2}`, `{minimumSignificantDigits:0}`, `{maximumSignificantDigits:22}`, `{roundingMode:"x"}`, `{notation:"compact"}`]) {
  add(T(`JSON.stringify(new Intl.PluralRules("en",${bad}).resolvedOptions())`));
}

// ---- 3. Intl.Segmenter.
const segLocales = ["en", "pt", "ja", "zh", "ko", "th", "ar", "de", "ru", "hi"];
const segTexts = [
  "Hello, world! How are you?",
  "👨‍👩‍👧‍👦 family 🇧🇷🇺🇸 flag 👍🏽",
  "日本語のテキスト。これは文です。",
  "สวัสดีครับ ผมชื่อจอห์น",
  "éà \r\n x\ty",
  "Mr. Smith went. He said: \"Hi!\" Ok? Yes… Dr. J. Doe, Ph.D. e.g. no.",
  "中文分词测试，你好世界。",
  "한국어 텍스트입니다. 좋아요!",
  "123,456.78 foo_bar don't can’t 3.14 a.b c:d",
  "😀😃 a😀b 🧑‍💻 ❤️ 1️⃣",
];
for (const gran of ["grapheme", "word", "sentence"]) for (const loc of segLocales) for (let t = 0; t < segTexts.length; t++) {
  const text = segTexts[t];
  const sg = `new Intl.Segmenter(${q(loc)},{granularity:${q(gran)}})`;
  add(T(`Array.from(${sg}.segment(${q(text)}),x=>[x.segment,x.index,x.isWordLike])`));
  const len = text.length;
  const idx = [...new Set([-1, 0, 1, 2, 3, Math.floor(len / 2), len - 2, len - 1, len, len + 1])];
  for (const i of idx) {
    add(T(`(x=>x&&[x.segment,x.index,x.isWordLike,x.input===${q(text)}])(${sg}.segment(${q(text)}).containing(${i}))`));
  }
}
for (const gran of ["grapheme", "word", "sentence"]) for (const loc of ["en", "ja", "th"]) {
  const sg = `new Intl.Segmenter(${q(loc)},{granularity:${q(gran)}}).segment("ab cd. Ef")`;
  for (const arg of ["", "undefined", "null", "NaN", "Infinity", "-Infinity", "1.5", "-0.5", "'2'", "{valueOf(){return 3}}", "2**32", "-1", "1e300", "2n", "Symbol()"]) {
    add(T(`(x=>x&&[x.segment,x.index])(${sg}.containing(${arg}))`));
  }
  add(T(`Object.keys(${sg}.containing(0)).join()`));
  add(T(`Object.getPrototypeOf(${sg}[Symbol.iterator]())[Symbol.toStringTag]`));
  add(T(`(it=>[it.next().value.segment,it[Symbol.iterator]()===it])(${sg}[Symbol.iterator]())`));
}
for (const text of ["", " ", "a", "ก", "\ud83d", "\ude00", "a\ud83d", "\r", "\r\n", "\n\r"]) for (const gran of ["grapheme", "word", "sentence"]) {
  add(T(`Array.from(new Intl.Segmenter("en",{granularity:${q(gran)}}).segment(${q(text)}),x=>[x.segment,x.index,x.isWordLike])`));
  add(T(`(x=>x&&x.index)(new Intl.Segmenter("th",{granularity:${q(gran)}}).segment(${q(text)}).containing(0))`));
}
for (const bad of [`{granularity:"x"}`, `{granularity:null}`, `{granularity:"Word"}`, `{localeMatcher:"x"}`, `1`, `null`]) {
  add(T(`JSON.stringify(new Intl.Segmenter("en",${bad}).resolvedOptions())`));
}
for (const loc of segLocales) add(T(`JSON.stringify(new Intl.Segmenter(${q(loc)},{granularity:"word"}).resolvedOptions())`));

// ---- Execução.
const goldenDir = require("./golden-prelude.js").GOLDEN_DIR;
const baseSet = new Set();
for (const name of fs.readdirSync(goldenDir)) {
  if (!name.endsWith("_bun.tsv") || name === "intl_list_plural_bun.tsv") continue;
  for (const program of knownPrograms("intl_list_plural_bun.tsv", [name])) baseSet.add(program);
}
const seen = new Set();
const jobs = [];
let dup = 0;
for (const expr of exprs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSet.has(source)) { dup++; continue; }
  jobs.push(source);
}

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 8000, killSignal: "SIGKILL" });
    let out = "";
    child.stdout.on("data", (chunk) => { out += chunk; });
    child.stderr.on("data", () => {});
    child.on("close", (code, signal) => resolve(code === 0 && !signal ? decodeResult(out) : null));
    child.on("error", () => resolve(null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i]);
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  const rows = [];
  let dropped = 0;
  jobs.forEach((source, i) => {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(source.slice(PRELUDE.length + 14)).slice(0, 160) + "\n");
      return;
    }
    rows.push({ source, result });
  });
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
  // Sem travessão literal no tsv: o en dash e o em dash do resultado saem como escape JSON.
  process.stdout.write(emitFactored("intl_list_plural", rows).split(String.fromCharCode(0x2013)).join("\\u2013").split(String.fromCharCode(0x2014)).join("\\u2014"));
})();
