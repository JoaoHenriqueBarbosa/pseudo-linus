const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/proxy_trace_bun.tsv: a SEQUÊNCIA de traps de Proxy disparadas por operações internas da
// linguagem (spread, for-in, Object.keys/entries, JSON.stringify, Array.prototype.* em proxy de array, instanceof,
// in, with, delete, class extends, Symbol.toPrimitive, Object.assign, destructuring...) e as violações de invariante
// (TypeError com mensagem), medidas no bun 1.4.2. `structuredClone` fica de fora: não é do JavaScriptCore.
// Cada programa cria um alvo, embrulha num Proxy cujo handler registra cada trap (nome e chave) e repassa para
// `Reflect`, roda uma operação e grava em `R` a sequência de traps seguida do resultado ou da exceção.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// O programa roda em modo sloppy (por causa do `with`).
// Uso: bun scripts/gen-proxy-trace-golden.js > tests/golden/proxy_trace_bun.tsv
const TRAPS = ["get", "set", "has", "deleteProperty", "defineProperty", "getOwnPropertyDescriptor", "ownKeys", "getPrototypeOf",
  "setPrototypeOf", "isExtensible", "preventExtensions", "apply", "construct"];

const PRELUDE =
  'var L=[];\n' +
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'Array.isArray(v)?"["+v.map(S).join(",")+"]":(v&&typeof v==="object")?"{"+Object.keys(v).map(function(k){return k+":"+S(v[k])}).join(",")+"}":String(v)}catch(e){return "?"}}\n' +
  'function K(n,a){return (n==="apply"||n==="construct"||n==="ownKeys"||n==="getPrototypeOf"||n==="isExtensible"||n==="preventExtensions")?n:n+":"+(typeof a[1]==="symbol"?a[1].toString():String(a[1]))}\n' +
  'function mk(t){var h={};' + JSON.stringify(TRAPS) + '.forEach(function(n){h[n]=function(){var a=[].slice.call(arguments);L.push(K(n,a));return Reflect[n].apply(null,a)}});return new Proxy(t,h)}\n' +
  'function run(f){L=[];var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.join(" ")+" => "+r}\n' +
  'var SYM=Symbol("s");\n';

const targets = {
  obj: '{a:1,b:2,[SYM]:3}',
  arr: '[1,2,3]',
  fn: 'function f(x){return x}',
  cls: 'class A{constructor(){this.v=1}}',
  accessor: '{get a(){return 1},set a(v){},b:2}',
  frozen: 'Object.freeze({a:1,b:2})',
};

