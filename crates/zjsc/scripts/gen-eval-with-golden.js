// Gera tests/golden/eval_with_bun.tsv: eval direto e indireto em grade de contextos, declarações e conflitos dentro de
// eval, this/arguments/new.target/super em eval, with com Symbol.unscopables e Proxy com log, delete de bindings criados
// por eval, `new Function`/`Function`, GeneratorFunction/AsyncFunction e declarações globais, medido no bun 1.4.2.
// Todo código de escopo global roda dentro de `(0,eval)("...")` aninhado, para a semântica ser a mesma no filho (que
// avalia o programa por eval indireto) e no teste Rust (que o roda como script). Programas cujo texto já aparece em
// qualquer outro golden são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// O prelúdio comum sai em tests/golden/eval_with.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-eval-with-golden.js > tests/golden/eval_with_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'globalThis.R="";function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function J(a,b){return S(a)+"|"+S(b)}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = JSON.stringify;

// ---- Contextos: cada um recebe a chamada de eval (ev), a sonda posterior (af) e declarações prévias (pre).
const ctx = {
  fn: (ev, af, pre = "") => `(function(){${pre}return J(${ev},${af})})()`,
  sfn: (ev, af, pre = "") => `(function(){'use strict';${pre}return J(${ev},${af})})()`,
  arrow: (ev, af, pre = "") => `(()=>{${pre}return J(${ev},${af})})()`,
  sarrow: (ev, af, pre = "") => `(()=>{'use strict';${pre}return J(${ev},${af})})()`,
  method: (ev, af, pre = "") => `({m(){${pre}return J(${ev},${af})}}).m()`,
  cmeth: (ev, af, pre = "") => `new (class{m(){${pre}return J(${ev},${af})}})().m()`,
  smeth: (ev, af, pre = "") => `(class{static m(){${pre}return J(${ev},${af})}}).m()`,
  field: (ev, af) => `(new (class{f=J(${ev},${af})})).f`,
  sfield: (ev, af) => `(class{static f=J(${ev},${af})}).f`,
  param: (ev, af) => `((a=J(${ev},${af}))=>a)()`,
  fparam: (ev, af) => `(function(a=J(${ev},${af})){return a})()`,
  with: (ev, af, pre = "") => `(function(){${pre}with({w:1}){return J(${ev},${af})}})()`,
  catch: (ev, af, pre = "") => `(function(){${pre}try{throw 1}catch(c){return J(${ev},${af})}})()`,
  block: (ev, af, pre = "") => `(function(){${pre}{let b=1;return J(${ev},${af})}})()`,
  global: (ev, af, pre = "") => `(0,eval)(${q(`${pre}J(${ev},${af})`)})`,
  sglobal: (ev, af, pre = "") => `(0,eval)(${q(`'use strict';${pre}J(${ev},${af})`)})`,
};
const ctxNames = Object.keys(ctx);
const prelessCtx = new Set(["field", "sfield", "param", "fparam"]);

const PR = "[typeof v,typeof f,typeof w,typeof a,typeof i,typeof x,typeof b].join()";
const X = (src) => `(function(){try{return eval(${q(src)})}catch(e){return e.name+": "+e.message}})()`;

// ---- 1. Declarações dentro de eval, direto, em cada contexto, com duas sondas.
const payloads = [
  "var v=1", "var v", "var v=1;var v=2", "function f(){return 1}", "let v=1", "const v=1", "class v{}", "var v=1;let w=2",
  "{function f(){}}", "var [a,b]=[1,2]", "var {a}={a:1}", "for(var i=0;i<1;i++);", "function f(){};var f", "var f;function f(){}",
  "let v;var v", "'use strict';var v=1", "'use strict';function f(){}", "var arguments", "function arguments(){}", "var eval",
  "var undefined", "var NaN", "function NaN(){}", "this===undefined", "typeof this", "arguments.length", "typeof arguments",
  "new.target", "super.x", "super()", "x=1", "var x=typeof x", "var v=1,w=v+1", "if(1){function f(){}}", "try{throw 1}catch(v){var v=2}",
  "let v=1;{var w=v}", "label:var v=1", "var v=function(){return 1}", "function v(){}", "var v=this", "1;var v=2", "x=>x",
  "'use strict';x=1", "'use strict';delete v", "var v;delete v", "switch(1){case 1:function f(){}}", "for(let v of [1]);", "var v=1;function v(){}",
];
for (const c of ctxNames) for (const p of payloads) {
  add(`T(()=>${ctx[c](`eval(${q(p)})`, PR)})`);
  if (!prelessCtx.has(c) || c === "param") add(`T(()=>${ctx[c](`eval(${q(p)})`, X("delete v"))})`);
}

// ---- 2. Conflitos com declarações prévias (SyntaxError exatos).
const pres = ["let v=0;", "var v=0;", "const v=0;", "function v(){};", "var v;", "let f=0;", "class v{};", "let w;", "var w;", "function f(){};"];
const conflictPayloads = ["var v", "var v=1", "function v(){}", "let v", "var f", "function f(){}", "var w", "let w", "{var v}", "{function v(){}}", "for(var v of []);", "class v{}"];
for (const c of ["fn", "sfn", "block", "catch", "arrow", "global", "sglobal", "with", "method"]) for (const pre of pres) for (const p of conflictPayloads) {
  add(`T(()=>${ctx[c](`eval(${q(p)})`, "typeof v+typeof f+typeof w", pre)})`);
}
// Parâmetros, catch com padrões e arguments.
for (const p of conflictPayloads) {
  add(
    `T(()=>(function(v){return J(eval(${q(p)}),typeof v)})(5))`, `T(()=>(function(v=1){return J(eval(${q(p)}),typeof v)})(5))`,
    `T(()=>(function(...v){return J(eval(${q(p)}),typeof v)})(5))`, `T(()=>(function({v}){return J(eval(${q(p)}),typeof v)})({v:5}))`,
    `T(()=>(()=>{try{throw 1}catch(v){return J(eval(${q(p)}),typeof v)}})())`, `T(()=>(()=>{try{throw [1]}catch([v]){return J(eval(${q(p)}),typeof v)}})())`,
    `T(()=>(()=>{try{throw {v:1}}catch({v}){return J(eval(${q(p)}),typeof v)}})())`, `T(()=>(function(){'use strict';let v;return J(eval(${q(p)}),typeof v)})())`,
    `T(()=>(function(a=eval(${q(p)}),b=typeof v){return J(a,b)})())`, `T(()=>(function(v,a=eval(${q(p)})){return J(a,typeof v)})(1))`,
    `T(()=>(function(){return J(eval(${q(p)}),typeof v)}).call(1))`, `T(()=>{for(let v=0;v<1;v++){return J(eval(${q(p)}),typeof v)}})`,
    `T(()=>{for(const v of [1]){return J(eval(${q(p)}),typeof v)}})`, `T(()=>{switch(1){case 1:let v=0;return J(eval(${q(p)}),typeof v)}})`,
    `T(()=>{label:{let v=0;return J(eval(${q(p)}),typeof v)}})`, `T(()=>{try{let v=0;return J(eval(${q(p)}),typeof v)}finally{}})`,
  );
}

