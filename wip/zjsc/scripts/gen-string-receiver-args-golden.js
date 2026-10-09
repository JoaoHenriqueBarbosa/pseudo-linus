// Gera tests/golden/string_receiver_args_bun.tsv: grade de RECEPTORES e ARGUMENTOS dos métodos básicos de
// String.prototype, medida no bun 1.4.2. Receptores: undefined/null (TypeError com a mensagem exata), números, -0, NaN,
// booleano, objetos com toString/valueOf/@@toPrimitive que registram a ordem das chamadas, Symbol, BigInt, String
// boxed, arrays e funções. Argumentos: undefined, NaN, -0, Infinity, negativos, fracionários, strings numéricas,
// objetos com valueOf registrando a ordem, Symbol (TypeError), BigInt, e RegExp passada a includes/startsWith/endsWith
// (TypeError "must not be a regular expression") inclusive com Symbol.match false.
// Métodos: charAt, charCodeAt, at, codePointAt, indexOf, lastIndexOf, includes, startsWith, endsWith, slice, substring,
// substr, concat, repeat, padStart, padEnd, trim*, split, toString, valueOf, localeCompare, normalize, search, match,
// matchAll, replace, replaceAll, toLowerCase, toUpperCase.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host. Programas de fonte repetida são descartados.
// O prelúdio comum sai em tests/golden/string_receiver_args.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-string-receiver-args-golden.js > tests/golden/string_receiver_args_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const os = require("os");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var L=[];\n' +
  'function S(v){try{if(typeof v==="string")return JSON.stringify(v);if(Object.is(v,-0))return "-0";if(typeof v==="bigint")return v+"n";' +
  'if(typeof v==="symbol")return v.toString();if(typeof v==="undefined")return "undefined";if(typeof v==="function")return "fn";' +
  'if(Array.isArray(v)){var r="["+Array.from({length:v.length},(_,i)=>i in v?S(v[i]):"<hole>").join(",")+"]";' +
  'if(v.index!==undefined)r+=" index="+v.index;if(v.input!==undefined)r+=" input="+S(v.input);if(v.groups!==undefined)r+=" groups="+S(v.groups);return r}' +
  'if(v!==null&&typeof v==="object"){return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k])).join(",")+"}"}return String(v)}catch(e){return "?"}}\n' +
  'function T(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.length?r+" | log="+L.map(S).join(","):r}\n';

const calls = [];
const add = (method, recv, args, wrap) => calls.push({ method, recv, args, wrap });

// Objetos que registram a ordem das conversões.
const logToString = (tag, value) => `{toString(){L.push('${tag}.ts');return ${value}},valueOf(){L.push('${tag}.vo');return ${value}}}`;
const logValueOf = (tag, value) => `{valueOf(){L.push('${tag}.vo');return ${value}}}`;
const logPrim = (tag, value) => `{[Symbol.toPrimitive](h){L.push('${tag}.tp '+h);return ${value}}}`;
const regexFalse = (re) => `(()=>{var r=${re};r[Symbol.match]=false;return r})()`;

const receivers = [
  "undefined", "null", "42", "-0", "0", "NaN", "Infinity", "1.5", "true", "false", "1e21", "123456789012345680000",
  "''", "'abcde'", "'  ab  '",
  logToString("r", "'abc'"), logValueOf("r", "'xyz'"), logPrim("r", "'pq'"), logPrim("r", "7"),
  "{toString(){L.push('r.ts');return {}},valueOf(){L.push('r.vo');return 'vo'}}",
  "{toString(){L.push('r.ts');return {}},valueOf(){L.push('r.vo');return {}}}",
  "{toString(){L.push('r.ts');return Symbol('q')}}", "{toString(){throw new RangeError('boom')}}",
  "{toString:undefined,valueOf(){L.push('r.vo');return 'only'}}", "{[Symbol.toPrimitive]:1}", "{[Symbol.toPrimitive](){return {}}}",
  "{}", "Object.create(null)", "[]", "['a','b']", "[1,[2,3]]", "[null,undefined]", "function f(a,b){}", "()=>1", "new Date(0)", "/x/g",
  "Symbol('s')", "Symbol()", "Symbol.iterator", "10n", "-5n", "0n",
  "new String('hello')", "new String('')", "Object('abc')", "Object(5)", "Object(true)", "Object(1n)",
  "(()=>{var s=new String('boxed');s.toString=function(){L.push('own.ts');return 'own'};return s})()",
  "Object.assign(new String('b2'),{valueOf(){L.push('own.vo');return 'ov'}})",
  "new Proxy({},{get(t,k){L.push('get '+String(k));return undefined}})",
];
// Receptores que a grade de argumentos usa.
const argReceivers = ["'abcde'", "'abcabc'", "''", "new String('hello')", logToString("r", "'abcde'"), "undefined", "null", "12345", "['a','b','c']"];

