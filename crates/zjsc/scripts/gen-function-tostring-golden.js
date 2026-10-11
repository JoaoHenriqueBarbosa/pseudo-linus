// Gera tests/golden/function_tostring_bun.tsv: Function.prototype.toString em grade de fontes, medido no bun 1.4.2.
// Cobre o texto exato do fonte (comentários, espaços, quebras de linha, separadores Unicode, nomes computados e
// escapados) de funções, geradores, async, arrows, métodos, getters e setters de literais e de classes (estáticos e
// privados), classes com herança e campos com arrows, bound functions e Proxy de função (`function () { [native code] }`),
// funções de `new Function` e dos construtores de gerador/async (cabeçalho `function anonymous(a,b\n) {\n...\n}`),
// eval retornando função, funções nativas (nomes de símbolo, prefixos get/set) e a TypeError de toString em não funções.
// Programas cuja expressão já aparece nos goldens function_proto_bun, native_function_bun e dynamic_fn_bun são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-function-tostring-golden.js > tests/golden/function_tostring_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v){return JSON.stringify(v===undefined?"<undefined>":v)}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'var TS=Function.prototype.toString;\n' +
  'function FS(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return [d.value,d.get,d.set].map(x=>typeof x==="function"?TS.call(x):x===undefined?null:"nf")}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

let seed = 20261008;
const rnd = (n) => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  return seed % n;
};
const pick = (list) => list[rnd(list.length)];

// ---- 1. function, generator, async e async generator: produto de prefixo, nome, parâmetros e corpo.
const prefixes = ["function", "function*", "async function", "async function*"];
const names = ["", "f", "$", "_x", "\\u0061b", "ünï"];
const params = ["", "a", "a,b", "a=1", "a,b=2,...c", "{a,b}", "[a,b]", "{a=1}={}", "...r", "a,"];
const bodies = ["", "return 1", " ", "\n", "/*c*/", "//c\n", "return `${1}`", "if(a){}else{}", "var x=1;return x", "return function(){}"];
for (const p of prefixes) for (const n of names) for (const ps of params.slice(0, 6)) for (const b of bodies.slice(0, 6)) {
  add(`T(()=>(${p} ${n}(${ps}){${b}}).toString())`);
}
// Variações de espaço e comentário entre cada token.
const gaps = [" ", "", "\n", "/*x*/", " //c\n", "\t", " ", " ", "\r\n", " ", "﻿", "  /* a\nb */  "];
const gapsNeeded = gaps.filter((g) => g !== "");
for (let i = 0; i < 360; i++) {
  const p = prefixes[i % 4];
  const n = pick(["", "f", "g1"]);
  const asyncGap = p.startsWith("async") ? " " : "";
  const head = p.startsWith("async") ? "async" + asyncGap + (p.endsWith("*") ? "function*" : "function") : p;
  const g1 = n ? pick(gapsNeeded) : pick(gaps);
  const g2 = pick(gaps), g3 = pick(gaps), g4 = pick(gaps), g5 = pick(gaps), g6 = pick(gaps), g7 = pick(gaps);
  const ps = pick(params.slice(0, 6));
  const b = pick(bodies.slice(0, 6));
  add(`T(()=>(${head}${g1}${n}${g2}(${g3}${ps}${g4})${g5}{${g6}${b}${g7}}).toString())`);
}

// ---- 2. arrows, async arrows, com espaços, comentários e corpos de expressão.
const arrowParams = ["a", "(a)", "(a,b)", "()", "({a})", "([a])", "(a=1)", "(...r)"];
const arrowBodies = ["a", "{}", "{return 1}", "({})", "a+1", "[]", "`t`", "(a,b)"];
const arrowGaps = [" ", "", "/*c*/", "\t"];
for (const asy of ["", "async "]) for (const ap of arrowParams) for (const ab of arrowBodies) for (const g of arrowGaps.slice(0, 3)) {
  if (asy && ap === "a" && g === "") continue;
  add(`T(()=>(${asy}${ap}${g}=>${g}${ab}).toString())`);
}
for (const ap of arrowParams.slice(1)) for (const g of ["\n  ", "  \n", "/*\n*/"]) {
  add(`T(()=>(${ap}${g.includes("\n") && g !== "/*\n*/" ? " " : g}=>${g}1).toString())`);
}
add(
  "T(()=>(x => x /* tail */).toString())", "T(()=>[x => x, y => y][1].toString())", "T(()=>(() => () => () => 1)().toString())",
  "T(()=>(() => async x => x)().toString())", "T(()=>((a,b)=>a)(1)===1)", "T(()=>(async()=>{}).toString())",
  "T(()=>(async x => x).toString())", "T(()=>(x=>x)\n.toString())", "T(()=>(x=>x)?.toString())",
);

