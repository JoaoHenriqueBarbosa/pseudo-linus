// Gera tests/golden/iter_protocol_bun.tsv: protocolo de iteração observável nos built-ins consumidores, medido no bun.
// Consumidores: Array.from, new Map/Set/WeakMap/WeakSet, Promise.all/allSettled/any/race, Object.fromEntries,
// TypedArray.from, spread, desestruturação de array, for-of e yield*. Os iteráveis (fábrica MK) registram num log a ordem
// de todos os acessos (get Symbol.iterator, chamada, get next, next, done, value, get return, return, get throw, throw) e
// variam: next que lança, resultado não objeto (TypeError exato), getter de done/value que lança, done não booleano,
// return ausente/lançando/não objeto, Symbol.iterator devolvendo primitivo. A segunda parte sobrescreve
// %ArrayIteratorPrototype%.next (e Array.prototype[Symbol.iterator]) antes de consumir arrays e TypedArrays.
// Um bun filho novo por programa, sem APIs de host, resultado em globalThis.R. Array.fromAsync fica de fora.
// Uso: bun scripts/gen-iter-protocol-golden.js
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, sampleByHash, GOLDEN_DIR } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v)){var a=[];for(var i=0;i<v.length;i++)a.push(i in v?S(v[i],d+1):"<hole>");return "["+a.join(",")+"]"}' +
  'var k=Reflect.ownKeys(v),b=[];for(var j=0;j<k.length;j++)b.push(S(k[j])+":"+S(v[k[j]],d+1));return "{"+b.join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  "function MK(o){o=o||{};var L=[],calls=0,n=o.n===undefined?3:o.n;" +
  "function res(done,val,c){return {get done(){L.push('done');if(o.doneThrowAt===c)throw new Error('dx');return done},get value(){L.push('value');if(o.valThrowAt===c)throw new Error('vx');return val}}}\n" +
  "var nf=function(){L.push('next');var c=calls++;if(o.throwAt===c)throw new Error('nx');if(o.badAt===c)return o.badVal;" +
  "if(c>=n){if(c>=n+2)return res(true,'e',c);return res('doneVal' in o?o.doneVal:true,'e',c)}" +
  "return res(o.doneMid!==undefined&&c===1?o.doneMid:false,c,c)};\n" +
  "function meth(mode,name){return function(v){L.push(name+'('+(arguments.length?S(v):'')+')');if(mode==='throw')throw new Error(name+'x');if(mode==='nonobj')return 1;return {value:'R',done:true}}}\n" +
  "var it={get next(){L.push('get next');return 'nextVal' in o?o.nextVal:nf}};\n" +
  "Object.defineProperty(it,'return',{get(){L.push('get return');var m=o.ret||'ok';if(m==='getthrow')throw new Error('grx');return m==='none'?undefined:m==='null'?null:m==='notfn'?1:meth(m,'return')},configurable:true});\n" +
  "Object.defineProperty(it,'throw',{get(){L.push('get throw');var m=o.thr||'ok';if(m==='getthrow')throw new Error('gtx');return m==='none'?undefined:m==='null'?null:m==='notfn'?1:meth(m,'throw')},configurable:true});\n" +
  "var obj={get [Symbol.iterator](){L.push('get iter');var m=o.iter||'ok';if(m==='undef')return undefined;if(m==='null')return null;if(m==='notfn')return 1;if(m==='throwget')throw new Error('gx');" +
  "return function(){L.push('call iter'+arguments.length);if(m==='throwcall')throw new Error('cx');if(m==='ret')return o.iterRet;return it}}};\n" +
  "return {o:obj,L:L,it:it}}\n";

const exprs = [];
const add = (...list) => exprs.push(...list);

// Consumidores do iterável X.
const consumers = [
  "[...X]", "Array.from(X)", "Array.from(X,(v,i)=>v+':'+i)", "Array.from(X,function(){throw new Error('mapfn')})",
  "(()=>{var [a]=X;return a})()", "(()=>{var [a,b]=X;return [a,b]})()", "(()=>{var []=X;return 1})()", "(()=>{var [,]=X;return 1})()",
  "(()=>{var [...r]=X;return r})()", "(()=>{var [a,...r]=X;return [a,r]})()", "(()=>{var [a=9]=X;return a})()", "(()=>{var a,b;[a,b]=X;return [a,b]})()",
  "(()=>{var s=[];for(var v of X)s.push(v);return s})()", "(()=>{var s=[];for(var v of X){s.push(v);break}return s})()",
  "(()=>{var s=[];for(var v of X){s.push(v);throw new RangeError('body')}})()", "(()=>{for(var v of X){return v}})()",
  "[...(function*(){yield* X})()]", "Array.from((function*(){return yield* X})())", "new Map(X)", "new Set(X)", "new WeakMap(X)", "new WeakSet(X)",
  "Object.fromEntries(X)", "Promise.all(X).catch(()=>{})", "Promise.allSettled(X).catch(()=>{})", "Promise.race(X).catch(()=>{})", "Promise.any(X).catch(()=>{})",
  "Math.max(...X)", "((...a)=>a)(...X)", "new Uint8Array(X)", "Uint8Array.from(X)", "Float64Array.from(X,v=>v*2)", "Int16Array.from(X,function(){throw new Error('tmap')})",
  "Map.groupBy(X,v=>v%2)", "Object.groupBy(X,v=>v)", "[].concat(X)", "Iterator.from(X).toArray()", "new Array(...X)", "[].push(...X)", "Array.of(...X)",
  "new AggregateError(X)", "new Intl.ListFormat('en').format(X)", "new (class extends Array{})(...X)", "(()=>{var [[a]]=X;return a})()",
  "(()=>{var {0:a}=X;return a})()", "String.fromCodePoint(...X)", "Reflect.apply(Math.min,null,[...X])",
];

