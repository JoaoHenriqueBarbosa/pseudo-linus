// Gera tests/golden/arguments_shape_bun.tsv: a forma do objeto `arguments` medida no bun 1.4.2 (um bun filho novo por
// programa, sem APIs de host, resultado em `globalThis.R`, TZ America/Sao_Paulo por padrão no filho).
// Cada programa cria uma função com `new Function`, em modo sloppy (arguments mapeado) ou estrito (arguments não mapeado,
// `callee` acessor que lança), com e sem captura do parâmetro por arrow (o que leva o JSC de DirectArguments a
// ScopedArguments), chamada com 0, 1 e 3 argumentos. Dentro dela roda:
//  - observações isoladas: Reflect.ownKeys, Object.keys, getOwnPropertyNames, getOwnPropertySymbols,
//    getOwnPropertyDescriptor de length/callee/Symbol.iterator/0, for-in, JSON.stringify, spread, Object.assign, values,
//    entries, toString e o estado completo;
//  - mutações de length, callee, Symbol.iterator, índice 0 e índice fora (5) por delete, atribuição, defineProperty de
//    valor e de acessor, mais a escrita no parâmetro, cada uma seguida de duas leituras (estado completo e
//    spread/JSON/for-in/arguments.length), para ver o aliasing do parâmetro e as chaves resultantes.
// Colunas: sufixo do programa (JSON), valor de `globalThis.R` (JSON) e o índice do prelúdio (scripts/golden-prelude.js).
// Programas já presentes nos goldens vizinhos (knownPrograms) são descartados.
// Uso: bun scripts/gen-arguments-shape-golden.js > tests/golden/arguments_shape_bun.tsv
const fs = require("fs");
const { emitFactored, knownPrograms } = require("./golden-prelude.js");
const { spawn } = require("child_process");
const { measureStable } = require("./golden-alternatives.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE = `function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}
function T(f){try{return f()}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}
function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";var p=[];if("value" in d)p.push("v="+S(d.value));if("get" in d)p.push("g="+typeof d.get+(d.get&&d.get===d.set?"=s":""));if("set" in d)p.push("s="+typeof d.set);if("writable" in d)p.push("w="+d.writable);p.push("e="+d.enumerable,"c="+d.configurable);return p.join(" ")}
function ST(g,a){var r=[];r.push("keys="+T(function(){return Reflect.ownKeys(g).map(String).join()}));r.push("len="+T(function(){return S(g.length)}));r.push("p0="+T(function(){return S(g[0])}));r.push("a="+S(a));["length","callee",Symbol.iterator,"0","5"].forEach(function(k){r.push(String(k)+":"+T(function(){return D(g,k)}))});return r.join(" | ")}
function A(strict,cap,n,body){var f=new Function("a","b","c",(strict?'"use strict";':'')+"var g=arguments,r=[];"+(cap?"var k=()=>a;":"")+body+";return r.join(' ; ')");var args=[1,"x",{}].slice(0,n);return T(function(){return f.apply(null,args)})}
`;

const KEYS = { length: '"length"', callee: '"callee"', iterator: "Symbol.iterator", idx0: "0", idx5: "5" };
const OBS = {
  ownKeys: "r.push(T(()=>Reflect.ownKeys(g).map(String).join()))",
  keys: "r.push(T(()=>Object.keys(g).join()))",
  names: "r.push(T(()=>Object.getOwnPropertyNames(g).join()))",
  syms: "r.push(T(()=>Object.getOwnPropertySymbols(g).map(String).join()))",
  gopdLength: 'r.push(T(()=>D(g,"length")))',
  gopdCallee: 'r.push(T(()=>D(g,"callee")))',
  gopdIter: "r.push(T(()=>D(g,Symbol.iterator)))",
  gopd0: 'r.push(T(()=>D(g,"0")))',
  forin: "r.push(T(function(){var q=[];for(var k in g)q.push(k);return q.join()}))",
  json: "r.push(T(()=>JSON.stringify(g)))",
  spread: "r.push(T(()=>S([...g])))",
  assign: "r.push(T(()=>S(Object.assign({},g))))",
  values: "r.push(T(()=>S(Object.values(g))))",
  entries: "r.push(T(()=>S(Object.entries(g))))",
  tag: "r.push(T(()=>Object.prototype.toString.call(g)+' '+typeof g.callee+' '+(g.length)+' '+(g[Symbol.iterator]===Array.prototype.values)))",
  state: "r.push(ST(g,a))",
};
const POST = {
  state: "r.push(ST(g,a))",
  views: "r.push(T(()=>S([...g])));r.push(T(()=>JSON.stringify(g)));r.push(T(function(){var q=[];for(var k in g)q.push(k);return q.join()}));r.push(T(()=>String(g.length)))",
};
function mutations() {
  const list = {};
  for (const [name, key] of Object.entries(KEYS)) {
    list["del_" + name] = `r.push(T(()=>String(delete g[${key}])))`;
    list["put_" + name] = `r.push(T(()=>{g[${key}]=7;return "ok"}))`;
    list["def_" + name] = `r.push(T(()=>{Object.defineProperty(g,${key},{value:7});return "ok"}))`;
    list["defacc_" + name] = `r.push(T(()=>{Object.defineProperty(g,${key},{get(){return 1},configurable:true});return "ok"}))`;
  }
  list.setParam = "a=9";
  list.delSetParam = "delete g[0];a=9";
  list.putThenSetParam = "g[0]=5;a=9";
  list.defRoThenSetParam = "r.push(T(()=>{Object.defineProperty(g,0,{value:3,writable:false});return 'ok'}));a=9";
  list.hideIdx0 = "r.push(T(()=>{Object.defineProperty(g,0,{enumerable:false});return 'ok'}))";
  list.lengthHide = 'r.push(T(()=>{Object.defineProperty(g,"length",{enumerable:true});return "ok"}))';
  return list;
}

const exprs = [];
const combos = [];
for (const strict of [false, true]) for (const cap of [false, true]) for (const n of [0, 1, 3]) combos.push([strict, cap, n]);
const call = (c, body) => `A(${c[0]},${c[1]},${c[2]},${JSON.stringify(body)})`;
for (const c of combos) for (const obs of Object.values(OBS)) exprs.push(call(c, obs));
for (const c of combos) for (const mut of Object.values(mutations())) for (const post of Object.values(POST)) exprs.push(call(c, mut + ";" + post));

const known = new Set(knownPrograms("arguments_shape_bun.tsv", (name) => name !== "arguments_shape_bun.tsv"));
const jobs = [];
const seen = new Set();
let dup = 0;
for (const expr of exprs) {
  const source = PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source) || known.has(source)) { dup++; continue; }
  seen.add(source);
  jobs.push({ expr, source });
}
const runChild = (job) => new Promise((resolve) => {
  // Processo fresco por programa: a ordem de reificação das tabelas estáticas do JSC depende do que rodou antes.
  const env = { ...process.env, TZ: process.env.GOLDEN_TZ || "America/Sao_Paulo" };
  const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env });
  let out = "";
  let err = "";
  child.stdout.on("data", (d) => (out += d));
  child.stderr.on("data", (d) => (err += d));
  const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
  child.on("close", (code) => { clearTimeout(timer); resolve({ ok: code === 0, out, err }); });
  child.stdin.end(job.source);
});
(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await measureStable(() => runChild(jobs[i]), 2);
    }
  };
  await Promise.all(Array.from({ length: 12 }, worker));
  let kept = 0;
  let dropped = 0;
  let varied = 0;
  const lines = [];
  const leak = /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i;
  jobs.forEach((job, i) => {
    const r = results[i];
    if (!r.ok) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + " " + r.err.slice(0, 100) + "\n"); return; }
    if (leak.test(r.out) || r.alternatives.some((alt) => leak.test(alt))) { dropped++; process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(job.expr).slice(0, 160) + "\n"); return; }
    kept++;
    if (r.alternatives.length) varied++;
    lines.push({ source: job.source, result: r.out, alternatives: r.alternatives });
  });
  process.stdout.write(emitFactored("arguments_shape", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos ${dup}, com alternativas ${varied}\n`);
})();