// ---- 3. métodos, getters e setters em literais, classes (instância, estático e privado).
const keys = [
  ["m", "m"], ["'s t'", "s t"], ["1", "1"], ["1.5", "1.5"], ["[k]", "kk"], ["[Symbol.iterator]", Symbol.iterator], ["[`a${1}`]", "a1"],
  ["0x10", "16"], ["\\u0061", "a"], ["async", "async"], ["get", "get"], ["static", "static"], ["[Symbol.for('x')]", Symbol.for("x")],
  ["'a\\nb'", "a\nb"], ["\"q\"", "q"], ["1e3", "1000"], ["[k+k]", "kkkk"], ["ünï", "ünï"],
];
const kinds = (K) => [`${K}(a){}`, `get ${K}(){return 1}`, `set ${K}(v){}`, `*${K}(){}`, `async ${K}(){}`, `async *${K}(){}`, `${K} ( a , b ) { /* c */ }`, `get  ${K} ( ) { }`];
const keyExpr = (v) => (typeof v === "symbol" ? (v === Symbol.iterator ? "Symbol.iterator" : "Symbol.for('x')") : JSON.stringify(v));
for (const [K, lookup] of keys) for (const m of kinds(K)) {
  const lk = keyExpr(lookup);
  const nonStaticOk = !(K === "static" && false);
  add(`T(()=>{var k="kk";var o={${m}};return FS(o,${lk})})`);
  add(`T(()=>{var k="kk";class C{${m}}return FS(C.prototype,${lk})})`);
  if (nonStaticOk) add(`T(()=>{var k="kk";class C{static ${m}}return FS(C,${lk})})`);
}
const privKeys = ["p", "$q", "_r", "\\u0061b"];
for (const K of privKeys) {
  const priv = `#${K}`;
  add(`T(()=>{class C{${priv}(a){return 1}static t(o){return o.${priv}}}return TS.call(C.t(new C))})`);
  add(`T(()=>{class C{static ${priv}(){}static t(){return C.${priv}}}return TS.call(C.t())})`);
  add(`T(()=>{class C{get ${priv}(){return 1}set ${priv}(v){}static t(){return Object.getOwnPropertyNames(C.prototype).length}}return C.t()})`);
  add(`T(()=>{class C{async *${priv}(){}static t(o){return o.${priv}}}return TS.call(C.t(new C))})`);
  add(`T(()=>{class C{${priv}=()=>1;static t(o){return o.${priv}}}return TS.call(C.t(new C))})`);
  add(`T(()=>{class C{static ${priv}=function(){};static t(){return C.${priv}}}return TS.call(C.t())})`);
}
add(
  "T(()=>{var o={f(){},g(){}};return TS.call(o.f)+'|'+TS.call(o.g)})",
  "T(()=>{var o={__proto__(){}};return TS.call(Object.getOwnPropertyDescriptor(o,'__proto__').value)})",
  "T(()=>{var o={f:function(){},g:()=>1,h:class{}};return [o.f,o.g,o.h].map(x=>TS.call(x))})",
  "T(()=>{var o={f:function  named ( ) { }};return TS.call(o.f)})",
);