// ---- 3. Formas de chamar eval: direto, indireto e variações.
const forms = {
  direct: (p) => `eval(${q(p)})`, paren: (p) => `(eval)(${q(p)})`, comma: (p) => `(0,eval)(${q(p)})`, alias: (p) => `(function(){var e=eval;return e(${q(p)})})()`,
  global: (p) => `globalThis.eval(${q(p)})`, optional: (p) => `eval?.(${q(p)})`, call: (p) => `eval.call(null,${q(p)})`, apply: (p) => `eval.apply(this,[${q(p)}])`,
  spread: (p) => `eval(...[${q(p)}])`, extra: (p) => `eval(${q(p)},"unused")`, noargs: () => `eval()`, nonstring: (p) => `eval(${p.length})`,
  object: () => `eval({toString(){return "1"}})`, strobj: () => `eval(new String("1+1"))`, assigned: (p) => `(eval=eval)(${q(p)})`,
  cond: (p) => `(true?eval:0)(${q(p)})`, shadow: (p) => `(function(eval){return eval(${q(p)})})(function(s){return "shadow:"+s})`,
  rebound: (p) => `(function(){var eval=globalThis.eval;return eval(${q(p)})})()`, bound: (p) => `eval.bind(null)(${q(p)})`, reflect: (p) => `Reflect.apply(eval,undefined,[${q(p)}])`,
  newcall: (p) => `new eval(${q(p)})`,
};
const formPayloads = ["var v=7;v", "typeof v", "this===globalThis", "typeof arguments", "typeof new.target", "let v=3;v", "(function(){return typeof v})()", "typeof f", "'use strict';this", "typeof w", "function f(){};typeof f", "x=1;typeof x", "typeof b", "1;;;", "({})", "{}", "{a:1}", "1\n2", "", "'x'"];
for (const c of ["fn", "sfn", "arrow", "with", "block", "method", "catch"]) for (const [fname, form] of Object.entries(forms)) for (const p of formPayloads) {
  if (["noargs", "nonstring", "object", "strobj"].includes(fname) && p !== formPayloads[0]) continue;
  add(`T(()=>${ctx[c](form(p), PR)})`);
}
// Valores de retorno e ToString do argumento.
for (const [src, label] of [["1+1", ""], ["if(1){2}", ""], ["var q=3", ""], ["do{4;break}while(0)", ""], ["5;if(0){6}", ""], ["7;try{8}finally{9}", ""], ["10;function z(){}", ""], ["11;class K{}", ""], ["12;let m=1", ""], ["switch(1){case 1:13}", ""], ["14;;", ""], ["for(var k=0;k<2;k++)k", ""], ["15;label:{16;break label}", ""], ["17;with({}){}", ""], ["18;throw 1", ""], ["'a';'b'", ""], ["({}).x", ""], ["a=>a", ""], ["`t${1}`", ""], ["19;do{20;continue}while(0)", ""]]) {
  add(`T(()=>eval(${q(src)}))`, `T(()=>(0,eval)(${q(src)}))`, `T(()=>(function(){'use strict';return eval(${q(src)})})())`);
}

// ---- 4. this, arguments, new.target e super em eval.
const specials = ["this", "arguments", "arguments.length", "arguments[0]", "new.target", "typeof new.target", "super.m", "super.m()", "super.x", "super()", "super.constructor", "(()=>this)()", "(()=>arguments)()", "(()=>new.target)()", "(()=>super.m)()", "(function(){return typeof arguments})()", "(function(){return this})()", "(function(){'use strict';return this})()", "(function(){return new.target})()", "(function(){return super.x})", "class K{[super.m]=1}", "({m(){return super.x}}).m()", "({__proto__:{x:5},m(){return super.x}}).m()", "this.constructor.name", "typeof this", "({a:this}).a", "new.target===undefined", "[this,arguments.length]", "({[this]:1})", "`${typeof this}`"];
const specialCtx = {
  fn: (e) => `(function(){return ${e}}).call("t",1,2)`, sfn: (e) => `(function(){'use strict';return ${e}}).call("t",1,2)`, arrowOuter: (e) => `(function(){return (()=>${e})()}).call("t",1,2)`,
  newfn: (e) => `new (function F(){this.r=${e}})`, newcall: (e) => `Reflect.construct(function(){return ${e}},[1],Array)`, method: (e) => `({__proto__:{m(){return "pm"},x:"px"},m(){return ${e}}}).m(1,2)`,
  cmeth: (e) => `new (class B{m(){return "bm"}}).constructor`, ctorDerived: (e) => `new (class A{m(){return "am"}static x=1},class extends Object{constructor(){var r=${e};super();this.r=r}})`,
  cmethBase: (e) => `(new (class A{m(){return "am"};n(){return ${e}}})).n(1,2)`, cderived: (e) => `(new (class extends (class A{m(){return "am"}}){n(){return ${e}}})).n(1,2)`,
  smeth: (e) => `(class A{static m(){return "sm"} static n(){return ${e}}}).n(1,2)`, field: (e) => `(new (class{f=${e}})).f`, sfield: (e) => `(class{static f=${e}}).f`,
  param: (e) => `(function(a=${e}){return a}).call("t",undefined)`, getter: (e) => `({get g(){return ${e}}}).g`, setter: (e) => `({set s(v){R2=${e}}}).s=1`,
  gen: (e) => `[...(function*(){yield ${e}}).call("t",1,2)]`, asyncfn: (e) => `(async function(){return ${e}}).call("t",1,2)===undefined`,
  staticBlock: (e) => `(class{static r;static{this.r=${e}}}).r`, arrowGlobal: (e) => `(()=>${e})()`, topthis: (e) => `${e}`,
};
for (const sp of specials) for (const [cn, wrap] of Object.entries(specialCtx)) {
  if (cn === "asyncfn" || cn === "cmeth") continue;
  const inner = `eval(${q(sp)})`;
  add(`T(()=>${wrap(inner)})`);
  if (["fn", "sfn", "method", "cmethBase", "cderived", "field", "param"].includes(cn)) add(`T(()=>${wrap(`(0,eval)(${q(sp)})`)})`);
}

