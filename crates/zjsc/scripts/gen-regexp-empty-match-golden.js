// Gera tests/golden/regexp_empty_match_bun.tsv: Symbol.split/replace/match/matchAll/search com regex que casa vazio,
// medido no bun 1.4.2. Cobre padrões que casam vazio (/(?:)/, /a*/, /\b/, /(?=x)/, lookbehind, âncoras), flags g, y, u, v
// combinadas, strings com surrogates (pares e isolados), limit 0/1/2**32-1/2**32/-1/NaN, replace com função (argumentos
// groups/offset/string) e com retorno de objeto com toString, lastIndex inicial variado (além do length, não inteiro,
// negativo, objeto com valueOf) e o avanço de lastIndex por code point (u/v) contra code unit.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda num bun filho novo,
// sem APIs de host, com timeout de 8 s; no máximo 6 filhos em paralelo. Programas de fonte repetida, ou já presentes em
// outro golden de tests/golden, são descartados. O prelúdio comum sai em tests/golden/regexp_empty_match.preludes.json.
// Uso: bun scripts/gen-regexp-empty-match-golden.js > tests/golden/regexp_empty_match_bun.tsv
const fs = require("fs");
const { spawn } = require("child_process");
const path = require("path");
const { emitFactored, readRows, GOLDEN_DIR, writeResultPreload, decodeResult } = require("./golden-prelude.js");

// Programas dos goldens vizinhos; arquivo em formato próprio é ignorado.
function knownSources() {
  const known = new Set();
  for (const file of fs.readdirSync(GOLDEN_DIR)) {
    if (!file.endsWith(".tsv") || file.startsWith("regexp_empty_match_bun")) continue;
    try {
      for (const row of readRows(file.replace(/(_bun)?\.tsv$/, ""), fs.readFileSync(path.join(GOLDEN_DIR, file), "utf8"))) known.add(row.source);
    } catch (e) {}
  }
  return known;
}

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const { usesHostApi } = require("./host-api.js");

const PRELUDE =
  '"use strict";\n' +
  'var L=[];\n' +
  'function S(v){try{if(typeof v==="string")return JSON.stringify(v);if(Object.is(v,-0))return "-0";if(typeof v==="bigint")return v+"n";' +
  'if(typeof v==="symbol")return v.toString();if(typeof v==="undefined")return "undefined";if(typeof v==="function")return "fn";' +
  'if(Array.isArray(v)){var r="["+Array.from({length:v.length},(_,i)=>i in v?S(v[i]):"<hole>").join(",")+"]";' +
  'if(v.index!==undefined)r+=" index="+v.index;if(v.input!==undefined)r+=" input="+S(v.input);if(v.groups!==undefined)r+=" groups="+S(v.groups);return r}' +
  'if(v!==null&&typeof v==="object"){return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k])).join(",")+"}"}return String(v)}catch(e){return "?"}}\n' +
  'function T(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.length?r+" | log="+L.map(S).join(","):r}\n' +
  'function D(re,str,fn){var o=[];var r=str.replace(re,function(){var a=Array.from(arguments);o.push(a.slice(0,-2).concat([a[a.length-2]]));return fn?fn.apply(null,a):"#"});return [r,o,re.lastIndex]}\n';

const programs = new Set();
const q = (s) => JSON.stringify(s).replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029");
// As grades grandes (A, B, C, D) entram amostradas (1 em 5, determinístico); as seções pequenas entram inteiras.
let thin = false;
let seen = 0;
const add = (body) => thin && seen++ % 5 ? 0 : programs.add(PRELUDE + `globalThis.R = T(()=>{${body}})`);