// ---- 4. classes: herança, corpo, nome, espaços, campos com arrows, static blocks.
const heritage = ["", " extends B", " extends (B)", " extends null", " extends mixin(B)", "\nextends\nB\n", " /*h*/ extends /*h*/ B "];
const classBodies = [
  "", "constructor(){super()}", "m(){}", "static s(){}", "a=1;b=()=>2", "static x=()=>1", "get g(){return 1}set g(v){}", "static{}",
  "[k](){}", "#p=1;t(){return this.#p}", "async *ag(){}", "static async m(){}", "/* c */", "\n\n", "'a'(){}", "static #p(){}",
];
for (const h of heritage) for (const b of classBodies) {
  const ctor = h.includes("null") ? b.replace("constructor(){super()}", "constructor(){}") : b;
  add(`T(()=>{var B=class{};var k="k";function mixin(c){return c}return TS.call(class${h.startsWith(" ") || h.startsWith("\n") ? "" : " "}${h}{${ctor}})})`);
  add(`T(()=>{var B=class{};var k="k";function mixin(c){return c}return TS.call(class Name${h}{${ctor}})})`);
}
for (const g of gaps) {
  add(`T(()=>TS.call(class${g === "" ? " " : g}C${g}{${g}}))`);
  add(`T(()=>TS.call(class${g === "" ? " " : g}extends${g === "" ? " " : g}Object${g}{${g}}))`);
}
add(
  "T(()=>{class C{f=()=>1;g=function(){};h=class{}}var c=new C;return [c.f,c.g,c.h].map(x=>TS.call(x))})",
  "T(()=>{class C{static f=()=>1;static g=async function*(){}}return [C.f,C.g].map(x=>TS.call(x))})",
  "T(()=>{class C{['a'+'b']=()=>1}return TS.call(new C().ab)})",
  "T(()=>{class C{static{C.t=()=>1}}return TS.call(C.t)})",
  "T(()=>{class C{constructor(){this.f=()=>this}}return TS.call(new C().f)})",
  "T(()=>{class C{m(){return ()=>1}}return TS.call(new C().m())})",
  "T(()=>{class C{}class D extends C{}return TS.call(D)+TS.call(Object.getPrototypeOf(D))})",
  "T(()=>{class C{}return TS.call(C.prototype.constructor)===TS.call(C)})",
  "T(()=>{class C{constructor(a,b){}}return TS.call(C)})",
  "T(()=>TS.call(class{}))", "T(()=>TS.call(class A{}))", "T(()=>TS.call(class extends Array{}))",
  "T(()=>{var C=class  X  {  };return TS.call(C)})", "T(()=>{class C{static name=1}return TS.call(C)})",
  "T(()=>{class C{static toString(){return 'x'}}return C.toString()+'|'+TS.call(C)})",
  "T(()=>{class C{}C.toString=()=>'y';return C+''+TS.call(C)})",
  "T(()=>{function f(){}f.toString=()=>'z';return f+'|'+TS.call(f)})",
  "T(()=>{function f(){}Object.defineProperty(f,'name',{value:'nn'});return TS.call(f)})",
  "T(()=>{function f(){}f.prototype=null;return TS.call(f)})",
  "T(()=>{function f(){}delete f.name;return TS.call(f)})",
  "T(()=>{function f(){}Object.setPrototypeOf(f,null);return TS.call(f)})",
  "T(()=>{function f(){return TS.call(f)}return f()})",
  "T(()=>{function f(){return TS.call(arguments.callee)}return f()})",
  "T(()=>{function outer(){function inner(){/*i*/}return inner}return TS.call(outer())+TS.call(outer)})",
  "T(()=>{var s=`a${function f(){}}b`;return s})", "T(()=>{var s='x'+(()=>1);return s})", "T(()=>String(function  g ( ) { }))",
  "T(()=>`${class  K  {}}`)", "T(()=>[function(){}]+'')", "T(()=>({f(){}}).f+'')", "T(()=>(async function  *  ( ) { }) + '')",
  "T(()=>{var o={get a(){return 1}};return String(Object.getOwnPropertyDescriptor(o,'a').get)})",
  "T(()=>{label:function f(){};return typeof f})",
);

