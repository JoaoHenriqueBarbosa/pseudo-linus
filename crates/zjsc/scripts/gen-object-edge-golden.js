// Gera tests/golden/object_edge_bun.tsv: borda de Object.* e Reflect.*, medido no bun 1.4.2.
// Cobre defineProperty/defineProperties com descritores parciais e inválidos, freeze/seal/preventExtensions em
// arrays, typed arrays e funções, getOwnPropertyDescriptors, fromEntries, groupBy, setPrototypeOf com ciclos,
// __proto__, __defineGetter__/__lookupGetter__, Object.assign com getters e símbolos, Reflect.construct com
// newTarget e Reflect.ownKeys (ordem inteira, string, símbolo). Programas cuja expressão já aparece nos goldens
// object_model_bun, reflect_bun e reflection_bun são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-object-edge-golden.js > tests/golden/object_edge_bun.tsv
const fs = require("fs");
const { emitRow, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. defineProperty com descritores parciais e inválidos, sobre propriedade nova e existente.
const fields = [
  "{}", "{value:1}", "{value:undefined}", "{writable:true}", "{writable:false}", "{enumerable:true}", "{configurable:true}",
  "{get:undefined}", "{set:undefined}", "{get(){return 1}}", "{set(v){}}", "{get(){return 1},set(v){}}", "{value:1,writable:true}",
  "{value:1,enumerable:true,configurable:true,writable:true}", "{get:1}", "{set:1}", "{get:null}", "{get:{}}", "{get(){},value:1}",
  "{set(v){},writable:true}", "{get(){},writable:false}", "{get:undefined,set:undefined}", "{get:undefined,value:1}", "{value:1,get:undefined}",
  "{writable:undefined}", "{enumerable:1,configurable:''}", "{enumerable:'x'}", "{writable:0,value:2}", "{__proto__:{value:7}}",
];
const priors = [
  "", "o.p=0;", "Object.defineProperty(o,'p',{value:0});", "Object.defineProperty(o,'p',{value:0,configurable:true});",
  "Object.defineProperty(o,'p',{value:0,writable:true});", "Object.defineProperty(o,'p',{get(){return 0},configurable:true});",
  "Object.defineProperty(o,'p',{get(){return 0}});", "Object.defineProperty(o,'p',{value:0,enumerable:true});",
];
for (const prior of priors) for (const f of fields) {
  add(`T(()=>{var o={};${prior}Object.defineProperty(o,'p',${f});return D(o,'p')})`);
}
add(
  "T(()=>Object.defineProperty({},'p',undefined))", "T(()=>Object.defineProperty({},'p',null))", "T(()=>Object.defineProperty({},'p',1))",
  "T(()=>Object.defineProperty({},'p','s'))", "T(()=>Object.defineProperty({},'p',true))", "T(()=>Object.defineProperty({},'p',Symbol()))",
  "T(()=>Object.defineProperty({},'p',()=>1))", "T(()=>Object.defineProperty({},'p',[]))", "T(()=>Object.defineProperty(1,'p',{}))",
  "T(()=>Object.defineProperty(null,'p',{}))", "T(()=>Object.defineProperty(undefined,'p',{}))", "T(()=>Object.defineProperty('s','p',{}))",
  "T(()=>Object.defineProperty(Symbol(),'p',{}))", "T(()=>Object.defineProperty({}))", "T(()=>Object.defineProperty())",
  "T(()=>Object.defineProperty([],'length',{value:-1}))", "T(()=>Object.defineProperty([],'length',{value:1.5}))",
  "T(()=>Object.defineProperty([],'length',{value:'3'}).length)", "T(()=>Object.defineProperty([],'length',{value:2**32}))",
  "T(()=>Object.defineProperty([],'length',{value:{valueOf(){return 4}}}).length)", "T(()=>Object.defineProperty([1,2,3],'length',{writable:false}).push(4))",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});a.length=0;return S(a)})",
  "T(()=>{'use strict';var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});a.length=0;return S(a)})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});a.length=0;return S(a)})",
  "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{get(){return 9}});return S(a)+a.length})",
  "T(()=>{var a=[];Object.defineProperty(a,'5',{value:1});return a.length})", "T(()=>{var a=[];Object.defineProperty(a,'4294967295',{value:1});return a.length})",
  "T(()=>{var a=[];Object.defineProperty(a,'4294967294',{value:1,configurable:true});return a.length})",
  "T(()=>{var a=[1];Object.defineProperty(a,'length',{writable:false});Object.defineProperty(a,1,{value:2})})",
  "T(()=>Object.defineProperty(function f(a,b){},'length',{value:5}).length)", "T(()=>Object.defineProperty(function f(){},'name',{value:'z'}).name)",
  "T(()=>Object.defineProperty(function f(){},'prototype',{value:1}).prototype)", "T(()=>Object.defineProperty(class{},'prototype',{value:1}))",
  "T(()=>Object.defineProperty(class{static x=1},'x',{value:2}).x)", "T(()=>D(Object.defineProperty(()=>1,'length',{value:3}),'length'))",
  "T(()=>{var o={};Object.defineProperty(o,Symbol.iterator,{value:1});return D(o,Symbol.iterator)})",
  "T(()=>{var o={};Object.defineProperty(o,{toString(){return 'k'}},{value:1});return D(o,'k')})",
  "T(()=>{var o={};Object.defineProperty(o,{toString(){throw new RangeError('k')}},{value:1})})",
  "T(()=>{var o={};Object.defineProperty(o,'p',{get value(){throw new EvalError('d')}})})",
  "T(()=>{var log=[];var d=new Proxy({},{has(t,k){log.push('has '+String(k));return false},get(t,k){log.push('get '+String(k))}});Object.defineProperty({},'p',d);return log.join()})",
  "T(()=>{var log=[];var d=new Proxy({value:1,get(){}},{has(t,k){log.push('has '+k);return k in t},get(t,k){log.push('get '+k);return t[k]}});try{Object.defineProperty({},'p',d)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>{var o=Object.preventExtensions({});Object.defineProperty(o,'p',{value:1})})",
  "T(()=>{var o=Object.preventExtensions({a:1});Object.defineProperty(o,'a',{value:2});return o.a})",
  "T(()=>Object.defineProperty({},'p',{value:1})===undefined)", "T(()=>{var o={};return Object.defineProperty(o,'p',{})===o})",
  "T(()=>{var o={};Object.defineProperty(o,'p',{value:1});o.p=2;return o.p})", "T(()=>{'use strict';var o={};Object.defineProperty(o,'p',{value:1});o.p=2;return o.p})",
  "T(()=>{'use strict';var o={};Object.defineProperty(o,'p',{get(){return 1}});o.p=2})",
  "T(()=>{'use strict';var o={};Object.defineProperty(o,'p',{get(){return 1}});delete o.p})",
  "T(()=>{var o={p:1};Object.defineProperty(o,'p',{get(){return 5}});return D(o,'p')})",
  "T(()=>{var o={get p(){return 1}};Object.defineProperty(o,'p',{value:5});return D(o,'p')})",
  "T(()=>{var o={get p(){return 1}};Object.defineProperty(o,'p',{set(v){}});return D(o,'p')})",
  "T(()=>{var o={};Object.defineProperty(o,'p',{value:NaN});Object.defineProperty(o,'p',{value:NaN});return 'ok'})",
  "T(()=>{var o={};Object.defineProperty(o,'p',{value:0});Object.defineProperty(o,'p',{value:-0})})",
  "T(()=>{var o={};Object.defineProperty(o,'p',{value:0,writable:true});Object.defineProperty(o,'p',{value:-0});return 1/o.p})",
  "T(()=>{var o={};Object.defineProperty(o,'p',{value:{}});Object.defineProperty(o,'p',{value:{}})})",
);

// ---- 2. defineProperties.
add(
  "T(()=>Object.defineProperties({},{a:{value:1},b:{value:2,enumerable:true}}))", "T(()=>D(Object.defineProperties({},{a:{value:1}}),'a'))",
  "T(()=>Object.defineProperties({},undefined))", "T(()=>Object.defineProperties({},null))", "T(()=>Object.defineProperties({},1))",
  "T(()=>Object.defineProperties({},'ab'))", "T(()=>Object.defineProperties({}))", "T(()=>Object.defineProperties(1,{}))", "T(()=>Object.defineProperties({},{a:1}))",
  "T(()=>Object.defineProperties({},{a:{value:1},b:2}))", "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:2})}catch(e){}return Object.getOwnPropertyNames(o).join()})",
  "T(()=>{var o={};try{Object.defineProperties(o,{a:{value:1},b:{get:1}})}catch(e){}return Object.getOwnPropertyNames(o).join()})",
  "T(()=>{var o={};Object.defineProperties(o,{[Symbol.for('s')]:{value:1,enumerable:true},z:{value:2}});return Reflect.ownKeys(o).map(String).join()})",
  "T(()=>{var p={a:{value:1,enumerable:true}};Object.defineProperty(p,'hidden',{value:{value:9},enumerable:false});return Object.getOwnPropertyNames(Object.defineProperties({},p)).join()})",
  "T(()=>{var p=Object.create({inh:{value:1}});p.own={value:2};return Object.getOwnPropertyNames(Object.defineProperties({},p)).join()})",
  "T(()=>{var log=[];var p=new Proxy({a:{value:1},b:{value:2}},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+k);return t[k]}});Object.defineProperties({},p);return log.join()})",
  "T(()=>Object.defineProperties([],{length:{value:3}}).length)", "T(()=>Object.defineProperties([1,2],{0:{value:'x'},2:{value:'y',enumerable:true}}).length)",
  "T(()=>{var o={};Object.defineProperties(o,{a:{get(){return 1},configurable:true}});Object.defineProperties(o,{a:{value:2}});return D(o,'a')})",
  "T(()=>Object.create({},{a:{value:1}}).a)", "T(()=>Object.create(null,{a:{value:1,enumerable:true}}).a)", "T(()=>Object.create(1))", "T(()=>Object.create())",
  "T(()=>Object.create(null,null))", "T(()=>Object.create({},undefined)===undefined)", "T(()=>Object.create({},{a:1}))",
  "T(()=>Object.getPrototypeOf(Object.create(Array.prototype,{x:{value:1}}))===Array.prototype)", "T(()=>Object.create(function(){}))",
  "T(()=>Object.create(Object.prototype)===Object.prototype)",
);

