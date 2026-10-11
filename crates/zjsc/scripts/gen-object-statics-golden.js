// Gera tests/golden/object_statics_bun.tsv: estáticos de Object e Function, protótipos de built-ins quanto a
// propriedades, medidos no bun 1.4.2 com `vm.runInThisContext` (JSC puro). Cobre:
//   - Object.keys/values/entries/assign/fromEntries/getOwnPropertyNames/Symbols/Descriptors sobre strings, arrays com
//     buracos, typed arrays, arguments, funções, classes, Proxies e objetos com herança;
//   - Proxies com armadilhas registrando a ordem de ownKeys/getOwnPropertyDescriptor/get dos estáticos;
//   - groupBy, hasOwn, is, setPrototypeOf, create/defineProperties com descritores, ordem de chaves (inteiras, string,
//     símbolo, herança);
//   - Object.prototype.toString com Symbol.toStringTag em todos os built-ins;
//   - propertyIsEnumerable, __lookupGetter__, __proto__, toLocaleString, valueOf, isPrototypeOf;
//   - Function.prototype (caller/arguments em strict e sloppy, bind, Symbol.hasInstance);
//   - um programa por objeto built-in listando nome, length, descritores de name/length, construtibilidade e forma
//     nativa de cada método e accessor (ordenado por chave, para isolar o que o golden de descritores não fixa).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-object-statics-golden.js > tests/golden/object_statics_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE = `
function K(k){return typeof k==="symbol"?"@@"+(k.description===undefined?"":k.description):String(k)}
function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";if(t==="bigint")return v+"n";if(v===null||t!=="object")return Object.is(v,-0)?"-0":String(v);if(d>2)return"{...}";if(Array.isArray(v)){var a=[];for(var i=0;i<v.length;i++)a.push(i in v?S(v[i],d+1):"<hole>");return"["+a.join(",")+"]"}var p=[];for(var k of Reflect.ownKeys(v)){var ds=Object.getOwnPropertyDescriptor(v,k);p.push(K(k)+":"+("value" in ds?S(ds.value,d+1):"acc"))}return"{"+p.join(",")+"}"}
function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return"none";return("value"in d?"v":"a")+(d.writable?"w":"-")+(d.enumerable?"e":"-")+(d.configurable?"c":"-")}
function T(f){try{return f()}catch(e){return e.name+": "+e.message}}
function DS(o){return S(Object.getOwnPropertyDescriptors(o))}
function LOG(){var l=[];var h={ownKeys(t){l.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){l.push("gopd:"+K(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){l.push("get:"+K(k));return Reflect.get(t,k,r)},set(t,k,v,r){l.push("set:"+K(k));return Reflect.set(t,k,v,r)},has(t,k){l.push("has:"+K(k));return Reflect.has(t,k)},defineProperty(t,k,d){l.push("def:"+K(k));return Reflect.defineProperty(t,k,d)},getPrototypeOf(t){l.push("gpo");return Reflect.getPrototypeOf(t)},setPrototypeOf(t,p){l.push("spo");return Reflect.setPrototypeOf(t,p)},isExtensible(t){l.push("isExt");return Reflect.isExtensible(t)},preventExtensions(t){l.push("pe");return Reflect.preventExtensions(t)},deleteProperty(t,k){l.push("del:"+K(k));return Reflect.deleteProperty(t,k)}};return{l:l,h:h}}
`;

const programs = [];
const add = body => programs.push(PRELUDE + body);
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);

// ---- 1. Estáticos de Object x alvos variados.
const targets = [
  "'abc'",
  "new String('ab')",
  "[1,,3]",
  "Object.assign([1,2],{x:1})",
  "new Uint8Array([5,6])",
  "new Float64Array(2)",
  "new ArrayBuffer(2)",
  "(function(){return arguments})(1,2)",
  "(function(){'use strict';return arguments})(1)",
  "function f(a,b){}",
  "(a,b,c)=>1",
  "class A{static x=1;static m(){}}",
  "class B extends Array{}",
  "new Proxy({a:1,b:2},{})",
  "new Proxy([1,2],{})",
  "new Proxy(function(){},{})",
  "Object.create({inh:1},{own:{value:2,enumerable:true},hid:{value:3}})",
  "{2:1,b:1,1:1,a:1,[Symbol('s')]:1,'-1':1,'01':1,4294967295:1,4294967294:1}",
  "new Map([[1,2]])",
  "Symbol.iterator",
  "1",
  "true",
  "10n",
  "null",
  "undefined",
  "new Date(0)",
  "/a/g",
  "new Error('m')",
  "Math",
  "JSON",
];
const ops = [
  "Object.keys(t)",
  "Object.values(t)",
  "Object.entries(t)",
  "Object.getOwnPropertyNames(t)",
  "Object.getOwnPropertySymbols(t)",
  "DS(t)",
  "Reflect.ownKeys(t)",
  "Object.assign({}, t)",
  "Object.fromEntries(Object.entries(t))",
  "Object.hasOwn(t, 'length') + ',' + Object.hasOwn(t, '0')",
  "[Object.isFrozen(t), Object.isSealed(t), Object.isExtensible(t)]",
  "Object.getPrototypeOf(t) === Object.prototype ? 'Object.prototype' : typeof Object.getPrototypeOf(t)",
  "(function(){var r=[];for(var k in t)r.push(k);return r})()",
  "JSON.stringify(t)",
  "Object.prototype.toString.call(t)",
  "Object.getOwnPropertyNames(Object(t))",
];
for (const t of targets) for (const op of ops) T(`var t = ${t}; R = S(T(function(){ return ${op} }))`);

// ---- 2. Proxies com armadilhas: ordem de operações de cada estático.
const proxyOps = [
  "Object.keys(p)",
  "Object.values(p)",
  "Object.entries(p)",
  "Object.assign({}, p)",
  "Object.fromEntries(Object.entries(p))",
  "Object.getOwnPropertyNames(p)",
  "Object.getOwnPropertySymbols(p)",
  "Object.getOwnPropertyDescriptors(p)",
  "Object.getOwnPropertyDescriptor(p, 'a')",
  "Object.hasOwn(p, 'a')",
  "Object.isFrozen(p)",
  "Object.isSealed(p)",
  "Object.freeze(p)",
  "Object.seal(p)",
  "Object.preventExtensions(p)",
  "Object.setPrototypeOf(p, null)",
  "Object.getPrototypeOf(p)",
  "Object.defineProperty(p, 'z', {value: 1})",
  "Object.defineProperties(p, {z: {value: 1}, y: {value: 2}})",
  "Object.groupBy(p, k => k)",
  "({}).propertyIsEnumerable.call(p, 'a')",
  "Object.prototype.toString.call(p)",
  "Object.prototype.hasOwnProperty.call(p, 'a')",
  "p.__proto__",
  "Object.prototype.isPrototypeOf.call(Object.prototype, p)",
  "JSON.stringify(p)",
  "Object.assign(p, {n: 1})",
  "({...p})",
  "Reflect.ownKeys(p)",
  "'a' in p",
  "delete p.a",
];
for (const op of proxyOps) {
  T(`var x = LOG(); var p = new Proxy({a:1,[Symbol.for('s')]:2,2:3,b:4}, x.h); var r = T(function(){ return S(${op}) }); R = r + ' | ' + x.l.join(',')`);
}
T("var x = LOG(); var p = new Proxy([1,2], x.h); Object.entries(p); R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy(function f(a){}, x.h); Object.assign({}, p); R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy({a:1}, x.h); Object.assign(p, {b: 2}, 'cd', [5]); R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy({a:1}, x.h); Object.create(p, {q:{value:1}}); R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy({a:1}, x.h); Object.setPrototypeOf({}, p); R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy({a:1}, x.h); var o = Object.create(p); Object.keys(o); for (var k in o); R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy({a:1}, x.h); R = Object.getOwnPropertyDescriptors(Object.create(p)) && x.l.join(',')");
T("var x = LOG(); var p = new Proxy({}, x.h); p.k = 1; p[3] = 2; R = x.l.join(',')");
T("var x = LOG(); var p = new Proxy({a:1,b:2}, {ownKeys(t){return ['b','a','b']}}); R = T(function(){return S(Object.keys(p))})");
T("var p = new Proxy({a:1}, {ownKeys(t){return ['a','z']}}); R = S(Object.keys(p)) + S(Reflect.ownKeys(p))");
T("var p = new Proxy({a:1}, {ownKeys(t){return [1]}}); R = S(Reflect.ownKeys(p))");
T("var p = new Proxy({}, {ownKeys(t){return []}, getOwnPropertyDescriptor(){return {value:1,configurable:true,enumerable:true}}}); R = S(Object.entries(p)) + S(Object.getOwnPropertyDescriptor(p,'a'))");
T("var p = new Proxy({}, {getOwnPropertyDescriptor(){return {value:1,configurable:false}}}); R = S(Object.getOwnPropertyDescriptor(p,'a'))");
T("var t = {}; Object.defineProperty(t,'a',{value:1}); var p = new Proxy(t, {ownKeys(){return []}}); R = S(Object.keys(p))");
T("var t = Object.preventExtensions({a:1}); var p = new Proxy(t, {ownKeys(){return ['a','b']}}); R = S(Object.keys(p))");
T("var p = new Proxy({}, {getPrototypeOf(){return 1}}); R = S(Object.getPrototypeOf(p))");
T("var p = new Proxy({}, {getPrototypeOf(){return Array.prototype}}); R = S([Object.getPrototypeOf(p) === Array.prototype, Array.isArray(p), p instanceof Array, Array.prototype.isPrototypeOf(p)])");
T("var r = Proxy.revocable({}, {}); r.revoke(); R = [T(function(){return Object.keys(r.proxy)}), T(function(){return Object.prototype.toString.call(r.proxy)}), T(function(){return typeof r.proxy})].join(' | ')");
T("var r = Proxy.revocable([], {}); r.revoke(); R = [T(function(){return Array.isArray(r.proxy)}), T(function(){return Object.getPrototypeOf(r.proxy)}), T(function(){return JSON.stringify(r.proxy)})].join(' | ')");
T("var r = Proxy.revocable(function(){}, {}); r.revoke(); R = [T(function(){return typeof r.proxy}), T(function(){return Object.prototype.toString.call(r.proxy)}), T(function(){return r.proxy()})].join(' | ')");

// ---- 3. Object.is, groupBy, fromEntries, hasOwn, setPrototypeOf, create/defineProperties.
const isPairs = ["0,-0", "-0,-0", "NaN,NaN", "NaN,0/0", "1,'1'", "null,undefined", "[],[]", "'a','a'", "10n,10n", "Symbol.iterator,Symbol.iterator", "Symbol(),Symbol()", "Infinity,-Infinity", "Object,Object", "undefined,void 0", "0n,-0n"];
for (const p of isPairs) T(`R = S(Object.is(${p}))`);
T("R = S([Object.is.length, Object.is.name, Object.is(), Object.is(undefined)])");
const groupCases = [
  "Object.groupBy([1,2,3,4,5], x => x % 2 ? 'odd' : 'even')",
  "Object.groupBy('abcabc', c => c)",
  "Object.groupBy([], x => x)",
  "Object.groupBy(new Set([1,2,3]), x => x > 1)",
  "Object.groupBy(new Map([[1,2],[3,4]]), ([k, v]) => v > 2)",
  "Object.groupBy([1,2], (x, i) => i)",
  "Object.groupBy([1,2,3], x => Symbol.for('s' + (x % 2)))",
  "Object.getPrototypeOf(Object.groupBy([1], x => x))",
  "Object.keys(Object.groupBy([3,1,2,'b','a'], x => x))",
  "Object.groupBy([1,2], x => ({toString(){return 'k' + x}}))",
  "Object.groupBy(1, x => x)",
  "Object.groupBy(null, x => x)",
  "Object.groupBy([1])",
  "Object.groupBy([1], 5)",
  "Object.groupBy({length: 2, 0: 'a', 1: 'b'}, x => x)",
  "Map.groupBy([1,2,3,4], x => x % 2).get(1)",
  "Array.from(Map.groupBy([1,2,3], x => x % 2).keys())",
  "Map.groupBy([-0, 0], x => x).size",
  "Object.groupBy.length + ',' + Object.groupBy.name",
  "Map.groupBy.length + ',' + Map.groupBy.name",
  "D(Object, 'groupBy') + D(Map, 'groupBy')",
  "Object.groupBy(function*(){ yield 1; yield 2 }(), x => x)",
  "Object.groupBy([1,2,3], function(){ return typeof this })",
  "Object.groupBy('😀a', c => c.length)",
  "Object.isFrozen(Object.groupBy([1], x => x))",
];
for (const c of groupCases) T(`R = S(${c})`);
const fromEntriesCases = [
  "Object.fromEntries([['a',1],['b',2]])",
  "Object.fromEntries([['a',1],['a',2]])",
  "Object.fromEntries(new Map([[1,'x'],[{},'y']]))",
  "Object.fromEntries([])",
  "Object.fromEntries('ab')",
  "Object.fromEntries([[Symbol.for('k'), 1]])",
  "Object.fromEntries([[1]])",
  "Object.fromEntries([1])",
  "Object.fromEntries([[ 'a', 1, 2 ]])",
  "Object.fromEntries({})",
  "Object.fromEntries(null)",
  "Object.fromEntries()",
  "Object.fromEntries([['__proto__', 1]]).__proto__",
  "Object.getPrototypeOf(Object.fromEntries([['__proto__', {x:1}]])) === Object.prototype",
  "Object.keys(Object.fromEntries([['b',1],[2,1],['a',1],[1,1]]))",
  "Object.fromEntries(new URLSearchParams ? [] : [])",
  "Object.fromEntries({ *[Symbol.iterator]() { yield ['q', 1] } })",
  "Object.fromEntries([['a', 1]].values())",
  "D(Object.fromEntries([['a',1]]), 'a')",
  "Object.fromEntries([[{toString(){return 'tk'}}, 1]])",
  "Object.fromEntries(Object.entries({a:1,b:2}).map(([k,v]) => [k, v*2]))",
  "Object.fromEntries([['a',1]].concat([['length', 2]])).length",
  "Object.fromEntries.length + ',' + Object.fromEntries.name",
];
for (const c of fromEntriesCases) {
  if (/URLSearchParams/.test(c)) continue;
  T(`R = S(${c})`);
}
const hasOwnCases = [
  "Object.hasOwn({a:1}, 'a')", "Object.hasOwn({a:1}, 'toString')", "Object.hasOwn('abc', 1)", "Object.hasOwn('abc', 'length')",
  "Object.hasOwn([1], 0)", "Object.hasOwn([1], '0')", "Object.hasOwn([,1], 0)", "Object.hasOwn(null, 'a')", "Object.hasOwn(undefined, 'a')",
  "Object.hasOwn(1, 'a')", "Object.hasOwn({}, {toString(){return 'x'}})", "Object.hasOwn({x:1}, {toString(){return 'x'}})",
  "Object.hasOwn({[Symbol.iterator]:1}, Symbol.iterator)", "Object.hasOwn(function(){}, 'prototype')", "Object.hasOwn(()=>1, 'prototype')",
  "Object.hasOwn(class{}, 'prototype')", "Object.hasOwn(async function(){}, 'prototype')", "Object.hasOwn(function*(){}, 'prototype')",
  "Object.hasOwn(function(){}.bind(), 'prototype')", "Object.hasOwn({m(){}}.m, 'prototype')", "Object.hasOwn(Math.max, 'prototype')",
  "Object.hasOwn(new Uint8Array(2), 1)", "Object.hasOwn(new Uint8Array(2), 2)", "Object.hasOwn(new Uint8Array(2), '-0')", "Object.hasOwn(new Uint8Array(2), '1.5')",
  "Object.hasOwn(Object.create({a:1}), 'a')", "Object.hasOwn(new Proxy({a:1},{}), 'a')", "Object.hasOwn((function(){return arguments})(1), 'callee')",
  "Object.hasOwn((function(){'use strict';return arguments})(1), 'callee')", "Object.hasOwn(Object, 'hasOwn')", "Object.hasOwn.length", "Object.hasOwn.name",
  "Object.hasOwn(globalThis, 'globalThis')", "Object.hasOwn(Symbol.prototype, 'description')", "Object.hasOwn('abc', '1.0')",
];
for (const c of hasOwnCases) T(`R = S(${c})`);
const setProtoCases = [
  "var o = {}; Object.setPrototypeOf(o, Array.prototype); R = Array.isArray(o) + ',' + (o instanceof Array)",
  "R = S(Object.setPrototypeOf(1, null))",
  "R = Object.setPrototypeOf(undefined, null)",
  "R = Object.setPrototypeOf({}, 1)",
  "R = Object.setPrototypeOf({})",
  "var o = Object.preventExtensions({}); R = Object.setPrototypeOf(o, null)",
  "var o = Object.preventExtensions({}); R = S(Object.setPrototypeOf(o, Object.prototype) === o)",
  "var a = {}, b = Object.create(a); R = Object.setPrototypeOf(a, b)",
  "R = Object.setPrototypeOf(Object.prototype, {})",
  "R = S(Object.setPrototypeOf(Object.prototype, null) === Object.prototype)",
  "var o = Object.setPrototypeOf({a:1}, null); R = S([Object.getPrototypeOf(o), Object.prototype.toString.call(o), 'toString' in o, T(function(){return o + ''})])",
  "var o = {}; Object.setPrototypeOf(o, function(){}); R = typeof o + ',' + typeof o.call",
  "var o = {}; R = S(Reflect.setPrototypeOf(o, o))",
  "var f = function(){}; Object.setPrototypeOf(f, null); R = S([typeof f, T(function(){return f()}), T(function(){return f.call})])",
  "R = S([Object.setPrototypeOf.length, Object.setPrototypeOf.name, Object.getPrototypeOf.length, Object.getPrototypeOf.name])",
  "R = S([Object.getPrototypeOf(1) === Number.prototype, Object.getPrototypeOf('a') === String.prototype, T(function(){return Object.getPrototypeOf(null)}), T(function(){return Object.getPrototypeOf(undefined)})])",
  "R = S([Object.getPrototypeOf(Object.prototype), Object.getPrototypeOf(Function.prototype) === Object.prototype, Object.getPrototypeOf(Object) === Function.prototype])",
  "R = S([Object.getPrototypeOf(Int8Array) === Object.getPrototypeOf(Uint8Array), Object.getPrototypeOf(Int8Array).name, Object.getPrototypeOf(Int8Array.prototype) === Object.getPrototypeOf(Uint8Array.prototype)])",
  "R = S([Object.getPrototypeOf(function*(){}) === Object.getPrototypeOf(function*(){}), Object.getPrototypeOf(function*(){}).constructor.name, Object.getPrototypeOf(async function(){}).constructor.name, Object.getPrototypeOf(async function*(){}).constructor.name])",
  "R = S([Object.getPrototypeOf(class A extends null{}) === Function.prototype, Object.getPrototypeOf((class A extends null{}).prototype)])",
  "R = S([Object.getPrototypeOf(Error) === Function.prototype, Object.getPrototypeOf(TypeError) === Error, Object.getPrototypeOf(TypeError.prototype) === Error.prototype])",
  "var o = {__proto__: null, a: 1}; R = S([Object.getPrototypeOf(o), Object.keys(o)])",
  "var o = {'__proto__': Array.prototype}; R = S(Object.getPrototypeOf(o) === Array.prototype)",
  "var o = {['__proto__']: Array.prototype}; R = S([Object.getPrototypeOf(o) === Object.prototype, Object.keys(o)])",
  "var __proto__ = Array.prototype; var o = {__proto__}; R = S([Object.getPrototypeOf(o) === Object.prototype, Object.keys(o)])",
  "var o = {__proto__(){}}; R = S([Object.getPrototypeOf(o) === Object.prototype, Object.keys(o)])",
  "var o = {__proto__: 5}; R = S(Object.getPrototypeOf(o) === Object.prototype)",
  "var o = {__proto__: 1, __proto__: 2}",
];
for (const c of setProtoCases) T(c);
const createCases = [
  "Object.create(null)",
  "Object.create({a:1}, {b:{value:2,enumerable:true}})",
  "DS(Object.create({}, {a:{value:1}, b:{get(){return 1}}, c:{set(v){}}, d:{get(){}, set(v){}, enumerable:true}}))",
  "DS(Object.create({}, {a:{}}))",
  "DS(Object.create({}, {a:{value:undefined}}))",
  "Object.create(1)",
  "Object.create()",
  "Object.create({}, null)",
  "Object.create({}, undefined)",
  "Object.create({}, 1)",
  "Object.create({}, 'ab')",
  "Object.create({}, {a:1})",
  "Object.create({}, {a:{get:1}})",
  "Object.create({}, {a:{get(){}, value:1}})",
  "Object.create({}, {a:{set:null}})",
  "Object.create({}, {a:{value:1,writable:true,enumerable:true,configurable:true}}).a",
  "Object.create({}, Object.create({x:{value:1}}))",
  "Object.keys(Object.create({}, {b:{value:1,enumerable:true},a:{value:1,enumerable:true},1:{value:1,enumerable:true}}))",
  "Reflect.ownKeys(Object.create({}, {[Symbol.iterator]:{value:1}, a:{value:1}, 0:{value:2}}))",
  "Object.getPrototypeOf(Object.create(Array.prototype)) === Array.prototype",
  "Array.isArray(Object.create(Array.prototype))",
  "Object.create(Array.prototype).length",
  "Object.create(function(){})",
  "Object.create(Object.create(null)).toString",
  "DS(Object.defineProperties({}, {a:{value:1,enumerable:true}, b:{get(){return 2}}}))",
  "Object.defineProperties({}, 1)",
  "Object.defineProperties(1, {})",
  "Object.defineProperties()",
  "var o = Object.freeze({}); Object.defineProperties(o, {a:{value:1}})",
  "var o = {}; Object.defineProperties(o, {a:{value:1}, b:{get:1}}); Object.getOwnPropertyNames(o)",
  "var o = {a:1}; Object.defineProperty(o, 'a', {enumerable:false}); DS(o)",
  "var o = {a:1}; Object.defineProperty(o, 'a', {get(){return 9}}); DS(o)",
  "var o = {get a(){return 1}}; Object.defineProperty(o, 'a', {value: 5}); DS(o)",
  "var o = Object.defineProperty({}, 'a', {value:1}); Object.defineProperty(o, 'a', {value:2})",
  "var o = Object.defineProperty({}, 'a', {value:1}); Object.defineProperty(o, 'a', {value:1})",
  "var o = Object.defineProperty({}, 'a', {value:NaN}); Object.defineProperty(o, 'a', {value:NaN})",
  "var o = Object.defineProperty({}, 'a', {value:0}); Object.defineProperty(o, 'a', {value:-0})",
  "var o = Object.defineProperty({}, 'a', {value:1,writable:true}); Object.defineProperty(o, 'a', {value:2}); DS(o)",
  "var o = Object.defineProperty({}, 'a', {value:1,writable:true}); Object.defineProperty(o, 'a', {writable:false}); Object.defineProperty(o, 'a', {writable:true})",
  "var o = Object.defineProperty({}, 'a', {get(){}}); Object.defineProperty(o, 'a', {set(v){}})",
  "var a = []; Object.defineProperty(a, 'length', {value: 1}); DS(a)",
  "var a = [1,2,3]; Object.defineProperty(a, 'length', {value: 1}); S(a)",
  "var a = [1,2,3]; Object.defineProperty(a, 'length', {writable:false}); a.push(1)",
  "var a = [1,2,3]; Object.defineProperty(a, 1, {configurable:false}); a.length = 0; S(a) + a.length",
  "var a = []; Object.defineProperty(a, 'length', {value: -1})",
  "var a = []; Object.defineProperty(a, 4294967295, {value: 1}); a.length",
  "var a = []; Object.defineProperty(a, 4294967294, {value: 1}); a.length",
  "var a = []; Object.defineProperty(a, 'length', {get(){}})",
  "var t = new Uint8Array(2); Object.defineProperty(t, 0, {value: 300}); t[0]",
  "var t = new Uint8Array(2); Object.defineProperty(t, 0, {value: 1, writable:false})",
  "var t = new Uint8Array(2); Object.defineProperty(t, 5, {value: 1})",
  "var t = new Uint8Array(2); Object.defineProperty(t, 0, {get(){}})",
  "var t = new Uint8Array(2); Object.defineProperty(t, 'x', {value: 1}); DS(t)",
  "var s = new String('ab'); Object.defineProperty(s, 0, {value: 'a'})",
  "var s = new String('ab'); Object.defineProperty(s, 0, {value: 'z'})",
  "var s = new String('ab'); Object.defineProperty(s, 2, {value: 'z'}); DS(s)",
  "var f = function(a){}; Object.defineProperty(f, 'name', {value: 'q'}); f.name + f.length",
  "var f = function(a){}; Object.defineProperty(f, 'length', {value: 7}); f.length + DS(f)",
  "var f = function(a){}; delete f.name; delete f.length; S([f.name, f.length, Object.getOwnPropertyNames(f)])",
  "var f = function(){}; Object.defineProperty(f, 'prototype', {value: 1}); DS(f)",
  "var f = function(){}; Object.defineProperty(f, 'prototype', {writable:false}); f.prototype = 3; D(f, 'prototype')",
  "var a = (function(){return arguments})(1,2); Object.defineProperty(a, 0, {value: 9}); S([a[0], a.length])",
  "var f = function(x){ Object.defineProperty(arguments, 0, {value: 9}); return x }; f(1)",
  "var f = function(x){ Object.defineProperty(arguments, 0, {writable:false}); x = 5; return arguments[0] }; f(1)",
  "var f = function(x){ Object.defineProperty(arguments, 0, {get(){return 7}}); x = 5; return arguments[0] + ',' + x }; f(1)",
  "var f = function(x){ arguments[0] = 4; return x }; f(1)",
  "var f = function(x){ 'use strict'; arguments[0] = 4; return x }; f(1)",
  "var f = function(x){ x = 4; return arguments[0] }; f()",
  "var f = function(x = 0){ arguments[0] = 4; return x }; f(1)",
  "var f = function(){ return D(arguments, 'callee') + D(arguments, 'length') + D(arguments, Symbol.iterator) }; f(1)",
  "var f = function(){ 'use strict'; var d = Object.getOwnPropertyDescriptor(arguments, 'callee'); return S([typeof d.get, d.get === d.set, d.enumerable, d.configurable]) }; f()",
  "var f = function(){ 'use strict'; return T(function(){ return arguments.callee }) }; f()",
  "var f = function(){ return arguments.callee === f }; f()",
  "var f = function(){ return Object.prototype.toString.call(arguments) + Reflect.ownKeys(arguments).map(K) }; f(1,2)",
];
for (const c of createCases) {
  if (/^(var |R =)/.test(c) || /;/.test(c)) {
    // sequência de instruções: a última expressão é o resultado
    const idx = c.lastIndexOf("; ");
    if (idx >= 0 && !c.startsWith("R =")) T(`${c.slice(0, idx)}; R = S(${c.slice(idx + 2)})`);
    else T(c);
  } else T(`R = S(${c})`);
}

// ---- 4. Ordem de chaves.
const orderCases = [
  "var o = {b:1, 2:1, a:1, 1:1, [Symbol('s')]:1, '3':1, '-1':1}; R = S(Reflect.ownKeys(o))",
  "var o = {}; o.z = 1; o[1] = 1; o.a = 1; o[0] = 1; o[Symbol.iterator] = 1; o[Symbol.for('x')] = 1; R = S(Reflect.ownKeys(o))",
  "var o = {}; o[4294967295] = 1; o[4294967294] = 1; o[4294967296] = 1; o['1e3'] = 1; o[1000] = 1; R = S(Reflect.ownKeys(o))",
  "var o = {}; o['007'] = 1; o[7] = 1; o['7.0'] = 1; o['-0'] = 1; o[-0] = 1; o[0] = 1; R = S(Reflect.ownKeys(o))",
  "var o = {}; o[2**32] = 1; o[2**31] = 1; o[2**53] = 1; o[9007199254740993] = 1; R = S(Reflect.ownKeys(o))",
  "var p = {b:1, 1:1}; var o = Object.create(p); o.a = 1; o[0] = 1; R = S((function(){var r=[];for(var k in o)r.push(k);return r})())",
  "var p = {a:1}; var o = Object.create(p); o.a = 2; o.b = 1; R = S((function(){var r=[];for(var k in o)r.push(k);return r})())",
  "var p = {a:1}; var o = Object.create(p); Object.defineProperty(o, 'a', {value:2, enumerable:false}); R = S((function(){var r=[];for(var k in o)r.push(k);return r})())",
  "var p = {1:1, b:1}; var o = Object.create(p); o.c = 1; o[0] = 1; R = S([Object.keys(o), (function(){var r=[];for(var k in o)r.push(k);return r})()])",
  "var o = {a:1,b:2,c:3}; var r = []; for (var k in o) { r.push(k); delete o.b; o.d = 1 } R = S(r)",
  "var o = {a:1,b:2}; delete o.a; o.a = 3; R = S(Object.keys(o))",
  "var o = {a:1,b:2}; Object.defineProperty(o, 'a', {value: 9}); R = S(Object.keys(o))",
  "var o = {a:1,b:2}; Object.defineProperty(o, 'a', {get(){}, configurable:true, enumerable:true}); R = S(Object.keys(o))",
  "var o = {2:'a', 1:'b'}; R = JSON.stringify(o) + Object.values(o)",
  "var o = {b:1, a:2}; R = JSON.stringify(Object.entries(o))",
  "var a = [1,2]; a.x = 1; a[5] = 1; a[Symbol.iterator] = 1; R = S(Reflect.ownKeys(a))",
  "var s = new String('ab'); s.x = 1; s[5] = 1; R = S(Reflect.ownKeys(s))",
  "var t = new Uint8Array(2); t.x = 1; t[Symbol.for('q')] = 1; R = S(Reflect.ownKeys(t))",
  "var f = function(a,b){}; f.x = 1; f[0] = 1; f[Symbol.for('q')] = 1; R = S(Reflect.ownKeys(f))",
  "var f = class { static a(){}; static b = 1; static [Symbol.for('z')] = 1; static 1 = 1 }; R = S(Reflect.ownKeys(f))",
  "var f = class { static get [Symbol.species](){return 1} static a(){} }; R = S(Reflect.ownKeys(f))",
  "var f = function(){}; R = S(Reflect.ownKeys(f))",
  "var f = function(){'use strict'}; R = S(Reflect.ownKeys(f))",
  "var f = () => 1; R = S(Reflect.ownKeys(f))",
  "var f = async function(){}; R = S(Reflect.ownKeys(f))",
  "var f = function*(){}; R = S(Reflect.ownKeys(f))",
  "var f = function(){}.bind(); R = S(Reflect.ownKeys(f))",
  "var f = ({m(){}}).m; R = S(Reflect.ownKeys(f))",
  "var f = ({get g(){return 1}}); R = S(Reflect.ownKeys(Object.getOwnPropertyDescriptor(f,'g').get))",
  "R = S(Reflect.ownKeys(class {}))",
  "R = S(Reflect.ownKeys(class A { constructor(a,b){} }))",
  "R = S(Reflect.ownKeys(class A { static name(){} }))",
  "R = S(Reflect.ownKeys(class A { static name = 1 }))",
  "R = S(Reflect.ownKeys(class { static name = 1 }))",
  "R = S(Reflect.ownKeys(class extends Array {}))",
  "R = S(Reflect.ownKeys((class { m(){} get g(){return 1} static s(){} }).prototype))",
  "R = S(Reflect.ownKeys(function(){}.prototype))",
  "R = S(Reflect.ownKeys((function*(){}).prototype))",
  "R = S(Reflect.ownKeys(Symbol))",
  "R = S(Reflect.ownKeys(Math).length)",
  "R = S(Reflect.ownKeys(globalThis).filter(k => typeof k === 'symbol'))",
  "var o = {a:1}; Object.freeze(o); R = S([Reflect.ownKeys(o), DS(o)])",
  "var o = {a:1, get b(){return 1}}; Object.seal(o); R = DS(o)",
  "var a = [1,2]; Object.freeze(a); R = S([DS(a), Object.isFrozen(a)])",
  "var t = new Uint8Array(2); R = T(function(){ return S(Object.freeze(t)) })",
  "var t = new Uint8Array(0); R = T(function(){ return S(Object.isFrozen(Object.freeze(t))) })",
  "var t = new Uint8Array(2); R = S([Object.isSealed(Object.seal(t)), Object.isFrozen(t)])",
  "R = S([Object.isFrozen(1), Object.isSealed('a'), Object.isExtensible(1), Object.freeze(1), Object.seal(1), Object.preventExtensions(1)])",
  "R = S([Object.isFrozen(Object.preventExtensions({})), Object.isFrozen(Object.preventExtensions({a:1})), Object.isSealed(Object.preventExtensions({a:1}))])",
  "var s = Object.freeze('abc'); R = S(s)",
  "var s = Object.freeze(new String('abc')); R = S([Object.isFrozen(s), DS(s)])",
  "R = S(Object.getOwnPropertyNames('abc'))",
  "R = S(Object.getOwnPropertyNames(''))",
  "R = S(Object.entries('ab'))",
  "R = S(Object.entries([,'a']))",
  "R = S(Object.values({get a(){ delete this.b; return 1 }, b: 2}))",
  "R = S(Object.entries({get a(){ delete this.b; return 1 }, b: 2}))",
  "R = S(Object.keys({get a(){ delete this.b; return 1 }, b: 2}))",
  "var o = {a:1, b:2}; var r = Object.entries(o); r[0][1] = 9; R = S(o)",
  "var o = {a:1}; var k = Object.keys(o); k.push('z'); R = S([o, Object.keys(o)])",
  "var a = {a:1, get b(){ return 2 }, set c(v){}}; R = S([Object.assign({}, a), Object.keys(a), Object.entries(a)])",
  "var target = {set a(v){ this.log = v }}; Object.assign(target, {a: 5}); R = S(target)",
  "var target = Object.freeze({}); R = T(function(){ return Object.assign(target, {a:1}) })",
  "var target = Object.defineProperty({}, 'a', {value:1}); R = T(function(){ return Object.assign(target, {a:2}) })",
  "R = S(Object.assign(1, {a:1}))",
  "R = S(typeof Object.assign(1, {a:1}))",
  "R = S(Object.assign({}, null, undefined, 'ab', 1, true, [9], {k:1}))",
  "R = Object.assign()",
  "R = Object.assign(null)",
  "R = S(Object.assign({}, {[Symbol.for('a')]:1, b:2}))",
  "R = S(Object.assign({}, Object.defineProperty({}, 'h', {value:1})))",
  "R = S(Object.assign({}, Object.create({inh:1}, {own:{value:1,enumerable:true}})))",
  "R = S(Object.assign([1,2,3], [4,5]))",
  "R = S(Object.assign([1], {length: 0}))",
  "R = S(Object.assign(new String('ab'), {0: 'z'}))",
  "R = S(T(function(){ return Object.assign(new String('ab'), {0: 'z'}) }))",
  "R = Object.assign.length + Object.assign.name",
  "R = S(Object.assign({}, new Proxy({a:1}, {})))",
  "R = S({...'ab', ...[1], ...null, ...1})",
  "R = S({...{get a(){return 1}}})",
  "R = S({...Object.create({inh:1})})",
  "R = S(Object.entries(Object.create({inh:1}, {own:{value:1,enumerable:true}})))",
  "R = S(Object.entries(Math))",
  "R = S(Object.entries(function(){}))",
  "R = S(Object.getOwnPropertyNames(function(){}).concat(Object.getOwnPropertyNames(()=>1)))",
  "R = S(Object.getOwnPropertyNames(Object.prototype).sort())",
  "R = S(Object.getOwnPropertyNames(Object).sort())",
  "R = S(Object.getOwnPropertyNames(Function.prototype).sort())",
  "R = S(Object.getOwnPropertyNames(Reflect).sort())",
  "R = S(Object.getOwnPropertySymbols(Object.prototype))",
  "R = S(Object.getOwnPropertySymbols(Symbol.prototype))",
  "R = S(Object.getOwnPropertySymbols(Array.prototype))",
  "R = S(Object.getOwnPropertySymbols(String.prototype))",
  "R = S(Object.getOwnPropertySymbols(RegExp.prototype))",
  "R = S(Object.getOwnPropertySymbols(Function.prototype))",
  "R = S(Object.getOwnPropertySymbols(Math))",
  "R = S(Object.getOwnPropertySymbols(Map.prototype))",
  "R = S(Object.getOwnPropertySymbols(Promise.prototype))",
  "R = S(Object.getOwnPropertySymbols(Date.prototype))",
  "R = S(Object.getOwnPropertySymbols(Array.prototype[Symbol.unscopables]))",
  "R = S(Object.keys(Array.prototype[Symbol.unscopables]))",
  "R = S(Object.getPrototypeOf(Array.prototype[Symbol.unscopables]))",
  "R = S(Object.getOwnPropertySymbols(Symbol))",
  "R = S(Object.getOwnPropertyNames(Symbol).sort())",
];
for (const c of orderCases) T(c);

// ---- 5. Object.prototype.toString e Symbol.toStringTag.
const tagInstances = [
  "undefined", "null", "1", "'s'", "true", "Symbol()", "1n", "[]", "{}", "function(){}", "()=>1", "class{}", "async function(){}",
  "function*(){}", "async function*(){}", "function(){return arguments}()", "new Date(0)", "/r/", "new Error", "new TypeError", "new AggregateError([])",
  "new Number(1)", "new String('s')", "new Boolean(true)", "Object(Symbol())", "Object(1n)", "new Map", "new Set", "new WeakMap", "new WeakSet", "new WeakRef({})",
  "new FinalizationRegistry(()=>{})", "Promise.resolve()", "new ArrayBuffer(1)", "new SharedArrayBuffer(1)", "new DataView(new ArrayBuffer(1))",
  "new Int8Array(1)", "new Uint8Array(1)", "new Uint8ClampedArray(1)", "new Int16Array(1)", "new Uint16Array(1)", "new Int32Array(1)", "new Uint32Array(1)",
  "new Float32Array(1)", "new Float64Array(1)", "new BigInt64Array(1)", "new BigUint64Array(1)", "Math", "JSON", "Reflect", "Atomics", "Intl", "WebAssembly",
  "globalThis", "Proxy", "new Proxy({}, {})", "new Proxy([], {})", "new Proxy(function(){}, {})", "(function*(){})()", "(async function*(){})()",
  "[][Symbol.iterator]()", "new Map()[Symbol.iterator]()", "new Set()[Symbol.iterator]()", "''[Symbol.iterator]()", "/a/g[Symbol.matchAll]('a')",
  "Iterator.prototype", "Iterator.from([1])", "[].values().map(x=>x)", "[].values().filter(x=>x)", "[].values().take(1)", "[].values().drop(1)", "[].values().flatMap(x=>[])",
  "Object.getPrototypeOf(Iterator.from({next(){}}))", "Object.getPrototypeOf(function*(){})", "Object.getPrototypeOf(function*(){}).prototype",
  "Object.getPrototypeOf(async function*(){}).prototype", "Object.getPrototypeOf(async function(){})",
  "Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype)", "Object.getPrototypeOf([][Symbol.iterator]())",
  "Object.getPrototypeOf(new Map()[Symbol.iterator]())", "Object.getPrototypeOf(new Set()[Symbol.iterator]())", "Object.getPrototypeOf(''[Symbol.iterator]())",
  "Object.getPrototypeOf(/a/[Symbol.matchAll](''))", "Object.getPrototypeOf(Int8Array)", "Object.getPrototypeOf(Int8Array).prototype",
  "Symbol.prototype", "BigInt.prototype", "Map.prototype", "Set.prototype", "WeakMap.prototype", "WeakSet.prototype", "WeakRef.prototype", "FinalizationRegistry.prototype",
  "Promise.prototype", "ArrayBuffer.prototype", "SharedArrayBuffer.prototype", "DataView.prototype", "Date.prototype", "RegExp.prototype", "Error.prototype",
  "Array.prototype", "String.prototype", "Number.prototype", "Boolean.prototype", "Function.prototype", "Object.prototype",
  "Intl.Collator.prototype", "Intl.DateTimeFormat.prototype", "Intl.NumberFormat.prototype", "Intl.PluralRules.prototype", "Intl.RelativeTimeFormat.prototype",
  "Intl.ListFormat.prototype", "Intl.Locale.prototype", "Intl.DisplayNames.prototype", "Intl.Segmenter.prototype",
  "new Intl.Collator", "new Intl.DateTimeFormat", "new Intl.NumberFormat", "new Intl.PluralRules", "new Intl.RelativeTimeFormat", "new Intl.ListFormat",
  "new Intl.Locale('en')", "new Intl.DisplayNames('en', {type:'region'})", "new Intl.Segmenter", "new Intl.Segmenter().segment('a')",
  "new Intl.Segmenter().segment('a')[Symbol.iterator]()",
  "WebAssembly.Module.prototype", "WebAssembly.Instance.prototype", "WebAssembly.Memory.prototype", "WebAssembly.Table.prototype", "WebAssembly.Global.prototype",
  "WebAssembly.Tag.prototype", "WebAssembly.Exception.prototype", "WebAssembly.CompileError.prototype", "WebAssembly.LinkError.prototype", "WebAssembly.RuntimeError.prototype",
  "new WebAssembly.Memory({initial:1})", "new WebAssembly.Table({element:'anyfunc', initial:1})", "new WebAssembly.Global({value:'i32'})",
  "new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]))", "new WebAssembly.CompileError", "new WebAssembly.RuntimeError", "new WebAssembly.LinkError",
  "(0,eval)('arguments', 0) || 0",
  "{[Symbol.toStringTag]: 'Custom'}", "{[Symbol.toStringTag]: 1}", "{[Symbol.toStringTag]: undefined}", "{[Symbol.toStringTag]: Symbol()}",
  "Object.assign([], {[Symbol.toStringTag]: 'X'})", "Object.assign(function(){}, {[Symbol.toStringTag]: 'X'})", "Object.assign(new Date, {[Symbol.toStringTag]: 'X'})",
  "Object.defineProperty([], Symbol.toStringTag, {get(){ return 'G' }})", "Object.defineProperty({}, Symbol.toStringTag, {get(){ throw new RangeError('boom') }})",
  "Object.create(Map.prototype)", "Object.create(Promise.prototype)", "Object.create(Date.prototype)", "Object.create(Array.prototype)", "Object.create(Error.prototype)",
  "Object.create(RegExp.prototype)", "Object.create(Boolean.prototype)", "Object.create(Number.prototype)", "Object.create(String.prototype)",
  "Object.setPrototypeOf({}, null)", "Object.setPrototypeOf([], null)", "Object.setPrototypeOf(function(){}, null)", "Object.setPrototypeOf(new Error, null)",
  "Object.setPrototypeOf(new Map, null)", "Object.setPrototypeOf(new Date, null)", "Object.setPrototypeOf(/r/, null)", "Object.setPrototypeOf(new Boolean(1), null)",
  "class A { get [Symbol.toStringTag]() { return 'AA' } }; new A", "class A extends Map {}; new A", "class A extends Array {}; new A", "class A extends Error {}; new A",
  "class A extends Promise {}; new A(()=>{})", "class A extends Uint8Array {}; new A(1)", "class A extends Date {}; new A",
];
for (const e of tagInstances) {
  T(`var v = (function(){ return (${e.includes(";") ? "function(){ " + e.replace(/;\s*(new [^;]+)$/, "; return $1") + " }()" : e}) })(); R = Object.prototype.toString.call(v)`);
}
const tagOwners = [
  "Symbol.prototype", "BigInt.prototype", "Map.prototype", "Set.prototype", "WeakMap.prototype", "WeakSet.prototype", "WeakRef.prototype",
  "FinalizationRegistry.prototype", "Promise.prototype", "ArrayBuffer.prototype", "SharedArrayBuffer.prototype", "DataView.prototype", "Math", "JSON", "Reflect",
  "Atomics", "Intl", "WebAssembly", "Object.getPrototypeOf(Int8Array).prototype", "Int8Array.prototype", "Iterator.prototype",
  "Object.getPrototypeOf(function*(){})", "Object.getPrototypeOf(function*(){}).prototype", "Object.getPrototypeOf(async function*(){})",
  "Object.getPrototypeOf(async function*(){}).prototype", "Object.getPrototypeOf(async function(){})", "Object.getPrototypeOf([][Symbol.iterator]())",
  "Object.getPrototypeOf(new Map()[Symbol.iterator]())", "Object.getPrototypeOf(new Set()[Symbol.iterator]())", "Object.getPrototypeOf(''[Symbol.iterator]())",
  "Object.getPrototypeOf(/a/[Symbol.matchAll](''))", "Intl.Collator.prototype", "Intl.DateTimeFormat.prototype", "Intl.NumberFormat.prototype",
  "Intl.PluralRules.prototype", "Intl.RelativeTimeFormat.prototype", "Intl.ListFormat.prototype", "Intl.Locale.prototype", "Intl.DisplayNames.prototype",
  "Intl.Segmenter.prototype", "WebAssembly.Module.prototype", "WebAssembly.Instance.prototype", "WebAssembly.Memory.prototype", "WebAssembly.Table.prototype",
  "WebAssembly.Global.prototype", "WebAssembly.Tag.prototype", "WebAssembly.Exception.prototype", "Object.getPrototypeOf([].values().map(x=>x))",
  "Object.getPrototypeOf(Iterator.from({next(){}}))", "Object.getPrototypeOf(new Intl.Segmenter().segment('a'))",
  "Object.getPrototypeOf(new Intl.Segmenter().segment('a')[Symbol.iterator]())", "Date.prototype", "Error.prototype", "Array.prototype", "String.prototype",
  "Number.prototype", "Boolean.prototype", "RegExp.prototype", "Function.prototype", "Object.prototype", "globalThis", "Proxy", "Symbol",
];
for (const o of tagOwners) {
  T(`var o = ${o}; R = S([D(o, Symbol.toStringTag), o[Symbol.toStringTag], Object.prototype.toString.call(o)])`);
}
T("R = S(Object.prototype.toString.call(function(){return arguments}.call(null)))");
T("R = [undefined, null].map(v => Object.prototype.toString.call(v)).join()");
T("R = Object.prototype.toString.length + Object.prototype.toString.name");
T("var o = {}; R = o.toString === Object.prototype.toString");
T("R = S([Object.prototype.toString.call(), Object.prototype.toString.apply(1), Object.prototype.toString.bind(true)()])");
T("var d = new Date(0); d[Symbol.toStringTag] = 'X'; R = Object.prototype.toString.call(d)");
T("'use strict'; var d = new Date(0); R = T(function(){ d[Symbol.toStringTag] = 'X'; return Object.prototype.toString.call(d) })");
T("var o = Object.defineProperty({}, Symbol.toStringTag, {value: 'Q', writable: false}); R = S([Object.prototype.toString.call(o), String(o), `${o}`, o + ''])");
T("var o = {toString(){return 'ts'}, valueOf(){return 7}}; R = S([String(o), `${o}`, o + '', o * 1, [o] + '', Object.prototype.toString.call(o)])");
T("R = S([String(Object.create(null) instanceof Object), T(function(){ return String(Object.create(null)) }), T(function(){ return `${Object.create(null)}` })])");
T("R = S([String(Symbol.iterator), T(function(){ return Symbol.iterator + '' }), T(function(){ return `${Symbol.iterator}` })])");
T("R = S([String([1,[2,3]]), String({}), String(function f(){}), String(class A{}), String(null), String(undefined), String(-0), String(1n)])");
T("R = S(String(Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get))");
T("R = S([T(function(){ return Math + '' }), JSON + '', Reflect + '', Atomics + '', Intl + '', WebAssembly + '', globalThis + ''])");

// ---- 6. propertyIsEnumerable, __lookupGetter__, __proto__, toLocaleString, valueOf, isPrototypeOf.
const pieCases = [
  "({a:1}).propertyIsEnumerable('a')", "({a:1}).propertyIsEnumerable('b')", "[1].propertyIsEnumerable(0)", "[1].propertyIsEnumerable('length')",
  "'abc'.propertyIsEnumerable(0)", "'abc'.propertyIsEnumerable('length')", "new String('abc').propertyIsEnumerable(1)", "Object.propertyIsEnumerable.call(null, 'a')",
  "Object.prototype.propertyIsEnumerable.call(undefined, 'a')", "Object.prototype.propertyIsEnumerable.call(1, 'a')", "Object.prototype.propertyIsEnumerable.call({}, {toString(){ throw new RangeError('k') }})",
  "Object.prototype.propertyIsEnumerable.call(null, {toString(){ throw new RangeError('k') }})", "Object.propertyIsEnumerable('prototype')",
  "(function(){}).propertyIsEnumerable('prototype')", "(function(){}).propertyIsEnumerable('name')", "Math.propertyIsEnumerable('PI')",
  "globalThis.propertyIsEnumerable('Array')", "globalThis.propertyIsEnumerable('globalThis')", "Object.create({a:1}).propertyIsEnumerable('a')",
  "Object.prototype.propertyIsEnumerable.call({[Symbol.iterator]: 1}, Symbol.iterator)", "Object.defineProperty({}, 'h', {value:1}).propertyIsEnumerable('h')",
  "new Uint8Array(2).propertyIsEnumerable(1)", "new Uint8Array(2).propertyIsEnumerable(2)", "new Uint8Array(2).propertyIsEnumerable('length')",
  "(function(){return arguments})(1).propertyIsEnumerable('length')", "(function(){return arguments})(1).propertyIsEnumerable(0)",
  "(function(){return arguments})(1).propertyIsEnumerable('callee')", "new Proxy({a:1}, {}).propertyIsEnumerable('a')",
  "Object.prototype.propertyIsEnumerable.length", "Object.prototype.propertyIsEnumerable.name",
];
for (const c of pieCases) T(`R = S(${c})`);
const lookupCases = [
  "var o = {get a(){return 1}}; typeof o.__lookupGetter__('a')",
  "var o = {get a(){return 1}}; o.__lookupSetter__('a')",
  "var o = {set a(v){}}; typeof o.__lookupSetter__('a') + typeof o.__lookupGetter__('a')",
  "var o = {a: 1}; o.__lookupGetter__('a')",
  "var o = Object.create({get a(){return 1}}); typeof o.__lookupGetter__('a')",
  "var o = Object.create({get a(){return 1}}); Object.defineProperty(o, 'a', {value: 1}); o.__lookupGetter__('a')",
  "Object.prototype.__lookupGetter__.call(null, 'a')",
  "Object.prototype.__lookupGetter__.call(1, 'a')",
  "Object.prototype.__lookupGetter__.call({}, {toString(){ throw new RangeError('k') }})",
  "var o = {}; o.__defineGetter__('a', function(){ return 5 }); [o.a, D(o, 'a')]",
  "var o = {}; o.__defineSetter__('a', function(v){ this._ = v }); o.a = 3; [o._, D(o, 'a')]",
  "var o = {}; o.__defineGetter__('a', 1)",
  "var o = {}; o.__defineSetter__('a', undefined)",
  "Object.prototype.__defineGetter__.call(null, 'a', function(){})",
  "Object.freeze({}).__defineGetter__('a', function(){})",
  "var o = Object.defineProperty({}, 'a', {value:1}); o.__defineGetter__('a', function(){})",
  "var o = {a:1}; o.__defineGetter__('a', function(){ return 2 }); [o.a, D(o, 'a')]",
  "[Object.prototype.__lookupGetter__.length, Object.prototype.__lookupSetter__.length, Object.prototype.__defineGetter__.length, Object.prototype.__defineSetter__.length]",
  "[Object.prototype.__lookupGetter__.name, Object.prototype.__defineSetter__.name]",
  "Object.prototype.__lookupGetter__.call(new Proxy({}, {getOwnPropertyDescriptor(t,k){ return {get: function(){}, configurable: true} }}), 'x') !== undefined",
  "var o = Object.create(new Proxy({}, {getOwnPropertyDescriptor(t,k){ return {get: function(){}, configurable: true} }})); typeof o.__lookupGetter__('x')",
  "var l = []; var o = Object.create(new Proxy({}, {getOwnPropertyDescriptor(t,k){ l.push('g:' + K(k)); return undefined }, getPrototypeOf(){ l.push('gpo'); return null }})); o.__lookupGetter__('x'); l",
  "typeof Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get + typeof Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); [d.get.name, d.set.name, d.get.length, d.set.length, d.enumerable, d.configurable]",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); [d.get.call(1) === Number.prototype, T(function(){ return d.get.call(null) }), T(function(){ return d.get.call(undefined) })]",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); [d.set.call(1, {}), T(function(){ return d.set.call(null, {}) }), T(function(){ return d.set.call({}, 1) }), T(function(){ return d.set.call({}) })]",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); var o = {}; [d.set.call(o, null), Object.getPrototypeOf(o)]",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); var o = Object.preventExtensions({}); T(function(){ return d.set.call(o, null) })",
  "var o = {}; o.__proto__ = Array.prototype; [Array.isArray(o), o instanceof Array, Object.keys(o)]",
  "var o = {}; o.__proto__ = 5; Object.getPrototypeOf(o) === Object.prototype",
  "var o = Object.create(null); o.__proto__ = Array.prototype; [Object.getPrototypeOf(o), Object.keys(o)]",
  "var o = {}; Object.defineProperty(o, '__proto__', {value: 1, enumerable: true}); [Object.keys(o), o.__proto__, Object.getPrototypeOf(o) === Object.prototype]",
  "var o = {['__proto__']: 1}; [Object.keys(o), Object.getPrototypeOf(o) === Object.prototype]",
  "var o = JSON.parse('{\"__proto__\": {\"x\": 1}}'); [Object.keys(o), Object.getPrototypeOf(o) === Object.prototype, o.x]",
  "var o = Object.assign({}, JSON.parse('{\"__proto__\": {\"x\": 1}}')); [Object.keys(o), o.x, Object.getPrototypeOf(o) === Object.prototype]",
  "var o = {...JSON.parse('{\"__proto__\": {\"x\": 1}}')}; [Object.keys(o), o.x]",
  "var o = Object.fromEntries([['__proto__', {x:1}]]); [Object.keys(o), o.x]",
  "'__proto__' in {}",
  "({}).hasOwnProperty('__proto__')",
  "Object.hasOwn(Object.prototype, '__proto__')",
  "(function(){ 'use strict'; var o = Object.freeze({}); try { o.__proto__ = {}; return 'no throw' } catch (e) { return e.name + ': ' + e.message } })()",
  "(function(){ var o = Object.freeze({}); o.__proto__ = {}; return Object.getPrototypeOf(o) === Object.prototype })()",
  "Object.getOwnPropertyDescriptor(Function.prototype, '__proto__')",
  "Function.prototype.__proto__ === Object.prototype",
  "[].__proto__ === Array.prototype && ''.__proto__ === String.prototype && (1).__proto__ === Number.prototype",
  "T(function(){ return (void 0).__proto__ })",
  "T(function(){ return null.__proto__ })",
];
for (const c of lookupCases) {
  const idx = c.lastIndexOf("; ");
  if (idx >= 0 && /^var /.test(c)) T(`${c.slice(0, idx)}; R = S(${c.slice(idx + 2)})`);
  else T(`R = S(${c})`);
}
const miscCases = [
  "(1.5).toLocaleString()", "(1234567.891).toLocaleString()", "(1234567.891).toLocaleString('de-DE')", "(1234567.891).toLocaleString('en-US', {style:'currency', currency:'USD'})",
  "(1234567.891).toLocaleString('pt-BR')", "new Date(0).toLocaleString('en-US', {timeZone:'UTC'})", "new Date(0).toLocaleDateString('en-US', {timeZone:'UTC'})",
  "new Date(0).toLocaleTimeString('en-US', {timeZone:'UTC'})", "10n.toLocaleString()", "12345678901234567890n.toLocaleString('en-US')",
  "[1234.5, new Date(0), null, undefined, 'x'].toLocaleString('en-US', {timeZone:'UTC'})", "[].toLocaleString()", "[null].toLocaleString()", "[[1,[2]]].toLocaleString()",
  "'abc'.toLocaleUpperCase()", "'I'.toLocaleLowerCase('tr')", "'i'.toLocaleUpperCase('tr')", "'a'.localeCompare('b')", "'a'.localeCompare('B', 'en', {sensitivity:'base'})",
  "new Uint8Array([1,2]).toLocaleString()", "new Float64Array([1234.5]).toLocaleString('de-DE')", "({}).toLocaleString()", "({toString(){return 'ts'}}).toLocaleString()",
  "Object.prototype.toLocaleString.call(1)", "Object.prototype.toLocaleString.call(null)", "Object.prototype.toLocaleString.call(undefined)", "Object.prototype.toLocaleString.call(Symbol())",
  "Object.prototype.toLocaleString.length + Object.prototype.toLocaleString.name", "Number.prototype.toLocaleString.length", "Array.prototype.toLocaleString.length",
  "Date.prototype.toLocaleString.length", "BigInt.prototype.toLocaleString.length", "String.prototype.toLocaleUpperCase.length", "String.prototype.localeCompare.length",
  "TypedArray = Object.getPrototypeOf(Int8Array), TypedArray.prototype.toLocaleString.length",
  "Object.prototype.valueOf.call(1) instanceof Number", "typeof Object.prototype.valueOf.call('s')", "typeof Object.prototype.valueOf.call(Symbol())",
  "T(function(){ return Object.prototype.valueOf.call(null) })", "T(function(){ return Object.prototype.valueOf.call(undefined) })", "({}).valueOf() !== undefined",
  "var o = {}; o.valueOf() === o", "[1].valueOf().length", "new Date(5).valueOf()", "new Number(5).valueOf()", "new String('x').valueOf()", "Object(Symbol.iterator).valueOf() === Symbol.iterator",
  "Object(1n).valueOf()", "new Boolean(false).valueOf()", "T(function(){ return Number.prototype.valueOf.call('1') })", "T(function(){ return String.prototype.valueOf.call(1) })",
  "T(function(){ return Boolean.prototype.valueOf.call(1) })", "T(function(){ return Symbol.prototype.valueOf.call(1) })", "T(function(){ return BigInt.prototype.valueOf.call(1) })",
  "T(function(){ return Date.prototype.valueOf.call({}) })", "Object.prototype.valueOf.length + Object.prototype.valueOf.name",
  "Object.prototype.isPrototypeOf.call(Object.prototype, {})", "Object.prototype.isPrototypeOf.call(Object.prototype, Object.create(null))", "Object.prototype.isPrototypeOf.call(Object.prototype, 1)",
  "Object.prototype.isPrototypeOf.call(Array.prototype, [])", "Array.prototype.isPrototypeOf(Object.create(Array.prototype))", "Function.prototype.isPrototypeOf(Object)",
  "Function.prototype.isPrototypeOf(class{})", "Object.prototype.isPrototypeOf(Function.prototype)", "Object.prototype.isPrototypeOf(Object.prototype)",
  "T(function(){ return Object.prototype.isPrototypeOf.call(null, {}) })", "T(function(){ return Object.prototype.isPrototypeOf.call(undefined, 1) })",
  "Object.prototype.isPrototypeOf.call(null, 1)", "Object.prototype.isPrototypeOf.call(1, {})", "Object.prototype.isPrototypeOf.length + Object.prototype.isPrototypeOf.name",
  "Number.prototype.isPrototypeOf(1)", "Number.prototype.isPrototypeOf(Object(1))", "Object.getPrototypeOf(Int8Array).prototype.isPrototypeOf(new Int8Array(1))",
  "var p = new Proxy({}, {getPrototypeOf(){ return Array.prototype }}); Array.prototype.isPrototypeOf(p)",
  "Object.prototype.hasOwnProperty.call('ab', 1)", "Object.prototype.hasOwnProperty.length + Object.prototype.hasOwnProperty.name",
  "T(function(){ return Object.prototype.hasOwnProperty.call(null, 'a') })", "T(function(){ return Object.prototype.hasOwnProperty.call(undefined, {toString(){ throw new RangeError('first') }}) })",
  "T(function(){ return Object.prototype.hasOwnProperty.call(null, {toString(){ throw new RangeError('first') }}) })",
  "Object.prototype.constructor === Object", "Object.prototype.constructor.name", "Object.getPrototypeOf(Object.prototype)",
  "Object.getOwnPropertyDescriptor(Object.prototype, 'constructor').enumerable",
  "Object.prototype.toString.call(Object.prototype)", "Object.prototype.toString.call(Object.getPrototypeOf(Object.prototype))",
];
for (const c of miscCases) {
  if (/^TypedArray = /.test(c)) T(`var TypedArray = Object.getPrototypeOf(Int8Array); R = S(${c.replace(/^TypedArray = [^,]+, /, "")})`);
  else T(`R = S(${c})`);
}

// ---- 7. Function.prototype.
const fnCases = [
  "var f = function(){}; [D(f, 'caller'), D(f, 'arguments'), Object.getOwnPropertyNames(f)]",
  "var f = function(){'use strict'}; [D(f, 'caller'), D(f, 'arguments'), f.hasOwnProperty('caller')]",
  "var f = () => 1; [D(f, 'caller'), D(f, 'arguments')]",
  "var f = class {}; [D(f, 'caller'), D(f, 'arguments')]",
  "var f = async function(){}; [D(f, 'caller'), D(f, 'arguments')]",
  "var f = function*(){}; [D(f, 'caller'), D(f, 'arguments')]",
  "var f = function(){}.bind(); [D(f, 'caller'), D(f, 'arguments')]",
  "var f = ({m(){}}).m; [D(f, 'caller'), D(f, 'arguments')]",
  "var f = function(){}; [typeof f.caller, typeof f.arguments]",
  "var f = function(){'use strict'}; [T(function(){ return f.caller }), T(function(){ return f.arguments })]",
  "var f = () => 1; [T(function(){ return f.caller }), T(function(){ return f.arguments })]",
  "var f = class {}; [T(function(){ return f.caller }), T(function(){ return f.arguments })]",
  "var f = function(){ return g() }; function g(){ return g.caller === f }; f()",
  "function f(){ return g() }; function g(){ return g.caller === f }; f()",
  "function f(){ 'use strict'; return g() }; function g(){ return g.caller }; f()",
  "function f(){ return g() }; function g(){ 'use strict'; return T(function(){ return g.caller }) }; f()",
  "function f(a){ return g() }; function g(){ return g.caller.arguments[0] }; f(7)",
  "function f(a){ return f.arguments[0] }; f(7)",
  "function f(a){ return f.arguments === arguments }; f(7)",
  "function f(){ return f.arguments }; f.arguments + '|' + f()",
  "function f(){}; f.caller",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); [typeof d.get, d.get === d.set, d.get.name, d.get.length, d.enumerable, d.configurable]",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); [typeof d.get, d.get === d.set, d.get.name, d.get.length, d.enumerable, d.configurable]",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); var e = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); d.get === e.get",
  "T(function(){ return Function.prototype.caller })",
  "T(function(){ return Function.prototype.arguments })",
  "T(function(){ Function.prototype.caller = 1; return 'ok' })",
  "(function(){ 'use strict'; return T(function(){ Function.prototype.caller = 1; return 'ok' }) })()",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); T(function(){ return d.get.call({}) })",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); T(function(){ return d.get.call(function(){'use strict'}) })",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); T(function(){ return d.get.call(1) })",
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); T(function(){ return d.set.call(function(){}, 1) })",
  "[Function.prototype.length, Function.prototype.name === '', typeof Function.prototype, Function.prototype()]",
  "[T(function(){ return new Function.prototype }), Function.prototype.hasOwnProperty('prototype')]",
  "Reflect.ownKeys(Function.prototype).map(K).sort()",
  "[D(Function.prototype, 'length'), D(Function.prototype, 'name'), D(Function.prototype, 'constructor')]",
  "[D(Function.prototype, Symbol.hasInstance), Function.prototype[Symbol.hasInstance].name, Function.prototype[Symbol.hasInstance].length]",
  "Function.prototype[Symbol.hasInstance].call(Array, [])",
  "Function.prototype[Symbol.hasInstance].call(Array, {})",
  "Function.prototype[Symbol.hasInstance].call({}, [])",
  "Function.prototype[Symbol.hasInstance].call(1, [])",
  "Function.prototype[Symbol.hasInstance].call(function(){}.bind(), {})",
  "var B = function(){}.bind(); Function.prototype[Symbol.hasInstance].call(B, new (function(){}))",
  "function A(){}; var B = A.bind(); [new A instanceof B, new B instanceof A, new B instanceof B]",
  "function A(){}; A.prototype = 1; T(function(){ return {} instanceof A })",
  "function A(){}; A.prototype = null; T(function(){ return {} instanceof A })",
  "function A(){}; [Function.prototype[Symbol.hasInstance].call(A, 1), Function.prototype[Symbol.hasInstance].call(A, null)]",
  "T(function(){ return {} instanceof {} })",
  "T(function(){ return {} instanceof (()=>1) })",
  "T(function(){ return {} instanceof {[Symbol.hasInstance]: v => 'truthy'} })",
  "T(function(){ return {} instanceof {[Symbol.hasInstance]: 1} })",
  "T(function(){ return {} instanceof {[Symbol.hasInstance]: null} })",
  "T(function(){ return {} instanceof Math.max })",
  "T(function(){ return 1 instanceof Object })",
  "var C = class { static [Symbol.hasInstance](v){ return v === 1 } }; [1 instanceof C, 2 instanceof C]",
  "var C = function(){}; Object.defineProperty(C, Symbol.hasInstance, {value: () => true}); [1 instanceof C, D(C, Symbol.hasInstance)]",
  "var C = function(){}; C[Symbol.hasInstance] = () => true; [1 instanceof C]",
  "Function.prototype.bind.length + Function.prototype.bind.name",
  "var f = function(a,b,c){}.bind(null, 1); [f.length, f.name, D(f, 'length'), D(f, 'name')]",
  "var f = function(a,b,c){}.bind(null, 1, 2, 3, 4); [f.length, f.name]",
  "var f = function f(){}; Object.defineProperty(f, 'name', {value: 5}); [f.bind().name]",
  "var f = function f(){}; Object.defineProperty(f, 'length', {value: Infinity}); [f.bind().length]",
  "var f = function f(){}; Object.defineProperty(f, 'length', {value: -Infinity}); [f.bind().length]",
  "var f = function f(){}; Object.defineProperty(f, 'length', {value: 2.7}); [f.bind().length]",
  "var f = function f(){}; Object.defineProperty(f, 'length', {value: '3'}); [f.bind().length]",
  "var f = function f(){}; delete f.length; [f.bind().length]",
  "var f = function f(){}; delete f.name; [f.bind().name]",
  "var f = function(){}.bind().bind(); [f.name, f.length]",
  "var f = Symbol.prototype.toString.bind(Symbol()); [f.name, f.length]",
  "var f = (class A { static m(){} }).m.bind(); [f.name]",
  "var o = {get a(){ return 1 }}; var d = Object.getOwnPropertyDescriptor(o, 'a'); [d.get.name, d.get.bind().name]",
  "var o = {[Symbol('desc')](){}}; var f = o[Object.getOwnPropertySymbols(o)[0]]; [f.name]",
  "var o = {[Symbol()](){}}; var f = o[Object.getOwnPropertySymbols(o)[0]]; [f.name]",
  "var f = function(){}.bind(); [Object.getPrototypeOf(f) === Function.prototype, f.hasOwnProperty('prototype'), T(function(){ return new f }) instanceof Object]",
  "var A = class { constructor(){ this.n = new.target === A } }; var B = A.bind(); [new B().n]",
  "Function.prototype.call.length + Function.prototype.call.name + Function.prototype.apply.length + Function.prototype.apply.name",
  "T(function(){ return Function.prototype.call.call(1) })",
  "T(function(){ return Function.prototype.apply.call(1) })",
  "T(function(){ return Function.prototype.bind.call(1) })",
  "T(function(){ return Function.prototype.toString.call(1) })",
  "T(function(){ return Function.prototype.toString.call({}) })",
  "Function.prototype.toString.call(Math.max)",
  "Function.prototype.toString.call(class A { m(){} })",
  "Function.prototype.toString.call(function  f ( a ){ /* c */ })",
  "Function.prototype.toString.call(Symbol)",
  "Function.prototype.toString.call(function(){}.bind())",
  "Function.prototype.toString.call(new Proxy(function(){}, {}))",
  "T(function(){ return Function.prototype.toString.call(new Proxy({}, {})) })",
  "Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Map.prototype, 'size').get)",
  "Function.prototype.toString.call(Object.getOwnPropertyDescriptor({get x(){ return 1 }}, 'x').get)",
  "Function.prototype.toString.call(async () => 1)",
  "Function.prototype.toString.call(({ async *m(){} }).m)",
  "Function.prototype.toString.call(({ 'a b'(){} })['a b'])",
  "Function.prototype.toString.call(({ [1+1](){} })[2])",
  "Function.prototype.toString.length + Function.prototype.toString.name",
  "new Function('a', 'b', 'return a + b').toString()",
  "new Function('a, b', 'c', 'return 1').length",
  "new Function().name + new Function().toString()",
  "Function('return this')() === globalThis",
  "Function('\"use strict\"; return this')()",
  "Function('a', '/*', '*/){')",
  "Function('a = 1', 'return a')()",
  "T(function(){ return Function('}, function(){') })",
  "T(function(){ return Function('a', 'a', '\"use strict\"') })",
  "Object.getPrototypeOf(Function('')) === Function.prototype",
  "Function.length + Function.name + Function.prototype.constructor.name",
  "Object.getPrototypeOf(function*(){}).constructor('yield 1')().next().value",
  "T(function(){ return Object.getPrototypeOf(async function(){}).constructor('await 1')() instanceof Promise })",
  "Object.getPrototypeOf(function*(){}).constructor('').toString()",
  "Object.getPrototypeOf(async function*(){}).constructor('').name",
  "Object.getPrototypeOf(async function(){}).constructor.length",
  "Object.getPrototypeOf(function*(){}).constructor.length",
  "Object.getPrototypeOf(function*(){}).constructor.name",
  "Object.getPrototypeOf(function*(){}).constructor.prototype === Object.getPrototypeOf(function*(){})",
  "Reflect.ownKeys(Object.getPrototypeOf(function*(){}).constructor).map(K)",
  "Reflect.ownKeys(Object.getPrototypeOf(function*(){})).map(K)",
  "Reflect.ownKeys(Object.getPrototypeOf(async function(){})).map(K)",
  "Reflect.ownKeys(Object.getPrototypeOf(async function*(){})).map(K)",
  "Reflect.ownKeys(Object.getPrototypeOf(function*(){}).prototype).map(K)",
  "Reflect.ownKeys(Object.getPrototypeOf(async function*(){}).prototype).map(K)",
  "[D(Function, 'prototype'), D(Function, 'length'), D(Function, 'name')]",
  "[D(function(){}, 'prototype'), D(class{}, 'prototype'), D(function*(){}, 'prototype'), D(async function*(){}, 'prototype')]",
  "[D(function f(){}, 'name'), D(function f(){}, 'length'), D(class{}, 'name'), D(class A{ static name(){} }, 'name')]",
  "[(class{}).name, (class{ static name = 'x' }).name, (class A{}).name, (()=>1).name, (function(){}).name, (async()=>1).name]",
  "var a = function(){}, b = () => 1, c = class {}, d = {e: function(){}, f: () => 1, g: class {}, ['h' + 1]: function(){}}; [a.name, b.name, c.name, d.e.name, d.f.name, d.g.name, d.h1.name]",
  "var a; a = function(){}; var b; b = class {}; var [c = function(){}] = []; var {d = () => 1} = {}; [a.name, b.name, c.name, d.name]",
  "var o = {}; o.x = function(){}; o['y'] = () => 1; [o.x.name, o.y.name]",
  "var o = {get a(){}, set a(v){}, get [Symbol('s')](){}, get 1(){}}; [Object.getOwnPropertyDescriptor(o,'a').get.name, Object.getOwnPropertyDescriptor(o,'a').set.name, Object.getOwnPropertyDescriptor(o,1).get.name]",
  "var o = {a(){}, async b(){}, *c(){}, async *d(){}}; [o.a.name, o.b.name, o.c.name, o.d.name, D(o.a, 'prototype'), D(o.c, 'prototype'), D(o.d, 'prototype')]",
  "var f = (function(){ return function(){} })(); [f.name === '', D(f, 'name')]",
  "var f = (0, function(){}); [f.name === '']",
  "var f = (function g(){}); var h = f; [h.name]",
  "var x = function(){} || 1, y = null ?? function(){}, z = (1, function(){}); [x.name, y.name, z.name]",
  "var x; x ||= function(){}; var y; y ??= () => 1; var z = 0; z ||= class {}; [x.name, y.name, z.name]",
  "var {a: f = function(){}} = {}; var [g = class{}] = []; [f.name, g.name]",
  "function f(a = function(){}, b = () => 1){ return [a.name, b.name] }; f()",
  "var o = {a: (function(){}), b: (() => 1), c: (class {})}; [o.a.name, o.b.name, o.c.name]",
  "var o = {a: function(){}.bind()}; [o.a.name]",
  "var s = Symbol('d'); var o = {[s]: function(){}}; [o[s].name]",
];
for (const c of fnCases) {
  const idx = c.lastIndexOf("; ");
  if (idx >= 0 && /^(var|function) /.test(c)) T(`${c.slice(0, idx)}; R = S(${c.slice(idx + 2)})`);
  else T(`R = S(${c})`);
}

// ---- 8. Um programa por objeto built-in: nome, length, descritores de name/length, construtibilidade, forma nativa.
const OBJ_PRELUDE = `
function IC(f){try{Reflect.construct(function(){},[],f);return"c"}catch(e){return"n"}}
function FI(f){return"("+JSON.stringify(f.name)+","+f.length+","+D(f,"name")+D(f,"length")+","+IC(f)+","+(Object.hasOwn(f,"prototype")?"P":"-")+","+(typeof Object.getPrototypeOf(f)==="function"&&Object.getPrototypeOf(f)===Function.prototype?"F":"o")+","+String(f).replace(/\\s+/g," ")+")"}
function LIST(o){if(o===undefined)return"absent";var out=[];var keys=Reflect.ownKeys(o).map(function(k){return[K(k),k]}).sort(function(a,b){return a[0]<b[0]?-1:a[0]>b[0]?1:0});for(var i=0;i<keys.length;i++){var k=keys[i][1];if(k==="appendStackTrace"||k==="prepareStackTrace")continue;var d=Object.getOwnPropertyDescriptor(o,k);var s=keys[i][0]+"="+D(o,k)+":";if("value"in d){s+=typeof d.value==="function"?FI(d.value):typeof d.value}else{s+="get"+(d.get?FI(d.get):"-")+"set"+(d.set?FI(d.set):"-")}out.push(s)}return out.join("\\n")}
`;
const ctors = [
  "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Date", "RegExp", "Error", "EvalError", "RangeError", "ReferenceError",
  "SyntaxError", "TypeError", "URIError", "AggregateError", "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "Promise", "Proxy",
  "ArrayBuffer", "SharedArrayBuffer", "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array",
  "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "Iterator", "Intl.Collator", "Intl.DateTimeFormat", "Intl.NumberFormat",
  "Intl.PluralRules", "Intl.RelativeTimeFormat", "Intl.ListFormat", "Intl.Locale", "Intl.DisplayNames", "Intl.Segmenter",
  "WebAssembly.Module", "WebAssembly.Instance", "WebAssembly.Memory", "WebAssembly.Table", "WebAssembly.Global", "WebAssembly.Tag", "WebAssembly.Exception",
  "WebAssembly.CompileError", "WebAssembly.LinkError", "WebAssembly.RuntimeError",
];
const safeObj = e => `(function(){ try { return ${e} } catch (x) { return undefined } })()`;
const objectExprs = [];
for (const c of ctors) {
  objectExprs.push([c, c]);
  objectExprs.push([c + ".prototype", `(${safeObj(c)} || {}).prototype`.replace(/^\(undefined \|\| \{\}\)\.prototype$/, "undefined")]);
}
objectExprs.push(
  ["TypedArray", "Object.getPrototypeOf(Int8Array)"],
  ["TypedArray.prototype", "Object.getPrototypeOf(Int8Array).prototype"],
  ["Math", "Math"], ["JSON", "JSON"], ["Reflect", "Reflect"], ["Atomics", "Atomics"], ["Intl", "Intl"], ["WebAssembly", "WebAssembly"],
  ["GeneratorFunction", "Object.getPrototypeOf(function*(){}).constructor"],
  ["GeneratorFunction.prototype", "Object.getPrototypeOf(function*(){})"],
  ["Generator.prototype", "Object.getPrototypeOf(function*(){}).prototype"],
  ["AsyncFunction", "Object.getPrototypeOf(async function(){}).constructor"],
  ["AsyncFunction.prototype", "Object.getPrototypeOf(async function(){})"],
  ["AsyncGeneratorFunction", "Object.getPrototypeOf(async function*(){}).constructor"],
  ["AsyncGeneratorFunction.prototype", "Object.getPrototypeOf(async function*(){})"],
  ["AsyncGenerator.prototype", "Object.getPrototypeOf(async function*(){}).prototype"],
  ["AsyncIterator.prototype", "Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype)"],
  ["ArrayIterator.prototype", "Object.getPrototypeOf([][Symbol.iterator]())"],
  ["MapIterator.prototype", "Object.getPrototypeOf(new Map()[Symbol.iterator]())"],
  ["SetIterator.prototype", "Object.getPrototypeOf(new Set()[Symbol.iterator]())"],
  ["StringIterator.prototype", "Object.getPrototypeOf(''[Symbol.iterator]())"],
  ["RegExpStringIterator.prototype", "Object.getPrototypeOf(/a/[Symbol.matchAll](''))"],
  ["IteratorHelper.prototype", "Object.getPrototypeOf([].values().map(x => x))"],
  ["WrapForValidIterator.prototype", "Object.getPrototypeOf(Iterator.from({next(){}}))"],
  ["Segments.prototype", "Object.getPrototypeOf(new Intl.Segmenter().segment('a'))"],
  ["SegmentIterator.prototype", "Object.getPrototypeOf(new Intl.Segmenter().segment('a')[Symbol.iterator]())"],
  ["Array.prototype[@@unscopables]", "Array.prototype[Symbol.unscopables]"],
  ["arguments.sloppy", "(function(){ return arguments })(1)"],
  ["arguments.strict", "(function(){ 'use strict'; return arguments })(1)"],
  ["ThrowTypeError", "Object.getOwnPropertyDescriptor((function(){ 'use strict'; return arguments })(), 'callee').get"],
);
for (const [label, expr] of objectExprs) {
  add(`${OBJ_PRELUDE}\n// ${label}\ntry { var o = (function(){ try { return ${expr} } catch (e) { return undefined } })(); R = LIST(o) } catch (e) { R = e.name + ': ' + e.message }`);
}
// Funções globais: nome e length.
add(`${OBJ_PRELUDE}\n// globais\ntry { var names = ["parseInt","parseFloat","isNaN","isFinite","decodeURI","decodeURIComponent","encodeURI","encodeURIComponent","escape","unescape","eval"]; R = names.map(function(n){ return n + "=" + (typeof globalThis[n] === "function" ? D(globalThis, n) + FI(globalThis[n]) : "absent") }).join("\\n") } catch (e) { R = e.name + ': ' + e.message }`);
add(`${OBJ_PRELUDE}\n// globais, valores\ntry { R = ["NaN","Infinity","undefined","globalThis"].map(function(n){ return n + "=" + D(globalThis, n) }).join(",") } catch (e) { R = e.name + ': ' + e.message }`);
// Getters de species e outros accessors estáticos.
for (const c of ["Array", "Map", "Set", "Promise", "ArrayBuffer", "SharedArrayBuffer", "RegExp", "Object.getPrototypeOf(Int8Array)"]) {
  T(`var C = ${c}; var d = Object.getOwnPropertyDescriptor(C, Symbol.species); R = S([typeof d.get, d.set, d.get.name, d.get.length, d.enumerable, d.configurable, C[Symbol.species] === C, d.get.call(1)])`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "objstat-golden-"));
// vm.runInThisContext roda como ProgramExecutable do JSC puro, sem o transpilador do bun.
const source_file = path.join(dir, "source.js");
const file = path.join(dir, "case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