const exprs = [];
const addOp = (body, only) => exprs.push({ body, only });
const objOnly = ["obj", "frozen", "accessor"];
const arrOnly = ["arr"];
// ---- Operações genéricas (valem em todos os alvos).
[
  "({...P})", "Object.keys(P)", "Object.values(P)", "Object.entries(P)", "Object.getOwnPropertyNames(P)", "Object.getOwnPropertySymbols(P)",
  "Object.getOwnPropertyDescriptors(P)", "(function(){var r=[];for(var k in P)r.push(k);return r})()", "JSON.stringify(P)", "JSON.stringify(P,null,1)",
  "P instanceof Object", "({}) instanceof P", "Object.create(P).x", "(function(){var o=Object.create(P);o.x=1;return o.x})()",
  '"a" in P', "0 in P", "SYM in P", "delete P.a", "delete P[0]", "delete P[SYM]", "P.a", "P[0]", "P[SYM]", "(function(){P.a=5;return P.a})()",
  "(function(){P.z=5;return P.z})()", "(function(){P.a++;return P.a})()", "(function(){P.a+=1;return P.a})()",
  "Object.assign({},P)", "Object.assign(P,{x:1})", "Object.assign(P,{a:5})", "Object.freeze(P)", "Object.isFrozen(P)", "Object.seal(P)", "Object.isSealed(P)",
  "Object.preventExtensions(P)", "Object.isExtensible(P)", "Object.setPrototypeOf(P,null)", "Object.getPrototypeOf(P)", "P.__proto__",
  "Object.defineProperty(P,'z',{value:1})", "Object.defineProperty(P,'a',{value:9})", "Object.defineProperties(P,{z:{value:1},y:{value:2}})",
  "Object.getOwnPropertyDescriptor(P,'a')", "Object.prototype.hasOwnProperty.call(P,'a')", "Object.prototype.propertyIsEnumerable.call(P,'a')",
  "Object.hasOwn(P,'a')", "Reflect.ownKeys(P)", "Reflect.has(P,'a')", "Reflect.get(P,'a')", "Reflect.set(P,'a',2)", "Reflect.defineProperty(P,'a',{value:3})",
  "(function(){var {a,...r}=P;return [a,r]})()", "(function(){var {a,b}=P;return [a,b]})()", "Object.prototype.toString.call(P)", "String(P)", "P+''", "`${P}`", "+P",
  "P==1", "P<1", "[P]+''", "Object.prototype.isPrototypeOf.call(P,{})", "Object.fromEntries(Object.entries(P))", "Object.is(P,P)", "typeof P",
  "Array.isArray(P)", "Array.from(P)", "[...P]", "Math.max(...P)", "(function(){var [x,y]=P;return [x,y]})()", "[].concat(P)", "Array.prototype.concat.call([],P)",
  "with(P){typeof a}", "with(P){a}", "with(P){a=1}", "with(P){typeof zzz}", "with(P){b}", "with(P){length}",
  "P()", "new P()", "P.call(null,1)", "P.bind(null,1)()", "Function.prototype.toString.call(P).length>0", "P.toString()", "P.hasOwnProperty('a')", "Object.keys(Object.create(P))",
  "(function(){class C extends P{};return typeof C})()", "(function(){class C extends P{};return Object.getPrototypeOf(C.prototype)===P.prototype})()",
  "(function(){class C extends P{};return new C()})()", "P.prototype", "P.length", "P.name",
].forEach(b => addOp(b));
// ---- Operações de Array.prototype em proxy de array.
const arrayMethods = ["map(function(x){return x})", "filter(function(x){return true})", "forEach(function(x){})", "push(9)", "pop()", "shift()", "unshift(0)",
  "splice(1,1)", "slice(0)", "slice(1,2)", "indexOf(2)", "lastIndexOf(2)", "includes(2)", "join('-')", "reverse()", "sort()", "fill(0)", "find(function(x){return x===2})",
  "findLast(function(x){return x===2})", "findIndex(function(x){return x===2})", "flat()", "flatMap(function(x){return [x]})", "at(-1)", "copyWithin(0,1)", "every(function(x){return x})",
  "some(function(x){return x>5})", "reduce(function(a,b){return a+b})", "reduceRight(function(a,b){return a+b})", "keys()", "entries()", "values()", "toString()", "toReversed()", "toSorted()",
  "toSpliced(0,1)", "with(0,9)", "concat([4])", "splice(0,0,7)", "splice(1)", "length"];
for (const m of arrayMethods) addOp(m === "length" ? "P.length" : `Array.prototype.${m.replace("(", ".call(P" + (/\(\)/.test(m) ? "" : ",")).replace(".call(P,)", ".call(P)")}`, arrOnly);
addOp("(function(){P.length=1;return P.length})()", arrOnly);
addOp("(function(){P.push(4,5);return P.length})()", arrOnly);
addOp("(function(){return [...P.entries()]})()", arrOnly);
addOp("P.map(function(x){return x*2})", arrOnly);
addOp("P.concat([9])", arrOnly);
addOp("[0].concat(P)", arrOnly);
addOp("(function(){var s=[];for(var x of P)s.push(x);return s})()", arrOnly);
addOp("JSON.stringify({k:P})", arrOnly);
addOp("P.sort(function(a,b){return b-a})", arrOnly);
addOp("P.indexOf(3)", arrOnly);
addOp("Object.keys(P)", arrOnly);
// ---- Symbol.toPrimitive e hasInstance.
addOp("(function(){P[Symbol.toPrimitive]=function(){return 5};return P*2})()", objOnly);
addOp("(function(){var q=mk({[Symbol.toPrimitive](h){return h}});return [`${q}`,q+'',+q]})()", ["obj"]);
addOp("({}) instanceof mk(class{static [Symbol.hasInstance](){return true}})", ["obj"]);
addOp("(function(){var q=mk(function(){});return Object.getPrototypeOf(new q())===q.prototype})()", ["fn"]);
addOp("(function(){var f=mk(function(){});f.prototype={};return {} instanceof f})()", ["fn"]);
addOp("Reflect.construct(function(){},[],P)", ["fn", "cls"]);
addOp("Reflect.apply(P,null,[1])", ["fn"]);
addOp("P.apply(null,[1])", ["fn"]);
addOp("new P(1)", ["cls"]);
addOp("(function(){class C extends P{constructor(){super();this.w=2}};return Object.keys(new C())})()", ["cls", "fn"]);
addOp("Object.getOwnPropertyNames(P)", ["fn", "cls"]);

