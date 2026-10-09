// Gera tests/golden/proxy_invariants_bun.tsv: grade de invariantes de Proxy, medida no bun 1.4.2.
// Cobre as 13 traps com retorno válido e cada violação de invariante contra alvos com propriedade não configurável,
// não gravável, accessor sem getter ou sem setter e alvo não extensível; a ordem de chamadas das traps por operação de
// alto nível (Object.keys, for-in, JSON.stringify, spread, assign, Array.isArray, instanceof, in, with, delete, class
// extends, Reflect.*, toString, typeof, concat/slice/splice, freeze/seal/isFrozen, fromEntries); a busca da trap no
// handler (valores não chamáveis, handler que é Proxy); revogação no meio da operação; Proxy como protótipo e Proxy de Proxy.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Programas cuja expressão já aparece nos goldens proxy_* existentes são descartados.
// O prelúdio comum sai em tests/golden/proxy_invariants.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-proxy-invariants-golden.js > tests/golden/proxy_invariants_bun.tsv
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
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function Q(v,p){if(v===p&&p!==undefined)return "p";if(typeof v==="function")return "fn";if(v!==null&&typeof v==="object")return Array.isArray(v)?S(v):"obj";return S(v)}\n' +
  'var TR=["getPrototypeOf","setPrototypeOf","isExtensible","preventExtensions","getOwnPropertyDescriptor","defineProperty","has","get","set","deleteProperty","ownKeys","apply","construct"];\n' +
  'function A1(a){var x=a[1];return typeof x==="symbol"?String(x):typeof x==="string"?x:x!==null&&typeof x==="object"?"obj":String(x)}\n' +
  'function mk(t,o,l,tag){var h={};TR.forEach(function(n){h[n]=function(){l.push((tag||"")+n+((n==="apply"||n==="construct"||arguments.length<2)?"":" "+A1(arguments)));return Reflect[n].apply(null,arguments)}});for(var k in o)h[k]=o[k];return new Proxy(t,h)}\n' +
  'function E(l,f,p){var r;try{r=Q(f(),p)}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return l.join()+" | "+r}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. Grade de invariantes: cada trap com retorno fixo, contra alvos diferentes.
const TG = [
  "{}", "{a:1}", "Object.freeze({a:1})", "Object.seal({a:1})", "Object.preventExtensions({a:1})", "Object.preventExtensions({})",
  "Object.defineProperty({},'a',{value:1})", "Object.defineProperty({},'a',{value:1,writable:true})",
  "Object.defineProperty({},'a',{set(v){}})", "Object.defineProperty({},'a',{get(){return 1}})",
  "Object.defineProperty({},'a',{value:1,configurable:true})", "[]",
];
const grid = (trap, rets, ops, targets = TG) => {
  for (const tg of targets) for (const ret of rets) for (const op of ops) {
    add(`T(()=>{var p=new Proxy(${tg},{${trap}(){return ${ret}}});return Q(${op},p)})`);
  }
};
grid("getPrototypeOf", ["null", "undefined", "{}", "Object.prototype", "1", "'x'", "Array.prototype", "Function.prototype", "Symbol()"],
  ["Object.getPrototypeOf(p)", "Reflect.getPrototypeOf(p)", "p.__proto__"]);
grid("setPrototypeOf", ["true", "false", "undefined", "0", "1", "'x'", "null"],
  ["Reflect.setPrototypeOf(p,Object.prototype)", "Reflect.setPrototypeOf(p,null)", "Reflect.setPrototypeOf(p,Array.prototype)", "Object.setPrototypeOf(p,null)===p", "(p.__proto__=null,'done')"]);
grid("isExtensible", ["true", "false", "undefined", "0", "1", "'x'", "null", "{}"], ["Object.isExtensible(p)", "Reflect.isExtensible(p)", "Object.isSealed(p)"]);
grid("preventExtensions", ["true", "false", "undefined", "0", "1", "null", "{}"],
  ["Reflect.preventExtensions(p)", "Object.preventExtensions(p)===p", "Object.freeze(p)===p", "Object.seal(p)===p"]);
grid("getOwnPropertyDescriptor", [
  "undefined", "null", "1", "'x'", "{}", "{value:1}", "{value:2}", "{value:1,configurable:true}", "{value:1,configurable:false}",
  "{value:1,writable:true,configurable:false}", "{value:1,writable:false,configurable:false}", "{get(){},configurable:false}",
  "{get:undefined,configurable:false}", "{value:1,configurable:true,enumerable:true}", "{configurable:false}", "{get:undefined,set(v){},configurable:false}",
], ["D(p,'a')", "D(p,'b')", "Object.keys(p).join()"]);
grid("defineProperty", ["true", "false", "undefined", "0", "1", "''", "null", "{}"], [
  "Reflect.defineProperty(p,'a',{value:1})", "Reflect.defineProperty(p,'a',{value:2,configurable:false})", "Reflect.defineProperty(p,'b',{value:1,configurable:false})",
  "Reflect.defineProperty(p,'a',{get(){},configurable:true})", "Object.defineProperty(p,'a',{value:1})===p",
  "Reflect.defineProperty(p,'b',{value:1,configurable:true,writable:true,enumerable:true})", "Reflect.defineProperty(p,'a',{writable:false,configurable:false})",
]);
grid("has", ["true", "false", "undefined", "0", "1", "'x'", "null"], [
  "'a' in p", "'b' in p", "Reflect.has(p,'a')", "Reflect.has(p,'b')", "Function('p','with(p){return typeof a}')(p)",
]);
grid("get", ["undefined", "1", "2", "'x'", "null", "()=>1", "{}"], ["p.a", "p.b", "Reflect.get(p,'a')", "p[Symbol.iterator]", "Reflect.get(p,'a',{})"]);
grid("set", ["true", "false", "undefined", "0", "1", "'x'", "null"], [
  "(p.a=5,'ok')", "Reflect.set(p,'a',5)", "Reflect.set(p,'a',1)", "Reflect.set(p,'b',5)", "(p.b=5,'ok')", "(p.a=1,'ok')",
]);
grid("deleteProperty", ["true", "false", "undefined", "0", "1", "null", "'x'"], ["delete p.a", "delete p.b", "Reflect.deleteProperty(p,'a')", "Reflect.deleteProperty(p,'b')"]);
grid("ownKeys", ["[]", "['a']", "['a','a']", "['b']", "['a','b']", "[1]", "[Symbol.iterator]", "undefined", "null", "1", "'ab'", "{length:1,0:'a'}", "['b','a']", "[{}]"],
  ["Reflect.ownKeys(p).join()", "Object.keys(p).join()", "Object.getOwnPropertyNames(p).join()"]);