const idx = [
  "undefined", "NaN", "-0", "0", "1", "2", "3", "4", "5", "6", "-1", "-2", "-5", "Infinity", "-Infinity", "1.9", "-1.9", "0.5", "-0.5",
  "2**31", "2**32", "2**53", "-(2**32)", "1e21", "'2'", "' 3 '", "'0x2'", "'1e1'", "'x'", "''", "null", "true", "false", "[]", "[3]", "[1,2]", "({})",
  "1n", "Symbol()", logValueOf("a", "2"), logValueOf("a", "-1"), logPrim("a", "1"), logPrim("a", "'3'"),
  "{valueOf(){L.push('a.vo');return {}},toString(){L.push('a.ts');return '2'}}",
  "{valueOf(){L.push('a.vo');return {}},toString(){L.push('a.ts');return {}}}",
  "{valueOf(){throw new EvalError('arg')}}", "new Number(2)", "new String('1')",
];
const idxSmall = ["undefined", "NaN", "-0", "0", "1", "3", "-1", "Infinity", "-Infinity", "1.9", "'2'", "null", "1n", "Symbol()", logValueOf("b", "2")];

const needles = [
  "undefined", "null", "NaN", "-0", "0", "1.5", "true", "''", "'c'", "'bc'", "'abc'", "'abcde'", "'abcdef'", "'xyz'", "'a'", "'e'", "'cd'",
  "'\\ud800'", "'a\\ud800'", "Symbol()", "1n", "[]", "['c']", "['b','c']", "({})", "new String('c')", "(function(){})", "123",
  logToString("n", "'cd'"), logValueOf("n", "'bc'"), logPrim("n", "'de'"), "{toString(){L.push('n.ts');return {}},valueOf(){L.push('n.vo');return 'c'}}",
  "{toString(){throw new RangeError('needle')}}", "{[Symbol.toPrimitive]:undefined,toString(){L.push('n.ts');return 'b'}}",
];
const regexes = [
  "/c/", "/c/g", "/c/y", "/c/i", "/(?:)/", "/x/", "new RegExp('c')", "RegExp('bc','g')", regexFalse("/c/"), regexFalse("/c/g"),
  "(()=>{var r=/c/;r[Symbol.match]=true;return r})()", "(()=>{var r=/c/;r[Symbol.match]=0;return r})()",
  "(()=>{var r=/c/;r[Symbol.match]=undefined;return r})()", "(()=>{var r=/c/;r[Symbol.match]='';return r})()",
  "(()=>{var r=/c/;r[Symbol.match]=null;return r})()", "(()=>{var r=/c/;r[Symbol.match]=NaN;return r})()",
  "(()=>{var r=/c/;r.toString=()=>'bc';r[Symbol.match]=false;return r})()",
  "(()=>{var r=/c/;r.constructor=null;r[Symbol.match]=false;return r})()",
  "{[Symbol.match]:true,toString(){return 'c'}}", "{[Symbol.match]:false,toString(){return 'c'}}", "{[Symbol.match]:1,toString(){L.push('o.ts');return 'c'}}",
  "{get [Symbol.match](){L.push('o.get');return true}}", "{get [Symbol.match](){L.push('o.get');return false},toString(){L.push('o.ts');return 'cd'}}",
  "new Proxy(/c/,{})", "new Proxy({},{get(t,k){L.push('p.get '+String(k));return false}})",
];

