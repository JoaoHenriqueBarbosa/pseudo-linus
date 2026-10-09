// Gera tests/golden/hostile_input_bun.tsv: entradas JS hostis aos builtins (comprimentos 2^32-1, 2^32, 2^53, NaN, -0,
// array-likes com length gigante em todos os métodos genéricos de Array.prototype, @@species hostis, Proxy com traps que
// lançam, toString/valueOf que lançam ou mutam o receptor, recursão infinita e profunda, RegExp com retrocesso pesado),
// medidas no bun 1.4.2. O objetivo é provar que nenhum caminho do porte entra em `panic!` onde o JSC lança um erro.
// Cada programa termina sozinho: o array-like gigante é um Proxy (`LIM`) que lança `RangeError: stop` na enésima trap, e os
// métodos que andariam 2^53 passos recebem `fromIndex`/`start` perto do fim. O resultado grava `ok:<valor>` ou
// `throw:<name>:<message>`, seguido do log das traps (`W`).
// Formato: `JSON(sufixo)<TAB>JSON(resultado)[<TAB>índice]`, prelúdio fatorado (ver golden-prelude.js). Um bun filho novo por
// programa (timeout de 15 s); o programa grava o texto em `globalThis.R`. Programa que excede o tempo é descartado e contado
// no stderr.
// Uso: GOLDEN_OUT_DIR=/tmp/hi bun scripts/gen-hostile-input-golden.js > /tmp/hi.tsv
const { emitFactored, knownPrograms, sampleByHash, GOLDEN_DIR, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  let source = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (d) => (source += d));
  process.stdin.on("end", () => {
    (0, eval)(source);
    process.exit(0);
  });
  return;
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:Math.min(v.length,20)},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+(v.length>20?",...":"")+"]";' +
  'return "{"+Reflect.ownKeys(v).slice(0,20).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'var LOG=[];\n' +
  'function T(f){try{return "ok:"+S(f())}catch(e){return "throw:"+(e&&e.name)+":"+(e&&e.message)}}\n' +
  'function W(f){LOG.length=0;var r=T(f);return r+"|"+LOG.join()}\n' +
  'function AL(len,p){var o={length:len};if(p)for(var k in p)o[k]=p[k];return o}\n' +
  'function D(o){return Reflect.ownKeys(o).map(function(k){return String(k)+"="+S(o[k])}).join(";")}\n' +
  'function LIM(len,n,o){o=o||{};var c=0;var tick=function(name,k){LOG.push(name+(k===undefined?"":":"+String(k)));if(++c>n)throw new RangeError("stop")};' +
  'return new Proxy({length:len},{get(t,k){tick("get",k);if(k===Symbol.isConcatSpreadable)return o.spread;if(k==="length")return len;return o.get?o.get(k):undefined},' +
  'has(t,k){tick("has",k);return !!o.has},set(t,k,v){tick("set",k);return true},defineProperty(t,k,d){tick("def",k);return true},' +
  'deleteProperty(t,k){tick("del",k);return true},getOwnPropertyDescriptor(t,k){tick("gopd",k);return undefined},ownKeys(){tick("keys");return []}})}\n' +
  'function NEST(n){var a=[];for(var i=0;i<n;i++)a=[a];return a}\n' +
  'function NESTO(n){var a={};for(var i=0;i<n;i++)a={a:a};return a}\n' +
  'function PX(trap,target){var h={};h[trap]=function(){throw new RangeError("trap "+trap)};return new Proxy(target,h)}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const AP = (m) => `Array.prototype.${m}`;

// ---- 1. Construtores e setters de comprimento.
const arrLens = ["2**32-1", "2**32", "2**53", "NaN", "-0", "-1", "'4294967296'", "{valueOf(){throw new RangeError('vo')}}", "1n"];
for (const L of arrLens) {
  add(
    `W(()=>{var a=new Array(${L});return a.length+'|'+Array.isArray(a)+'|'+(0 in a)})`,
    `W(()=>{var a=[];a.length=${L};return a.length})`,
    `W(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{value:${L}});return D(a)})`,
    `W(()=>{var a=[1,2,3];return Reflect.set(a,'length',${L})+'|'+a.length})`,
    `W(()=>{return Array.from({length:${L}}).length})`,
    `W(()=>{return new Float64Array(${L}).length})`,
  );
}
for (const L of ["NaN", "-0", "2**32", "2**53", "Infinity"]) {
  add(
    `W(()=>new ArrayBuffer(${L}).byteLength)`,
    `W(()=>new Uint8Array(${L}).length)`,
    `W(()=>new Int32Array(${L}).length)`,
    `W(()=>new Uint8Array(new ArrayBuffer(8)).subarray(${L}).length)`,
    `W(()=>new ArrayBuffer(8).slice(${L}).byteLength)`,
    `W(()=>new ArrayBuffer(8,{maxByteLength:${L}}).maxByteLength)`,
    `W(()=>'ab'.repeat(${L}).length)`,
    `W(()=>'abc'.padStart(${L},'xy').length)`,
    `W(()=>'abc'.padEnd(${L},'').length)`,
    `W(()=>[1,2,3,4].fill(0,${L},${L}).length+D([1,2,3,4].fill(9,${L}))+D([1,2,3,4].fill(8,0,${L})))`,
    `W(()=>D([1,2,3,4,5].copyWithin(${L},0,${L}))+D([1,2,3,4,5].copyWithin(0,${L}))+D([1,2,3,4,5].copyWithin(1,2,${L})))`,
    `W(()=>D([1,2,3,4,5].splice(${L}))+D([1,2,3,4,5].splice(0,${L}))+D([1,2,3,4,5].splice(1,${L},'x')))`,
    `W(()=>D(new Uint8Array([1,2,3,4]).fill(7,${L},${L}))+D(new Uint8Array([1,2,3,4]).copyWithin(${L},0,${L}))+D(new Uint8Array([1,2,3,4]).slice(${L})))`,
  );
}
add(
  "W(()=>'a'.repeat(2**32).length)",
  "W(()=>'a'.repeat(2**53))",
  "W(()=>''.repeat(2**32).length)",
  "W(()=>''.repeat(2**53).length)",
  "W(()=>''.repeat(Infinity))",
  "W(()=>'abc'.repeat(2**31).length)",
  "W(()=>'a'.repeat(-0)+'|'+'a'.repeat(NaN)+'|'+'a'.repeat('2.9'))",
  "W(()=>'a'.padStart(2**32,'b').length)",
  "W(()=>'a'.padEnd(2**53,'b').length)",
  "W(()=>'a'.padStart(2**32,'').length)",
  "W(()=>'a'.padStart(2**31,'bc').length)",
  "W(()=>'abc'.at(2**53)+'|'+'abc'.at(-0)+'|'+'abc'.at(NaN)+'|'+'abc'.at(-Infinity))",
  "W(()=>'abc'.slice(-0,NaN)+'|'+'abc'.substring(NaN,2**53)+'|'+'abc'.substr(2**32,2**53)+'|'+'abc'.substr(-0,Infinity))",
  "W(()=>'a,b,c'.split(',',2**32).length+'|'+'a,b,c'.split(',',2**32+1).length+'|'+'a,b,c'.split(',',-1).length+'|'+'a,b,c'.split(',',NaN).length)",
  "W(()=>'abc'.codePointAt(2**53)+'|'+'abc'.charAt(-0)+'|'+'abc'.charCodeAt(2**32)+'|'+'abc'.indexOf('',2**53)+'|'+'abc'.lastIndexOf('',NaN)+'|'+'abc'.includes('',2**53)+'|'+'abc'.startsWith('',2**53)+'|'+'abc'.endsWith('',2**53))",
  "W(()=>String.fromCharCode(2**32+65,-1,NaN,65.9)+'|'+String.fromCharCode(2**53+1).length)",
  "W(()=>String.fromCodePoint(2**32))",
  "W(()=>String.fromCodePoint(0x110000))",
  "W(()=>String.fromCodePoint(-0,1.5))",
  "W(()=>String.fromCodePoint(NaN))",
  "W(()=>String.raw({raw:LIM(2**53,6)}))",
  "W(()=>String.raw({raw:AL(2**32,{})}).length)",
  "W(()=>String.raw({raw:'abc'},1,2,3,4))",
  "W(()=>String.raw({raw:{length:-1}}))",
  "W(()=>String.raw({raw:{length:NaN}}))",
  "W(()=>String.raw({raw:{length:2,0:'a',1:'b'}},{toString(){throw new RangeError('sub')}}))",
);

