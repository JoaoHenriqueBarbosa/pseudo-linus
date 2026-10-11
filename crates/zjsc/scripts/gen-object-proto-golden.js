// Gera tests/golden/object_proto_bun.tsv: o que faltava em Object.prototype e Object, medido no bun 1.4.2.
// Cobre o getter e o setter de __proto__ (primitivos, protótipo nulo, ciclos, Proxy, objetos não extensíveis, literais,
// JSON), __defineGetter__/__defineSetter__/__lookupGetter__/__lookupSetter__ (ordem de efeitos e erros),
// Object.prototype.toString com @@toStringTag em todos os built-ins e objetos exóticos, toLocaleString, valueOf em
// primitivos, isPrototypeOf/propertyIsEnumerable com Proxy, Object.assign e spread com getters e símbolos,
// Object.entries/values/keys em strings e arrays esparsos, Object.setPrototypeOf/Reflect.setPrototypeOf com ciclos e
// protótipo imutável (Object.prototype), Object.hasOwn e Object.groupBy. structuredClone fica fora.
// Cada programa roda num bun filho novo (o JSC reifica tabelas estáticas por ordem de acesso), sem APIs de host.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Programas cuja expressão já aparece nos goldens vizinhos (object_edge, object_statics, accessor, object_model,
// reflect, reflection, proxy*) são descartados.
// Uso: bun scripts/gen-object-proto-golden.js > tests/golden/object_proto_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

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
  'function P(p){if(p===null)return "null";if(p===undefined)return "undef";var k=[[Object.prototype,"OP"],[Array.prototype,"AP"],[Function.prototype,"FP"],[String.prototype,"SP"],[Number.prototype,"NP"],[Error.prototype,"EP"]];for(var i=0;i<k.length;i++)if(k[i][0]===p)return k[i][1];return typeof p==="function"?"fn":"obj"}\n' +
  'var PD=Object.getOwnPropertyDescriptor(Object.prototype,"__proto__"),PG=PD.get,PS=PD.set;\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const L = body => `T(()=>{${body}})`;
const E = expr => `T(()=>${expr})`;

const vals = [
  "undefined", "null", "true", "0", "-0", "1", "NaN", "''", "'s'", "'__proto__'", "1n", "Symbol()", "{}", "[]", "function(){}",
  "Object.create(null)", "new Proxy({},{})", "new Date(0)", "/x/", "new Number(1)",
];
const prims = ["undefined", "null", "true", "false", "0", "-0", "1", "NaN", "''", "'s'", "'abc'", "1n", "Symbol()", "Symbol.iterator"];

// ---- A. __proto__ (getter, setter, literais, JSON).
for (const v of vals) {
  add(L(`return P(PG.call(${v}))`), L(`return S(PS.call(${v},{}))`), L(`return S(PS.call(${v},null))`), L(`return S(PS.call(${v},undefined))`));
}
const targets = {
  obj: "{}", arr: "[]", fn: "function(){}", nullproto: "Object.create(null)", frozen: "Object.freeze({})", noext: "Object.preventExtensions({})",
  sealed: "Object.seal({a:1})", proxy: "new Proxy({},{})", date: "new Date(0)", re: "/x/", err: "new Error('m')", map: "new Map", u8: "new Uint8Array(1)",
  args: "(function(){return arguments})()", cls: "class C{}", arrow: "()=>1",
};
for (const [name, t] of Object.entries(targets)) {
  for (const v of vals) {
    add(L(`var o=${t};var r=S(PS.call(o,${v}));return r+' '+P(Object.getPrototypeOf(o))`));
  }
  for (const v of ["null", "{}", "Object.prototype", "Array.prototype", "1", "function(){}"]) {
    add(L(`var o=${t};o.__proto__=${v};return P(o.__proto__)+' '+Object.hasOwn(o,'__proto__')+' '+P(Object.getPrototypeOf(o))`));
  }
  add(L(`var o=${t};return P(o.__proto__)+' '+('__proto__' in o)+' '+Object.hasOwn(o,'__proto__')`));
}
// protótipo nulo: __proto__ vira propriedade própria comum.
for (const v of vals) {
  add(L(`var o=Object.create(null);o.__proto__=${v};return D(o,'__proto__')+' '+P(Object.getPrototypeOf(o))`));
  add(L(`var o=Object.create(null);Object.defineProperty(o,'__proto__',{value:${v},configurable:true});return D(o,'__proto__')+' '+P(o.__proto__)`));
}
add(
  L("var o=Object.setPrototypeOf({},null);o.__proto__=Object.prototype;return P(Object.getPrototypeOf(o))+Object.hasOwn(o,'__proto__')"),
  L("var o=Object.create(null);o['__proto__']=1;return Object.keys(o).join()+o.__proto__"),
  L("var o=Object.create(null);o.__proto__=1;return Object.getOwnPropertyNames(o).join()+('toString' in o)"),
  L("var o=Object.create(null);return o.__proto__+' '+('__proto__' in o)+' '+Object.hasOwn(o,'__proto__')"),
  L("var o=Object.create({__proto__:null});o.__proto__=1;return Object.hasOwn(o,'__proto__')"),
);
// ciclos.
add(
  L("var a={},b={};a.__proto__=b;b.__proto__=a"), L("var a={};a.__proto__=a"), L("var a={},b={},c={};a.__proto__=b;b.__proto__=c;c.__proto__=a"),
  L("var a={},b={},c={},d={};a.__proto__=b;b.__proto__=c;c.__proto__=d;d.__proto__=a"),
  L("var a={},b={};a.__proto__=b;try{b.__proto__=a}catch(e){}return P(a.__proto__)+P(b.__proto__)+(Object.getPrototypeOf(b)===Object.prototype)"),
  L("var a={},b=Object.create(a);return S(PS.call(a,b))"), L("var a={},b=Object.create(a);try{a.__proto__=b}catch(e){return e.constructor===TypeError}"),
  L("var a=[],b=Object.create(a);a.__proto__=b"), L("var f=function(){},g=Object.create(f);f.__proto__=g"),
  L("var a={};a.__proto__=Object.create(Object.create(a))"), L("var a={};return S(PS.call(a,Object.prototype))+P(a.__proto__)"),
  L("var a={};a.__proto__=Object.prototype;return Object.getPrototypeOf(a)===Object.prototype"),
  L("var a={},p=new Proxy({},{});a.__proto__=p;p.__proto__=a;return 'proxy cycle ok'"),
  L("var a={},p=new Proxy(a,{});a.__proto__=p;return 'ok'"),
  L("var a={},p=new Proxy({},{getPrototypeOf(){return a}});a.__proto__=p;return 'ok2'"),
  L("var a=Object.create(null),b=Object.create(a);a.__proto__=b;return Object.keys(a).join()+Object.getPrototypeOf(a)"),
  L("var a=Object.create(null),b=Object.create(a);return Reflect.setPrototypeOf(a,b)"),
  L("var a=Object.create(null);return Reflect.setPrototypeOf(a,a)"),
  L("var a=Object.create(null);return Object.setPrototypeOf(a,a)"),
);
// Proxy e __proto__.
add(
  L("var log=[];var p=new Proxy({},{getPrototypeOf(t){log.push('gpo');return Array.prototype}});return P(p.__proto__)+log.join()"),
  L("var log=[];var p=new Proxy({},{get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});var r=P(p.__proto__);return r+log.join()"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo');return true}});p.__proto__={};return log.join()+P(Object.getPrototypeOf(p))"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo');return false}});p.__proto__={};return log.join()"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo');return false}});return S(PS.call(p,{}))"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo');return false}});return Reflect.set(p,'__proto__',{})+log.join()"),
  L("var log=[];var p=new Proxy({},{set(t,k,v,r){log.push('set '+String(k));return Reflect.set(t,k,v,r)},defineProperty(t,k,d){log.push('def '+String(k));return Reflect.defineProperty(t,k,d)},setPrototypeOf(t,v){log.push('spo');return Reflect.setPrototypeOf(t,v)}});p.__proto__=Array.prototype;return log.join()"),
  L("var log=[];var p=new Proxy({},{getPrototypeOf(t){log.push('gpo');return null}});var o=Object.create(p);return o.__proto__+' '+log.join()+' '+P(Object.getPrototypeOf(o))"),
  L("var p=new Proxy({},{getPrototypeOf(t){return 1}});return PG.call(p)"), L("var p=new Proxy({},{getPrototypeOf(t){return undefined}});return PG.call(p)"),
  L("var p=new Proxy(Object.preventExtensions({}),{getPrototypeOf(t){return Array.prototype}});return PG.call(p)"),
  L("var p=new Proxy(Object.preventExtensions({}),{getPrototypeOf(t){return Object.prototype}});return P(PG.call(p))"),
  L("var p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(t,v){return true}});return S(PS.call(p,Array.prototype))"),
  L("var p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(t,v){return true}});return S(PS.call(p,Object.prototype))"),
  L("var p=new Proxy({},{setPrototypeOf(){throw new RangeError('trap')}});p.__proto__={}"),
  L("var p=new Proxy({},{getPrototypeOf(){throw new RangeError('trap')}});return p.__proto__"),
  L("var r=Proxy.revocable({},{});r.revoke();return r.proxy.__proto__"), L("var r=Proxy.revocable({},{});r.revoke();r.proxy.__proto__={}"),
  L("var r=Proxy.revocable({},{});r.revoke();return PG.call(r.proxy)"), L("var r=Proxy.revocable({},{});r.revoke();return PS.call(r.proxy,null)"),
  L("var r=Proxy.revocable({},{});r.revoke();return '__proto__' in r.proxy"), L("var r=Proxy.revocable({},{});r.revoke();return Object.getPrototypeOf(r.proxy)"),
  L("var r=Proxy.revocable({},{});r.revoke();return Object.setPrototypeOf(r.proxy,null)"), L("var r=Proxy.revocable({},{});r.revoke();return Reflect.setPrototypeOf(r.proxy,null)"),
  L("var p=new Proxy(function(){},{});p.__proto__=null;return P(Object.getPrototypeOf(p))"), L("var p=new Proxy([],{});return P(p.__proto__)+Array.isArray(p)"),
  L("var p=new Proxy(Object.create(null),{});return p.__proto__"),
);
// Descritor, nome e comprimento.
add(
  E("D(Object.prototype,'__proto__')"), E("PG.name+PG.length+PS.name+PS.length"), E("typeof PG+typeof PS"), E("PG.hasOwnProperty('prototype')"),
  E("Object.getOwnPropertyNames(PG).join()"), E("Object.getOwnPropertyNames(PS).join()"), E("new PG"), E("new PS"), E("Object.getPrototypeOf(PG)===Function.prototype"),
  E("PD.enumerable+','+PD.configurable"), E("Object.keys(Object.prototype).length"), E("Object.getOwnPropertyNames(Object.prototype).join()"),
  E("Object.getOwnPropertyNames(Object.prototype).map(k=>D(Object.prototype,k)).join('|')"), E("Reflect.ownKeys(Object.prototype).length"),
  E("Object.prototype.hasOwnProperty('__proto__')"), E("'__proto__' in Object.prototype"), E("Object.prototype.__proto__"), E("typeof Object.prototype.__proto__"),
  E("Object.getPrototypeOf(Object.prototype)"), E("Object.isExtensible(Object.prototype)"), E("Object.isFrozen(Object.prototype)"), E("Object.isSealed(Object.prototype)"),
  E("Object.prototype.constructor===Object"), E("Object.prototype.constructor.prototype===Object.prototype"),
  E("Object.getOwnPropertyNames(Object.prototype).filter(k=>typeof Object.prototype[k]==='function').map(k=>k+':'+Object.prototype[k].length).join()"),
  L("var r=[];for(var k in Object.prototype)r.push(k);return r.length"),
  L("var r=[];Object.defineProperty(Object.prototype,'zz',{value:1,enumerable:true,configurable:true});for(var k in {})r.push(k);delete Object.prototype.zz;return r.join()"),
  L("PD.set.call(1,{});return 1"), L("PD.get.call(undefined)"), L("PD.get.call(null)"), L("PD.set.call(undefined,{})"), L("PD.set.call(null,null)"),
  L("PD.get.call()"), L("PD.set.call()"), L("PD.set.call({})"), L("var o={};PD.set.call(o);return P(Object.getPrototypeOf(o))"),
  L("var o={};return S(PD.set.call(o,{}))"), L("return S(PD.set.call(1,{}))"), L("return S(PD.set.call('s',null))"),
  L("return S(PD.get.call(1))"), L("return P(PD.get.call(1))"), L("return P(PD.get.call('s'))"), L("return P(PD.get.call(true))"), L("return P(PD.get.call(1n))"), L("return P(PD.get.call(Symbol()))"),
  L("return PD.get.call(1)===Number.prototype"), L("return PD.get.call('s')===String.prototype"), L("return PD.get.call(Symbol())===Symbol.prototype"), L("return PD.get.call(1n)===BigInt.prototype"),
  L("delete Object.prototype.__proto__;var o={};o.__proto__=null;return Object.hasOwn(o,'__proto__')+' '+P(Object.getPrototypeOf(o))"),
  L("delete Object.prototype.__proto__;return P({}.__proto__)+('__proto__' in {})"),
  L("delete Object.prototype.__proto__;var o={__proto__:null};return P(Object.getPrototypeOf(o))+Object.hasOwn(o,'__proto__')"),
  L("Object.defineProperty(Object.prototype,'__proto__',{set(v){this.seen=v},get(){return 'g'}});var o={};o.__proto__=5;return o.seen+o.__proto__"),
  L("Object.defineProperty(Object.prototype,'__proto__',{value:1,writable:true,configurable:true});var o={__proto__:null};return P(Object.getPrototypeOf(o))"),
  L("var o={};PD.set.call(o,Array.prototype);return Array.isArray(o)+' '+(o instanceof Array)+' '+('push' in o)"),
  L("var o=[];PD.set.call(o,null);return Array.isArray(o)+' '+(o.push===undefined)+' '+Object.prototype.toString.call(o)"),
  L("var o=function(){};PD.set.call(o,null);return typeof o+' '+(o.call===undefined)"),
  L("var o={};PD.set.call(o,Function.prototype);return typeof o+' '+(typeof o.call)"),
);
// Literais de objeto com __proto__.
const litVals = ["null", "undefined", "1", "'s'", "true", "Symbol()", "{}", "[]", "Object.prototype", "Array.prototype", "function(){}", "new Proxy({},{})", "Object.create(null)", "1n", "NaN"];
for (const v of litVals) {
  add(
    L(`var o={__proto__:${v}};return P(Object.getPrototypeOf(o))+' '+Object.keys(o).join()`), L(`var o={'__proto__':${v}};return P(Object.getPrototypeOf(o))+' '+Object.keys(o).join()`),
    L(`var o={['__proto__']:${v}};return P(Object.getPrototypeOf(o))+' '+Object.keys(o).join()+' '+D(o,'__proto__').slice(0,3)`),
    L(`var __proto__=${v};var o={__proto__};return P(Object.getPrototypeOf(o))+' '+Object.keys(o).join()`),
    L(`var o={__proto__:${v},a:1};return P(Object.getPrototypeOf(o))+Object.getOwnPropertyNames(o).join()`),
  );
}
add(
  L("return typeof ({__proto__(){}}).__proto__+Object.keys({__proto__(){}}).join()"), L("var o={get __proto__(){return 7}};return o.__proto__+' '+P(Object.getPrototypeOf(o))"),
  L("var o={set __proto__(v){this.seen=v}};o.__proto__=3;return o.seen+' '+P(Object.getPrototypeOf(o))"), L("var o={async __proto__(){}};return P(Object.getPrototypeOf(o))+Object.keys(o).join()"),
  L("var o={*__proto__(){}};return P(Object.getPrototypeOf(o))+Object.keys(o).join()"), L("return eval('({__proto__:1,__proto__:2})')"), L("return eval('({__proto__:1,\"__proto__\":2})')"),
  L("return eval('({__proto__:1,[\"__proto__\"]:2})').__proto__"), L("return eval('({__proto__:1,__proto__(){}})').__proto__.name"), L("return eval('({__proto__:null,get __proto__(){return 1}})').__proto__"),
  L("return eval('({__proto__,__proto__})')"), L("var __proto__=1;return Object.keys(eval('({__proto__,__proto__:null})')).join()"),
  L("var __proto__=null;var o={__proto__,__proto__:Array.prototype};return P(Object.getPrototypeOf(o))"),
  L("return eval('var {__proto__:a,__proto__:b}={};1')"), L("var {__proto__:a}={x:1};return P(a)"), L("var {__proto__}=Object.create(null);return S(__proto__)"),
  L("var {__proto__:a}=1;return P(a)"), L("var {__proto__:a}=null;return a"), L("var o={};({__proto__:o.p}=[]);return P(o.p)"), L("var {...r}={__proto__:{a:1},b:2};return Object.keys(r).join()+P(Object.getPrototypeOf(r))"),
  L("var o={__proto__:{a:1}};var {a}=o;return a"), L("var s={};var o={__proto__:s,...{__proto__:null}};return P(Object.getPrototypeOf(o))"),
  L("var o={...JSON.parse('{\"__proto__\":null}')};return P(Object.getPrototypeOf(o))+Object.hasOwn(o,'__proto__')"),
  L("var o=JSON.parse('{\"__proto__\":{\"x\":1}}');return Object.hasOwn(o,'__proto__')+' '+P(Object.getPrototypeOf(o))+' '+o.x+' '+o.__proto__.x"),
  L("var o=JSON.parse('{\"__proto__\":1,\"a\":2}');return Object.keys(o).join()+o.__proto__"), L("var o=JSON.parse('{\"__proto__\":1,\"__proto__\":2}');return Object.keys(o).join()+o.__proto__"),
  L("var o=JSON.parse('[1]',function(k,v){return v});return P(Object.getPrototypeOf(o))"),
  L("var o=JSON.parse('{\"a\":{\"__proto__\":[]}}',function(k,v){return v});return Array.isArray(o.a.__proto__)+' '+Array.isArray(o.a)"),
  L("var o=Object.assign({},JSON.parse('{\"__proto__\":[]}'));return Array.isArray(o)+' '+P(Object.getPrototypeOf(o))"),
  L("var o={...JSON.parse('{\"__proto__\":[]}')};return Array.isArray(o)+' '+Object.hasOwn(o,'__proto__')"),
  L("var o=Object.fromEntries([['__proto__',[]]]);return Array.isArray(o)+' '+Object.hasOwn(o,'__proto__')+P(Object.getPrototypeOf(o))"),
  L("var m=new Map([['__proto__',1]]);var o=Object.fromEntries(m);return Object.hasOwn(o,'__proto__')+' '+o.__proto__"),
  L("var o=Object.defineProperties({},{__proto__:{value:1,enumerable:true}});return Object.keys(o).join()"),
  L("var o=Object.create(null,{__proto__:{value:1,enumerable:true}});return Object.keys(o).join()+o.__proto__"),
  L("var o=Object.create({},{__proto__:{value:1,enumerable:true}});return Object.keys(o).join()+Object.hasOwn(o,'__proto__')+P(Object.getPrototypeOf(o))"),
  L("var o=Object.groupBy(['a'],()=>'__proto__');return Object.keys(o).join()+Object.hasOwn(o,'__proto__')+P(Object.getPrototypeOf(o))"),
  L("var o=Object.groupBy(['a'],()=>'__proto__');return S(o.__proto__)"),
  L("var o=Object.entries({['__proto__']:1});return S(o)"), L("var o={['__proto__']:1};return Object.entries(o).join()+P(Object.getPrototypeOf(o))"),
  L("var o={};o['__proto__']=Array.prototype;return Array.isArray(o)+' '+(o instanceof Array)"),
  L("var k='__proto__';var o={};o[k]=null;return P(Object.getPrototypeOf(o))"), L("var o={};o['__pro'+'to__']=null;return P(Object.getPrototypeOf(o))"),
  L("var o={};o[{toString(){return '__proto__'}}]=null;return P(Object.getPrototypeOf(o))"), L("var o={};Reflect.set(o,'__proto__',null);return P(Object.getPrototypeOf(o))"),
  L("var o={};return Reflect.set(o,'__proto__',1)+' '+P(Object.getPrototypeOf(o))+Object.hasOwn(o,'__proto__')"),
  L("var o={};return Reflect.get(o,'__proto__',null)"), L("var o={};return Reflect.get(o,'__proto__',1)===Number.prototype"), L("var o={};var r=Object.create(null);return Reflect.set(o,'__proto__',null,r)+' '+Object.hasOwn(r,'__proto__')"),
  L("var o={};var r={};Reflect.set(o,'__proto__',null,r);return P(Object.getPrototypeOf(r))"), L("var o={};return Reflect.has(o,'__proto__')+' '+Reflect.has(Object.create(null),'__proto__')"),
  L("var o=Object.create(null);Object.defineProperty(o,'__proto__',{get(){return 5}});return o.__proto__"), L("return Reflect.ownKeys(Object.prototype).indexOf('__proto__')>=0"),
  L("var o={};with(o){__proto__=null}return P(Object.getPrototypeOf(o))"),
);
// sloppy: o.__proto__ = x que falha não lança, e a atribuição devolve x.
const sloppy = (body) => `T(()=>{return new Function(${JSON.stringify(body)})()})`;
for (const v of ["1", "{}", "null", "undefined", "'s'", "Object.prototype"]) {
  add(
    sloppy(`var o=Object.preventExtensions({});var r=(o.__proto__=${v});return String(typeof r)+' '+String(Object.getPrototypeOf(o)===Object.prototype)`),
    sloppy(`var o=Object.freeze([]);o.__proto__=${v};return String(Object.getPrototypeOf(o)===Array.prototype)`),
    sloppy(`var o=Object.create(null);var r=(o.__proto__=${v});return String(Object.getPrototypeOf(o))+' '+String(r===${v})`),
    sloppy(`var p={};var o=Object.create(p);try{p.__proto__=o}catch(e){return e.name}return 'noerr'`),
  );
}
add(
  sloppy("Object.prototype.__proto__=null;return 'ok'"), sloppy("Object.prototype.__proto__={};return 'ok'"), sloppy("var r=Object.prototype.__proto__={};return typeof r"),
  sloppy("return String(Object.getPrototypeOf(Object.prototype))"), sloppy("var a={};var b=Object.create(a);try{a.__proto__=b}catch(e){return e.message}"),
  sloppy("var o=Object.preventExtensions({});try{o.__proto__={}}catch(e){return e.message}return 'noerr'"), sloppy("try{Object.prototype.__proto__={}}catch(e){return e.message}return 'noerr'"),
  E("(function(){'use strict';var o=Object.preventExtensions({});o.__proto__={}})()"), E("(function(){'use strict';var o=Object.preventExtensions({});o.__proto__=Object.prototype;return 'same ok'})()"),
  E("(function(){'use strict';Object.prototype.__proto__=null;return 'ok'})()"), E("(function(){'use strict';Object.prototype.__proto__={}})()"),
  E("(function(){'use strict';var a={},b=Object.create(a);a.__proto__=b})()"),
);

