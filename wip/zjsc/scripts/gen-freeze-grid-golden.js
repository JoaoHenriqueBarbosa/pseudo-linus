// Gera tests/golden/freeze_grid_bun.tsv: grade de Object.freeze/seal/preventExtensions/isFrozen/isSealed/isExtensible
// sobre alvos variados (objeto, acessores, array, array esparso, array com length não gravável, função, classe,
// typed array vazio e com elementos, arguments, String boxed, Proxy simples e com traps que logam, objeto de protótipo
// nulo no estilo de namespace de módulo) cruzada com operações posteriores (add, delete, set, defineProperty,
// push/pop/shift/unshift/splice/sort/reverse/fill, length, setPrototypeOf, __proto__, Reflect.*) em modo estrito e
// frouxo. Também cobre defineProperty/defineProperties/create com descritores inválidos e getters que logam a ordem de
// leitura, getOwnPropertyDescriptors, ciclos de setPrototypeOf e as mensagens exatas de TypeError. Medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo (a ordem de reificação das tabelas estáticas depende do que rodou antes), sem
// APIs de host, e os filhos rodam em paralelo.
// Uso: bun scripts/gen-freeze-grid-golden.js > tests/golden/freeze_grid_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function Z(x){try{return Reflect.ownKeys(x).map(k=>String(k)+"="+D(x,k)).join("|")}catch(e){return "zthrow "+e.name+": "+e.message}}\n' +
  'function Q(x){try{return [Object.isFrozen(x),Object.isSealed(x),Object.isExtensible(x)].join()}catch(e){return "qthrow "+e.name+": "+e.message}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. A grade principal: alvo x estado x operação x modo.
const LOGGING_PROXY =
  "new Proxy({a:1},{defineProperty(t,k,d){L.push('def '+String(k));return Reflect.defineProperty(t,k,d)}," +
  "deleteProperty(t,k){L.push('del '+String(k));return Reflect.deleteProperty(t,k)},preventExtensions(t){L.push('pe');return Reflect.preventExtensions(t)}," +
  "set(t,k,v,r){L.push('set '+String(k));return Reflect.set(t,k,v,r)},setPrototypeOf(t,p){L.push('spo');return Reflect.setPrototypeOf(t,p)}," +
  "isExtensible(t){L.push('ext');return Reflect.isExtensible(t)},getOwnPropertyDescriptor(t,k){L.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}})";
const targets = {
  obj: "{a:1,b:2}",
  accessor: "{get a(){return 1},set a(v){},b:2}",
  array: "[1,2,3]",
  sparse: "[1,,3]",
  lenro: "(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});return a})()",
  fn: "function f(a){}",
  cls: "class C{static s=1}",
  u8: "new Uint8Array([1,2,3])",
  u8empty: "new Uint8Array(0)",
  args: "(function(){return arguments})(1,2)",
  str: "new String('ab')",
  proxy: "new Proxy({a:1},{})",
  logproxy: LOGGING_PROXY,
  nslike: "Object.create(null,{a:{value:1,enumerable:true,writable:true,configurable:false},[Symbol.toStringTag]:{value:'Module'}})",
  symobj: "{[Symbol.iterator]:1,0:'z',a:1}",
  date: "new Date(0)",
};
const freezers = { none: "", freeze: "Object.freeze(x)", seal: "Object.seal(x)", pe: "Object.preventExtensions(x)" };
const ops = {
  add: "x.zz=1", addIdx: "x[7]=1", setFirst: "x[k]=9", setLen: "x.length=0", delFirst: "delete x[k]", delMissing: "delete x.nope",
  defNew: "Object.defineProperty(x,'zz',{value:1})", defChange: "Object.defineProperty(x,k,{value:42})", defSame: "Object.defineProperty(x,k,{})",
  defEnum: "Object.defineProperty(x,k,{enumerable:true})", defAcc: "Object.defineProperty(x,k,{get(){return 1}})",
  push: "Array.prototype.push.call(x,5)", pop: "Array.prototype.pop.call(x)", shift: "Array.prototype.shift.call(x)",
  unshift: "Array.prototype.unshift.call(x,0)", splice1: "Array.prototype.splice.call(x,0,1)", splice2: "Array.prototype.splice.call(x,0,0,9)",
  sort: "Array.prototype.sort.call(x)", reverse: "Array.prototype.reverse.call(x)", fill: "Array.prototype.fill.call(x,0)",
  protoNull: "Object.setPrototypeOf(x,null)", protoSame: "Object.setPrototypeOf(x,Object.getPrototypeOf(x))", protoObj: "Object.setPrototypeOf(x,{})",
  protoAssign: "x.__proto__=null", protoAssignObj: "x.__proto__={}",
  rSet: "Reflect.set(x,'zz',1)", rDefine: "Reflect.defineProperty(x,'zz',{value:1})", rDelete: "Reflect.deleteProperty(x,k)",
};
for (const [tn, t] of Object.entries(targets)) {
  for (const [fn, fz] of Object.entries(freezers)) {
    for (const [on, op] of Object.entries(ops)) {
      for (const strict of [false, true]) {
        const body =
          `var L=[];var x=${t};var k=Reflect.ownKeys(x)[0];var m="";` + (fz ? `try{${fz}}catch(e){m=e.name+": "+e.message}` : "") +
          `var r;try{r=S((function(){${strict ? "'use strict';" : ""}return ${op}})())}catch(e){r="throw "+e.name+": "+e.message}` +
          `return [m,r,Q(x),Z(x),L.join()].join(" || ")`;
        add(`T(()=>{${body}})`);
      }
    }
  }
}