// ---- 1. Receptor x método, com zero, um e dois argumentos típicos.
const typical = {
  charAt: ["", "1"], charCodeAt: ["", "1"], at: ["", "-1"], codePointAt: ["", "0"],
  indexOf: ["", "'b'", "'b',1"], lastIndexOf: ["", "'b'", "'b',1"], includes: ["", "'b'", "'b',1"], startsWith: ["", "'a'", "'b',1"], endsWith: ["", "'c'", "'b',2"],
  slice: ["", "1", "1,3"], substring: ["", "1", "1,3"], substr: ["", "1", "1,3"], concat: ["", "'x'", "'x',1,null"], repeat: ["", "2", "0"],
  padStart: ["", "6", "6,'xy'"], padEnd: ["", "6", "6,'xy'"], trim: [""], trimStart: [""], trimEnd: [""], trimLeft: [""], trimRight: [""],
  split: ["", "'b'", "'',2"], toString: [""], valueOf: [""], localeCompare: ["", "'b'", "'abc'"], normalize: ["", "'NFD'", "'bad'"],
  search: ["", "'b'", "/b/"], match: ["", "'b'", "/b/g"], matchAll: ["", "'b'", "/b/g"], replace: ["", "'b','X'", "/b/g,'[$&]'"], replaceAll: ["", "'b','X'", "/b/g,'[$&]'"],
  toLowerCase: [""], toUpperCase: [""], toLocaleLowerCase: [""], toLocaleUpperCase: [""],
  isWellFormed: [""], toWellFormed: [""],
};
for (const recv of receivers) for (const [method, list] of Object.entries(typical)) for (const args of list) add(method, recv, args);

// ---- 2. Argumentos de índice única.
for (const method of ["charAt", "charCodeAt", "at", "codePointAt", "slice", "substring", "substr", "repeat"]) {
  for (const recv of argReceivers) for (const a of idx) add(method, recv, a);
}
// Pares de índices.
for (const method of ["slice", "substring", "substr"]) {
  for (const recv of ["'abcde'", logToString("r", "'abcde'")]) for (const a of idx) for (const b of idxSmall) add(method, recv, `${a},${b}`);
}
// padStart/padEnd: comprimento x preenchimento.
const fills = ["undefined", "''", "'x'", "'ab'", "null", "0", "Symbol()", "1n", logToString("f", "'zz'"), logPrim("f", "'q'"), "[1,2]", "' '"];
for (const method of ["padStart", "padEnd"]) {
  for (const recv of ["'abc'", logToString("r", "'abc'"), "undefined"]) for (const a of idx) add(method, recv, `${a}`);
  for (const a of idxSmall) for (const f of fills) add(method, "'abc'", `${a === "undefined" ? "6" : a},${f}`);
  for (const a of ["5", "2**30", "2**31", "2**32", "2**53", "Infinity"]) for (const f of ["'x'", "'xyz'", "''"]) add(method, "'abc'", `${a},${f}`);
}
// repeat com ordem de conversão receptor/contagem.
for (const recv of [logToString("r", "'ab'"), logPrim("r", "'ab'"), "undefined", "null"]) for (const a of idxSmall) add("repeat", recv, a);

// ---- 3. Busca: agulha x posição.
for (const method of ["indexOf", "lastIndexOf", "includes", "startsWith", "endsWith"]) {
  const checksRegex = ["includes", "startsWith", "endsWith"].includes(method);
  for (const recv of argReceivers) for (const n of needles) add(method, recv, n);
  for (const recv of ["'abcabc'", logToString("r", "'abcabc'")]) for (const n of ["'b'", "''", "'abc'", logToString("n", "'c'")]) for (const p of idx) add(method, recv, `${n},${p}`);
  for (const recv of ["'abcde'", "undefined", logToString("r", "'abcde'"), "null", "new String('abc')"]) for (const r of regexes) add(method, recv, r);
  for (const r of regexes.slice(0, 10)) for (const p of idxSmall) add(method, "'abcde'", `${r},${p}`);
  if (checksRegex) for (const r of regexes) add(method, "'abc/c/def'", `${r},1`);
  add(method, "'abc'", "...[]"); add(method, "'abc'", "...['b',1]"); add(method, "'abc'", "'b',undefined"); add(method, "'abc'", "undefined,undefined");
}