// ---- B. __defineGetter__, __defineSetter__, __lookupGetter__, __lookupSetter__.
const recv = { obj: "{}", arr: "[]", fn: "function(){}", nullproto: "Object.create(null)", frozen: "Object.freeze({})", noext: "Object.preventExtensions({})", proxy: "new Proxy({},{})", str: "new String('ab')", u8: "new Uint8Array(1)" };
const getters = ["function(){return 1}", "()=>2", "undefined", "null", "1", "{}", "'s'", "Symbol()", "class{}", "function*(){}", "async function(){}", "new Proxy(function(){},{})", "new Proxy({},{})", "Math.max", "Object.prototype.valueOf.bind({})"];
for (const [rn, r] of Object.entries(recv)) {
  for (const g of getters) {
    add(
      L(`var o=${r};var x=o.__defineGetter__ ? Object.prototype.__defineGetter__.call(o,'k',${g}) : 0;return S(x)+' '+D(o,'k')`),
      L(`var o=${r};var x=Object.prototype.__defineSetter__.call(o,'k',${g});return S(x)+' '+D(o,'k')`),
    );
  }
}
for (const p of prims) {
  add(
    L(`return S(Object.prototype.__defineGetter__.call(${p},'k',function(){}))`), L(`return S(Object.prototype.__defineSetter__.call(${p},'k',function(){}))`),
    L(`return S(Object.prototype.__lookupGetter__.call(${p},'k'))`), L(`return S(Object.prototype.__lookupSetter__.call(${p},'toString'))`),
    L(`return S(Object.prototype.__lookupGetter__.call(${p},'__proto__'))===S(PG)`), L(`return S(Object.prototype.__lookupSetter__.call(${p},'__proto__'))===S(PS)`),
  );
}
const keys = ["'k'", "0", "-0", "1.5", "'1'", "Symbol.iterator", "Symbol('d')", "null", "undefined", "true", "{toString(){return 'ts'}}", "{valueOf(){return 'vo'},toString:undefined}", "[1,2]", "1n", "''", "'__proto__'", "'constructor'", "'length'", "'toString'"];
for (const k of keys) {
  add(
    L(`var o={};o.__defineGetter__(${k},function(){return 1});return Reflect.ownKeys(o).map(String).join()+' '+(D(o,Reflect.ownKeys(o)[0]).slice(0,12))`),
    L(`var o={};o.__defineSetter__(${k},function(v){});return Reflect.ownKeys(o).map(String).join()+' '+(D(o,Reflect.ownKeys(o)[0]).slice(0,40))`),
    L(`var o={};o.__defineGetter__(${k},function(){return 1});return D(o,Reflect.ownKeys(o)[0]).replace(/g=fn/,'g')`),
    L(`var o={};o.__defineGetter__(${k},function(){return 7});o.__defineSetter__(${k},function(v){});return S(o.__lookupGetter__(${k})())+typeof o.__lookupSetter__(${k})`),
    L(`var o={};return S(o.__lookupGetter__(${k}))+S(o.__lookupSetter__(${k}))`),
    L(`var g=function(){};var o=Object.defineProperty({},'k',{get:g,configurable:true});return o.__lookupGetter__('k')===g`),
    L(`var o=Object.create({get [${k}](){return 1}});return typeof o.__lookupGetter__(${k})+' '+typeof o.__lookupSetter__(${k})+' '+Object.hasOwn(o,${k})`),
  );
}
// ordem de efeitos.
add(
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{({}).__defineGetter__(k,1)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{({}).__defineSetter__(k,1)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{Object.prototype.__defineGetter__.call(null,k,1)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{Object.prototype.__defineGetter__.call(null,k,function(){})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{Object.prototype.__defineSetter__.call(undefined,k,function(){})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{Object.prototype.__lookupGetter__.call(null,k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};try{Object.prototype.__lookupSetter__.call(undefined,k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');throw new RangeError('k')}};try{({}).__lookupGetter__(k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');throw new RangeError('k')}};try{({}).__defineGetter__(k,function(){})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'k'}};({}).__lookupGetter__(k);({}).__lookupSetter__(k);return log.join()"),
  L("var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push('def '+String(k)+' '+Object.keys(d).join('/')+' '+d.enumerable+d.configurable);return Reflect.defineProperty(t,k,d)}});p.__defineGetter__('a',function(){});p.__defineSetter__('b',function(){});return log.join()"),
  L("var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push('def');return false}});p.__defineGetter__('a',function(){})"),
  L("var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push('def');return false}});p.__defineSetter__('a',function(){})"),
  L("var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push('def');return true}});p.__defineGetter__('a',function(){});return log.join()"),
  L("var log=[];var p=new Proxy({},{defineProperty(t,k,d){log.push('def');throw new EvalError('d')}});p.__defineGetter__('a',function(){})"),
  L("var log=[];var p=new Proxy({},{getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},getPrototypeOf(t){log.push('gpo');return Reflect.getPrototypeOf(t)}});p.__lookupGetter__('a');return log.join()"),
  L("var log=[];var p=new Proxy({get a(){return 1}},{getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},getPrototypeOf(t){log.push('gpo');return Reflect.getPrototypeOf(t)}});var g=p.__lookupGetter__('a');return log.join()+typeof g"),
  L("var log=[];var p=new Proxy({},{getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return undefined},getPrototypeOf(t){log.push('gpo');return {get z(){return 1}}}});var g=p.__lookupGetter__('z');return log.join()+typeof g"),
  L("var log=[];var p=new Proxy({},{getOwnPropertyDescriptor(t,k){log.push('gopd');return undefined},getPrototypeOf(t){log.push('gpo');return null}});p.__lookupSetter__('z');return log.join()"),
  L("var log=[];var p=new Proxy({},{get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});p.__lookupGetter__('a');return log.join()"),
  L("var log=[];var p=new Proxy({},{has(t,k){log.push('has '+String(k));return false}});p.__lookupGetter__('a');p.__defineGetter__('b',function(){});return log.join()"),
  L("var log=[];var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {value:1,configurable:true}}});return S(p.__lookupGetter__('a'))"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {get(){return 9},configurable:true}}});return S(p.__lookupGetter__('a')())"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {set(v){},configurable:true}}});return typeof p.__lookupSetter__('a')+typeof p.__lookupGetter__('a')"),
  L("var r=Proxy.revocable({},{});r.revoke();r.proxy.__lookupGetter__('a')"), L("var r=Proxy.revocable({},{});r.revoke();r.proxy.__defineGetter__('a',function(){})"),
  L("var r=Proxy.revocable({},{});r.revoke();r.proxy.__defineSetter__('a',1)"), L("var r=Proxy.revocable({},{});r.revoke();r.proxy.__lookupSetter__('a')"),
  L("var p=new Proxy({},{getPrototypeOf(){throw new RangeError('gpo')}});return p.__lookupGetter__('a')"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(){throw new RangeError('gopd')}});return p.__lookupGetter__('a')"),
  L("var o=Object.create(new Proxy({},{getOwnPropertyDescriptor(t,k){return {get(){return 5},configurable:true}}}));return S(o.__lookupGetter__('q')())"),
  L("var log=[];var o=Object.create(new Proxy({},{getOwnPropertyDescriptor(t,k){log.push('gopd '+k);return undefined},getPrototypeOf(t){log.push('gpo');return null}}));o.__lookupGetter__('q');return log.join()"),
);
// interação com propriedades existentes.
add(
  L("var o={a:1};o.__defineGetter__('a',function(){return 2});return D(o,'a').replace(/g=fn/,'g')"), L("var o={get a(){return 1}};o.__defineGetter__('a',function(){return 2});return o.a"),
  L("var o={get a(){return 1},set a(v){this.s=v}};o.__defineGetter__('a',function(){return 2});o.a=5;return o.a+' '+o.s"), L("var o={get a(){return 1},set a(v){this.s=v}};o.__defineSetter__('a',function(v){this.t=v});o.a=5;return o.a+' '+o.s+' '+o.t"),
  L("var o={};o.__defineGetter__('a',function(){return 1});return Object.keys(o).join()+D(o,'a').slice(-12)"), L("var o={};o.__defineSetter__('a',function(){});return D(o,'a').slice(-12)"),
  L("var o={};o.__defineGetter__('a',function(){return 1});o.__defineGetter__('a',function(){return 2});return o.a"), L("var o={};o.__defineGetter__('a',function(){return 1});o.a=3;return o.a"),
  L("'use strict';var o={};o.__defineGetter__('a',function(){return 1});o.a=3"), L("var o={};o.__defineSetter__('a',function(v){this._a=v});o.a=3;return o._a+' '+S(o.a)"),
  L("var o={};Object.defineProperty(o,'a',{value:1,configurable:false});o.__defineGetter__('a',function(){})"), L("var o={};Object.defineProperty(o,'a',{get(){},configurable:false});o.__defineGetter__('a',function(){})"),
  L("var o={};Object.defineProperty(o,'a',{get(){},set(v){},configurable:false});o.__defineSetter__('a',function(){})"), L("var o={};var g=function(){};Object.defineProperty(o,'a',{get:g,configurable:false});o.__defineGetter__('a',g);return 'same getter ok'"),
  L("var o=Object.freeze({a:1});o.__defineGetter__('a',function(){})"), L("var o=Object.freeze({});o.__defineGetter__('a',function(){})"), L("var o=Object.seal({a:1});o.__defineGetter__('a',function(){return 1});return 'sealed ok'"),
  L("var o=Object.preventExtensions({});o.__defineSetter__('b',function(){})"), L("var o=Object.preventExtensions({a:1});o.__defineSetter__('a',function(){});return D(o,'a').slice(0,20)"),
  L("var a=[1,2];a.__defineGetter__('length',function(){})"), L("var a=[1,2];a.__defineGetter__(0,function(){return 'g'});return a[0]+a.length"), L("var a=[];a.__defineGetter__(5,function(){return 'g'});return a.length"),
  L("var a=[1,2];a.__defineSetter__(0,function(v){this.s=v});a[0]=9;return a.s+' '+a.length"), L("var u=new Uint8Array(1);u.__defineGetter__(0,function(){})"), L("var u=new Uint8Array(1);u.__defineGetter__(5,function(){});return 'ok'"),
  L("var u=new Uint8Array(1);u.__defineGetter__('length',function(){return 7});return u.length"), L("var s=new String('ab');s.__defineGetter__(0,function(){})"), L("var s=new String('ab');s.__defineGetter__(2,function(){return 'x'});return s[2]+s.length"),
  L("var f=function(){};f.__defineGetter__('name',function(){return 'N'});return f.name"), L("var f=function(){};f.__defineGetter__('prototype',function(){})"), L("var f=function(){};f.__defineGetter__('length',function(){return 3});return f.length"),
  L("var c=class{static x=1};c.__defineGetter__('x',function(){return 2});return c.x"), L("Math.__defineGetter__('PI',function(){return 3})"), L("Math.__defineGetter__('Z',function(){return 3});return Math.Z+' '+D(Math,'Z').slice(-12)"),
  L("globalThis.__defineGetter__('zz9',function(){return 'gg'});return zz9"), L("globalThis.__defineGetter__('undefined',function(){return 1})"), L("globalThis.__defineSetter__('zz8',function(v){globalThis.s8=v});zz8=4;return s8"),
  L("Object.prototype.__defineGetter__('zz7',function(){return typeof this});var r=(1).zz7+' '+'s'.zz7+' '+({}).zz7;delete Object.prototype.zz7;return r"),
  L("Object.prototype.__defineGetter__('zz6',function(){'use strict';return typeof this});var r=(1).zz6+' '+true.zz6;delete Object.prototype.zz6;return r"),
  L("Object.prototype.__defineSetter__('zz5',function(v){'use strict';this.t=typeof this});var o={};o.zz5=1;var r=o.t;delete Object.prototype.zz5;return r"),
  L("Number.prototype.__defineGetter__('zz4',function(){'use strict';return typeof this});return (5).zz4"), L("Number.prototype.__defineSetter__('zz3',function(v){'use strict';globalThis.q=typeof this});(5).zz3=1;return globalThis.q"),
  L("String.prototype.__defineGetter__('zz2',function(){'use strict';return this});return 'ab'.zz2"),
  L("var o={};var g=function(){return 1};o.__defineGetter__('a',g);return o.__lookupGetter__('a')===g+' '+(o.__lookupSetter__('a')===undefined)"),
  L("var o={};var s=function(){};o.__defineSetter__('a',s);return (o.__lookupSetter__('a')===s)+' '+(o.__lookupGetter__('a')===undefined)"),
  L("var p={get a(){return 1}};var o=Object.create(p);o.a2=1;return typeof o.__lookupGetter__('a')+' '+typeof o.__lookupSetter__('a')"),
  L("var p={get a(){return 1}};var o=Object.create(p);Object.defineProperty(o,'a',{value:3});return typeof o.__lookupGetter__('a')"),
  L("var p={get a(){return 1}};var o=Object.create(p);Object.defineProperty(o,'a',{set(v){}});return typeof o.__lookupGetter__('a')+typeof o.__lookupSetter__('a')"),
  L("var p={a:1};var o=Object.create({__proto__:p});return typeof o.__lookupGetter__('a')"), L("var o={a:1};return o.__lookupGetter__('a')"), L("var o=Object.create(null);return typeof o.__lookupGetter__"),
  L("var o=Object.create(null);return Object.prototype.__lookupGetter__.call(o,'a')"), L("var o=Object.create(null);Object.defineProperty(o,'a',{get(){return 4},configurable:true});return Object.prototype.__lookupGetter__.call(o,'a')()"),
  L("return typeof Object.prototype.__lookupGetter__.call(Object.prototype,'__proto__')"), L("return Object.prototype.__lookupGetter__.call(Object.prototype,'__proto__')===PG"), L("return Object.prototype.__lookupSetter__.call({},'__proto__')===PS"),
  L("return Object.prototype.__lookupGetter__.call(new Map,'size').name"), L("return typeof Object.prototype.__lookupGetter__.call(Map.prototype,'size')"), L("return Object.prototype.__lookupGetter__.call(Map.prototype,'size').call(new Map([[1,2]]))"),
  L("return typeof Object.prototype.__lookupGetter__.call(RegExp.prototype,'flags')"), L("return Object.prototype.__lookupGetter__.call(RegExp.prototype,'global').call(/x/g)"),
  L("return typeof Object.prototype.__lookupGetter__.call(new Uint8Array(1),'length')"), L("return Object.prototype.__lookupGetter__.call(Object.getPrototypeOf(Uint8Array.prototype),'length').call(new Uint8Array(3))"),
  L("return typeof ({}).__lookupGetter__.call(globalThis,'globalThis')"), L("return Object.prototype.__lookupGetter__.call(Symbol.prototype,'description').call(Symbol('d'))"),
  L("return typeof Object.prototype.__lookupSetter__.call(Array.prototype,'length')"), L("return typeof Object.prototype.__lookupGetter__.call(Function.prototype,'caller')"), L("return typeof Object.prototype.__lookupSetter__.call(Function.prototype,'arguments')"),
  L("return Object.prototype.__lookupGetter__.length+','+Object.prototype.__lookupSetter__.length+','+Object.prototype.__defineGetter__.length+','+Object.prototype.__defineSetter__.length"),
  L("return Object.prototype.__defineGetter__.name+','+Object.prototype.__defineSetter__.name+','+Object.prototype.__lookupGetter__.name+','+Object.prototype.__lookupSetter__.name"),
  L("return ['__defineGetter__','__defineSetter__','__lookupGetter__','__lookupSetter__'].map(k=>D(Object.prototype,k).replace(/v=fn/,'v')).join('|')"),
  L("new Object.prototype.__defineGetter__"), L("new Object.prototype.__lookupGetter__"), L("return Object.prototype.__defineGetter__.hasOwnProperty('prototype')"),
  L("return Object.prototype.__defineGetter__.call()"), L("return Object.prototype.__lookupGetter__.call()"), L("return Object.prototype.__lookupGetter__.call({})"), L("return Object.prototype.__defineGetter__.call({})"),
  L("var o={};o.__defineGetter__('a');"), L("var o={};o.__defineSetter__();"), L("var o={};o.__defineGetter__();"), L("var o={};return S(o.__lookupGetter__())+S(o.__lookupSetter__())"),
  L("var o={};o.__defineGetter__(undefined,function(){return 'u'});return o.undefined"), L("var o={};o.__defineGetter__('a',function(){return 1},'extra');return o.a"),
  L("var d=0;var o={};o.__defineGetter__('a',function(){d++;return d});o.a;o.a;return d"), L("var o={};o.__defineGetter__('a',function(){return this===o});return o.a"),
  L("var o={};o.__defineGetter__('a',function(){return this===o});var c=Object.create(o);return c.a"), L("var o={};o.__defineGetter__('a',()=>typeof this);return o.a"),
  L("var o={};o.__defineGetter__('a',function*(){});return typeof o.a"), L("var o={};o.__defineGetter__('a',async function(){return 1});return Object.prototype.toString.call(o.a)"),
  L("var o={};o.__defineGetter__('a',class{});return 'noerr'"), L("var o={};o.__defineGetter__('a',class{});return o.a"),
  L("var o={};o.__defineGetter__('a',Math.max);return o.a"), L("var o={};o.__defineGetter__('a',Array);return o.a.length"), L("var o={};o.__defineGetter__('a',Object);return typeof o.a"),
  L("var o={};o.__defineSetter__('a',Array);o.a=3;return Object.keys(o).join()"), L("var o={};o.__defineGetter__('a',function(){}.bind());return typeof o.a"),
  L("var o={};o.__defineGetter__('a',Function.prototype);return typeof o.a"), L("var o={};o.__defineSetter__('a',Function.prototype);o.a=1;return 'ok'"),
);