// ---- 5. with + Symbol.unscopables + Proxy com log.
const withTargets = {
  plain: "{x:1}", unsF: "{x:1,[Symbol.unscopables]:{x:false}}", unsT: "{x:1,[Symbol.unscopables]:{x:true}}", unsTruthy: "{x:1,[Symbol.unscopables]:{x:'yes'}}",
  unsOther: "{x:1,[Symbol.unscopables]:{y:true}}", unsNum: "{x:1,[Symbol.unscopables]:1}", unsNull: "{x:1,[Symbol.unscopables]:null}", unsStr: "{x:1,[Symbol.unscopables]:'x'}",
  unsFn: "{x:1,[Symbol.unscopables]:function(){}}", unsArr: "{x:1,[Symbol.unscopables]:['x']}", unsProto: "{x:1,[Symbol.unscopables]:Object.create({x:true})}",
  unsGet: "{x:1,get [Symbol.unscopables](){L.push('getU');return {x:true}}}", proto: "Object.create({x:1})", protoUns: "Object.create({x:1,[Symbol.unscopables]:{x:true}})",
  accessor: "{get x(){L.push('getX');return 1},set x(v){L.push('setX '+v)}}", frozen: "Object.freeze({x:1})", nonwritable: "Object.defineProperty({},'x',{value:1})",
  arr: "[1,2]", str: "'xyz'", num: "5", bool: "true", sym: "Symbol('s')", fnobj: "function x(){}", date: "new Date(0)", map: "new Map", global: "globalThis", nullp: "Object.create(null,{x:{value:1,writable:true}})",
};
const withOps = ["x", "typeof x", "x=5", "x++", "x+=1", "x()", "delete x", "var x=7", "var x", "x&&1", "y", "typeof y", "y=3", "(function(){return x})()", "eval('x')", "x?.y", "length", "typeof length", "toFixed", "typeof toFixed", "toString===Object.prototype.toString", "keys", "typeof values", "[x]=[9]", "({x}={x:2})", "for(x of [3]);", "for(x in {a:1});", "x||=3", "x??=3", "x**=2", "(()=>x)()", "typeof (()=>x)", "x`t`", "new x", "function g(){return x};g()", "this===O", "(function(){return this})()===O", "x.y", "(0,x)", "eval('var x=4')", "eval('x=6')", "eval('function x(){}')"];
for (const [tn, t] of Object.entries(withTargets)) for (const op of withOps) {
  add(`T(()=>{var L=[];var O=${t};var r=(function(){with(O){return ${op === "var x=7" || op === "var x" || op === "function g(){return x};g()" || op.startsWith("for") ? `(function(){${op};return typeof x})()` : op}}}).call(O);return S(r)+"|"+L.join()+"|"+(O!=null&&typeof O==="object"&&!(O instanceof Date)&&!(O instanceof Map)?D(O,"x"):"")})`);
}
const ptraps = {
  all: "has(t,k){L.push('has '+String(k));return Reflect.has(t,k)},get(t,k,r){L.push('get '+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){L.push('set '+String(k)+' '+v);return Reflect.set(t,k,v,r)},deleteProperty(t,k){L.push('delete '+String(k));return Reflect.deleteProperty(t,k)},getOwnPropertyDescriptor(t,k){L.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},defineProperty(t,k,d){L.push('define '+String(k));return Reflect.defineProperty(t,k,d)}",
  hasOnly: "has(t,k){L.push('has '+String(k));return Reflect.has(t,k)}",
  hasTrue: "has(t,k){L.push('has '+String(k));return true},get(t,k){L.push('get '+String(k));return k===Symbol.unscopables?undefined:42}",
  unsTrap: "has(t,k){L.push('has '+String(k));return true},get(t,k,r){L.push('get '+String(k));return k===Symbol.unscopables?{x:true}:Reflect.get(t,k,r)}",
};
const ptargets = ["{x:1}", "{}", "{x:{y:1}}", "{x(){return this===P}}"];
const pops = ["x", "typeof x", "x=5", "x++", "x+=1", "x()", "delete x", "(function(){var x=7;return x})()", "x&&1", "y", "typeof y", "y=3", "(function(){return x})()", "eval('x')", "x?.y", "[x]=[1]", "({x}={x:1})", "for(x of [1]);", "x||=3", "x??=3", "x**=2", "x`t`", "eval('var x=2')", "eval('function x(){}')", "eval('x=3')", "typeof eval", "x in {}", "x instanceof Object"];
for (const [tn, traps] of Object.entries(ptraps)) for (const t of ptargets) for (const op of pops) {
  add(`T(()=>{var L=[];var P=new Proxy(${t},{${traps}});var r;try{r=S((function(){with(P){return ${op}}})())}catch(e){r="throw "+e.name+": "+e.message}return r+"|"+L.join()})`);
}
// `delete x` em with sobre Proxy: has, get de Symbol.unscopables e deleteProperty na ordem, trap que devolve falso, que
// lança, que devolve verdadeiro sobre propriedade não configurável, e has/get lançando antes do delete.
const dlog = (n) => `${n}(t,k,r){L.push('${n} '+String(k));return Reflect.${n === "get" ? "get(t,k,r)" : n + "(t,k)"}}`;
const dtraps = {
  base: [dlog("has"), dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));return Reflect.deleteProperty(t,k)}", "getOwnPropertyDescriptor(t,k){L.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}"],
  delFalse: [dlog("has"), dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));return false}"],
  delFalsy0: [dlog("has"), dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));return 0}"],
  delTrue: [dlog("has"), dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));return true}"],
  delThrow: [dlog("has"), dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));throw new RangeError('d')}"],
  hasThrow: ["has(t,k){L.push('has '+String(k));throw new RangeError('h')}", dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));return true}"],
  getThrow: [dlog("has"), "get(t,k){L.push('get '+String(k));throw new RangeError('g')}", "deleteProperty(t,k){L.push('delete '+String(k));return true}"],
  unsAll: [dlog("has"), "get(t,k,r){L.push('get '+String(k));return k===Symbol.unscopables?{x:true}:Reflect.get(t,k,r)}", "deleteProperty(t,k){L.push('delete '+String(k));return true}"],
  unsOther: [dlog("has"), "get(t,k,r){L.push('get '+String(k));return k===Symbol.unscopables?{y:true}:Reflect.get(t,k,r)}", "deleteProperty(t,k){L.push('delete '+String(k));return true}"],
  hasFalse: ["has(t,k){L.push('has '+String(k));return false}", "deleteProperty(t,k){L.push('delete '+String(k));return true}"],
  hasTrueAbsent: ["has(t,k){L.push('has '+String(k));return true}", dlog("get"), "deleteProperty(t,k){L.push('delete '+String(k));return Reflect.deleteProperty(t,k)}"],
  noDelTrap: [dlog("has"), dlog("get")],
};
const dtargets = ["{x:1}", "{}", "Object.defineProperty({},'x',{value:1})", "Object.defineProperty({},'x',{value:1,configurable:true})", "Object.preventExtensions({x:1})", "Object.freeze({x:1})", "[1]", "function x(){}"];
for (const [tn, traps] of Object.entries(dtraps)) for (const t of dtargets) for (const op of ["delete x", "typeof delete x", "[delete x, typeof x]", "delete x,x", "delete (x)", "(function(){return delete x})()", "eval('delete x')", "(()=>delete x)()"]) {
  add(`T(()=>{var L=[];var P=new Proxy(${t},{${traps.join(",")}});var r;try{r=S((function(){with(P){return ${op}}})())}catch(e){r="throw "+e.name+": "+e.message}return r+"|"+L.join()})`);
}
// Array.prototype[Symbol.unscopables] e with sobre primitivos.
const unsNames = ["at", "copyWithin", "entries", "fill", "find", "findIndex", "findLast", "findLastIndex", "flat", "flatMap", "includes", "keys", "toReversed", "toSorted", "toSpliced", "values", "push", "map", "length", "concat", "group", "groupBy", "with", "indexOf", "toString", "forEach", "constructor"];
for (const n of unsNames) add(`T(()=>(function(){with([]){return typeof ${n}}})())`, `T(()=>(function(){with([1,2]){return typeof ${n}}})())`, `T(()=>Array.prototype[Symbol.unscopables][${q(n)}])`, `T(()=>(function(){with(new Uint8Array(1)){return typeof ${n}}})())`);
add(
  "T(()=>Object.keys(Array.prototype[Symbol.unscopables]).join())", "T(()=>Object.getPrototypeOf(Array.prototype[Symbol.unscopables]))", "T(()=>D(Array.prototype,Symbol.unscopables))",
  "T(()=>Reflect.ownKeys(Array.prototype[Symbol.unscopables]).join())", "T(()=>Object.getOwnPropertyDescriptor(Array.prototype[Symbol.unscopables],'at'))",
  "T(()=>(function(){with(null){}})())", "T(()=>(function(){with(undefined){}})())", "T(()=>(function(){with(1){return toFixed.length}})())", "T(()=>(function(){with('abc'){return length+charAt(1)}})())",
  "T(()=>(function(){with(Symbol('s')){return description}})())", "T(()=>(function(){with(10n){return typeof toString}})())", "T(()=>(function(){with({}){return typeof hasOwnProperty}})())",
  "T(()=>(function(){with(Object.create(null)){return typeof hasOwnProperty}})())", "T(()=>(function(){var o={get [Symbol.unscopables](){throw new RangeError('u')},x:1};with(o){return x}})())",
  "T(()=>(function(){var o={get x(){throw new RangeError('gx')}};with(o){return typeof x}})())", "T(()=>(function(){var o={toString(){throw new RangeError('ts')}};with(o){return 1}})())",
  "T(()=>(function(){with({a:1}){with({b:2}){return a+b}}})())", "T(()=>(function(){with({a:1}){with({a:2}){return a}}})())", "T(()=>(function(){var a=0;with({a:1}){return a}})())", "T(()=>(function(){var a=0;with({}){a=5}return a})())",
  "T(()=>(function(){with({a:1}){var a=9}return a})())", "T(()=>(function(){var o={a:1};with(o){var a=9}return o.a+','+a})())", "T(()=>(function(){var o={};with(o){var a=9}return o.a+','+a})())",
  "T(()=>(function(){var o={a:1};with(o){function a(){}}return typeof o.a+typeof a})())", "T(()=>(function(){var o={a:1};with(o){delete a}return 'a' in o})())", "T(()=>(function(){var o={a:1};with(o){a=2;delete o.a;a=3}return JSON.stringify(o)+typeof a})())",
  "T(()=>(function(){var o={a:1};with(o){return (function(){o={};return a})()}})())", "T(()=>(function(){var o={a:1};with(o){return (()=>{delete o.a;return typeof a})()}})())", "T(()=>(function(){var o={a:1};with(o){return (()=>{o.a=7;return a})()}})())",
  "T(()=>(function(){'use strict';with({}){}})())", "T(()=>eval('\"use strict\";with({}){}'))", "T(()=>new Function('\"use strict\";with({}){}'))", "T(()=>(function(){with({}){'use strict';return this===undefined}})())",
  "T(()=>(function(){with({f(){return this}}){return f()===undefined}})())", "T(()=>(function(){var o={f(){return this}};with(o){return f()===o}}).call())", "T(()=>(function(){var o={f(){return this}};with(o){return (0,f)()===o}}).call())",
  "T(()=>(function(){var o={f(){return this}};with(o){return (f)()===o}}).call())", "T(()=>(function(){var o={f(){return this===globalThis}};with(o){return eval('f()')}}).call())", "T(()=>(function(){var o={f(){return typeof this}};with(o){return f.call(1)}}).call())",
  "T(()=>(function(){var o={x:1};with(o){return [typeof x,delete x,typeof x]}}).call())", "T(()=>(function(){var o=Object.defineProperty({},'x',{value:1,configurable:true});with(o){x=2;return o.x}}).call())",
  "T(()=>(function(){'use strict';var o=Object.defineProperty({},'x',{value:1});return eval('with(o){x=2}')}).call())", "T(()=>(function(){var o=Object.defineProperty({},'x',{value:1});with(o){x=2;return o.x}}).call())",
  "T(()=>(function(){var o=Object.defineProperty({},'x',{value:1});with(o){return (function(){'use strict';x=2;return o.x})()}}).call())", "T(()=>(function(){var o=Object.freeze({x:1});with(o){return (function(){'use strict';x=2})()}}).call())",
  "T(()=>(function(){var o={};with(o){return (function(){'use strict';z1=2})()}}).call())", "T(()=>(function(){with({}){z2=2}return typeof z2})())", "T(()=>(function(){var L=[];var o=new Proxy({},{has(t,k){L.push(String(k));return false}});with(o){z3=1}return L.join()})())",
  "T(()=>(function(){var L=[];var o=new Proxy({x:1},{has(t,k){L.push('has '+String(k));return true},get(t,k){L.push('get '+String(k));return t[k]},set(t,k,v){L.push('set '+String(k));t[k]=v;return false}});with(o){x=2}return L.join()})())",
  "T(()=>(function(){'use strict';var L=[];var o=new Proxy({x:1},{has(t,k){L.push('has '+String(k));return true},get(t,k){L.push('get '+String(k));return t[k]},set(t,k,v){L.push('set '+String(k));return false}});return eval('with(o){x=2}')})())",
);