const FT = ["function(){return 1}", "()=>1", "Object.freeze(function(){})", "class{}", "Object.preventExtensions(function(){})", "function(){}.bind()"];
grid("apply", ["undefined", "1", "'x'", "null", "{}", "()=>2", "Symbol()", "NaN"], ["p()", "p.call(1)", "Reflect.apply(p,undefined,[1,2])", "p.apply(null,[3])", "typeof p"], FT);
grid("construct", ["{}", "1", "undefined", "null", "'x'", "[]", "function(){}", "Symbol()"], ["new p()", "Reflect.construct(p,[])", "Reflect.construct(p,[],Array)", "typeof p"],
  ["function(){}", "class{}", "()=>1", "function(){}.bind()", "Object", "function*(){}", "async function(){}", "Math.max"]);
// Trap que lança no meio da operação e trap com efeito colateral no alvo.
for (const trap of TR13()) {
  add(`T(()=>{var p=new Proxy({a:1},{${trap}(){throw new RangeError('t-${trap}')}});return Q(${opFor(trap)},p)})`);
  add(`T(()=>{var t={a:1};var p=new Proxy(t,{${trap}(){Object.defineProperty(t,'a',{value:9,configurable:false,writable:false});return Reflect[${JSON.stringify(trap)}].apply(null,arguments)}});return Q(${opFor(trap)},p)+'|'+D(t,'a')})`);
  add(`T(()=>{var t={a:1};var p=new Proxy(t,{${trap}(){Object.preventExtensions(t);return ${["getOwnPropertyDescriptor"].includes(trap) ? "undefined" : "true"}}});return Q(${opFor(trap)},p)+'|'+Object.isExtensible(t)})`);
}
function TR13() {
  return ["getPrototypeOf", "setPrototypeOf", "isExtensible", "preventExtensions", "getOwnPropertyDescriptor", "defineProperty", "has", "get", "set", "deleteProperty", "ownKeys", "apply", "construct"];
}
function opFor(trap) {
  return {
    getPrototypeOf: "Object.getPrototypeOf(p)", setPrototypeOf: "Reflect.setPrototypeOf(p,null)", isExtensible: "Object.isExtensible(p)",
    preventExtensions: "Reflect.preventExtensions(p)", getOwnPropertyDescriptor: "D(p,'a')", defineProperty: "Reflect.defineProperty(p,'a',{value:3})",
    has: "'a' in p", get: "p.a", set: "Reflect.set(p,'a',2)", deleteProperty: "Reflect.deleteProperty(p,'a')", ownKeys: "Reflect.ownKeys(p).join()",
    apply: "Reflect.apply(p,1,[])", construct: "Reflect.construct(p,[])",
  }[trap];
}

// ---- 2. Ordem de chamadas da trap por operação de alto nível, com o log completo.
const OT = ["{a:1,b:2}", "[1,2]", "function(){}", "{a:1,[Symbol.for('s')]:2}", "Object.create({x:1},{y:{value:1,enumerable:true}})", "Object.freeze({a:1})"];
const OPS = [
  "Object.keys(p)", "Object.values(p)", "Object.entries(p)", "Object.getOwnPropertyNames(p)", "Object.getOwnPropertySymbols(p)",
  "(()=>{var r=[];for(var k in p)r.push(k);return r})()", "JSON.stringify(p)", "({...p})", "Object.assign({},p)", "Object.assign(p,{c:3})",
  "Array.isArray(p)", "p instanceof Object", "p instanceof Array", "'a' in p", "Function('p','with(p){return typeof a}')(p)", "delete p.a",
  "(()=>{class X extends p{};return typeof X})()", "(()=>{class X extends p{};return typeof new X})()", "Function.prototype.toString.call(p)", "typeof p",
  "[].concat(p)", "[1].concat(p,[2])", "Array.prototype.slice.call(p)", "Array.prototype.splice.call(p,0,1)", "Object.freeze(p)===p", "Object.seal(p)===p",
  "Object.isFrozen(p)", "Object.isSealed(p)", "Object.isExtensible(p)", "Object.preventExtensions(p)===p", "Object.fromEntries(p)", "p.length",
  "Object.getOwnPropertyDescriptors(p)", "Object.prototype.toString.call(p)", "Object.prototype.hasOwnProperty.call(p,'a')",
  "Object.prototype.propertyIsEnumerable.call(p,'a')", "Array.from(p)", "[...p]", "p.toString()", "String(p)", "p+''", "`${p}`", "Object.setPrototypeOf(p,null)===p",
  "Object.defineProperty(p,'z',{value:1})===p", "Object.defineProperties(p,{z:{value:1}})===p", "Object.create(p)!==p", "Object.hasOwn(p,'a')",
  "Array.prototype.join.call(p)", "Array.prototype.reverse.call(p)===p", "Array.prototype.sort.call(p)===p", "Array.prototype.push.call(p,1)", "Array.prototype.pop.call(p)",
  "Array.prototype.shift.call(p)", "Array.prototype.unshift.call(p,0)", "Array.prototype.includes.call(p,1)", "Array.prototype.indexOf.call(p,1)",
  "Array.prototype.map.call(p,x=>x)", "Array.prototype.forEach.call(p,x=>x)", "Array.prototype.flat.call(p)", "Array.prototype.fill.call(p,7)===p",
  "p.hasOwnProperty('a')", "Object.getPrototypeOf(p)", "Object.prototype.isPrototypeOf.call(p,{})", "({}).isPrototypeOf(p)", "(()=>{p.a=7;return p.a})()",
  "(()=>{p.q=7;return Object.keys(p)})()", "(()=>{p[0]=7;return p[0]})()", "Object.groupBy([1],()=>p)", "new Map([[p,1]]).get(p)", "new Set([p]).has(p)",
  "new WeakMap([[p,1]]).has(p)", "Object.is(p,p)", "p===p", "!!p", "isNaN(p)", "Number(p)", "p==1", "p<1", "Symbol.keyFor(Symbol.for('x'))",
  "(()=>{var {a}=p;return a})()", "(()=>{var {...r}=p;return r})()", "(()=>{var [x]=p;return x})()", "(()=>{return Reflect.ownKeys(p)})()",
  "Reflect.getPrototypeOf(p)", "Reflect.setPrototypeOf(p,null)", "Reflect.isExtensible(p)", "Reflect.preventExtensions(p)", "Reflect.getOwnPropertyDescriptor(p,'a')",
  "Reflect.defineProperty(p,'z',{value:1})", "Reflect.has(p,'a')", "Reflect.get(p,'a')", "Reflect.set(p,'a',3)", "Reflect.deleteProperty(p,'a')",
  "Reflect.ownKeys(p)", "Reflect.apply(p,null,[1])", "Reflect.construct(p,[1])", "p()", "new p(1)", "p.call(null)", "p.bind(null)()",
  "Object.entries(Object.getOwnPropertyDescriptors(p)).length", "structuredCloneLike(p)",
].filter(o => !o.startsWith("structuredCloneLike"));
for (const tg of OT) for (const op of OPS) add(`T(()=>{var l=[];var p=mk(${tg},{},l);return E(l,()=>${op},p)})`);