// ---- 4. concat.
for (const recv of ["'a'", logToString("r", "'a'"), "undefined", "new String('s')", "['x']"]) {
  for (const n of needles) add("concat", recv, n);
  for (const n of ["undefined", "null", "-0", "1n", "Symbol()", logToString("a", "'1'"), logPrim("b", "'2'")]) {
    for (const m of ["undefined", "-0", "1n", "Symbol()", logToString("b", "'2'"), "[]", "['q','w']", "{}", "true"]) add("concat", recv, `${n},${m}`);
  }
}

// ---- 5. split, localeCompare, normalize.
const limits = ["undefined", "0", "1", "2", "-1", "2**32", "2**32+1", "NaN", "Infinity", "'x'", "'2'", "null", "1.9", "-0", "1n", "Symbol()", logValueOf("l", "1"), logPrim("l", "2")];
for (const recv of ["'a,b,c,d'", "'abc'", "''", logToString("r", "'a,b'"), "undefined", "123", "['x','y']", "new String('a b')"]) {
  for (const n of [...needles, ",", "''", "'a,'", "' '", "undefined"].map((n) => (n === "," ? "','" : n))) add("split", recv, n);
  for (const r of regexes.slice(0, 8)) add("split", recv, r);
}
for (const n of ["','", "''", "undefined", "/,/", "{[Symbol.split](s,l){L.push('sp '+s+' '+l);return 'custom'}}", "{[Symbol.split]:null,toString(){return ','}}", "{[Symbol.split]:1}"]) {
  for (const l of limits) add("split", "'a,b,c'", `${n},${l}`);
}
for (const n of needles) { add("localeCompare", "'b'", n); add("localeCompare", logToString("r", "'b'"), n); add("localeCompare", "undefined", n); }
for (const n of ["'a'", "'b'", "'B'", "'ä'", "'z'", "''", "'10'", "'9'", "'abc'"]) for (const r of ["'a'", "'b'", "'B'", "'ä'", "'10'", "''", "'abd'"]) add("localeCompare", r, n);
for (const a of ["'a','en'", "'a',undefined,{sensitivity:'base'}", "'A',undefined,{sensitivity:'base'}", "'a','xx-bad-tag'", "'a',null", "'a',1", "'a',['en','de']", "'a',Symbol()", "'a','en',{numeric:true}"]) add("localeCompare", "'b'", a);
const forms = ["undefined", "'NFC'", "'NFD'", "'NFKC'", "'NFKD'", "'nfc'", "'NFX'", "''", "null", "1", "true", "Symbol()", "1n", "[]", "['NFD']", logToString("f", "'NFD'"), logPrim("f", "'NFKC'"), "{toString(){throw new TypeError('form')}}", "'NFC '", "'\\u004eFC'"];
for (const recv of ["'\\u00e9'", "'e\\u0301'", "'\\ufb01'", "'abc'", "''", "undefined", "null", logToString("r", "'\\u00e9'"), "12"]) for (const f of forms) add("normalize", recv, f);