// ---- C. Object.prototype.toString e @@toStringTag.
const tagTargets = {
  "undefined": "undefined", "null": "null", "true": "true", "num": "1", "nan": "NaN", "str": "'s'", "empty": "''", "sym": "Symbol()", "bigint": "1n",
  "obj": "{}", "arr": "[]", "sparse": "[,]", "fn": "function(){}", "arrow": "()=>1", "async": "async function(){}", "gen": "function*(){}", "agen": "async function*(){}",
  "method": "({m(){}}).m", "asyncarrow": "async()=>1", "cls": "class{}", "bound": "function(){}.bind()", "native": "Math.max", "proxyfn": "new Proxy(function(){},{})",
  "proxyarr": "new Proxy([],{})", "proxyobj": "new Proxy({},{})", "proxyproxy": "new Proxy(new Proxy([],{}),{})", "date": "new Date(0)", "re": "/x/", "err": "new Error",
  "typeerr": "new TypeError", "rangeerr": "new RangeError", "syntaxerr": "new SyntaxError", "evalerr": "new EvalError", "referr": "new ReferenceError", "urierr": "new URIError",
  "aggerr": "new AggregateError([])", "boolobj": "new Boolean(true)", "numobj": "new Number(1)", "strobj": "new String('s')", "symobj": "Object(Symbol())", "bigobj": "Object(1n)",
  "args": "(function(){return arguments})()", "sargs": "(function(){'use strict';return arguments})()", "map": "new Map", "set": "new Set", "wmap": "new WeakMap", "wset": "new WeakSet",
  "wref": "new WeakRef({})", "freg": "new FinalizationRegistry(()=>{})", "promise": "Promise.resolve()", "ab": "new ArrayBuffer(1)", "sab": "new SharedArrayBuffer(1)",
  "dv": "new DataView(new ArrayBuffer(1))", "i8": "new Int8Array(1)", "u8": "new Uint8Array(1)", "u8c": "new Uint8ClampedArray(1)", "i16": "new Int16Array(1)", "u16": "new Uint16Array(1)",
  "i32": "new Int32Array(1)", "u32": "new Uint32Array(1)", "f32": "new Float32Array(1)", "f64": "new Float64Array(1)", "bi64": "new BigInt64Array(1)", "bu64": "new BigUint64Array(1)",
  "json": "JSON", "math": "Math", "reflect": "Reflect", "atomics": "Atomics", "intl": "Intl", "global": "globalThis", "wasm": "WebAssembly", "temporal": "Temporal",
  "genobj": "(function*(){})()", "agenobj": "(async function*(){})()", "arrit": "[][Symbol.iterator]()", "arrkeys": "[].keys()", "arrent": "[].entries()", "mapit": "new Map()[Symbol.iterator]()",
  "setit": "new Set().values()", "strit": "''[Symbol.iterator]()", "reit": "/x/g[Symbol.matchAll]('')", "u8it": "new Uint8Array(0)[Symbol.iterator]()", "iterhelper": "Iterator.from([1]).map(x=>x)",
  "iterctor": "Iterator", "iterproto": "Iterator.prototype", "iterfrom": "Iterator.from({next(){}})", "wrapfor": "Iterator.from({next(){}})",
  "collator": "new Intl.Collator", "dtf": "new Intl.DateTimeFormat", "nf": "new Intl.NumberFormat", "pr": "new Intl.PluralRules", "rtf": "new Intl.RelativeTimeFormat", "lf": "new Intl.ListFormat",
  "seg": "new Intl.Segmenter", "segs": "new Intl.Segmenter().segment('a')", "segit": "new Intl.Segmenter().segment('a')[Symbol.iterator]()", "loc": "new Intl.Locale('en')",
  "dn": "new Intl.DisplayNames('en',{type:'region'})", "df": "new Intl.DurationFormat", "tdur": "new Temporal.Duration", "tinst": "new Temporal.Instant(0n)", "tpd": "new Temporal.PlainDate(2020,1,1)",
  "tpdt": "new Temporal.PlainDateTime(2020,1,1)", "tpt": "new Temporal.PlainTime", "tpym": "new Temporal.PlainYearMonth(2020,1)", "tpmd": "new Temporal.PlainMonthDay(1,1)", "tzdt": "new Temporal.ZonedDateTime(0n,'UTC')",
  "tnow": "Temporal.Now", "shadow": "new ShadowRealm", "dispstack": "new DisposableStack", "adispstack": "new AsyncDisposableStack", "suppressed": "new SuppressedError(1,2)",
  "mapproto": "Map.prototype", "setproto": "Set.prototype", "promproto": "Promise.prototype", "symproto": "Symbol.prototype", "bigproto": "BigInt.prototype", "abproto": "ArrayBuffer.prototype",
  "dvproto": "DataView.prototype", "u8proto": "Uint8Array.prototype", "taproto": "Object.getPrototypeOf(Uint8Array.prototype)", "ta": "Object.getPrototypeOf(Uint8Array)", "genproto": "Object.getPrototypeOf(function*(){}).prototype",
  "genfnproto": "Object.getPrototypeOf(function*(){})", "agenfnproto": "Object.getPrototypeOf(async function*(){})", "afnproto": "Object.getPrototypeOf(async function(){})",
  "arrproto": "Array.prototype", "fnproto": "Function.prototype", "objproto": "Object.prototype", "strproto": "String.prototype", "numproto": "Number.prototype", "boolproto": "Boolean.prototype",
  "dateproto": "Date.prototype", "reproto": "RegExp.prototype", "errproto": "Error.prototype", "typeerrproto": "TypeError.prototype", "arrit proto": "Object.getPrototypeOf([][Symbol.iterator]())",
  "mapitproto": "Object.getPrototypeOf(new Map()[Symbol.iterator]())", "setitproto": "Object.getPrototypeOf(new Set()[Symbol.iterator]())", "stritproto": "Object.getPrototypeOf(''[Symbol.iterator]())",
  "reitproto": "Object.getPrototypeOf(/x/g[Symbol.matchAll](''))", "ithelpproto": "Object.getPrototypeOf(Iterator.from([1]).map(x=>x))", "wrapproto": "Object.getPrototypeOf(Iterator.from({next(){}}))",
  "agenproto": "Object.getPrototypeOf(async function*(){}).prototype", "asynciterproto": "Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}).prototype)",
  "wmapproto": "WeakMap.prototype", "wsetproto": "WeakSet.prototype", "wrefproto": "WeakRef.prototype", "fregproto": "FinalizationRegistry.prototype", "collproto": "Intl.Collator.prototype",
  "dtfproto": "Intl.DateTimeFormat.prototype", "nfproto": "Intl.NumberFormat.prototype", "locproto": "Intl.Locale.prototype", "segproto": "Intl.Segmenter.prototype", "prproto": "Intl.PluralRules.prototype",
  "rtfproto": "Intl.RelativeTimeFormat.prototype", "lfproto": "Intl.ListFormat.prototype", "dnproto": "Intl.DisplayNames.prototype", "sabproto": "SharedArrayBuffer.prototype",
  "wasmmod": "WebAssembly.Module.prototype", "wasminst": "WebAssembly.Instance.prototype", "wasmmem": "WebAssembly.Memory.prototype", "wasmtab": "WebAssembly.Table.prototype", "wasmglob": "WebAssembly.Global.prototype",
  "wasmmodinst": "new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]))", "wasmmeminst": "new WebAssembly.Memory({initial:1})", "wasmtabinst": "new WebAssembly.Table({initial:1,element:'anyfunc'})",
  "wasmglobinst": "new WebAssembly.Global({value:'i32'},1)", "wasminstinst": "new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])))",
  "wasmtag": "new WebAssembly.Tag({parameters:[]})", "wasmexc": "new WebAssembly.Exception(new WebAssembly.Tag({parameters:[]}),[])", "wasmcompile": "new WebAssembly.CompileError", "wasmlink": "new WebAssembly.LinkError", "wasmrt": "new WebAssembly.RuntimeError",
  "evalfn": "eval", "pint": "parseInt", "ctorsym": "Symbol", "ctorbig": "BigInt", "ctorproxy": "Proxy", "ctorprom": "Promise", "ctormap": "Map", "ctorobj": "Object", "ctorarr": "Array", "ctorfn": "Function",
};
for (const [name, t] of Object.entries(tagTargets)) {
  add(
    E(`Object.prototype.toString.call(${t})`),
    E(`String(${t}[Symbol.toStringTag])`),
    E(`D(Object(${t}),Symbol.toStringTag)`),
    E(`Object.prototype.toString.call(Object.setPrototypeOf(Object(${t}),null))`),
  );
}
const tagVals = ["'Z'", "''", "undefined", "null", "1", "true", "{}", "Symbol()", "'Array'", "'Function'", "'Object'", "'Error'", "'Boolean'", "'Number'", "'String'", "'Date'", "'RegExp'", "'Arguments'", "'Null'", "'Undefined'", "{toString(){return 'ts'}}", "function(){return 'f'}", "new String('Q')"];
const tagBase = { obj: "{}", arr: "[]", fn: "function(){}", date: "new Date(0)", re: "/x/", err: "new Error", bool: "new Boolean(true)", num: "new Number(1)", str: "new String('s')", args: "(function(){return arguments})()", map: "new Map", prom: "Promise.resolve()", u8: "new Uint8Array(0)", nullp: "Object.create(null)" };
for (const [bn, b] of Object.entries(tagBase)) {
  for (const v of tagVals) {
    add(L(`var o=${b};Object.defineProperty(o,Symbol.toStringTag,{value:${v},configurable:true});return Object.prototype.toString.call(o)`));
  }
}
for (const v of tagVals) {
  add(
    L(`var o={};o[Symbol.toStringTag]=${v};return Object.prototype.toString.call(o)`),
    L(`var o=Object.create({[Symbol.toStringTag]:${v}});return Object.prototype.toString.call(o)`),
    L(`var o={get [Symbol.toStringTag](){return ${v}}};return Object.prototype.toString.call(o)`),
    L(`var p=new Proxy([],{get(t,k){return k===Symbol.toStringTag?${v}:Reflect.get(t,k)}});return Object.prototype.toString.call(p)`),
    L(`var p=new Proxy(function(){},{get(t,k){return k===Symbol.toStringTag?${v}:Reflect.get(t,k)}});return Object.prototype.toString.call(p)`),
    L(`return Object.prototype.toString.call(Object.assign(1,{}))`) ,
    L(`Number.prototype[Symbol.toStringTag]=${v};var r=Object.prototype.toString.call(1);delete Number.prototype[Symbol.toStringTag];return r`),
    L(`String.prototype[Symbol.toStringTag]=${v};var r=Object.prototype.toString.call('s');delete String.prototype[Symbol.toStringTag];return r`),
    L(`Boolean.prototype[Symbol.toStringTag]=${v};var r=Object.prototype.toString.call(true);delete Boolean.prototype[Symbol.toStringTag];return r`),
    L(`BigInt.prototype[Symbol.toStringTag]=${v};var r=Object.prototype.toString.call(1n);BigInt.prototype[Symbol.toStringTag]='BigInt';return r`),
    L(`Object.defineProperty(Symbol.prototype,Symbol.toStringTag,{value:${v}});return Object.prototype.toString.call(Symbol())`),
    L(`Object.defineProperty(Array.prototype,Symbol.toStringTag,{value:${v},configurable:true});var r=Object.prototype.toString.call([]);delete Array.prototype[Symbol.toStringTag];return r`),
    L(`Object.defineProperty(Function.prototype,Symbol.toStringTag,{value:${v},configurable:true});var r=Object.prototype.toString.call(function(){});delete Function.prototype[Symbol.toStringTag];return r`),
    L(`Object.defineProperty(Object.prototype,Symbol.toStringTag,{value:${v},configurable:true});var r=Object.prototype.toString.call({})+Object.prototype.toString.call([])+Object.prototype.toString.call(null);delete Object.prototype[Symbol.toStringTag];return r`),
  );
}
add(
  L("var log=[];var o=new Proxy({},{get(t,k){log.push(String(k));return Reflect.get(t,k)}});Object.prototype.toString.call(o);return log.join()"),
  L("var log=[];var o=new Proxy([],{get(t,k){log.push(String(k));return Reflect.get(t,k)},has(t,k){log.push('has '+String(k));return Reflect.has(t,k)},getPrototypeOf(t){log.push('gpo');return Reflect.getPrototypeOf(t)}});Object.prototype.toString.call(o);return log.join()"),
  L("var o={get [Symbol.toStringTag](){throw new RangeError('tag')}};return Object.prototype.toString.call(o)"), L("var o=new Proxy({},{get(){throw new RangeError('tag')}});return Object.prototype.toString.call(o)"),
  L("var r=Proxy.revocable([],{});r.revoke();return Object.prototype.toString.call(r.proxy)"), L("var r=Proxy.revocable({},{});r.revoke();return Object.prototype.toString.call(r.proxy)"),
  L("var r=Proxy.revocable(function(){},{});r.revoke();return Object.prototype.toString.call(r.proxy)"), L("var r=Proxy.revocable(function(){},{});r.revoke();return typeof r.proxy"),
  L("var r=Proxy.revocable([],{});var p=new Proxy(r.proxy,{});r.revoke();return Object.prototype.toString.call(p)"),
  L("return Object.prototype.toString.call(new Proxy(new Proxy([],{}),{}))"), L("return Object.prototype.toString.call(new Proxy(new Date(0),{}))"), L("return Object.prototype.toString.call(new Proxy(new Error,{}))"),
  L("return Object.prototype.toString.call(new Proxy(new Map,{}))"), L("return Object.prototype.toString.call(new Proxy(/x/,{}))"), L("return Object.prototype.toString.call(new Proxy(new Boolean(1),{}))"),
  L("return Object.prototype.toString.call(new Proxy((function(){return arguments})(),{}))"), L("return Object.prototype.toString.call(new Proxy(class{},{}))"), L("return Object.prototype.toString.call(new Proxy(async function(){},{}))"),
  L("return Object.prototype.toString.call(new Proxy(function*(){},{}))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf([],null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(function(){},null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(new Date,null))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf(new Error,null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(new Map,null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(new Boolean(1),null))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf(new Number(1),null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(new String(''),null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(/x/,null))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf(Object(Symbol()),null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(Object(1n),null))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf((function(){return arguments})(),null))"), L("return Object.prototype.toString.call(Object.setPrototypeOf(Promise.resolve(),null))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf(new Uint8Array(1),Array.prototype))"), L("return Object.prototype.toString.call(Object.setPrototypeOf({},Array.prototype))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf({},Date.prototype))"), L("return Object.prototype.toString.call(Object.setPrototypeOf({},Map.prototype))"),
  L("return Object.prototype.toString.call(Object.setPrototypeOf({},Error.prototype))"), L("return Object.prototype.toString.call(Object.setPrototypeOf([],Date.prototype))"),
  L("class A extends Array{};return Object.prototype.toString.call(new A)"), L("class A extends Map{};return Object.prototype.toString.call(new A)"), L("class A extends Error{};return Object.prototype.toString.call(new A)"),
  L("class A extends Promise{};return Object.prototype.toString.call(new A(()=>{}))"), L("class A extends Function{};return Object.prototype.toString.call(new A)"), L("class A extends Boolean{};return Object.prototype.toString.call(new A)"),
  L("class A extends Date{};return Object.prototype.toString.call(new A)"), L("class A extends RegExp{};return Object.prototype.toString.call(new A)"), L("class A extends Uint8Array{};return Object.prototype.toString.call(new A(1))"),
  L("class A{get [Symbol.toStringTag](){return 'AA'}};return Object.prototype.toString.call(new A)"), L("class A{static get [Symbol.toStringTag](){return 'SS'}};return Object.prototype.toString.call(A)"),
  L("class A{};return Object.prototype.toString.call(A)+Object.prototype.toString.call(new A)"), L("return Object.prototype.toString.call(new (class extends Object{}))"),
  L("return Object.prototype.toString.call(function(){return arguments}.call())"), L("var a=(function(){return arguments})();a[Symbol.toStringTag]='X';return Object.prototype.toString.call(a)"),
  L("var a=(function(){return arguments})();Object.defineProperty(a,Symbol.toStringTag,{value:'X'});return Object.prototype.toString.call(a)"),
  L("var f=function(){};f[Symbol.toStringTag]='ff';return Object.prototype.toString.call(f)"), L("var d=new Date(0);d[Symbol.toStringTag]='dd';return Object.prototype.toString.call(d)"),
  L("var d=new Error;d[Symbol.toStringTag]='dd';return Object.prototype.toString.call(d)"), L("var d=/x/;d[Symbol.toStringTag]='dd';return Object.prototype.toString.call(d)"),
  L("var d=new Boolean(1);d[Symbol.toStringTag]='dd';return Object.prototype.toString.call(d)"), L("return Object.prototype.toString.call(Object.freeze({[Symbol.toStringTag]:'F'}))"),
  L("return Object.prototype.toString.call({__proto__:null,[Symbol.toStringTag]:'NP'})"), L("return Object.prototype.toString.call({[Symbol.toStringTag]:'a'.repeat(3)})"),
  L("return Object.prototype.toString.call({[Symbol.toStringTag]:'\\u0000'}).length"), L("return Object.prototype.toString.call({[Symbol.toStringTag]:'\\n'}).length"),
  L("return Object.prototype.toString.call({[Symbol.toStringTag]:'\\u{1F600}'})"), L("return Object.prototype.toString.call({[Symbol.toStringTag]:'\\ud800'}).length"),
  L("return Object.prototype.toString.call({[Symbol.toStringTag]:'[object X]'})"), L("return Object.prototype.toString.call({[Symbol.toStringTag]:'a b'})"),
  L("return Object.prototype.toString.call()"), L("return Object.prototype.toString.length+Object.prototype.toString.name"), L("return Object.prototype.toString.call(Object.prototype.toString)"),
  L("return Object.prototype.toString.apply(1)+Object.prototype.toString.apply('s')+Object.prototype.toString.apply(true)"), L("return Object.prototype.toString.bind(null)()"), L("return Object.prototype.toString.bind(undefined)()"),
  L("return String(Object.prototype.toString.call.call(Object.prototype.toString,[]))"), L("return ({}).toString.call(Symbol.prototype)+({}).toString.call(Symbol())"),
  L("return ({}+'')+([]+'')+(String(function(){}).length>0)+(String(Symbol('a')))"), L("return `${{}}`+`${[]}`+`${{[Symbol.toStringTag]:'Q'}}`"), L("return ({[Symbol.toStringTag]:'Q'})+''"),
  L("return String({[Symbol.toStringTag]:'Q',toString:undefined})"), L("return String({[Symbol.toStringTag]:'Q',toString:null,valueOf(){return 3}})"),
  L("return D(Object,'prototype')"), L("return D(Symbol,'toStringTag')"), L("return Symbol.toStringTag.toString()+Symbol.toStringTag.description"),
  L("return Object.getOwnPropertySymbols(Math).map(String).join()+Object.getOwnPropertySymbols(JSON).map(String).join()+Object.getOwnPropertySymbols(Reflect).map(String).join()+Object.getOwnPropertySymbols(Atomics).map(String).join()"),
  L("return Object.getOwnPropertySymbols(globalThis).map(String).join()"), L("return Object.getOwnPropertySymbols(Intl).map(String).join()"), L("return Object.getOwnPropertySymbols(Promise.prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(Map.prototype).map(String).join()"), L("return Object.getOwnPropertySymbols(Set.prototype).map(String).join()"), L("return Object.getOwnPropertySymbols(Symbol.prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(Object.getPrototypeOf(Uint8Array.prototype)).map(String).join()"), L("return Object.getOwnPropertySymbols(Array.prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(Iterator.prototype).map(String).join()"), L("return Object.getOwnPropertySymbols(Object.getPrototypeOf(function*(){}).prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(Date.prototype).map(String).join()"), L("return Object.getOwnPropertySymbols(RegExp.prototype).map(String).join()"), L("return Object.getOwnPropertySymbols(String.prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(ArrayBuffer.prototype).map(String).join()+Object.getOwnPropertySymbols(DataView.prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(WeakMap.prototype).map(String).join()+Object.getOwnPropertySymbols(WeakSet.prototype).map(String).join()+Object.getOwnPropertySymbols(WeakRef.prototype).map(String).join()"),
  L("return Object.getOwnPropertySymbols(Function.prototype).map(String).join()+Object.getOwnPropertySymbols(Error.prototype).map(String).join()"),
);