// ---- 3. Busca da trap no handler: valores não chamáveis, handler que é Proxy, getter que lança.
const HOPS = {
  getPrototypeOf: "Object.getPrototypeOf(p)", setPrototypeOf: "Reflect.setPrototypeOf(p,null)", isExtensible: "Object.isExtensible(p)",
  preventExtensions: "Reflect.preventExtensions(p)", getOwnPropertyDescriptor: "Object.getOwnPropertyDescriptor(p,'a')", defineProperty: "Reflect.defineProperty(p,'a',{value:3})",
  has: "'a' in p", get: "p.a", set: "Reflect.set(p,'a',2)", deleteProperty: "Reflect.deleteProperty(p,'a')", ownKeys: "Reflect.ownKeys(p)",
  apply: "p()", construct: "new p()",
};
for (const trap of TR13()) {
  const tg = trap === "apply" || trap === "construct" ? "function(){return 1}" : "{a:1}";
  for (const v of ["1", "'x'", "{}", "null", "undefined", "true", "[]", "Symbol()", "class{}"]) {
    add(`T(()=>{var h={};h[${JSON.stringify(trap)}]=${v};var p=new Proxy(${tg},h);return Q(${HOPS[trap]},p)})`);
  }
  add(`T(()=>{var l=[];var p=new Proxy(${tg},new Proxy({},{get(t,k){l.push(String(k));return undefined}}));return E(l,()=>${HOPS[trap]},p)})`);
  add(`T(()=>{var l=[];var p=new Proxy(${tg},new Proxy({},{get(t,k){l.push(String(k));return Reflect[${JSON.stringify(trap)}]}}));return E(l,()=>${HOPS[trap]},p)})`);
  add(`T(()=>{var p=new Proxy(${tg},{get ${trap}(){throw new SyntaxError('g')}});return Q(${HOPS[trap]},p)})`);
  add(`T(()=>{var l=[];var h={};Object.defineProperty(h,${JSON.stringify(trap)},{get(){l.push('get');return undefined}});var p=new Proxy(${tg},h);return E(l,()=>${HOPS[trap]},p)})`);
  add(`T(()=>{var h=Object.create({${trap}(){return ${trap === "has" || trap === "set" || trap === "deleteProperty" || trap === "isExtensible" || trap === "preventExtensions" || trap === "defineProperty" || trap === "setPrototypeOf" ? "false" : "undefined"}}});var p=new Proxy(${tg},h);return Q(${HOPS[trap]},p)})`);
  add(`T(()=>{var h={};var p=new Proxy(${tg},h);h[${JSON.stringify(trap)}]=function(){return Reflect[${JSON.stringify(trap)}].apply(null,arguments)};return Q(${HOPS[trap]},p)})`);
  add(`T(()=>{var p=new Proxy(${tg},{${trap}:Reflect.${trap}.bind(Reflect)});return Q(${HOPS[trap]},p)})`);
}

// ---- 4. Revogação no meio da operação e uso de Proxy revogado.
for (const trap of TR13()) {
  const tg = trap === "apply" || trap === "construct" ? "function(){return 1}" : "{a:1,b:2}";
  const rops = {
    getPrototypeOf: ["Object.getPrototypeOf(p)", "p instanceof Object", "Object.prototype.isPrototypeOf.call(Object.prototype,p)", "Object.isFrozen(p)"],
    setPrototypeOf: ["Reflect.setPrototypeOf(p,null)", "Object.setPrototypeOf(p,null)===p"],
    isExtensible: ["Object.isExtensible(p)", "Object.isFrozen(p)", "Object.isSealed(p)"],
    preventExtensions: ["Reflect.preventExtensions(p)", "Object.freeze(p)===p", "Object.seal(p)===p"],
    getOwnPropertyDescriptor: ["Object.getOwnPropertyDescriptor(p,'a')", "Object.keys(p)", "Object.entries(p)", "({...p})", "Object.assign({},p)", "Object.getOwnPropertyDescriptors(p)", "JSON.stringify(p)"],
    defineProperty: ["Reflect.defineProperty(p,'a',{value:3})", "Object.freeze(p)===p", "Object.defineProperties(p,{a:{value:1},b:{value:2}})===p"],
    has: ["'a' in p", "Reflect.has(p,'a')", "Function('p','with(p){return typeof a}')(p)"],
    get: ["p.a", "JSON.stringify(p)", "Object.entries(p)", "Object.values(p)", "Array.prototype.join.call(p)"],
    set: ["Reflect.set(p,'a',2)", "(p.a=2,'ok')", "Object.assign(p,{a:2})===p"],
    deleteProperty: ["Reflect.deleteProperty(p,'a')", "delete p.a"],
    ownKeys: ["Reflect.ownKeys(p)", "Object.keys(p)", "Object.entries(p)", "for_in", "Object.freeze(p)===p", "Object.getOwnPropertyNames(p)", "JSON.stringify(p)", "Object.isFrozen(p)"],
    apply: ["p()", "Reflect.apply(p,null,[])", "p.call(null)"],
    construct: ["new p()", "Reflect.construct(p,[])"],
  }[trap];
  for (const op of rops) {
    const body = op === "for_in" ? "(()=>{var r=[];for(var k in p)r.push(k);return r})()" : op;
    add(`T(()=>{var l=[];var r=Proxy.revocable(${tg},{});var p=r.proxy;var q=mk(${tg},{${trap}(){l.push('revoke');r.revoke();return Reflect[${JSON.stringify(trap)}].apply(null,arguments)}},l);p=q;var rv=Proxy.revocable(q,{});return E(l,()=>${body},p)})`);
    add(`T(()=>{var l=[];var rv={};var h={};var tg=${tg};h[${JSON.stringify(trap)}]=function(){l.push('revoke');rv.r.revoke();return Reflect[${JSON.stringify(trap)}].apply(null,arguments)};rv=Proxy.revocable(tg,h);rv.r=rv;var p=rv.proxy;return E(l,()=>${body},p)})`);
  }
}
const REV = [
  "Object.getPrototypeOf(p)", "Reflect.getPrototypeOf(p)", "Object.setPrototypeOf(p,null)", "Object.isExtensible(p)", "Object.preventExtensions(p)", "Object.getOwnPropertyDescriptor(p,'a')",
  "Object.defineProperty(p,'a',{})", "'a' in p", "p.a", "p.a=1", "delete p.a", "Object.keys(p)", "Reflect.ownKeys(p)", "typeof p", "Array.isArray(p)", "Object.prototype.toString.call(p)",
  "JSON.stringify(p)", "p instanceof Object", "Function.prototype.toString.call(p)", "String(p)", "[].concat(p).length", "Object.is(p,p)", "new Proxy(p,{})===p", "Object.assign({},p)",
  "({...p})", "[...p]", "Object.freeze(p)", "Object.isFrozen(p)", "Object.entries(p)", "Object.create(p)!==p", "Reflect.has(p,'a')", "Reflect.get(p,'a')", "Reflect.set(p,'a',1)",
  "Reflect.getOwnPropertyDescriptor(p,'a')", "Reflect.defineProperty(p,'a',{})", "Reflect.deleteProperty(p,'a')", "Reflect.isExtensible(p)", "Reflect.preventExtensions(p)",
  "Reflect.setPrototypeOf(p,null)", "p()", "new p()", "Reflect.apply(p,null,[])", "Reflect.construct(p,[])", "Symbol.keyFor(Symbol.for('a'))", "JSON.stringify([p])", "Object.prototype.hasOwnProperty.call(p,'a')",
  "typeof p.call", "Object.hasOwn(p,'a')", "Array.from(p).length", "Array.prototype.slice.call(p)", "class X extends p{}", "new Map([[p,1]]).size", "p.constructor",
];
for (const tg of ["{a:1}", "[1]", "function(){}", "class{}", "new Proxy({a:1},{})", "null_"]) {
  if (tg === "null_") continue;
  for (const op of REV) {
    add(`T(()=>{var r=Proxy.revocable(${tg},{});r.revoke();var p=r.proxy;return Q(${op},p)})`);
  }
}
add(
  "T(()=>{var r=Proxy.revocable({},{});r.revoke();r.revoke();return typeof r.proxy})", "T(()=>{var r=Proxy.revocable({},{});r.revoke();return r.revoke()})",
  "T(()=>{var r=Proxy.revocable({},{});r.revoke();return Object.keys(r).join()+r.revoke.length+r.revoke.name+Object.getOwnPropertyNames(r.revoke).join()})",
  "T(()=>{var r=Proxy.revocable({},{});r.revoke();return new Proxy(r.proxy,{})!==null})", "T(()=>{var r=Proxy.revocable({},{});r.revoke();return new Proxy({},r.proxy)!==null})",
  "T(()=>{var r=Proxy.revocable({},{});r.revoke();var p=new Proxy({},r.proxy);return Object.keys(p)})", "T(()=>{var r=Proxy.revocable(function(){},{});r.revoke();return typeof r.proxy})",
  "T(()=>{var r=Proxy.revocable(function(){},{});r.revoke();return r.proxy()})", "T(()=>{var r=Proxy.revocable(class{},{});r.revoke();return new r.proxy()})",
  "T(()=>{var r=Proxy.revocable([],{});r.revoke();return Array.isArray(r.proxy)})", "T(()=>{var r=Proxy.revocable({},{});var s=Proxy.revocable(r.proxy,{});r.revoke();return Object.keys(s.proxy)})",
  "T(()=>{var r=Proxy.revocable({},{});var s=Proxy.revocable(r.proxy,{});r.revoke();return typeof s.proxy})", "T(()=>{var r=Proxy.revocable({},{});var s=Proxy.revocable(r.proxy,{});r.revoke();return Array.isArray(s.proxy)})",
  "T(()=>{var r=Proxy.revocable([],{});var s=Proxy.revocable(r.proxy,{});r.revoke();return Array.isArray(s.proxy)})", "T(()=>Proxy.revocable.length+Proxy.revocable.name+Proxy.length+typeof Proxy.prototype)",
  "T(()=>{var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();return o.a})", "T(()=>{var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();o.a=1;return Object.keys(o)})",
  "T(()=>{var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();return 'a' in o})", "T(()=>{var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();return o instanceof Object})",
  "T(()=>{var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();var x=[];for(var k in o)x.push(k);return x})",
);