// ---- 2. Métodos genéricos de Array.prototype em array-like gigante (Proxy que lança na 8ª trap).
const GM = {
  at: "0", concat: "[]", copyWithin: "0,1", every: "x=>true", fill: "1", filter: "x=>true", find: "x=>false", findIndex: "x=>false",
  findLast: "x=>false", findLastIndex: "x=>false", flat: "", flatMap: "x=>x", forEach: "x=>0", includes: "1", indexOf: "1", join: "",
  lastIndexOf: "1", map: "x=>x", pop: "", push: "1", reduce: "(a,b)=>a", reduceRight: "(a,b)=>a", reverse: "", shift: "", slice: "",
  some: "x=>false", sort: "", splice: "0,1", toLocaleString: "", toReversed: "", toSorted: "", toSpliced: "0,1", toString: "",
  unshift: "1", with: "0,1",
};
const ITER = ["entries", "keys", "values"];
const call = (m, recv, args) => `${AP(m)}.call(${recv}${args ? "," + args : ""})`;
const limOpts = (m) => (m === "concat" ? "{spread:true}" : "{}");
for (const L of ["2**32", "2**53"]) {
  for (const [m, args] of Object.entries(GM)) {
    if (L === "2**53" && !["concat", "fill", "indexOf", "join", "map", "reverse", "shift", "sort", "splice", "unshift", "flat", "reduceRight"].includes(m)) continue;
    add(`W(()=>S(${call(m, `LIM(${L},8,${limOpts(m)})`, args)}))`);
  }
  for (const m of ITER) add(`W(()=>S(${AP(m)}.call(LIM(${L},8)).next()))`);
}
for (const m of ["at", "every", "fill", "forEach", "includes", "indexOf", "join", "lastIndexOf", "map", "pop", "reduce", "slice", "some", "toString"]) {
  add(`W(()=>S(${call(m, "LIM(2**32-1,8)", GM[m])}))`);
}
for (const L of ["NaN", "-0"]) {
  for (const m of ["fill", "forEach", "join", "pop", "push", "reverse", "shift", "slice", "splice", "sort"]) {
    add(`W(()=>S(${call(m, `LIM(${L},8)`, GM[m])}))`);
  }
}
add(
  `W(()=>S([].concat(LIM(2**53-1,8,{spread:true}),1)))`,
  `W(()=>S([1].concat(LIM(2**53-1,8,{spread:true}))))`,
  `W(()=>S([].concat(LIM(2**32,8,{spread:true}))))`,
  `W(()=>S([].concat(LIM(2**32-1,8,{spread:true}))))`,
  `W(()=>S([1,2].concat(LIM(2**32-2,8,{spread:true}))))`,
  `W(()=>{var o=AL(2**53-1);o[Symbol.isConcatSpreadable]=true;return S([1].concat(o))})`,
  `W(()=>{var o=AL(2**53-1);o[Symbol.isConcatSpreadable]=true;return S([].concat(o,o))})`,
  `W(()=>{var a=[];a.length=2**32-1;return S(a.concat([1]))})`,
  `W(()=>{var a=[];a.length=2**32-1;return S(a.concat(a))})`,
  `W(()=>{var a=[];a.length=2**32-1;return S(a.push(1))+'|'+a.length})`,
  `W(()=>{var a=[];a.length=2**32-1;return S(a.push())+'|'+a.length})`,
  `W(()=>{var a=[];a.length=2**32-1;a[2**32-2]='z';return S(a.pop())+'|'+a.length})`,
  `W(()=>{var a=[];a.length=2**32-1;a[2**32-2]='z';return S(a.slice(-2))+'|'+S(a.at(-1))+'|'+a.indexOf('z',-2)+'|'+a.lastIndexOf('z')+'|'+a.includes('z',-3)})`,
  `W(()=>{var a=[];a.length=2**32-1;a[2**32-2]='z';return S(a.splice(-1,1))+'|'+a.length})`,
  `W(()=>{var a=[];a.length=2**32-1;return S(a.splice(2**32-2,0,'x'))+'|'+a.length})`,
  `W(()=>{var a=[];a.length=2**32-1;return S(a.splice(2**32-2,0,'x','y'))+'|'+a.length})`,
  `W(()=>{var a=[];a.length=2**32-1;a.fill('f',2**32-3);return S(a.slice(-3))})`,
  `W(()=>{var a=[];a.length=2**32-1;a[0]='h';a.copyWithin(2**32-3,0,1);return S(a.slice(-3))})`,
  `W(()=>{var a=[];a.length=2**32-1;a[2**32-2]='z';return S(a.with(-1,'w').length)})`,
  `W(()=>{var a=[];a.length=2**32;return a.length})`,
  `W(()=>{var a=[1];a[2**32-2]='z';return a.length+'|'+Object.keys(a)})`,
  `W(()=>{var a=[1];a[2**32-1]='z';return a.length+'|'+Object.keys(a)})`,
  `W(()=>{var a=[1];a[2**32]='z';return a.length+'|'+Object.keys(a)})`,
  `W(()=>{var a=[1,2,3];a[2**32-2]='z';a.length=1;return a.length+'|'+Object.keys(a)})`,
  `W(()=>{var a=[1,2,3];a[2**32-2]='z';return a.indexOf('z')+'|'+a.lastIndexOf('z')+'|'+a.includes('z')+'|'+a.findLast(x=>x==='z')+'|'+a.at(-1)})`,
  `W(()=>{var a=[1,2,3];a[2**32-2]='z';return a.reverse().length})`,
  `W(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});return T(()=>a.push(4))+'|'+T(()=>a.pop())+'|'+T(()=>a.shift())+'|'+T(()=>a.unshift(0))+'|'+T(()=>a.splice(0,1))+'|'+a.length})`,
  `W(()=>{var a=[1,2,3];Object.freeze(a);return T(()=>a.push(4))+'|'+T(()=>a.pop())+'|'+T(()=>a.reverse())+'|'+T(()=>a.sort())+'|'+T(()=>a.fill(0))+'|'+T(()=>a.copyWithin(0,1))+'|'+T(()=>a.length=0)})`,
);