// ---- 5. bound functions e Proxy de função.
const wrapSources = [
  "function f(){}", "function  f ( a , b ) { return a }", "function*g(){}", "async function h(){}", "async function*ag(){}", "()=>1", "async x=>x",
  "class C{}", "class D extends Object{}", "{m(){}}.m", "Math.max", "Array", "Function.prototype", "Symbol", "Object.prototype.toString",
  "function(){}.bind(null)", "Array.prototype[Symbol.iterator]", "Map.prototype.get", "new Function('a','return a')", "async()=>{}",
];
const wrapExprs = wrapSources.map((s) => (s.startsWith("{") ? `(${s})` : `(${s})`));
for (const w of wrapExprs) {
  add(`T(()=>TS.call(${w}.bind(null)))`, `T(()=>TS.call(${w}.bind(null).bind(1)))`, `T(()=>TS.call(${w}.bind(null,1,2)))`);
  add(`T(()=>TS.call(new Proxy(${w},{})))`, `T(()=>TS.call(new Proxy(new Proxy(${w},{}),{})))`, `T(()=>TS.call(new Proxy(${w}.bind(null),{get(){return 1}})))`);
  add(`T(()=>TS.call(Proxy.revocable(${w},{}).proxy))`, `T(()=>{var r=Proxy.revocable(${w},{});r.revoke();return TS.call(r.proxy)})`);
  add(`T(()=>(new Proxy(${w},{})).toString())`, `T(()=>String(${w}.bind(null)))`, `T(()=>Reflect.apply(TS,${w},[]))`);
  add(`T(()=>TS.call(${w}.call.bind(${w})))`, `T(()=>TS.call(Function.prototype.bind.call(${w},null)))`);
}
add(
  "T(()=>TS.call(Proxy))", "T(()=>TS.call(new Proxy({},{})))", "T(()=>TS.call(new Proxy([],{})))", "T(()=>TS.call(new Proxy(function(){},{apply(){return 1}})))",
  "T(()=>{var r=Proxy.revocable({},{});r.revoke();return TS.call(r.proxy)})", "T(()=>TS.call(new Proxy(class{},{construct(){return {}}})))",
  "T(()=>{var p=new Proxy(function(){},{get(t,k){throw new Error('g')}});return TS.call(p)})",
  "T(()=>{var p=new Proxy(function(){},{getPrototypeOf(){throw new Error('p')}});return TS.call(p)})",
  "T(()=>(function(){}).bind().name)", "T(()=>TS.call(function(){}.bind()).length)", "T(()=>{function f(){}var b=f.bind();return TS.call(b)===TS.call(f.bind(1))})",
);

// ---- 6. new Function e construtores de gerador, async e async generator.
const dynParams = [[], ["a"], ["a", "b"], ["a,b"], ["a=1"], ["...r"], ["{a}"], ["[a]"], ["a /*c*/"], ["a //c"], ["//c\na"], ["", "a"], ["a", "b", "c"], [" a "], ["a,b=1,...c"], ["\n"], ["a", "\n"], ["a,"], ["/*", "*/a"], ["a= "]];
const dynBodies = ["", "return 1", "\n", "//c", "/*c*/", "return 1\n//c", "'use strict'", "}", "}{", "return `\n`", "a\nb", " ", "  x  ", "yield 1", "await 1", "-->", "<!--", "});(function(){", "/*", "return a", "super()", "new.target", "var a;", "\"use strict\";with(a){}"];
const ctors = [
  ["Function", "Function"],
  ["GeneratorFunction", "Object.getPrototypeOf(function*(){}).constructor"],
  ["AsyncFunction", "Object.getPrototypeOf(async function(){}).constructor"],
  ["AsyncGeneratorFunction", "Object.getPrototypeOf(async function*(){}).constructor"],
];
ctors.forEach(([name, ex], idx) => {
  const ps = idx === 0 ? dynParams : dynParams.slice(0, 10);
  const bs = idx === 0 ? dynBodies : dynBodies.slice(0, 12);
  for (const p of ps) for (const b of bs) {
    const args = [...p, b].map((x) => JSON.stringify(x)).join(",");
    add(`T(()=>TS.call(new (${ex})(${args})))`);
  }
});
add(
  "T(()=>TS.call(Function()))", "T(()=>TS.call(Function('')))", "T(()=>TS.call(new Function))", "T(()=>new Function('a','b','return a').toString())",
  "T(()=>Function('a','b','return a')+'')", "T(()=>TS.call(Function.prototype.constructor('x','return x')))",
  "T(()=>TS.call(new Function({toString(){return 'p'}},{toString(){return 'q'}})))", "T(()=>TS.call(new Function(1,2)))",
  "T(()=>TS.call(new Function(undefined)))", "T(()=>TS.call(new Function(null)))", "T(()=>TS.call(new Function('a','b')).length)",
  "T(()=>TS.call(Reflect.construct(Function,['a'],Object)))", "T(()=>{class F extends Function{}return TS.call(new F('a','return a'))})",
  "T(()=>{class F extends Function{}return TS.call(new F())})", "T(()=>Function.prototype.toString.call(Function.prototype))",
  "T(()=>TS.call(Function.prototype.toString))", "T(()=>TS.call(Function))", "T(()=>TS.call(Function.prototype.constructor))",
  "T(()=>TS.call(Object.getPrototypeOf(function*(){}).constructor))", "T(()=>TS.call(Object.getPrototypeOf(async function(){}).constructor))",
);