// ---- 5. Proxy como protótipo, com o receptor de cada trap.
const PT = ["{a:1}", "{}", "[]", "Object.freeze({a:1})", "{get a(){return this===o?'self':'other'}}", "Object.defineProperty({},'a',{value:1,writable:false})", "Object.defineProperty({},'a',{set(v){this.s=v}})", "function(){}"];
const POPS = [
  "o.a", "o.b", "(o.a=5,Object.keys(o).join()+'|'+D(o,'a'))", "(o.b=5,Object.keys(o).join()+'|'+D(o,'b'))", "'a' in o", "'b' in o", "(()=>{var r=[];for(var k in o)r.push(k);return r})()",
  "delete o.a", "Reflect.set(o,'a',5)", "Reflect.set(o,'a',5,o)", "Reflect.set(o,'a',5,{})", "(o.a++,'ok')", "Object.keys(o).join()", "Object.getPrototypeOf(o)===p", "p.isPrototypeOf(o)",
  "o instanceof Object", "Reflect.get(o,'a',{})", "o.a", "Object.getOwnPropertyDescriptor(o,'a')", "o.hasOwnProperty('a')", "o[Symbol.iterator]", "o.toString", "Object.prototype.toString.call(o)",
  "(()=>{'use strict';o.a=5;return 'ok'})()", "Object.assign(o,{a:2}).a", "({...o})", "Object.entries(o)", "JSON.stringify(o)", "o.length", "Reflect.has(o,'a')",
  "(()=>{o.a=5;return Object.getOwnPropertyDescriptor(o,'a')!==undefined})()", "Object.setPrototypeOf(o,null)===o", "(()=>{Object.setPrototypeOf(o,{z:1});return o.z})()",
];
for (const tg of PT) for (const op of POPS) {
  add(`T(()=>{var l=[];var o;var p=mk(${tg},{get(t,k,r){l.push('get '+String(k)+' '+(r===o?'o':r===p?'p':'?'));return Reflect.get(t,k,r)},set(t,k,v,r){l.push('set '+String(k)+' '+(r===o?'o':r===p?'p':'?'));return Reflect.set(t,k,v,r)},has(t,k){l.push('has '+String(k));return k in t},defineProperty(t,k,d){l.push('def '+String(k));return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){l.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)}},l);o=Object.create(p);return E(l,()=>${op},p)})`);
}
for (const op of ["o.a", "'a' in o", "(()=>{var r=[];for(var k in o)r.push(k);return r})()", "(o.a=1,Object.keys(o).join())", "Object.keys(o).join()", "o instanceof Object", "Reflect.getPrototypeOf(o)===p", "o.toString()", "o.hasOwnProperty('a')"]) {
  add(`T(()=>{var l=[];var p=mk({a:1},{},l);var o=Object.create(Object.create(p));return E(l,()=>${op},p)})`);
  add(`T(()=>{var l=[];var p=mk({a:1},{},l);var o=Object.create(Object.create(Object.create(p)));return E(l,()=>${op},p)})`);
  add(`T(()=>{var l=[];var p=mk({a:1},{},l);function F(){};F.prototype=p;var o=new F;return E(l,()=>${op},p)})`);
  add(`T(()=>{var l=[];var p=mk({a:1},{},l);class C{};Object.setPrototypeOf(C.prototype,p);var o=new C;return E(l,()=>${op},p)})`);
}
add(
  "T(()=>{var l=[];var p=mk({},{},l);var o=Object.create(p);o.x=1;o.y=2;return E(l,()=>Object.keys(o).join(),p)})",
  "T(()=>{var l=[];var p=mk({},{},l);var o={__proto__:p};return E(l,()=>'z' in o,p)})", "T(()=>{var l=[];var p=mk({},{},l);var o={__proto__:p,z:1};return E(l,()=>'z' in o,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);function F(){};F.prototype=p;return E(l,()=>new F instanceof F,p)})",
  "T(()=>{var l=[];var p=mk({},{},l);return E(l,()=>Object.setPrototypeOf(Object.create(null),p)===undefined,p)})",
  "T(()=>{var l=[];var p=mk({},{},l);var c=class{};c.prototype=p;return 1})",
  "T(()=>{var l=[];var p=mk({},{},l);var q=Object.create(p);return E(l,()=>{var cy=Object.setPrototypeOf(p,q);return cy},p)})",
  "T(()=>{var l=[];var p=mk({},{},l);var q=Object.create(p);return E(l,()=>Reflect.setPrototypeOf(p,q),p)})",
  "T(()=>{var p=new Proxy({},{});var q=Object.create(p);return Reflect.setPrototypeOf(p,q)})", "T(()=>{var p=new Proxy({},{});var q=Object.create(p);return Object.setPrototypeOf(p,q)})",
  "T(()=>{var p=new Proxy({},{getPrototypeOf(){return q}});var q=Object.create(p);return Object.getPrototypeOf(p)===q})",
  "T(()=>{var p=new Proxy({},{getPrototypeOf(){return q}});var q=Object.create(p);return p instanceof Object})",
  "T(()=>{var p=new Proxy({},{getPrototypeOf(){return q}});var q=Object.create(p);return Object.prototype.isPrototypeOf.call(Object.prototype,p)})",
  "T(()=>{var p=new Proxy({},{getPrototypeOf(){return q}});var q=Object.create(p);return 'zz' in p})", "T(()=>{var p=new Proxy({},{getPrototypeOf(){return q}});var q=Object.create(p);return Object.getPrototypeOf(q)===p})",
);