// ---- 2. defineProperty/defineProperties/create/Reflect.defineProperty com descritores que logam a ordem de leitura.
const fieldValue = {
  value: "1", writable: "true", enumerable: "true", configurable: "true", get: "function(){return 7}", set: "function(v){}",
  getBad: "1", setBad: "'x'", valueUndef: "undefined", writableFalse: "false", configurableFalse: "false",
  getUndef: "undefined", setUndef: "undefined", getNull: "null", valueThrow: "THROW", writableThrow: "THROW",
};
const keyName = { getBad: "get", setBad: "set", valueUndef: "value", writableFalse: "writable", configurableFalse: "configurable", getUndef: "get", setUndef: "set", getNull: "get", valueThrow: "value", writableThrow: "writable" };
function loggingDescriptor(fields) {
  const parts = fields.map(f => {
    const name = keyName[f] || f;
    const val = fieldValue[f];
    return val === "THROW" ? `get ${name}(){L.push('${name}');throw new EvalError('${name}')}` : `get ${name}(){L.push('${name}');return ${val}}`;
  });
  return `{${parts.join(",")}}`;
}
const shapes = [
  [], ["value"], ["value", "writable"], ["value", "writable", "enumerable", "configurable"], ["get"], ["get", "set"], ["get", "value"],
  ["set", "writable"], ["get", "configurable"], ["enumerable", "configurable"], ["getBad"], ["setBad"], ["valueThrow"], ["writableThrow"],
  ["valueUndef", "writableFalse", "configurableFalse"], ["getUndef", "setUndef"], ["getNull", "enumerable"], ["configurable", "enumerable", "writable", "value", "get", "set"],
];
const states = {
  fresh: "{}", existing: "{p:0}", frozen: "Object.freeze({p:0})", pe: "Object.preventExtensions({})", sealed: "Object.seal({p:0})", array: "[1,2]",
};
for (const shape of shapes) {
  const d = loggingDescriptor(shape);
  for (const [sn, st] of Object.entries(states)) {
    const key = sn === "array" ? "'0'" : "'p'";
    const containers = {
      defineProperty: `Object.defineProperty(x,${key},${d})`,
      defineProperties: `Object.defineProperties(x,{[${key}]:${d},q:{value:2,configurable:true}})`,
      reflect: `Reflect.defineProperty(x,${key},${d})`,
    };
    for (const [cn, call] of Object.entries(containers)) {
      add(`T(()=>{var L=[];var x=${st};var r;try{r=S(${call})}catch(e){r="throw "+e.name+": "+e.message}return [r,L.join(),Q(x),Z(x)].join(" || ")})`);
    }
  }
  add(`T(()=>{var L=[];var r;try{var o=Object.create({},{p:${d}});r=Z(o)}catch(e){r="throw "+e.name+": "+e.message}return [r,L.join()].join(" || ")})`);
}
const invalidDescriptors = ["undefined", "null", "1", "'s'", "true", "Symbol('s')", "10n", "()=>1", "[]", "{get:1}", "{set:'x'}", "{get(){},value:1}", "{get(){},writable:true}", "{set(v){},writable:false}", "{get:{}}", "{get:undefined,value:2}"];
for (const d of invalidDescriptors) {
  add(
    `T(()=>Object.defineProperty({},'p',${d}))`, `T(()=>Object.defineProperties({},{p:${d}}))`, `T(()=>Object.create({},{p:${d}}))`, `T(()=>Reflect.defineProperty({},'p',${d}))`,
    `T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},p:${d},c:{value:3}})}catch(e){}return Z(o)})`,
    `T(()=>{var o=Object.freeze({});return Object.defineProperty(o,'p',${d})})`, `T(()=>{var o=Object.freeze({p:1});return Object.defineProperty(o,'p',${d})})`,
    `T(()=>{var o=Object.seal({p:1});Object.defineProperty(o,'p',${d});return Z(o)})`,
  );
}