// ---- 7. eval retornando função.
const evalSources = [
  "(function  f ( ) { })", "(function*(){})", "(async function(){})", "(x=>x)", "(async x => x)", "(class A { })", "({m(){}}).m", "({get a(){return 1}})",
  "(function/*c*/(a,b){})", "(function\nf(){})", "var q=function g(){};q", "function h(){}\nh", "(()=>{})", "(class extends Object{})",
  "/*lead*/(function(){})//tail", "  (  function  (  )  {  }  )  ", "(function(){}).bind()", "[function(){}][0]", "(0,function z(){})", "`${1}`;(function(){})",
];
for (const e of evalSources) {
  const lit = JSON.stringify(e);
  add(`T(()=>TS.call(eval(${lit})))`, `T(()=>TS.call((0,eval)(${lit})))`, `T(()=>TS.call(new Function('return '+${JSON.stringify(e)})())) `.trim());
  add(`T(()=>{var f=eval(${lit});return typeof f==="function"?String(f):typeof f})`);
}
add(
  "T(()=>TS.call(eval('(function(){})')))", "T(()=>TS.call(eval('0,function(){}')))", "T(()=>{'use strict';return TS.call(eval('(function(){})'))})",
  "T(()=>TS.call(eval('(function(){ \"use strict\" })')))", "T(()=>eval('(function(){}).toString()'))",
);