// Função NATIVA chamada dentro de `with`: o `resolve_scope` devolve o objeto do `with` (`JSScope::objectAtScope`) e
// o `call` o recebe como `this`, então `join()`, `valueOf()` e `size` agem sobre o objeto, não sobre o escopo.
add(
  "T(()=>(function(){with([1,2]){return join('-')}})())", "T(()=>(function(){with({valueOf:Object.prototype.valueOf,x:1}){return valueOf().x}})())",
  "T(()=>(function(){with(new Map([[1,2]])){return size}})())", "T(()=>(function(){with(new Map([[1,2]])){return get(1)}})())",
  "T(()=>(function(){with(new Set([1,2])){return has(2)+','+size}})())", "T(()=>(function(){var o={x:1};with(o){return valueOf()===o}})())",
  "T(()=>(function(){var o={x:1};with(o){return toString()}})())", "T(()=>(function(){var o={x:1};with(o){return hasOwnProperty('x')}})())",
  "T(()=>(function(){var o={x:1};with(o){return isPrototypeOf(Object.create(o))}})())", "T(()=>(function(){with('abc'){return charAt(1)+toUpperCase()}})())",
  "T(()=>(function(){with(5){return toFixed(2)}})())", "T(()=>(function(){with([3,1,2]){return sort().join()+indexOf(2)+slice(1)}})())",
  "T(()=>(function(){var d=new Date(0);with(d){return getTime()}})())", "T(()=>(function(){with(/a/g){return test('a')+','+lastIndex}})())",
  "T(()=>(function(){'use strict';return eval('with([1,2]){join(\"+\")}')})())", "T(()=>(function(){with([1,2]){return (function(){return join('/')})()}})())",
);