// ---- 6. search / match / matchAll / replace / replaceAll.
const symbolArgs = (sym, tag) => [
  `{[Symbol.${sym}](s){L.push('${tag} '+S(s));return 'custom'}}`, `{[Symbol.${sym}]:null,toString(){L.push('o.ts');return 'b'}}`,
  `{[Symbol.${sym}]:undefined,toString(){L.push('o.ts');return 'b'}}`, `{[Symbol.${sym}]:1}`, `{[Symbol.${sym}]:{}}`,
  `{get [Symbol.${sym}](){L.push('o.get');return function(){return 'g'}}}`, `{[Symbol.${sym}](){throw new RangeError('sym')}}`,
  `Object.assign(Object('b'),{[Symbol.${sym}](){return 'boxed'}})`,
  `(()=>{var r=/b/;r[Symbol.${sym}]=function(s){L.push('own '+s);return 'own'};return r})()`,
  `(()=>{var r=/b/;r[Symbol.${sym}]=null;return r})()`,
];
const regexOnly = ["/b/", "/b/g", "/b/y", "/b/gy", "/(b)/", "/(?<n>b)/", "/(?<n>b)/g", "/x/", "/x/g", "/(?:)/", "/(?:)/g", "/b*/g", "/./gu", "/\\b/g", "/$/", "/^/gm", "new RegExp('[')"].map((r) => (r.startsWith("new") ? `(()=>{try{return ${r}}catch(e){return 'bad'}})()` : r));
for (const [method, sym] of [["search", "search"], ["match", "match"], ["matchAll", "matchAll"], ["replace", "replace"], ["replaceAll", "replaceAll"]]) {
  const wrapArgs = (a) => (method === "replace" || method === "replaceAll" ? `${a},'X'` : a);
  for (const recv of ["'abcabc'", logToString("r", "'abcabc'"), "undefined", "null", "12321", "new String('abab')", "['b']", "Symbol('b')"]) {
    for (const n of needles) add(method, recv, wrapArgs(n));
    for (const r of regexOnly) add(method, recv, wrapArgs(r));
  }
  for (const a of symbolArgs(method === "replaceAll" ? "replace" : sym, method)) for (const recv of ["'abcabc'", logToString("r", "'abcabc'")]) add(method, recv, wrapArgs(a));
  if (method === "matchAll" || method === "replaceAll") for (const r of ["/b/", "/b/y", "/(?:)/", "(()=>{var r=/b/g;r.flags='';return r})()", "(()=>{var r=/b/g;Object.defineProperty(r,'flags',{get(){L.push('flags');return 'g'}});return r})()", "(()=>{var r=/b/;Object.defineProperty(r,'flags',{value:undefined});return r})()", "(()=>{var r=/b/g;Object.defineProperty(r,'flags',{value:null});return r})()", "(()=>{var r=/b/g;Object.defineProperty(r,'flags',{value:'x'});return r})()"]) add(method, "'abcabc'", wrapArgs(r));
}
const replacements = ["undefined", "null", "''", "'X'", "'$&'", "'$`'", "\"$'\"", "'$$'", "'$1'", "'$<n>'", "'$0'", "'$10'", "'[$&$&]'", "1", "-0", "true", "1n", "Symbol()", "[]", "({})",
  logToString("p", "'Z'"), logPrim("p", "'Y'"), "{toString(){throw new SyntaxError('rep')}}",
  "function(){L.push('fn '+Array.prototype.map.call(arguments,S).join('|'));return 'F'}", "function(){L.push('fn');return undefined}", "function(){L.push('fn');return Symbol()}",
  "function(){L.push('fn');return {toString(){L.push('ret.ts');return 'T'}}}", "()=>1n", "class{}", "async function(){}", "Symbol.iterator",
];
for (const method of ["replace", "replaceAll"]) {
  for (const recv of ["'abcabc'", logToString("r", "'abcabc'")]) {
    for (const s of ["'b'", "''", "'x'", "/b/", "/b/g", "/(b)(c)/g", "/(?<n>b)/g", "/b/y", "'abcabc'", logToString("s", "'c'"), "1", "undefined"]) {
      for (const rep of replacements) add(method, recv, `${s},${rep}`);
    }
  }
}
for (const method of ["match", "matchAll", "search"]) for (const a of ["", "undefined", "null", "'('", "/(/", "'[a-'", "'(?<n>b)'"]) {
  if (a === "/(/") continue;
  add(method, "'abcabc'", a); add(method, logToString("r", "'abc'"), a);
}

// ---- 7. Métodos sem argumento: receptor x método (cobertura de caso, com argumentos supérfluos).
for (const method of ["trim", "trimStart", "trimEnd", "toString", "valueOf", "toLowerCase", "toUpperCase", "toLocaleLowerCase", "toLocaleUpperCase", "isWellFormed", "toWellFormed"]) {
  for (const a of ["1", "'x',2", "undefined", logToString("a", "'q'"), "Symbol()"]) for (const recv of ["'  Ab\\u00c7 '", "'\\ud800x'", logToString("r", "'  P  '"), "undefined", "new String(' b ')", "5", "Symbol()", "null"]) add(method, recv, a);
}
for (const recv of ["'  x \\n\\t\\u00a0\\ufeff\\u2028\\u2029\\u1680\\u2003\\u200b'", "'\\u180e a'", "'\\u0085a\\u0085'", "'ÇßİıΣς\\ufb01'", "'ǅ'", "'\\ud83d\\ude00'", "'\\ude00\\ud83d'"]) {
  for (const method of ["trim", "trimStart", "trimEnd", "toLowerCase", "toUpperCase", "toLocaleLowerCase", "toLocaleUpperCase", "isWellFormed", "toWellFormed"]) add(method, recv, "");
}
for (const lang of ["'tr'", "'lt'", "'az'", "'en'", "undefined", "null", "1", "Symbol()", "['tr']", "'x-bad'", "''", "'tr','en'", logToString("l", "'tr'")]) for (const method of ["toLocaleLowerCase", "toLocaleUpperCase"]) for (const recv of ["'Iİıi'", "'abc'", "undefined", logToString("r", "'I'")]) add(method, recv, lang);