// ---- 6. Proxy de Proxy (duas e três camadas), logs com prefixo por camada.
const NT = ["{a:1,b:2}", "[1,2]", "function(){}", "Object.freeze({a:1})", "Object.preventExtensions({a:1})", "Object.defineProperty({},'a',{value:1})"];
const NOPS = [
  "Object.keys(o)", "Object.entries(o)", "(()=>{var r=[];for(var k in o)r.push(k);return r})()", "JSON.stringify(o)", "({...o})", "Object.assign({},o)", "Array.isArray(o)",
  "o instanceof Object", "'a' in o", "delete o.a", "Reflect.ownKeys(o)", "Object.getPrototypeOf(o)===Object.prototype", "Object.setPrototypeOf(o,null)===o", "Object.isExtensible(o)",
  "Object.preventExtensions(o)===o", "Object.getOwnPropertyDescriptor(o,'a')", "Object.defineProperty(o,'z',{value:1,configurable:true})===o", "(o.a=3,'ok')", "o.a", "Reflect.set(o,'a',3)",
  "Object.freeze(o)===o", "Object.seal(o)===o", "Object.isFrozen(o)", "Object.isSealed(o)", "typeof o", "Object.prototype.toString.call(o)", "Function.prototype.toString.call(o)",
  "[].concat(o)", "Array.prototype.slice.call(o)", "o()", "new o()", "Reflect.apply(o,null,[])", "Reflect.construct(o,[])", "Object.getOwnPropertyNames(o)",
];
for (const tg of NT) for (const op of NOPS) {
  add(`T(()=>{var l=[];var i=mk(${tg},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>${op},o)})`);
  add(`T(()=>{var l=[];var i=mk(${tg},{},l,'i:');var m=mk(i,{},l,'m:');var o=mk(m,{},l,'o:');return E(l,()=>${op},o)})`);
}
for (const op of ["Object.keys(o)", "Object.getOwnPropertyDescriptor(o,'a')", "'a' in o", "o.a", "Reflect.set(o,'a',2)", "Object.isExtensible(o)", "Object.getPrototypeOf(o)===null", "Reflect.ownKeys(o)", "delete o.a"]) {
  add(`T(()=>{var l=[];var i=mk({a:1},{ownKeys(){return ['a','b']}},l,'i:');var o=mk(Object.preventExtensions({a:1}),{ownKeys(){l.push('o:ownKeys');return Reflect.ownKeys(i)}},l,'o:');return E(l,()=>${op},o)})`);
  add(`T(()=>{var l=[];var i=mk({a:1},{},l,'i:');var o=mk(Object.defineProperty({},'a',{value:1}),{getOwnPropertyDescriptor(t,k){l.push('o:gopd');return Reflect.getOwnPropertyDescriptor(i,k)}},l,'o:');return E(l,()=>${op},o)})`);
  add(`T(()=>{var l=[];var i=mk(Object.freeze({a:1}),{},l,'i:');var o=mk({a:1},{getOwnPropertyDescriptor(t,k){l.push('o:gopd');return Reflect.getOwnPropertyDescriptor(i,k)}},l,'o:');return E(l,()=>${op},o)})`);
  add(`T(()=>{var l=[];var i=mk({a:1},{isExtensible(){l.push('i:isExt');return false}},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>${op},o)})`);
  add(`T(()=>{var l=[];var i=mk({a:1},{},l,'i:');var o=mk(i,{get(t,k,r){l.push('o:get '+String(k)+' '+(r===o?'o':'?'));return Reflect.get(t,k,r)}},l,'o:');return E(l,()=>${op},o)})`);
}
add(
  "T(()=>{var l=[];var i=mk({},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>Object.prototype.toString.call(o),o)})",
  "T(()=>{var l=[];var i=mk([],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>Object.prototype.toString.call(o)+Array.isArray(o)+JSON.stringify(o),o)})",
  "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.length,o)})", "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>(o.push(3),o.length),o)})",
  "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>[...o],o)})", "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.map(x=>x*2),o)})",
  "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>[].concat(o),o)})", "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.splice(0,1),o)})",
  "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.slice(1),o)})", "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.concat([3]),o)})",
  "T(()=>{var l=[];var i=mk([3,1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.sort()===o,o)})", "T(()=>{var l=[];var i=mk([3,1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.reverse()===o,o)})",
  "T(()=>{var l=[];var i=mk([[1],[2]],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.flat(),o)})", "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.includes(2),o)})",
  "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>(o.length=0,o.length),o)})", "T(()=>{var l=[];var i=mk([1,2],{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>Object.freeze(o)===o,o)})",
  "T(()=>{var l=[];var i=mk(function f(a,b){return a+b},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o(1,2),o)})", "T(()=>{var l=[];var i=mk(function f(a,b){this.v=a},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>new o(1).v,o)})",
  "T(()=>{var l=[];var i=mk(function f(a,b){return a+b},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.name+o.length,o)})", "T(()=>{var l=[];var i=mk(function f(a,b){return a+b},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.call(null,1,2),o)})",
  "T(()=>{var l=[];var i=mk(function f(a,b){return a+b},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.bind(null,1)(2),o)})", "T(()=>{var l=[];var i=mk(function f(a,b){return a+b},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.apply(null,[1,2]),o)})",
  "T(()=>{var l=[];var i=mk(class A{},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>(class B extends o{},1),o)})", "T(()=>{var l=[];var i=mk(class A{},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o.prototype===i.prototype,o)})",
  "T(()=>{var l=[];var i=mk(class A{},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>new o instanceof o,o)})", "T(()=>{var l=[];var i=mk(class A{},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>Reflect.construct(o,[],Object).constructor===Object,o)})",
  "T(()=>{var l=[];var i=mk(class A{},{},l,'i:');var o=mk(i,{},l,'o:');return E(l,()=>o(),o)})",
);