// ---- 3. Perto do fim de um comprimento 2^53-1: termina porque só toca os últimos elementos.
const M = "2**53-1";
const E = (n) => `2**53-${n}`;
const tail = (o) => `{[${E(2)}]:'e',[${E(3)}]:'d'${o ? "," + o : ""}}`;
add(
  `W(()=>{var o=AL(${M});${AP("fill")}.call(o,7,${E(3)});return D(o)})`,
  `W(()=>{var o=AL(${M});${AP("fill")}.call(o,7,-2);return D(o)})`,
  `W(()=>{var o=AL(${M});${AP("fill")}.call(o,7,${E(4)},-1);return D(o)})`,
  `W(()=>{var o=AL(${M},{0:'a',1:'b'});${AP("copyWithin")}.call(o,${E(3)},0,2);return D(o)})`,
  `W(()=>{var o=AL(${M},{[${E(3)}]:'y',[${E(2)}]:'z',0:'q'});${AP("copyWithin")}.call(o,0,${E(3)});return D(o)})`,
  `W(()=>{var o=AL(${M},{[${E(3)}]:'y',[${E(2)}]:'z',0:'q'});${AP("copyWithin")}.call(o,-1,-3);return D(o)})`,
  `W(()=>${AP("indexOf")}.call(AL(${M},${tail()}),'e',${E(3)}))`,
  `W(()=>${AP("indexOf")}.call(AL(${M},${tail()}),'e',-2))`,
  `W(()=>${AP("includes")}.call(AL(${M},${tail()}),'e',${E(3)}))`,
  `W(()=>${AP("includes")}.call(AL(${M},${tail()}),'x',-1))`,
  `W(()=>${AP("includes")}.call(AL(${M},${tail()}),undefined,${E(1)}))`,
  `W(()=>${AP("lastIndexOf")}.call(AL(${M},${tail()}),'e'))`,
  `W(()=>${AP("lastIndexOf")}.call(AL(${M},${tail()}),'d',-2))`,
  `W(()=>${AP("lastIndexOf")}.call(AL(${M},${tail()}),'d',${E(1)}))`,
  `W(()=>${AP("at")}.call(AL(${M},${tail()}),-1))`,
  `W(()=>${AP("at")}.call(AL(${M},${tail()}),2**53))`,
  `W(()=>${AP("at")}.call(AL(${M},${tail()}),-(2**53)))`,
  `W(()=>${AP("with")}.call(AL(${M}),0,1))`,
  `W(()=>${AP("toSorted")}.call(AL(${M})))`,
  `W(()=>${AP("toSpliced")}.call(AL(${M}),0,${M}))`,
  `W(()=>${AP("slice")}.call(AL(${M},${tail()}),${E(3)}))`,
  `W(()=>${AP("slice")}.call(AL(${M},${tail()}),-2,-1))`,
  `W(()=>${AP("slice")}.call(AL(${M},${tail()}),${E(5)},2**53+10).length)`,
  `W(()=>${AP("slice")}.call(AL(${M},${tail()}),2**53))`,
  `W(()=>${AP("splice")}.call(AL(${M},${tail("length:" + M)}),${E(3)},1))`,
  `W(()=>{var o=AL(${M},${tail()});var r=${AP("splice")}.call(o,${E(3)},1);return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL(${E(2)});var r=${AP("splice")}.call(o,${E(2)},0,'x');return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL(${M});var r=${AP("splice")}.call(o,${E(2)},0,'x');return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL(${M});var r=${AP("splice")}.call(o,${E(2)},2**53);return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL(${E(2)});var r=${AP("push")}.call(o,'a');return r+'|'+D(o)})`,
  `W(()=>{var o=AL(${E(2)});var r=${AP("push")}.call(o,'a','b');return r+'|'+D(o)})`,
  `W(()=>{var o=AL(${M});var r=${AP("push")}.call(o,'a');return r+'|'+D(o)})`,
  `W(()=>{var o=AL(${M});var r=${AP("push")}.call(o);return r+'|'+D(o)})`,
  `W(()=>{var o=AL(2**32-1);var r=${AP("push")}.call(o,'a');return r+'|'+D(o)})`,
  `W(()=>{var o=AL(2**32);var r=${AP("push")}.call(o,'a','b');return r+'|'+D(o)})`,
  `W(()=>{var o=AL(${M},${tail()});var r=${AP("pop")}.call(o);return r+'|'+D(o)})`,
  `W(()=>{var o=AL(${M});var r=${AP("unshift")}.call(o);return r+'|'+D(o)})`,
  `W(()=>{var o=AL(${M});var r=${AP("unshift")}.call(o,1);return r+'|'+D(o)})`,
  `W(()=>{var o=AL(0);var r=${AP("unshift")}.call(o,1,2,3);return r+'|'+D(o)})`,
  `W(()=>{var o=AL(-5);var r=${AP("push")}.call(o,1);return r+'|'+D(o)})`,
  `W(()=>{var o=AL(NaN);var r=${AP("pop")}.call(o);return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL(-0);var r=${AP("shift")}.call(o);return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL(1.9,{0:'a',1:'b'});var r=${AP("pop")}.call(o);return S(r)+'|'+D(o)})`,
  `W(()=>{var o=AL('2',{0:'a',1:'b'});return ${AP("join")}.call(o,'-')})`,
  `W(()=>${AP("reduce")}.call(AL(${M},${tail()}),(a,b,i)=>{throw new RangeError('stop'+i)},0))`,
  `W(()=>${AP("reduceRight")}.call(AL(${M},${tail()}),(a,b,i)=>{throw new RangeError('stop'+i)}))`,
  `W(()=>${AP("findLast")}.call(AL(${M},${tail()}),(v,i)=>i<${E(3)}))`,
  `W(()=>${AP("findLastIndex")}.call(AL(${M},${tail()}),(v,i)=>v==='d'))`,
  `W(()=>${AP("every")}.call(AL(${M}),()=>{throw new RangeError('ev')}))`,
  `W(()=>${AP("some")}.call(AL(${M},{0:1}),(v,i)=>v===1))`,
  `W(()=>${AP("find")}.call(AL(${M},{5:'f'}),(v)=>v==='f'))`,
  `W(()=>{var n=0;${AP("forEach")}.call(AL(${M},{5:1,6:2}),(v,i)=>{if(++n>1)throw new RangeError('fe'+i)})})`,
  `W(()=>{var n=0;${AP("map")}.call(AL(2**32,{5:1}),(v,i)=>{n++;return v});return n})`,
  `W(()=>${AP("filter")}.call(AL(2**32,{3:1,4:2}),(v)=>true).length)`,
  `W(()=>${AP("flatMap")}.call(LIM(2**32,5,{has:true,get:()=>1}),(v)=>v).length)`,
  `W(()=>${AP("flat")}.call(LIM(2**32,5,{has:true,get:()=>[1]})).length)`,
  `W(()=>{return ${AP("sort")}.call(LIM(${M},8,{has:true,get:(k)=>k==='0'?'b':'a'}),(a,b)=>{throw new RangeError('cmp')})})`,
  `W(()=>{var o=AL(3,{0:'b',2:'a'});${AP("sort")}.call(o);return D(o)})`,
  `W(()=>{var o=AL(3,{0:'b',2:'a'});${AP("reverse")}.call(o);return D(o)})`,
  `W(()=>{var o=AL(2**32,{0:'b'});o[2**32-1]='z';return ${AP("lastIndexOf")}.call(o,'z')})`,
  `W(()=>${AP("join")}.call(AL(3,{0:null,1:undefined,2:1}),undefined))`,
  `W(()=>${AP("toString")}.call({join:()=>'j'}))`,
  `W(()=>${AP("toString")}.call({join:1}))`,
  `W(()=>${AP("toString")}.call(1)+${AP("toString")}.call(null))`,
  `W(()=>${AP("toLocaleString")}.call(AL(2,{0:{toLocaleString(){throw new RangeError('tl')}},1:2})))`,
  `W(()=>${AP("toLocaleString")}.call(AL(2,{0:1,1:{toLocaleString(){return {}}}})))`,
  `W(()=>Array.from(LIM(2**32-1,6)).length)`,
  `W(()=>Array.from(LIM(2**53,6)).length)`,
  `W(()=>Array.from.call(function(n){LOG.push('n='+S(n));return {}},LIM(2**53,6)).length)`,
  `W(()=>Array.from.call(function(n){LOG.push('n='+S(n));return {}},LIM(5,10)).length)`,
  `W(()=>Array.from.call(function(){return Object.freeze({})},[1,2]).length)`,
  `W(()=>Array.from.call(function(){return new Proxy({},{set(){throw new RangeError('s')}})},[1,2]).length)`,
  `W(()=>Array.from.call(function(){return new Proxy({},{defineProperty(){throw new RangeError('dp')}})},{length:1,0:1}).length)`,
  `W(()=>Array.from((function*(){for(var i=0;;i++)yield i})(),(x,i)=>{if(i>3)throw new RangeError('stop');return x}))`,
  `W(()=>Array.from({length:2**32}))`,
  `W(()=>Array.from({length:2**53}))`,
  `W(()=>Array.from({length:Infinity}))`,
  `W(()=>Array.from({length:-5}).length+'|'+Array.from({length:NaN}).length+'|'+Array.from({length:'2'}).length)`,
  `W(()=>Array.from.call(Object,{length:2,0:'a',1:'b'}))`,
  `W(()=>Array.of.call(function(n){LOG.push('n='+S(n));return {}},1,2))`,
  `W(()=>Array.of.call(function(){return Object.freeze({})},1))`,
  `W(()=>Array.of.call(1,1,2))`,
  `W(()=>Array.of.call(function(){return new Proxy({},{set(){throw new RangeError('s')}})},1,2))`,
  `W(()=>Array.of(...[1,2,3]).length)`,
  `W(()=>Math.max.apply(null,{length:2**32}))`,
  `W(()=>Math.max.apply(null,{length:2**53}))`,
  `W(()=>Math.max.apply(null,{length:Infinity}))`,
  `W(()=>Math.max.apply(null,{length:70000}))`,
  `W(()=>Math.max.apply(null,{length:NaN,0:5}))`,
  `W(()=>Math.max.apply(null,{length:-0,0:5}))`,
  `W(()=>Math.max.apply(null,{length:2.9,0:5,1:7,2:100}))`,
  `W(()=>Math.max.apply(null,LIM(3,3)))`,
  `W(()=>Reflect.apply(Math.max,null,{length:2**32}))`,
  `W(()=>Reflect.construct(Array,{length:2**53}))`,
  `W(()=>Reflect.construct(Array,{length:2}).length)`,
  `W(()=>String.fromCharCode.apply(null,{length:2**32}))`,
  `W(()=>Function.prototype.bind.apply(function(){},{length:2**32}))`,
  `W(()=>new Function('a','return a')===undefined)`,
);