const PATTERNS = [
  "(?:)", "a*", "\\b", "\\B", "(?=x)", "(?<=a)", "(?<!a)", "a*?", "^", "$", "(a)?", "x*|a", "(?:a|)", "(?<n>a*)", "(?=(a))", "[^]*?", ".*?", "\\b|a", "(?<=\\ud83d)", "\\p{L}*",
];
const PATTERNS_NO_U = PATTERNS.filter((p) => !p.includes("\\p"));
const FLAGS = ["", "g", "y", "gy", "u", "gu", "yu", "guy", "v", "gv", "yv", "gvy", "gi", "gm", "gs", "gd"];
const INPUTS = [
  "", "abc", "aaa", "xax", "a b", "a\\ud83d\\ude00b", "a\\ud83d", "\\ude00a", "\\ud83d\\ude00\\ud83d\\ude00", "xa\\ud83d\\ude00x", "\\ud83d\\ude00", "ab\\n",
];
const LIMITS = ["undefined", "0", "1", "2", "3", "4294967295", "4294967296", "-1", "NaN", "'2'", "1.9", "4294967297", "Infinity", "-0", "{valueOf(){L.push('lim');return 2}}", "null", "true"];
const LASTS = ["0", "1", "2", "3", "4", "7", "100", "1.5", "-1", "NaN", "'1'", "2**32", "2**53", "-0", "Infinity", "{valueOf(){L.push('li');return 1}}", "undefined", "null", "0.9", "-1.5"];
const REPLS = ["'[$&]'", "'<$`|$\\'>'", "'$1'", "'$<n>'", "'$$'", "''", "'$0$01'", "'$10'"];
const FNS = [
  "null", "function(){return 'X'}", "function(){return {toString(){L.push('ts');return '<'+arguments.length+'>'}}}",
  "function(){return undefined}", "function(){return {valueOf(){return 1},toString(){return 'T'}}}", "function(){return 7}",
  "function(){return {toString(){throw new RangeError('boom')}}}", "function(){return Symbol.iterator.description}",
];

const mk = (p, f) => `new RegExp(${q(p)},${q(f)})`;
const valid = (p, f) => { try { new RegExp(p.replace(/\\\\/g, "\\"), f); return true; } catch { return false; } };
const jsStr = (s) => `"${s}"`;
let counter = 0;
const pick = (list) => list[counter++ % list.length];

thin = true;
// A. split: cada combinação com dois limits rotativos.
for (const p of PATTERNS) for (const f of FLAGS) {
  if (!valid(p, f)) continue;
  for (const s of INPUTS) {
    for (let k = 0; k < 2; k++) {
      const lim = pick(LIMITS);
      add(`var re=${mk(p, f)};re.lastIndex=${pick(LASTS)};var s=${jsStr(s)};return [S(s.split(re,${lim})),re.lastIndex]`);
    }
  }
}
thin = false;
// A2. split com cada limit sobre poucos padrões.
for (const p of ["(?:)", "a*", "\\b", "(?=x)", "(?<=a)", "(a)?"]) for (const f of ["", "u", "v", "y"]) for (const s of ["abc", "a\\ud83d\\ude00b", "\\ud83d\\ude00\\ud83d\\ude00", "aaa"]) for (const lim of LIMITS) {
  add(`return S(${jsStr(s)}.split(${mk(p, f)},${lim}))`);
}
add(`return S(RegExp.prototype[Symbol.split].call(/(?:)/u,"\\ud83d\\ude00a",undefined))`);
add(`return S(RegExp.prototype[Symbol.split].call(/(?:)/,"\\ud83d\\ude00a",3))`);
add(`var re=/a*/y;re.lastIndex=2;var r=S("baaac".split(re));return [r,re.lastIndex,re.flags]`);
add(`return S("".split(/(?:)/)) + S("".split(/a/)) + S("".split(/a*/y))`);