// ---- D. toLocaleString.
const lsThis = ["undefined", "null", "1", "-0", "'s'", "true", "1n", "Symbol('q')", "{}", "[]", "[1,[2,3]]", "function(){}", "new Date(NaN)", "/x/g", "new Error('e')", "new Number(5)", "Object.create(null)", "{toString(){return 'T'}}", "{toString:1}", "{toString:undefined}", "new Proxy({},{})", "new Proxy({toString(){return 'PT'}},{})", "Object.create({toString(){return 'inh'}})", "{get toString(){return function(){return 'gT'}}}", "{get toString(){throw new RangeError('g')}}", "{toString(){throw new RangeError('t')}}", "{toString(){return {}}}", "{toString(){return Symbol()}}", "{toString(){return 1}}", "{toString(){return this}}"];
for (const v of lsThis) {
  add(
    E(`Object.prototype.toLocaleString.call(${v})`),
    L(`return typeof Object.prototype.toLocaleString.call(${v})`),
    L(`var o=Object(${v});return Object.prototype.toLocaleString.call(o)===undefined`),
  );
}
add(
  L("var t;var o={toString(){t=typeof this;return 'x'}};Object.prototype.toLocaleString.call(o);return t"),
  L("var t,a;Object.defineProperty(Number.prototype,'toString',{value:function(){'use strict';t=typeof this;a=arguments.length;return 'n'},configurable:true,writable:true});var r=Object.prototype.toLocaleString.call(1)+t+a;delete Number.prototype.toString;return r"),
  L("var t;Object.defineProperty(String.prototype,'toString',{value:function(){'use strict';t=typeof this;return 'ss'},configurable:true,writable:true});var r=Object.prototype.toLocaleString.call('a')+t;delete String.prototype.toString;return r"),
  L("var t;Object.defineProperty(Boolean.prototype,'toString',{value:function(){t=typeof this;return 'bb'},configurable:true,writable:true});var r=Object.prototype.toLocaleString.call(true)+t;delete Boolean.prototype.toString;return r"),
  L("var t;Object.defineProperty(Symbol.prototype,'toString',{value:function(){'use strict';t=typeof this;return 'sy'},configurable:true,writable:true});var r=Object.prototype.toLocaleString.call(Symbol())+t;delete Symbol.prototype.toString;return r"),
  L("var t;Object.defineProperty(BigInt.prototype,'toString',{value:function(){'use strict';t=typeof this;return 'bi'},configurable:true,writable:true});var r=Object.prototype.toLocaleString.call(1n)+t;delete BigInt.prototype.toString;return r"),
  L("var log=[];var o=new Proxy({toString(){return 'p'}},{get(t,k){log.push(String(k));return Reflect.get(t,k)}});Object.prototype.toLocaleString.call(o);return log.join()"),
  L("var log=[];var o={get toString(){log.push('get');return function(){log.push('call');return 'x'}},toLocaleString:undefined};Object.prototype.toLocaleString.call(o);return log.join()"),
  L("var o={toString(){return 'S'},toLocaleString:Object.prototype.toLocaleString};return o.toLocaleString()"), L("return Object.prototype.toLocaleString.length+Object.prototype.toLocaleString.name"),
  L("return D(Object.prototype,'toLocaleString').replace(/v=fn/,'v')"), L("new Object.prototype.toLocaleString"), L("return Object.prototype.toLocaleString.call()"), L("return Object.prototype.toLocaleString.call(1,2,3)"),
  L("return Object.prototype.toLocaleString.call([1,2])"), L("return Object.prototype.toLocaleString.call(Math)"), L("return Object.prototype.toLocaleString.call(new Map)"), L("return Object.prototype.toLocaleString.call(Symbol.prototype)"),
  L("return Object.prototype.toLocaleString.call(function f(){})===String(function f(){})"), L("return Object.prototype.toLocaleString.call(new Date(0))===new Date(0).toString()"),
  L("return Object.prototype.toLocaleString.call(new Date(0))===new Date(0).toLocaleString()"), L("return Object.prototype.toLocaleString.call([1,2])===[1,2].toLocaleString()"),
  L("var o={toString:function(){return 'a'}};return Object.keys(o).length+Object.prototype.toLocaleString.call(o)"), L("return [].toLocaleString.call({length:2,0:'a',1:{toLocaleString(){return 'b'}}})"),
  L("return [null,undefined,1,{toLocaleString(){return 'q'}}].toLocaleString()"), L("return [1234.5,new Date(0).getTime()].toLocaleString('en-US')"), L("return (1234.5).toLocaleString('en-US')+(1234.5).toLocaleString('de-DE')"),
  L("return 'ab'.toLocaleString===Object.prototype.toLocaleString"), L("return Number.prototype.toLocaleString===Object.prototype.toLocaleString"), L("return [Array,Date,Number,BigInt,String,Boolean,Symbol,Function,Error,RegExp,Map,Promise,Object].map(c=>c.prototype.hasOwnProperty('toLocaleString')).join()"),
  L("return [Uint8Array.prototype,Object.getPrototypeOf(Uint8Array.prototype)].map(p=>p.hasOwnProperty('toLocaleString')).join()"),
);

// ---- E. valueOf em primitivos.
const vo = ["undefined", "null", "true", "false", "0", "-0", "1", "NaN", "Infinity", "''", "'s'", "'abc'", "1n", "0n", "Symbol()", "Symbol.iterator", "Symbol.for('x')", "{}", "[]", "function(){}", "new Number(2)", "new String('x')", "new Boolean(false)", "Object(1n)", "Object(Symbol())", "new Proxy({},{})", "Object.create(null)", "new Date(0)", "/x/", "new Map", "new Uint8Array(1)"];
for (const v of vo) {
  add(
    L(`var r=Object.prototype.valueOf.call(${v});return typeof r+' '+(r===${v})+' '+Object.prototype.toString.call(r)`),
    L(`var r=Object.prototype.valueOf.call(${v});return (r instanceof Object)+' '+(Object(r)===r)`),
    L(`var x=${v};var r=Object.prototype.valueOf.call(x);return String(Object.is(Object.prototype.valueOf.call(x),Object.prototype.valueOf.call(x)))`),
    L(`var x=${v};return typeof Object(x)+' '+(Object(x)===x)+' '+Object.prototype.toString.call(Object(x))`),
    L(`var x=${v};var o=new Object(x);return typeof o+' '+(o===x)+' '+Object.getPrototypeOf(o)===undefined`),
    L(`return typeof Object.prototype.valueOf.call(Object(${v}))`),
  );
}
add(
  L("return Object.prototype.valueOf.length+Object.prototype.valueOf.name"), L("return Object.prototype.valueOf.call()"), L("new Object.prototype.valueOf"), L("return D(Object.prototype,'valueOf').replace(/v=fn/,'v')"),
  L("var t;Object.defineProperty(Number.prototype,'zz1',{get(){'use strict';t=typeof this;return 1},configurable:true});(5).zz1;delete Number.prototype.zz1;return t"),
  L("var t;Object.defineProperty(Number.prototype,'zz1',{get(){t=typeof this;return 1},configurable:true});(5).zz1;delete Number.prototype.zz1;return t"),
  L("var t;Number.prototype.zz2=function(){'use strict';t=[typeof this,this];};(5).zz2();return t.join()"), L("var t;Number.prototype.zz3=function(){t=[typeof this,this.valueOf()];};(5).zz3();return t.join()"),
  L("String.prototype.zz4=function(){'use strict';return typeof this};return 'a'.zz4()"), L("String.prototype.zz5=function(){return typeof this+(this instanceof String)};return 'a'.zz5()"),
  L("Boolean.prototype.zz6=function(){'use strict';return this};return true.zz6()"), L("Boolean.prototype.zz7=function(){return this};return typeof false.zz7()"),
  L("Symbol.prototype.zz8=function(){'use strict';return typeof this};return Symbol().zz8()"), L("BigInt.prototype.zz9=function(){'use strict';return typeof this};return (1n).zz9()"),
  L("Object.prototype.zza=function(){'use strict';return typeof this};return [1,'s',true,1n,Symbol()].map(v=>v.zza()).join()"),
  L("Object.prototype.zzb=function(){return typeof this};return [1,'s',true,1n,Symbol()].map(v=>v.zzb()).join()"),
  L("Object.prototype.zzc=function(){'use strict';return this};return [1,'s',true].map(v=>v.zzc()).join()"), L("Object.prototype.zzd=function(){'use strict';return this};null.zzd()"),
  L("Object.prototype.zze=function(){return this===globalThis};return undefined===void 0"), L("return (function(){return typeof this}).call(1)+(function(){'use strict';return typeof this}).call(1)"),
  L("return Object.prototype.hasOwnProperty.call(1,'toFixed')+' '+Object.prototype.hasOwnProperty.call('s','0')+' '+Object.prototype.hasOwnProperty.call('s','length')+' '+Object.prototype.hasOwnProperty.call(1n,'x')"),
  L("return Object.keys(Object(1n)).length+Object.getOwnPropertyNames(Object('ab')).join()+Object.getOwnPropertyNames(Object(1)).length+Object.getOwnPropertyNames(Object(Symbol())).length"),
  L("return Object.getOwnPropertyNames(Object('')).join()+Object.getOwnPropertySymbols(Object('s')).length"), L("return Reflect.ownKeys(Object('ab')).join()"), L("return Object.getOwnPropertyDescriptor(Object('ab'),'0').writable"),
  L("return D(Object('ab'),'length')"), L("return D(Object('ab'),'1')"), L("return D(Object('ab'),'2')"), L("return D('ab','0')"), L("return D('ab','length')"), L("return D(1,'x')"),
  L("var s=Object('ab');s[0]='z';return s[0]"), L("var s=Object('ab');s.x=1;return Reflect.ownKeys(s).join()"), L("var s=Object('ab');s[5]=1;return Reflect.ownKeys(s).join()+s.length"), L("var s=Object('ab');delete s[0]"),
  L("var s=Object('ab');return Object.freeze(s)===s && Object.isFrozen(s)"), L("return Object.isFrozen('ab')+' '+Object.isSealed(1)+' '+Object.isExtensible(1)+' '+Object.isFrozen(Symbol())"),
  L("return Object.freeze(1)+' '+Object.seal('s')+' '+Object.preventExtensions(true)+' '+typeof Object.freeze(Symbol())"), L("return Object.freeze(1n)"),
  L("return Object.getPrototypeOf(1)===Number.prototype && Object.getPrototypeOf('s')===String.prototype && Object.getPrototypeOf(true)===Boolean.prototype && Object.getPrototypeOf(1n)===BigInt.prototype && Object.getPrototypeOf(Symbol())===Symbol.prototype"),
  L("return Object.getPrototypeOf(null)"), L("return Object.getPrototypeOf(undefined)"), L("return Object.getPrototypeOf()"), L("return Object.setPrototypeOf(1,null)"), L("return Object.setPrototypeOf(undefined,null)"),
  L("return Object.setPrototypeOf(1,{})"), L("return Object.setPrototypeOf('s',{})"), L("return Object.setPrototypeOf(1)"), L("return Object.setPrototypeOf({})"), L("return Object.setPrototypeOf()"),
);

