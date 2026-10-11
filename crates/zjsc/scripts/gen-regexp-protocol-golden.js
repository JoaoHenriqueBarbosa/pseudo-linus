// Gera tests/golden/regexp_protocol_bun.tsv: protocolo observável de RegExp, medido no bun 1.4.2.
// Cobre Symbol.match/matchAll/replace/search/split em objetos regexp-like e em subclasses com `exec` sobrescrito (valores
// de retorno estranhos: null, não objeto, índices e groups esquisitos), a ordem de leitura dos getters de flags
// (hasIndices, global, ignoreCase, multiline, dotAll, unicode, unicodeSets, sticky) via objetos com getters e Proxy,
// `lastIndex` não gravável (TypeError), `lastIndex` com valueOf/toString/toPrimitive e `replaceAll`/`matchAll` com
// regexp sem a flag g (TypeError). Cada programa roda num bun filho novo, sem APIs de host, com timeout de 8 s; no
// máximo 6 filhos em paralelo. Programas repetidos, ou já presentes em goldens vizinhos (regexp_*), são descartados.
// O prelúdio comum sai em tests/golden/regexp_protocol.preludes.json.
// Uso: bun scripts/gen-regexp-protocol-golden.js > tests/golden/regexp_protocol_bun.tsv
const fs = require("fs");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");

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
  'function E(){var a=arguments,i=0;return function(){L.push("e"+this.lastIndex);return i<a.length?a[i++]:null}}\n' +
  'function I(it){var r=[];for(var i=0;i<4;i++){var n=it.next();if(n.done)break;r.push(n.value)}return r}\n' +
  'var THROW={};\n' +
  'var NAMES=["hasIndices","global","ignoreCase","multiline","dotAll","unicode","unicodeSets","sticky"];\n' +
  'function G(ov,src){var o={};NAMES.forEach(function(n){Object.defineProperty(o,n,{get:function(){L.push(n);if(n in ov){var v=ov[n];if(v===THROW)throw new EvalError("t"+n);return v}return false}})});' +
  'Object.defineProperty(o,"source",{value:src===undefined?"a":src});return o}\n' +
  'function P(t,raw){return new Proxy(t,{get:function(x,k,r){L.push(typeof k==="symbol"?k.toString():k);return raw?Reflect.get(x,k,r):Reflect.get(x,k,x)}})}\n';

const programs = new Set();
const add = (body) => programs.add(PRELUDE + `globalThis.R = T(()=>{${body}})`);
const q = (s) => JSON.stringify(s);

// ---- A. exec sobrescrito devolvendo valores estranhos.
const vals = [
  "null", "undefined", "0", '"s"', "true", 'Symbol("q")', "{}", "[]", "function(){}", "{length:0}",
  '{0:"a",index:0}', '{0:"a",index:1.5}', '{0:"ab",index:-1}', '{0:"",index:0}', '{0:"a"}',
  '{length:3,0:"a",1:"b",2:"c",index:0,groups:{x:"G"}}', '{0:"a",index:0,groups:null}', '{0:"a",index:0,groups:5}',
  '{0:"a",index:0,groups:{n:undefined}}', '{0:{toString(){L.push("ts");return "b"}},index:0}',
  '{0:{toString(){throw new RangeError("bad")}},index:0}', '{0:"a",index:{valueOf(){L.push("iv");return 1}}}',
  '{0:"a",index:"2"}', '{0:"a",index:NaN}', '{0:"a",index:Infinity}', '{length:2**32,0:"a",index:0}',
  '{length:-1,0:"a",index:0}', '{length:1.9,0:"a",1:"b",index:0}', "{0:undefined,index:0}", '{0:"abc",index:0,1:"z"}',
  'new Proxy({0:"a",index:0},{get(t,k){L.push("g:"+String(k));return t[k]}})',
];
const inputs = ['"aXbX"', '""'];
const plainFlags = ['""', '"g"', '"gu"', '"y"'];
const plainOps = [
  (s) => `RegExp.prototype[Symbol.match].call(o,${s})`,
  (s) => `RegExp.prototype[Symbol.replace].call(o,${s},"[$&|$1|$<n>]")`,
  (s) => `RegExp.prototype[Symbol.replace].call(o,${s},function(){L.push("fn:"+arguments.length);return "R"})`,
  (s) => `RegExp.prototype[Symbol.replace].call(o,${s},"x")`,
  (s) => `RegExp.prototype[Symbol.search].call(o,${s})`,
];
for (const v of vals) for (const f of plainFlags) for (const s of inputs) for (const op of plainOps)
  add(`var o={exec:E(${v}),flags:${f},lastIndex:0};return [${op(s)},o.lastIndex]`);