// ---- 6. delete de bindings criados por eval.
const deletePre = ["", "var v=0;", "let v=0;", "function v(){};"];
const deleteOps = [
  "eval('var v=1');delete v", "eval('var v=1');delete v;typeof v", "eval('function v(){}');delete v", "eval('let v=1');delete v", "eval('v=1');delete v", "eval('var v=1');var r=[delete v,delete v,typeof v];r.join()",
  "eval('var v=1;var w=2');[delete v,delete w,typeof v+typeof w].join()", "eval('var v=1');eval('delete v');typeof v", "eval('var v=1');eval('delete v')", "eval('var v=1');eval('\\'use strict\\';delete v')",
  "eval('var v=1');(function(){return delete v})()", "eval('var v=1');(function(){'use strict';return delete v})", "eval('var v=1');this.v", "eval('var v=1');delete this.v", "eval('var v=1');Object.keys(this).join()",
  "eval('var v=1');eval('var v=2');delete v", "eval('var v=1');v=5;delete v;typeof v", "eval('var v=1');eval('var v;v');", "eval('var v=1');(0,eval)('delete v')", "eval('var v=1');delete eval('v')",
  "eval('var v=1');delete (v)", "eval('var v=1');delete ((v))", "eval('var v=1');delete (0,v)", "eval('var v=1');delete [v]", "eval('var v=1');delete v.x", "eval('var v=1');delete 1", "delete undefined", "delete NaN", "delete arguments", "delete eval", "typeof delete v",
  "delete globalThis", "delete this", "delete v", "delete x", "(function(a){return delete a})(1)", "(function(){var z;return delete z})()", "(function(){function z(){}return delete z})()", "(function(){return delete arguments})()", "(function(){return eval('delete arguments')})()",
  "eval('var arguments=1');delete arguments", "eval('function arguments(){}');typeof arguments", "eval('x1=1');delete x1", "eval('x1=1');(0,eval)('delete x1')", "(0,eval)('var x2=1;delete x2')", "(0,eval)('var x2=1');delete x2",
  "(0,eval)('var x2=1');D(globalThis,'x2')", "(0,eval)('function x2(){}');D(globalThis,'x2')", "(0,eval)('let x2=1');D(globalThis,'x2')", "(0,eval)('x2=1');D(globalThis,'x2')", "(0,eval)('var x2=1');(0,eval)('delete x2');typeof x2",
];
for (const pre of deletePre) for (const op of deleteOps) for (const c of ["fn", "arrow", "block", "catch", "with"]) {
  if (op.includes("'use strict'") && c !== "fn") continue;
  const wrapped = { fn: `(function(){${pre}${op.replace(/(.*);([^;]*)$/, "$1;return $2")}})()`, arrow: `(()=>{${pre}${op.replace(/(.*);([^;]*)$/, "$1;return $2")}})()`, block: `(function(){${pre}{let b=1;${op.replace(/(.*);([^;]*)$/, "$1;return $2")}}})()`, catch: `(function(){${pre}try{throw 1}catch(c){${op.replace(/(.*);([^;]*)$/, "$1;return $2")}}})()`, with: `(function(){${pre}with({w:1}){${op.replace(/(.*);([^;]*)$/, "$1;return $2")}}})()` }[c];
  add(`T(()=>${wrapped.replace(/return ([a-z]+)\)/g, "return $1)")})`);
}