// ---- 4. Typed arrays e ArrayBuffer com argumentos extremos.
for (const v of ["2**32", "NaN", "-0", "{valueOf(){throw new RangeError('vo')}}"]) {
  add(
    `W(()=>D(new Uint8Array([1,2,3,4]).subarray(${v},${v})))`,
    `W(()=>D(new Uint8Array([1,2,3,4]).subarray(0,${v})))`,
    `W(()=>D(new Uint8Array([1,2,3,4]).slice(${v})))`,
    `W(()=>new Uint8Array([1,2,3,4]).indexOf(4,${v})+'|'+new Uint8Array([1,2,3,4]).lastIndexOf(4,${v})+'|'+new Uint8Array([1,2,3,4]).includes(4,${v}))`,
    `W(()=>{var u=new Uint8Array(4);u.set([1,2],${v});return D(u)})`,
    `W(()=>{var u=new Uint8Array(4);u.set({length:2,0:1,1:2},${v});return D(u)})`,
    `W(()=>new Uint8Array([1,2,3,4]).at(${v}))`,
    `W(()=>D(new Uint8Array([1,2,3,4]).with(${v},9)))`,
    `W(()=>D(new Uint8Array([1,2,3,4]).toSorted()) + new Uint8Array([1,2,3,4]).join('-'.repeat(1)))`,
    `W(()=>{var b=new ArrayBuffer(4,{maxByteLength:8});b.resize(${v});return b.byteLength})`,
    `W(()=>new ArrayBuffer(4).transfer(${v}).byteLength)`,
    `W(()=>new DataView(new ArrayBuffer(4),${v}).byteLength)`,
    `W(()=>new DataView(new ArrayBuffer(4),0,${v}).byteLength)`,
    `W(()=>new DataView(new ArrayBuffer(4)).getUint8(${v}))`,
    `W(()=>new DataView(new ArrayBuffer(4)).setUint32(${v},1))`,
    `W(()=>new Uint16Array(new ArrayBuffer(8),${v}).length)`,
    `W(()=>new Uint16Array(new ArrayBuffer(8),0,${v}).length)`,
  );
}
add(
  "W(()=>Uint8Array.from({length:2**53}).length)",
  "W(()=>Uint8Array.from({length:2**33}).length)",
  "W(()=>Uint8Array.from(LIM(2**53,6)).length)",
  "W(()=>Uint8Array.of.call(function(n){return {}},1))",
  "W(()=>new Uint8Array({length:2**53}).length)",
  "W(()=>new Uint8Array(LIM(2**53,6)).length)",
  "W(()=>new Uint8Array(LIM(3,20)).length)",
  "W(()=>new Uint8Array({length:3,0:{valueOf(){throw new RangeError('vo')}}}).length)",
  "W(()=>{var u=new Uint8Array(4);u.set(LIM(2**53,6));return u.length})",
  "W(()=>{var u=new Uint8Array(4);u.set({length:2**32});return u.length})",
  "W(()=>{var u=new Uint8Array(4);u.set(new Uint8Array(5));return u.length})",
  "W(()=>{var u=new Uint8Array(4);u.set({length:1,0:{valueOf(){u.buffer.transfer();return 1}}});return D(u)})",
  "W(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);u.set({length:2,0:{valueOf(){b.resize(0);return 1}},1:2});return D(u)+b.byteLength})",
  "W(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);return u.fill({valueOf(){b.resize(2);return 1}},0,8).length+'|'+D(u)})",
  "W(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);return D(u.slice({valueOf(){b.resize(2);return 1}}))})",
  "W(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b);return u.indexOf(0,{valueOf(){b.resize(0);return 0}})})",
  "W(()=>{var b=new ArrayBuffer(8);var u=new Uint8Array(b);u.sort((x,y)=>{b.transfer();return 0});return u.length})",
  "W(()=>{var b=new ArrayBuffer(8);var u=new Uint8Array(b);return D(u.map((x,i)=>{if(i==0)b.transfer();return 1}))})",
  "W(()=>{var b=new ArrayBuffer(8);var u=new Uint8Array(b);var n=0;u.forEach(()=>{if(n++==0)b.transfer()});return n})",
  "W(()=>{var b=new ArrayBuffer(8);var u=new Uint8Array(b);b.transfer();return u.length+'|'+u.byteLength+'|'+u.byteOffset+'|'+T(()=>u.fill(1))+'|'+T(()=>u.at(0))+'|'+T(()=>u.join())})",
  "W(()=>{var b=new ArrayBuffer(8,{maxByteLength:16});var u=new Uint8Array(b,4);b.resize(2);return u.length+'|'+u.byteLength+'|'+u.byteOffset+'|'+T(()=>u.fill(1))})",
  "W(()=>new ArrayBuffer(8,{maxByteLength:4}))",
  "W(()=>new ArrayBuffer(8,{maxByteLength:2**53}))",
  "W(()=>new ArrayBuffer(2**53-1))",
  "W(()=>new ArrayBuffer(2**33).byteLength)",
  "W(()=>ArrayBuffer.prototype.slice.call(new ArrayBuffer(4),NaN,-0).byteLength)",
  "W(()=>ArrayBuffer.isView(new Proxy(new Uint8Array(2),{})))",
  "W(()=>new SharedArrayBuffer(2**53))",
  "W(()=>new Float64Array(2**33).length)",
  "W(()=>new BigInt64Array(2**32).length)",
);

// ---- 5. @@species e constructor hostis em concat/slice/map/filter/splice.
const speciesList = [
  "function(n){LOG.push('n='+S(n));return {}}",
  "function(n){return Object.freeze({})}",
  "function(n){return new Proxy({},{defineProperty(){throw new RangeError('dp')}})}",
  "function(n){var o={};Object.defineProperty(o,'0',{value:9,configurable:false,writable:true});return o}",
  "function(n){var o={};Object.defineProperty(o,'length',{set(v){throw new RangeError('len')},get(){return 0}});return o}",
  "undefined", "null", "1", "()=>[]", "function(n){throw new RangeError('sp')}", "function(n){return new Uint8Array(1)}",
  "function(n){return new Array(2**32-1)}", "function(n){return 5}", "Object", "Array.bind(null)",
  "function(n){return new Proxy([],{set(){throw new RangeError('set')}})}",
  "function(n){return {get length(){throw new RangeError('gl')},set length(v){LOG.push('sl='+v)}}}",
];
const speciesMethods = [["concat", "[4],[5]"], ["slice", "0,2"], ["map", "x=>x*2"], ["splice", "0,2"]];
for (const [m, args] of speciesMethods) {
  for (const sp of speciesList.filter((_, i) => i % 3 !== 1)) {
    add(`W(()=>{var a=[1,2,3];a.constructor={[Symbol.species]:${sp}};var r=a.${m}(${args});return S(r)+'|'+Array.isArray(r)+'|'+D(a)})`);
  }
}
for (const ctor of ["1", "null", "{[Symbol.species]:1}", "()=>1", "Array.bind()", "new Proxy(Array,{})"]) {
  for (const m of ["concat", "slice", "map"]) {
    add(`W(()=>{var a=[1,2,3];a.constructor=${ctor};var r=a.${m}(${m === "map" ? "x=>x" : "0,1"});return S(r)+'|'+Array.isArray(r)})`);
  }
}
add(
  `W(()=>{var a=[1,2,3];Object.defineProperty(a,'constructor',{get(){throw new RangeError('ctor')}});return S(a.slice())})`,
  `W(()=>{var a=[1,2,3];Object.defineProperty(a,'constructor',{get(){throw new RangeError('ctor')}});return S(a.concat())})`,
  `W(()=>{var a=[1,2,3];Object.defineProperty(a,'constructor',{get(){throw new RangeError('ctor')}});return S(a.flat())+S(a.flatMap(x=>x))+S(a.toSorted())})`,
  `W(()=>{var a=[1,2,3];a.constructor={get [Symbol.species](){throw new RangeError('sp')}};return S(a.map(x=>x))})`,
  `W(()=>{var a=[1,2,3];a.constructor={get [Symbol.species](){a.length=0;a.push(9,9);return Array}};return S(a.map(x=>x))+D(a)})`,
  `W(()=>{var a=[1,2,3];a.constructor={get [Symbol.species](){a.length=2**20;return Array}};return a.slice(0,2).length})`,
  `W(()=>{var a=[1,2,3,4];a.constructor={get [Symbol.species](){a.length=1;return Array}};return S(a.splice(1,3))+D(a)})`,
  `W(()=>{class A extends Array{static get [Symbol.species](){return function(n){LOG.push('n='+n);return new Array(n)}}};var a=A.from([1,2,3]);return S(a.map(x=>x))+S(a.filter(x=>x>1))+S(a.slice(1))+S(a.concat([9]))+S(a.splice(0,1))})`,
  `W(()=>{class A extends Array{static get [Symbol.species](){return Object}};var a=A.from([1,2,3]);return S(a.map(x=>x))})`,
  `W(()=>{class A extends Array{constructor(n){super(n);if(n===3)throw new RangeError('three')}};return S(A.from([1,2,3]))+S(new A(3))})`,
  `W(()=>{class A extends Array{constructor(){super();Object.defineProperty(this,'length',{writable:false})}};var a=new A();return S(a.push(1))})`,
  `W(()=>{class A extends Array{constructor(){super();Object.freeze(this)}};return S(A.of(1))})`,
  `W(()=>{var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return new Proxy([],{get(t,k){LOG.push('get:'+String(k));return t[k]},defineProperty(t,k,d){LOG.push('def:'+String(k));return Reflect.defineProperty(t,k,d)},set(t,k,v){LOG.push('set:'+String(k));t[k]=v;return true}})}};return S(a.splice(1,1,'x','y'))+D(a)})`,
  `W(()=>{var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return Object.defineProperty([],'length',{writable:false})}};return S(a.slice(0,2))})`,
  `W(()=>{var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return Object.defineProperty([],'length',{writable:false})}};return S(a.map(x=>x))})`,
  `W(()=>{var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return Object.defineProperty([],'length',{writable:false})}};return S(a.splice(0,0))})`,
  `W(()=>{var a=[1,2,3];a.constructor={[Symbol.species]:function(n){return Object.defineProperty([],'length',{writable:false})}};return S(a.filter(x=>false))})`,
);