const badVals = ["1", "'s'", "null", "undefined", "true", "Symbol('q')", "1n", "0", "''", "function(){}"];
const behaviors = ["{}", "{n:0}", "{n:1}", "{n:5}", "{throwAt:0}", "{throwAt:1}", "{throwAt:3}"];
for (const at of [0, 1]) for (const v of badVals) behaviors.push(`{badAt:${at},badVal:${v}}`);
behaviors.push("{badAt:3,badVal:1}", "{badAt:3,badVal:null}", "{badAt:2,badVal:1}", "{badAt:2,badVal:undefined}");
for (const v of ["1", "undefined", "null", "{}", "'str'"]) behaviors.push(`{nextVal:${v}}`);
for (const at of [0, 2, 3]) behaviors.push(`{doneThrowAt:${at}}`);
for (const at of [0, 1]) behaviors.push(`{valThrowAt:${at}}`);
for (const v of ["0", "''", "null", "undefined", "1", "'x'", "{}"]) behaviors.push(`{doneVal:${v}}`);
for (const v of ["1", "'x'"]) behaviors.push(`{doneMid:${v}}`);
for (const m of ["none", "throw", "nonobj", "null", "notfn", "getthrow"]) behaviors.push(`{ret:'${m}'}`);
for (const m of ["undef", "null", "notfn", "throwget", "throwcall"]) behaviors.push(`{iter:'${m}'}`);
for (const v of ["1", "'s'", "null", "undefined", "true", "Symbol('q')", "1n", "{}", "function(){}", "[]"]) behaviors.push(`{iter:'ret',iterRet:${v}}`);
for (const m of ["none", "throw", "nonobj", "null", "notfn", "getthrow"]) {
  for (const base of ["throwAt:1", "doneThrowAt:1", "valThrowAt:1", "badAt:1,badVal:1"]) behaviors.push(`{ret:'${m}',${base}}`);
}

const wrap = (b, c) =>
  `T(()=>{var m=MK(${b}),X=m.o,r,v;try{v=${c}}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}if(r===undefined)r=S(v);return m.L.join()+" => "+r})`;
for (const b of behaviors) for (const c of consumers) add(wrap(b, c));

// yield* dirigido por passos (next/return/throw) sobre iteráveis com log.
const stepPlans = ["n", "nn", "nnn", "nnnn", "r", "nr", "nnr", "t", "nt", "nnt", "ntn", "nrn", "rn", "tn", "ntr"];
const yieldBehaviors = sampleByHash(behaviors, Math.ceil(behaviors.length / 3)).concat(["{thr:'none'}", "{thr:'throw'}", "{thr:'nonobj'}", "{thr:'null'}", "{thr:'notfn'}", "{thr:'getthrow'}", "{ret:'none',thr:'none'}"]);
for (const b of yieldBehaviors) for (const plan of stepPlans) {
  add(
    `T(()=>{var m=MK(${b}),out=[];var g=(function*(){var r=yield* m.o;out.push('ret='+S(r));return 'g'})();` +
      `var plan='${plan}';for(var i=0;i<plan.length;i++){var c=plan[i];try{var x=c==='n'?g.next(i):c==='r'?g.return('rv'+i):g.throw(new Error('t'+i));out.push(S(x))}catch(e){out.push('throw '+(e&&e.name)+': '+(e&&e.message))}}` +
      `return m.L.join()+' => '+out.join(' | ')})`,
  );
}