// ---- 3. freeze / seal / preventExtensions em vários alvos.
const targets = {
  array: "[1,2,3]", sparse: "[1,,3]", empty: "[]", obj: "{a:1,get g(){return 2},set g(v){}}", fn: "function f(a){}", arrow: "()=>1",
  cls: "class C{static s=1}", u8: "new Uint8Array([1,2,3])", u8empty: "new Uint8Array(0)", f64: "new Float64Array(2)", bigint64: "new BigInt64Array(1)",
  sym: "{[Symbol.iterator]:1}", map: "new Map([[1,2]])", date: "new Date(0)", re: "/x/g", err: "new Error('m')", args: "(function(){return arguments})(1,2)",
  str: "new String('ab')", boxed: "Object(1)", proxy: "new Proxy({a:1},{})", ab: "new ArrayBuffer(4)", dv: "new DataView(new ArrayBuffer(2))",
};
for (const [name, t] of Object.entries(targets)) {
  for (const op of ["freeze", "seal", "preventExtensions"]) {
    add(
      `T(()=>{var x=${t};Object[${JSON.stringify(op)}](x);return [Object.isFrozen(x),Object.isSealed(x),Object.isExtensible(x)].join()})`,
      `T(()=>{var x=Object.${op}(${t});return Reflect.ownKeys(x).map(k=>String(k)+":"+D(x,k)).join(" | ")})`,
    );
  }
  add(
    `T(()=>{'use strict';var x=Object.freeze(${t});x.zz=1;return 1})`, `T(()=>{'use strict';var x=Object.freeze(${t});for(var k of Reflect.ownKeys(x)){try{x[k]=0}catch(e){return k.toString()+" "+e.message}}return "all-ok"})`,
    `T(()=>{'use strict';var x=Object.freeze(${t});for(var k of Reflect.ownKeys(x)){try{delete x[k]}catch(e){return k.toString()+" "+e.message}}return "all-ok"})`,
    `T(()=>{'use strict';var x=Object.seal(${t});x.zz=1;return 1})`, `T(()=>{'use strict';var x=Object.seal(${t});for(var k of Reflect.ownKeys(x)){try{delete x[k]}catch(e){return k.toString()+" "+e.message}}return "all-ok"})`,
    `T(()=>{var x=Object.preventExtensions(${t});return Reflect.defineProperty(x,"zz",{value:1})+","+Reflect.set(x,"zz",1)+","+Reflect.isExtensible(x)})`,
    `T(()=>{var x=Object.freeze(${t});return [Reflect.set(x,"0",9),Reflect.deleteProperty(x,"0"),Reflect.defineProperty(x,"0",{value:1})].join()})`,
  );
}
add(
  "T(()=>Object.freeze(1))", "T(()=>Object.freeze('s'))", "T(()=>Object.freeze(null))", "T(()=>Object.freeze(undefined))", "T(()=>Object.freeze())", "T(()=>Object.freeze(Symbol())===undefined)",
  "T(()=>Object.seal(1))", "T(()=>Object.seal(null))", "T(()=>Object.preventExtensions(1))", "T(()=>Object.preventExtensions(undefined))",
  "T(()=>Object.isFrozen(1))", "T(()=>Object.isFrozen('s'))", "T(()=>Object.isFrozen(null))", "T(()=>Object.isSealed(undefined))", "T(()=>Object.isExtensible(1))", "T(()=>Object.isExtensible(null))",
  "T(()=>Object.isFrozen({}))", "T(()=>Object.isFrozen(Object.preventExtensions({})))", "T(()=>Object.isSealed(Object.preventExtensions({})))",
  "T(()=>Object.isFrozen(Object.preventExtensions({a:1})))", "T(()=>Object.isFrozen(Object.seal({a:1})))", "T(()=>Object.isFrozen(Object.seal({get a(){return 1}})))",
  "T(()=>Object.isFrozen(Object.preventExtensions(new Uint8Array(0))))", "T(()=>Object.isFrozen(Object.preventExtensions(new Uint8Array(1))))",
  "T(()=>Object.isSealed(Object.preventExtensions(new Uint8Array(1))))", "T(()=>Object.isSealed(Object.seal(new Uint8Array(1))))",
  "T(()=>Object.freeze(new Float32Array(1)))", "T(()=>Object.freeze(new Uint8Array(new ArrayBuffer(8,{maxByteLength:16}))))",
  "T(()=>Object.seal(new Uint8Array(new ArrayBuffer(8,{maxByteLength:16}))).length)", "T(()=>Object.preventExtensions(new Uint8Array(new ArrayBuffer(8,{maxByteLength:16}))).length)",
  "T(()=>{var a=Object.freeze([1,2]);return a.push(3)})", "T(()=>{var a=Object.freeze([1,2]);return a.pop()})", "T(()=>{var a=Object.freeze([1,2]);return a.shift()})",
  "T(()=>{var a=Object.freeze([1,2]);return a.unshift(0)})", "T(()=>{var a=Object.freeze([1,2]);return a.splice(0,1)})", "T(()=>{var a=Object.freeze([2,1]);return a.sort()})",
  "T(()=>{var a=Object.freeze([1,2]);return a.reverse()})", "T(()=>{var a=Object.freeze([1,2]);return a.fill(0)})", "T(()=>{var a=Object.freeze([1,2]);return a.copyWithin(0,1)})",
  "T(()=>{var a=Object.freeze([1,2]);a.length=0;return a.length})", "T(()=>{'use strict';var a=Object.freeze([1,2]);a.length=0})", "T(()=>{'use strict';var a=Object.freeze([1,2]);a[5]=0})",
  "T(()=>{var a=Object.freeze([1,2]);return S(a.concat(3))})", "T(()=>{var a=Object.freeze([3,1,2]);return S(a.toSorted())+S(a.toReversed())+S(a.with(0,9))})",
  "T(()=>{var a=Object.seal([1,2]);a.push(3)})", "T(()=>{var a=Object.seal([1,2]);return a.pop()})", "T(()=>{var a=Object.seal([1,2]);a[0]=7;return S(a)})",
  "T(()=>{var a=Object.preventExtensions([1,2]);a.push(3)})", "T(()=>{var a=Object.preventExtensions([1,2]);return a.pop()+S(a)})", "T(()=>{var a=Object.preventExtensions([1,2]);a.length=5;return a.length})",
  "T(()=>{var a=Object.preventExtensions([1,2]);a.unshift(0)})", "T(()=>{var a=Object.preventExtensions([1,2,3]);a.splice(1,1);return S(a)})", "T(()=>{var a=Object.preventExtensions([1,,3]);a[1]=2})",
  "T(()=>{var u=Object.freeze(new Uint8Array(2));u[0]=5;return u[0]})", "T(()=>{'use strict';var u=Object.seal(new Uint8Array(2));u[0]=5;return u[0]})",
  "T(()=>{var u=Object.preventExtensions(new Uint8Array(2));u[0]=5;u[3]=1;return S(u)+Object.keys(u).join()})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'0',{value:9});return u[0]})",
  "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'0',{value:9,writable:false})})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'0',{get(){}})})",
  "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'5',{value:9})})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'0',{value:9,configurable:false})})",
  "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'-0',{value:9})})", "T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,'1.5',{value:9});return Object.keys(u).join()})",
  "T(()=>{var u=new Uint8Array(2);return Reflect.defineProperty(u,'0',{value:3,writable:true,enumerable:true,configurable:true})+S(u)})",
  "T(()=>{var f=Object.freeze(function(){});f.x=1;return f.x})", "T(()=>{var f=Object.freeze(function(){});return Reflect.ownKeys(f).map(k=>String(k)+D(f,k)).join('|')})",
  "T(()=>{var f=Object.freeze(function(){});f.prototype.y=1;return f.prototype.y})", "T(()=>{'use strict';var f=Object.freeze(function(){});f.prototype=1})",
  "T(()=>{'use strict';var f=Object.freeze(function(){});f.name='x'})", "T(()=>{var f=Object.freeze(()=>1);return Object.isFrozen(f)+D(f,'length')})",
  "T(()=>{class A{static x=1}Object.freeze(A);A.x=2;return A.x})", "T(()=>{'use strict';class A{static x=1}Object.freeze(A);A.x=2})", "T(()=>{class A{}Object.freeze(A);A.prototype.m=1;return A.prototype.m})",
  "T(()=>{var o=Object.freeze({a:{b:1}});o.a.b=2;return o.a.b})", "T(()=>{var o=Object.freeze(Object.create({inh:1}));o.inh=2;return Object.keys(o).length+o.inh})",
  "T(()=>{'use strict';var o=Object.freeze(Object.create({set inh(v){this._=v}}));o.inh=2;return o._})",
  "T(()=>{'use strict';var o=Object.freeze(Object.create({inh:1}));o.inh=2})", "T(()=>{'use strict';var o=Object.preventExtensions(Object.create({inh:1}));o.inh=2})",
  "T(()=>{var p=Object.freeze({x:1});var o=Object.create(p);o.x=2;return o.x+Object.keys(o).join()})", "T(()=>{var p=Object.freeze({x:1});var o=Object.create(p);Object.defineProperty(o,'x',{value:2});return o.x})",
  "T(()=>{var o=Object.freeze({});Object.setPrototypeOf(o,Object.prototype)===o;return 1})", "T(()=>{var o=Object.freeze({});Object.setPrototypeOf(o,null)})",
  "T(()=>{var o=Object.preventExtensions({});Object.setPrototypeOf(o,Array.prototype)})", "T(()=>{var o=Object.preventExtensions({});return Object.setPrototypeOf(o,Object.prototype)===o})",
  "T(()=>{var o=Object.preventExtensions({});o.__proto__=null;return 1})", "T(()=>{var o=Object.preventExtensions({});o.__proto__=Object.prototype;return 1})",
  "T(()=>Reflect.setPrototypeOf(Object.freeze({}),null))", "T(()=>Reflect.setPrototypeOf(Object.freeze({}),Object.prototype))", "T(()=>Reflect.setPrototypeOf(Object.preventExtensions({}),{}))",
);