// ---- 7. Casos soltos de invariantes e mensagens.
add(
  "T(()=>{var p=new Proxy({},{ownKeys(){return ['a','a']}});return Reflect.ownKeys(p)})", "T(()=>{var p=new Proxy({},{ownKeys(){return [1]}});return Reflect.ownKeys(p)})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return [Symbol.iterator,'a']}});return Reflect.ownKeys(p).length})", "T(()=>{var p=new Proxy({},{ownKeys(){return {length:2,0:'a',1:'b'}}});return Reflect.ownKeys(p).join()})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return {length:-1}}});return Reflect.ownKeys(p).length})", "T(()=>{var p=new Proxy({},{ownKeys(){return 'ab'}});return Reflect.ownKeys(p)})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return new Proxy(['a'],{})}});return Reflect.ownKeys(p).join()})", "T(()=>{var l=[];var p=new Proxy({},{ownKeys(){return mk(['a','b'],{},l)}});return E(l,()=>Reflect.ownKeys(p).join(),p)})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return {length:1,get 0(){throw new EvalError('x')}}}});return Reflect.ownKeys(p)})",
  "T(()=>{var p=new Proxy({},{getOwnPropertyDescriptor(){return new Proxy({value:1,configurable:true},{})}});return D(p,'a')})",
  "T(()=>{var l=[];var p=new Proxy({},{getOwnPropertyDescriptor(){return mk({value:1,configurable:true},{},l)}});return E(l,()=>D(p,'a'),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{defineProperty(t,k,d){l.push(Reflect.ownKeys(d).join());return true}});return E(l,()=>Object.defineProperty(p,'a',{value:1,get:undefined}),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{defineProperty(t,k,d){l.push(Reflect.ownKeys(d).join());return true}});return E(l,()=>Object.defineProperty(p,'a',{set(v){},enumerable:true}),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{defineProperty(t,k,d){l.push(Reflect.ownKeys(d).join());return true}});return E(l,()=>Object.defineProperty(p,'a',{}),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{defineProperty(t,k,d){l.push(Reflect.ownKeys(d).join());return true}});return E(l,()=>Object.defineProperties(p,{a:{value:1},b:{get(){}}}),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{defineProperty(t,k,d){l.push(String(k)+':'+Object.keys(d).join());return true}});return E(l,()=>Object.assign(p,{a:1}),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{defineProperty(t,k,d){l.push(String(k)+':'+Object.keys(d).join());return Reflect.defineProperty(t,k,d)}});return E(l,()=>(p.a=1,p.a=2,Object.keys(p).join()),p)})",
  "T(()=>{var l=[];var p=new Proxy({a:1},{defineProperty(t,k,d){l.push(String(k)+':'+Object.keys(d).join());return Reflect.defineProperty(t,k,d)}});return E(l,()=>(p.a=2),p)})",
  "T(()=>{var l=[];var p=new Proxy([],{defineProperty(t,k,d){l.push(String(k)+':'+Object.keys(d).join());return Reflect.defineProperty(t,k,d)}});return E(l,()=>(p.push(1,2),p.length),p)})",
  "T(()=>{var l=[];var p=new Proxy([1,2,3],{defineProperty(t,k,d){l.push(String(k)+':'+Object.keys(d).join());return Reflect.defineProperty(t,k,d)},deleteProperty(t,k){l.push('del '+String(k));return Reflect.deleteProperty(t,k)}});return E(l,()=>(p.length=1,p.length),p)})",
  "T(()=>{var l=[];var p=new Proxy({},{set(t,k,v,r){l.push('set '+String(k)+' '+(r===p));return true}});var o=Object.create(p);return E(l,()=>(o.x=1,Object.keys(o).join()),p)})",
  "T(()=>{var p=new Proxy({},{get(t,k,r){return r===p}});return p.a+','+Reflect.get(p,'a')+','+Reflect.get(p,'a',1)})",
  "T(()=>{var p=new Proxy({},{get(t,k,r){return typeof r}});return Reflect.get(p,'a',1)+Reflect.get(p,'a','s')+Reflect.get(p,'a',undefined)})",
  "T(()=>{var p=new Proxy({},{set(t,k,v,r){return typeof r==='number'}});return Reflect.set(p,'a',1,5)+','+Reflect.set(p,'a',1)})",
  "T(()=>{var p=new Proxy({},{has(){return false}});with(p){return typeof Math}})", "T(()=>{var p=new Proxy({},{has(){return true},get(t,k){return k===Symbol.unscopables?undefined:1}});return Function('p','with(p){return Math}')(p)})",
  "T(()=>{var l=[];var p=new Proxy({},{has(t,k){l.push('has '+String(k));return true},get(t,k){l.push('get '+String(k));return undefined}});return E(l,()=>Function('p','with(p){return x}')(p),p)})",
  "T(()=>{var l=[];var p=new Proxy({x:1},{has(t,k){l.push('has '+String(k));return k in t},get(t,k){l.push('get '+String(k));return t[k]},set(t,k,v){l.push('set '+String(k));t[k]=v;return true}});return E(l,()=>Function('p','with(p){x=2;return x}')(p),p)})",
  "T(()=>{var l=[];var p=new Proxy({x:1},{has(t,k){l.push('has '+String(k));return k in t},get(t,k){l.push('get '+String(k));return t[k]},deleteProperty(t,k){l.push('del '+String(k));return delete t[k]}});return E(l,()=>Function('p','with(p){return delete x}')(p),p)})",
  "T(()=>{var l=[];var p=new Proxy({x:1},{has(t,k){l.push('has '+String(k));return k in t},get(t,k){l.push('get '+String(k));return t[k]}});return E(l,()=>Function('p','with(p){return typeof x}')(p),p)})",
  "T(()=>{var l=[];var p=new Proxy({x(){return this===p}},{has(t,k){l.push('has '+String(k));return k in t},get(t,k){l.push('get '+String(k));return t[k]}});return E(l,()=>Function('p','with(p){return x()}')(p),p)})",
  "T(()=>{var l=[];var p=new Proxy({x:1},{has(t,k){l.push('has '+String(k));return k in t},get(t,k){l.push('get '+String(k));return k===Symbol.unscopables?{x:true}:t[k]}});return E(l,()=>Function('p','var x=5;with(p){return x}')(p),p)})",
  "T(()=>typeof new Proxy(function(){},{}))", "T(()=>typeof new Proxy(class{},{}))", "T(()=>typeof new Proxy({},{}))", "T(()=>typeof new Proxy(Math.max,{}))", "T(()=>typeof new Proxy(()=>1,{}))",
  "T(()=>typeof new Proxy(new Proxy(function(){},{}),{}))", "T(()=>typeof new Proxy([],{}))", "T(()=>typeof new Proxy(Symbol,{}))", "T(()=>typeof new Proxy(Object(1n),{}))",
  "T(()=>Object.prototype.toString.call(new Proxy(function(){},{})))", "T(()=>Object.prototype.toString.call(new Proxy(new Date,{})))", "T(()=>Object.prototype.toString.call(new Proxy(new Error,{})))",
  "T(()=>Object.prototype.toString.call(new Proxy(new Map,{})))", "T(()=>Object.prototype.toString.call(new Proxy(/x/,{})))", "T(()=>Object.prototype.toString.call(new Proxy(Object('s'),{})))",
  "T(()=>Object.prototype.toString.call(new Proxy((function(){return arguments})(),{})))", "T(()=>Function.prototype.toString.call(new Proxy(function f(){},{})))",
  "T(()=>Function.prototype.toString.call(new Proxy(class A{},{})))", "T(()=>Function.prototype.toString.call(new Proxy(Math.max,{})))", "T(()=>Function.prototype.toString.call(new Proxy({},{})))",
  "T(()=>Function.prototype.toString.call(new Proxy(()=>1,{})))", "T(()=>Function.prototype.toString.call(new Proxy(new Proxy(function(){},{}),{})))", "T(()=>Function.prototype.toString.call(new Proxy(async function(){},{})))",
  "T(()=>Function.prototype.toString.call(new Proxy(function*(){},{})))", "T(()=>Function.prototype.toString.call(new Proxy(function(){}.bind(),{})))",
  "T(()=>Array.isArray(new Proxy([],{}))+','+Array.isArray(new Proxy({},{}))+','+Array.isArray(new Proxy(new Proxy([],{}),{})))", "T(()=>JSON.stringify(new Proxy([1,2],{})))", "T(()=>JSON.stringify(new Proxy({a:1},{})))",
  "T(()=>JSON.stringify(new Proxy({a:1},{ownKeys(){return ['a','b']}})))", "T(()=>JSON.stringify(new Proxy({a:1,b:2},{ownKeys(){return ['b','a']}})))",
  "T(()=>JSON.stringify(new Proxy({a:1,b:2},{getOwnPropertyDescriptor(t,k){var d=Reflect.getOwnPropertyDescriptor(t,k);if(k==='a')d.enumerable=false;return d}})))",
  "T(()=>JSON.stringify(new Proxy([1,2],{get(t,k,r){return k==='length'?1:t[k]}})))", "T(()=>JSON.stringify({p:new Proxy({a:1},{get(t,k){return k==='toJSON'?()=>'J':t[k]}})}))",
  "T(()=>Object.keys(new Proxy({a:1,b:2},{getOwnPropertyDescriptor(t,k){var d=Reflect.getOwnPropertyDescriptor(t,k);if(k==='a')d.enumerable=false;return d}})).join())",
  "T(()=>Object.keys(new Proxy({a:1,b:2},{ownKeys(){return ['b','a','c']}})).join())", "T(()=>Object.keys(new Proxy({a:1},{ownKeys(){return ['a','a']}})).join())",
  "T(()=>Object.getOwnPropertyNames(new Proxy({a:1},{ownKeys(){return ['a',Symbol.iterator]}})).join())", "T(()=>Object.getOwnPropertySymbols(new Proxy({a:1},{ownKeys(){return ['a',Symbol.iterator]}})).length)",
  "T(()=>Object.isFrozen(new Proxy(Object.freeze({a:1}),{})))", "T(()=>Object.isFrozen(new Proxy({a:1},{isExtensible(){return false}})))", "T(()=>Object.isFrozen(new Proxy(Object.preventExtensions({}),{})))",
  "T(()=>Object.isSealed(new Proxy(Object.seal({a:1}),{})))", "T(()=>Object.isFrozen(new Proxy(Object.preventExtensions({a:1}),{getOwnPropertyDescriptor(){return {value:1,configurable:true}}})))",
  "T(()=>Object.freeze(new Proxy({a:1},{})).a)", "T(()=>Object.isFrozen(Object.freeze(new Proxy({a:1},{}))))", "T(()=>{var t={a:1};Object.freeze(new Proxy(t,{}));return Object.isFrozen(t)})",
  "T(()=>{var t={a:1};Object.seal(new Proxy(t,{}));return Object.isSealed(t)})", "T(()=>{var t={a:1};Object.preventExtensions(new Proxy(t,{}));return Object.isExtensible(t)})",
  "T(()=>Object.fromEntries(new Proxy([['a',1],['b',2]],{})).b)", "T(()=>JSON.stringify(Object.fromEntries(new Proxy(new Map([['a',1]]),{}))))",
  "T(()=>JSON.stringify(Object.fromEntries(new Proxy([new Proxy(['a',1],{})],{}))))",
  "T(()=>{var l=[];return E(l,()=>JSON.stringify(Object.fromEntries(mk([['a',1],['b',2]],{},l))),0)})", "T(()=>{var l=[];return E(l,()=>JSON.stringify(Object.fromEntries([mk(['a',1],{},l)])),0)})",
  "T(()=>{var l=[];return E(l,()=>Array.prototype.concat.call([],mk([1,2],{},l)).length,0)})", "T(()=>{var l=[];return E(l,()=>Array.prototype.concat.call([],mk([1,2],{[Symbol.isConcatSpreadable]:true},l)).length,0)})",
  "T(()=>{var l=[];return E(l,()=>[].concat(mk({length:2,0:'a',1:'b',[Symbol.isConcatSpreadable]:true},{},l)).length,0)})", "T(()=>{var l=[];return E(l,()=>[].concat(mk({length:2},{},l)).length,0)})",
  "T(()=>{var l=[];return E(l,()=>Array.from(mk(new Set([1,2]),{},l)).length,0)})", "T(()=>{var l=[];return E(l,()=>Array.from(mk({length:2,0:'a',1:'b'},{},l)).join(),0)})",
  "T(()=>{var l=[];return E(l,()=>[...mk([1,2],{},l)].length,0)})", "T(()=>{var l=[];return E(l,()=>Math.max(...mk([1,2],{},l)),0)})",
  "T(()=>{var l=[];return E(l,()=>Object.assign({},mk({a:1,b:2},{},l)).b,0)})", "T(()=>{var l=[];return E(l,()=>Object.assign(mk({},{},l),{a:1,b:2}).b,0)})",
  "T(()=>{var l=[];return E(l,()=>({...mk({a:1,[Symbol.for('s')]:2},{},l)})[Symbol.for('s')],0)})", "T(()=>{var l=[];return E(l,()=>Object.entries(mk({a:1,b:2},{},l)).length,0)})",
  "T(()=>{var l=[];var p=mk({a:1,b:2},{getOwnPropertyDescriptor(t,k){l.push('gopd '+k);return k==='a'?undefined:Reflect.getOwnPropertyDescriptor(t,k)}},l);return E(l,()=>Object.keys(p).join(),p)})",
  "T(()=>{var l=[];var p=mk({a:1,b:2},{getOwnPropertyDescriptor(t,k){l.push('gopd '+k);return k==='a'?undefined:Reflect.getOwnPropertyDescriptor(t,k)}},l);return E(l,()=>Object.entries(p).join(';'),p)})",
  "T(()=>{var l=[];var p=mk({a:1,b:2},{ownKeys(t){l.push('ownKeys');return ['b','a']}},l);return E(l,()=>(()=>{var r=[];for(var k in p)r.push(k);return r})().join(),p)})",
  "T(()=>{var l=[];var p=mk({a:1,b:2},{ownKeys(t){l.push('ownKeys');return ['b','a']}},l);return E(l,()=>Object.entries(p).join(';'),p)})",
  "T(()=>{var l=[];var p=mk({a:1,b:2},{get(t,k,r){l.push('get '+String(k));return k==='toJSON'?undefined:'v'+String(k)}},l);return E(l,()=>JSON.stringify(p),p)})",
  "T(()=>{var l=[];var p=mk({a:1},{},l);var s=new Set([p]);return E(l,()=>s.has(p)+','+s.size,p)})",
  "T(()=>{var l=[];var p=mk({},{},l);return E(l,()=>{var o={};o[p]=1;return Object.keys(o).join()},p)})", "T(()=>{var l=[];var p=mk({toString(){return 'k'}},{},l);return E(l,()=>{var o={};o[p]=1;return Object.keys(o).join()},p)})",
  "T(()=>{var l=[];var p=mk({},{},l);return E(l,()=>p+'',p)})", "T(()=>{var l=[];var p=mk({valueOf(){return 3}},{},l);return E(l,()=>p*2,p)})", "T(()=>{var l=[];var p=mk({[Symbol.toPrimitive](){return 4}},{},l);return E(l,()=>+p,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>p instanceof Function,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>({}) instanceof p,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Function.prototype[Symbol.hasInstance].call(p,{}),p)})", "T(()=>{var l=[];var p=mk(class{},{},l);return E(l,()=>(new p) instanceof p,p)})",
  "T(()=>{var l=[];var p=mk({[Symbol.hasInstance](){return true}},{},l);return E(l,()=>1 instanceof p,p)})", "T(()=>{var l=[];var p=mk({},{},l);return E(l,()=>1 instanceof p,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Object.keys(p).length+p.name+p.length,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>p.prototype.constructor===p,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>{class B extends p{};return Object.getPrototypeOf(B)===p},p)})", "T(()=>{var l=[];var p=mk({},{},l);return E(l,()=>{class B extends p{}},p)})",
  "T(()=>{var l=[];var p=mk(function(){},{get(t,k,r){l.push('get '+String(k));return k==='prototype'?null:Reflect.get(t,k,r)}},l);return E(l,()=>{class B extends p{};return Object.getPrototypeOf(B.prototype)},p)})",
  "T(()=>{var l=[];var p=mk(function(){},{get(t,k,r){l.push('get '+String(k));return k==='prototype'?1:Reflect.get(t,k,r)}},l);return E(l,()=>{class B extends p{}},p)})",
  "T(()=>{var l=[];var p=mk(()=>1,{},l);return E(l,()=>{class B extends p{}},p)})", "T(()=>{var l=[];var p=mk(function*(){},{},l);return E(l,()=>{class B extends p{}},p)})",
  "T(()=>{var l=[];var p=mk(class{},{},l);return E(l,()=>{class B extends p{constructor(){super();this.z=1}};return new B().z},p)})",
  "T(()=>{var l=[];var p=mk(class{},{construct(t,a,n){l.push('construct '+(n===B?'B':'?'));return Reflect.construct(t,a,n)}},l);class B extends p{};return E(l,()=>new B instanceof B,p)})",
  "T(()=>{var l=[];var p=mk(class{},{construct(t,a,n){l.push('construct');return {}}},l);class B extends p{};return E(l,()=>new B instanceof B,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>new p instanceof p,p)})", "T(()=>{var l=[];var p=mk(function(){this.a=1},{},l);return E(l,()=>new p().a,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(p,[],p)!==undefined,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Object,[],p)!==undefined,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(function(){},[],p)!==undefined,p)})", "T(()=>{var l=[];var p=mk(function(){},{get(t,k,r){l.push('get '+String(k));return k==='prototype'?Array.prototype:Reflect.get(t,k,r)}},l);return E(l,()=>Array.isArray(Reflect.construct(function(){},[],p)),p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>(new p).constructor===p,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Array,[3],p).length,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Date,[0],p) instanceof Date,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Map,[],p) instanceof Map,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Promise,[()=>{}],p) instanceof Promise,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Error,['m'],p).message,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(RegExp,['a'],p).source,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Function,[],p) instanceof Function,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(String,['ab'],p).length,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Number,[1],p)+1,p)})",
  "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(Boolean,[0],p)?1:2,p)})", "T(()=>{var l=[];var p=mk(function(){},{},l);return E(l,()=>Reflect.construct(WeakMap,[],p) instanceof WeakMap,p)})",
);