// %ArrayIteratorPrototype%.next e Array.prototype[Symbol.iterator] sobrescritos.
const overrides = {
  log: ["AIP.next=function(){L.push('next');return orig.call(this)}"],
  nonobj: ["AIP.next=function(){L.push('next');return 1}"],
  undef: ["AIP.next=function(){L.push('next');return undefined}"],
  doneNow: ["AIP.next=function(){L.push('next');return {done:true,value:9}}"],
  count: ["var k=0;AIP.next=function(){L.push('next');k++;return k>2?{done:true}:{done:false,value:'v'+k}}"],
  throws: ["AIP.next=function(){L.push('next');throw new RangeError('anx')}"],
  deleted: ["delete AIP.next"],
  notfn: ["AIP.next=1"],
  getter: ["Object.defineProperty(AIP,'next',{get(){L.push('get next');return orig},configurable:true})"],
  getterThrows: ["Object.defineProperty(AIP,'next',{get(){L.push('get next');throw new RangeError('gnx')},configurable:true})"],
  doneGetter: ["AIP.next=function(){L.push('next');var r=orig.call(this);return {get done(){L.push('done');return r.done},get value(){L.push('value');return r.value}}}"],
  addReturn: ["AIP.return=function(){L.push('return');return {}}"],
  returnNonObj: ["AIP.return=function(){L.push('return');return 1}"],
  returnThrows: ["AIP.return=function(){L.push('return');throw new RangeError('arx')}"],
  arrIterLog: ["Array.prototype[Symbol.iterator]=function(){L.push('arr iter');return av.call(this)}"],
  arrIterUndef: ["Array.prototype[Symbol.iterator]=undefined"],
  arrIterThrows: ["Array.prototype[Symbol.iterator]=function(){L.push('arr iter');throw new RangeError('aix')}"],
  arrIterPrim: ["Array.prototype[Symbol.iterator]=function(){L.push('arr iter');return 1}"],
  arrIterOwn: ["Array.prototype[Symbol.iterator]=function(){L.push('arr iter');return {next(){L.push('own next');return {done:true}}}}"],
};
const sources = ["[1,2,3]", "[[1,2],[3,4]]", "new Uint8Array([1,2,3])", "[1,2,3].values()"];
const aipConsumers = [
  "[...X]", "Array.from(X)", "Array.from(X,(v,i)=>v+':'+i)", "(()=>{var [a,b]=X;return [a,b]})()", "(()=>{var [a,...r]=X;return [a,r]})()",
  "(()=>{var s=[];for(var v of X)s.push(v);return s})()", "(()=>{var s=[];for(var v of X){s.push(v);break}return s})()", "[...(function*(){yield* X})()]",
  "new Map(X)", "new Set(X)", "Object.fromEntries(X)", "Promise.all(X).catch(()=>{})", "Promise.allSettled(X).catch(()=>{})", "Promise.any(X).catch(()=>{})",
  "Promise.race(X).catch(()=>{})", "Math.max(...X)", "Uint8Array.from(X)", "new Uint8Array(X)", "Map.groupBy(X,v=>v)", "new WeakSet(X)", "new WeakMap(X)",
  "(()=>{var [[a]]=X;return a})()",
];
for (const [name, install] of Object.entries(overrides)) for (const src of sources) for (const c of aipConsumers) {
  add(
    `T(()=>{var AIP=Object.getPrototypeOf([][Symbol.iterator]()),orig=AIP.next,av=Array.prototype[Symbol.iterator],L=[],X=${src},v,r;` +
      `${install[0]};try{v=${c}}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}finally{Object.defineProperty(AIP,'next',{value:orig,writable:true,configurable:true});delete AIP.return;Array.prototype[Symbol.iterator]=av}` +
      `if(r===undefined)r=S(v);return L.join()+" => "+r}) /*${name}*/`,
  );
}

// Dedup contra os goldens de iteração vizinhos.
const neighbours = knownPrograms("iter_protocol_bun.tsv", (file) => /iter|gener|collection|promise|array|typed|spread|destruct/i.test(file) && file !== "iter_protocol_bun.tsv");
const knownExprs = new Set(neighbours.map((program) => program.slice(program.lastIndexOf("globalThis.R = ") + 15)));
const seen = new Set();
let dup = 0;
let host = 0;
const jobs = [];
for (const expr of exprs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  if (knownExprs.has(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (usesHostApi(source)) { host++; continue; }
  jobs.push({ expr, source });
}

function runChild(source) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => { child.kill("SIGKILL"); reject(new Error("tempo esgotado")); }, 8000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (status) => { clearTimeout(timer); status === 0 ? resolve(out) : reject(new Error(err.slice(0, 200) || "filho falhou")); });
    child.stdin.end(source);
  });
}

async function main() {
  const results = new Array(jobs.length).fill(null);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const i = next++;
      try {
        results[i] = await runChild(jobs[i].source);
      } catch (e) {
        process.stderr.write("erro de programa: " + JSON.stringify(jobs[i].expr).slice(0, 120) + " " + e + "\n");
      }
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  const rows = [];
  let dropped = 0;
  for (let i = 0; i < jobs.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; continue; }
    rows.push({ source: jobs[i].source, result });
  }
  fs.writeFileSync(path.join(GOLDEN_DIR, "iter_protocol_bun.tsv"), emitFactored("iter_protocol", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos ${dup}, host ${host}\n`);
}
main();
