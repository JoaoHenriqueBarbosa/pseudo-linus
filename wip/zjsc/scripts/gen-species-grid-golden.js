// Gera tests/golden/species_grid_bun.tsv: Symbol.species em grade, medido no bun 1.4.2.
// Cobre Array (map, filter, slice, splice, concat, flat, flatMap), TypedArray (map, filter, slice, subarray),
// ArrayBuffer/SharedArrayBuffer.slice, Promise.then/catch/finally, RegExp[Symbol.split]/[Symbol.matchAll] e as
// operações de Map/Set (que não consultam species) mais os getters `Symbol.species` dos construtores embutidos.
// Eixos: o valor de species (undefined, null, não construtor com o TypeError exato, construtor que devolve objeto
// menor, de outro tipo, primitivo, congelado, que lança, classe, bound, proxy), a forma de entregar a species
// (objeto com a chave, função com getter que conta, getter que lança), `constructor` que não é objeto ou cujo getter
// lança, receptores (array, array-like, proxy, subclasse com species estático) e a contagem de chamadas e argumentos
// (registrados em `L`). Cada programa roda num bun filho novo (no máximo 6 em paralelo, 8 s de limite), sem APIs de
// host. Programas já presentes em outros goldens (primeira coluna) são descartados.
// Uso: bun scripts/gen-species-grid-golden.js
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { knownPrograms, emitFactored, sampleByHash } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function Q(v,a){if(v===null||typeof v!=="object"&&typeof v!=="function")return S(v);var s=Object.prototype.toString.call(v)+(v===a?"=same":"");' +
  'if(ArrayBuffer.isView(v))return s+S(Array.from(v,function(x){return typeof x==="bigint"?x+"n":x}))+"/"+v.byteLength;' +
  'if(v instanceof ArrayBuffer||v instanceof SharedArrayBuffer)return s+S(Array.from(new Uint8Array(v)))+"/"+v.byteLength+"/"+(v.resizable||v.growable?"r"+v.maxByteLength:"f");' +
  'if(v instanceof Promise)return s+"P"+(Object.getPrototypeOf(v)===Promise.prototype);' +
  'if(v instanceof Set)return s+"Set"+S(Array.from(v));if(v instanceof Map)return s+"Map"+S(Array.from(v));' +
  'if(Array.isArray(v))return s+S(v)+"/"+v.length+(Object.getPrototypeOf(v)===Array.prototype?"":"/proto");' +
  'if(typeof v==="function")return s+"fn";return s+S(v)}\n';

// Entradas de species: [rótulo, expressão]. As expressões podem usar L, S, a (o receptor) e arguments.
const GENERIC = [
  ["undef", "undefined"], ["null", "null"], ["num", "1"], ["obj", "{}"], ["arrow", "()=>1"], ["method", "({m(){}}).m"],
  ["mathmax", "Math.max"], ["str", '"s"'], ["sym", 'Symbol("q")'], ["genfn", "function*(){}"], ["asyncfn", "async function(){}"],
  ["true", "true"], ["bigint", "1n"],
];
const rec = (body) => `function(){L.push("c:"+arguments.length+":"+S([].slice.call(arguments)));${body}}`;
const subclassOf = (base) => `class extends ${base}{constructor(...x){L.push("sub:"+x.length);super(...x)}}`;
const proxyCtor = (base) => `new Proxy(${base},{construct(t,x,nt){L.push("pc:"+x.length+":"+S(x));return Reflect.construct(t,x,nt)}})`;
const COMMON_CTOR = (base) => [
  ["real", base], ["bound", `${base}.bind(null)`], ["sub", subclassOf(base)], ["proxyCtor", proxyCtor(base)],
  ["classEmpty", "class{}"], ["Object", "Object"], ["throw", rec('throw new RangeError("boom")')], ["noop", rec("")],
  ["retPrim", rec("return 5")], ["retNull", rec("return null")], ["retFn", rec("return function(){}")], ["retObj", rec("return {}")],
];
const entriesArray = () => [
  ...GENERIC, ...COMMON_CTOR("Array"),
  ["retArr0", rec("return []")], ["retNewN", rec("return new Array(arguments[0])")], ["retLen5", rec("return {length:5}")],
  ["retFrozen", rec("return Object.freeze([])")], ["retFrozenObj", rec("return Object.freeze({})")], ["retU8", rec("return new Uint8Array(2)")],
  ["retProxy", rec('return new Proxy([],{defineProperty(t,k,d){L.push("dp:"+String(k));return Reflect.defineProperty(t,k,d)}})')],
  ["retBig", rec("var x=[];x.length=10;return x")], ["retFixedLen", rec("var x=[];Object.defineProperty(x,'length',{writable:false});return x")],
  ["retNonConfig", rec("return Object.defineProperty({},'0',{value:0,configurable:false,writable:true})")],
  ["retSelf", rec("return a")], ["retSetter", rec("return Object.defineProperty({},'0',{set(v){L.push('set0:'+v)},configurable:true})")],
];
const entriesTyped = (T, O, M) => {
  const sub = (s) => s.replace(/\$T/g, T).replace(/\$O/g, O).replace(/\$M/g, M);
  return [
    ...GENERIC, ...COMMON_CTOR(T),
    ["good", rec("return Reflect.construct($T,arguments)")], ["short", rec("return new $T(1)")], ["zero", rec("return new $T(0)")],
    ["long", rec("return new $T(10)")], ["other", rec("return Reflect.construct($O,arguments)")], ["mismatch", rec("return new $M(arguments[0])")],
    ["plainArr", rec("return []")], ["retSelf", rec("return a")], ["proxyTA", rec("return new Proxy(new $T(4),{})")],
    ["realOther", "$O"], ["realMismatch", "$M"], ["abstract", "Object.getPrototypeOf($T)"], ["subOther", subclassOf("$O")],
  ].map(([l, e]) => [l, sub(e)]);
};
const entriesBuffer = (A, X) => [
  ...GENERIC, ...COMMON_CTOR(A),
  ["good", rec("return new $A(arguments[0])")], ["short", rec("return new $A(1)")], ["zero", rec("return new $A(0)")],
  ["long", rec("return new $A(100)")], ["self", rec("return a")], ["otherKind", rec("return new $X(arguments[0])")],
  ["u8", rec("return new Uint8Array(arguments[0])")], ["resizable", rec("return new $A(arguments[0],{maxByteLength:100})")],
  ["realOther", "$X"],
].map(([l, e]) => [l, e.replace(/\$A/g, A).replace(/\$X/g, X)]);
const execMatch = "var n=0;return {lastIndex:0,exec(s){L.push('ex:'+this.lastIndex+':'+(n++));if(n===1){this.lastIndex=1;var m=['b'];m.index=1;return m}return null}}";
const entriesRegExp = () => [
  ...GENERIC, ...COMMON_CTOR("RegExp"),
  ["good", rec("return new RegExp(arguments[0],arguments[1])")],
  ["splitterNull", rec("return {lastIndex:0,exec(s){L.push('ex:'+this.lastIndex);return null}}")], ["execMatch", rec(execMatch)],
  ["execThrows", rec("return {lastIndex:0,exec(){throw new RangeError('x')}}")], ["execPrim", rec("return {lastIndex:0,exec(){return 1}}")],
  ["bareNull", rec("return Object.create(null)")], ["subLog", `class extends RegExp{constructor(p,f){L.push("sub:"+S([typeof p,f]));super(p,f)}}`],
];
const entriesPromise = () => [
  ...GENERIC,
  ["real", "Promise"], ["bound", "Promise.bind(null)"], ["sub", `class extends Promise{constructor(ex){L.push("sub:"+typeof ex);super(ex)}}`],
  ["proxyCtor", proxyCtor("Promise")], ["classEmpty", "class{}"], ["Object", "Object"],
  ["good", 'function(ex){L.push("c:"+typeof ex+":"+ex.length+":"+JSON.stringify(ex.name));ex(function(){},function(){})}'],
  ["noCall", 'function(ex){L.push("c")}'], ["twice", "function(ex){ex(function(){},function(){});ex(function(){},function(){})}"],
  ["undefRes", "function(ex){ex(undefined,function(){})}"], ["undefRej", "function(ex){ex(function(){},undefined)}"],
  ["nums", "function(ex){ex(1,2)}"], ["objs", "function(ex){ex({},{})}"], ["throws", 'function(ex){throw new RangeError("boom")}'],
  ["throwAfter", 'function(ex){ex(function(){},function(){});throw new RangeError("late")}'],
  ["retObj", "function(ex){ex(function(){},function(){});return {}}"], ["retPrim", "function(ex){ex(function(){},function(){});return 5}"],
  ["retThenable", "function(ex){ex(function(){},function(){});return {then(){}}}"], ["noop0", "function(){}"],
  ["resLogs", "function(ex){ex(function(v){L.push('res')},function(v){L.push('rej')})}"],
  ["lateRes", "function(ex){ex(function(){},function(){});this.then=function(){L.push('own-then')}}"],
];
const entriesCollection = (base) => [
  ...GENERIC, ["real", base], ["sub", subclassOf(base)], ["retObj", rec("return {}")], ["throw", rec('throw new RangeError("boom")')], ["noop", rec("")],
];

// Valores de `constructor` que não passam por species.
const CTOR_VALUES = [
  ["cv-num", "1"], ["cv-str", '"x"'], ["cv-null", "null"], ["cv-undef", "undefined"], ["cv-true", "true"], ["cv-sym", 'Symbol("c")'],
  ["cv-bigint", "0n"], ["cv-Object", "Object"], ["cv-fn", "function(){}"], ["cv-FunctionProto", "Function.prototype"],
  ["cv-arrow", "()=>1"], ["cv-emptyobj", "{}"], ["cv-nullproto", "Object.create(null)"],
];

// Formas de entregar a species: devolve K = [rótulo, "value" | "getter", expressão].
function ks(entries, w2Every) {
  const out = [];
  // Um em cada `w2Every` valores ganha também a forma w2, escolhido por hash do rótulo (`sampleByHash`), não pela posição.
  const w2Labels = new Set(sampleByHash(entries.map(([label]) => label), Math.ceil(entries.length / w2Every)));
  entries.forEach(([label, x]) => {
    out.push([`w1-${label}`, "value", `{[Symbol.species]:${x}}`]);
    if (w2Labels.has(label)) {
      out.push([`w2-${label}`, "value", `(function(){function C(){}Object.defineProperty(C,Symbol.species,{get(){L.push("g");return ${x}}});return C})()`]);
    }
  });
  out.push(["w3-throw", "value", '{get [Symbol.species](){L.push("g");throw new RangeError("sp")}}']);
  out.push(["cv-getter", "getter", ""]);
  for (const [label, x] of CTOR_VALUES) out.push([label, "value", x]);
  return out;
}
const install = (target, [, kind, expr]) => kind === "getter"
  ? `Object.defineProperty(${target},"constructor",{get(){L.push("get");throw new SyntaxError("cg")},configurable:true})`
  : `Object.defineProperty(${target},"constructor",{value:${expr},writable:true,configurable:true})`;

const exprs = []; // programas completos já com `globalThis.R = ...`
const program = (setup, op) =>
  `globalThis.R = (function(){var L=[],r,a;try{${setup};r=Q(${op},a)}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return r+" | "+S(L)})()`;

// Array.
const arrayOps = [
  ["a.map(x=>x)", "any"], ["a.filter(x=>x!=2)", "any"], ["a.slice()", "any"], ["a.slice(1)", "any"], ["a.splice(0,2)", "any"],
  ["a.splice(1,0,9)", "any"], ["a.concat([4],5)", "any"], ["a.concat()", "any"], ["a.flat()", "nested"], ["a.flatMap(x=>[x,x])", "any"],
];
const arrayInputs = { any: ["[1,2,3]", "[1,,3,4]"], nested: ["[[1],[2,[3]],4]", "[[],[,1]]"] };
const arrayKs = ks(entriesArray(), 2);
for (const k of arrayKs) {
  for (const [op, input] of arrayOps) {
    for (const text of arrayInputs[input]) exprs.push(program(`a=${text};${install("a", k)}`, op));
  }
}
const lessOps = ["a.map(x=>x)", "a.filter(x=>x!=2)", "a.slice(1)", "a.splice(0,2)"];
const arrayLessKs = sampleByHash(arrayKs, Math.ceil(arrayKs.length / 2), (k) => k[0]);
for (const k of arrayLessKs) {
  for (const op of lessOps) {
    exprs.push(program(`var t=[1,2,3];${install("t", k)};a=new Proxy(t,{get(o,p,r){if(p==="constructor")L.push("get-ctor");return Reflect.get(o,p,r)}})`, op));
    exprs.push(program(`class A extends Array{};a=new A(1,2,3);${install("a", k)}`, op));
    const call = op.replace("a.", "Array.prototype.") .replace("(", ".call(a,").replace(",)", ")");
    exprs.push(program(`a={length:3,0:1,1:2,2:3};${install("a", k)}`, call));
  }
}
for (const [label, x] of entriesArray()) {
  for (const op of lessOps) exprs.push(program(`class A extends Array{static get [Symbol.species](){L.push("sp");return ${x}}};a=new A(1,2,3)`, op));
}

// TypedArray.
const typedTypes = [
  ["Uint8Array", "Int8Array", "BigInt64Array", "[1,2,3,4]", "x=>x*2", "x=>x>1"],
  ["Int16Array", "Uint16Array", "BigUint64Array", "[1,2,3,4]", "x=>x*2", "x=>x>1"],
  ["Float32Array", "Float64Array", "BigInt64Array", "[1.5,2,3,4]", "x=>x*2", "x=>x>1"],
  ["BigInt64Array", "BigUint64Array", "Uint8Array", "[1n,2n,3n,4n]", "x=>x*2n", "x=>x>1n"],
];
for (const [T, O, M, src, mapper, pred] of typedTypes) {
  const ops = [`a.map(${mapper})`, `a.filter(${pred})`, "a.slice()", "a.slice(1,3)", "a.subarray()", "a.subarray(1,3)"];
  const entries = entriesTyped(T, O, M);
  for (const k of ks(entries, 2)) {
    for (const op of ops) exprs.push(program(`a=new ${T}(${src});${install("a", k)}`, op));
  }
  for (const [label, x] of entries) {
    for (const op of [ops[0], ops[2], ops[4]]) {
      exprs.push(program(`class A extends ${T}{static get [Symbol.species](){L.push("sp");return ${x}}};a=new A(${src})`, op));
    }
  }
}

// ArrayBuffer e SharedArrayBuffer.
const bufferReceivers = [
  ["ArrayBuffer", "SharedArrayBuffer", "new ArrayBuffer(8)"],
  ["ArrayBuffer", "SharedArrayBuffer", "new ArrayBuffer(8,{maxByteLength:16})"],
  ["SharedArrayBuffer", "ArrayBuffer", "new SharedArrayBuffer(8)"],
  ["SharedArrayBuffer", "ArrayBuffer", "new SharedArrayBuffer(8,{maxByteLength:16})"],
];
for (const [A, X, make] of bufferReceivers) {
  const entries = entriesBuffer(A, X);
  for (const k of ks(entries, 2)) {
    for (const op of ["a.slice(2,6)", "a.slice()", "a.slice(-3)"]) {
      exprs.push(program(`a=${make};new Uint8Array(a).set([1,2,3,4,5,6,7,8]);${install("a", k)}`, op));
    }
  }
  for (const [label, x] of entries) {
    exprs.push(program(`a=${make};class B extends ${A}{static get [Symbol.species](){L.push("sp");return ${x}}};a=Reflect.construct(${A},[8],B);new Uint8Array(a).set([1,2,3,4,5,6,7,8])`, "a.slice(2,6)"));
  }
}

// Promise.
const promiseOps = [
  "a.then(function(){})", "a.then(function(){},function(){})", "a.then()", "a.then(1,2)", "a.finally(function(){})", "a.finally()", "a.finally(1)",
  "a.catch(function(){})",
];
const promiseEntries = entriesPromise();
for (const k of ks(promiseEntries, 2)) {
  for (const op of promiseOps) {
    exprs.push(program(`a=Promise.resolve(1);${install("a", k)}`, op));
    exprs.push(program(`a=new Promise(function(){});${install("a", k)}`, op));
  }
}
for (const [label, x] of promiseEntries) {
  for (const op of ["a.then(function(){})", "a.finally(function(){})", "a.catch(function(){})"]) {
    exprs.push(program(`class P extends Promise{static get [Symbol.species](){L.push("sp");return ${x}}};a=P.resolve(1)`, op));
  }
}

// RegExp.
const regexps = ["/b/", "/b/g", "/(?:)/", "/b/iu", "/b/y"];
const regexpOps = ['a[Symbol.split]("abcbd")', 'a[Symbol.split]("abcbd",1)', 'a[Symbol.split]("")', 'a[Symbol.split]("abc",0)', 'Array.from(a[Symbol.matchAll]("abab"),m=>m[0])'];
for (const k of ks(entriesRegExp(), 2)) {
  for (const re of regexps) {
    for (const op of regexpOps) exprs.push(program(`a=${re};${install("a", k)}`, op));
  }
}

// Map e Set.
const setOps = [
  "a.union(b)", "a.intersection(b)", "a.difference(b)", "a.symmetricDifference(b)", "a.isSubsetOf(b)", "a.isSupersetOf(b)", "a.isDisjointFrom(b)",
  "new Set(a)", "Array.from(a)",
];
for (const k of ks(entriesCollection("Set"), 2)) {
  for (const op of setOps) exprs.push(program(`a=new Set([1,2,3]);var b=new Set([2,3,4]);${install("a", k)};${install("b", k)}`, op));
}
for (const k of ks(entriesCollection("Map"), 2)) {
  for (const op of ["new Map(a)", "Array.from(a)", "new Map([...a])"]) exprs.push(program(`a=new Map([[1,2],[3,4]]);${install("a", k)}`, op));
}

// Getters `Symbol.species` dos construtores embutidos.
const holders = ["Array", "Map", "Set", "Promise", "RegExp", "ArrayBuffer", "SharedArrayBuffer", "Object.getPrototypeOf(Int8Array)"];
const receivers = ["undefined", "null", "1", '"s"', "{}", "H", "Symbol('r')", "()=>1", "Object.create(H)", "H.bind(null)", "new Proxy(H,{})", "function(){}"];
for (const h of holders) {
  const setup = `var H=${h}`;
  const getter = "Object.getOwnPropertyDescriptor(H,Symbol.species).get";
  exprs.push(program(`${setup};a=H`, `S([Object.getOwnPropertyNames(Object.getOwnPropertyDescriptor(H,Symbol.species)),${getter}.name,${getter}.length,Object.getOwnPropertyDescriptor(H,Symbol.species).set,Object.getOwnPropertyDescriptor(H,Symbol.species).enumerable,Object.getOwnPropertyDescriptor(H,Symbol.species).configurable])`));
  for (const r of receivers) {
    exprs.push(program(`${setup};a=${r}`, `${getter}.call(a)===a`));
    exprs.push(program(`${setup};a=${r}`, `Reflect.get(H,Symbol.species,a)===a`));
  }
}

const known = new Set();
for (const p of knownPrograms("species_grid_bun.tsv", (file) => file !== "species_grid_bun.tsv")) known.add(JSON.stringify(p));
const seen = new Set();
const jobs = [];
let dup = 0;
let host = 0;
for (const expr of exprs) {
  const source = PRELUDE + expr;
  const key = JSON.stringify(source);
  if (seen.has(key)) continue;
  seen.add(key);
  if (known.has(key)) { dup++; continue; }
  if (usesHostApi(source)) { host++; continue; }
  jobs.push({ expr, source });
}

const BAD = new RegExp("/home/|/tmp/|/Users/|\\.js:\\d|bun|" + String.fromCharCode(0x2014) + "|" + String.fromCharCode(0x2013), "i");

function runChild(job) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 8000);
    child.stdout.on("data", (chunk) => { out += chunk; });
    child.on("close", (code) => { clearTimeout(timer); resolve({ timedOut, code, out }); });
    child.stdin.on("error", () => {});
    child.stdin.end(job.source);
  });
}

async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const index = next++;
      results[index] = await runChild(jobs[index]);
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  let dropped = 0;
  let timeouts = 0;
  const rows = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.timedOut) { timeouts++; continue; }
    if (r.code !== 0 || BAD.test(r.out)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(jobs[i].expr).slice(0, 200) + "\n  " + JSON.stringify(r.out).slice(0, 200) + "\n");
      continue;
    }
    rows.push({ source: jobs[i].source, result: r.out });
  }
  const tsv = path.join(__dirname, "..", "tests", "golden", "species_grid_bun.tsv");
  fs.writeFileSync(tsv, emitFactored("species_grid", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, timeouts ${timeouts}, repetidos ${dup}, host ${host}\n`);
}
main();