// ---- 6. Proxy com traps que lançam, como receptor e como argumento.
const traps = ["get", "has", "set", "defineProperty", "deleteProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf", "isExtensible"];
const proxyOps = [
  ["forEach", "x=>0"], ["push", "9"], ["pop", ""], ["reverse", ""], ["join", ""], ["slice", ""], ["splice", "0,1"], ["indexOf", "2"],
  ["includes", "2"], ["sort", ""], ["fill", "0"], ["concat", "[1]"], ["map", "x=>x"], ["shift", ""], ["unshift", "0"], ["flat", ""],
  ["at", "0"], ["copyWithin", "0,1"], ["toString", ""], ["lastIndexOf", "2"],
];
for (const trap of traps.slice(0, 5)) {
  for (const [m, args] of proxyOps.filter((_, i) => i % 2 === 0)) add(`W(()=>S(${call(m, `PX('${trap}',[1,2,3])`, args)}))`);
}
add(
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>Array.isArray(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>[].concat(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>JSON.stringify(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>JSON.stringify({a:r.proxy}))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>Array.prototype.join.call(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>Object.prototype.toString.call(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>Array.from(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>[...r.proxy])})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});r.revoke();return T(()=>Object.keys(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable(function(){},{});r.revoke();return T(()=>typeof r.proxy)+T(()=>r.proxy())+T(()=>Function.prototype.toString.call(r.proxy))})`,
  `W(()=>{var r=Proxy.revocable([1,2],{});var x=[0].concat(r.proxy);r.revoke();return S(x)})`,
  `W(()=>JSON.stringify(PX('ownKeys',{a:1})))`,
  `W(()=>JSON.stringify(PX('get',{a:1})))`,
  `W(()=>JSON.stringify(PX('getOwnPropertyDescriptor',{a:1})))`,
  `W(()=>JSON.stringify({x:PX('get',[1])}))`,
  `W(()=>JSON.stringify(PX('get',[1])))`,
  `W(()=>JSON.stringify(PX('ownKeys',[1])))`,
  `W(()=>Array.isArray(PX('get',[1])))`,
  `W(()=>Array.isArray(PX('getPrototypeOf',[1])))`,
  `W(()=>[].concat(PX('get',[1,2])))`,
  `W(()=>[].concat(PX('has',[1,2])))`,
  `W(()=>[].concat(PX('getPrototypeOf',[1,2])))`,
  `W(()=>Array.from(PX('get',[1,2])))`,
  `W(()=>Array.from(PX('has',[1,2])))`,
  `W(()=>[...PX('get',[1,2])])`,
  `W(()=>Object.keys(PX('ownKeys',{a:1})))`,
  `W(()=>Object.keys(PX('getOwnPropertyDescriptor',{a:1})))`,
  `W(()=>Object.assign({},PX('ownKeys',{a:1})))`,
  `W(()=>Object.assign({},PX('get',{a:1})))`,
  `W(()=>Object.entries(PX('get',{a:1})))`,
  `W(()=>Object.getOwnPropertyNames(PX('ownKeys',{a:1})))`,
  `W(()=>Object.freeze(PX('defineProperty',{a:1})))`,
  `W(()=>Object.freeze(PX('ownKeys',{a:1})))`,
  `W(()=>Object.isFrozen(PX('isExtensible',{a:1})))`,
  `W(()=>new Proxy({},{ownKeys(){return [1]}}) && Object.keys(new Proxy({},{ownKeys(){return [1]}})))`,
  `W(()=>Object.keys(new Proxy({},{ownKeys(){return ['a','a']}})))`,
  `W(()=>Object.keys(new Proxy({},{ownKeys(){return 5}})))`,
  `W(()=>Object.keys(new Proxy({},{ownKeys(){return {length:2**32}}})))`,
  `W(()=>Object.keys(new Proxy({},{ownKeys(){return {length:2**53}}})))`,
  `W(()=>Object.keys(new Proxy({},{ownKeys(){return LIM(2**32,10)}})))`,
  `W(()=>Reflect.ownKeys(new Proxy({},{ownKeys(){return {length:3,0:'a',1:Symbol.iterator,2:'a'}}})))`,
  `W(()=>Reflect.ownKeys(new Proxy({},{ownKeys(){return {length:1,0:{}}}})))`,
  `W(()=>Reflect.ownKeys(new Proxy({},{ownKeys(){return {length:-1}}})))`,
  `W(()=>Reflect.ownKeys(new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return []}})))`,
  `W(()=>Reflect.ownKeys(new Proxy(Object.freeze({a:1}),{ownKeys(){return ['a','b']}})))`,
  `W(()=>Reflect.getPrototypeOf(new Proxy({},{getPrototypeOf(){return 1}})))`,
  `W(()=>Reflect.get(new Proxy(Object.freeze({a:1}),{get(){return 2}}),'a'))`,
  `W(()=>Reflect.set(new Proxy(Object.freeze({a:1}),{set(){return true}}),'a',2))`,
  `W(()=>Reflect.has(new Proxy(Object.freeze({a:1}),{has(){return false}}),'a'))`,
  `W(()=>Reflect.deleteProperty(new Proxy(Object.freeze({a:1}),{deleteProperty(){return true}}),'a'))`,
  `W(()=>Reflect.defineProperty(new Proxy({},{defineProperty(){return true}}),'a',{value:1,configurable:false}))`,
  `W(()=>Reflect.getOwnPropertyDescriptor(new Proxy({},{getOwnPropertyDescriptor(){return {value:1,configurable:false}}}),'a'))`,
  `W(()=>Reflect.getOwnPropertyDescriptor(new Proxy({},{getOwnPropertyDescriptor(){return 1}}),'a'))`,
  `W(()=>Reflect.isExtensible(new Proxy({},{isExtensible(){return false}})))`,
  `W(()=>Reflect.preventExtensions(new Proxy({},{preventExtensions(){return true}})))`,
  `W(()=>Reflect.setPrototypeOf(new Proxy(Object.preventExtensions({}),{setPrototypeOf(){return true}}),Array.prototype))`,
);

// ---- 7. toString/valueOf que lançam ou mutam o receptor.
const THROWER = "{toString(){throw new RangeError('ts')},valueOf(){throw new RangeError('vo')}}";
add(
  `W(()=>[1,${THROWER},3].join())`,
  `W(()=>[1,2,3].join(${THROWER}))`,
  `W(()=>[1,2,3].join({toString(){return 7}}))`,
  `W(()=>[1,2,3].join({toString(){return {}},valueOf(){return {}}}))`,
  `W(()=>[1,2,3].join(Symbol()))`,
  `W(()=>[1,Symbol()].join())`,
  `W(()=>[1,2,3].toString.call({join(){throw new RangeError('j')}}))`,
  `W(()=>{var a=[1,2,3];return a.join({toString(){a.length=0;return ','}})})`,
  `W(()=>{var a=[1,2,3];return a.join({toString(){a.length=2**16;return ','}}).length})`,
  `W(()=>{var a=[1,2,3];return a.join({toString(){a.push(4,5,6);return '-'}})})`,
  `W(()=>{var a=[1,{toString(){a.length=0;return 'x'}},3];return a.join()})`,
  `W(()=>{var a=[1,{toString(){a.length=0;return 'x'}},3];return a.toString()})`,
  `W(()=>{var a=[1,{toString(){a.length=0;return 'x'}},3];return a.toLocaleString()})`,
  `W(()=>{var a=[{toLocaleString(){a.length=0;return 'x'}},2,3];return a.toLocaleString()})`,
  `W(()=>{var a=[1,2,3];a.push(a);return a.join()+'|'+a.toString()+'|'+a.toLocaleString()})`,
  `W(()=>{var a=[1,2,3];a.push({toString(){return a.join()}});return a.join()})`,
  `W(()=>{var a=[1,2,3];return a.indexOf(3,{valueOf(){a.length=0;return 0}})})`,
  `W(()=>{var a=[1,2,3];return a.includes(undefined,{valueOf(){a.length=0;return 0}})})`,
  `W(()=>{var a=[1,2,3];return a.lastIndexOf(3,{valueOf(){a.length=0;return 2}})})`,
  `W(()=>{var a=[1,2,3];return D(a.slice({valueOf(){a.length=0;return 0}}))})`,
  `W(()=>{var a=[1,2,3];return D(a.slice(0,{valueOf(){a.length=1;return 3}}))})`,
  `W(()=>{var a=[1,2,3];return D(a.splice({valueOf(){a.length=0;return 1}},{valueOf(){a.push(7,8,9);return 2}}))+D(a)})`,
  `W(()=>{var a=[1,2,3];return D(a.fill(0,{valueOf(){a.length=1;return 0}}))})`,
  `W(()=>{var a=[1,2,3];return D(a.fill({valueOf(){a.length=0;return 0}},0,3))})`,
  `W(()=>{var a=[1,2,3,4,5];return D(a.copyWithin(0,{valueOf(){a.length=2;return 3}}))})`,
  `W(()=>{var a=[1,2,3,4,5];return D(a.copyWithin({valueOf(){a.length=2;return 0}},3,5))})`,
  `W(()=>{var a=[1,2,3];return S(a.at({valueOf(){a.length=0;return 1}}))})`,
  `W(()=>{var a=[1,2,3];return S(a.with({valueOf(){a.length=0;return 1}},9))})`,
  `W(()=>{var a=[1,2,3];return S(a.toSpliced({valueOf(){a.length=0;return 1}},1,9))})`,
  `W(()=>{var a=[1,2,3];return S(a.flat({valueOf(){a.length=0;return 1}}))})`,
  `W(()=>{var a=[1,2,3];return S(a.sort((x,y)=>{a.length=0;return x-y}))+D(a)})`,
  `W(()=>{var a=[3,2,1];return S(a.sort((x,y)=>{a.push(9,9);return x-y}))+D(a)})`,
  `W(()=>{var a=[3,2,1];try{a.sort((x,y)=>{throw new RangeError('cmp')})}catch(e){}return D(a)})`,
  `W(()=>{var a=[3,2,1];return S(a.sort((x,y)=>{a.length=2**20;return x-y}).length)})`,
  `W(()=>{var a=[3,2,1];return S(a.sort((x,y)=>NaN))+S(a.sort((x,y)=>({valueOf(){throw new RangeError('cv')}})))})`,
  `W(()=>{var a=[3,2,1];return S(a.sort((x,y)=>Symbol()))})`,
  `W(()=>{var a=[3,2,1];return S(a.sort(null))})`,
  `W(()=>{var a=[3,2,1];return S(a.sort({}))})`,
  `W(()=>{var a=[3,2,1];return S(a.toSorted(1))})`,
  `W(()=>[${THROWER},1].sort())`,
  `W(()=>[1,${THROWER}].sort())`,
  `W(()=>[${THROWER}].sort())`,
  `W(()=>{var a=[1,2,3];var n=0;return S(a.map((x,i)=>{if(i==0)a.push(9,9,9);n++;return x}))+n})`,
  `W(()=>{var a=[1,2,3];var n=0;a.forEach((x,i)=>{if(i==0)a.length=1;n++});return n})`,
  `W(()=>{var a=[1,2,3];var n=0;a.forEach((x,i)=>{if(i==0)delete a[1];n++});return n})`,
  `W(()=>{var a=[1,2,3];return S(a.reduce((p,x,i)=>{a.length=0;return p+x}))})`,
  `W(()=>{var a=[1,2,3];return S(a.reduceRight((p,x,i)=>{a.length=0;return p+x}))})`,
  `W(()=>{var a=[1,2,3];return S(a.filter((x,i)=>{a.length=0;return true}))})`,
  `W(()=>{var a=[1,2,3];return S(a.flatMap((x,i)=>{a.length=0;return [x,x]}))})`,
  `W(()=>{var a=[1,2,3];return S(a.find((x,i)=>{a.length=0;return false}))+S(a.findLast((x,i)=>{a.push(1);return false}))})`,
  `W(()=>{var a=[1,2,3];return S(a.every((x,i)=>{a.length=0;return true}))+S(a.some((x,i)=>{a.length=0;return false}))})`,
  `W(()=>{var a=[1,2,3];return S(Array.from(a,(x,i)=>{a.length=0;return x}))})`,
  `W(()=>{var a=[1,2,3];return S([...a.entries()].length)+S(Array.from(a.keys(),(x)=>{if(a.length<6)a.push(9);return x}).length)})`,
  `W(()=>{var a=[1,2,3];var it=a.values();a.length=0;return S(it.next())})`,
  `W(()=>{var a=[1,2,3];var it=a.values();it.next();a.length=0;a.push(7);return S(it.next())+S(it.next())})`,
  `W(()=>{var a=[1,2];var it=a.values();it.next();it.next();it.next();a.push(7);return S(it.next())})`,
  `W(()=>{var a=[1,2];var it=a.entries();a.length=2**32-1;it.next();return S(it.next())})`,
  `W(()=>{var a=[1,2];var it=a.keys();a.length=2**32-1;return S(it.next())})`,
  `W(()=>'x'.padStart(${THROWER}))`,
  `W(()=>'x'.padStart(4,${THROWER}))`,
  `W(()=>'x'.repeat(${THROWER}))`,
  `W(()=>'x'.repeat({valueOf(){return 3}}))`,
  `W(()=>'abc'.slice(${THROWER}))`,
  `W(()=>'abc'.split(',',${THROWER}))`,
  `W(()=>'abc'.split(${THROWER}))`,
  `W(()=>'abc'.concat(1,${THROWER}))`,
  `W(()=>'abc'.localeCompare(${THROWER}))`,
  `W(()=>'abc'.replace('b',${THROWER}))`,
  `W(()=>'abc'.replace(${THROWER},'x'))`,
  `W(()=>'abc'.replace(/b/,()=>(${THROWER})))`,
  `W(()=>'abc'.includes(${THROWER}))`,
  `W(()=>'abc'.at(${THROWER}))`,
  `W(()=>'abc'.normalize(${THROWER}))`,
  `W(()=>'abc'.normalize('X'))`,
  `W(()=>String.prototype.padStart.call(${THROWER},4))`,
  `W(()=>String.prototype.repeat.call(null,4))`,
  `W(()=>Array.prototype.fill.call(${THROWER},1))`,
  `W(()=>Array.prototype.fill.call(null,1))`,
  `W(()=>Array.prototype.fill.call(undefined,1))`,
  `W(()=>Array.prototype.fill.call(1,1))`,
  `W(()=>Array.prototype.fill.call('ab',1))`,
  `W(()=>Array.prototype.indexOf.call('ab','b'))`,
  `W(()=>Array.prototype.slice.call('ab'))`,
  `W(()=>Array.prototype.push.call('ab',1))`,
  `W(()=>Array.prototype.pop.call('ab'))`,
  `W(()=>Array.prototype.sort.call('ba'))`,
  `W(()=>Array.prototype.reverse.call(Symbol()))`,
  `W(()=>Array.prototype.concat.call(${THROWER},1))`,
  `W(()=>[].concat({[Symbol.isConcatSpreadable]:true,length:${THROWER}}))`,
  `W(()=>[].concat({[Symbol.isConcatSpreadable]:true,length:2,0:1,get 1(){throw new RangeError('g1')}}))`,
  `W(()=>[].concat({get [Symbol.isConcatSpreadable](){throw new RangeError('sp')}}))`,
  `W(()=>[].concat({[Symbol.isConcatSpreadable]:true,length:'3',2:'x'}))`,
  `W(()=>{var a=[1,2,3];a[Symbol.isConcatSpreadable]=false;return S([0].concat(a))})`,
  `W(()=>{var a=[1,2,3];a.length=2**32-1;a[Symbol.isConcatSpreadable]=false;return S([0].concat(a).length)})`,
  `W(()=>Number.prototype.toFixed.call(1,${THROWER}))`,
  `W(()=>(1).toString({valueOf(){return 2**32}}))`,
  `W(()=>(1).toString(NaN))`,
  `W(()=>(1).toFixed(2**53))`,
  `W(()=>(1).toFixed(-0)+(1).toPrecision(NaN))`,
  `W(()=>(1).toPrecision(2**32))`,
  `W(()=>(1).toExponential(2**53))`,
  `W(()=>(1n).toString(2**32))`,
  `W(()=>BigInt.asUintN(2**53,1n)+'|'+BigInt.asUintN(2**53+2,1n))`,
  `W(()=>BigInt.asUintN(2**53-1,-1n))`,
  `W(()=>BigInt.asIntN(2**32,1n))`,
  `W(()=>BigInt.asUintN(-1,1n))`,
  `W(()=>BigInt.asUintN(NaN,1n)+'|'+BigInt.asUintN(-0,1n))`,
  `W(()=>1n<<(2n**64n))`,
  `W(()=>1n<<(2n**40n))`,
  `W(()=>1n>>(2n**64n))`,
  `W(()=>2n**(2n**40n))`,
  `W(()=>(2n**64n)**(2n**64n))`,
  `W(()=>0n**(2n**64n))`,
  `W(()=>(-1n)**(2n**64n+1n))`,
  `W(()=>2n**-1n)`,
  `W(()=>1n/0n)`,
  `W(()=>BigInt(1e300).toString().length)`,
  `W(()=>BigInt(Infinity))`,
  `W(()=>BigInt('1'.repeat(1000)).toString().length)`,
);

// ---- 8. Recursão profunda e infinita.
add(
  `W(()=>JSON.stringify(NEST(1e6)).length)`,
  `W(()=>JSON.stringify(NEST(1e6),null,2).length)`,
  `W(()=>JSON.stringify(NESTO(1e6)).length)`,
  `W(()=>JSON.stringify(NESTO(1e6),null,'\\t').length)`,
  `W(()=>JSON.stringify(NEST(100)).length)`,
  `W(()=>JSON.stringify(NESTO(100),null,1).length)`,
  `W(()=>JSON.stringify(NEST(1e6),(k,v)=>v).length)`,
  `W(()=>JSON.stringify(NEST(1e6),['a']).length)`,
  `W(()=>JSON.stringify({toJSON(){return NEST(1e6)}}).length)`,
  `W(()=>{var o={};o.o=o;return JSON.stringify(o)})`,
  `W(()=>{var a=[];a.push(a);return JSON.stringify(a)})`,
  `W(()=>{var o={toJSON(){return o}};return JSON.stringify(o)})`,
  `W(()=>{var o={toJSON(){return JSON.stringify(o)}};return JSON.stringify(o)})`,
  `W(()=>{var o={a:{}};return JSON.stringify(o,function(k,v){return {x:v}})})`,
  `W(()=>JSON.stringify({a:1},function f(k,v){return JSON.stringify({b:1},f)}))`,
  `W(()=>Array.isArray(JSON.parse('['.repeat(1e6)+']'.repeat(1e6))))`,
  `W(()=>JSON.parse('{"a":'.repeat(1e5)+'1'+'}'.repeat(1e5)).a.a===undefined)`,
  `W(()=>JSON.parse('['.repeat(1e6)))`,
  `W(()=>JSON.parse('['.repeat(1e6)+']'.repeat(1e6-1)))`,
  `W(()=>JSON.parse('['.repeat(1e5)+']'.repeat(1e5),(k,v)=>v).length)`,
  `W(()=>JSON.parse('['.repeat(1e5)+']'.repeat(1e5),function(k,v){return 1}))`,
  `W(()=>JSON.parse('['.repeat(10)+']'.repeat(10),function(k,v){throw new RangeError('rv'+k)}))`,
  `W(()=>String(NEST(1e6)).length)`,
  `W(()=>NEST(1e6).join().length)`,
  `W(()=>NEST(1e6).toString().length)`,
  `W(()=>NEST(1e6).toLocaleString().length)`,
  `W(()=>NEST(1e6).flat(Infinity).length)`,
  `W(()=>NEST(1000).flat(Infinity).length)`,
  `W(()=>NEST(1e6).flat(2**53).length)`,
  `W(()=>NEST(50).flat(NaN).length+'|'+NEST(50).flat(-1).length+'|'+NEST(50).flat(2**32).length)`,
  `W(()=>NEST(1e6).flatMap(x=>x).length)`,
  `W(()=>NEST(1000).join().length+'|'+NEST(1000).toString().length)`,
  `W(()=>[].concat(NEST(1e6)).length)`,
  `W(()=>NEST(1e6)+'')`,
  `W(()=>\`\${NEST(1e6)}\`)`,
  `W(()=>Array.isArray(NEST(1e6)))`,
  `W(()=>Object.prototype.toString.call(NEST(1e6)))`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return Array.isArray(p)})`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return p.x})`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return 'x' in p})`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return Object.keys(p)})`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return JSON.stringify(p)})`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return Object.getPrototypeOf(p)===Object.prototype})`,
  `W(()=>{var p={};for(var i=0;i<1e5;i++)p=new Proxy(p,{});p.x=1;return p.x})`,
  `W(()=>{var p=function(){};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return Function.prototype.toString.call(p)})`,
  `W(()=>{var p=function(){return 1};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return p()})`,
  `W(()=>{var p=function(){return 1};for(var i=0;i<1e5;i++)p=new Proxy(p,{});return new p()})`,
  `W(()=>{var f=function(){return 1};for(var i=0;i<3000;i++)f=f.bind(null);return f()})`,
  `W(()=>{var f=function(){return 1};for(var i=0;i<3000;i++)f=f.bind(null);return f.length+'|'+(typeof Function.prototype.toString.call(f))})`,
  `W(()=>{var o={};o.toString=function(){return String(o)};return String(o)})`,
  `W(()=>{var o={};o.toString=function(){return ''+o};return o+''})`,
  `W(()=>{var o={};o.valueOf=function(){return +o};return +o})`,
  `W(()=>{var o={};o[Symbol.toPrimitive]=function(){return o+1};return o+1})`,
  `W(()=>{var a=[];a.join=function(){return a.toString()};return a.toString()})`,
  `W(()=>{var a=[1];a[0]={toString(){return a.join()}};return a.join()})`,
  `W(()=>{var a=[1];a[0]={toLocaleString(){return a.toLocaleString()}};return a.toLocaleString()})`,
  `W(()=>{var f=function(){return f.toString()};return f()})`,
  `W(()=>{var f=function(){return 1+f.call()};return f()})`,
  `W(()=>{var f=function(){return 1+Function.prototype.call.call(f)};return f()})`,
  `W(()=>{var f=function(){return 1+Reflect.apply(f,null,[])};return f()})`,
  `W(()=>{var f=function(){return 1+f.apply(null,arguments)};return f()})`,
  `W(()=>{var f=function(){return new f()};return new f()})`,
  `W(()=>{var f=function(){return Reflect.construct(f,[])};return f()})`,
  `W(()=>{var f=function(){return [1].map(f)};return f()})`,
  `W(()=>{var f=function(){return [1,2].sort(f)};return f()})`,
  `W(()=>{var f=function(){return Array.from([1],f)};return f()})`,
  `W(()=>{var f=function(){return JSON.stringify({a:1},f)};return f()})`,
  `W(()=>{var f=function(){return JSON.parse('1',f)};return f()})`,
  `W(()=>{var f=function(){return 'a'.replace(/a/,f)};return f()})`,
  `W(()=>{var f=function(){return Promise.resolve().then(f)};f();return 1})`,
  `W(()=>{var f=function(){return [1].reduce(f)};return [1,2].reduce(f)})`,
  `W(()=>{var p=new Proxy({},{get(t,k,r){return r[k]}});return p.x})`,
  `W(()=>{var p=new Proxy({},{has(t,k){return k in p}});return 'x' in p})`,
  `W(()=>{var p=new Proxy({},{ownKeys(t){return Object.keys(p)}});return Object.keys(p)})`,
  `W(()=>{var p=new Proxy([],{get(t,k,r){return Array.prototype.join.call(r)}});return p.join()})`,
  `W(()=>{var p=new Proxy(function(){},{apply(t,th,a){return p()}});return p()})`,
  `W(()=>{var p=new Proxy(function(){},{construct(t,a){return new p()}});return new p()})`,
  `W(()=>{var o={get x(){return o.x}};return o.x})`,
  `W(()=>{var o={set x(v){o.x=v}};o.x=1})`,
  `W(()=>{class A{static get [Symbol.species](){return A.x}static get x(){return A[Symbol.species]}};return A[Symbol.species]})`,
  `W(()=>{var a=[];a.constructor={get [Symbol.species](){return a.map(x=>x).constructor[Symbol.species]}};return a.map(x=>x)})`,
  `W(()=>{var o={};Object.defineProperty(o,'__proto__',{get(){return o.__proto__}});return o.__proto__})`,
  `W(()=>{function f(){f()};return f()})`,
  `W(()=>{function f(n){return n===0?0:1+f(n-1)};return f(500)})`,
  `W(()=>{function f(n){return n===0?0:1+f(n-1)};return f(1e7)})`,
  `W(()=>{var s='';for(var i=0;i<2e3;i++)s+='[';return T(()=>eval(s)).slice(0,60)})`,
  `W(()=>{var s='(';return T(()=>new Function('return '+'('.repeat(1e5)+'1'+')'.repeat(1e5))())})`,
  `W(()=>T(()=>new Function('return '+'['.repeat(1e3)+']'.repeat(1e3))()).slice(0,60))`,
  `W(()=>T(()=>new Function('return '+'-'.repeat(1e5)+'1')()).slice(0,60))`,
  `W(()=>T(()=>new Function('return '+'1+'.repeat(1e5)+'1')()))`,
  `W(()=>T(()=>new Function('return '+'a?'.repeat(1e4)+'1'+':2'.repeat(1e4))))`,
  `W(()=>T(()=>new Function('{'.repeat(1e5)+'}'.repeat(1e5))).slice(0,60))`,
  `W(()=>T(()=>new Function('if(1)'.repeat(1e5)+';')).slice(0,60))`,
  `W(()=>T(()=>new Function('return '+'x=>'.repeat(1e5)+'1')).slice(0,60))`,
  `W(()=>T(()=>new Function('return \\x60'+'$\\x7b\\x60'.repeat(1e4)+'\\x60}'.repeat(1e4)+'\\x60')).slice(0,60))`,
  `W(()=>T(()=>new RegExp('('.repeat(1e5)+')'.repeat(1e5))).slice(0,80))`,
  `W(()=>T(()=>new RegExp('(?:'.repeat(1e4)+'a'+')'.repeat(1e4)).test('a')))`,
  `W(()=>T(()=>new RegExp('(?:'.repeat(1e6)+'a'+')'.repeat(1e6)).test('a')).slice(0,80))`,
  `W(()=>T(()=>new RegExp('[\\\\s'.repeat(1e4)+']'.repeat(1)).test('a')))`,
  `W(()=>T(()=>new RegExp('a'.repeat(1e6)).test('a'.repeat(1e6))))`,
  `W(()=>T(()=>new RegExp('(?=a)'.repeat(1e4)).test('a')))`,
  `W(()=>T(()=>new RegExp('(?<=a)'.repeat(1e4)).test('a')))`,
  `W(()=>T(()=>new RegExp('(?<!a)(?!a)'.repeat(1e3)).test('b')))`,
);