// ---- F. isPrototypeOf e propertyIsEnumerable com Proxy.
const protoCases = ["null", "undefined", "1", "'s'", "true", "Symbol()", "1n", "{}", "[]", "function(){}", "Object.create(null)", "new Proxy({},{})", "new Proxy([],{})", "Object.create(Object.create({}))"];
for (const v of protoCases) {
  add(
    L(`return Object.prototype.isPrototypeOf.call(Object.prototype,${v})`), L(`return Object.prototype.isPrototypeOf.call(null,${v})`), L(`return Object.prototype.isPrototypeOf.call(undefined,${v})`),
    L(`return Object.prototype.isPrototypeOf.call(1,${v})`), L(`return Array.prototype.isPrototypeOf(${v})`), L(`return Function.prototype.isPrototypeOf(${v})`),
    L(`return ({}).isPrototypeOf(${v})`), L(`var p={};return p.isPrototypeOf(Object.create(p,{x:{value:${v}}}))`),
  );
}
add(
  L("var log=[];var p=new Proxy({},{getPrototypeOf(t){log.push('gpo');return Array.prototype}});var r=Array.prototype.isPrototypeOf(p);return r+' '+log.join()"),
  L("var log=[];var p=new Proxy({},{getPrototypeOf(t){log.push('gpo');return Array.prototype}});var r=Object.prototype.isPrototypeOf(p);return r+' '+log.join()"),
  L("var log=[];var p=new Proxy({},{getPrototypeOf(t){log.push('gpo');return null}});var r=Object.prototype.isPrototypeOf(p);return r+' '+log.join()"),
  L("var log=[];var base={};var mid=new Proxy(Object.create(base),{getPrototypeOf(t){log.push('gpo');return Reflect.getPrototypeOf(t)}});var o=Object.create(mid);return base.isPrototypeOf(o)+' '+log.join()"),
  L("var log=[];var o=Object.create(new Proxy({},{getPrototypeOf(t){log.push('gpo');return Array.prototype}}));return Array.prototype.isPrototypeOf(o)+' '+log.join()"),
  L("var p=new Proxy({},{getPrototypeOf(){throw new RangeError('gpo')}});return Object.prototype.isPrototypeOf(p)"), L("var p=new Proxy({},{getPrototypeOf(){return 1}});return Object.prototype.isPrototypeOf(p)"),
  L("var r=Proxy.revocable({},{});r.revoke();return Object.prototype.isPrototypeOf(r.proxy)"), L("var r=Proxy.revocable({},{});r.revoke();return r.proxy.isPrototypeOf({})"),
  L("var r=Proxy.revocable({},{});r.revoke();return Object.prototype.isPrototypeOf.call(r.proxy,{})"), L("var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();return Object.prototype.isPrototypeOf.call({},o)"),
  L("var r=Proxy.revocable({},{});var o=Object.create(r.proxy);r.revoke();return Object.prototype.isPrototypeOf.call(r.proxy,o)"),
  L("var t={};var p=new Proxy(t,{});return t.isPrototypeOf(p)+' '+p.isPrototypeOf(t)+' '+Object.prototype.isPrototypeOf.call(p,t)"),
  L("var t={};var p=new Proxy(t,{});var o=Object.create(p);return t.isPrototypeOf(o)+' '+p.isPrototypeOf(o)"),
  L("var t={};var p=new Proxy(t,{});var o=Object.create(t);return p.isPrototypeOf(o)"),
  L("var p=new Proxy({},{});return Object.prototype.isPrototypeOf(p)+' '+Object.prototype.isPrototypeOf(Object.create(p))"),
  L("var p=new Proxy(function(){},{});return Function.prototype.isPrototypeOf(p)+' '+Object.prototype.isPrototypeOf(p)"), L("return Object.prototype.isPrototypeOf(Object.prototype)"), L("return Object.prototype.isPrototypeOf.call(Object.prototype,Object.create(null))"),
  L("var a={},b=Object.create(a),c=Object.create(b);return a.isPrototypeOf(c)+' '+c.isPrototypeOf(a)+' '+b.isPrototypeOf(c)+' '+a.isPrototypeOf(a)"),
  L("var log=[];var o={valueOf(){log.push('vo');return 1},toString(){log.push('ts');return 'x'}};Object.prototype.isPrototypeOf.call(o,1);Object.prototype.isPrototypeOf.call(o,{});return log.join()"),
  L("var o=Object.create(null);return Object.prototype.isPrototypeOf.call(Object.prototype,o)+' '+Object.prototype.isPrototypeOf.call(o,{})"), L("return Object.prototype.isPrototypeOf.length+Object.prototype.isPrototypeOf.name"),
  L("return Object.prototype.isPrototypeOf.call()"), L("return Object.prototype.isPrototypeOf.call({})"), L("return Object.prototype.isPrototypeOf.call(null)"), L("return Object.prototype.isPrototypeOf.call(undefined)"),
  L("return Number.prototype.isPrototypeOf(Object(1))+' '+Number.prototype.isPrototypeOf(1)+' '+Object.prototype.isPrototypeOf(1)"), L("return Symbol.prototype.isPrototypeOf(Object(Symbol()))+' '+BigInt.prototype.isPrototypeOf(Object(1n))"),
  L("return Object.prototype.isPrototypeOf.call('s',Object('s'))+' '+Object.prototype.isPrototypeOf.call(String.prototype,Object('s'))"),
  L("return Iterator.prototype.isPrototypeOf([].values())+' '+Iterator.prototype.isPrototypeOf((function*(){})())+' '+Iterator.prototype.isPrototypeOf(new Map().keys())"),
  L("return Error.prototype.isPrototypeOf(new TypeError)+' '+TypeError.prototype.isPrototypeOf(new RangeError)+' '+Function.prototype.isPrototypeOf(class{})"),
);
const peKeys = ["'a'", "'0'", "0", "'length'", "Symbol.iterator", "Symbol('s')", "'toString'", "'__proto__'", "{toString(){return 'a'}}", "null", "undefined", "1n", "-0", "'-0'", "'1e0'"];
for (const k of peKeys) {
  add(
    L(`var o={a:1,0:2,[Symbol.iterator]:3};return Object.prototype.propertyIsEnumerable.call(o,${k})`),
    L(`var o=Object.defineProperties({},{a:{value:1},0:{value:2},[Symbol.iterator]:{value:3}});return Object.prototype.propertyIsEnumerable.call(o,${k})`),
    L(`return Object.prototype.propertyIsEnumerable.call('ab',${k})`), L(`return Object.prototype.propertyIsEnumerable.call([7],${k})`), L(`return Object.prototype.propertyIsEnumerable.call(new Uint8Array(1),${k})`),
    L(`return Object.prototype.propertyIsEnumerable.call(Object.create({a:1,0:1}),${k})`), L(`return Object.prototype.propertyIsEnumerable.call(Object.prototype,${k})`),
    L(`var p=new Proxy({a:1,0:1},{});return p.propertyIsEnumerable(${k})`),
    L(`var log=[];var p=new Proxy({a:1},{getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},has(t,k){log.push('has');return true},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});var r=Object.prototype.propertyIsEnumerable.call(p,${k});return r+' '+log.join()`),
  );
}
add(
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {value:1,enumerable:true,configurable:true}}});return Object.prototype.propertyIsEnumerable.call(p,'z')"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {value:1,enumerable:false,configurable:true}}});return Object.prototype.propertyIsEnumerable.call(p,'z')"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return undefined}});return Object.prototype.propertyIsEnumerable.call(p,'z')"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return {value:1,enumerable:true}}});return Object.prototype.propertyIsEnumerable.call(p,'z')"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){return 1}});return Object.prototype.propertyIsEnumerable.call(p,'z')"), L("var p=new Proxy({},{getOwnPropertyDescriptor(t,k){throw new RangeError('x')}});return p.propertyIsEnumerable('z')"),
  L("var p=new Proxy({a:1},{getOwnPropertyDescriptor(t,k){return undefined}});return p.propertyIsEnumerable('a')"), L("var p=new Proxy(Object.preventExtensions({a:1}),{getOwnPropertyDescriptor(t,k){return undefined}});return p.propertyIsEnumerable('a')"),
  L("var p=new Proxy(Object.freeze({a:1}),{getOwnPropertyDescriptor(t,k){return {value:1,enumerable:false,configurable:true}}});return p.propertyIsEnumerable('a')"),
  L("var r=Proxy.revocable({a:1},{});r.revoke();return Object.prototype.propertyIsEnumerable.call(r.proxy,'a')"),
  L("var log=[];var k={toString(){log.push('key');return 'a'}};try{Object.prototype.propertyIsEnumerable.call(null,k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');return 'a'}};try{Object.prototype.propertyIsEnumerable.call(undefined,k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');throw new RangeError('k')}};try{Object.prototype.propertyIsEnumerable.call(null,k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');throw new RangeError('k')}};try{Object.prototype.propertyIsEnumerable.call({},k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var o={get a(){log.push('getter');return 1}};Object.prototype.propertyIsEnumerable.call(o,'a');return log.join()"),
  L("var o={};Object.defineProperty(o,'a',{get(){return 1},enumerable:true});return o.propertyIsEnumerable('a')"), L("var o={};Object.defineProperty(o,'a',{set(v){},enumerable:false});return o.propertyIsEnumerable('a')"),
  L("var a=[1];Object.defineProperty(a,0,{enumerable:false});return a.propertyIsEnumerable(0)+' '+Object.keys(a).length"), L("var a=[,1];return a.propertyIsEnumerable(0)+' '+a.propertyIsEnumerable(1)"),
  L("var f=function(){};return ['length','name','prototype','caller','arguments'].map(k=>f.propertyIsEnumerable(k)).join()"), L("return Object.prototype.propertyIsEnumerable.length+Object.prototype.propertyIsEnumerable.name"),
  L("return Object.prototype.propertyIsEnumerable.call()"), L("return Object.prototype.propertyIsEnumerable.call({})"), L("return ({a:1}).propertyIsEnumerable()"), L("return ({undefined:1}).propertyIsEnumerable()"),
  L("return globalThis.propertyIsEnumerable('Array')+' '+globalThis.propertyIsEnumerable('globalThis')+' '+globalThis.propertyIsEnumerable('undefined')"), L("return Math.propertyIsEnumerable('PI')+' '+JSON.propertyIsEnumerable(Symbol.toStringTag)"),
  L("class A{static x=1;static m(){}};return A.propertyIsEnumerable('x')+' '+A.propertyIsEnumerable('m')+' '+A.prototype.propertyIsEnumerable('constructor')"),
  L("var a=(function(){return arguments})(1);return a.propertyIsEnumerable('0')+' '+a.propertyIsEnumerable('length')+' '+a.propertyIsEnumerable('callee')+' '+a.propertyIsEnumerable(Symbol.iterator)"),
  L("var e=new Error('m');return e.propertyIsEnumerable('message')+' '+e.propertyIsEnumerable('stack')+' '+e.propertyIsEnumerable('cause')"), L("var e=new Error('m',{cause:1});return e.propertyIsEnumerable('cause')"),
  L("var m=/x/g.exec('x');return ['0','index','input','groups','length'].map(k=>m.propertyIsEnumerable(k)).join()"), L("var r=/x/;return r.propertyIsEnumerable('lastIndex')"),
);

// ---- G. Object.assign e spread com getters e símbolos.
const asSrc = {
  getters: "{get a(){log.push('ga');return 1},get b(){log.push('gb');return 2}}", symbols: "{[Symbol.for('s1')]:1,a:2,[Symbol.for('s2')]:3}",
  nonenum: "Object.defineProperties({},{a:{value:1,enumerable:false},b:{value:2,enumerable:true},[Symbol.for('h')]:{value:3,enumerable:false},[Symbol.for('v')]:{value:4,enumerable:true}})",
  order: "{b:1,2:1,a:1,1:1,[Symbol.for('z')]:1,'-1':1,'01':1,'4294967294':1,'4294967295':1}", inherited: "Object.create({inh:1},{own:{value:2,enumerable:true}})",
  str: "'abc'", emptystr: "''", num: "5", bool: "true", sym: "Symbol('q')", big: "1n", nul: "null", und: "undefined", arr: "['x','y']", sparse: "[1,,3]", strobj: "new String('ab')",
  u8: "new Uint8Array([7,8])", args: "(function(){return arguments})(1,2)", fn: "Object.assign(function(){},{a:1})", map: "new Map([[1,2]])", date: "Object.assign(new Date(0),{x:1})",
  err: "new Error('m',{cause:3})", proxy: "new Proxy({a:1,b:2,[Symbol.for('p')]:3},{})", nullproto: "Object.assign(Object.create(null),{a:1,b:2})",
  protokey: "JSON.parse('{\"__proto__\":{\"zz\":1},\"a\":1}')", setter: "{set a(v){log.push('set')},b:1}", delself: "{get a(){delete this.b;return 1},b:2,c:3}", addself: "{get a(){this.z=9;return 1},b:2}",
  throwing: "{a:1,get b(){throw new RangeError('gb')},c:3}", frozensrc: "Object.freeze({a:1,[Symbol.for('f')]:2})", boxedsym: "Object(Symbol('b'))", boxednum: "Object(7)",
};
const asTgt = {
  plain: "{}", withset: "{set a(v){log.push('tset '+v)},set b(v){log.push('tsetb '+v)}}", frozen: "Object.freeze({})", frozena: "Object.freeze({a:0})", ro: "Object.defineProperty({},'a',{value:0,writable:false})",
  getonly: "{get a(){return 0}}", noext: "Object.preventExtensions({})", arr: "[]", arr2: "[9,9,9]", str: "new String('xyz')", num: "Object(1)", fn: "function(){}", nullproto: "Object.create(null)",
  proxy: "new Proxy({},{set(t,k,v,r){log.push('pset '+String(k));return Reflect.set(t,k,v,r)},defineProperty(t,k,d){log.push('pdef '+String(k));return Reflect.defineProperty(t,k,d)}})",
  rejectproxy: "new Proxy({},{set(){return false}})", u8: "new Uint8Array(2)", withproto: "Object.create({set a(v){log.push('psetter')}})", rop: "Object.create(Object.freeze({a:0}))",
};
for (const [sn, s] of Object.entries(asSrc)) {
  add(
    L(`var log=[];var r=Object.assign({},${s});return S(r)+' '+log.join()`),
    L(`var log=[];var r={...${s}};return S(r)+' '+log.join()`),
    L(`var log=[];var r=Object.assign(Object.create(null),${s});return Reflect.ownKeys(r).map(String).join()+' '+log.join()`),
    L(`var log=[];var r=Object.assign({},${s},${s});return Reflect.ownKeys(r).map(String).join()+' '+log.join()`),
    L(`var log=[];var r=Object.assign([],${s});return S(r)+' '+r.length+' '+log.join()`),
    L(`var log=[];var r=Object.assign(function(){},${s});return Reflect.ownKeys(r).map(String).join()+' '+log.join()`),
    L(`var log=[];var r=Object.assign('t',${s});return typeof r+' '+S(r)`),
    L(`var log=[];var r={a:'orig',...${s},z:'last'};return Reflect.ownKeys(r).map(String).join()+' '+log.join()`),
    L(`var log=[];var r={...${s}};return Object.keys(r).map(k=>D(r,k)).join('|')`),
    L(`var log=[];var r=[...[1],...Object.values(Object.assign({},${s}))];return r.length`),
  );
}
for (const [tn, t] of Object.entries(asTgt)) {
  for (const sn of ["getters", "symbols", "nonenum", "order", "arr", "str", "setter", "throwing", "u8", "proxy"]) {
    add(L(`var log=[];var t=${t};var r=Object.assign(t,${asSrc[sn]});return Reflect.ownKeys(t).map(String).join()+' '+log.join()+' '+(r===t)`));
  }
}
add(
  L("return Object.assign()"), L("return Object.assign(null)"), L("return Object.assign(undefined,{})"), L("return typeof Object.assign(1)"), L("return typeof Object.assign('s',null)"), L("return Object.assign({},null,undefined,1,true,Symbol())"),
  L("return S(Object.assign({},null,'ab',null,[1]))"), L("return S(Object.assign({}))"), L("var o={};return Object.assign(o)===o"), L("return Object.assign.length+Object.assign.name"), L("new Object.assign"),
  L("var t=Object.assign(1,{a:1});return typeof t+' '+t.a+' '+(t instanceof Number)"), L("var t=Object.assign(Symbol('x'),{a:1});return typeof t+' '+t.a"), L("var t=Object.assign(1n,{a:1});return typeof t+' '+t.a"),
  L("var t=Object.assign(true,{a:1});return typeof t+' '+t.a"), L("var t=Object.assign('ab',{a:1});return typeof t+' '+t.a+' '+t.length"), L("return Object.assign('ab',{0:'z'})"), L("return Object.assign(Object('ab'),{0:'z'})"),
  L("var log=[];var s=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});Object.assign({},s);return log.join()"),
  L("var log=[];var s=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});var r={...s};return log.join()"),
  L("var log=[];var s=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)},has(t,k){log.push('has');return true}});var {...r}=s;return log.join()"),
  L("var log=[];var s=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return ['b','a']},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});var r=Object.assign({},s);return Object.keys(r).join()+log.join()"),
  L("var log=[];var s=new Proxy({a:1},{getOwnPropertyDescriptor(t,k){log.push('gopd');return {value:1,enumerable:false,configurable:true}}});var r=Object.assign({},s);return Object.keys(r).length+log.join()"),
  L("var s=new Proxy({a:1},{ownKeys(){throw new RangeError('ok')}});return Object.assign({},s)"), L("var s=new Proxy({a:1},{ownKeys(){throw new RangeError('ok')}});return {...s}"),
  L("var s=new Proxy({a:1},{get(){throw new RangeError('g')}});return Object.assign({},s)"), L("var s=new Proxy({a:1},{get(){throw new RangeError('g')}});return {...s}"),
  L("var r=Proxy.revocable({a:1},{});r.revoke();return Object.assign({},r.proxy)"), L("var r=Proxy.revocable({a:1},{});r.revoke();return {...r.proxy}"), L("var r=Proxy.revocable({},{});r.revoke();return Object.assign(r.proxy,{a:1})"),
  L("var s=new Proxy({a:1},{ownKeys(){return ['a','a']}});return Object.assign({},s)"), L("var s=new Proxy({a:1},{ownKeys(){return [1]}});return Object.assign({},s)"), L("var s=new Proxy({a:1},{ownKeys(){return ['a','b']}});return Object.assign({},s)"),
  L("var s=new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return ['a','b']}});return Object.assign({},s)"), L("var s=new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return []}});return Object.assign({},s)"),
  L("var s={get a(){return 1}};var t={};Object.defineProperty(t,'a',{set(v){this.seen=v},configurable:true});Object.assign(t,s);return t.seen+' '+D(t,'a').slice(0,2)"),
  L("var t=Object.defineProperty({},'a',{value:0,writable:true,enumerable:false,configurable:true});Object.assign(t,{a:1});return D(t,'a')"), L("var t={};var s=Object.defineProperty({},'a',{get(){return 4},enumerable:true});Object.assign(t,s);return D(t,'a')"),
  L("var t={};var s={...Object.defineProperty({},'a',{get(){return 4},enumerable:true})};return D(s,'a')"), L("var s={get a(){return 4}};var t={...s};return D(t,'a')+' '+D(s,'a').slice(0,2)"),
  L("var s=Object.defineProperty({},'a',{value:1,enumerable:true,writable:false,configurable:false});var t={...s};return D(t,'a')"), L("var s=Object.defineProperty([],'length',{writable:false});var t={...s};return S(t)"),
  L("var t={...'ab'};return S(t)"), L("var t={...[1,,2]};return S(t)+Object.keys(t).length"), L("var t={...new Uint8Array([1,2])};return S(t)"), L("var t={...1,...true,...Symbol(),...1n,...null,...undefined};return Reflect.ownKeys(t).length"),
  L("var t={...function(){}};return Reflect.ownKeys(t).length"), L("var t={...class{static s=1}};return Reflect.ownKeys(t).join()"), L("var t={...new Map([[1,2]])};return Reflect.ownKeys(t).length"), L("var t={...new Error('m',{cause:1})};return Reflect.ownKeys(t).join()"),
  L("var t={...(function(){return arguments})(1,2)};return Reflect.ownKeys(t).join()"), L("var t={...Math};return Reflect.ownKeys(t).length"), L("var t={...globalThis};return Reflect.ownKeys(t).length>=0"), L("var t={...Object.create({x:1})};return Reflect.ownKeys(t).length"),
  L("var s1=Symbol('1'),s2=Symbol('2');var t={...{[s2]:1,[s1]:2,b:1,a:2,1:1,0:2}};return Reflect.ownKeys(t).map(String).join()"), L("var s1=Symbol('1'),s2=Symbol('2');var t=Object.assign({},{[s2]:1,[s1]:2,b:1,a:2,1:1,0:2});return Reflect.ownKeys(t).map(String).join()"),
  L("var s=Symbol('s');var t=Object.assign({},{[s]:1});return (s in t)+' '+t[s]+' '+D(t,s)"), L("var s=Symbol('s');var o=Object.defineProperty({},s,{value:1,enumerable:false});return Reflect.ownKeys(Object.assign({},o)).length+Reflect.ownKeys({...o}).length"),
  L("var s=Symbol('s');var o=Object.defineProperty({},s,{get(){return 2},enumerable:true});return Object.assign({},o)[s]+{...o}[s]"), L("var s=Symbol('s');var t={set [s](v){this.q=v}};Object.assign(t,{[s]:5});return t.q+' '+Reflect.ownKeys(t).length"),
  L("var s=Symbol('s');var t={set [s](v){this.q=v}};var r={...t,...{[s]:5}};return S(r.q)+' '+typeof D(r,s)"), L("var t={set a(v){this.q=v}};var r={...t,...{a:5}};return D(r,'a')"),
  L("var t={set a(v){this.q=v}};var r=Object.assign(t,{a:5});return t.q+' '+D(t,'a').slice(0,2)"), L("var t={a:1};var r={...t,get a(){return 2}};return D(r,'a').slice(0,2)"), L("var t={get a(){return 1},...{a:2}};return D(t,'a')"),
  L("var n=0;var r={...{get a(){return ++n}},...{get a(){return ++n}}};return r.a+' '+n"), L("var n=0;var s={get a(){return ++n}};var r=Object.assign({},s,s);return r.a+' '+n"),
  L("var s={get a(){delete this.b;return 1},b:2,c:3};return S(Object.assign({},s))"), L("var s={get a(){delete this.b;return 1},b:2,c:3};return S({...s})"), L("var s={a:1,get b(){this.c=7;return 2},c:3};return S({...s})"),
  L("var s={get a(){Object.defineProperty(this,'b',{enumerable:false});return 1},b:2};return S({...s})+S(Object.assign({},s))"), L("var s={get a(){Object.defineProperty(this,'b',{enumerable:false});return 1},b:2};return S(Object.assign({},s))"),
  L("var s={a:1,get b(){Object.setPrototypeOf(this,{inh:1});return 2}};return S({...s})"), L("var s={a:1,get b(){this.a=100;return 2}};return S({...s})+S(Object.assign({},s))"),
  L("var t={};var s={get a(){t.b='late';return 1},b:2};Object.assign(t,s);return S(t)"), L("var t={a:0};Object.assign(t,{a:1,a2:2},{a:3});return S(t)"),
  L("var t=Object.freeze({a:0});try{Object.assign(t,{a:1})}catch(e){return e.name+': '+e.message}"), L("var t=Object.freeze({a:0});try{Object.assign(t,{b:1})}catch(e){return e.name+': '+e.message}"),
  L("var t=Object.freeze({a:0});try{Object.assign(t,{})}catch(e){return e.name}return 'noerr'"), L("var t=Object.defineProperty({},'a',{value:0});try{Object.assign(t,{a:0})}catch(e){return e.name+': '+e.message}"),
  L("var t={get a(){return 0}};try{Object.assign(t,{a:1})}catch(e){return e.name+': '+e.message}"), L("var t=Object.preventExtensions({});try{Object.assign(t,{a:1})}catch(e){return e.name+': '+e.message}"),
  L("var t=new Proxy({},{set(){return false}});try{Object.assign(t,{a:1})}catch(e){return e.name+': '+e.message}"), L("var t=[1];Object.defineProperty(t,'length',{writable:false});try{Object.assign(t,{1:1})}catch(e){return e.name}return t.length"),
  L("var t=[];Object.assign(t,{length:3,1:'x'});return S(t)"), L("var t=[1,2,3];Object.assign(t,{length:1});return S(t)"), L("var t=new Uint8Array(2);Object.assign(t,{0:300,1:'7',5:1});return S(t)"), L("var t=new Uint8Array(2);Object.assign(t,{length:9});return t.length"),
  L("var t=new String('ab');try{Object.assign(t,{0:'z'})}catch(e){return e.name+': '+e.message}"), L("var t=new String('ab');Object.assign(t,{2:'z'});return S(t)+t.length"), L("var t=function(){};try{Object.assign(t,{name:'n'})}catch(e){return e.name+': '+e.message}"),
  L("var t=function(){};Object.assign(t,{length:5});return t.length"), L("var t=class{};try{Object.assign(t,{prototype:1})}catch(e){return e.name+': '+e.message}"), L("var t=Math;try{Object.assign(t,{PI:3})}catch(e){return e.name+': '+e.message}"),
  L("var t=Object.assign(Object.create({set a(v){log='p'+v}}),{a:1});return typeof t+Object.keys(t).length"), L("var t=Object.create(Object.freeze({a:0}));try{Object.assign(t,{a:1})}catch(e){return e.name+': '+e.message}"),
  L("var t=Object.create(Object.freeze({a:0}));var r={...t,...{a:1}};return S(r)"), L("var t=Object.create(Object.freeze({a:0}));Object.defineProperty(t,'a',{value:1,writable:true});return t.a"),
);