// Receptor genérico via call/apply com this primitivo em modo sloppy e strict.
for (const method of ["charAt", "indexOf", "concat", "slice", "trim", "toString", "valueOf", "repeat", "at", "split", "replace"]) {
  for (const t of ["undefined", "null", "1", "true", "Symbol()", "1n", "'s'", "{}", "[]"]) {
    calls.push({ method, recv: t, args: method === "replace" ? "'s','x'" : method === "repeat" ? "2" : "0", wrap: "strict-this" });
    calls.push({ method, recv: t, args: method === "replace" ? "'s','x'" : method === "repeat" ? "2" : "0", wrap: "apply" });
    calls.push({ method, recv: t, args: method === "replace" ? "'s','x'" : method === "repeat" ? "2" : "0", wrap: "reflect" });
  }
}
// Métodos tirados do protótipo e chamados sem receptor.
for (const method of ["charAt", "indexOf", "trim", "toString", "valueOf", "at", "concat", "slice", "toLowerCase", "split", "match", "replace", "normalize", "localeCompare"]) {
  calls.push({ method, recv: "", args: "", wrap: "detached" });
  calls.push({ method, recv: "", args: "", wrap: "detached-strict" });
}
// Identidade e propriedades dos métodos.
for (const method of Object.keys(typical)) {
  calls.push({ method, recv: "", args: "", wrap: "meta" });
}
const unique = [];
const seenSources = new Set();

function buildSource(c) {
  const m = JSON.stringify(c.method);
  const proto = `String.prototype[${m}]`;
  let body;
  const matchAllWrap = (expr) => (c.method === "matchAll" ? `(()=>{var it=${expr};var r=Array.from(it);return typeof it+':'+Object.prototype.toString.call(it)+' '+S(r)})()` : expr);
  switch (c.wrap) {
    case "strict-this":
      body = `(function(){'use strict';return ${proto}.call(${c.recv},${c.args})})()`; break;
    case "apply":
      body = `${proto}.apply(${c.recv},[${c.args}])`; break;
    case "reflect":
      body = `Reflect.apply(${proto},${c.recv},[${c.args}])`; break;
    case "detached":
      body = `(0,${proto})(${c.args})`; break;
    case "detached-strict":
      body = `(function(){'use strict';var f=${proto};return f()})()`; break;
    case "meta":
      body = `(()=>{var d=Object.getOwnPropertyDescriptor(String.prototype,${m});return [typeof d.value,d.value.name,d.value.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(d.value,'prototype'),String(Object.getPrototypeOf(d.value)===Function.prototype)].join()})()`; break;
    default:
      body = matchAllWrap(`${proto}.call(${c.recv}${c.args === "" ? "" : "," + c.args})`);
  }
  return '"use strict";\n' + PRELUDE + `globalThis.R = T(()=>${body})`;
}
for (const c of calls) {
  const source = buildSource(c);
  if (seenSources.has(source)) continue;
  seenSources.add(source);
  unique.push(source);
}

// ---- Execução, com concorrência. Cada programa roda num bun filho novo.
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", () => {});
    child.on("close", (code) => resolve(code === 0 ? decodeResult(out) : null));
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(unique.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(4, os.cpus().length) }, async () => {
    while (next < unique.length) {
      const i = next++;
      results[i] = await runChild(unique[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < unique.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(unique[i].slice(-160)) + "\n");
      continue;
    }
    kept++;
    lines.push({ source: unique[i], result: result });
  }
  process.stdout.write(emitFactored("string_receiver_args", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
})();