const subFlags = ['""', '"g"', '"y"', '"gy"'];
const subOps = [
  (s) => `${s}.match(o)`,
  (s) => `${s}.replace(o,"[$&|$1]")`,
  (s) => `${s}.replace(o,function(){L.push("fn:"+arguments.length);return "R"})`,
  (s) => `${s}.search(o)`,
  (s) => `${s}.split(o,3)`,
  (s) => `I(${s}.matchAll(o))`,
  (s) => `${s}.replaceAll(o,"z")`,
];
for (const v of vals) for (const f of subFlags) for (const s of inputs) for (const op of subOps)
  add(`class X extends RegExp{} X.prototype.exec=E(${v});var o=new X("a",${f});return [${op(s)},o.lastIndex]`);

// ---- B. ordem de leitura das flags.
const flagNames = ["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky"];
const flagVals = ["true", "false", "1", "0", '""', '"x"', "{}", "null", "undefined", "THROW", "NaN"];
const flagPaths = [
  (ov) => `Object.getOwnPropertyDescriptor(RegExp.prototype,"flags").get.call(G(${ov}))`,
  (ov) => `RegExp.prototype.toString.call(G(${ov},"a"))`,
  (ov) => `RegExp.prototype.toString.call(P(G(${ov},"b"),false))`,
];
for (const n of flagNames) for (const v of flagVals) for (const path of flagPaths) add(`return ${path(`{${n}:${v}}`)}`);
for (let i = 0; i < flagNames.length; i++) for (let j = i + 1; j < flagNames.length; j++) for (const a of ["true", "THROW"]) for (const b of ["true", "THROW"])
  for (const path of flagPaths.slice(0, 2)) add(`return ${path(`{${flagNames[i]}:${a},${flagNames[j]}:${b}}`)}`);
for (const n of flagNames) for (const path of flagPaths) add(`return ${path(`{${flagNames.join(":true,")}:true}`)}`), add(`return ${path(`{${n}:true,${flagNames[(flagNames.indexOf(n) + 3) % 8]}:true}`)}`);

const proxyFlags = ['""', '"g"', '"y"', '"gi"', '"dgimsuy"', '"v"', '"gv"', '"gm"', '"su"', '"d"'];
const proxyOps = [
  (s) => `RegExp.prototype[Symbol.match].call(o,${s})`,
  (s) => `RegExp.prototype[Symbol.replace].call(o,${s},"x")`,
  (s) => `RegExp.prototype[Symbol.search].call(o,${s})`,
  (s) => `RegExp.prototype[Symbol.split].call(o,${s})`,
  (s) => `I(RegExp.prototype[Symbol.matchAll].call(o,${s}))`,
  (s) => `${s}.match(o)`,
  (s) => `${s}.replace(o,"y")`,
];
for (const f of proxyFlags) for (const raw of ["true", "false"]) for (const s of inputs) for (const op of proxyOps)
  add(`var o=P(new RegExp("a",${f}),${raw});return ${op(s)}`);
const objFlags = ['"g"', '""', '"y"', '"gy"', "undefined", "null", "1", "{}", '"i"', '"gimsuy"', '"dg"', '"vg"'];
for (const f of objFlags) for (const s of inputs) for (const op of proxyOps.slice(0, 5))
  add(`var o=P({exec:E(null),flags:${f},lastIndex:0},true);return ${op(s)}`);

// ---- C. lastIndex com valueOf/toString/toPrimitive e não gravável.
const lastVals = [
  '{valueOf(){L.push("vo");return 1}}', '{valueOf(){L.push("vo");return -1}}', '{valueOf(){L.push("vo");return 2**53}}',
  '{valueOf(){L.push("vo");return Infinity}}', '{valueOf(){L.push("vo");return NaN}}', '{valueOf(){L.push("vo");return "2"}}',
  '{valueOf(){throw new RangeError("v")}}', '{toString(){L.push("ts");return "1"}}', '{[Symbol.toPrimitive](h){L.push(h);return 1}}',
  'Symbol("s")', "1n", '"abc"', "1.7", "-0", "-1.5", "2**32", "2**53-1", "4294967297", "null", "undefined", "true",
  '{valueOf(){L.push("vo");return 99}}', '{valueOf(){L.push("vo");return {}},toString(){L.push("ts");return "0"}}',
];
const reFlags = ['""', '"g"', '"y"', '"gy"', '"gu"'];
const liOps = [
  `re.exec("aXbX")`, `re.test("aXbX")`, `"aXbX".match(re)`, `"aXbX".replace(re,"x")`, `"aXbX".search(re)`,
  `"aXbX".split(re)`, `I("aXbX".matchAll(re))`, `"aXbX".replaceAll(re,"q")`,
];
for (const v of lastVals) for (const f of reFlags) for (const op of liOps)
  add(`var re=new RegExp("a|b|",${f});re.lastIndex=${v};var r=${op};return [r,typeof re.lastIndex==="object"?"obj":re.lastIndex]`);