// ---- 4. getOwnPropertyDescriptor(s), getOwnPropertyNames, getOwnPropertySymbols, keys/values/entries.
const gopdSubjects = [
  "{a:1,get b(){return 2},set b(v){},[Symbol.for('s')]:3}", "[7,,9]", "'str'", "new String('ab')", "function f(a,b){}", "class C{static m(){}get g(){return 1}}", "new Uint8Array(2)",
  "(function(){return arguments})(1,2)", "Object.create({inh:1})", "Object.create(null,{x:{value:1}})", "new Proxy({a:1},{})", "Math", "globalThis.JSON", "Symbol", "[].concat", "new Error('m')",
  "Object.freeze({a:1})", "Object.seal([1])", "/x/g", "new Map", "1", "true", "Symbol('q')", "10n",
];
for (const s of gopdSubjects) {
  add(
    `T(()=>S(Object.getOwnPropertyDescriptors(${s})))`,
    `T(()=>{var x=${s};return Object.keys(Object.getOwnPropertyDescriptors(x)).join()===Object.getOwnPropertyNames(x).filter(k=>Object.getOwnPropertyDescriptor(x,k)).join()})`,
    `T(()=>Object.getOwnPropertyNames(${s}).join())`, `T(()=>Object.getOwnPropertySymbols(${s}).map(String).join())`,
    `T(()=>Object.keys(${s}).join())`, `T(()=>S(Object.values(${s})))`, `T(()=>S(Object.entries(${s})))`,
    `T(()=>Reflect.ownKeys(Object(${s})).map(String).join())`,
  );
}
add(
  "T(()=>Object.getOwnPropertyDescriptors(null))", "T(()=>Object.getOwnPropertyDescriptors(undefined))", "T(()=>Object.getOwnPropertyDescriptors())",
  "T(()=>Object.getOwnPropertyNames(null))", "T(()=>Object.getOwnPropertySymbols(undefined))", "T(()=>Object.keys(null))", "T(()=>Object.values(undefined))", "T(()=>Object.entries(null))",
  "T(()=>{var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});Object.getOwnPropertyDescriptors(p);return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+k);return t[k]}});Object.entries(p);return log.join()})",
  "T(()=>{var p=new Proxy({a:1},{getOwnPropertyDescriptor(){return undefined}});return S(Object.getOwnPropertyDescriptors(p))+Object.keys(p).length})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return ['a','a']}});return Object.keys(p)})", "T(()=>{var p=new Proxy({},{ownKeys(){return [1]}});return Object.keys(p)})",
  "T(()=>{var p=new Proxy(Object.freeze({a:1}),{ownKeys(){return []}});return Object.keys(p)})",
  "T(()=>{var o={a:1,b:2};var r=[];for(var [k,v] of Object.entries(o)){r.push(k+v);delete o.b}return r.join()})",
  "T(()=>{var o={get a(){delete this.b;return 1},b:2};return S(Object.values(o))})", "T(()=>{var o={get a(){this.c=3;return 1},b:2};return S(Object.entries(o))})",
  "T(()=>{var o={a:1};Object.defineProperty(o,'h',{value:2});o[Symbol()]=3;return S(Object.entries(o))})",
  "T(()=>S(Object.entries('ab')))", "T(()=>S(Object.values([5,,6])))", "T(()=>S(Object.entries(new Uint8Array([4,5]))))",
);

// ---- 5. Reflect.ownKeys e a ordem inteira/string/símbolo.
const orderSets = [
  "{b:1,a:2,1:3,0:4}", "{'-1':1,'1.5':2,'01':3,'1':4,'4294967294':5,'4294967295':6,'4294967296':7}", "{[Symbol('s')]:1,x:2,3:3,[Symbol.iterator]:4,1:5}",
  "{z:1,[Symbol.for('a')]:1,y:2,[Symbol.for('b')]:2,10:1,9:1}", "{'2':1,'1':1,a:1,'0':1,length:1}", "{['__proto__']:1,a:2}", "{'':1,' ':2,'0':3}",
  "{9007199254740991:1,9007199254740992:2,1e21:3,1e-7:4}", "{'0x10':1,'16':2,'1e3':3,'1000':4,'Infinity':5,'NaN':6,'-0':7,'0':8}",
  "{[Symbol('b')]:1,[Symbol('a')]:1,[Symbol.toPrimitive]:1}", "{'4294967295':1,'4294967294':2,'2147483648':3,'2147483647':4}",
];
for (const s of orderSets) {
  add(
    `T(()=>Reflect.ownKeys(${s}).map(String).join())`, `T(()=>Object.keys(${s}).join())`, `T(()=>Object.getOwnPropertyNames(${s}).join())`,
    `T(()=>{var r=[];for(var k in ${s})r.push(k);return r.join()})`, `T(()=>JSON.stringify(${s}))`, `T(()=>S(Object.assign({},${s})))`, `T(()=>S({...${s}}))`,
    `T(()=>Reflect.ownKeys(Object.freeze(${s})).map(String).join())`, `T(()=>Reflect.ownKeys(Object.defineProperties({},Object.getOwnPropertyDescriptors(${s}))).map(String).join())`,
    `T(()=>S(Object.fromEntries(Reflect.ownKeys(${s}).map(k=>[k,1]))))`,
  );
}
add(
  "T(()=>{var o={};o.b=1;o[2]=1;o.a=1;o[1]=1;o[Symbol.for('x')]=1;o[0]=1;delete o.b;o.b=2;return Reflect.ownKeys(o).map(String).join()})",
  "T(()=>{var o={};o[5]=1;o[3]=1;delete o[5];o[5]=1;o[4]=1;return Reflect.ownKeys(o).join()})",
  "T(()=>{var a=[1,2];a.x=1;a[Symbol.for('s')]=2;a[5]=3;return Reflect.ownKeys(a).map(String).join()})", "T(()=>Reflect.ownKeys([]).join())",
  "T(()=>Reflect.ownKeys(function f(a){}).join())", "T(()=>Reflect.ownKeys(()=>1).join())", "T(()=>Reflect.ownKeys(class{static a(){}}).join())", "T(()=>Reflect.ownKeys(class{static a(){}static[Symbol.iterator](){}}).map(String).join())",
  "T(()=>Reflect.ownKeys('ab').join())", "T(()=>Reflect.ownKeys(new String('ab')).join())", "T(()=>Reflect.ownKeys(new Uint8Array(3)).join())", "T(()=>{var u=new Uint8Array(2);u.x=1;u[Symbol.for('q')]=1;return Reflect.ownKeys(u).map(String).join()})",
  "T(()=>Reflect.ownKeys(/x/).join())", "T(()=>Reflect.ownKeys(new Error('m')).join())", "T(()=>Reflect.ownKeys(new Error('m',{cause:1})).join())", "T(()=>Reflect.ownKeys((function(){return arguments})(1,2)).map(String).join())",
  "T(()=>Reflect.ownKeys((function(){'use strict';return arguments})(1)).map(String).join())", "T(()=>Reflect.ownKeys(Symbol.prototype).map(String).join())",
  "T(()=>Reflect.ownKeys(Reflect).map(String).join())", "T(()=>Reflect.ownKeys(Object).join())", "T(()=>Reflect.ownKeys(Object.prototype).join())", "T(()=>Reflect.ownKeys(Function.prototype).map(String).join())",
  "T(()=>Reflect.ownKeys(1))", "T(()=>Reflect.ownKeys(null))", "T(()=>Reflect.ownKeys('s'))", "T(()=>Reflect.ownKeys())", "T(()=>Reflect.ownKeys(Symbol()))",
  "T(()=>{var p=new Proxy({},{ownKeys(){return ['b',Symbol.for('s'),'a','1']}});return Reflect.ownKeys(p).map(String).join()})", "T(()=>{var p=new Proxy({},{ownKeys(){return {length:2,0:'a',1:'b'}}});return Reflect.ownKeys(p).join()})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return ['a',1]}});return Reflect.ownKeys(p)})", "T(()=>{var p=new Proxy({},{ownKeys(){return ['a','a']}});return Reflect.ownKeys(p)})",
  "T(()=>{var p=new Proxy({},{ownKeys(){return undefined}});return Reflect.ownKeys(p)})", "T(()=>{var p=new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return ['a','b']}});return Reflect.ownKeys(p)})",
  "T(()=>{var p=new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return []}});return Reflect.ownKeys(p)})", "T(()=>{var t={};Object.defineProperty(t,'a',{value:1});var p=new Proxy(t,{ownKeys(){return []}});return Reflect.ownKeys(p)})",
  "T(()=>{var o={};o[1e3]=1;o[999]=1;o['1000']=2;return Reflect.ownKeys(o).join()})", "T(()=>{var o={b:1,a:1};Object.defineProperty(o,'0',{value:1,enumerable:false});return Reflect.ownKeys(o).join()+'|'+Object.keys(o).join()})",
  "T(()=>{class A{constructor(){this.z=1;this[1]=1;this.a=1}}return Reflect.ownKeys(new A).join()})", "T(()=>{class A{x=1;1=2;static y=3;}return Reflect.ownKeys(new A).join()+Reflect.ownKeys(A).join()})",
);