// ---- 9. RegExp: retrocesso pesado (termina no bun em até alguns segundos), quantificadores gigantes, limites.
for (const n of [12, 26]) {
  add(
    `W(()=>/(a+)+b/.test('a'.repeat(${n})))`,
    `W(()=>/(a|aa)+$/.test('a'.repeat(${n})+'!'))`,
    `W(()=>/(x+x+)+y/.test('x'.repeat(${n})))`,
    `W(()=>/(?:a?){${n}}a{${n}}/.test('a'.repeat(${n - 1})))`,
    `W(()=>/^(\\w+\\s?)*$/.test('aaaa '.repeat(${n >> 2})+'!'))`,
    `W(()=>'a'.repeat(${n}).replace(/(a*)*b/g,'x').length)`,
    `W(()=>'a'.repeat(${n}).split(/(a+)+b/).length)`,
    `W(()=>'a'.repeat(${n}).match(/(a*)*c/))`,
    `W(()=>'a'.repeat(${n}).search(/(a+)+$\\n/))`,
    `W(()=>[...('a'.repeat(${n})+'!').matchAll(/(a+)+!/g)].length)`,
    `W(()=>/(?<n>a+)+\\k<n>b/.test('a'.repeat(${n})))`,
    `W(()=>/(a*)*\\1c/.test('a'.repeat(${n})))`,
    `W(()=>/(?:(?:a|b)*)*c/u.test('ab'.repeat(${n >> 1})))`,
  );
}
add(
  `W(()=>/a{4294967295}/.test('a'))`,
  `W(()=>/a{4294967296}/.test('a'))`,
  `W(()=>/a{0,4294967295}b/.test('a'.repeat(100)))`,
  `W(()=>/a{2147483647}/.test('a'))`,
  `W(()=>/a{2147483648,}/.test('a'))`,
  `W(()=>/(?:a{65536}){65536}/.test('a'))`,
  `W(()=>/(?:a{1000}){1000}/.test('a'))`,
  `W(()=>/(?:a{1000}){1000}b/.test('a'.repeat(1000)))`,
  `W(()=>/(?:a{100000}){100000}/.test('a'))`,
  `W(()=>new RegExp('a{99999999999999999999}').test('a'))`,
  `W(()=>new RegExp('a{99999999999999999999,}').test('a'))`,
  `W(()=>new RegExp('a{2,1}'))`,
  `W(()=>new RegExp('(a)'.repeat(1e5)))`,
  `W(()=>new RegExp('(a)'.repeat(70000)))`,
  `W(()=>new RegExp('(a)'.repeat(1000)).exec('a'.repeat(1000)).length)`,
  `W(()=>new RegExp('\\\\'+'9'.repeat(30)).test('a'))`,
  `W(()=>new RegExp('\\\\k'+'<a>'.repeat(3)))`,
  `W(()=>new RegExp('(?<a>x)'.repeat(2)))`,
  `W(()=>new RegExp('[a-'+'\\\\u{10ffff}'+']','u').test('a'))`,
  `W(()=>new RegExp('\\\\u{110000}','u'))`,
  `W(()=>new RegExp('[\\\\u{0}-\\\\u{10ffff}]','u').test('a'))`,
  `W(()=>'a'.repeat(1e6).replace(/a/g,'bb').length)`,
  `W(()=>'a'.repeat(1e5).replace(/a*?$/,'x').length)`,
  `W(()=>'a'.repeat(1e5).match(/(?:a)*/)[0].length)`,
  `W(()=>'a'.repeat(1e6).match(/(a|b)*/)[0].length)`,
  `W(()=>'a'.repeat(1e5).match(/(?:a|b)*c/))`,
  `W(()=>'ab'.repeat(1e5).replace(/(?:a|b)+?c/g,'').length)`,
  `W(()=>/(?:)/g[Symbol.replace]('abc','-'))`,
  `W(()=>{var r=/a/g;r.lastIndex=2**32;return r.test('a')+'|'+r.lastIndex})`,
  `W(()=>{var r=/a/g;r.lastIndex=2**53;return r.test('a')+'|'+r.lastIndex})`,
  `W(()=>{var r=/a/y;r.lastIndex=-1;return r.test('a')+'|'+r.lastIndex})`,
  `W(()=>{var r=/a/g;r.lastIndex={valueOf(){throw new RangeError('li')}};return r.test('a')})`,
  `W(()=>{var r=/a/g;Object.defineProperty(r,'lastIndex',{writable:false});return T(()=>r.test('a'))+T(()=>r.test('b'))})`,
  `W(()=>{var r=/a/;r.exec=function(){throw new RangeError('ex')};return r.test('a')})`,
  `W(()=>{var r=/a/;r.exec=function(){return 1};return r.test('a')})`,
  `W(()=>{var r=/a/g;r.exec=function(){return {index:0,length:2**53,0:'a'}};return 'a'.replace(r,'x')})`,
  `W(()=>{var r=/a/g;r.exec=function(){return {index:2**53,length:1,0:'a'}};return 'a'.replace(r,'x')})`,
  `W(()=>{var r=/a/g;var n=0;r.exec=function(){return n++<3?{index:0,length:1,0:''}:null};return 'abc'.replace(r,'x')})`,
  `W(()=>{var r=/a/g;r.exec=function(){return {index:0,length:LIM(2**32,5)}};return 'a'.replace(r,'x')})`,
  `W(()=>/a/[Symbol.split].call({constructor:{[Symbol.species]:function(){return {exec(){return null},flags:'',lastIndex:0}}},flags:'',lastIndex:0},'abc',2**32))`,
  `W(()=>/a/[Symbol.split].call(/a/,'banana',2**32+2).length)`,
  `W(()=>/a/[Symbol.split].call(/a/,'banana',-1).length)`,
  `W(()=>/a/[Symbol.split].call(/a/,'banana',NaN).length)`,
  `W(()=>'banana'.split(/a/,2**32-1).length)`,
  `W(()=>'banana'.split(/(?:)/u).length)`,
  `W(()=>/a/[Symbol.matchAll].call({constructor:function(){return {exec(){return null},lastIndex:0}},flags:'g'},'a'))`,
  `W(()=>/a/[Symbol.replace].call({exec(){return null},flags:'g',lastIndex:0},'abc','x'))`,
  `W(()=>/a/[Symbol.replace].call({exec(){return null},flags:{toString(){throw new RangeError('fl')}},lastIndex:0},'abc','x'))`,
  `W(()=>RegExp.prototype.flags)`,
  `W(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,'global').get.call({}))`,
  `W(()=>RegExp.prototype.test.call({exec:()=>null},'a'))`,
  `W(()=>RegExp.prototype.toString.call({source:{toString(){throw new RangeError('s')}},flags:'g'}))`,
);