const frozenOps = [
  `re.exec(s)`, `re.test(s)`, `s.match(re)`, `s.replace(re,"x")`, `s.search(re)`, `s.split(re)`, `I(s.matchAll(re))`,
  `s.replaceAll(re,"q")`, `RegExp.prototype[Symbol.replace].call(re,s,function(){return "f"})`,
];
for (const v of ["0", "1", "4", "10"]) for (const f of reFlags) for (const style of ["define", "freeze"]) for (const s of ['"aXbX"', '""']) for (const op of frozenOps)
  add(`var re=new RegExp("a|b|",${f});re.lastIndex=${v};` +
    (style === "define" ? `Object.defineProperty(re,"lastIndex",{writable:false});` : `Object.freeze(re);`) +
    `var s=${s};var r=${op};return [r,re.lastIndex]`);
const accessorSetters = ['set(v){L.push("set"+v)}', 'set(v){throw new RangeError("setter")}', 'set(v){}'];
for (const v of vals.slice(0, 3).concat(['{0:"a",index:0}', '{0:"",index:0}'])) for (const f of ['"g"', '"gy"', '""', '"y"']) for (const set of accessorSetters) for (const op of plainOps.slice(0, 2).concat(plainOps.slice(4)))
  add(`var o={exec:E(${v}),flags:${f}};Object.defineProperty(o,"lastIndex",{get(){L.push("get");return 0},${set}});return ${op('"aXbX"')}`);

// ---- D. replaceAll/matchAll com regexp sem g.
const realFlags = ["", "i", "m", "s", "u", "y", "d", "v", "im", "ims", "iu", "imsuyd", "my", "dy", "sv", "g", "gi", "gy", "gd", "gv", "gimsuyd"];
const raOps = [
  `s.replaceAll(re,"x")`, `s.replaceAll(re,function(){return "f"})`, `I(s.matchAll(re))`, `s.replaceAll(re,"$&$&")`, `s.replaceAll(re,"$<n>")`,
];
for (const f of realFlags) for (const s of ['"aXbX"', '""', '"abab"']) for (const op of raOps) add(`var s=${s};var re=new RegExp("a|b",${q(f)});return ${op}`);
const likeFlags = ['"g"', '""', '"i"', "undefined", "null", "1", '{toString(){L.push("fts");return "g"}}', 'Symbol("f")', '"gg"', '"G"', '"yg"', "[]", '["g"]', "{toString(){throw new RangeError('fts')}}"];
const likeMatch = ["true", "false", "undefined", "0", '"x"', "{}", "null", "1"];
for (const f of likeFlags) for (const m of likeMatch) for (const via of ["value", "getter"]) for (const op of [`"aXbX".replaceAll(o,"x")`, `I("aXbX".matchAll(o))`, `"aXbX".replaceAll(o,function(){return "f"})`]) {
  const flagsDef = via === "value" ? `flags:${f}` : `get flags(){L.push("flags");return ${f}}`;
  add(`var o={[Symbol.match]:${m},${flagsDef},[Symbol.replace](){L.push("rep");return "RR"},[Symbol.matchAll](){L.push("ma");return [1,2][Symbol.iterator]()}};return ${op}`);
}
for (const f of likeFlags) for (const rf of ['""', '"g"']) for (const op of [`"aXbX".replaceAll(re,"x")`, `I("aXbX".matchAll(re))`])
  add(`var re=new RegExp("a",${rf});Object.defineProperty(re,"flags",{value:${f}});return ${op}`);
for (const m of likeMatch) for (const rf of ['""', '"g"', '"y"']) for (const op of [`"aXbX".replaceAll(re,"x")`, `"aXbX".match(re)`, `"aXbX".split(re)`])
  add(`var re=new RegExp("a",${rf});re[Symbol.match]=${m};return ${op}`);

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
  const known = new Set(knownPrograms("regexp_protocol_bun.tsv", (name) => /^regexp/.test(name) && name !== "regexp_protocol_bun.tsv"));
  const lone = /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;
  // Sem travessão literal e sem substituto isolado na fonte.
  const unique = [...programs].filter((p) => !usesHostApi(p) && !known.has(p) && !/[–—]/.test(p) && !lone.test(p) && !/\t/.test(p.slice(PRELUDE.length)));
  const results = new Array(unique.length);
  let next = 0;
  await Promise.all(Array.from({ length: 6 }, async () => {
    while (next < unique.length) {
      const i = next++;
      results[i] = await runChild(unique[i]);
    }
  }));
  let kept = 0;
  let dropped = 0;
  const rows = [];
  for (let i = 0; i < unique.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result) || /[–—\n\r\t]/.test(result) || result.length > 6000 || lone.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(unique[i].slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
      continue;
    }
    kept++;
    rows.push({ source: unique[i], result });
  }
  process.stdout.write(emitFactored("regexp_protocol", rows));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
})();