// ---- 6. fromEntries e groupBy.
add(
  "T(()=>Object.fromEntries([['a',1],['b',2]]))", "T(()=>Object.fromEntries([]))", "T(()=>Object.fromEntries(new Map([[1,'x'],['1','y']])))", "T(()=>Object.fromEntries(new Map([[{},1]])))",
  "T(()=>Object.fromEntries())", "T(()=>Object.fromEntries(null))", "T(()=>Object.fromEntries(undefined))", "T(()=>Object.fromEntries(1))", "T(()=>Object.fromEntries('ab'))", "T(()=>Object.fromEntries({}))",
  "T(()=>Object.fromEntries([1]))", "T(()=>Object.fromEntries(['ab']))", "T(()=>Object.fromEntries([[]]))", "T(()=>Object.fromEntries([['a']]))", "T(()=>Object.fromEntries([null]))", "T(()=>Object.fromEntries([undefined]))",
  "T(()=>Object.fromEntries([{0:'k',1:'v'}]))", "T(()=>Object.fromEntries([[Symbol.for('s'),1]]))", "T(()=>Object.fromEntries([[1,1],['01',2],[1.0,3]]))", "T(()=>Object.fromEntries([['__proto__',1]]))",
  "T(()=>Object.getPrototypeOf(Object.fromEntries([['__proto__',{x:1}]]))===Object.prototype)", "T(()=>Object.fromEntries([['a',1],['a',2]]))", "T(()=>Object.fromEntries([[null,1],[undefined,2],[true,3]]))",
  "T(()=>Object.fromEntries([[{toString(){return 'k'}},1]]))", "T(()=>Object.fromEntries([[{toString(){throw new RangeError('ts')}},1]]))", "T(()=>Object.fromEntries([[-0,1]]))",
  "T(()=>Object.fromEntries({[Symbol.iterator]:function*(){yield['x',1];yield['y',2]}}))", "T(()=>Object.fromEntries((function*(){yield['a',1];throw new EvalError('g')})()))",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return{next(){log.push('next');return{done:log.length>2,value:['k',1]}},return(){log.push('return');return{}}}}};Object.fromEntries(it);return log.join()})",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return{next(){log.push('next');return{done:false,value:1}},return(){log.push('return');return{}}}}};try{Object.fromEntries(it)}catch(e){log.push(e.name)}return log.join()})",
  "T(()=>S(Object.fromEntries(Object.entries({a:1,b:[2]}))))", "T(()=>Object.fromEntries(new URLSearchParams('a=1&b=2')).b)", "T(()=>S(Object.fromEntries(new Set([['a',1]]))))",
  "T(()=>Object.fromEntries.length+Object.fromEntries.name)", "T(()=>Object.groupBy.length+Object.groupBy.name)", "T(()=>Object.fromEntries({}.x))",
  "T(()=>S(Object.groupBy([1,2,3,4],x=>x%2?'odd':'even')))", "T(()=>Object.getPrototypeOf(Object.groupBy([1],x=>x)))", "T(()=>S(Object.groupBy([],x=>x)))", "T(()=>S(Object.groupBy('abca',c=>c)))",
  "T(()=>S(Object.groupBy([1,2,3],(x,i)=>i)))", "T(()=>S(Object.groupBy([1,2,3],x=>Symbol.for('s'))))", "T(()=>S(Object.groupBy([1,2],x=>({toString(){return 'k'}}))))", "T(()=>S(Object.groupBy([1,2],x=>-0)))",
  "T(()=>S(Object.groupBy([1,2],x=>null)))", "T(()=>S(Object.groupBy([1,2],x=>undefined)))", "T(()=>S(Object.groupBy([1,2],x=>x===1?'__proto__':'a')))", "T(()=>S(Object.groupBy([1,2,3],x=>x>1?10:2)))",
  "T(()=>S(Object.groupBy([1,2,3],x=>[x])))", "T(()=>S(Object.groupBy(new Set([1,2,2,3]),x=>x%2)))", "T(()=>S(Object.groupBy(new Map([[1,2]]),([k,v])=>k)))",
  "T(()=>Object.groupBy())", "T(()=>Object.groupBy(null,x=>x))", "T(()=>Object.groupBy([1]))", "T(()=>Object.groupBy([1],1))", "T(()=>Object.groupBy(1,x=>x))", "T(()=>Object.groupBy({},x=>x))",
  "T(()=>Object.groupBy([1],()=>{throw new RangeError('cb')}))", "T(()=>S(Object.groupBy({length:2,0:'a',1:'b'},x=>x)))",
  "T(()=>{var log=[];Object.groupBy([1,2],function(x,i){log.push(this===undefined?'undef':typeof this);return x});return log.join()})",
  "T(()=>{var log=[];var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},return(){log.push('return');return{}}}}};try{Object.groupBy(it,()=>{throw 1})}catch(e){log.push('c')}return log.join()})",
  "T(()=>S(Map.groupBy([1,2,3],x=>x%2)))", "T(()=>Map.groupBy([0,-0],x=>x).size)", "T(()=>S(Map.groupBy([1,2],x=>({})).size))", "T(()=>Map.groupBy.length+Map.groupBy.name)",
  "T(()=>S(Map.groupBy([NaN,NaN],x=>x)))", "T(()=>Map.groupBy())", "T(()=>Map.groupBy([1]))",
);

// ---- 7. setPrototypeOf, __proto__, getPrototypeOf e ciclos.
add(
  "T(()=>{var a={},b={};Object.setPrototypeOf(a,b);Object.setPrototypeOf(b,a)})", "T(()=>{var a={};Object.setPrototypeOf(a,a)})", "T(()=>{var a={},b=Object.create(a),c=Object.create(b);Object.setPrototypeOf(a,c)})",
  "T(()=>{var a={},b={};a.__proto__=b;b.__proto__=a})", "T(()=>{var a={};a.__proto__=a})", "T(()=>{var a={},b={};Reflect.setPrototypeOf(a,b);return Reflect.setPrototypeOf(b,a)})",
  "T(()=>{var a={};return Reflect.setPrototypeOf(a,a)})", "T(()=>{var a={},b=Object.create(a);return Reflect.setPrototypeOf(a,b)})",
  "T(()=>{var p=new Proxy({},{});var a=Object.create(p);return Reflect.setPrototypeOf(p,a)})", "T(()=>{var p=new Proxy({},{getPrototypeOf(){return null}});var a={};Object.setPrototypeOf(a,p);return Object.setPrototypeOf(p,a)===p})",
  "T(()=>Object.setPrototypeOf({},null)===undefined)", "T(()=>Object.getPrototypeOf(Object.setPrototypeOf({},null)))", "T(()=>Object.setPrototypeOf({},1))", "T(()=>Object.setPrototypeOf({},undefined))",
  "T(()=>Object.setPrototypeOf({}))", "T(()=>Object.setPrototypeOf())", "T(()=>Object.setPrototypeOf(null,{}))", "T(()=>Object.setPrototypeOf(undefined,{}))", "T(()=>Object.setPrototypeOf(1,{}))",
  "T(()=>Object.setPrototypeOf(1,null))", "T(()=>Object.setPrototypeOf('s',null))", "T(()=>Object.setPrototypeOf({},'s'))", "T(()=>Object.setPrototypeOf({},()=>1)!==undefined)", "T(()=>Object.setPrototypeOf({},Symbol()))",
  "T(()=>Object.getPrototypeOf(1)===Number.prototype)", "T(()=>Object.getPrototypeOf('s')===String.prototype)", "T(()=>Object.getPrototypeOf(Symbol())===Symbol.prototype)", "T(()=>Object.getPrototypeOf(1n)===BigInt.prototype)",
  "T(()=>Object.getPrototypeOf(null))", "T(()=>Object.getPrototypeOf(undefined))", "T(()=>Object.getPrototypeOf())", "T(()=>Object.getPrototypeOf(Object.prototype))",
  "T(()=>Object.getPrototypeOf(Function.prototype)===Object.prototype)", "T(()=>Object.getPrototypeOf(function*(){})===Object.getPrototypeOf(function*(){}))",
  "T(()=>Object.getPrototypeOf(async function(){}).constructor.name)", "T(()=>Object.getPrototypeOf(class A extends null{})===Function.prototype)", "T(()=>Object.getPrototypeOf(class A extends Array{})===Array)",
  "T(()=>Object.getPrototypeOf(new Proxy({},{getPrototypeOf(){return Array.prototype}}))===Array.prototype)", "T(()=>Object.getPrototypeOf(new Proxy({},{getPrototypeOf(){return 1}})))",
  "T(()=>Object.getPrototypeOf(new Proxy(Object.preventExtensions({}),{getPrototypeOf(){return null}})))", "T(()=>Object.getPrototypeOf(new Proxy({},{getPrototypeOf(){return undefined}})))",
  "T(()=>Reflect.getPrototypeOf(1))", "T(()=>Reflect.getPrototypeOf(null))", "T(()=>Reflect.getPrototypeOf({})===Object.prototype)", "T(()=>Reflect.setPrototypeOf(1,{}))", "T(()=>Reflect.setPrototypeOf({},1))",
  "T(()=>Reflect.setPrototypeOf({},undefined))", "T(()=>Reflect.setPrototypeOf({}))", "T(()=>Reflect.setPrototypeOf({},null))", "T(()=>Reflect.setPrototypeOf(Object.prototype,{}))", "T(()=>Reflect.setPrototypeOf(Object.prototype,null))",
  "T(()=>Object.setPrototypeOf(Object.prototype,{}))", "T(()=>Object.setPrototypeOf(Object.prototype,null)===Object.prototype)", "T(()=>Object.setPrototypeOf(Object.prototype,Object.prototype))",
  "T(()=>{var o={__proto__:null};return Object.getPrototypeOf(o)+'|'+('__proto__' in o)+'|'+o.__proto__})", "T(()=>{var o={__proto__:1};return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var o={__proto__:Array.prototype};return Array.isArray(o)+','+(o instanceof Array)})", "T(()=>{var o={'__proto__':null};return Object.getPrototypeOf(o)})", "T(()=>{var o={['__proto__']:null};return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var __proto__=null;var o={__proto__};return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={__proto__(){}};return typeof o.__proto__+(Object.getPrototypeOf(o)===Object.prototype)})",
  "T(()=>{var o={get __proto__(){return 1}};return o.__proto__+(Object.getPrototypeOf(o)===Object.prototype)})", "T(()=>{var o={__proto__:null,__proto__:null}})", "T(()=>JSON.parse('{\"__proto__\":null}').__proto__)",
  "T(()=>Object.getPrototypeOf(JSON.parse('{\"__proto__\":{\"x\":1}}'))===Object.prototype)", "T(()=>Object.keys(JSON.parse('{\"__proto__\":1}')).join())", "T(()=>D(Object.prototype,'__proto__'))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.call(1)===Number.prototype)", "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.call(null))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(null,{}))", "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(1,{}))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call({},1))", "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call({}))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.call(Object.freeze({}),null))", "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.length+Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.name)",
  "T(()=>{var o=Object.create(null);o.__proto__=5;return Object.keys(o).join()+o.__proto__})", "T(()=>{var o={};o.__proto__=5;return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{var o={};Object.defineProperty(o,'__proto__',{value:1,enumerable:true});return Object.keys(o).join()+o.__proto__+(Object.getPrototypeOf(o)===Object.prototype)})",
  "T(()=>{var o={};o.__proto__={x:1};return o.x+Object.keys(o).join()})", "T(()=>{var o={};o['__proto__']={x:1};return o.x})", "T(()=>{var k='__proto__';var o={};o[k]={x:2};return o.x})",
  "T(()=>{var o={};Object.prototype.__lookupGetter__.call(o,'__proto__').call(o)===Object.prototype})", "T(()=>{var o=Object.create({});return o.hasOwnProperty('__proto__')+','+('__proto__' in o)})",
  "T(()=>{var p={};var o=Object.create(p);return p.isPrototypeOf(o)+','+Object.prototype.isPrototypeOf(o)+','+o.isPrototypeOf(o)})", "T(()=>Object.prototype.isPrototypeOf.call(null,{}))",
  "T(()=>Object.prototype.isPrototypeOf.call(undefined,1))", "T(()=>Object.prototype.isPrototypeOf(1))", "T(()=>Object.prototype.isPrototypeOf.call(null,1))",
  "T(()=>{class A{}class B extends A{}return A.isPrototypeOf(B)+','+B.isPrototypeOf(A)+','+(Object.getPrototypeOf(B)===A)})", "T(()=>{class A{}class B extends A{}Object.setPrototypeOf(B.prototype,null);return new B instanceof A})",
  "T(()=>{function F(){}F.prototype=1;return ({} instanceof F)})", "T(()=>{function F(){}F.prototype=1;return Object.getPrototypeOf(new F)===Object.prototype})",
  "T(()=>{var o=Object.create(Array.prototype);o.length=2;return Array.isArray(o)+','+o.length+','+typeof o.map})",
);