// ---- 3. getOwnPropertyDescriptors sobre cada alvo em cada estado.
for (const [tn, t] of Object.entries(targets)) {
  for (const [fn, fz] of Object.entries(freezers)) {
    const setup = `var L=[];var x=${t};var m="";` + (fz ? `try{${fz}}catch(e){m=e.name+": "+e.message}` : "");
    add(
      `T(()=>{${setup}return [m,S(Object.getOwnPropertyDescriptors(x)),L.join()].join(" || ")})`,
      `T(()=>{${setup}return [m,Object.entries(Object.getOwnPropertyDescriptors(x)).map(([k,d])=>String(k)+":"+["value" in d?"v":"a",d.writable,d.enumerable,d.configurable].join("/")).join(" "),Q(x),L.join()].join(" || ")})`,
      `T(()=>{${setup}var d=Object.getOwnPropertyDescriptors(x);var y=Object.defineProperties({},d);return [m,Z(y),Q(y),Reflect.ownKeys(d).length===Reflect.ownKeys(x).length].join(" || ")})`,
    );
  }
}

// ---- 4. setPrototypeOf com ciclos numa cadeia de quatro objetos, com o objeto alvo em três estados.
const protoApis = {
  object: "Object.setPrototypeOf(a[I],a[J])",
  reflect: "Reflect.setPrototypeOf(a[I],a[J])",
  assign: "(a[I].__proto__=a[J])",
  proxy: "Object.setPrototypeOf(a[I],new Proxy(a[J],{}))",
  proxyTrap: "Reflect.setPrototypeOf(a[I],new Proxy(a[J],{getPrototypeOf(t){return null}}))",
};
for (let i = 0; i < 4; i++) for (let j = 0; j < 4; j++) {
  for (const [pn, api] of Object.entries(protoApis)) {
    for (const [fn, fz] of [["none", ""], ["pe", "Object.preventExtensions(a[I])"], ["freeze", "Object.freeze(a[I])"]]) {
      const call = api.replace(/I/g, i).replace(/J/g, j);
      const pre = fz.replace(/I/g, i);
      add(
        `T(()=>{var a=[{}];for(var n=1;n<4;n++)a[n]=Object.create(a[n-1]);${pre ? pre + ";" : ""}var r;try{r=S(${call}===a[${i}])}catch(e){r="throw "+e.name+": "+e.message}return r+" || "+a.map(o=>a.indexOf(Object.getPrototypeOf(o))).join()})`,
      );
    }
  }
}
add(
  "T(()=>{var p=new Proxy({},{});var a=Object.create(p);return Reflect.setPrototypeOf(p,a)})", "T(()=>{var p=new Proxy({},{});var a=Object.create(p);return Object.setPrototypeOf(p,a)===p})",
  "T(()=>{var a={};return Object.setPrototypeOf(a,a)})", "T(()=>{var a={};a.__proto__=a})", "T(()=>{'use strict';var a={};a.__proto__=a})", "T(()=>{var a={};return Reflect.setPrototypeOf(a,a)})",
  "T(()=>Object.setPrototypeOf(Object.prototype,Object.create(Object.prototype)))", "T(()=>Object.setPrototypeOf(Function.prototype,Function))", "T(()=>{var f=function(){};return Object.setPrototypeOf(Object.prototype,f)})",
  "T(()=>{class A{}class B extends A{}return Object.setPrototypeOf(A,B)})", "T(()=>{class A{}class B extends A{}return Object.setPrototypeOf(A.prototype,B.prototype)})",
  "T(()=>{var a=[];return Object.setPrototypeOf(a,a)})", "T(()=>{var o=Object.create(null);return Object.setPrototypeOf(o,o)})", "T(()=>{var o=Object.create(null);return Object.setPrototypeOf(o,Object.prototype)===o})",
  "T(()=>{var f=Object.freeze(function(){});return Object.setPrototypeOf(f,Function.prototype)===f})", "T(()=>{var f=Object.freeze(function(){});return Object.setPrototypeOf(f,null)})",
  "T(()=>{var o=Object.seal({});return Object.setPrototypeOf(o,{})})", "T(()=>{var o=Object.seal({});return Object.setPrototypeOf(o,Object.prototype)===o})",
);