const seen = new Set();
const programs = [];
const addProgram = source => { if (!seen.has(source)) { seen.add(source); programs.push(source); } };
for (const { body, only } of exprs) {
  for (const name of Object.keys(targets)) {
    if (only && !only.includes(name)) continue;
    const withBody = /^with\(P\)\{(.*)\}$/.exec(body);
    const stmt = withBody ? `with(P){return ${withBody[1].startsWith("a=") ? "(" + withBody[1] + ")" : withBody[1]}}` : `return ${body}`;
    addProgram(`${PRELUDE}var T0=${targets[name]};var P=mk(T0);globalThis.R=run(function(){${stmt}});`);
  }
}

// ---- Invariantes violadas (handler explícito, só os traps que o caso viola).
const inv = [];
const addInv = (setup, body) => inv.push({ setup, body });
const NC = "var t={};Object.defineProperty(t,'a',{value:1,writable:false,configurable:false});";
const NCW = "var t={};Object.defineProperty(t,'a',{value:1,writable:true,configurable:false});";
const NCA = "var t={};Object.defineProperty(t,'a',{get:undefined,set:undefined,configurable:false});";
const FRZ = "var t=Object.freeze({a:1});";
const NEXT = "var t=Object.preventExtensions({a:1});";
const mkp = h => `var P=new Proxy(t,${h});`;
addInv(NC + mkp("{get(){return 2}}"), "P.a");
addInv(NC + mkp("{get(){return 1}}"), "P.a");
addInv(NCA + mkp("{get(){return 1}}"), "P.a");
addInv(NC + mkp("{set(){return true}}"), "(function(){P.a=2;return 'ok'})()");
addInv(NC + mkp("{set(){return true}}"), "(function(){P.a=1;return 'ok'})()");
addInv(NCA + mkp("{set(){return true}}"), "(function(){P.a=1;return 'ok'})()");
addInv(NC + mkp("{set(){return false}}"), "(function(){'use strict';P.a=2;return 'ok'})()");
addInv(NC + mkp("{set(){return false}}"), "(function(){P.a=2;return 'ok'})()");
addInv(NC + mkp("{has(){return false}}"), "'a' in P");
addInv(NEXT + mkp("{has(){return false}}"), "'a' in P");
addInv(NC + mkp("{deleteProperty(){return true}}"), "delete P.a");
addInv(NEXT + mkp("{deleteProperty(){return true}}"), "delete P.a");
addInv(FRZ + mkp("{deleteProperty(){return true}}"), "delete P.a");
addInv(NC + mkp("{defineProperty(){return true}}"), "Object.defineProperty(P,'a',{value:2})");
addInv(NC + mkp("{defineProperty(){return true}}"), "Object.defineProperty(P,'b',{value:2,configurable:false})");
addInv(NEXT + mkp("{defineProperty(){return true}}"), "Object.defineProperty(P,'b',{value:2})");
addInv("var t={a:1};" + mkp("{defineProperty(){return true}}"), "Object.defineProperty(P,'a',{value:2,configurable:false})");
addInv(NC + mkp("{defineProperty(){return false}}"), "Object.defineProperty(P,'a',{value:1})");
addInv(NC + mkp("{defineProperty(){return false}}"), "Reflect.defineProperty(P,'a',{value:1})");
addInv(NCW + mkp("{defineProperty(){return true}}"), "Object.defineProperty(P,'a',{value:1,writable:false})");
addInv(NC + mkp("{getOwnPropertyDescriptor(){return undefined}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv(NEXT + mkp("{getOwnPropertyDescriptor(){return undefined}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv("var t={};" + mkp("{getOwnPropertyDescriptor(){return {value:1,configurable:false}}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv("var t={};" + mkp("{getOwnPropertyDescriptor(){return {value:1,configurable:true}}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv(NEXT + mkp("{getOwnPropertyDescriptor(){return {value:1,configurable:true}}}"), "Object.getOwnPropertyDescriptor(P,'zz')");
addInv(NC + mkp("{getOwnPropertyDescriptor(){return {value:2,configurable:false}}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv("var t={a:1};" + mkp("{getOwnPropertyDescriptor(){return 1}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv("var t={a:1};" + mkp("{getOwnPropertyDescriptor(){return {value:1,configurable:false,writable:false}}}"), "Object.getOwnPropertyDescriptor(P,'a')");
addInv(NC + mkp("{ownKeys(){return []}}"), "Object.keys(P)");
addInv(NEXT + mkp("{ownKeys(){return []}}"), "Object.keys(P)");
addInv(NEXT + mkp("{ownKeys(){return ['a','b']}}"), "Object.keys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){return ['a','a']}}"), "Object.keys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){return ['a',1]}}"), "Object.keys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){return 1}}"), "Object.keys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){return {length:1,0:'a'}}}"), "Object.keys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){return null}}"), "Reflect.ownKeys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){return [Symbol.iterator,{}]}}"), "Reflect.ownKeys(P)");
addInv(NEXT + mkp("{getPrototypeOf(){return Array.prototype}}"), "Object.getPrototypeOf(P)");
addInv("var t={};" + mkp("{getPrototypeOf(){return 1}}"), "Object.getPrototypeOf(P)");
addInv("var t={};" + mkp("{getPrototypeOf(){return undefined}}"), "Object.getPrototypeOf(P)");
addInv("var t={};" + mkp("{getPrototypeOf(){return null}}"), "Object.getPrototypeOf(P)");
addInv("var t={};" + mkp("{getPrototypeOf(){return Array.prototype}}"), "P instanceof Array");
addInv(NEXT + mkp("{setPrototypeOf(){return true}}"), "Object.setPrototypeOf(P,Array.prototype)");
addInv(NEXT + mkp("{setPrototypeOf(){return false}}"), "Object.setPrototypeOf(P,Array.prototype)");
addInv("var t={};" + mkp("{setPrototypeOf(){return false}}"), "Object.setPrototypeOf(P,null)");
addInv("var t={};" + mkp("{setPrototypeOf(){return false}}"), "Reflect.setPrototypeOf(P,null)");
addInv("var t={};" + mkp("{isExtensible(){return false}}"), "Object.isExtensible(P)");
addInv(NEXT + mkp("{isExtensible(){return true}}"), "Object.isExtensible(P)");
addInv("var t={};" + mkp("{preventExtensions(){return true}}"), "Object.preventExtensions(P)");
addInv("var t={};" + mkp("{preventExtensions(){return false}}"), "Object.preventExtensions(P)");
addInv("var t={};" + mkp("{preventExtensions(){return false}}"), "Reflect.preventExtensions(P)");
addInv("var t={};" + mkp("{preventExtensions(){return false}}"), "Object.freeze(P)");
addInv("var t={};" + mkp("{preventExtensions(){return false}}"), "Object.seal(P)");
addInv("var t={a:1};" + mkp("{defineProperty(){return false}}"), "Object.freeze(P)");
addInv("var t={a:1};" + mkp("{defineProperty(){return false}}"), "Object.seal(P)");
addInv("var t=function(){};" + mkp("{apply(){return 7}}"), "P()");
addInv("var t={};" + mkp("{apply(){return 7}}"), "P()");
addInv("var t=function(){};" + mkp("{construct(){return 1}}"), "new P()");
addInv("var t=function(){};" + mkp("{construct(){return {c:1}}}"), "new P()");
addInv("var t=function(){};" + mkp("{construct:1}"), "new P()");
addInv("var t=function(){};" + mkp("{get:1}"), "P.a");
addInv("var t=function(){};" + mkp("{get:null}"), "P.a");
addInv("var t=function(){};" + mkp("{get:undefined}"), "P.a");
addInv("var t=function(){};" + mkp("{has:{}}"), "'a' in P");
addInv("var t=function(){};" + mkp("{ownKeys:'x'}"), "Object.keys(P)");
addInv("var t=function(){};" + mkp("{apply:true}"), "P()");
addInv("var t={};" + mkp("{get(){throw new RangeError('boom')}}"), "P.a");
addInv("var t={};" + mkp("{get(){throw new RangeError('boom')}}"), "({...P})");
addInv("var t={a:1};" + mkp("{ownKeys(){throw new RangeError('boom')}}"), "Object.keys(P)");
addInv("var t={a:1};" + mkp("{ownKeys(){throw new RangeError('boom')}}"), "JSON.stringify(P)");
addInv("var t={a:1};" + mkp("{has(){throw new RangeError('boom')}}"), "'a' in P");
addInv("var t={a:1};" + mkp("{has(){throw new RangeError('boom')}}"), "(function(){with(P){return a}})()");
addInv("var t={a:1};" + mkp("{getPrototypeOf(){throw new RangeError('boom')}}"), "P instanceof Object");
addInv("var t={a:1};" + mkp("{deleteProperty(){throw new RangeError('boom')}}"), "delete P.a");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "P.a");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "Object.keys(P)");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "typeof P");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "Array.isArray(P)");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "JSON.stringify(P)");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "({...P})");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "'a' in P");
addInv("var r=Proxy.revocable({a:1},{});r.revoke();var P=r.proxy;", "Object.prototype.toString.call(P)");
addInv("var r=Proxy.revocable([1],{});r.revoke();var P=r.proxy;", "Array.isArray(P)");
addInv("var r=Proxy.revocable(function(){},{});r.revoke();var P=r.proxy;", "typeof P");
addInv("var r=Proxy.revocable(function(){},{});r.revoke();var P=r.proxy;", "P()");
addInv("var r=Proxy.revocable({},{get(){r.revoke();return 1}});var P=r.proxy;", "[P.a,P.b]");
addInv("var r=Proxy.revocable({a:1,b:2},{ownKeys(t){r.revoke();return Reflect.ownKeys(t)}});var P=r.proxy;", "Object.keys(P)");
addInv("var t={a:1};var P=new Proxy(new Proxy(t,{}),{});", "Object.keys(P)");
addInv("var t=[1,2];var P=new Proxy(new Proxy(t,{}),{});", "Array.isArray(P)");
addInv("var L2=[];var t={a:1};var P=new Proxy(new Proxy(t,{get(t,k,r){L2.push('in:'+String(k));return Reflect.get(t,k,r)}}),{get(t,k,r){L2.push('out:'+String(k));return Reflect.get(t,k,r)}});", "[P.a,L2.join()]");
addInv("var t={a:1};var P=new Proxy(t,{get(t,k,r){return r===P}});", "[P.a,Object.create(P).a]");
addInv("var t={};var P=new Proxy(t,{set(t,k,v,r){return Reflect.set(t,k,v,r)}});", "(function(){P.x=1;return Object.keys(t)})()");
addInv("var t={};var P=new Proxy(t,{set(t,k,v,r){return Reflect.set(t,k,v,r)},defineProperty(t,k,d){L.push('def:'+k);return Reflect.defineProperty(t,k,d)}});", "(function(){P.x=1;return L.join()})()");
addInv("var t={a:1};var P=new Proxy(t,{ownKeys(){L.push('ok');return ['a','b']},getOwnPropertyDescriptor(t,k){L.push('gopd:'+k);return k==='a'?Reflect.getOwnPropertyDescriptor(t,k):{value:2,enumerable:true,configurable:true}}});", "[Object.keys(P),L.join()]");
addInv("var t={a:1};var P=new Proxy(t,{ownKeys(){return ['b','a']}});", "JSON.stringify(P)");
addInv("var t={a:1};var P=new Proxy(t,{ownKeys(){return ['b','a']}});", "(function(){var k=[];for(var x in P)k.push(x);return k})()");
addInv("var t={a:1};var P=new Proxy(t,{ownKeys(){return ['a',Symbol.iterator]}});", "Reflect.ownKeys(P).length");
addInv("var t={a:1,b:2};var P=new Proxy(t,{deleteProperty(t,k){L.push('del:'+k);return false}});", "(function(){'use strict';delete P.a})()");
addInv("var t={a:1,b:2};var P=new Proxy(t,{deleteProperty(t,k){L.push('del:'+k);return false}});", "(function(){return [delete P.a,L.join()]})()");
addInv("var t=[1,2,3];var P=new Proxy(t,{deleteProperty(){return false}});", "(function(){'use strict';P.pop()})()");
addInv("var t=[1,2,3];var P=new Proxy(t,{set(){return false}});", "(function(){'use strict';P.push(4)})()");
addInv("var t=[1,2,3];var P=new Proxy(t,{set(){return false}});", "P.push(4)");
addInv("var t=[3,1,2];var P=new Proxy(t,{set(){return false}});", "P.sort()");
addInv("var t=Object.freeze([1,2]);var P=new Proxy(t,{});", "P.push(3)");
addInv("var t=Object.freeze([1,2]);var P=new Proxy(t,{});", "P.pop()");
addInv("var t=Object.freeze([1,2]);var P=new Proxy(t,{});", "P.reverse()");
addInv("var t=Object.freeze({a:1});var P=new Proxy(t,{});", "(function(){'use strict';P.a=2})()");
addInv("var t=Object.freeze({a:1});var P=new Proxy(t,{});", "(function(){'use strict';delete P.a})()");
addInv("var t=Object.freeze({a:1});var P=new Proxy(t,{});", "(function(){'use strict';P.b=2})()");
addInv("var t=Object.freeze({a:1});var P=new Proxy(t,{});", "Object.isFrozen(P)");
addInv("var t={};var P=new Proxy(t,{get(t,k){return k===Symbol.toPrimitive?function(){return {}}:undefined}});", "P+1");
addInv("var t={};var P=new Proxy(t,{get(t,k){return k===Symbol.toPrimitive?1:undefined}});", "P+1");
addInv("var t={};var P=new Proxy(t,{get(t,k){return k==='toString'?function(){return {}}:k==='valueOf'?function(){return {}}:undefined}});", "P+1");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){String(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){try{P+1}catch(e){};return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){[].concat(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){JSON.stringify(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Array.from(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){try{[...P]}catch(e){};return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Object.prototype.toString.call(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Promise.resolve(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){new Map([[P,1]]);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){new Error('x',P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){RegExp.prototype.exec.call(/a/,P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Object.assign({},P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){new Date(P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Reflect.construct(function(){},[],P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Object.create(null,P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Object.defineProperty({},'a',P);return L.join()})()");
addInv("var t={};var P=new Proxy(t,{get(t,k){L.push(String(k));return undefined}});", "(function(){Object.defineProperty({},'a',new Proxy({value:1,enumerable:1},{get(t,k){L.push(String(k));return t[k]},has(t,k){L.push('has:'+String(k));return k in t}}));return L.join()})()");
for (const { setup, body } of inv) addProgram(`${PRELUDE}${setup}globalThis.R=run(function(){return ${body}});`);

const dups = new Set();
let n = 0;
for (const source of programs) {
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(source.slice(PRELUDE.length)).slice(0, 160) + " " + e + "\n");
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) { process.stderr.write("caminho no resultado\n"); continue; }
  n++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${n}\n`);