// ---- 8. __defineGetter__, __defineSetter__, __lookupGetter__, __lookupSetter__.
const O = "Object.prototype";
add(
  "T(()=>{var o={};o.__defineGetter__('a',function(){return 1});return D(o,'a')})", "T(()=>{var o={};o.__defineSetter__('a',function(v){this._=v});o.a=5;return D(o,'a')+o._})",
  "T(()=>{var o={};o.__defineGetter__('a',()=>1);o.__defineSetter__('a',v=>{});return D(o,'a')})", "T(()=>{var o={a:1};o.__defineGetter__('a',()=>2);return D(o,'a')})",
  "T(()=>{var o={};o.__defineGetter__('a',1)})", "T(()=>{var o={};o.__defineGetter__('a')})", "T(()=>{var o={};o.__defineGetter__('a',null)})", "T(()=>{var o={};o.__defineGetter__('a',{})})",
  "T(()=>{var o={};o.__defineSetter__('a',1)})", "T(()=>{var o={};o.__defineSetter__('a',undefined)})", "T(()=>{var o={};o.__defineGetter__()})", "T(()=>{var o={};o.__defineGetter__(Symbol.for('s'),()=>1);return D(o,Symbol.for('s'))})",
  "T(()=>{var o={};o.__defineGetter__(1,()=>1);return D(o,'1')})", "T(()=>{var o={};o.__defineGetter__({toString(){return 'k'}},()=>1);return Object.keys(o).join()})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{value:1});o.__defineGetter__('a',()=>2)})", "T(()=>{var o={};Object.defineProperty(o,'a',{get(){},configurable:false});o.__defineGetter__('a',()=>2)})",
  "T(()=>{var o={};Object.defineProperty(o,'a',{get(){},configurable:true});o.__defineGetter__('a',()=>2);return D(o,'a')})",
  "T(()=>{var o=Object.preventExtensions({});o.__defineGetter__('a',()=>2)})", "T(()=>Object.freeze({}).__defineGetter__('a',()=>1))", "T(()=>Object.prototype.__defineGetter__.call(null,'a',()=>1))",
  "T(()=>Object.prototype.__defineGetter__.call(undefined,'a',()=>1))", "T(()=>Object.prototype.__defineGetter__.call(1,'a',()=>1))", "T(()=>Object.prototype.__defineGetter__.call('s','a',()=>1))",
  "T(()=>Object.prototype.__defineSetter__.call(null,'a',()=>1))", "T(()=>{var o={};return o.__defineGetter__('a',()=>1)})", "T(()=>{var a=[];a.__defineGetter__('length',()=>1)})",
  "T(()=>{var a=[];a.__defineGetter__(0,()=>5);return a.length+','+a[0]})", "T(()=>{var u=new Uint8Array(1);u.__defineGetter__(0,()=>5)})",
  "T(()=>{var f=function(){};f.__defineGetter__('x',()=>3);return f.x})", "T(()=>{var o={};o.__defineGetter__('a',()=>1);return Object.keys(o).join()+D(o,'a')})",
  "T(()=>{var p=new Proxy({},{defineProperty(t,k,d){return Reflect.defineProperty(t,k,Object.assign(d,{x:1}))}});p.__defineGetter__('a',()=>1);return D(p,'a')})",
  "T(()=>{var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push(k+':'+Object.keys(d).join('/'));return Reflect.defineProperty(t,k,d)}});p.__defineGetter__('a',()=>1);p.__defineSetter__('b',()=>1);return log.join()})",
  "T(()=>{var o={get a(){return 1}};return typeof o.__lookupGetter__('a')+typeof o.__lookupSetter__('a')})", "T(()=>{var o={set a(v){}};return typeof o.__lookupGetter__('a')+typeof o.__lookupSetter__('a')})",
  "T(()=>{var o={a:1};return o.__lookupGetter__('a')+','+o.__lookupSetter__('a')})", "T(()=>{var o={};return o.__lookupGetter__('zz')})", "T(()=>{var p={get a(){return 3}};var o=Object.create(p);return o.__lookupGetter__('a')===Object.getOwnPropertyDescriptor(p,'a').get})",
  "T(()=>{var p={get a(){return 3}};var o=Object.create(p);o.a=1;return typeof o.__lookupGetter__('a')})", "T(()=>{var p={get a(){return 3}};var o=Object.create(p);Object.defineProperty(o,'a',{value:1});return o.__lookupGetter__('a')})",
  "T(()=>{var o={};return o.__lookupGetter__()})", "T(()=>{var o={};return o.__lookupGetter__(Symbol.for('s'))})", "T(()=>{var s=Symbol.for('s');var o={get [s](){return 1}};return typeof o.__lookupGetter__(s)})",
  "T(()=>{var o={get 1(){return 1}};return typeof o.__lookupGetter__(1)})", "T(()=>{var o={get a(){return 1}};return typeof o.__lookupGetter__({toString(){return 'a'}})})",
  "T(()=>Object.prototype.__lookupGetter__.call(null,'a'))", "T(()=>Object.prototype.__lookupGetter__.call(undefined,'a'))", "T(()=>Object.prototype.__lookupGetter__.call(1,'a'))", "T(()=>Object.prototype.__lookupSetter__.call(null,'a'))",
  "T(()=>typeof Object.prototype.__lookupGetter__.call('s','length'))", "T(()=>typeof Object.prototype.__lookupGetter__.call(Object.prototype,'__proto__'))", "T(()=>typeof Object.prototype.__lookupSetter__.call({},'__proto__'))",
  "T(()=>{var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {get(){},configurable:true}},getPrototypeOf(){return null}});return typeof p.__lookupGetter__})",
  "T(()=>{var log=[];var p=new Proxy({},{getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return undefined},getPrototypeOf(t){log.push('gpo');return null}});Object.prototype.__lookupGetter__.call(p,'a');return log.join()})",
  "T(()=>Object.prototype.__defineGetter__.length+','+Object.prototype.__defineSetter__.length+','+Object.prototype.__lookupGetter__.length+','+Object.prototype.__lookupSetter__.length)",
  "T(()=>Object.getOwnPropertyNames(Object.prototype).join())", "T(()=>['__defineGetter__','__defineSetter__','__lookupGetter__','__lookupSetter__','__proto__'].map(k=>D(Object.prototype,k)).join('|'))",
  "T(()=>Object.getOwnPropertyDescriptor(Object.prototype,'__defineGetter__').enumerable)", "T(()=>(Object.create(null)).__defineGetter__)", "T(()=>{var o=Object.create(null);Object.prototype.__defineGetter__.call(o,'a',()=>9);return o.a})",
);