// ---- 5. isFrozen/isSealed/isExtensible/freeze/seal/preventExtensions sobre valores que não são objetos.
const primitives = ["undefined", "null", "1", "0", "NaN", "-0", "'s'", "''", "true", "Symbol('s')", "1n", "Symbol.iterator"];
for (const v of primitives) {
  for (const f of ["Object.freeze", "Object.seal", "Object.preventExtensions", "Object.isFrozen", "Object.isSealed", "Object.isExtensible", "Reflect.isExtensible", "Reflect.preventExtensions", "Object.getPrototypeOf", "Reflect.getPrototypeOf"]) {
    add(`T(()=>${f}(${v}))`);
  }
  add(`T(()=>Object.setPrototypeOf(${v},null))`, `T(()=>Object.setPrototypeOf(${v},{}))`, `T(()=>Reflect.setPrototypeOf(${v},null))`, `T(()=>Object.getOwnPropertyDescriptors(${v}))`, `T(()=>Object.defineProperty(${v},'p',{}))`);
}

// ---- 6. Pares de propriedades de espécies diferentes, antes e depois de freeze/seal/preventExtensions.
const propKinds = {
  writable: "{value:1,writable:true,configurable:true,enumerable:true}", readonly: "{value:1,writable:false,configurable:true,enumerable:true}",
  nonconfig: "{value:1,writable:true,configurable:false,enumerable:true}", locked: "{value:1,writable:false,configurable:false,enumerable:false}",
  accessor: "{get(){return 1},configurable:true}", accessorLocked: "{get(){return 1},configurable:false}", setterLocked: "{set(v){},configurable:false}", absent: null,
};
for (const [pn, pd] of Object.entries(propKinds)) {
  for (const [qn, qd] of Object.entries(propKinds)) {
    for (const [fn, fz] of Object.entries(freezers)) {
      const defs = (pd ? `Object.defineProperty(x,'p',${pd});` : "") + (qd ? `Object.defineProperty(x,'q',${qd});` : "");
      add(`T(()=>{var x={};${defs}${fz ? fz + ";" : ""}return [Q(x),D(x,'p'),D(x,'q')].join(" || ")})`);
    }
  }
}