// ---- 7. new Function, Function, GeneratorFunction, AsyncFunction.
const GF = "Object.getPrototypeOf(function*(){}).constructor";
const AF = "Object.getPrototypeOf(async function(){}).constructor";
const AGF = "Object.getPrototypeOf(async function*(){}).constructor";
const fctors = { Function: "Function", Generator: GF, Async: AF, AsyncGen: AGF };
const argLists = [
  [], [""], ["return 1"], ["a", "return a"], ["a,b", "return a+b"], ["a", "b", "return a+b"], ["a,a", ""], ["a", "a", ""], ["a=1", "return a"], ["...a", "return a"], ["[a]", "return a"], ["{a}", "return a"], ["a,", "return a"], ["a /*", "*/ return a"],
  ["/*", "*/"], ["a //", "return a"], ["", "})("], ["", "}) , function(){"], ["", "//"], ["", "-->x"], ["", "<!--x"], ["a", "'use strict';return a"], ["a=1", "'use strict'"], ["{a}", "'use strict'"], ["...a", "'use strict'"], ["a,a", "'use strict'"],
  ["eval", "'use strict'"], ["arguments", "'use strict'"], ["yield", ""], ["await", ""], ["let", ""], ["static", "'use strict'"], ["eval", ""], ["arguments", ""], ["1", ""], ["a b", ""], ["a;b", ""], ["a,b,c,d", "return d"], ["", "return this"], ["", "'use strict';return this"],
  ["", "return typeof arguments"], ["", "return arguments.length"], ["", "return new.target"], ["", "return super.x"], ["", "super()"], ["", "yield 1"], ["", "yield* [1,2]"], ["", "await 1"], ["", "return await 1"], ["", "for await(x of []);"], ["", "var yield=1"], ["", "var await=1"],
  ["", "let a;var a"], ["", "return typeof f;function f(){}"], ["", "return typeof x"], ["", "return typeof Function"], ["", "x=1;return typeof x"], ["", "var x=1;return x"], ["", "label:;"], ["", "return;"], ["", "return 1;;"], ["", "debugger"], ["", "if(1)"], ["", "{"], ["", "}"], ["", "a=>"],
  ["", "class A{}"], ["", "class A extends B{constructor(){super()}}"], ["", "import('x')"], ["", "import.meta"], ["", "import x from 'y'"], ["", "export var x"], ["a", "var a"], ["a", "let a"], ["a", "function a(){}"], ["a=1", "var a"], ["", "\n"], ["a\n", "return a"], ["a\n,b", "return b"],
  ["a", "return a\n}\n{"], ["", "/*"], ["", "'"], ["", "`"], ["", "\\u{110000}"], ["", "return '\\u2028'"], ["", "return 0o8"], ["", "return 1_0"], ["", "return 08"], ["", "'use strict';return 08"], ["", "'use strict';return 0o7"], ["", "'\\08'"], ["", "'use strict';'\\08'"],
  ["x", "return x?.y"], ["x", "return x??1"], ["x", "x||=1;return x"], ["x", "return #x in x"], ["", "return 1n"], ["", "return /(?<n>a)/.exec('a').groups.n"], ["", "return globalThis===this"], ["a=this", "return a"], ["a=arguments", "return typeof a"], ["a=new.target", "return a"],
  ["a", "return arguments.length"], ["a,b", "return arguments.length"], ["a", "arguments[0]=9;return a"], ["a", "'use strict';arguments[0]=9;return a"], ["a=0", "arguments[0]=9;return a"], ["", "return function(){return this}()"], ["", "return (()=>this)()"],
];
for (const [fname, ctor] of Object.entries(fctors)) for (const args of argLists) {
  const call = `${ctor}(${args.map(q).join(",")})`;
  add(`T(()=>${call})`, `T(()=>(${call}).toString())`, `T(()=>new ${ctor}(${args.map(q).join(",")}).toString())`);
  if (fname === "Function") add(`T(()=>${call}(1,2,3))`, `T(()=>new ${call}(1,2))`, `T(()=>{var f=${call};return f.length+","+f.name+","+Object.getOwnPropertyNames(f).join()})`, `T(()=>{var f=${call};return typeof f.prototype})`, `T(()=>${call}.call("s",4,5))`);
  if (fname === "Generator") add(`T(()=>[...${call}(1,2,3)])`, `T(()=>{var f=${call};return f.length+","+f.name+","+Object.getOwnPropertyNames(f).join()+","+typeof f.prototype})`, `T(()=>Object.prototype.toString.call(${call}))`, `T(()=>{var g=${call}(1);return Object.prototype.toString.call(g)+","+typeof g.next})`);
  if (fname === "Async") add(`T(()=>{var f=${call};return f.length+","+f.name+","+Object.getOwnPropertyNames(f).join()+","+typeof f.prototype})`, `T(()=>${call}(1) instanceof Promise)`, `T(()=>Object.prototype.toString.call(${call}))`);
  if (fname === "AsyncGen") add(`T(()=>{var f=${call};return f.length+","+f.name+","+Object.getOwnPropertyNames(f).join()+","+typeof f.prototype})`, `T(()=>Object.prototype.toString.call(${call}))`, `T(()=>Object.prototype.toString.call(${call}(1)))`);
}
add(
  "T(()=>Function.length)", "T(()=>Function.name)", "T(()=>Object.getOwnPropertyNames(Function).join())", "T(()=>Function.prototype.constructor===Function)", "T(()=>Function('return this')()===globalThis)", "T(()=>new Function('return this')()===globalThis)",
  "T(()=>Function('\"use strict\";return this')())", "T(()=>(function(){var x=1;return Function('return typeof x')()})())", "T(()=>(function(){var x=1;return (0,eval)('typeof x')})())", "T(()=>(function(){var x=1;return eval('typeof x')})())",
  "T(()=>Function('a','b','return a+b').toString())", "T(()=>Function('a,b','return a+b').toString())", "T(()=>Function().toString())", "T(()=>Function('').toString())", "T(()=>Function('a\\n','').toString())", "T(()=>Function('//','').toString())",
  "T(()=>Function('a','/*','*/').toString())", "T(()=>Function({toString(){return 'a'}},{toString(){return 'return a'}})(5))", "T(()=>Function({toString(){throw new RangeError('p')}},{toString(){throw new EvalError('b')}}))", "T(()=>Function('a',{toString(){throw new EvalError('b')}}))",
  "T(()=>{var L=[];try{Function({toString(){L.push('p1');return 'a'}},{toString(){L.push('p2');return 'b'}},{toString(){L.push('body');return '1'}})}catch(e){L.push(e.name)}return L.join()})", "T(()=>Function(null,undefined,'return 1'))", "T(()=>Function(1,2,'return 1'))",
  "T(()=>Function(Symbol()))", "T(()=>Function('a',Symbol()))", "T(()=>Function(...['a','b','return a*b'])(3,4))", "T(()=>Function.call(null,'a','return a')(7))", "T(()=>Function.apply(null,['a','return a'])(8))", "T(()=>Reflect.construct(Function,['return new.target'],Array)())",
  "T(()=>Object.getPrototypeOf(Reflect.construct(Function,['return 1'],Array))===Array.prototype)", "T(()=>Object.getPrototypeOf(Reflect.construct(Function,['return 1'],Object))===Object.prototype)", "T(()=>Object.getPrototypeOf(Reflect.construct(Function,['return 1'],class{}))===Function.prototype)",
  "T(()=>{class F extends Function{};var f=new F('return 1');return f()+','+(f instanceof F)+','+Object.getPrototypeOf(f)===F.prototype})", "T(()=>{class F extends Function{};var f=new F('a','return a');return f(3)+','+(f instanceof F)})",
  "T(()=>{class F extends Function{constructor(){super('return this');}};return new F()()===globalThis})", "T(()=>Function.prototype.toString.call(Function('a','return a')))", "T(()=>Function.prototype.toString.call(function  f ( a ) { }))",
  "T(()=>Function.prototype.toString.call(class  A { }))", "T(()=>Function.prototype.toString.call(Math.max))", "T(()=>Function.prototype.toString.call(Function))", "T(()=>Function.prototype.toString.call(Function.prototype))", "T(()=>Function.prototype.toString.call({}))",
  "T(()=>Function.prototype.toString.call(function(){}.bind()))", "T(()=>Function.prototype.toString.call(new Proxy(function(){},{})))", "T(()=>Function.prototype.toString.call(new Proxy({},{})))", "T(()=>Function.prototype.toString.call(async()=>1))",
  "T(()=>Function.prototype.toString.call({m(){}}.m))", "T(()=>Function.prototype.toString.call({get g(){return 1}}.__lookupGetter__('g')))", "T(()=>Function.prototype.toString.call({*g(){}}.g))", "T(()=>Function.prototype.toString.call({async *g(){}}.g))",
  "T(()=>Function.prototype.toString.call(Symbol))", "T(()=>Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Map.prototype,'size').get))", "T(()=>Function.prototype.toString.call(Symbol.prototype[Symbol.toPrimitive]))",
  `T(()=>${GF}.length)`, `T(()=>${GF}.name)`, `T(()=>${AF}.name)`, `T(()=>${AGF}.name)`, `T(()=>Object.getPrototypeOf(${GF})===Function)`, `T(()=>Object.getPrototypeOf(${AF})===Function)`, `T(()=>${GF}.prototype[Symbol.toStringTag])`, `T(()=>${AF}.prototype[Symbol.toStringTag])`, `T(()=>${AGF}.prototype[Symbol.toStringTag])`,
  `T(()=>D(${GF},'prototype'))`, `T(()=>D(${GF}.prototype,'constructor'))`, `T(()=>D(${GF}.prototype,'prototype'))`, `T(()=>D(${AF}.prototype,'constructor'))`, `T(()=>${GF}.prototype.prototype===Object.getPrototypeOf(function*(){}.prototype))`,
  `T(()=>Object.getPrototypeOf(${GF}.prototype)===Function.prototype)`, `T(()=>Object.getOwnPropertyNames(${GF}.prototype).join())`, `T(()=>Object.getOwnPropertyNames(${AF}.prototype).join())`, `T(()=>Object.getOwnPropertyNames(${GF}).join())`, `T(()=>Reflect.ownKeys(${AGF}.prototype).map(String).join())`,
  `T(()=>{var g=new ${GF}('a','yield a;yield a*2');return [...g(3)].join()})`, `T(()=>{var g=${GF}('yield 1;return 2');var i=g();return S([i.next(),i.next(),i.next()])})`, `T(()=>{var g=${GF}('return this');return g.call(7).next().value===globalThis})`,
  `T(()=>${GF}('yield','')) `, `T(()=>${GF}('a=yield',''))`, `T(()=>${GF}('','var yield'))`, `T(()=>${GF}('','yield\\n/1/g'))`, `T(()=>${GF}('','yield\\n*2'))`, `T(()=>${AF}('await',''))`, `T(()=>${AF}('a=await 1',''))`, `T(()=>${AF}('','var await'))`, `T(()=>${AF}('','await\\n/1/g'))`,
  `T(()=>Reflect.construct(${GF},['yield 1'],Array))`, `T(()=>Object.getPrototypeOf(Reflect.construct(${GF},['yield 1'],Array))===Array.prototype)`, `T(()=>${GF}.call(null,'yield 1').toString())`, `T(()=>${GF}.apply(null,['a','yield a']).toString())`,
  `T(()=>(function*(){}).constructor===${GF})`, `T(()=>(async function(){}).constructor===${AF})`, `T(()=>(async function*(){}).constructor===${AGF})`, `T(()=>(function(){}).constructor===Function)`, `T(()=>(()=>1).constructor===Function)`, `T(()=>(async()=>1).constructor===${AF})`,
  `T(()=>({*g(){}}).g.constructor===${GF})`, `T(()=>({async g(){}}).g.constructor===${AF})`, `T(()=>(class{static async*m(){}}).m.constructor===${AGF})`,
);