// ---- 9. Object.assign com getters, símbolos, fontes primitivas e alvos congelados.
add(
  "T(()=>S(Object.assign({},{get a(){return 1}})))", "T(()=>D(Object.assign({},{get a(){return 1}}),'a'))", "T(()=>{var n=0;var o=Object.assign({},{get a(){return ++n}});return o.a+o.a+n})",
  "T(()=>S(Object.assign({},{a:1},null,undefined,{b:2})))", "T(()=>S(Object.assign({},'ab')))", "T(()=>S(Object.assign({},'ab',1,true,Symbol())))", "T(()=>S(Object.assign({},[5,6])))", "T(()=>S(Object.assign([1,2,3],[9])))",
  "T(()=>S(Object.assign({},new String('xy'))))", "T(()=>S(Object.assign({}, 10n)))", "T(()=>Object.assign())", "T(()=>Object.assign(null))", "T(()=>Object.assign(undefined,{a:1}))", "T(()=>typeof Object.assign(1,{a:1}))",
  "T(()=>Object.assign(1,{a:1}).a)", "T(()=>typeof Object.assign('s'))", "T(()=>Object.assign(Symbol(),{a:1}).a)", "T(()=>Object.assign({}).constructor===Object)", "T(()=>Object.assign.length+Object.assign.name)",
  "T(()=>{var s=Symbol('s');var o=Object.assign({},{[s]:1,a:2});return Reflect.ownKeys(o).map(String).join()})", "T(()=>{var s=Symbol('s');var src={};Object.defineProperty(src,s,{value:1,enumerable:false});return Reflect.ownKeys(Object.assign({},src)).length})",
  "T(()=>{var s=Symbol('s');var src={get [s](){return 5}};var o=Object.assign({},src);return D(o,s)})", "T(()=>{var src={};Object.defineProperty(src,'h',{value:1});return Object.keys(Object.assign({},src)).length})",
  "T(()=>{var src={b:1,a:2,1:3,[Symbol.for('z')]:4,0:5};return Reflect.ownKeys(Object.assign({},src)).map(String).join()})", "T(()=>{var log=[];var src={get a(){log.push('a');return 1},get b(){log.push('b');return 2},1:0};Object.assign({},src);return log.join()})",
  "T(()=>{var log=[];var tgt={set a(v){log.push('set a '+v)},set b(v){log.push('set b '+v)}};Object.assign(tgt,{b:1,a:2});return log.join()})",
  "T(()=>{var src={get a(){delete this.b;return 1},b:2};return S(Object.assign({},src))})", "T(()=>{var src={get a(){this.c=3;return 1},b:2};return S(Object.assign({},src))})",
  "T(()=>Object.assign({get a(){return 1}},{a:2}))", "T(()=>Object.assign({set a(v){}},{a:2}).a)", "T(()=>Object.assign(Object.freeze({a:1}),{a:2}))", "T(()=>Object.assign(Object.freeze({a:1}),{}))",
  "T(()=>Object.assign(Object.freeze({a:1}),{b:2}))", "T(()=>Object.assign(Object.preventExtensions({}),{b:2}))", "T(()=>Object.assign(Object.preventExtensions({b:1}),{b:2}).b)", "T(()=>Object.assign(Object.seal({b:1}),{b:2}).b)",
  "T(()=>Object.assign(Object.seal({b:1}),{c:2}))", "T(()=>Object.assign(Object.freeze([1]),[2]))", "T(()=>Object.assign(new Uint8Array(2),[7,8,9]))", "T(()=>Object.assign(new Uint8Array(2),{5:1}).length)",
  "T(()=>S(Object.assign(new Uint8Array(2),{0:300})))", "T(()=>S(Object.assign({}, new Uint8Array([1,2]))))", "T(()=>S(Object.assign({},new Map([[1,2]]))))", "T(()=>S(Object.assign({},function(){}, {x:1})))",
  "T(()=>{var o=Object.create({inh:1});o.own=2;return S(Object.assign({},o))})", "T(()=>{var tgt=Object.create({set x(v){this._x=v}});Object.assign(tgt,{x:1});return Object.keys(tgt).join()+tgt._x})",
  "T(()=>{var tgt=Object.create(Object.freeze({x:1}));Object.assign(tgt,{x:2});return tgt.x})", "T(()=>{var p=new Proxy({},{set(t,k,v){return false}});Object.assign(p,{a:1})})",
  "T(()=>{var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+k);return t[k]}});Object.assign({},p);return log.join()})",
  "T(()=>{var log=[];var p=new Proxy({},{set(t,k,v,r){log.push('set '+k);return Reflect.set(t,k,v,r)},defineProperty(t,k,d){log.push('def '+k);return Reflect.defineProperty(t,k,d)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}});Object.assign(p,{a:1});return log.join()})",
  "T(()=>{var o={a:1};Object.assign(o,o);return S(o)})", "T(()=>{var a=[1,2];Object.assign(a,{length:1});return S(a)})", "T(()=>{var o={};Object.assign(o,{__proto__:{x:1},y:2});return S(o)+o.x})",
  "T(()=>{var o={};Object.assign(o,JSON.parse('{\"__proto__\":{\"x\":1}}'));return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{var o={};Object.assign(o,JSON.parse('{\"__proto__\":{\"x\":1}}'));return o.x})",
  "T(()=>{var src={a:1};var r=Object.assign({},src,{a:2},{a:3});return r.a})", "T(()=>{var args=[1,2];return S(Object.assign({}, ...args.map(x=>({['k'+x]:x}))))})",
  "T(()=>{var first={get a(){throw new RangeError('g')}};var second={b:1};var t={};try{Object.assign(t,{c:1},first,second)}catch(e){}return S(t)})",
  "T(()=>S({...{get a(){return 1}}}))", "T(()=>D({...{get a(){return 1}}},'a'))", "T(()=>S({...'ab'}))", "T(()=>S({...null,...undefined,...1}))", "T(()=>S({...[1,2]}))", "T(()=>S({...{[Symbol.for('s')]:1}}))",
  "T(()=>{var s=Symbol('s');var o={...{[s]:1}};return o[s]})", "T(()=>S({a:1,...{a:2},a:3}))", "T(()=>{var log=[];var p=new Proxy({a:1},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+k);return t[k]}});({...p});return log.join()})",
  "T(()=>{var o=Object.freeze({a:1});var c={...o};c.a=2;return c.a+Object.isFrozen(c)})", "T(()=>{var o={__proto__:{x:1},y:2};return S({...o})})",
);