// ---- H. Object.entries / values / keys em strings, arrays esparsos e outros.
const evTargets = {
  str: "'ab'", empty: "''", multi: "'a\\ud83d\\ude00b'", lone: "'\\ud800'", num: "5", bool: "true", sym: "Symbol()", big: "1n", arr: "[1,2,3]", sparse: "[1,,3]", sparse2: "[,,]", sparsebig: "(()=>{var a=[];a[5]=1;a[2]=2;return a})()",
  hugeidx: "(()=>{var a=[];a[4294967294]=1;a[0]=0;return a})()", overidx: "(()=>{var a=[];a[4294967295]=1;a[4294967296]=2;a[1]=3;return a})()", extra: "(()=>{var a=[1,2];a.x=1;a[Symbol.for('s')]=2;a[-1]=3;a['01']=4;return a})()",
  lenonly: "(()=>{var a=[];a.length=3;return a})()", frozen: "Object.freeze([1,2])", u8: "new Uint8Array([1,2])", f64: "new Float64Array([1.5,NaN])", bi64: "new BigInt64Array([1n])", ab: "new ArrayBuffer(2)", dv: "new DataView(new ArrayBuffer(1))",
  strobj: "new String('ab')", strobjx: "Object.assign(new String('ab'),{x:1,5:2,1.5:3})", numobj: "new Number(1)", args: "(function(){return arguments})(1,2)", argsh: "(function(a,b){delete arguments[0];return arguments})(1,2)",
  fn: "function(a,b){}", fnx: "Object.assign(function(){},{a:1})", cls: "class{static a=1;static b(){}}", map: "new Map([[1,2]])", set: "new Set([1])", date: "new Date(0)", re: "/x/g", err: "new Error('m')",
  proxy: "new Proxy({a:1,b:2},{})", proxyarr: "new Proxy([1,2],{})", nullproto: "Object.assign(Object.create(null),{a:1})", inherited: "Object.create({a:1},{b:{value:2,enumerable:true},c:{value:3}})",
  getters: "{get a(){return 'ga'},get b(){throw new RangeError('b')}}", symkeys: "{[Symbol.for('s')]:1,a:2}", order: "{b:1,2:1,a:1,1:1,'-1':1,'1.5':1,'01':1}", numkeys: "{1e21:1,0.5:2,'1e21':3,[-0]:4,[1n]:5}",
  mod: "{a:1,get b(){delete this.c;return 2},c:3,d:4}", mod2: "{a:1,get b(){Object.defineProperty(this,'c',{enumerable:false});return 2},c:3}", mod3: "{get a(){this.zz=1;return 1},b:2}", mod4: "{get a(){delete this.a2;return 1},a2:2}",
  globalish: "Object.create(null,{a:{value:1,enumerable:true},b:{value:2,enumerable:false}})", argsmapped: "(function(a){a=5;return arguments})(1)", boxedsym: "Object(Symbol())", boxedbig: "Object(1n)",
  arrlike: "{length:2,0:'a',1:'b'}", iter: "[][Symbol.iterator]()", gen: "(function*(){})()", promise: "Promise.resolve(1)", wm: "new WeakMap", sab: "new SharedArrayBuffer(1)", intl: "new Intl.NumberFormat",
  nul: "null", und: "undefined",
};
for (const [n, t] of Object.entries(evTargets)) {
  add(
    L(`return S(Object.entries(${t}))`), L(`return S(Object.values(${t}))`), L(`return S(Object.keys(${t}))`),
    L(`return S(Object.getOwnPropertyNames(${t}))`), L(`return S(Object.getOwnPropertySymbols(${t}))`), L(`return S(Reflect.ownKeys(Object(${t})))`),
    L(`return S(Object.getOwnPropertyDescriptors(${t}))`), L(`var r=[];for(var k in ${t})r.push(k);return r.join()`), L(`return S(Object.fromEntries(Object.entries(${t})))`),
    L(`var r=[];for(var [k,v] of Object.entries(${t}))r.push(k+'='+S(v));return r.join()`), L(`return Object.entries(${t}).length+Object.values(${t}).length+Object.keys(${t}).length`),
    L(`return JSON.stringify(Object.entries(${t}))`), L(`return Object.entries(${t}).map(e=>e.length+typeof e[0]).join()`), L(`return Object.isFrozen(Object.entries(${t}))+' '+Array.isArray(Object.entries(${t}))+' '+Object.getPrototypeOf(Object.entries(${t}))===Array.prototype`),
  );
}
add(
  L("var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});Object.entries(p);return log.join()"),
  L("var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});Object.values(p);return log.join()"),
  L("var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});Object.keys(p);return log.join()"),
  L("var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});for(var k in p);return log.join()"),
  L("var log=[];var p=new Proxy({a:1,b:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});Object.fromEntries(Object.entries(p));return log.join()"),
  L("var log=[];var p=new Proxy({a:1,[Symbol.for('s')]:2},{ownKeys(t){log.push('ownKeys');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){log.push('get '+String(k));return Reflect.get(t,k)}});Object.entries(p);return log.join()"),
  L("var p=new Proxy({a:1},{getOwnPropertyDescriptor(){return undefined}});return S(Object.entries(p))+S(Object.keys(p))"), L("var p=new Proxy({a:1},{getOwnPropertyDescriptor(){return {value:1,enumerable:false,configurable:true}}});return S(Object.entries(p))"),
  L("var p=new Proxy({a:1},{ownKeys(){return ['a','b']},getOwnPropertyDescriptor(t,k){return {value:k,enumerable:true,configurable:true}}});return S(Object.entries(p))"),
  L("var p=new Proxy({a:1},{ownKeys(){return ['z']},getOwnPropertyDescriptor(t,k){return {value:k,enumerable:true,configurable:true}},get(t,k){return 'G'+k}});return S(Object.entries(p))"),
  L("var p=new Proxy({},{ownKeys(){return [Symbol.iterator,'a']},getOwnPropertyDescriptor(t,k){return {value:1,enumerable:true,configurable:true}}});return S(Object.keys(p))+S(Object.entries(p))"),
  L("var p=new Proxy({},{ownKeys(){throw new RangeError('ok')}});return Object.entries(p)"), L("var p=new Proxy({a:1},{get(){throw new RangeError('g')}});return Object.entries(p)"), L("var p=new Proxy({a:1},{get(){throw new RangeError('g')}});return Object.keys(p)"),
  L("var p=new Proxy({a:1},{getOwnPropertyDescriptor(){throw new RangeError('d')}});return Object.keys(p)"), L("var r=Proxy.revocable({a:1},{});r.revoke();return Object.keys(r.proxy)"), L("var r=Proxy.revocable({a:1},{});r.revoke();return Object.entries(r.proxy)"),
  L("var r=Proxy.revocable({a:1},{});r.revoke();for(var k in r.proxy);return 1"),
  L("var a=[1,2,3];var r=[];for(var [k,v] of Object.entries(a)){r.push(k+typeof k);if(k==='0')a.push(4)}return r.join()+a.length"), L("var a=[1,2,3];var e=Object.entries(a);a[0]=9;return S(e)"),
  L("var o={a:1,b:2};var e=Object.entries(o);e[0][1]=9;return o.a"), L("var o={a:{x:1}};var e=Object.entries(o);e[0][1].x=9;return o.a.x"), L("var o={a:1};return Object.entries(o)[0]===Object.entries(o)[0]"),
  L("return Object.entries.length+Object.entries.name+Object.values.length+Object.values.name+Object.keys.length+Object.keys.name"), L("new Object.entries({})"), L("return Object.entries()"), L("return Object.values()"), L("return Object.keys()"),
  L("return Object.entries(null)"), L("return Object.values(undefined)"), L("return Object.keys(null)"), L("return Object.getOwnPropertyNames(null)"), L("return Object.getOwnPropertySymbols(undefined)"), L("return Object.getOwnPropertyDescriptors(null)"),
  L("return Object.getOwnPropertyDescriptor(null,'a')"), L("return Object.getOwnPropertyDescriptor(undefined,'a')"), L("return Object.getOwnPropertyDescriptor(1,'a')"), L("return Object.getOwnPropertyDescriptor('s','length').value"),
  L("var o={a:1,get b(){return 2}};return Object.entries(Object.getOwnPropertyDescriptors(o)).map(([k,d])=>k+':'+Object.keys(d).join('/')).join()"),
  L("return Object.entries({a:1}).flat().join()+Object.entries('ab').flat().join()"), L("return Object.entries([,'b']).flat().join()"), L("return Object.entries(new Array(3)).length"), L("return Object.values(new Array(3)).length+' '+Object.keys(new Array(3)).length"),
  L("var a=[1,2,3];a.length=1;return Object.entries(a).length"), L("var a=[1,2,3];delete a[1];return Object.keys(a).join()+Object.values(a).join()+Object.entries(a).join(';')"),
  L("var a=[1,2,3];Object.defineProperty(a,1,{enumerable:false});return Object.keys(a).join()+Object.values(a).join()+Object.getOwnPropertyNames(a).join()"),
  L("var a=[3,2,1];a.sort();return Object.entries(a).join(';')"), L("var a=[];a[1e9]=1;return Object.keys(a).join()"), L("var a=[];a[2**32-2]=1;return Object.keys(a).join()+a.length"), L("var a=[];a[2**32-1]=1;return Object.keys(a).join()+a.length"),
  L("var a=[];a['1.0']=1;a['+1']=2;a['0x1']=3;a[' 1']=4;a['1 ']=5;a['1e0']=6;return Object.keys(a).join('|')+a.length"), L("var a=[];a['-0']=1;a[-0]=2;return Object.keys(a).join('|')+a.length"),
  L("var o={};o[1.5]=1;o[1e21]=2;o[1e-7]=3;o[-1]=4;o[0xff]=5;o[1n]=6;return Object.keys(o).join('|')"), L("var o={};o['b']=1;o[10]=1;o[9]=1;o['a']=1;o[Symbol('s')]=1;o['2']=1;return Object.keys(o).join('|')"),
  L("var o={b:1,a:2};delete o.b;o.b=3;return Object.keys(o).join()"), L("var o={0:1,b:1};o[-1]=1;o[5]=1;return Object.keys(o).join()"), L("var o=Object.create({z:1});o.a=1;return Object.keys(o).join()+Object.values(o).join()+Object.entries(o).join()"),
  L("var s=new String('ab');s[5]='x';s.y=1;return Object.keys(s).join()+Object.values(s).join()+Object.getOwnPropertyNames(s).join()"), L("var s='ab';s.x=1;return s.x"), L("'use strict';var s='ab';s.x=1"), L("'use strict';var s='ab';s[0]='z'"), L("'use strict';var s='ab';s.length=1"),
  L("'use strict';var s='ab';delete s[0]"), L("'use strict';var s='ab';delete s.length"), L("'use strict';var n=1;n.x=1"), L("'use strict';var n=Symbol();n.x=1"), L("'use strict';var n=1n;n.x=1"), L("'use strict';var n=true;n.x=1"),
  L("var s='ab';s.x=1;return s.x+' '+Object.keys(s).join()"), L("var s=1;s.x=1;return s.x"), L("var s='ab';return s[0]+s[1]+s[2]+(2 in Object(s))+(1 in Object(s))"),
  L("var s='\\ud83d\\ude00';return Object.keys(s).join()+Object.entries(s).map(e=>e[1].charCodeAt(0).toString(16)).join()"), L("return Object.values('a\\u0000b').length"),
  L("return Object.entries({a:1}).concat(Object.entries('b')).join(';')"), L("return Object.keys('abc').map(Number).reduce((a,b)=>a+b)"), L("return Object.values('abc').join('')"),
  L("return Object.keys(function(a,b){}).length+Object.keys(class{}).length+Object.keys(()=>1).length+Object.keys(async function(){}).length"), L("return Object.getOwnPropertyNames(function(){}).join()"), L("return Object.getOwnPropertyNames(()=>1).join()"),
  L("return Object.getOwnPropertyNames(async()=>1).join()"), L("return Object.getOwnPropertyNames(function*(){}).join()"), L("return Object.getOwnPropertyNames(class{}).join()"), L("return Object.getOwnPropertyNames(class{static x=1;static m(){}}).join()"),
  L("return Object.getOwnPropertyNames(function(){}.bind()).join()"), L("return Object.getOwnPropertyNames(Math.max).join()"), L("return Object.getOwnPropertyNames(class{constructor(){}}.prototype).join()"), L("return Object.getOwnPropertyNames(function(){'use strict'}).join()"),
  L("return Object.getOwnPropertyNames(function(){}.prototype).join()"), L("return Object.getOwnPropertyNames((function*(){}).prototype).join()+'|'+Object.getOwnPropertyNames(async function*(){}.prototype).join()"),
  L("return Object.getOwnPropertyNames((function(){return arguments})()).join()"), L("return Object.getOwnPropertyNames((function(){'use strict';return arguments})()).join()"),
  L("return Object.getOwnPropertyNames(new Error('m')).join()"), L("return Object.getOwnPropertyNames(new Error).join()"), L("return Object.getOwnPropertyNames(new Error('m',{cause:1})).join()"), L("return Object.getOwnPropertyNames(new AggregateError([],'m')).join()"),
  L("return Object.getOwnPropertyNames(/x/).join()+Object.getOwnPropertyNames(/x/.exec('x')).join()"), L("return Object.getOwnPropertyNames(new Date).join()+Object.getOwnPropertyNames(new Map).join()+Object.getOwnPropertyNames(Promise.resolve()).join()"),
  L("return Object.getOwnPropertyNames([]).join()+Object.getOwnPropertyNames([1]).join()+Object.getOwnPropertyNames(new Uint8Array(2)).join()"),
);