// ---- 8. funções nativas: enumera os métodos, acessores e construtores das globais ECMAScript.
const globalsList = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Math", "JSON", "Reflect", "Date", "RegExp", "Error", "TypeError",
  "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "Promise", "ArrayBuffer", "DataView", "Proxy", "Atomics", "Iterator",
  "Uint8Array", "Float64Array", "Int32Array", "SharedArrayBuffer", "AggregateError",
];
const wellKnown = Object.getOwnPropertyNames(Symbol).filter((n) => typeof Symbol[n] === "symbol");
const keyText = (k) => {
  if (typeof k === "string") return JSON.stringify(k);
  const wk = wellKnown.find((n) => Symbol[n] === k);
  return wk ? `Symbol.${wk}` : null;
};
const skipNames = new Set(["captureStackTrace", "prepareStackTrace", "stackTraceLimit", "caller", "arguments"]);
const nativeTargets = [];
const addNatives = (objExpr, obj) => {
  for (const k of Reflect.ownKeys(obj)) {
    if (typeof k === "string" && skipNames.has(k)) continue;
    const kt = keyText(k);
    if (!kt) continue;
    const d = Object.getOwnPropertyDescriptor(obj, k);
    if (typeof d.value !== "function" && !d.get && !d.set) continue;
    nativeTargets.push(`T(()=>FS(${objExpr},${kt}))`);
  }
};
for (const g of globalsList) {
  const v = globalThis[g];
  if (!v) continue;
  addNatives(g, v);
  if (v.prototype && typeof v.prototype === "object") addNatives(`${g}.prototype`, v.prototype);
}
addNatives("Object.getPrototypeOf(Uint8Array)", Object.getPrototypeOf(Uint8Array));
addNatives("Object.getPrototypeOf(Uint8Array).prototype", Object.getPrototypeOf(Uint8Array).prototype);
addNatives("Object.getPrototypeOf([][Symbol.iterator]())", Object.getPrototypeOf([][Symbol.iterator]()));
addNatives("Object.getPrototypeOf(new Map()[Symbol.iterator]())", Object.getPrototypeOf(new Map()[Symbol.iterator]()));
addNatives("Object.getPrototypeOf(new Set()[Symbol.iterator]())", Object.getPrototypeOf(new Set()[Symbol.iterator]()));
addNatives("Object.getPrototypeOf(''[Symbol.iterator]())", Object.getPrototypeOf(""[Symbol.iterator]()));
addNatives("Object.getPrototypeOf(function*(){}).prototype", Object.getPrototypeOf(function* () {}).prototype);
addNatives("Object.getPrototypeOf(async function*(){}).prototype", Object.getPrototypeOf(async function* () {}).prototype);
addNatives("Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype)", Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype));
addNatives("Function.prototype", Function.prototype);
for (const t of nativeTargets) add(t);
add(
  "T(()=>TS.call(Symbol.prototype[Symbol.toPrimitive]))", "T(()=>TS.call(RegExp.prototype[Symbol.matchAll]))", "T(()=>TS.call(Function.prototype[Symbol.hasInstance]))",
  "T(()=>TS.call(Object.getOwnPropertyDescriptor(RegExp,Symbol.species).get))", "T(()=>TS.call(Object.getOwnPropertyDescriptor(Map,Symbol.species).get))",
  "T(()=>TS.call(Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set))", "T(()=>TS.call(Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get))",
  "T(()=>TS.call(Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array).prototype,Symbol.toStringTag).get))",
  "T(()=>TS.call(Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get))", "T(()=>{var f=Object.getOwnPropertyDescriptor(Map.prototype,'size').get;return f.name+TS.call(f)})",
  "T(()=>{var f=Object.getOwnPropertyDescriptor(RegExp.prototype,'flags').get;return f.name})", "T(()=>TS.call(Function.prototype.call.bind(Math.max)))",
  "T(()=>{var f=Array.prototype[Symbol.iterator];return f===Array.prototype.values?TS.call(f):'no'})",
  "T(()=>TS.call(Symbol.for))", "T(()=>TS.call(parseInt))", "T(()=>TS.call(escape))", "T(()=>TS.call(decodeURIComponent))", "T(()=>TS.call(isNaN))", "T(()=>TS.call(eval))",
  "T(()=>TS.call(globalThis.Iterator.prototype[Symbol.iterator]))", "T(()=>TS.call(Object.getPrototypeOf(function*(){}).prototype.next))",
);