thin = true;
// B. replace com string de substituição e com função.
for (const p of PATTERNS) for (const f of FLAGS) {
  if (!valid(p, f)) continue;
  for (const s of INPUTS) {
    add(`var re=${mk(p, f)};re.lastIndex=${pick(LASTS)};return [${jsStr(s)}.replace(re,${pick(REPLS)}),re.lastIndex]`);
    const fn = pick(FNS);
    add(`var re=${mk(p, f)};re.lastIndex=${pick(LASTS)};return D(re,${jsStr(s)},${fn})`);
  }
}
thin = false;
for (const r of REPLS) for (const p of ["(?:)", "a*", "(a)?", "(?<n>a*)", "\\b"]) for (const f of ["g", "gu", "gv", ""]) {
  add(`return ${jsStr("a\\ud83d\\ude00b")}.replace(${mk(p, f)},${r})`);
  add(`return ${jsStr("xaax")}.replaceAll ? S([${jsStr("xaax")}.replace(${mk(p, f)},${r})]) : 0`);
}
for (const fn of FNS) for (const p of ["(?:)", "a*", "(a)?", "(?<n>a*)"]) for (const f of ["g", "gu", "y", "gy"]) {
  add(`return D(${mk(p, f)},${jsStr("aab\\ud83d\\ude00")},${fn})`);
}
// replaceAll exige g.
for (const p of PATTERNS_NO_U) for (const f of ["", "g", "gu", "y", "gy", "gv"]) {
  if (!valid(p, f)) continue;
  add(`return S("axa".replaceAll(${mk(p, f)},"-"))`);
  add(`return D(${mk(p, f)},"a\\ud83d\\ude00",null)`);
}

thin = true;
// C. match, matchAll, search.
for (const p of PATTERNS) for (const f of FLAGS) {
  if (!valid(p, f)) continue;
  for (const s of INPUTS) {
    add(`var re=${mk(p, f)};re.lastIndex=${pick(LASTS)};var r=${jsStr(s)}.match(re);return [r,re.lastIndex]`);
    add(`var re=${mk(p, f)};re.lastIndex=${pick(LASTS)};var r=Array.from(${jsStr(s)}.matchAll(re),function(m){return [m[0],m.index]});return [r,re.lastIndex]`);
    add(`var re=${mk(p, f)};re.lastIndex=${pick(LASTS)};var r=${jsStr(s)}.search(re);return [r,re.lastIndex]`);
  }
}

thin = true;
// D. lastIndex inicial variado em exec/test/laço de avanço manual.
for (const p of ["(?:)", "a*", "\\b", "(?=x)", "(?<=a)", "x*|a", "(?<!a)"]) for (const f of ["g", "y", "gy", "gu", "yu", "gv", "yv"]) {
  for (const s of ["abc", "a\\ud83d\\ude00b", "\\ud83d\\ude00", "xax"]) for (const li of LASTS) {
    add(`var re=${mk(p, f)};re.lastIndex=${li};var m=re.exec(${jsStr(s)});return [m,re.lastIndex]`);
    add(`var re=${mk(p, f)};re.lastIndex=${li};var t=re.test(${jsStr(s)});return [t,re.lastIndex]`);
  }
}
// D2. laço de exec: o lastIndex após cada casamento vazio (não avança sozinho).
for (const p of ["(?:)", "a*", "\\b", "(?=x)", "(?<=a)"]) for (const f of ["g", "gy", "gu", "gv", "guy"]) for (const s of INPUTS) {
  add(`var re=${mk(p, f)};var o=[];for(var i=0;i<8;i++){var m=re.exec(${jsStr(s)});o.push(m?m.index+":"+m[0].length:null,re.lastIndex);if(!m)break;if(m[0]==="")re.lastIndex++}return o`);
}
thin = false;
// D3. avanço por code point: matchAll/match/replace/split sobre pares em u/v contra code unit.
for (const f of ["g", "gu", "gv", "gy", "guy", "gvy"]) for (const s of ["\\ud83d\\ude00", "\\ud83d\\ude00\\ud83d\\ude00x", "\\ud83d", "\\ude00", "\\ud83d\\ud83d\\ude00", "a\\ud83d\\ude00\\ude00"]) {
  add(`var o=[];for(var m of ${jsStr(s)}.matchAll(${mk("(?:)", f)}))o.push(m.index);return o`);
  add(`return D(${mk("", f)},${jsStr(s)},null)`);
  add(`return ${jsStr(s)}.match(${mk("(?:)", f)})`);
  add(`return S(${jsStr(s)}.split(${mk("", f.replace("g", ""))}))`);
  add(`return S(${jsStr(s)}.split(${mk("", f.replace("g", ""))},2))`);
  add(`return D(${mk("[^]*?", f)},${jsStr(s)},null)`);
  add(`var re=${mk("(?:)", f)};re.lastIndex=1;return D(re,${jsStr(s)},null)`);
}
// D4. matchAll: o iterador clona o regex, copia lastIndex e não mexe no original.
for (const li of LASTS) for (const f of ["g", "gu", "gy", "gv"]) {
  add(`var re=${mk("a*", f)};re.lastIndex=${li};var it="baa\\ud83d\\ude00".matchAll(re);var r=Array.from(it,function(m){return [m[0],m.index]});return [r,re.lastIndex]`);
  add(`var re=${mk("(?:)", f)};re.lastIndex=${li};var it="\\ud83d\\ude00a".matchAll(re);var a=it.next().value;return [a&&a.index,re.lastIndex,it.next().value&&it.next().value.index]`);
}
for (const f of ["", "u", "y", "v", "i"]) add(`return "aa".matchAll(${mk("a*", f)})`);
add(`return Object.prototype.toString.call("a".matchAll(/a*/g))`);
add(`return "aaa".matchAll(/(?:)/g).toString()`);