// ---- 10. Reflect.construct, newTarget, e demais Reflect.*.
add(
  "T(()=>{function A(){this.t=new.target===B}function B(){}var o=Reflect.construct(A,[],B);return Object.getPrototypeOf(o)===B.prototype&&o.t})",
  "T(()=>{function A(){return new.target}function B(){}return Reflect.construct(A,[],B)===B})", "T(()=>{function A(){return new.target}return Reflect.construct(A,[])===A})",
  "T(()=>{class A{constructor(){this.n=new.target.name}}class B{}return Reflect.construct(A,[],B).n})", "T(()=>{class A{}class B{}var o=Reflect.construct(A,[],B);return (o instanceof A)+','+(o instanceof B)})",
  "T(()=>{class A{}function B(){}B.prototype=null;var o=Reflect.construct(A,[],B);return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>{class A{}function B(){}B.prototype=1;var o=Reflect.construct(A,[],B);return Object.getPrototypeOf(o)===Object.prototype})",
  "T(()=>{class A{}function B(){}B.prototype={x:1};return Reflect.construct(A,[],B).x})", "T(()=>{class A{}var nt=new Proxy(function(){},{get(t,k){return k==='prototype'?{p:1}:t[k]}});return Reflect.construct(A,[],nt).p})",
  "T(()=>{var log=[];class A{}var nt=new Proxy(function(){},{get(t,k){log.push(String(k));return t[k]}});Reflect.construct(A,[],nt);return log.join()})",
  "T(()=>{class A{}var nt=function(){};Object.defineProperty(nt,'prototype',{get(){throw new EvalError('p')}});return Reflect.construct(A,[],nt)})",
  "T(()=>Reflect.construct(Array,[3],Object).length)", "T(()=>Array.isArray(Reflect.construct(Array,[3],Object)))", "T(()=>Object.getPrototypeOf(Reflect.construct(Array,[],Object))===Object.prototype)",
  "T(()=>Object.getPrototypeOf(Reflect.construct(Date,[0],Object))===Object.prototype)", "T(()=>Object.prototype.toString.call(Reflect.construct(Date,[0],Object)))", "T(()=>Object.getPrototypeOf(Reflect.construct(Error,['m'],Array))===Array.prototype)",
  "T(()=>Reflect.construct(Error,['m'],Array).message)", "T(()=>Object.prototype.toString.call(Reflect.construct(Error,['m'],Object)))", "T(()=>Reflect.construct(Map,[],Set) instanceof Set)", "T(()=>Reflect.construct(Map,[[[1,2]]],Set).get(1))",
  "T(()=>Reflect.construct(Promise,[()=>{}],Object) instanceof Promise)", "T(()=>Reflect.construct(Uint8Array,[2],Array).length)", "T(()=>Reflect.construct(RegExp,['a','g'],Object).global)",
  "T(()=>Reflect.construct(Function,['return 1'],Object)())", "T(()=>Object.getPrototypeOf(Reflect.construct(Function,['return 1'],Object))===Object.prototype)", "T(()=>typeof Reflect.construct(Object,[],Array))",
  "T(()=>Array.isArray(Reflect.construct(Object,[],Array)))", "T(()=>Object.getPrototypeOf(Reflect.construct(Object,[],Array))===Array.prototype)", "T(()=>Object.getPrototypeOf(Reflect.construct(Object,[1],Array))===Number.prototype)",
  "T(()=>Reflect.construct(String,['ab'],Object).length)", "T(()=>Reflect.construct(Number,['5'],Object)+1)", "T(()=>Reflect.construct(Boolean,[0],Object) instanceof Boolean)", "T(()=>Reflect.construct(Symbol,[],Object))",
  "T(()=>Reflect.construct(BigInt,[1],Object))", "T(()=>Reflect.construct(()=>{},[]))", "T(()=>Reflect.construct(function*(){},[]))", "T(()=>Reflect.construct(async function(){},[]))", "T(()=>Reflect.construct({m(){}}.m,[]))",
  "T(()=>Reflect.construct(Math.max,[]))", "T(()=>Reflect.construct(Symbol.prototype.toString,[]))", "T(()=>Reflect.construct(parseInt,[]))", "T(()=>Reflect.construct(class{},[],()=>{}))", "T(()=>Reflect.construct(class{},[],{}))",
  "T(()=>Reflect.construct(class{},[],1))", "T(()=>Reflect.construct(class{},[],undefined) instanceof Object)", "T(()=>Reflect.construct(class{},[],null))", "T(()=>Reflect.construct(class{},[],Math.max))", "T(()=>Reflect.construct(class{},[],Reflect.construct))",
  "T(()=>Reflect.construct(class{},[],class{}) instanceof Object)", "T(()=>Reflect.construct(class{},[],function*(){}))", "T(()=>Reflect.construct(class{},[],async()=>{}))", "T(()=>Reflect.construct(class{},[],new Proxy(function(){},{}))!==undefined)",
  "T(()=>Reflect.construct(class{},[],new Proxy(()=>{},{})))", "T(()=>Reflect.construct(class{},[],new Proxy({},{})))", "T(()=>Reflect.construct(class{},[],Function.prototype.bind.call(function(){})))",
  "T(()=>Reflect.construct(function(){},1))", "T(()=>Reflect.construct(function(){},'ab'))", "T(()=>Reflect.construct(function(){},null))", "T(()=>Reflect.construct(function(){}))", "T(()=>Reflect.construct())", "T(()=>Reflect.construct(1,[]))",
  "T(()=>Reflect.construct(function(){return arguments.length},{length:2,0:'a',1:'b'})!==undefined)", "T(()=>{function A(){this.n=arguments.length}return Reflect.construct(A,{length:3}).n})", "T(()=>{function A(){this.n=[].slice.call(arguments).join()}return Reflect.construct(A,{length:2,0:'a',1:'b'}).n})",
  "T(()=>{function A(){this.n=arguments.length}return Reflect.construct(A,'ab')})", "T(()=>{function A(){this.n=arguments.length}return Reflect.construct(A,new Uint8Array(3)).n})", "T(()=>{function A(){this.n=arguments.length}return Reflect.construct(A,{length:-1}).n})",
  "T(()=>{function A(){this.n=arguments.length}return Reflect.construct(A,{length:'2'}).n})", "T(()=>{function A(){this.n=arguments.length}return Reflect.construct(A,{}).n})", "T(()=>{function A(){return 1}return typeof Reflect.construct(A,[])})",
  "T(()=>{function A(){return {r:1}}return Reflect.construct(A,[]).r})", "T(()=>{function A(){return {r:1}}function B(){}return Object.getPrototypeOf(Reflect.construct(A,[],B))===Object.prototype})",
  "T(()=>{class A{constructor(){return {r:1}}}class B{}return Reflect.construct(A,[],B) instanceof B})", "T(()=>{class A{constructor(){return 1}}return typeof Reflect.construct(A,[])})",
  "T(()=>{class A extends Object{constructor(){super();this.t=new.target===B}}class B{}var o=Reflect.construct(A,[],B);return o.t+','+(o instanceof B)})", "T(()=>{class A extends Array{}var o=Reflect.construct(A,[3],Object);return Array.isArray(o)+','+o.length+','+(o instanceof A)})",
  "T(()=>{class A extends Array{}class B{}var o=Reflect.construct(A,[3],B);return Array.isArray(o)+','+(o instanceof Array)+','+(o instanceof B)})", "T(()=>{class A extends Map{}var o=Reflect.construct(A,[],Object);return o instanceof A})",
  "T(()=>{class A{#p=1;static has(o){return #p in o}}class B{}var o=Reflect.construct(A,[],B);return A.has(o)+','+(o instanceof B)})", "T(()=>{class A{static x=1;y=2}return S(Reflect.construct(A,[],class B{static z(){}}))})",
  "T(()=>{var log=[];class A{constructor(){log.push(new.target===A)}}Reflect.construct(A,[]);Reflect.construct(A,[],A);Reflect.construct(A,[],Object);return log.join()})",
  "T(()=>{function A(){}A.prototype.m=1;var B=Object.assign(function(){},{prototype:{m:2}});return Reflect.construct(A,[],B).m})", "T(()=>Reflect.construct.length+Reflect.construct.name)",
  "T(()=>{var o=Reflect.construct(Object,[]);return Object.getPrototypeOf(o)===Object.prototype})", "T(()=>Reflect.construct(Object,[null]) instanceof Object)", "T(()=>S(Reflect.construct(Array,[1,2,3])))",
  "T(()=>S(Reflect.construct(Array,[2.5])))", "T(()=>S(Reflect.construct(Array,['2'])))", "T(()=>S(Reflect.construct(Date,[2020,0,1]).getFullYear()))", "T(()=>S(Reflect.construct(Set,[[1,1,2]]).size))",
  "T(()=>Reflect.construct(new Proxy(function(){this.a=1},{construct(t,a,nt){return {viaProxy:nt===undefined}}}),[]).viaProxy)", "T(()=>{var P=new Proxy(function(){},{construct(t,a,nt){return nt}});return Reflect.construct(P,[],Array)===Array})",
  "T(()=>{var P=new Proxy(function(){},{construct(t,a,nt){return nt}});return new P===P})", "T(()=>{var P=new Proxy(function(){},{construct(){return 1}});return new P})", "T(()=>{var P=new Proxy(function(){},{construct:1});return new P})",
  "T(()=>{var P=new Proxy({},{construct(){return {}}});return new P})", "T(()=>{var P=new Proxy(class{},{});return typeof new P})", "T(()=>{var P=new Proxy(()=>{},{});return new P})",
  "T(()=>{var B=function(){}.bind(null);return Reflect.construct(B,[])!==undefined})", "T(()=>{function A(){this.n=new.target}var B=A.bind(null);return Reflect.construct(B,[])===undefined})",
  "T(()=>{function A(){this.n=new.target===A}var B=A.bind(null);return new B().n})", "T(()=>{function A(){this.n=new.target===C}function C(){}var B=A.bind(null);return Reflect.construct(B,[],C).n})",
  "T(()=>{function A(){this.n=new.target===C}function C(){}var B=A.bind(null);return Reflect.construct(B,[],B).n})",
);
// Os demais Reflect.* sobre alvos e receivers variados.
const reflectTargets = ["{}", "{a:1}", "[1,2]", "function(){}", "Object.freeze({a:1})", "Object.preventExtensions({a:1})", "Object.seal({a:1})", "new Uint8Array(2)", "Object.create({a:1})", "new Proxy({a:1},{})", "'str'", "1", "null"];
for (const t of reflectTargets) {
  add(
    `T(()=>Reflect.has(${t},'a'))`, `T(()=>Reflect.get(${t},'a'))`, `T(()=>Reflect.get(${t},'0'))`, `T(()=>Reflect.get(${t},'length'))`, `T(()=>Reflect.set(${t},'a',2))`, `T(()=>Reflect.set(${t},'0',9))`, `T(()=>Reflect.set(${t},'b',2))`,
    `T(()=>Reflect.deleteProperty(${t},'a'))`, `T(()=>Reflect.deleteProperty(${t},'zz'))`, `T(()=>Reflect.defineProperty(${t},'a',{value:5}))`, `T(()=>Reflect.defineProperty(${t},'q',{value:5,configurable:true}))`,
    `T(()=>Reflect.defineProperty(${t},'a',1))`, `T(()=>{var x=${t};return Reflect.getOwnPropertyDescriptor(x,'a')&&D(x,'a')})`, `T(()=>Reflect.isExtensible(${t}))`, `T(()=>Reflect.preventExtensions(${t}))`,
    `T(()=>{var x=${t};Reflect.preventExtensions(x);return Reflect.isExtensible(x)})`, `T(()=>Reflect.getPrototypeOf(${t})===Object.prototype)`, `T(()=>Reflect.apply(function(){return this},${t},[])===${t})`,
  );
}
add(
  "T(()=>{var o={get a(){return this.v},v:1};return Reflect.get(o,'a',{v:2})})", "T(()=>{var o={get a(){return this.v},v:1};return Reflect.get(o,'a')})", "T(()=>{var o={set a(x){this.v=x}};var r={};Reflect.set(o,'a',3,r);return S(r)})",
  "T(()=>{var o={a:1};var r={};return Reflect.set(o,'a',3,r)+S(r)+S(o)})", "T(()=>{var o={a:1};var r=Object.freeze({});return Reflect.set(o,'a',3,r)})", "T(()=>{var o={a:1};var r=Object.freeze({a:0});return Reflect.set(o,'a',3,r)})",
  "T(()=>{var o={a:1};var r={get a(){return 1}};return Reflect.set(o,'a',3,r)})", "T(()=>{var o={a:1};var r={};Object.defineProperty(r,'a',{value:0,writable:false,configurable:true});return Reflect.set(o,'a',3,r)})",
  "T(()=>{var o={a:1};var r={};Object.defineProperty(r,'a',{value:0,writable:true,enumerable:false,configurable:true});Reflect.set(o,'a',3,r);return D(r,'a')})", "T(()=>Reflect.set({a:1},'a',2,1))", "T(()=>Reflect.set({a:1},'a',2,null))",
  "T(()=>Reflect.set({a:1},'a',2,'s'))", "T(()=>Reflect.set({},'a',2,undefined))", "T(()=>{var r=[];return Reflect.set({},'0',5,r)+S(r)+r.length})", "T(()=>{var r=[];return Reflect.set({},'length',5,r)+S(r)})",
  "T(()=>Reflect.set(Object.freeze({}),'a',1,{}))", "T(()=>Reflect.set([],'length',-1))", "T(()=>Reflect.set([1],'length',0))", "T(()=>Reflect.set(Object.freeze([1]),'length',0))", "T(()=>Reflect.set(new Uint8Array(1),'5',1))",
  "T(()=>{var u=new Uint8Array(1);var r={};return Reflect.set(u,'0',9,r)+S(r)+u[0]})", "T(()=>{var u=new Uint8Array(1);return Reflect.set(u,'0',9,u)+S(u)})", "T(()=>{var u=new Uint8Array(1);return Reflect.set({},'0',9,u)+S(u)})",
  "T(()=>{var u=new Uint8Array(1);return Reflect.set({},'5',9,u)+S(u)+Object.keys(u).join()})", "T(()=>{var p={set a(v){this.z=v}};var o=Object.create(p);Reflect.set(o,'a',1);return Object.keys(o).join()})",
  "T(()=>{var p=Object.freeze({a:1});var o=Object.create(p);return Reflect.set(o,'a',2)})", "T(()=>{var p={};Object.defineProperty(p,'a',{get(){return 1}});var o=Object.create(p);return Reflect.set(o,'a',2)})",
  "T(()=>Reflect.get({a:1},'a',null))", "T(()=>Reflect.get({get a(){return typeof this}},'a',1))", "T(()=>Reflect.get({get a(){'use strict';return typeof this}},'a',1))", "T(()=>Reflect.get({get a(){'use strict';return this}},'a'))",
  "T(()=>Reflect.get({get [Symbol.for('s')](){return 7}},Symbol.for('s')))", "T(()=>Reflect.get({1:'x'},1))", "T(()=>Reflect.get({1:'x'},{toString(){return '1'}}))", "T(()=>Reflect.get({},{toString(){throw new RangeError('k')}}))",
  "T(()=>Reflect.has({a:1},{toString(){return 'a'}}))", "T(()=>Reflect.has(Object.create({a:1}),'a'))", "T(()=>Reflect.has([],'length'))", "T(()=>Reflect.has('s','length'))", "T(()=>Reflect.has(1,'a'))",
  "T(()=>Reflect.has(new Proxy({},{has(){return 1}}),'a'))", "T(()=>Reflect.has(new Proxy({},{has(){return 0}}),'a'))", "T(()=>Reflect.has(new Proxy(Object.freeze({a:1}),{has(){return false}}),'a'))",
  "T(()=>Reflect.has(new Proxy(Object.preventExtensions({a:1}),{has(){return false}}),'a'))", "T(()=>Reflect.has(new Proxy(Object.preventExtensions({}),{has(){return true}}),'a'))",
  "T(()=>Reflect.defineProperty({},'a',{get:1}))", "T(()=>Reflect.defineProperty({},'a',{get(){},value:1}))", "T(()=>Reflect.defineProperty(Object.freeze({a:1}),'a',{value:1}))", "T(()=>Reflect.defineProperty(Object.freeze({a:1}),'a',{value:2}))",
  "T(()=>Reflect.defineProperty(Object.freeze({a:NaN}),'a',{value:NaN}))", "T(()=>Reflect.defineProperty(Object.freeze({a:0}),'a',{value:-0}))", "T(()=>Reflect.defineProperty([],'length',{value:-1}))", "T(()=>Reflect.defineProperty([],'length',{value:2**32}))",
  "T(()=>Reflect.defineProperty([1,2],'length',{value:1,writable:false}))", "T(()=>{var a=[1,2];Reflect.defineProperty(a,'length',{writable:false});return Reflect.defineProperty(a,'length',{value:5})})",
  "T(()=>{var a=[1,2];Reflect.defineProperty(a,'length',{writable:false});return Reflect.defineProperty(a,'5',{value:5})})", "T(()=>{var a=[1,2,3];Object.defineProperty(a,1,{configurable:false});return Reflect.defineProperty(a,'length',{value:0})+','+a.length})",
  "T(()=>Reflect.defineProperty(function(){},'prototype',{value:1}))", "T(()=>Reflect.defineProperty(function(){},'prototype',{enumerable:true}))", "T(()=>Reflect.defineProperty(class{},'prototype',{writable:true}))",
  "T(()=>Reflect.deleteProperty([1,2],'length'))", "T(()=>Reflect.deleteProperty(function(){},'prototype'))", "T(()=>Reflect.deleteProperty(function(){},'name'))", "T(()=>Reflect.deleteProperty(Object.freeze({}),'a'))", "T(()=>Reflect.deleteProperty('s','length'))",
  "T(()=>Reflect.deleteProperty(new String('s'),'0'))", "T(()=>Reflect.deleteProperty(new Uint8Array(1),'0'))", "T(()=>Reflect.deleteProperty(new Uint8Array(1),'5'))", "T(()=>Reflect.deleteProperty(new Uint8Array(1),'-0'))",
  "T(()=>Reflect.deleteProperty(Math,'PI'))", "T(()=>Reflect.deleteProperty(globalThis,'undefined'))", "T(()=>Reflect.deleteProperty(globalThis,'NaN'))",
  "T(()=>Reflect.apply(Math.max,null,[1,3,2]))", "T(()=>Reflect.apply(Math.max,null,{length:2,0:5,1:9}))", "T(()=>Reflect.apply(Math.max,null))", "T(()=>Reflect.apply(Math.max,null,null))", "T(()=>Reflect.apply(Math.max,null,1))",
  "T(()=>Reflect.apply(1,null,[]))", "T(()=>Reflect.apply(class{},null,[]))", "T(()=>Reflect.apply(function(){return typeof this},1,[]))", "T(()=>Reflect.apply(function(){'use strict';return typeof this},1,[]))",
  "T(()=>Reflect.apply(function(){'use strict';return this},undefined,[]))", "T(()=>Reflect.apply(function(){return this===globalThis},undefined,[]))", "T(()=>Reflect.apply.length+Reflect.apply.name)",
  "T(()=>Reflect[Symbol.toStringTag]+Object.prototype.toString.call(Reflect))", "T(()=>Object.getOwnPropertyNames(Reflect).join())", "T(()=>Reflect.getOwnPropertySymbols)", "T(()=>typeof Reflect+typeof Reflect.ownKeys)",
  "T(()=>D(globalThis,'Reflect'))", "T(()=>D(Reflect,Symbol.toStringTag))", "T(()=>Object.getOwnPropertyNames(Reflect).map(k=>D(Reflect,k)).join('|'))", "T(()=>new Reflect)", "T(()=>Reflect())", "T(()=>Object.getPrototypeOf(Reflect)===Object.prototype)",
);

