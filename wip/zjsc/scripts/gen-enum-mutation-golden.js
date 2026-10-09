// Gera tests/golden/enum_mutation_bun.tsv: enumeração de propriedades com mutação do próprio objeto durante a
// enumeração, medida no bun 1.4.2 (um bun filho novo por programa, sem APIs de host, resultado em `globalThis.R`).
// Cobre JSON.stringify (valor, indentação, array embrulhado, lista de chaves, replacer) e Object.keys/values/entries/
// assign/fromEntries/spread/for-in sobre objetos cujo getter muta o próprio objeto no meio da enumeração: adiciona chave
// (string, inteira, símbolo), remove chave adiante, atrás e a própria, torna não enumerável, redefine como acessor ou
// valor, remove e reinsere, preventExtensions, freeze, seal, troca de protótipo. Os contêineres são objeto comum, com
// chaves inteiras, array (length, push, splice), typed array (fixo e sobre ArrayBuffer redimensionável, com resize no
// meio), objeto com protótipo enumerável, String, arguments, instância de classe, função, Proxy sem traps (mutando pelo
// proxy e pelo alvo) e Proxy com traps que registram a ordem das chamadas. A segunda parte são Proxies cujo ownKeys e
// getOwnPropertyDescriptor mentem (TypeError de invariante exato) contra alvos extensíveis, não configuráveis,
// congelados, selados e não extensíveis. A terceira é o for-in com mutação no corpo do laço (chaves adicionadas ou
// removidas ainda não visitadas, protótipo mutado no meio).
// Colunas: sufixo do programa (JSON), valor de `globalThis.R` (JSON) e o índice do prelúdio (scripts/golden-prelude.js).
// Quarta coluna opcional: JSON das outras saídas aceitas. Cada programa roda 7 vezes em processos bun separados
// (scripts/golden-alternatives.js), porque a mensagem do invariante de ownKeys do Proxy escolhe a chave citada pela
// ordem de um HashSet de ponteiros, que varia entre execuções; se as saídas divergem, todas ficam registradas.
// Programas já presentes nos goldens vizinhos (knownPrograms) são descartados.
// Uso: bun scripts/gen-enum-mutation-golden.js > tests/golden/enum_mutation_bun.tsv
const fs = require("fs");
const { emitFactored, knownPrograms, sampleByHash } = require("./golden-prelude.js");
const { spawn } = require("child_process");
const { measureStable } = require("./golden-alternatives.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE = `function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}
function post(m){return Reflect.ownKeys(m).map(function(k){return String(k)+(Object.prototype.propertyIsEnumerable.call(m,k)?"":"!")}).join()}
function err(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}
var NX=function(ks,i){return ks[(i+1)%4]},PV=function(ks,i){return ks[(i+3)%4]};
function hide(o,k){Object.defineProperty(o,k,{enumerable:false})}
var MUT={
add:["*",function(o){o.z=9}],
addSym:["*",function(o){o[Symbol.for("s")]=1}],
addInt:["*",function(o){o[7]=7}],
addMany:["*",function(o){o.z1=1;o.z2=2;o.z3=3}],
delNext:["*",function(o,ks,i){delete o[NX(ks,i)]}],
delPrev:["*",function(o,ks,i){delete o[PV(ks,i)]}],
delSelf:["*",function(o,ks,i){delete o[ks[i]]}],
delOthers:["*",function(o,ks,i){ks.forEach(function(k,j){if(j!==i)delete o[k]})}],
hideNext:["*",function(o,ks,i){hide(o,NX(ks,i))}],
hidePrev:["*",function(o,ks,i){hide(o,PV(ks,i))}],
hideSelf:["*",function(o,ks,i){hide(o,ks[i])}],
accNext:["*",function(o,ks,i){Object.defineProperty(o,NX(ks,i),{get:function(){return "A"},enumerable:true,configurable:true})}],
valNext:["*",function(o,ks,i){Object.defineProperty(o,NX(ks,i),{value:"V",enumerable:true,configurable:true,writable:true})}],
valPrev:["*",function(o,ks,i){Object.defineProperty(o,PV(ks,i),{value:"V",enumerable:true,configurable:true,writable:true})}],
replSelf:["*",function(o,ks,i){Object.defineProperty(o,ks[i],{value:"R",enumerable:true,configurable:true,writable:true})}],
setNext:["*",function(o,ks,i){o[NX(ks,i)]="S"}],
reNext:["*",function(o,ks,i){var k=NX(ks,i),v=o[k];delete o[k];o[k]=v+"'"}],
rePrev:["*",function(o,ks,i){var k=PV(ks,i),v=o[k];delete o[k];o[k]=v+"'"}],
noExtAdd:["*",function(o){Object.preventExtensions(o);o.z=1}],
noExtDel:["*",function(o,ks,i){Object.preventExtensions(o);delete o[NX(ks,i)]}],
freeze:["*",function(o){Object.freeze(o)}],
seal:["*",function(o){Object.seal(o)}],
setProto:["*",function(o){Object.setPrototypeOf(o,{h:1,a:"pa"})}],
setNull:["*",function(o){Object.setPrototypeOf(o,null)}],
aPush:["arr",function(o){o.push(99)}],
aTrunc:["arr",function(o){o.length=1}],
aClear:["arr",function(o){o.length=0}],
aGrow:["arr",function(o){o.length=6}],
aSplice:["arr",function(o){o.splice(0,1)}],
aUnshift:["arr",function(o){o.unshift(5)}],
aReverse:["arr",function(o){o.reverse()}],
tFill:["ta",function(o){o.fill(9)}],
tSet0:["ta",function(o){o[0]=50}],
rShrink:["rs",function(o){o.buffer.resize(1)}],
rShrink0:["rs",function(o){o.buffer.resize(0)}],
rGrow:["rs",function(o){o.buffer.resize(4)}],
rGrowMax:["rs",function(o){o.buffer.resize(6)}],
pAdd:["pr",function(o){Object.getPrototypeOf(o).pz=1}],
pDel:["pr",function(o){delete Object.getPrototypeOf(o).h}],
pShadow:["pr",function(o){o.h="own"}],
pShadowHide:["pr",function(o){Object.defineProperty(o,"h",{value:"own",enumerable:false,configurable:true})}],
pSwap:["pr",function(o){Object.setPrototypeOf(o,{h:"N1",x:"N2"})}]
};
var TRAPS=["getPrototypeOf","setPrototypeOf","isExtensible","preventExtensions","getOwnPropertyDescriptor","defineProperty","has","get","set","deleteProperty","ownKeys"];
function H(log){var h={};TRAPS.forEach(function(n){h[n]=function(t,k){log.push(n+(n==="ownKeys"||n==="getPrototypeOf"||n==="isExtensible"||n==="preventExtensions"?"":" "+String(k)));return Reflect[n].apply(null,arguments)}});return h}
function B(kind,i){
var ks=["a","b","c","d"],log=[],base,direct=false;
if(kind==="arr"){ks=["0","1","2","3"];base=[10,11,12,13]}
else if(kind==="ta"||kind==="tars"||kind==="tarf"){ks=["0","1","p","q"];
if(kind==="ta")base=new Uint8Array([1,2]);else{var buf=new ArrayBuffer(2,{maxByteLength:6});base=kind==="tars"?new Uint8Array(buf):new Uint8Array(buf,0,2)}
base.p="P";base.q="Q"}
else if(kind==="str"){ks=["0","1","p","q"];base=new String("ab");base.p="P";base.q="Q"}
else if(kind==="args"){ks=["0","1","p","q"];base=(function(){return arguments})(1,2);base.p="P";base.q="Q"}
else if(kind==="objproto"){base=Object.create({h:"H1",h2:"H2"});base.a=1;base.b=2;base.c=3;base.d=4}
else if(kind==="objint"){ks=["1","2","b","a"];base={};base.b=4;base.a=3;base[2]=2;base[1]=1}
else if(kind==="inst"){base=new (class C{constructor(){this.a=1;this.b=2;this.c=3;this.d=4}})}
else if(kind==="fn"){base=function f(){};base.a=1;base.b=2;base.c=3;base.d=4}
else{base={a:1,b:2,c:3,d:4};direct=kind==="proxyd"||kind==="plogd"}
var target=base,o=base,done=false,mn=B.mn,mut=mn&&MUT[mn][1];
if(i>=0)Object.defineProperty(target,ks[i],{enumerable:true,configurable:true,get:function(){if(!done){done=true;mut(direct?target:this,ks,i)}return "g"+ks[i]}});
if(kind==="proxy"||kind==="proxyd")o=new Proxy(target,{});
else if(kind==="plog"||kind==="plogd")o=new Proxy(target,H(log));
return {o:o,m:target,ks:ks,log:log}}
var OPS={
keys:function(o){return Object.keys(o).join()},
values:function(o){return S(Object.values(o))},
entries:function(o){return S(Object.entries(o))},
assign:function(o){return S(Object.assign({},o))},
json:function(o){return JSON.stringify(o)},
jsonI:function(o){return JSON.stringify(o,null,1)},
jsonA:function(o){return JSON.stringify([o])},
jsonL:function(o,ks){return JSON.stringify(o,ks)},
jsonR:function(o){return JSON.stringify(o,function(k,v){return typeof v==="string"?v+"!":v})},
spread:function(o){return S({...o})},
fromE:function(o){return S(Object.fromEntries(Object.entries(o)))},
forin:function(o){var r=[];for(var k in o)r.push(k+"="+S(o[k]));return r.join()},
snap:function(o){var ks=Object.keys(o);return ks.map(function(k){return k+"="+S(o[k])}).join()},
names:function(o){return Object.getOwnPropertyNames(o).join()},
rk:function(o){return Reflect.ownKeys(o).map(String).join()}
};
function X(kind,i,mn,op){B.mn=mn;var b=B(kind,i),r;try{r=OPS[op](b.o,b.ks)}catch(e){r=err(e)}return r+" | "+post(b.m)+(b.log.length?" | "+b.log.join(";"):"")}
function Z(kind,at,mn,rd){B.mn=mn;var b=B(kind,-1),o=b.o,r=[],n=0,mut=MUT[mn][1];try{for(var k in o){r.push(rd?k+"="+S(o[k]):k);if(n++===at)mut(o,b.ks,at)}}catch(e){r.push(err(e))}return r.join()+" | "+post(b.m)+(b.log.length?" | "+b.log.join(";"):"")}
var TG={plain:function(){return {a:1,b:2}},
ncfg:function(){return Object.defineProperty({a:1},"b",{value:2,enumerable:true,configurable:false})},
ncfgHidden:function(){return Object.defineProperty({a:1},"h",{value:2,enumerable:false,configurable:false})},
ncfgRO:function(){return Object.defineProperty({a:1},"b",{value:2,writable:false,enumerable:true,configurable:false})},
frozen:function(){return Object.freeze({a:1,b:2})},
sealed:function(){return Object.seal({a:1,b:2})},
nonext:function(){return Object.preventExtensions({a:1,b:2})},
arr:function(){return [1,2]}};
var OK={honest:null,
omitLast:function(r){return r.slice(0,-1)},
omitFirst:function(r){return r.slice(1)},
extra:function(r){return r.concat("x")},
dup:function(r){return r.concat(r.slice(0,1))},
num:function(r){return r.concat(5)},
reverse:function(r){return r.reverse()},
sym:function(r){return r.concat(Symbol.for("q"))},
empty:function(r){return []},
undef:function(r){return undefined},
arrayLike:function(r){return {length:r.length,0:r[0],1:r[1]}}};
var GD={honest:null,
undef:function(t,k){return undefined},
enumAll:function(t,k){return {value:1,enumerable:true,configurable:true}},
ncfg:function(t,k){return {value:1,enumerable:true,configurable:false}},
num:function(t,k){return 1},
hidden:function(t,k){return {value:1,enumerable:false,configurable:true}},
acc:function(t,k){return {get:function(){return "g"},enumerable:true,configurable:true}},
undefFirst:function(t,k){return k==="a"?undefined:Reflect.getOwnPropertyDescriptor(t,k)},
ro:function(t,k){return {value:1,writable:false,enumerable:true,configurable:false}},
nul:function(t,k){return null},
wncfg:function(t,k){return {value:1,writable:true,enumerable:true,configurable:false}}};
function Y(tg,ok,gd,op){var t=TG[tg](),h={};if(OK[ok])h.ownKeys=function(t){return OK[ok](Reflect.ownKeys(t))};if(GD[gd])h.getOwnPropertyDescriptor=function(t,k){return GD[gd](t,k)};var p=new Proxy(t,h),r;try{r=OPS[op](p,["a","b"])}catch(e){r=err(e)}return r}
`;

// ---- Geração dos casos.
// Amostra determinística por hash do texto (sampleByHash), sem depender da posição na lista.
const sample = (list, count) => sampleByHash(list, count);

// Reproduz em JS o filtro de MUT por contêiner (etiqueta -> tipos aceitos).
const MUT_TAGS = {
  add: "*", addSym: "*", addInt: "*", addMany: "*", delNext: "*", delPrev: "*", delSelf: "*", delOthers: "*",
  hideNext: "*", hidePrev: "*", hideSelf: "*", accNext: "*", valNext: "*", valPrev: "*", replSelf: "*", setNext: "*",
  reNext: "*", rePrev: "*", noExtAdd: "*", noExtDel: "*", freeze: "*", seal: "*", setProto: "*", setNull: "*",
  aPush: "arr", aTrunc: "arr", aClear: "arr", aGrow: "arr", aSplice: "arr", aUnshift: "arr", aReverse: "arr",
  tFill: "ta", tSet0: "ta", rShrink: "rs", rShrink0: "rs", rGrow: "rs", rGrowMax: "rs",
  pAdd: "pr", pDel: "pr", pShadow: "pr", pShadowHide: "pr", pSwap: "pr",
};
function allowed(tag, kind) {
  if (tag === "*") return true;
  if (tag === "arr") return kind === "arr";
  if (tag === "ta") return /^ta/.test(kind);
  if (tag === "rs") return kind === "tars" || kind === "tarf";
  return kind === "objproto";
}
const INDEXED_PLUS_NAMED = new Set(["ta", "tars", "tarf", "str", "args"]);
const OPS_A = ["keys", "values", "entries", "assign", "json", "jsonI", "jsonA", "jsonL", "jsonR", "spread", "fromE", "forin", "snap", "names"];
const KINDS_A = ["obj", "objint", "arr", "ta", "tars", "tarf", "objproto", "proxy", "proxyd", "plog", "plogd", "str", "args", "inst", "fn"];
const KINDS_D = ["obj", "objint", "arr", "ta", "tars", "tarf", "objproto", "proxy", "plog", "str", "args", "inst", "fn"];

const exprs = [];
// Parte A: getter que muta durante a enumeração.
const partA = [];
for (const kind of KINDS_A) {
  const positions = INDEXED_PLUS_NAMED.has(kind) ? [2, 3] : [0, 1, 2, 3];
  for (const mutation of Object.keys(MUT_TAGS)) {
    if (!allowed(MUT_TAGS[mutation], kind)) continue;
    for (const position of positions) for (const op of OPS_A) partA.push(`X(${JSON.stringify(kind)},${position},${JSON.stringify(mutation)},${JSON.stringify(op)})`);
  }
}
exprs.push(...sample(partA, 3000));
// Parte B: Proxy cujo ownKeys e getOwnPropertyDescriptor mentem.
const OPS_C = ["keys", "values", "entries", "assign", "json", "spread", "fromE", "forin", "names", "rk"];
const targets = ["plain", "ncfg", "ncfgHidden", "ncfgRO", "frozen", "sealed", "nonext", "arr"];
const okeys = ["honest", "omitLast", "omitFirst", "extra", "dup", "num", "reverse", "sym", "empty", "undef", "arrayLike"];
const gdops = ["honest", "undef", "enumAll", "ncfg", "num", "hidden", "acc", "undefFirst", "ro", "nul", "wncfg"];
let combo = 0;
for (const target of targets) for (const okey of okeys) for (const gd of gdops) {
  for (let j = 0; j < 3; j++) exprs.push(`Y(${JSON.stringify(target)},${JSON.stringify(okey)},${JSON.stringify(gd)},${JSON.stringify(OPS_C[(combo * 3 + j * 3 + j) % OPS_C.length])})`);
  combo++;
}
// Parte C: for-in com mutação no corpo do laço.
const partD = [];
for (const kind of KINDS_D) for (const mutation of Object.keys(MUT_TAGS)) {
  if (!allowed(MUT_TAGS[mutation], kind)) continue;
  for (const at of [0, 1, 2]) for (const read of [false, true]) partD.push(`Z(${JSON.stringify(kind)},${at},${JSON.stringify(mutation)},${read})`);
}
exprs.push(...sample(partD, 1700));

const known = new Set(knownPrograms("enum_mutation_bun.tsv", (name) => name !== "enum_mutation_bun.tsv")); // sem o próprio tsv, senão a regeração descarta tudo
const jobs = [];
let dup = 0;
const seen = new Set();
for (const expr of exprs) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source) || known.has(source)) { dup++; continue; }
  seen.add(source);
  jobs.push({ expr, source });
}
// Mensagens de invariante do ownKeys que citam uma chave escolhida pela ordem de um HashSet de ponteiros.
const UNSTABLE_MESSAGE = /has (?:the non-configurable|configurable) property '/;
const runChild =(job) => new Promise((resolve) => {
  // Processo fresco por programa: a ordem de reificação das tabelas estáticas do JSC depende do que rodou antes.
  const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
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
      results[i] = await measureStable(() => runChild(jobs[i]), 7, { suspect: UNSTABLE_MESSAGE });
    }
  };
  await Promise.all(Array.from({ length: 12 }, worker));
  let kept = 0;
  let dropped = 0;
  let varied = 0;
  const lines = [];
  jobs.forEach((job, i) => {
    const r = results[i];
    if (!r.ok) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + " " + r.err.slice(0, 100) + "\n"); return; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(job.expr).slice(0, 160) + "\n"); return; }
    kept++;
    if (r.alternatives.some((alt) => /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(alt))) { dropped++; process.stderr.write("caminho ou marca em alternativa: " + JSON.stringify(job.expr).slice(0, 160) + "\n"); kept--; return; }
    if (r.alternatives.length) varied++;
    lines.push({ source: job.source, result: r.out, alternatives: r.alternatives });
  });
  process.stdout.write(emitFactored("enum_mutation", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos ${dup}, com alternativas ${varied}\n`);
})();