// ---- Execução.
const known = new Set();
for (const name of require("fs").readdirSync(GOLDEN_DIR).filter((n) => n.endsWith(".tsv") && n !== "hostile_input_bun.tsv")) {
  try {
    for (const source of knownPrograms("hostile_input_bun.tsv", [name])) known.add(source);
  } catch (e) {
    // Golden vizinho fora do formato de programa: não participa da deduplicação.
  }
}
const seen = new Set();
const jobs = [];
let dup = 0;
// A grade completa passa de 900 programas; a amostra por hash do texto mantém a proporção de cada família e não se desloca
// quando entra ou sai um programa.
const TARGET = 450;
for (const expr of sampleByHash(exprs, TARGET)) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source) || known.has(source) || usesHostApi(source.slice(PRELUDE.length))) { dup++; continue; }
  seen.add(source);
  jobs.push({ expr, source });
}

const TIMEOUT_MS = 15000;
let timedOut = 0;
function runChild(source) {
  return new Promise((resolve, reject) => {
    // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    let killed = false;
    const timer = setTimeout(() => { killed = true; child.kill("SIGKILL"); }, TIMEOUT_MS);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      clearTimeout(timer);
      if (killed) { timedOut++; return reject(new Error("timeout")); }
      const decoded = code === 0 ? decodeResult(out) : null;
      decoded !== null ? resolve(decoded) : reject(new Error(err.slice(0, 200) || "filho falhou"));
    });
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      try { results[i] = await runChild(jobs[i].source); } catch (e) { results[i] = null; process.stderr.write("erro de programa: " + JSON.stringify(jobs[i].expr).slice(0, 160) + " " + e + "\n"); }
    }
  });
  await Promise.all(workers);
  const rows = [];
  let dropped = 0;
  for (let i = 0; i < jobs.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; continue; }
    rows.push({ source: jobs[i].source, result });
  }
  process.stdout.write(emitFactored("hostile_input", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped} (timeout ${timedOut}), repetidos ${dup}\n`);
})();