// E. exec personalizado e species no Symbol.split/matchAll.
add(`var re=/a*/g;var calls=0;re.exec=function(s){calls++;L.push("e"+this.lastIndex);return calls<3?Object.assign([""],{index:this.lastIndex}):null};return "abc".replace(re,"-")`);
add(`var re=/a*/g;re.exec=function(s){L.push("e"+this.lastIndex);return null};return [ "abc".match(re), re.lastIndex ]`);
add(`var re=/(?:)/gu;var k=0;re.exec=function(s){L.push("e"+this.lastIndex);return k++<2?Object.assign([""],{index:this.lastIndex}):null};return "\\ud83d\\ude00a".replace(re,"-")`);
add(`var re=/(?:)/;re.constructor={[Symbol.species]:function(p,f){L.push("sp:"+f);return /(?:)/y}};return S("abc".split(re))`);
add(`var re=/(?:)/u;re.constructor={[Symbol.species]:function(p,f){L.push("sp:"+f);return new RegExp(p,f)}};return S("a\\ud83d\\ude00".split(re,2))`);
add(`var re=/a*/gu;re.constructor={[Symbol.species]:function(p,f){L.push("sp:"+f);return new RegExp(p,f)}};return Array.from("baa".matchAll(re),function(m){return m.index})`);
add(`var re=/a*/y;re.lastIndex=1;return S("baac".split(re))+re.lastIndex`);
add(`var re=/(?:)/g;Object.defineProperty(re,"lastIndex",{writable:false,value:0});return "abc".replace(re,"-")`);
add(`var re=/(?:)/y;Object.defineProperty(re,"lastIndex",{writable:false,value:5});return "abc".replace(re,"-")`);
add(`var re=/a*/g;Object.defineProperty(re,"lastIndex",{writable:false,value:0});return "aab".match(re)`);
add(`var re=/a*/;Object.defineProperty(re,"lastIndex",{writable:false,value:0});return "aab".replace(re,"-")`);
add(`var re=/a*/y;Object.defineProperty(re,"lastIndex",{writable:false,value:1});return "aab".replace(re,"-")`);
add(`var re=/a*/g;Object.defineProperty(re,"lastIndex",{get(){L.push("get");return 0},set(v){L.push("set"+v)}});return "ab".replace(re,"-")`);

// ---- Execução, com concorrência. Cada programa roda num bun filho novo.
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", () => {});
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve(code === 0 ? decodeResult(out) : null);
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const known = knownSources();
  let unique = [...programs].filter((p) => !usesHostApi(p) && !known.has(p));
  // Sem travessão literal e sem substituto isolado na fonte.
  unique = unique.filter((p) => !/[–—]/.test(p) && !/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(p) && !/[\t]/.test(p.slice(PRELUDE.length)));
  const results = new Array(unique.length);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
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
    if (
      result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result) || /[–—\n\r\t]/.test(result) || result.length > 6000 ||
      /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(result)
    ) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(unique[i].slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
      continue;
    }
    kept++;
    lines.push({ source: unique[i], result });
  }
  process.stdout.write(emitFactored("regexp_empty_match", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
})();