// ---- I. setPrototypeOf / Reflect.setPrototypeOf.
const spTargets = {
  obj: "{}", arr: "[]", fn: "function(){}", cls: "class{}", nullproto: "Object.create(null)", frozen: "Object.freeze({})", sealed: "Object.seal({})", noext: "Object.preventExtensions({})", objproto: "Object.prototype",
  arrproto: "Array.prototype", fnproto: "Function.prototype", u8: "new Uint8Array(1)", date: "new Date(0)", map: "new Map", err: "new Error", re: "/x/", proxy: "new Proxy({},{})",
  proxytrap: "new Proxy({},{setPrototypeOf(t,p){return Reflect.setPrototypeOf(t,p)}})", proxyfalse: "new Proxy({},{setPrototypeOf(){return false}})", proxytrue: "new Proxy({},{setPrototypeOf(){return true}})",
  proxynoext: "new Proxy(Object.preventExtensions({}),{})", proxynoexttrue: "new Proxy(Object.preventExtensions({}),{setPrototypeOf(){return true}})", args: "(function(){return arguments})()",
  boxed: "Object(1)", ab: "new ArrayBuffer(1)", promise: "Promise.resolve()", gen: "(function*(){})()", math: "Math", json: "JSON", reflect: "Reflect", intl: "Intl", wasm: "WebAssembly",
  protoofproto: "Object.getPrototypeOf(Object.prototype)",
};
const spProtos = ["null", "{}", "Object.prototype", "Array.prototype", "Function.prototype", "function(){}", "Object.create(null)", "new Proxy({},{})", "1", "'s'", "true", "Symbol()", "1n", "undefined", "NaN", "[]", "new Date(0)"];
for (const [tn, t] of Object.entries(spTargets)) {
  for (const p of spProtos) {
    add(
      L(`var o=${t};var r=Object.setPrototypeOf(o,${p});return (r===o)+' '+(Object.getPrototypeOf(o)===${p === "undefined" ? "Object.getPrototypeOf(o)" : "r&&Object.getPrototypeOf(r)"})`),
      L(`var o=${t};var p=${p};var r=Reflect.setPrototypeOf(o,p);return r+' '+(Object.getPrototypeOf(o)===p)`),
    );
  }
}
for (const t of ["1", "'s'", "true", "Symbol()", "1n", "null", "undefined", "NaN"]) {
  for (const p of ["null", "{}", "undefined", "1"]) {
    add(L(`return S(Object.setPrototypeOf(${t},${p}))`), L(`return Reflect.setPrototypeOf(${t},${p})`));
  }
}
// cadeias e ciclos.
for (let n = 1; n <= 6; n++) {
  const mk = `var c=[];for(var i=0;i<${n};i++){c.push({});if(i)Object.setPrototypeOf(c[i],c[i-1])}`;
  add(
    L(`${mk}return Reflect.setPrototypeOf(c[0],c[${n - 1}])`), L(`${mk}return Object.setPrototypeOf(c[0],c[${n - 1}])`), L(`${mk}try{Object.setPrototypeOf(c[0],c[${n - 1}])}catch(e){return e.name+': '+e.message}`),
    L(`${mk}return Reflect.setPrototypeOf(c[${n - 1}],c[0])`), L(`${mk}return Reflect.setPrototypeOf(c[0],c[0])`), L(`${mk}return Reflect.setPrototypeOf(c[0],null)+' '+P(Object.getPrototypeOf(c[0]))`),
    L(`${mk}Reflect.setPrototypeOf(c[0],Object.create(null));return Object.getPrototypeOf(c[${n - 1}])===c[${Math.max(n - 2, 0)}]`),
    L(`${mk}var p=new Proxy(c[${n - 1}],{});return Reflect.setPrototypeOf(c[0],p)`), L(`${mk}var p=new Proxy(c[${n - 1}],{});Reflect.setPrototypeOf(c[0],p);return Object.getPrototypeOf(c[0])===p`),
    L(`${mk}var p=new Proxy(c[0],{});return Reflect.setPrototypeOf(c[${n - 1}],p)+' '+Reflect.setPrototypeOf(c[0],c[${n - 1}])`),
    L(`${mk}var p=new Proxy({},{setPrototypeOf(t,v){return Reflect.setPrototypeOf(t,v)}});return Reflect.setPrototypeOf(p,c[${n - 1}])`),
    L(`${mk}c[0].__proto__=Array.prototype;return Array.isArray(c[${n - 1}])+' '+(c[${n - 1}] instanceof Array)+' '+('push' in c[${n - 1}])`),
    L(`${mk}return c.map(o=>Object.getPrototypeOf(o)===null).join()`),
  );
}
add(
  L("return Object.setPrototypeOf(Object.prototype,null)===Object.prototype"), L("return Reflect.setPrototypeOf(Object.prototype,null)"), L("return Reflect.setPrototypeOf(Object.prototype,{})"), L("return Reflect.setPrototypeOf(Object.prototype,Object.prototype)"),
  L("return Reflect.setPrototypeOf(Object.prototype,Array.prototype)"), L("return Reflect.setPrototypeOf(Object.prototype,undefined)"), L("return Object.setPrototypeOf(Object.prototype,{})"), L("return Object.setPrototypeOf(Object.prototype,Object.create(null))"),
  L("try{Object.setPrototypeOf(Object.prototype,{})}catch(e){return e.constructor===TypeError}"), L("try{Object.setPrototypeOf(Object.prototype,{})}catch(e){return e.message}"), L("try{Object.setPrototypeOf(Object.prototype,Object.create(null))}catch(e){return e.message}"),
  L("try{Object.prototype.__proto__={}}catch(e){return e.message}"), L("try{Object.prototype.__proto__=Object.create(null)}catch(e){return e.message}"), L("try{Reflect.setPrototypeOf(1,null)}catch(e){return e.message}"),
  L("try{Object.setPrototypeOf({},1)}catch(e){return e.message}"), L("try{Object.setPrototypeOf({},undefined)}catch(e){return e.message}"), L("try{Object.setPrototypeOf({})}catch(e){return e.message}"), L("try{Object.setPrototypeOf(null,{})}catch(e){return e.message}"),
  L("try{Object.setPrototypeOf(undefined,{})}catch(e){return e.message}"), L("try{Object.setPrototypeOf(Object.preventExtensions({}),{})}catch(e){return e.message}"), L("try{Object.setPrototypeOf(Object.freeze({}),{})}catch(e){return e.message}"),
  L("var a={},b=Object.create(a);try{Object.setPrototypeOf(a,b)}catch(e){return e.message}"), L("var a={};try{Object.setPrototypeOf(a,a)}catch(e){return e.message}"), L("try{Object.setPrototypeOf(new Proxy({},{setPrototypeOf(){return false}}),{})}catch(e){return e.message}"),
  L("var r=Proxy.revocable({},{});r.revoke();try{Object.setPrototypeOf(r.proxy,{})}catch(e){return e.message}"), L("try{Reflect.setPrototypeOf({})}catch(e){return e.message}"), L("try{Reflect.setPrototypeOf({},1)}catch(e){return e.message}"), L("try{Reflect.setPrototypeOf({},undefined)}catch(e){return e.message}"),
  L("try{Reflect.setPrototypeOf()}catch(e){return e.message}"), L("try{Reflect.setPrototypeOf(null,null)}catch(e){return e.message}"), L("try{Reflect.setPrototypeOf('s',null)}catch(e){return e.message}"), L("try{Reflect.getPrototypeOf(1)}catch(e){return e.message}"),
  L("try{Reflect.getPrototypeOf()}catch(e){return e.message}"), L("try{Reflect.getPrototypeOf(null)}catch(e){return e.message}"), L("return Object.getPrototypeOf('s')===String.prototype"),
  L("return Object.setPrototypeOf.length+Object.setPrototypeOf.name+Reflect.setPrototypeOf.length+Reflect.setPrototypeOf.name+Object.getPrototypeOf.length+Reflect.getPrototypeOf.length"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo '+(v===Array.prototype));return true}});var r=Object.setPrototypeOf(p,Array.prototype);return (r===p)+' '+log.join()"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo '+(v===null));return false}});var r=Reflect.setPrototypeOf(p,null);return r+' '+log.join()"),
  L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo');return 0}});return Reflect.setPrototypeOf(p,null)+log.join()"), L("var log=[];var p=new Proxy({},{setPrototypeOf(t,v){log.push('spo');return 'yes'}});return Reflect.setPrototypeOf(p,null)+log.join()"),
  L("var p=new Proxy({},{setPrototypeOf(t,v){return true}});return Reflect.setPrototypeOf(p,null)+' '+P(Object.getPrototypeOf(p))"), L("var p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(t,v){return true}});return Reflect.setPrototypeOf(p,null)"),
  L("var p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(t,v){return true}});return Reflect.setPrototypeOf(p,Object.getPrototypeOf(p))"), L("var p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(t,v){return true}});return Reflect.setPrototypeOf(p,Object.prototype)"),
  L("var p=new Proxy(Object.preventExtensions({}),{setPrototypeOf(t,v){return true},getPrototypeOf(){return null}});return Reflect.setPrototypeOf(p,null)"), L("var p=new Proxy({},{setPrototypeOf(){throw new RangeError('x')}});return Reflect.setPrototypeOf(p,null)"),
  L("var p=new Proxy({},{setPrototypeOf:null});return Reflect.setPrototypeOf(p,null)"), L("var p=new Proxy({},{setPrototypeOf:1});return Reflect.setPrototypeOf(p,null)"), L("var p=new Proxy({},{setPrototypeOf:{}});return Reflect.setPrototypeOf(p,null)"),
  L("var o={};var r=Reflect.setPrototypeOf(o,{x:1});return r+' '+o.x"), L("var o={x:0};Object.setPrototypeOf(o,{x:1,y:2});return o.x+' '+o.y"), L("var o=Object.create(null);Object.setPrototypeOf(o,Object.prototype);return typeof o.toString+' '+o.hasOwnProperty('a')"),
  L("var o=[];Object.setPrototypeOf(o,Object.prototype);return Array.isArray(o)+' '+typeof o.push+' '+o.length"), L("var o=[1,2];Object.setPrototypeOf(o,null);return o.length+' '+Object.keys(o).join()+' '+Array.isArray(o)"),
  L("var o=function(){return 1};Object.setPrototypeOf(o,null);return o()+' '+typeof o.call+' '+typeof o"), L("var o=function(){return 1};Object.setPrototypeOf(o,{});return o()+' '+(o instanceof Function)"),
  L("var o=class{};Object.setPrototypeOf(o,null);return typeof o+' '+typeof new o"), L("class A{static f(){return 'A'}};class B extends A{};Object.setPrototypeOf(B,null);return typeof B.f"), L("class A{static f(){return 'A'}};class B{};Object.setPrototypeOf(B,A);return B.f()"),
  L("class A{m(){return 'A'}};class B{};Object.setPrototypeOf(B.prototype,A.prototype);return new B().m()"), L("class A{constructor(){this.k=1}};class B extends A{};Object.setPrototypeOf(B,Object);return 'ok'"),
  L("class A{};class B extends A{};Object.setPrototypeOf(B,null);try{new B}catch(e){return e.name+': '+e.message}"), L("class A{};class B extends A{};Object.setPrototypeOf(B,Function.prototype);try{new B}catch(e){return e.name+': '+e.message}"),
  L("class A{};class B extends A{};Object.setPrototypeOf(B.prototype,null);return Object.getPrototypeOf(new B)===B.prototype"), L("var o=new Uint8Array(2);Object.setPrototypeOf(o,Array.prototype);return o.length+' '+o[0]+' '+typeof o.push"),
  L("var o=new Uint8Array(2);Object.setPrototypeOf(o,null);return o.length+' '+o[0]+' '+Object.keys(o).join()"), L("var o=new Date(0);Object.setPrototypeOf(o,null);try{return Date.prototype.getTime.call(o)}catch(e){return e.name}"),
  L("var o=new Map;Object.setPrototypeOf(o,null);try{return Map.prototype.size}catch(e){return e.name}"), L("var o=new Map([[1,2]]);Object.setPrototypeOf(o,null);return Map.prototype.get.call(o,1)"), L("var o=new Error('m');Object.setPrototypeOf(o,null);return o.message+' '+Object.prototype.toString.call(o)+' '+(o instanceof Error)"),
  L("var o=/x/g;Object.setPrototypeOf(o,null);return RegExp.prototype.test.call(o,'x')+' '+o.lastIndex"), L("var o=Promise.resolve(1);Object.setPrototypeOf(o,null);return typeof o.then"), L("var o=(function*(){yield 1})();Object.setPrototypeOf(o,null);return typeof o.next"),
  L("var o=(function*(){yield 1})();Object.setPrototypeOf(o,{next:Object.getPrototypeOf(o).next});return o.next().value"), L("var o=new String('ab');Object.setPrototypeOf(o,null);return o.length+' '+o[0]+' '+Object.keys(o).join()"),
  L("var o=Object(1);Object.setPrototypeOf(o,null);try{return o+1}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return o+''}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return `${o}`}catch(e){return e.name+': '+e.message}"),
  L("var o={};Object.setPrototypeOf(o,null);try{return String(o)}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return o==1}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return +o}catch(e){return e.name+': '+e.message}"),
  L("var o={};Object.setPrototypeOf(o,null);try{return o<1}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return JSON.stringify(o)}catch(e){return e.name+': '+e.message}"), L("var o={a:1};Object.setPrototypeOf(o,null);return JSON.stringify(o)+Object.keys(o)"),
  L("var o={};Object.setPrototypeOf(o,null);try{return [o].join()}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return Number(o)}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return BigInt(o)}catch(e){return e.name+': '+e.message}"),
  L("var o={};Object.setPrototypeOf(o,null);try{return o instanceof Object}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return 1 in o}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return new Date(o).getTime()}catch(e){return e.name+': '+e.message}"),
  L("var o={};Object.setPrototypeOf(o,null);try{return Symbol(o).description}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return 'a'.concat(o)}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return new Error(o).message}catch(e){return e.name+': '+e.message}"),
  L("var o={};Object.setPrototypeOf(o,null);try{return [1].includes(o)+' '+Object.is(o,o)}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{var {a}=o;return a}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{var [a]=o;return a}catch(e){return e.name+': '+e.message}"),
  L("var o={};Object.setPrototypeOf(o,null);try{return {...o,a:1}.a}catch(e){return e.name+': '+e.message}"), L("var o={};Object.setPrototypeOf(o,null);try{return Math.max(o)}catch(e){return e.name+': '+e.message}"), L("var o={[Symbol.toPrimitive](){return 5}};Object.setPrototypeOf(o,null);return o+1"),
  L("var o=Object.create(null);o[Symbol.toPrimitive]=()=>7;return o+1"), L("var o=Object.create(null);o.toString=()=>'ts';return o+'!'"), L("var o=Object.create(null);o.valueOf=()=>9;return o+1"), L("var o=Object.create(null);o.toString=()=>1;return o+'x'"),
  L("var s=Symbol();var o={};Object.setPrototypeOf(o,{[s]:1});return o[s]+' '+(s in o)"), L("var o={};Object.setPrototypeOf(o,{get g(){return this===o}});return o.g"), L("var p={set s(v){this.z=v}};var o=Object.setPrototypeOf({},p);o.s=3;return o.z+' '+Object.hasOwn(o,'s')"),
  L("var p=Object.freeze({s:1});var o=Object.setPrototypeOf({},p);o.s=3;return o.s+' '+Object.hasOwn(o,'s')"), L("'use strict';var p=Object.freeze({s:1});var o=Object.setPrototypeOf({},p);o.s=3"),
  L("var p={};var o=Object.create(p);Object.setPrototypeOf(p,{inh:1});return o.inh"), L("var p={};var o=Object.create(p);var q={};Object.setPrototypeOf(p,q);Object.setPrototypeOf(q,p)"), L("var p={};var o=Object.create(p);Reflect.setPrototypeOf(p,o)"),
  L("var f=Object.freeze({});return Reflect.setPrototypeOf(f,Object.prototype)+' '+Reflect.setPrototypeOf(f,null)+' '+Reflect.setPrototypeOf(f,f)"), L("var n=Object.preventExtensions(Object.create(null));return Reflect.setPrototypeOf(n,null)+' '+Reflect.setPrototypeOf(n,{})"),
  L("var n=Object.preventExtensions([]);return Reflect.setPrototypeOf(n,Array.prototype)+' '+Reflect.setPrototypeOf(n,Object.prototype)"), L("var n=Object.seal(function(){});return Reflect.setPrototypeOf(n,Function.prototype)+' '+Reflect.setPrototypeOf(n,null)"),
  L("var f=Object.freeze(new Uint8Array(0));return Reflect.setPrototypeOf(f,Uint8Array.prototype)+' '+Reflect.setPrototypeOf(f,null)"), L("return Reflect.setPrototypeOf(globalThis,Object.getPrototypeOf(globalThis))"),
  L("return Reflect.setPrototypeOf(Object,Function.prototype)+' '+Reflect.setPrototypeOf(Object,null)+' '+typeof Object.call"), L("return Reflect.setPrototypeOf(Function.prototype,null)+' '+Reflect.setPrototypeOf(Function.prototype,Object.prototype)+' '+Reflect.setPrototypeOf(Function.prototype,Array.prototype)"),
  L("return Reflect.setPrototypeOf(Array.prototype,Object.prototype)+' '+Reflect.setPrototypeOf(Array.prototype,null)"), L("Reflect.setPrototypeOf(Array.prototype,null);return typeof [].toString+' '+Object.prototype.toString.call([])+' '+([] instanceof Object)"),
  L("var o=Object.setPrototypeOf({},Object.create(null));return typeof o.toString+' '+typeof o.hasOwnProperty+' '+(o instanceof Object)"), L("var o=Object.setPrototypeOf({a:1},Object.create(null,{b:{value:2}}));return o.b+' '+Object.keys(o)"),
  L("var o={};return Object.setPrototypeOf(o,Object.getPrototypeOf(o))===o"), L("var o={};return Reflect.setPrototypeOf(o,Object.getPrototypeOf(o))"), L("return Object.setPrototypeOf([],Array.prototype) instanceof Array"), L("return Object.setPrototypeOf(function(){},Function.prototype) instanceof Function"),
  L("var r=Object.setPrototypeOf({},null);return Object.getPrototypeOf(r)===null && Reflect.getPrototypeOf(r)===null && r.__proto__===undefined"), L("return Object.getPrototypeOf(Object.create(null))"), L("return Object.getPrototypeOf(Object.getPrototypeOf(function*(){}).prototype)===Iterator.prototype"),
  L("return Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))===Iterator.prototype"), L("return Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(Uint8Array)))===Function.prototype||Object.getPrototypeOf(Uint8Array)===Function.prototype"),
  L("return Object.getPrototypeOf(Function.prototype)===Object.prototype"), L("return Object.getPrototypeOf(Object)===Function.prototype"), L("return Object.getPrototypeOf(Function)===Function.prototype"), L("return Object.getPrototypeOf(Symbol.prototype)===Object.prototype"),
  L("return Object.getPrototypeOf(TypeError)===Error && Object.getPrototypeOf(TypeError.prototype)===Error.prototype"), L("return Object.getPrototypeOf(AggregateError)===Error"), L("return Object.getPrototypeOf(Math)===Object.prototype && Object.getPrototypeOf(JSON)===Object.prototype"),
  L("return Object.getPrototypeOf(Promise)===Function.prototype && Object.getPrototypeOf(Intl)===Object.prototype"), L("return Object.getPrototypeOf(async function(){}).constructor.name+Object.getPrototypeOf(function*(){}).constructor.name+Object.getPrototypeOf(async function*(){}).constructor.name"),
);