// ---- Execução, com concorrência limitada.
const baseSources = [];
for (const file of fs.readdirSync(path.join(__dirname, "..", "tests", "golden")).filter(f => /^proxy.*\.tsv$/.test(f) && f !== "proxy_invariants_bun.tsv")) {
  for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  if (baseText.includes(expr)) { dup++; continue; }
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
}
const runChild = job => new Promise(resolve => {
  // Processo fresco por programa: a ordem de reificação das tabelas estáticas do JSC depende do que rodou antes.
  const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
  let out = "";
  let err = "";
  child.stdout.on("data", d => (out += d));
  child.stderr.on("data", d => (err += d));
  // Programa que não termina (laço por protótipo cíclico via Proxy) é descartado em vez de travar a geração.
  const timer = setTimeout(() => child.kill("SIGKILL"), 4000);
  child.on("close", code => { clearTimeout(timer); const decoded = code === 0 ? decodeResult(out) : null; resolve({ ok: decoded !== null, out: decoded, err }); });
  child.stdin.end(job.source);
});
(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i]);
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  jobs.forEach((job, i) => {
    const r = results[i];
    if (!r.ok) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + " " + r.err.slice(0, 100) + "\n"); return; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(job.expr).slice(0, 160) + "\n"); return; }
    kept++;
    lines.push({ source: job.source, result: r.out });
  });
  process.stdout.write(emitFactored("proxy_invariants", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