// ---- 11. Demais Object.* (is, hasOwn, entries, toString, valueOf, propertyIsEnumerable e coerção).
add(
  "T(()=>Object.is(NaN,NaN)+','+Object.is(0,-0)+','+Object.is(-0,-0))", "T(()=>Object.is())", "T(()=>Object.is(undefined))", "T(()=>Object.is(1n,1n))", "T(()=>Object.is('a','a'))", "T(()=>Object.is({},{}))",
  "T(()=>Object.hasOwn({a:1},'a'))", "T(()=>Object.hasOwn({},'toString'))", "T(()=>Object.hasOwn(null,'a'))", "T(()=>Object.hasOwn(undefined,'a'))", "T(()=>Object.hasOwn('s','length'))", "T(()=>Object.hasOwn('s',0))",
  "T(()=>Object.hasOwn([],'length'))", "T(()=>Object.hasOwn({},{toString(){return 'a'}}))", "T(()=>Object.hasOwn({},{toString(){throw new RangeError('k')}}))", "T(()=>Object.hasOwn(null,{toString(){throw new RangeError('k')}}))",
  "T(()=>Object.hasOwn(new Uint8Array(1),'0')+','+Object.hasOwn(new Uint8Array(1),'1'))", "T(()=>Object.hasOwn({[Symbol.for('s')]:1},Symbol.for('s')))", "T(()=>Object.hasOwn.length+Object.hasOwn.name)",
  "T(()=>Object.prototype.hasOwnProperty.call(null,'a'))", "T(()=>Object.prototype.hasOwnProperty.call(null,{toString(){throw new RangeError('k')}}))", "T(()=>Object.prototype.hasOwnProperty.call(undefined,{toString(){throw new RangeError('k')}}))",
  "T(()=>Object.prototype.propertyIsEnumerable.call({a:1},'a'))", "T(()=>Object.prototype.propertyIsEnumerable.call([1],'length'))", "T(()=>Object.prototype.propertyIsEnumerable.call('ab',0))", "T(()=>Object.prototype.propertyIsEnumerable.call(null,'a'))",
  "T(()=>Object.prototype.propertyIsEnumerable.call({},'toString'))", "T(()=>Object.create({a:1}).propertyIsEnumerable('a'))", "T(()=>Object.prototype.toString.call(null)+Object.prototype.toString.call(undefined))",
  "T(()=>[1,'s',true,1n,Symbol(),[],{},function(){},new Date(0),/x/,new Error,new Map,new Set,new Uint8Array(0),Promise.resolve(),JSON,Math,Reflect,globalThis,(function(){return arguments})()].map(v=>Object.prototype.toString.call(v)).join())",
  "T(()=>Object.prototype.toString.call({[Symbol.toStringTag]:'Z'}))", "T(()=>Object.prototype.toString.call({[Symbol.toStringTag]:1}))", "T(()=>Object.prototype.toString.call(Object.defineProperty([],Symbol.toStringTag,{value:'Q'})))",
  "T(()=>Object.prototype.toString.call(new Proxy([],{})))", "T(()=>Object.prototype.toString.call(new Proxy(function(){},{})))", "T(()=>Object.prototype.toString.call(new Proxy({},{get(){return 'Fake'}})))",
  "T(()=>{var r=Proxy.revocable([],{});r.revoke();return Object.prototype.toString.call(r.proxy)})", "T(()=>Object.prototype.toLocaleString.call(null))", "T(()=>Object.prototype.toLocaleString.call(1))",
  "T(()=>Object.prototype.valueOf.call(null))", "T(()=>typeof Object.prototype.valueOf.call(1))", "T(()=>Object.prototype.valueOf.call('s') instanceof String)", "T(()=>Object(null) instanceof Object)", "T(()=>typeof Object(1n))",
  "T(()=>typeof Object(Symbol()))", "T(()=>Object(undefined).constructor===Object)", "T(()=>{var o={};return Object(o)===o})", "T(()=>new Object(1) instanceof Number)", "T(()=>new Object('s').length)",
  "T(()=>Object.length+Object.name)", "T(()=>Object.getOwnPropertyNames(Object).join())", "T(()=>Object.keys.name+Object.entries.length)", "T(()=>Object.getOwnPropertyNames(Object).map(k=>D(Object,k)).join('|'))",
  "T(()=>S(Object.entries('ab')))", "T(()=>S(Object.entries(1)))", "T(()=>S(Object.keys('ab')))", "T(()=>S(Object.values(function(){})))", "T(()=>S(Object.entries(Symbol())))",
  "T(()=>{var o={a:1,b:2,c:3};var r=[];for(var k in o){r.push(k);if(k==='a')delete o.b}return r.join()})", "T(()=>{var o={a:1};var r=[];for(var k in o){r.push(k);o.z=1}return r.join()})",
  "T(()=>{var p={x:1};var o=Object.create(p);o.y=2;var r=[];for(var k in o)r.push(k);return r.join()})", "T(()=>{var p={x:1};var o=Object.create(p);Object.defineProperty(o,'x',{value:2,enumerable:false});var r=[];for(var k in o)r.push(k);return r.join()})",
  "T(()=>{var r=[];for(var k in 'ab')r.push(k);return r.join()})", "T(()=>{var r=[];for(var k in [1,,3])r.push(k);return r.join()})", "T(()=>{var r=[];for(var k in new Uint8Array(2))r.push(k);return r.join()})",
  "T(()=>{var r=[];for(var k in {[Symbol.for('s')]:1,a:1})r.push(k);return r.join()})", "T(()=>{var r=[];for(var k in null)r.push(k);return r.length})", "T(()=>{var r=[];for(var k in new Proxy({a:1,b:2},{}))r.push(k);return r.join()})",
  "T(()=>{var log=[];for(var k in new Proxy({a:1},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getPrototypeOf(t){log.push('gpo');return null},getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return Reflect.getOwnPropertyDescriptor(t,k)}}));return log.join()})",
);

// ---- Execução.
const baseSources = [];
baseSources.push(...knownPrograms("object_edge_bun.tsv", ["object_model_bun.tsv", "reflect_bun.tsv", "reflection_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
const PRELOAD = writeResultPreload();
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida.
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + (/^T\(/.test(expr) ? `globalThis.R = ${expr}` : `globalThis.R = T(()=>{${/^(var|class)\b/.test(expr) ? expr.replace(/;([^;]*)$/, ";return $1") : "return " + expr}})`);
  let result;
  try {
    // Processo fresco por programa: o JSC reifica a tabela estática de JSON, Symbol, Reflect etc. por ordem de acesso,
    // então a ordem de Reflect.ownKeys deles depende do que rodou antes no mesmo processo.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