// ---- 7. Arrays com elemento não configurável ou length não gravável, em modo frouxo e estrito.
const lengthOps = ["a.length=0", "a.length=1", "a.length=2", "a.length=5", "a.pop()", "a.push(9)", "a.splice(0,2)", "a.shift()"];
for (const idx of [0, 1, 2]) for (const lenWritable of [true, false]) for (const lop of lengthOps) for (const strict of [false, true]) {
  add(
    `T(()=>{${strict ? "'use strict';" : ""}var a=[1,2,3];Object.defineProperty(a,${idx},{configurable:false});` +
    (lenWritable ? "" : "Object.defineProperty(a,'length',{writable:false});") +
    `var r;try{r=S(${lop})}catch(e){r="throw "+e.name+": "+e.message}return [r,S(a),a.length,Q(a)].join(" || ")})`,
  );
}

// ---- 8. globalThis parcial: preventExtensions e seal no objeto global (a variável R já existe).
const globalOps = [
  "globalThis.zz=1", "(0,eval)('var qq=1')", "(0,eval)('function ff(){}')", "Object.defineProperty(globalThis,'zz',{value:1})", "Reflect.set(globalThis,'zz',1)",
  "delete globalThis.R", "delete globalThis.S", "Object.defineProperty(globalThis,'S',{value:1})", "Object.isExtensible(globalThis)", "Object.isSealed(globalThis)", "Object.isFrozen(globalThis)",
  "Function('zq=1')()", "Function('return typeof zq')()",
];
for (const fz of ["Object.preventExtensions(globalThis)", "Object.seal(globalThis)"]) {
  for (const op of globalOps) {
    for (const strict of [false, true]) {
      add(`T(()=>{${fz};var r;try{r=S((function(){${strict ? "'use strict';" : ""}return ${op}})())}catch(e){r="throw "+e.name+": "+e.message}return r+" || "+Q(globalThis)})`);
    }
  }
}
add(
  "T(()=>[D(globalThis,'NaN'),D(globalThis,'undefined'),D(globalThis,'Infinity')].join(' | '))", "T(()=>{'use strict';NaN=1})", "T(()=>{'use strict';undefined=1})", "T(()=>{'use strict';Infinity=1})",
  "T(()=>{NaN=1;return NaN})", "T(()=>{'use strict';delete globalThis.NaN})", "T(()=>delete globalThis.undefined)", "T(()=>Object.defineProperty(globalThis,'NaN',{value:1}))",
  "T(()=>Object.defineProperty(globalThis,'NaN',{value:NaN})===globalThis)", "T(()=>Reflect.defineProperty(globalThis,'undefined',{value:1}))",
);

// ---- 9. Dedup e execução paralela.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const baseSources = [];
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "freeze_grid_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const baseSet = new Set(baseSources);
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));

function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"]);
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 30000);
    child.stdout.on("data", d => { out += d; });
    child.stderr.on("data", d => { err += d; });
    child.on("close", code => { clearTimeout(timer); const decoded = code === 0 ? decodeResult(out) : null; resolve({ code: code === 0 && decoded === null ? -1 : code, out: decoded, err: code === 0 && decoded === null ? "filho sem resultado" : err }); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

async function main() {
  let dup = 0;
  const jobs = [];
  for (const expr of unique) {
    const source = "var R;\n" + PRELUDE + `R = ${expr};`;
    if (baseSet.has(source)) { dup++; continue; }
    jobs.push({ expr, source });
  }
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(2, os.cpus().length) }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i].source);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const { code, out, err } = results[i];
    if (code !== 0) { dropped++; process.stderr.write("filho falhou: " + JSON.stringify(jobs[i].expr).slice(0, 160) + " " + err.slice(0, 200) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(out)) { dropped++; process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(jobs[i].expr).slice(0, 160) + "\n"); continue; }
    if (sourceHasEmDash(jobs[i].source) || out.includes("\u2014") || out.includes("\u2013")) { dropped++; continue; }
    kept++;
    lines.push(JSON.stringify(jobs[i].source) + "\t" + JSON.stringify(out));
  }
  process.stdout.write(emitFactoredLines("freeze_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}

// Travessão literal nunca entra em fonte nem em resultado.
function sourceHasEmDash(text) {
  return text.includes("\u2014") || text.includes("\u2013");
}

main();