// ---- 8. Declarações globais via eval indireto, var vs let, configurabilidade.
const gDecls = [
  "var g1=1", "var g1", "function g1(){}", "let g1=1", "const g1=1", "class g1{}", "var g1=1;var g1=2", "var g1;function g1(){}", "x1=1", "this.g1=1", "g1=1;var g1", "var g1=this", "{function g1(){}}", "if(1){function g1(){}}", "for(var g1 of [1]);", "var [g1,g2]=[1,2]", "var {g1}={g1:3}",
  "'use strict';var g1=1", "'use strict';function g1(){}", "'use strict';x1=1", "'use strict';this.g1=1", "var undefined", "var NaN=1", "var Infinity", "var globalThis=1", "var eval", "var Array=1", "function Array(){}", "function undefined(){}", "function NaN(){}", "function Infinity(){}", "function globalThis(){}",
  "function eval(){}", "function parseInt(){}", "function JSON(){}", "function R2(){}", "let undefined", "let NaN", "let Array", "let globalThis", "const undefined=1", "class Array{}", "var g1=1;delete this.g1", "function g1(){};delete this.g1", "var g1=1;typeof this.g1", "let g1=1;typeof this.g1",
];
const gPrior = [
  "", "Object.defineProperty(globalThis,'g1',{value:0,configurable:true,writable:true,enumerable:true});", "Object.defineProperty(globalThis,'g1',{value:0,configurable:false,writable:true,enumerable:true});", "Object.defineProperty(globalThis,'g1',{value:0,configurable:false,writable:false,enumerable:false});",
  "Object.defineProperty(globalThis,'g1',{value:0,configurable:true,writable:false,enumerable:false});", "Object.defineProperty(globalThis,'g1',{get(){return 1},configurable:true});", "Object.defineProperty(globalThis,'g1',{get(){return 1},configurable:false});",
  "Object.defineProperty(globalThis,'g1',{value:0,configurable:false,writable:true,enumerable:false});", "Object.defineProperty(globalThis,'g1',{value:0,configurable:true,writable:true,enumerable:false});", "Object.preventExtensions(globalThis);",
];
const gProbe = "[typeof g1,typeof g2,typeof x1,D(globalThis,'g1'),D(globalThis,'x1')].join('|')";
for (const prior of gPrior) for (const p of gDecls) {
  add(`T(()=>{${prior}var r;try{r=(0,eval)(${q(p)})}catch(e){r="throw "+e.name+": "+e.message}return S(r)+"~"+${gProbe}})`);
}
for (const k of ["undefined", "NaN", "Infinity", "globalThis", "eval", "Array", "parseInt", "JSON", "Math", "Symbol", "Function", "Object", "isNaN", "escape", "Reflect", "Proxy", "Atomics", "WebAssembly", "Intl", "console", "queueMicrotask"]) {
  add(`T(()=>D(globalThis,${q(k)}))`.replace(/^T\(\(\)=>D\(globalThis,"(console|queueMicrotask|WebAssembly)"\)\)$/, "T(()=>typeof globalThis.$1)"), `T(()=>(0,eval)("typeof ${k}"))`, `T(()=>(function(){'use strict';${k}=1;return 1})())`, `T(()=>(0,eval)("${k}=1;typeof ${k}"))`);
  if (!["console", "queueMicrotask", "WebAssembly"].includes(k)) add(`T(()=>delete globalThis.${k})`, `T(()=>(function(){'use strict';return delete globalThis.${k}})())`, `T(()=>Reflect.deleteProperty(globalThis,${q(k)}))`);
}
add(
  "T(()=>this===globalThis)", "T(()=>(0,eval)('this')===globalThis)", "T(()=>(0,eval)('\"use strict\";this')===globalThis)", "T(()=>(function(){return this})()===globalThis)", "T(()=>(function(){'use strict';return this})())", "T(()=>(0,eval)('(function(){return this})()')===globalThis)",
  "T(()=>globalThis.globalThis===globalThis)", "T(()=>D(globalThis,'globalThis'))", "T(()=>Object.prototype.toString.call(globalThis).slice(0,8))", "T(()=>typeof globalThis)", "T(()=>(0,eval)('typeof window+typeof self+typeof global'))",
  "T(()=>(0,eval)('var g3=1;this.g3===g3'))", "T(()=>(0,eval)('var g3=1;g3=2;this.g3'))", "T(()=>(0,eval)('let g3=1;this.g3'))", "T(()=>(0,eval)('let g3=1;g3=2;g3'))", "T(()=>(0,eval)('const g3=1;g3=2'))", "T(()=>(0,eval)('const g3=1;try{g3=2}catch(e){e.name+\": \"+e.message}'))",
  "T(()=>(0,eval)('let g3=1;(0,eval)(\"typeof g3\")'))", "T(()=>(0,eval)('let g3=1;eval(\"typeof g3\")'))", "T(()=>(0,eval)('let g3=1;eval(\"var g3\")'))", "T(()=>(0,eval)('var g3=1;eval(\"let g3=2;g3\")'))", "T(()=>(0,eval)('var g3=1;eval(\"var g3=2\");g3'))", "T(()=>(0,eval)('function g3(){return 1};eval(\"var g3=2\");typeof g3'))",
  "T(()=>(0,eval)('g3;let g3=1'))", "T(()=>(0,eval)('typeof g3;let g3=1'))", "T(()=>(0,eval)('g3=1;let g3'))", "T(()=>(0,eval)('var g3;let g3'))", "T(()=>(0,eval)('let g3;var g3'))", "T(()=>(0,eval)('let g3;let g3'))", "T(()=>(0,eval)('const g3'))", "T(()=>(0,eval)('let g3;function g3(){}'))",
  "T(()=>(0,eval)('function g3(){};function g3(){return 2};g3()'))", "T(()=>(0,eval)('function g3(){};var g3;typeof g3'))", "T(()=>(0,eval)('var g3=1;function g3(){};typeof g3'))", "T(()=>(0,eval)('{function g3(){}};typeof g3'))", "T(()=>(0,eval)('typeof g3;{function g3(){}}'))",
  "T(()=>(0,eval)('\"use strict\";{function g3(){}};typeof g3'))", "T(()=>(0,eval)('if(0){function g3(){}};typeof g3'))", "T(()=>(0,eval)('if(0){function g3(){}};g3'))", "T(()=>(0,eval)('switch(1){case 1:function g3(){}};typeof g3'))", "T(()=>(0,eval)('try{}catch(g3){var g3=1};typeof g3'))",
  "T(()=>(0,eval)('for(var g3 in {a:1});g3'))", "T(()=>(0,eval)('for(var g3=0;g3<2;g3++);g3'))", "T(()=>(0,eval)('var g3=g3+1;g3'))", "T(()=>(0,eval)('var g3=1;var g3;g3'))", "T(()=>(0,eval)('x4=1;x4'))", "T(()=>(0,eval)('\"use strict\";x4=1'))", "T(()=>(0,eval)('x4;'))", "T(()=>(0,eval)('typeof x4'))",
  "T(()=>{(0,eval)('var g5=1');return Object.keys(globalThis).includes('g5')})", "T(()=>{(0,eval)('function g5(){}');return Object.keys(globalThis).includes('g5')})", "T(()=>{(0,eval)('g5=1');return Object.keys(globalThis).includes('g5')})", "T(()=>{(0,eval)('let g5=1');return Object.keys(globalThis).includes('g5')})",
  "T(()=>{globalThis.g5=1;return D(globalThis,'g5')})", "T(()=>{Object.defineProperty(globalThis,'g5',{get(){return 7},configurable:true});return (0,eval)('g5')})", "T(()=>{Object.defineProperty(globalThis,'g5',{get(){return 7},configurable:true});return (0,eval)('var g5=1;g5')})",
  "T(()=>{Object.defineProperty(globalThis,'g5',{get(){return 7},configurable:true});(0,eval)('var g5=1');return D(globalThis,'g5')})", "T(()=>{Object.defineProperty(globalThis,'g5',{get(){return 7},configurable:true});(0,eval)('function g5(){}');return D(globalThis,'g5')})",
  "T(()=>{Object.defineProperty(globalThis,'g5',{value:1,configurable:false,writable:false});return (0,eval)('g5=2;g5')})", "T(()=>{Object.defineProperty(globalThis,'g5',{value:1,configurable:false,writable:false});return (0,eval)('\"use strict\";g5=2')})",
  "T(()=>{Object.defineProperty(globalThis,'g5',{value:1,configurable:false,writable:false});return (0,eval)('var g5=2;g5')})", "T(()=>{Object.defineProperty(globalThis,'g5',{value:1,configurable:false,writable:false});return (0,eval)('delete g5')})",
  "T(()=>{Object.defineProperty(globalThis,'g5',{value:1,configurable:true});return (0,eval)('delete g5')+','+typeof g5})", "T(()=>{globalThis.g5=1;return (0,eval)('delete g5')+','+typeof g5})", "T(()=>{globalThis.g5=1;return (0,eval)('\"use strict\";delete g5')})",
  "T(()=>{Object.preventExtensions(globalThis);return (0,eval)('var g6=1')})", "T(()=>{Object.preventExtensions(globalThis);return (0,eval)('function g6(){}')})", "T(()=>{Object.preventExtensions(globalThis);return (0,eval)('g6=1;typeof g6')})", "T(()=>{Object.preventExtensions(globalThis);return (0,eval)('\"use strict\";g6=1')})",
  "T(()=>{Object.preventExtensions(globalThis);return (0,eval)('let g6=1;g6')})", "T(()=>{Object.preventExtensions(globalThis);return (0,eval)('var R;typeof R')})", "T(()=>{Object.freeze(globalThis);return (0,eval)('var S;typeof S')})",
);

// ---- Execução com filhos em paralelo e timeout de 5 s.
const baseSources = new Set();
const dir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(dir)) {
  if (!file.endsWith(".tsv") || file === "eval_with_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(dir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.add(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const programs = [];
let dup = 0;
for (const expr of unique) {
  const source = PRELUDE + `globalThis.R = ${expr}`;
  if (baseSources.has(source)) { dup++; continue; }
  programs.push({ expr, source });
}

function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let done = false;
    const finish = (value) => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => { try { child.kill("SIGKILL"); } catch (e) {} finish(null); }, 5000);
    child.stdout.on("data", (d) => (out += d));
    child.on("error", () => finish(null));
    child.on("close", (code) => finish(code === 0 ? decodeResult(out) : null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (next < programs.length) {
      const i = next++;
      results[i] = await runChild(programs[i].source);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < programs.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(programs[i].expr).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    lines.push({ source: programs[i].source, result: result });
  }
  process.stdout.write(emitFactored("eval_with", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