// ---- J. Object.hasOwn.
const hoTargets = ["{a:1,0:2,[Symbol.iterator]:3}", "[1,,3]", "'abc'", "Object('abc')", "1", "true", "Symbol()", "1n", "function f(a){}", "class{static s=1}", "new Uint8Array(2)", "(function(){return arguments})(1)", "Object.create({a:1})", "Object.create(null,{a:{value:1}})", "new Proxy({a:1},{})", "Math", "globalThis", "new Error('m')", "/x/g", "new Map", "[]", "Object.prototype", "Array.prototype", "Function.prototype"];
const hoKeys = ["'a'", "0", "'0'", "-0", "'-0'", "1", "2", "'length'", "'name'", "'prototype'", "'toString'", "'__proto__'", "'constructor'", "Symbol.iterator", "Symbol.toStringTag", "'callee'", "'caller'", "'message'", "'stack'", "'lastIndex'", "'PI'", "'s'", "{toString(){return 'a'}}", "null", "undefined", "1n", "true", "[0]"];
for (const t of hoTargets) {
  for (const k of hoKeys) {
    add(L(`return Object.hasOwn(${t},${k})`));
  }
}
add(
  L("return Object.hasOwn()"), L("return Object.hasOwn({})"), L("return Object.hasOwn({undefined:1})"), L("return Object.hasOwn({undefined:1},undefined)"), L("return Object.hasOwn(null)"), L("return Object.hasOwn(undefined)"),
  L("var log=[];var p=new Proxy({a:1},{getOwnPropertyDescriptor(t,k){log.push('gopd '+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},has(t,k){log.push('has');return true},get(t,k){log.push('get');return 1},getPrototypeOf(t){log.push('gpo');return null}});var r=Object.hasOwn(p,'a');return r+' '+log.join()"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(){return {value:1,configurable:true}}});return Object.hasOwn(p,'z')"), L("var p=new Proxy({},{getOwnPropertyDescriptor(){return undefined}});return Object.hasOwn(p,'z')"), L("var p=new Proxy({},{getOwnPropertyDescriptor(){return 1}});return Object.hasOwn(p,'z')"),
  L("var p=new Proxy({},{getOwnPropertyDescriptor(){throw new RangeError('d')}});return Object.hasOwn(p,'z')"), L("var r=Proxy.revocable({},{});r.revoke();return Object.hasOwn(r.proxy,'a')"),
  L("var log=[];var k={toString(){log.push('key');return 'a'}};try{Object.hasOwn(null,k)}catch(e){log.push(e.name)}return log.join()"), L("var log=[];var k={toString(){log.push('key');return 'a'}};try{Object.hasOwn({},k)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var k={toString(){log.push('key');throw new RangeError('k')}};try{Object.hasOwn(undefined,k)}catch(e){log.push(e.name)}return log.join()"), L("var log=[];var k={[Symbol.toPrimitive](h){log.push(h);return 'a'}};Object.hasOwn({a:1},k);return log.join()"),
  L("var k={[Symbol.toPrimitive](h){return {}}};return Object.hasOwn({a:1},k)"), L("var k={toString(){return {}},valueOf(){return 'a'}};return Object.hasOwn({a:1},k)"), L("var k={toString(){return {}},valueOf(){return {}}};return Object.hasOwn({a:1},k)"),
  L("return Object.hasOwn.call(null,{a:1},'a')"), L("return Object.hasOwn({a:1},'a','b')"), L("return Object.hasOwn.length+Object.hasOwn.name"), L("new Object.hasOwn({},'a')"), L("return D(Object,'hasOwn').replace(/v=fn/,'v')"),
  L("return Object.hasOwn(Object('ab'),'1')+' '+Object.hasOwn(Object('ab'),'2')+' '+Object.hasOwn(Object('ab'),'-0')+' '+Object.hasOwn(Object('ab'),'01')"), L("return Object.hasOwn(new Uint8Array(2),'1')+' '+Object.hasOwn(new Uint8Array(2),'2')+' '+Object.hasOwn(new Uint8Array(2),'-0')+' '+Object.hasOwn(new Uint8Array(2),'1.5')"),
  L("var a=[];a[5]=1;return Object.hasOwn(a,5)+' '+Object.hasOwn(a,4)+' '+Object.hasOwn(a,'length')+' '+Object.hasOwn(a,'5.0')"), L("var o=Object.create(null);o.a=1;return Object.hasOwn(o,'a')+' '+Object.hasOwn(o,'__proto__')"),
  L("var o={};Object.defineProperty(o,'h',{value:1,enumerable:false});return Object.hasOwn(o,'h')+' '+('h' in o)+' '+o.hasOwnProperty('h')+' '+Object.keys(o).length"), L("var o={get g(){throw 1}};return Object.hasOwn(o,'g')"),
  L("var o={a:1};delete o.a;return Object.hasOwn(o,'a')"), L("var o=Object.freeze({a:1});return Object.hasOwn(o,'a')"), L("var o={a:undefined};return Object.hasOwn(o,'a')+' '+('a' in o)+' '+(o.a!==undefined)"),
  L("return Object.hasOwn(function(){},'caller')+' '+Object.hasOwn(function(){'use strict'},'caller')+' '+Object.hasOwn(()=>1,'prototype')+' '+Object.hasOwn(async function(){},'prototype')+' '+Object.hasOwn(function*(){},'prototype')"),
  L("return Object.hasOwn(class{},'prototype')+' '+Object.hasOwn(class{},'name')+' '+Object.hasOwn(class A{},'name')+' '+Object.hasOwn(class{static name='x'},'name')+' '+Object.hasOwn(class{static name(){}},'name')"),
  L("return Object.hasOwn(Symbol.prototype,'description')+' '+Object.hasOwn(Symbol(),'description')+' '+Object.hasOwn(Symbol.prototype,'constructor')+' '+Object.hasOwn(Symbol,'iterator')"),
  L("return Object.hasOwn(globalThis,'globalThis')+' '+Object.hasOwn(globalThis,'Object')+' '+Object.hasOwn(globalThis,'undefined')+' '+Object.hasOwn(globalThis,'NaN')+' '+Object.hasOwn(globalThis,'toString')"),
  L("return Object.hasOwn(Object.prototype,'hasOwnProperty')+' '+Object.hasOwn(Object,'hasOwn')+' '+Object.hasOwn(Object,'groupBy')+' '+Object.hasOwn(Object,'fromEntries')+' '+Object.hasOwn(Object,'entries')+' '+Object.hasOwn(Object,'values')"),
  L("return Object.hasOwn(Map,'groupBy')+' '+Object.hasOwn(Array,'fromAsync')+' '+Object.hasOwn(Promise,'withResolvers')+' '+Object.hasOwn(Promise,'try')+' '+Object.hasOwn(Symbol,'dispose')+' '+Object.hasOwn(Iterator,'from')"),
);

// ---- K. Object.groupBy (e Map.groupBy).
const gbItems = ["[1,2,3,4,5]", "[]", "'abcde'", "''", "new Set([1,2,3])", "new Map([[1,2],[3,4]])", "[,1,,2]", "[1,2,3].values()", "(function*(){yield 'a';yield 'bb';yield 'cc'})()", "{[Symbol.iterator](){var i=0;return {next(){return {done:i>=3,value:i++}}}}}", "[{k:'a',v:1},{k:'b',v:2},{k:'a',v:3}]", "[null,undefined,0,'',NaN,false]", "'a\\ud83d\\ude00b'", "[1.5,-0,0,NaN,NaN]"];
const gbFns = [
  "x=>x%2?'odd':'even'", "x=>x", "x=>String(x)", "(x,i)=>i%2", "(x,i)=>i", "x=>x>2", "x=>typeof x", "x=>x&&x.k", "x=>Symbol.for('s')", "x=>-0", "x=>0", "x=>NaN", "x=>({})", "x=>[1,2]", "x=>null", "x=>undefined", "x=>1n", "x=>'__proto__'", "x=>'constructor'", "x=>'toString'", "x=>1.5", "x=>1e21", "x=>true", "x=>x*0.1",
  "x=>String(x).length", "function(){return this===undefined?'u':typeof this}", "()=>'k'", "(x,i,a)=>a", "(x,i,a)=>arguments.length", "(...a)=>a.length", "x=>({toString(){return 'ts'+x}})", "x=>({valueOf(){return 5},toString:null})", "x=>({[Symbol.toPrimitive](){return 'tp'}})",
];
for (const it of gbItems) {
  for (const fn of gbFns.slice(0, 12)) {
    add(L(`var r=Object.groupBy(${it},${fn});return S(r)+' '+P(Object.getPrototypeOf(r))`));
  }
}
for (const fn of gbFns) {
  add(
    L(`var r=Object.groupBy([1,2,3,4],${fn});return Reflect.ownKeys(r).map(String).join()+' '+Object.keys(r).map(k=>S(r[k])).join('|')`),
    L(`var r=Map.groupBy([1,2,3,4],${fn});return S([...r.keys()])+' '+S([...r.values()])`),
    L(`var r=Object.groupBy('xyz',${fn});return Reflect.ownKeys(r).map(String).join()`),
    L(`var r=Object.groupBy([1,2],${fn});return Object.keys(r).map(k=>D(r,k)).join('|')`),
  );
}
add(
  L("return Object.groupBy()"), L("return Object.groupBy([1])"), L("return Object.groupBy([1],1)"), L("return Object.groupBy([1],null)"), L("return Object.groupBy([1],{})"), L("return Object.groupBy(null,x=>x)"), L("return Object.groupBy(undefined,x=>x)"),
  L("return Object.groupBy(1,x=>x)"), L("return Object.groupBy(true,x=>x)"), L("return Object.groupBy({},x=>x)"), L("return Object.groupBy({length:2,0:'a',1:'b'},x=>x)"), L("return Object.groupBy(Symbol(),x=>x)"), L("return Object.groupBy(1n,x=>x)"),
  L("return Object.groupBy([],1)"), L("return Object.groupBy([],null)"), L("return Object.groupBy(null,null)"), L("return Object.groupBy.length+Object.groupBy.name+Map.groupBy.length+Map.groupBy.name"), L("new Object.groupBy([],x=>x)"),
  L("return D(Object,'groupBy').replace(/v=fn/,'v')"), L("return Object.groupBy.call(null,[1],x=>x)[1].length"), L("return Object.groupBy.call(1,[1],x=>x)[1].length"), L("return Object.groupBy([1],x=>x,'extra')[1].length"),
  L("var log=[];var r=Object.groupBy([5,6],function(x,i,...rest){log.push(x+':'+i+':'+rest.length+':'+arguments.length);return 'k'});return log.join()"), L("var log=[];Object.groupBy([5,6],function(){log.push(typeof this+(this===undefined))});return log.join()"),
  L("var log=[];Object.groupBy([5,6],function(){'use strict';log.push(typeof this)});return log.join()"), L("var log=[];Object.groupBy([5],x=>{log.push('cb');return {toString(){log.push('ts');return 'k'}}});return log.join()"),
  L("var log=[];Object.groupBy([5],x=>{log.push('cb');return {toString(){log.push('ts');throw new RangeError('ts')}}})"), L("var log=[];try{Object.groupBy([5,6],x=>{log.push('cb'+x);throw new RangeError('cb')})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var it={[Symbol.iterator](){log.push('iter');return {next(){log.push('next');return {done:true}},return(){log.push('return')}}}};Object.groupBy(it,x=>x);return log.join()"),
  L("var log=[];var it={[Symbol.iterator](){return {i:0,next(){log.push('next');return {done:this.i>1,value:this.i++}},return(){log.push('return');return {}}}}};try{Object.groupBy(it,x=>{throw new RangeError('x')})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var it={[Symbol.iterator](){return {i:0,next(){log.push('next');return {done:this.i>1,value:this.i++}},return(){log.push('return');throw new EvalError('ret')}}}};try{Object.groupBy(it,x=>{throw new RangeError('x')})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var it={[Symbol.iterator](){return {i:0,next(){log.push('next');return {done:this.i>1,value:this.i++}},return(){log.push('return');return 1}}}};try{Object.groupBy(it,x=>{throw new RangeError('x')})}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var it={[Symbol.iterator](){return {next(){log.push('next');throw new RangeError('nx')},return(){log.push('return')}}}};try{Object.groupBy(it,x=>x)}catch(e){log.push(e.name)}return log.join()"),
  L("var log=[];var it={[Symbol.iterator](){return {next(){return 1}}}};try{Object.groupBy(it,x=>x)}catch(e){log.push(e.name+': '+e.message)}return log.join()"), L("var it={[Symbol.iterator](){return 1}};return Object.groupBy(it,x=>x)"), L("var it={[Symbol.iterator]:1};return Object.groupBy(it,x=>x)"),
  L("var it={[Symbol.iterator](){return {}}};return Object.groupBy(it,x=>x)"), L("var it={[Symbol.iterator](){return {next:1}}};return Object.groupBy(it,x=>x)"), L("var it={[Symbol.iterator](){return {next(){return {done:true}}}}};return S(Object.groupBy(it,x=>x))"),
  L("var a=[1,2,3];var r=Object.groupBy(a,(x,i)=>{if(i===0)a.push(4);return 'k'});return r.k.join()"), L("var a=[1,2,3];var r=Object.groupBy(a,(x,i)=>{a.length=1;return 'k'});return r.k.join()"), L("var a=[1,2,3];var r=Object.groupBy(a,(x,i)=>{a[2]='z';return 'k'});return r.k.join()"),
  L("var r=Object.groupBy([1,2],x=>'a');r.a.push(9);return r.a.join()+Array.isArray(r.a)"), L("var r=Object.groupBy([1],x=>'a');return Object.isFrozen(r)+' '+Object.isExtensible(r)+' '+Object.isFrozen(r.a)"), L("var r=Object.groupBy([1],x=>'a');return Object.hasOwn(r,'a')+' '+('toString' in r)+' '+(r.toString)"),
  L("var r=Object.groupBy(['a','b'],x=>'__proto__');return r.__proto__+' '+Object.keys(r).join()"), L("var r=Object.groupBy([1],x=>'__proto__');return D(r,'__proto__').slice(0,6)"), L("var r=Object.groupBy([1],x=>Symbol.iterator);return Reflect.ownKeys(r).map(String).join()"),
  L("var s=Symbol('s');var r=Object.groupBy([1,2,3],x=>x==2?s:'n');return Reflect.ownKeys(r).map(String).join()+' '+r[s]"), L("var r=Object.groupBy([1,2,3],x=>x==2?1:'a');return Reflect.ownKeys(r).join()"), L("var r=Object.groupBy([1,2,3],x=>3-x);return Reflect.ownKeys(r).join()"),
  L("var r=Object.groupBy([1,2,3],x=>x==1?'b':x==2?'a':10);return Reflect.ownKeys(r).join()"), L("var r=Object.groupBy(['x','y'],x=>-0);return Reflect.ownKeys(r).join()+' '+Object.is(+Object.keys(r)[0],0)"), L("var r=Map.groupBy(['x','y'],x=>-0);return Object.is([...r.keys()][0],0)+' '+r.size"),
  L("var r=Map.groupBy([1,2,3],x=>NaN);return r.size+' '+r.get(NaN).join()"), L("var o={};var r=Map.groupBy([1,2,3],x=>o);return r.size+' '+(r.get(o)===r.get(o))+' '+r.get(o).join()"), L("var r=Map.groupBy([1,2,3],x=>({}));return r.size"), L("var r=Map.groupBy([1,2,3],x=>x%2);return Object.getPrototypeOf(r)===Map.prototype+' '+r.size+' '+[...r.keys()].join()"),
  L("var r=Map.groupBy([1,2,3],x=>'k'+(x%2));return [...r].map(e=>e[0]+'='+e[1].join()).join(';')"), L("var r=Map.groupBy([],x=>x);return r.size+' '+(r instanceof Map)"), L("return Map.groupBy()"), L("return Map.groupBy([1])"), L("return Map.groupBy(null,x=>x)"), L("return Map.groupBy(1,x=>x)"),
  L("var r=Object.groupBy('a\\ud83d\\ude00b',x=>x.length);return S(r)"), L("var r=Object.groupBy(new Map([[1,2],[3,4]]),([k,v])=>k>1);return S(r)"), L("var r=Object.groupBy(new Set('aab'),x=>x);return S(r)"), L("var r=Object.groupBy(new Uint8Array([1,2,3]),x=>x%2);return S(r)"), L("var r=Object.groupBy(new Float64Array([0.5,1.5]),x=>x>1);return S(r)"),
  L("var r=Object.groupBy(new BigInt64Array([1n,2n]),x=>x>1n);return S(r)"), L("var r=Object.groupBy((function(){return arguments})(1,2,3),x=>x>1);return S(r)"), L("var r=Object.groupBy([1,2,3].entries(),([i,v])=>v>1);return S(r)"), L("var r=Object.groupBy(Object.entries({a:1,b:2}),([k,v])=>v>1);return S(r)"),
  L("var r=Object.groupBy(new Proxy([1,2,3],{}),x=>x>1);return S(r)"), L("var log=[];var r=Object.groupBy(new Proxy([1,2],{get(t,k){log.push(String(k));return Reflect.get(t,k)}}),x=>x);return log.join()"), L("var r=Object.groupBy(Object.assign([1,2],{extra:1}),x=>x);return S(r)"),
  L("var r=Object.groupBy(Object.defineProperty([1,2],1,{get(){return 'g'}}),x=>x);return S(r)"), L("var r=Object.groupBy({__proto__:[1,2,3]},x=>x);return S(r)"), L("class A extends Array{};var r=Object.groupBy(A.from([1,2]),x=>'a');return Array.isArray(r.a)+' '+(r.a instanceof A)"),
  L("var k=Object.groupBy([1],x=>'a');var r=Object.entries(k);return S(r)"), L("var r=Object.groupBy([1,2,3,4,5,6],x=>x%3);return JSON.stringify(r)"), L("var r=Object.groupBy(['apple','avocado','banana'],s=>s[0]);return JSON.stringify(r)"),
  L("return JSON.stringify(Object.groupBy([{t:'a',n:1},{t:'b',n:2},{t:'a',n:3}],o=>o.t))"), L("return JSON.stringify(Object.fromEntries(Map.groupBy([1,2,3,4],x=>x%2?'o':'e')))"), L("return JSON.stringify([...Map.groupBy([1,2,3,4],x=>x%2)])"),
  L("var s=0;Object.groupBy([1,2,3],x=>{s+=x;return 'k'});return s"), L("var r=Object.groupBy([1,2],x=>'a');var r2=Object.groupBy([1,2],x=>'a');return r.a===r2.a"), L("var r=Object.groupBy([[1],[2]],x=>'a');r.a[0].push(9);return r.a[0].length"),
);

// ---- Execução.
const baseSeen = new Set();
const baseText = [];
for (const src of knownPrograms("object_proto_bun.tsv", (file) => /^(object_|accessor_|reflect|reflection_|proxy)/.test(file) && file !== "object_proto_bun.tsv")) {
  const at = src.indexOf("globalThis.R = ");
  baseSeen.add(at >= 0 ? src.slice(at + 15) : src);
}
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
const out = [];
const [shard, shards] = (process.env.SHARD || "0/1").split("/").map(Number);
for (const [index, expr] of unique.entries()) {
  if (index % shards !== shard) continue;
  // As grades geram mais de sete mil programas; mantém dois em cada cinco (3022) para o golden não passar de 7 MB.
  if (Math.floor(index / shards) % 5 >= 2) continue;
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSeen.has(expr)) { dup++; continue; }
  let result;
  try {
    // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 20000 });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + String(e).slice(0, 80) + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("object_proto", rows));