// ---- 9. toString chamado em não funções e `this` inválido: mensagem de TypeError exata.
const nonFunctions = [
  "undefined", "null", "0", "-0", "1.5", "NaN", "''", "'function(){}'", "true", "false", "Symbol()", "Symbol.iterator", "1n", "{}", "[]", "[function(){}]",
  "new Date(0)", "/x/", "new Map", "new Set", "new Error('e')", "Object.create(Function.prototype)", "{call(){},toString(){}}", "new String('s')", "new Number(1)",
  "new Boolean(true)", "Object(Symbol())", "Object(1n)", "globalThis", "Math", "JSON", "Reflect", "Atomics", "arguments", "new Proxy({},{})", "new Proxy([],{})",
  "Object.create(null)", "Object.assign(()=>1,{})!==1&&{__proto__:Function.prototype}", "new (class{})", "new (function(){})", "(function*(){})()", "Promise.resolve()",
  "new ArrayBuffer(1)", "new Uint8Array(1)", "{[Symbol.toStringTag]:'Function'}", "Object.create(Function.prototype,{call:{value:1}})", "WeakRef.prototype",
  "Function.prototype.prototype", "Math.max.prototype", "(()=>1).prototype", "(async()=>1).prototype", "(function*(){}).prototype", "class{}.prototype",
];
for (const nf of nonFunctions) {
  add(`T(()=>TS.call(${nf}))`, `T(()=>TS.apply(${nf}))`, `T(()=>Reflect.apply(TS,${nf},[]))`, `T(()=>{var o={f:TS};return o.f()})`.replace("o.f()", "o.f.call(" + nf + ")"));
  add(`T(()=>Function.prototype.toString.call(${nf}))`, `T(()=>TS.bind(${nf})())`);
}
add(
  "T(()=>TS())", "T(()=>(0,TS)())", "T(()=>{'use strict';return TS()})", "T(()=>{var f=TS;return f()})", "T(()=>{var o={toString:TS};return o.toString()})",
  "T(()=>{var o={toString:TS};return o+''})", "T(()=>{var o={toString:TS};return `${o}`})", "T(()=>{var o={toString:TS};return String(o)})",
  "T(()=>{Number.prototype.ts=TS;try{return (1).ts()}finally{delete Number.prototype.ts}})", "T(()=>TS.call())", "T(()=>TS.length)", "T(()=>TS.name)",
  "T(()=>Object.getOwnPropertyNames(TS).sort().join())", "T(()=>new TS())", "T(()=>new (class extends Function{}))", "T(()=>Reflect.construct(TS,[]))",
  "T(()=>typeof TS.prototype)", "T(()=>TS.hasOwnProperty('prototype'))", "T(()=>TS.call(TS)===TS.call(Function.prototype.toString))", "T(()=>Function.prototype.toString.call(TS.bind()))",
);

// ---- Dedup por fonte contra os goldens existentes e execução em processos novos.
const baseSources = [];
for (const file of ["function_proto_bun.tsv", "native_function_bun.tsv", "dynamic_fn_bun.tsv", "arguments_super_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
      if (!line) continue;
      try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
    }
  } catch (e) {}
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const programs = [];
let dup = 0;
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSources.includes(source)) { dup++; continue; }
  programs.push({ expr, source });
}

// Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso, então a ordem das chaves
// dos globais depende do que rodou antes no mesmo processo. Oito filhos em paralelo, ordem de saída preservada.
const esc = (t) => t.split(String.fromCharCode(0x2028)).join("\\u2028").split(String.fromCharCode(0x2029)).join("\\u2029");
const runChild = (source) => new Promise((resolve, reject) => {
  const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
  let out = "", err = "";
  child.stdout.setEncoding("utf8"); child.stderr.setEncoding("utf8");
  child.stdout.on("data", (c) => (out += c));
  child.stderr.on("data", (c) => (err += c));
  child.on("close", (code) => (code === 0 ? (decodeResult(out) === null ? reject(new Error("filho sem resultado")) : resolve(decodeResult(out))) : reject(new Error(err.split("\n")[0] || "filho falhou"))));
  child.stdin.on("error", () => {});
  child.stdin.end(source);
});

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  const worker = async () => {
    while (next < programs.length) {
      const i = next++;
      try { results[i] = { ok: true, value: await runChild(programs[i].source) }; } catch (e) { results[i] = { ok: false, error: String(e) }; }
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0, dropped = 0;
  const outLines = [];
  const resultsSeen = new Set();
  programs.forEach((p, i) => {
    const r = results[i];
    if (!r.ok) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(p.expr).slice(0, 160) + " " + r.error + "\n"); return; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.value)) { dropped++; process.stderr.write("caminho ou marca: " + JSON.stringify(p.expr).slice(0, 160) + "\n"); return; }
    kept++;
    outLines.push(esc(JSON.stringify(p.source)) + "\t" + esc(JSON.stringify(r.value)));
    resultsSeen.add(r.value);
  });
  process.stdout.write(emitFactoredLines("function_tostring", outLines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}, resultados distintos ${resultsSeen.size}\n`);
})();
