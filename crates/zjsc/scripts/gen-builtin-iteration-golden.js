// Gera tests/golden/builtin_iteration_bun.tsv: protocolos de iteração dos built-ins, medidos no bun 1.4.2.
// Cobre os iteradores de Array, String, Map, Set, TypedArray, arguments e RegExp String Iterator (cadeia de protótipos,
// @@toStringTag, `next` com this inválido, exaustão e reuso, mutação durante a iteração), os consumidores do protocolo
// (spread, Array.from, desestruturação, for-of, yield*, Promise.all, construtores de Map/Set/WeakMap/WeakSet,
// Object.fromEntries, concat) sobre iteráveis cujo next/return/throw registram os acessos, Symbol.iterator trocado em
// protótipos built-in e em instâncias, strings com surrogates, arrays esparsos, array-likes, TypedArray com buffer
// destacado ou redimensionado, %IteratorPrototype% e %AsyncIteratorPrototype%. Programas cuja expressão já aparece nos
// goldens de iteração, coleções, arrays, strings, typed arrays e regexp são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-builtin-iteration-golden.js > tests/golden/builtin_iteration_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

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

// Fábrica de iteráveis que registram cada acesso (get Symbol.iterator, chamada, get next, next, done, value, return, throw).
const MK =
  "function MK(o){o=o||{};var L=[],i=0,n=o.n===undefined?3:o.n;" +
  "var nf=function(){L.push('next');if(o.throwAt===i)throw new Error('nx');if(o.badAt===i){i++;return o.badVal}" +
  "if(i>=n)return {get done(){L.push('done');return true},get value(){L.push('value');return 'e'}};" +
  "var v=i++;return {get done(){L.push('done');return false},get value(){L.push('value');return v}}};" +
  "function meth(mode,name){return function(v){L.push(name+'('+(arguments.length?S(v):'')+')');if(mode==='throw')throw new Error(name+'x');if(mode==='nonobj')return 1;return {value:'R',done:true}}}" +
  "var it={get next(){L.push('get next');return 'nextVal' in o?o.nextVal:nf}};" +
  "Object.defineProperty(it,'return',{get(){L.push('get return');var m=o.ret||'ok';return m==='none'?undefined:m==='null'?null:m==='notfn'?1:meth(m,'return')},configurable:true});" +
  "Object.defineProperty(it,'throw',{get(){L.push('get throw');var m=o.thr||'ok';return m==='none'?undefined:m==='null'?null:m==='notfn'?1:meth(m,'throw')},configurable:true});" +
  "var obj={get [Symbol.iterator](){L.push('get iter');var m=o.iter||'ok';if(m==='undef')return undefined;if(m==='null')return null;if(m==='notfn')return 1;if(m==='throwget')throw new Error('gx');" +
  "return function(){L.push('call iter'+arguments.length);if(m==='throwcall')throw new Error('cx');if(m==='nonobj')return 1;return it}}};" +
  "return {o:obj,L:L,it:it}}\n";

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- Consumidores do protocolo, X é o iterável.
const consumers = [
  "[...X]", "Array.from(X)", "Array.from(X,(v,i)=>v+':'+i)", "Array.from(X,function(){throw new Error('mapfn')})",
  "(()=>{var [a]=X;return a})()", "(()=>{var [a,b]=X;return [a,b]})()", "(()=>{var [,]=X;return 1})()", "(()=>{var []=X;return 1})()",
  "(()=>{var [...r]=X;return r})()", "(()=>{var [a,...r]=X;return [a,r]})()", "(()=>{var [a=9]=X;return a})()", "(()=>{var a,b;[a,b]=X;return [a,b]})()",
  "(()=>{var s=[];for(var v of X)s.push(v);return s})()", "(()=>{var s=[];for(var v of X){s.push(v);break}return s})()",
  "(()=>{var s=[];for(var v of X){s.push(v);throw new RangeError('body')}})()", "(()=>{var s=[];for(var v of X){s.push(v);continue}return s})()",
  "(()=>{for(var v of X){return v}})()", "(()=>{var s=[];a:for(var u of [1,2]){for(var v of X){s.push(v);continue a}}return s})()",
  "[...(function*(){yield* X})()]", "Array.from((function*(){return yield* X})())", "new Map(X)", "new Set(X)", "new WeakMap(X)", "new WeakSet(X)",
  "Object.fromEntries(X)", "Promise.all(X).catch(()=>{})", "Promise.allSettled(X).catch(()=>{})", "Promise.race(X).catch(()=>{})", "Promise.any(X).catch(()=>{})",
  "Math.max(...X)", "((...a)=>a)(...X)", "new Uint8Array(X)", "Uint8Array.from(X)", "Map.groupBy(X,v=>v%2)", "Object.groupBy(X,v=>v)",
  "[].concat(X)", "Array.prototype.concat.call([],X,X)", "Iterator.from(X)", "Iterator.from(X).toArray()", "new Array(...X)", "[].push(...X)", "Array.of(...X)",
  "new Intl.ListFormat('en').format(X)", "new AggregateError(X)", "new (class extends Array{})(...X)",
];
const coreConsumers = [0, 1, 4, 5, 8, 12, 13, 14, 18, 20, 21, 22, 24, 29, 30, 36, 33].map(i => consumers[i]);

// ---- 1. Comportamentos do iterável × consumidores.
const behaviors = [
  "{}", "{n:0}", "{n:1}", "{n:5}", "{throwAt:0}", "{throwAt:1}", "{badAt:0,badVal:1}", "{badAt:1,badVal:null}", "{badAt:0,badVal:undefined}",
  "{nextVal:1}", "{nextVal:undefined}", "{ret:'none'}", "{ret:'throw'}", "{ret:'nonobj'}", "{ret:'null'}", "{ret:'notfn'}",
  "{iter:'undef'}", "{iter:'null'}", "{iter:'notfn'}", "{iter:'throwget'}", "{iter:'throwcall'}", "{iter:'nonobj'}",
];
for (const b of behaviors) for (const c of consumers) {
  add(`T(()=>{var m=MK(${b}),X=m.o,r;try{r=S(${c})}catch(e){r="throw "+e.name+": "+e.message}return m.L.join()+" => "+r})`);
}
// Combinação de falha no corpo e return com defeito.
for (const ret of ["ok", "throw", "nonobj", "none"]) for (const c of ["(()=>{for(var v of X){break}return 1})()", "(()=>{for(var v of X){throw new RangeError('body')}})()", "(()=>{var [a]=X;return a})()", "(()=>{for(var v of X){return v}})()"]) {
  add(`T(()=>{var m=MK({ret:'${ret}'}),X=m.o,r;try{r=S(${c})}catch(e){r="throw "+e.name+": "+e.message}return m.L.join()+" => "+r})`);
}

// ---- 2. yield* delegando a iteráveis com log, dirigido por passos.
const stepPlans = ["n", "nn", "nnn", "nnnn", "r", "nr", "nnr", "t", "nt", "nnt", "ntn", "nrn", "rn", "tn", "ntr", "nnrn", "nttn"];
for (const b of behaviors.concat(["{thr:'none'}", "{thr:'throw'}", "{thr:'nonobj'}", "{thr:'null'}", "{thr:'notfn'}", "{ret:'none',thr:'none'}"])) for (const plan of stepPlans) {
  add(`T(()=>{var m=MK(${b}),out=[];var g=(function*(){var r=yield* m.o;out.push('ret='+S(r));return 'g'})();` +
    `var k=0;for(var c of ${JSON.stringify(plan)}){k++;try{var res=c==='n'?g.next('v'+k):c==='r'?g.return('rv'+k):g.throw(new Error('t'+k));out.push(S(res))}catch(e){out.push('throw '+e.name+': '+e.message)}}` +
    `return out.join(' ; ')+' | '+m.L.join()})`);
}

// ---- 3. Os iteradores built-in: sondagens por espécie.
const kinds = {
  arrValues: "[1,2,3].values()", arrKeys: "[1,2,3].keys()", arrEntries: "['a','b'].entries()", arrSym: "[1,2][Symbol.iterator]()",
  str: "'a\\ud83d\\ude00'[Symbol.iterator]()", mapEntries: "new Map([[1,2],[3,4]]).entries()", mapKeys: "new Map([[1,2],[3,4]]).keys()",
  mapValues: "new Map([[1,2],[3,4]]).values()", mapSym: "new Map([[1,2]])[Symbol.iterator]()", setValues: "new Set([1,2]).values()",
  setKeys: "new Set([1,2]).keys()", setEntries: "new Set([1,2]).entries()", setSym: "new Set([1,2])[Symbol.iterator]()",
  re: "'a1b2'.matchAll(/\\d/g)", u8Values: "new Uint8Array([1,2]).values()", u8Keys: "new Uint8Array([1,2]).keys()",
  u8Entries: "new Uint8Array([1,2]).entries()", args: "(function(){return arguments[Symbol.iterator]()})(1,2)",
  gen: "(function*(){yield 1})()", wrap: "Iterator.from({next(){return {done:true}}})", helper: "[1,2].values().map(x=>x)",
  agen: "(async function*(){})()",
};
const probes = [
  "Object.prototype.toString.call(it)",
  "(()=>{var r=[],p=Object.getPrototypeOf(it);while(p){r.push(Object.prototype.toString.call(p)+'['+Reflect.ownKeys(p).map(String).join(',')+']');p=Object.getPrototypeOf(p)}return r.join(' > ')})()",
  "D(Object.getPrototypeOf(it),'next')", "D(Object.getPrototypeOf(it),Symbol.toStringTag)",
  "D(Object.getPrototypeOf(Object.getPrototypeOf(it)),Symbol.iterator)", "it[Symbol.iterator]===undefined?'none':it[Symbol.iterator]()===it",
  "typeof it.return+typeof it.throw", "Object.getPrototypeOf(it).next.length+Object.getPrototypeOf(it).next.name",
  "Reflect.ownKeys(it).length", "(()=>{var a=[];for(var i=0;i<4;i++){var r=it.next();a.push(S(r)+Object.keys(r))}return a.join(' ')})()",
  "[...it]", "Array.from(it)", "[[...it].length,[...it].length]", "(()=>{var [a]=it;return [a,it.next()]})()",
  "(()=>{for(var v of it){break}return it.next()})()", "(()=>{it.next();return [...it]})()",
  "Object.getOwnPropertyNames(Object.getPrototypeOf(it).next)", "Object.isExtensible(it)+','+Object.isFrozen(it)",
  "(()=>{var r=it.next();return Object.getPrototypeOf(r)===Object.prototype&&Reflect.ownKeys(r).join()})()",
  "(()=>{var r=it.next();return D(r,'value')+'/'+D(r,'done')})()", "(()=>{it.next=()=>({done:true});return [...it]})()",
  "(()=>{Object.getPrototypeOf(it).next=function(){return {done:true}};return [...it]})()",
  "(()=>{delete Object.getPrototypeOf(it).next;return [...it]})()",
  "(()=>{Object.getPrototypeOf(it)[Symbol.toStringTag]='Z';return Object.prototype.toString.call(it)})()",
  "(()=>{var p=Object.getPrototypeOf(it);return T(()=>new p.next())})()", "(()=>{var p=Object.getPrototypeOf(it);return T(()=>p.next())})()",
  "typeof Object.getPrototypeOf(it)[Symbol.iterator]", "Object.getPrototypeOf(it).hasOwnProperty(Symbol.iterator)",
  "Reflect.ownKeys(Object.getPrototypeOf(it)).map(k=>typeof k==='symbol'?k.toString():k).join()",
  "(()=>{var p=Object.getPrototypeOf(it);return Object.getPrototypeOf(p)===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))})()",
  "(()=>{var p=Object.getPrototypeOf(it);return Object.getOwnPropertyNames(p).map(k=>k+':'+D(p,k)).join(' | ')})()",
];
for (const [name, k] of Object.entries(kinds)) for (const p of probes) add(`T(()=>{var it=${k};return ${p}})`);

// ---- 4. `next` com this inválido, de cada espécie, e `next` de uma espécie chamado em outra.
const thises = [
  "undefined", "null", "0", "'s'", "true", "Symbol()", "1n", "{}", "[]", "function(){}", "Object.create(it)", "new Proxy(it,{})",
  "[1].values()", "new Map().entries()", "new Set().values()", "'x'[Symbol.iterator]()", "(function*(){})()", "new Uint8Array(1).values()",
];
for (const [name, k] of Object.entries(kinds)) for (const t of thises) {
  add(`T(()=>{var it=${k};var nx=Object.getPrototypeOf(it).next;return nx.call(${t})})`);
}
const crossKinds = ["arrValues", "str", "mapEntries", "setValues", "re", "u8Values", "gen", "helper"];
for (const a of crossKinds) for (const b of crossKinds) {
  add(`T(()=>{var nx=Object.getPrototypeOf(${kinds[a]}).next;return nx.call(${kinds[b]})})`);
}

// ---- 5. Mutação durante a iteração.
const arrayInits = ["[1,2,3]", "[1,2,3,4,5]", "[1,,3]", "[]", "[1]"];
const arrayOps = [
  "a.push(9)", "a.pop()", "a.shift()", "a.unshift(0)", "a.length=0", "a.length=1", "a.splice(0,1)", "a.splice(1,0,'x')", "a.reverse()", "a.sort()",
  "a[a.length+2]=7", "delete a[2]", "a.fill(0)", "a.copyWithin(0,1)",
];
for (const init of arrayInits) for (const op of arrayOps) for (const at of [0, 1]) for (const m of ["values", "keys", "entries"]) {
  add(`T(()=>{var a=${init},o=[],i=0;for(var v of a.${m}()){o.push(v);if(i++===${at})${op};if(o.length>12)break}return o})`);
}
for (const init of arrayInits) for (const op of arrayOps) {
  add(`T(()=>{var a=${init},it=a.values();it.next();${op};return [it.next(),it.next(),it.next()]})`);
  add(`T(()=>{var a=${init},it=a.entries();var r=[...it];${op};return [r.length,it.next()]})`);
}
const mapInits = ["new Map([[1,'a'],[2,'b'],[3,'c']])", "new Map()", "new Map([[1,1]])"];
const mapOps = ["m.delete(1)", "m.delete(2)", "m.set(4,'d')", "m.set(1,'z')", "m.clear()", "m.clear();m.set(9,9)", "m.delete(2);m.set(2,'again')", "m.set(2,'v')"];
for (const init of mapInits) for (const op of mapOps) for (const at of [0, 1]) for (const m of ["entries", "keys", "values"]) {
  add(`T(()=>{var m=${init},o=[],i=0;for(var v of m.${m}()){o.push(v);if(i++===${at})${op};if(o.length>12)break}return o})`);
}
const setInits = ["new Set([1,2,3])", "new Set()", "new Set([1])"];
const setOps = ["m.delete(1)", "m.delete(2)", "m.add(4)", "m.add(1)", "m.clear()", "m.clear();m.add(9)", "m.delete(2);m.add(2)", "m.add(2)"];
for (const init of setInits) for (const op of setOps) for (const at of [0, 1]) for (const m of ["values", "entries"]) {
  add(`T(()=>{var m=${init},o=[],i=0;for(var v of m.${m}()){o.push(v);if(i++===${at})${op};if(o.length>12)break}return o})`);
}
for (const op of mapOps) add(`T(()=>{var m=new Map([[1,1],[2,2]]),it=m.entries();var r=[...it];${op};return [r.length,it.next(),[...m.keys()]]})`);
for (const op of setOps) add(`T(()=>{var m=new Set([1,2]),it=m.values();var r=[...it];${op};return [r.length,it.next(),[...m]]})`);

// ---- 6. Symbol.iterator e métodos trocados em protótipos built-in.
const overrides = [
  "Array.prototype[Symbol.iterator]=function*(){yield 'ov'}", "Array.prototype[Symbol.iterator]=undefined", "delete Array.prototype[Symbol.iterator]",
  "Object.getPrototypeOf([][Symbol.iterator]()).next=function(){return {done:true}}",
  "var N=Object.getPrototypeOf([][Symbol.iterator]()).next;Object.getPrototypeOf([][Symbol.iterator]()).next=function(){L.push('an');return N.call(this)}",
  "Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))[Symbol.iterator]=function(){L.push('ip');return this}",
  "Object.defineProperty(Array.prototype,Symbol.iterator,{get(){L.push('getter');return function*(){yield 1}}})",
  "String.prototype[Symbol.iterator]=function*(){yield 's'}", "Map.prototype[Symbol.iterator]=function*(){yield [7,8]}",
  "Map.prototype.entries=function*(){yield [7,8]}", "Set.prototype[Symbol.iterator]=function*(){yield 5}", "Set.prototype.values=function*(){yield 5}",
  "Set.prototype.add=function(v){L.push('add '+v);return this}", "Map.prototype.set=function(k,v){L.push('set '+k);return this}",
  "WeakMap.prototype.set=function(k,v){L.push('wset');return this}", "WeakSet.prototype.add=function(v){L.push('wadd');return this}",
  "Uint8Array.prototype[Symbol.iterator]=function*(){yield 3}", "Object.getPrototypeOf(Uint8Array.prototype)[Symbol.iterator]=function*(){yield 3}",
  "Object.getPrototypeOf(Int8Array).prototype.values=function*(){yield 3}", "Array.prototype.values=function*(){yield 'v'}",
  "Array.prototype.push=function(){L.push('push');return 0}", "Object.defineProperty(Array.prototype,0,{set(v){L.push('set0')},configurable:true})",
  "Array.prototype[Symbol.iterator]=function(){return {next(){L.push('x');return {done:true}}}}",
  "Object.prototype[Symbol.iterator]=function*(){yield 'obj'}", "Function.prototype[Symbol.iterator]=function*(){yield 'fn'}",
  "Number.prototype[Symbol.iterator]=function*(){yield 'n'}", "Object.prototype.next=function(){L.push('OPnext');return {done:true}}",
  "Object.prototype.return=function(){L.push('OPreturn');return {}}", "Object.prototype.throw=function(){L.push('OPthrow');return {done:true}}",
  "Object.prototype.done=true", "Object.prototype.value='pv'",
  "Object.getPrototypeOf(function*(){}).prototype.next=function(){return {done:true}}",
  "Object.getPrototypeOf(Object.getPrototypeOf((function*(){})())).next=function(){return {done:true}}",
  "Symbol.iterator", "Object.defineProperty(Symbol,'iterator',{value:Symbol('x')})",
];
const overrideConsumers = [
  "[...[1,2]]", "[...'ab']", "[...new Map([[1,2]])]", "[...new Set([1,2])]", "[...new Uint8Array([1,2])]", "Array.from([1,2])", "Array.from('ab')",
  "(()=>{var [a,b]=[1,2];return [a,b]})()", "(()=>{var s=[];for(var v of [1,2])s.push(v);return s})()", "(()=>{var s=[];for(var v of [1,2,3]){s.push(v);break}return s})()",
  "[...new Map([[1,2]]).keys()]", "[...new Set([1,2]).entries()]", "new Set([1,2]).size", "[...new Set([3,4])]", "new Map([[1,2],[3,4]]).size",
  "Object.fromEntries([['a',1]])", "(()=>{var s=new Set([1,2]);return [...new Set(s)]})()", "[...(function(){return arguments})(1,2)]",
  "(()=>{var f=function(){return arguments.length};return f(...[1,2,3])})()", "Math.max(...[1,5])", "[...{}]", "[...5]", "(()=>{var [a]={};return a})()",
  "new WeakSet([{}]) instanceof WeakSet", "new WeakMap([[{},1]]) instanceof WeakMap", "Array.from({length:2,0:'a',1:'b'})",
  "[...(function*(){yield* [1,2]})()]", "[...[1,2].entries()]", "Promise.all([1,2]).constructor===Promise",
];
for (const ov of overrides) for (const c of overrideConsumers) {
  add(`T(()=>{var L=[];${ov};var r=T(()=>${c});return r+' /'+L.join()})`);
}

// ---- 7. Symbol.iterator e métodos trocados em instâncias.
const instances = [
  "var x=[1,2,3];x[Symbol.iterator]=function*(){yield 'own'}", "var x=[1,2,3];x[Symbol.iterator]=undefined", "var x=[1,2,3];x[Symbol.iterator]=null",
  "var x=[1,2,3];x[Symbol.iterator]=1", "var x=[1,2,3];Object.defineProperty(x,Symbol.iterator,{get(){L.push('g');return Array.prototype.values}})",
  "var x=[1,2,3];x[Symbol.iterator]=function(){L.push('c');return [9][Symbol.iterator]()}",
  "var x=new String('ab');x[Symbol.iterator]=function*(){yield 'S'}", "var x=new Map([[1,2]]);x[Symbol.iterator]=function*(){yield [7,8]}",
  "var x=new Set([1,2]);x[Symbol.iterator]=function*(){yield 5}", "var x=new Set([1,2]);x.values=function*(){yield 5}",
  "var x=new Uint8Array([1,2]);x[Symbol.iterator]=function*(){yield 4}", "var x=(function(){return arguments})(1,2);x[Symbol.iterator]=function*(){yield 'a'}",
  "var x=(function(){return arguments})(1,2);delete x[Symbol.iterator]", "var x=(function(){return arguments})(1,2)", "var x=(function(){'use strict';return arguments})(1,2)",
  "var x=[1,2].values();x[Symbol.iterator]=function(){return [8,9].values()}", "var x=[1,2].values();x.next=function(){return {done:true}}",
  "var x=new Map([[1,2]]).entries();x.next=function(){L.push('n');return {done:true}}", "var x={length:2,0:'a',1:'b',[Symbol.iterator]:Array.prototype.values}",
  "var x={__proto__:Array.prototype,length:2,0:'a',1:'b'}", "var x=Object.create([1,2,3])", "var x=new Proxy([1,2],{get(t,k,r){L.push(String(k));return Reflect.get(t,k,r)}})",
  "var x=new (class extends Array{})(1,2)", "var x=new (class extends Array{*[Symbol.iterator](){yield 'sub'}})(1,2)",
  "var x=new (class extends Map{*[Symbol.iterator](){yield [1,'sub']}})()", "var x=new (class extends Set{*values(){yield 'sub'}})([1])",
  "var x=new (class extends Set{})([1,2])", "var x=Object.assign(Object.create(null),{length:1,0:'z',[Symbol.iterator]:Array.prototype[Symbol.iterator]})",
];
for (const setup of instances) for (const c of consumers.filter((_, i) => i < 42 && ![25, 26, 27, 28, 36, 37].includes(i) || coreConsumers.includes(consumers[i]))) {
  add(`T(()=>{var L=[];${setup};var r=T(()=>${c.replace(/\bX\b/g, "x")});return r+' /'+L.join()})`);
}

// ---- 8. Iteradores de String com surrogates.
const strings = [
  "''", "'a'", "'\\ud83d\\ude00'", "'\\ud83d'", "'\\ude00'", "'a\\ud83d'", "'\\ud83dx'", "'\\ud83d\\ud83d\\ude00'", "'\\ude00\\ud83d'", "'e\\u0301'",
  "'\\u{1F468}\\u200d\\u{1F469}'", "'ab\\ud83d\\ude00cd'", "'\\0'", "'a\\u2028'", "'\\ud800\\udc00\\udbff\\udfff'", "new String('x\\ud83d\\ude00')",
];
const stringOps = [
  "[...s].map(c=>c.length+':'+c.codePointAt(0).toString(16))", "Array.from(s).length", "(()=>{var r=[];for(var c of s)r.push(c.length);return r})()",
  "(()=>{var [a,b]=s;return [a,b]})()", "Array.from(s,c=>c.charCodeAt(0))", "new Set(s).size", "s.length-[...s].length", "[...s].join('|')",
  "(()=>{var it=s[Symbol.iterator]();return [it.next(),it.next(),it.next(),it.next()]})()", "Object.fromEntries([...s].map((c,i)=>[i,c]))",
  "(()=>{var it=s[Symbol.iterator]();it.next();return [...it]})()", "[...s].reverse().join('')", "Array.from({length:1,0:s}).flatMap(x=>[...x]).length",
  "(()=>{var m=new Map();for(var c of s)m.set(c,(m.get(c)||0)+1);return [...m]})()", "[...s.matchAll(/./gu)].length+','+[...s.matchAll(/./g)].length",
  "[...s.split('')].length+','+s.split(/(?:)/u).length",
];
for (const s of strings) for (const op of stringOps) add(`T(()=>{var s=${s};return ${op}})`);
const strIterThis = ["undefined", "null", "0", "1n", "true", "Symbol()", "{}", "{toString(){return 'ob'}}", "[1,2]", "function(){}", "new String('ns')", "123", "-0", "NaN", "{toString(){throw new RangeError('ts')}}"];
for (const t of strIterThis) {
  add(`T(()=>[...String.prototype[Symbol.iterator].call(${t})])`, `T(()=>String.prototype[Symbol.iterator].call(${t}).next())`,
    `T(()=>Object.prototype.toString.call(String.prototype[Symbol.iterator].call(${t})))`);
}
add(
  "T(()=>String.prototype[Symbol.iterator].name+String.prototype[Symbol.iterator].length)", "T(()=>D(String.prototype,Symbol.iterator))",
  "T(()=>new String.prototype[Symbol.iterator]())", "T(()=>String.prototype[Symbol.iterator]())",
  "T(()=>{var s=new String('ab');var it=s[Symbol.iterator]();s.length;String.prototype.concat;return [...it]})",
  "T(()=>{var it='ab'[Symbol.iterator]();return Object.getPrototypeOf(it)===Object.getPrototypeOf('cd'[Symbol.iterator]())})",
  "T(()=>{var o={toString(){L.push('ts');return 'xy'}};var L=[];var it=String.prototype[Symbol.iterator].call(o);o.toString=()=>'zz';return [L.join(),[...it]]})",
);

// ---- 9. Array.prototype.{values,keys,entries} em array-likes, esparsos e this estranho.
const arrayLikes = [
  "{length:2,0:'a',1:'b'}", "'ab'", "{}", "{length:-1}", "{length:2**53}", "{get length(){L.push('len');return 2}}",
  "new Proxy({length:2,0:1},{get(t,k){L.push('get '+String(k));return t[k]}})", "function(a,b){}", "1", "true", "1n", "null", "undefined", "Symbol()",
  "new Uint8Array(2)", "new String('xy')", "{length:3,1:'x'}", "{length:'2',0:'q'}", "{length:{valueOf(){L.push('vo');return 1}}}", "Object('s')",
  "(function(){return arguments})(7,8)", "new Map([[1,2]])", "{length:1.9,0:'f'}", "{length:NaN,0:'n'}",
];
for (const a of arrayLikes) for (const m of ["values", "keys", "entries"]) {
  add(`T(()=>{var L=[];var it=Array.prototype.${m}.call(${a});return [S([it.next(),it.next(),it.next()]),L.join()]})`);
  add(`T(()=>{var L=[];var it=Array.prototype.${m}.call(${a});var r=[];for(var i=0;i<3;i++)r.push(it.next().done);return [r,L.join()]})`);
}
add(
  "T(()=>{var o={length:3,0:'a',1:'b',2:'c'};var it=Array.prototype.values.call(o);it.next();o.length=1;return [it.next(),it.next()]})",
  "T(()=>{var o={length:1,0:'a'};var it=Array.prototype.values.call(o);it.next();o.length=3;o[1]='b';return [it.next(),it.next()]})",
  "T(()=>{var o={length:3,0:'a',1:'b',2:'c'};var it=Array.prototype.keys.call(o);it.next();o.length=0;return [it.next(),it.next()]})",
  "T(()=>{var o={length:2,0:'a'};var it=Array.prototype.entries.call(o);return [it.next(),it.next(),it.next()]})",
  "T(()=>{var a=[1,2];var it=a.values();a.length=0;var r=it.next();a.push(5);return [r,it.next()]})",
  "T(()=>{var a=[1,2];var it=a.values();it.next();it.next();it.next();a.push(5);return it.next()})",
  "T(()=>{var a=[1,2];var it=a.keys();it.next();it.next();it.next();a.push(5);return [it.next(),a.length]})",
  "T(()=>{var o={get length(){throw new RangeError('gl')}};var it=Array.prototype.values.call(o);return it.next()})",
  "T(()=>{var o={length:1,get 0(){throw new RangeError('g0')}};var it=Array.prototype.values.call(o);return it.next()})",
  "T(()=>{var o={length:1,get 0(){throw new RangeError('g0')}};var it=Array.prototype.keys.call(o);return it.next()})",
  "T(()=>Array.prototype.values===Array.prototype[Symbol.iterator])", "T(()=>Array.prototype.values.name+Array.prototype.values.length)",
  "T(()=>Array.prototype.entries.name+Array.prototype.keys.length)", "T(()=>D(Array.prototype,Symbol.iterator))", "T(()=>D(Array.prototype,'values'))",
  "T(()=>Array.prototype[Symbol.unscopables].values+','+Object.getPrototypeOf(Array.prototype[Symbol.unscopables]))",
  "T(()=>Object.keys(Array.prototype[Symbol.unscopables]).join())", "T(()=>[].values().toString())", "T(()=>String([].entries()))",
  "T(()=>{var a=[1,2];return a[Symbol.iterator]===a.values})", "T(()=>Uint8Array.prototype[Symbol.iterator]===Uint8Array.prototype.values)",
  "T(()=>Object.getPrototypeOf(Uint8Array.prototype)[Symbol.iterator]===Object.getPrototypeOf(Uint8Array.prototype).values)",
  "T(()=>Map.prototype[Symbol.iterator]===Map.prototype.entries)", "T(()=>Set.prototype[Symbol.iterator]===Set.prototype.values)",
  "T(()=>Set.prototype.keys===Set.prototype.values)", "T(()=>Map.prototype.keys===Set.prototype.keys)",
  "T(()=>String.prototype[Symbol.iterator]===Array.prototype.values)", "T(()=>RegExp.prototype[Symbol.matchAll].name+RegExp.prototype[Symbol.matchAll].length)",
  "T(()=>(function(){return arguments[Symbol.iterator]===Array.prototype.values})())", "T(()=>(function(){return D(arguments,Symbol.iterator)})())",
  "T(()=>(function(){'use strict';return D(arguments,Symbol.iterator)})())", "T(()=>(function(){return Object.getOwnPropertyNames(arguments).join()+Reflect.ownKeys(arguments).length})())",
  "T(()=>(function(a){var it=arguments[Symbol.iterator]();a=9;return it.next()})(1))", "T(()=>(function(a){var it=arguments.values?1:0;return it})(1))",
  "T(()=>(function(a,b){var it=arguments[Symbol.iterator]();it.next();arguments.length=5;return [it.next(),it.next(),it.next()]})(1,2))",
  "T(()=>(function(a,b){var it=arguments[Symbol.iterator]();it.next();arguments.length=1;return [it.next()]})(1,2))",
  "T(()=>(function(){var it=arguments[Symbol.iterator]();delete arguments[0];return [it.next(),it.next()]})(1,2))",
  "T(()=>(function(){return [...arguments]})(...[1,,3]))", "T(()=>(function(){return Array.from(arguments)})(undefined,null))",
  "T(()=>(function(){var r=[];for(var v of arguments){r.push(v);if(r.length<3)arguments.length++}return r})(1))",
  "T(()=>(function(){return Object.prototype.toString.call(arguments[Symbol.iterator]())})(1))",
  "T(()=>(function(){var a=arguments;return (()=>[...a])()})(5,6))",
);
const sparse = ["[,,]", "[1,,3]", "[,1]", "new Array(3)", "[1,2,,]", "Object.assign([],{5:1})", "[undefined,,undefined]", "[...Array(2)]", "[,'a',,'b']"];
for (const a of sparse) for (const m of ["values", "keys", "entries", "[Symbol.iterator]"]) for (const c of ["S([...it])", "S(Array.from(it))", "Object.keys([...it]).join()", "S(it.next())+S(it.next())"]) {
  const call = m.startsWith("[") ? `a${m}()` : `a.${m}()`;
  add(`T(()=>{var a=${a};var it=${call};return ${c}})`);
}
for (const m of ["values", "entries", "[Symbol.iterator]"]) for (const proto of ["Array.prototype[1]='p'", "Array.prototype[0]='q'", "Object.prototype[1]='o'", "Array.prototype[2]='z'"]) {
  const call = m.startsWith("[") ? `a${m}()` : `a.${m}()`;
  add(`T(()=>{${proto};var a=[0,,2];return S([...${call}])})`, `T(()=>{${proto};var a=[,,];return S(Array.from(${call}))})`);
}

// ---- 10. TypedArray: destacado e redimensionado.
const typedKinds = ["Uint8Array", "Int8Array", "Uint8ClampedArray", "Int16Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
for (const K of typedKinds) for (const m of ["values", "keys", "entries"]) {
  add(
    `T(()=>{var ta=new ${K}(3);var it=ta.${m}();it.next();ta.buffer.transfer();return [it.next()]})`,
    `T(()=>{var ta=new ${K}(3);ta.buffer.transfer();return ta.${m}()})`,
    `T(()=>{var ta=new ${K}(3);var it=ta.${m}();ta.buffer.transfer();return it.next()})`,
    `T(()=>{var ta=new ${K}(3),o=[];for(var v of ta.${m}()){o.push(v);ta.buffer.transfer()}return o})`,
    `T(()=>{var ta=new ${K}(2);var it=ta.${m}();it.next();it.next();it.next();ta.buffer.transfer();return it.next()})`,
    `T(()=>{var ta=new ${K}(2);var it=ta.${m}();ta.buffer.transfer();try{it.next()}catch(e){}return it.next()})`,
    `T(()=>{var ta=new ${K}(2);var ab=ta.buffer;ab.transfer();return [...${K}.prototype.${m}.call(new ${K}(1))].length+ta.length+ta.byteLength})`,
    `T(()=>{var ta=new ${K}(2);return [...ta.${m}()].length+Object.prototype.toString.call(ta.${m}())})`,
  );
}
const views = {
  tracking: (K) => `new ${K}(ab)`, fixed2: (K) => `new ${K}(ab,0,2)`, offsetTracking: (K) => `new ${K}(ab,${K}.BYTES_PER_ELEMENT)`,
  offsetFixed: (K) => `new ${K}(ab,${K}.BYTES_PER_ELEMENT,2)`,
};
for (const K of ["Uint8Array", "Int16Array", "Float64Array", "BigInt64Array"]) for (const [vn, view] of Object.entries(views)) for (const m of ["values", "keys", "entries"]) {
  for (const op of ["ab.resize(0)", "ab.resize(bpe)", "ab.resize(8*bpe)", "ab.resize(2*bpe)", "ab.resize(3*bpe)"]) for (const at of [0, 1, 2]) {
    add(`T(()=>{var bpe=${K}.BYTES_PER_ELEMENT;var ab=new ArrayBuffer(4*bpe,{maxByteLength:8*bpe});var ta=${view(K)};var o=[],i=0;for(var v of ta.${m}()){o.push(typeof v==='bigint'?v+'n':v);if(i++===${at})${op};if(o.length>12)break}return [o,ta.length,ta.byteLength]})`);
  }
  add(`T(()=>{var bpe=${K}.BYTES_PER_ELEMENT;var ab=new ArrayBuffer(4*bpe,{maxByteLength:8*bpe});var ta=${view(K)};ab.resize(0);return [...ta.${m}()]})`);
  add(`T(()=>{var bpe=${K}.BYTES_PER_ELEMENT;var ab=new ArrayBuffer(4*bpe,{maxByteLength:8*bpe});var ta=${view(K)};var it=ta.${m}();it.next();ab.resize(0);var r=[];try{r.push(it.next())}catch(e){r.push(e.name+': '+e.message)}ab.resize(4*bpe);try{r.push(it.next())}catch(e){r.push(e.name+': '+e.message)}return r})`);
}

// ---- 11. Map, Set, WeakMap, WeakSet: construtores e receptores.
const badThises = ["undefined", "null", "0", "'s'", "{}", "[]", "new Set()", "new Map()", "new WeakMap()", "function(){}", "Symbol()", "new Proxy(new Map(),{})", "Object.create(new Map())", "new Uint8Array(1)"];
for (const t of badThises) for (const m of ["Map.prototype.entries", "Map.prototype.keys", "Map.prototype.values", "Map.prototype[Symbol.iterator]", "Set.prototype.values", "Set.prototype.entries", "Set.prototype[Symbol.iterator]"]) {
  add(`T(()=>${m}.call(${t}).next())`);
}
add(
  "T(()=>new Map([[1]]).get(1))", "T(()=>new Map([1]))", "T(()=>new Map([[1,2,3]]).get(1))", "T(()=>new Map('ab'))", "T(()=>new Map(null).size)", "T(()=>new Map(undefined).size)",
  "T(()=>new Set(null).size)", "T(()=>new Set(1))", "T(()=>new Set('aab').size)", "T(()=>new WeakMap([[1,2]]))", "T(()=>new WeakSet([1]))", "T(()=>new WeakMap([[{},1]]) instanceof WeakMap)",
  "T(()=>Map([]))", "T(()=>Set([]))", "T(()=>WeakMap([]))", "T(()=>new Map(new Map([[1,2]])).get(1))", "T(()=>[...new Set(new Map([[1,2]]))])",
  "T(()=>[...new Map(new Set([[1,2]]))])", "T(()=>[...new Map([[NaN,1],[NaN,2]]).keys()])", "T(()=>[...new Set([NaN,NaN,0,-0])].map(x=>Object.is(x,-0)))",
  "T(()=>[...new Map([[-0,1]]).keys()].map(x=>Object.is(x,-0)))",
  "T(()=>{var m=new Map([[1,2]]);var a=m.entries().next().value,b=m.entries().next().value;return a===b})",
  "T(()=>{var m=new Map([[1,2]]);var a=m.entries().next().value;a[1]=9;return [m.get(1),Array.isArray(a),Object.isFrozen(a),a.length]})",
  "T(()=>{var s=new Set([1]);var a=s.entries().next().value;return [a,a[0]===a[1]]})",
  "T(()=>{var o={};var s=new Set([o]);return s.entries().next().value[1]===o})",
  "T(()=>{var L=[];var ent={get 0(){L.push('k');return 1},get 1(){L.push('v');return 2}};new Map([ent]);return L.join()})",
  "T(()=>{var L=[];var m=new Map();var orig=Map.prototype.set;Map.prototype.set=function(k,v){L.push('s'+k);return orig.call(this,k,v)};new Map([[1,2],[3,4]]);return L.join()})",
  "T(()=>{var L=[];Object.defineProperty(Map.prototype,'set',{get(){L.push('get set');return function(){return this}},configurable:true});new Map([[1,2]]);new Map();return L.join()})",
  "T(()=>{var L=[];Object.defineProperty(Set.prototype,'add',{get(){L.push('get add');return function(){return this}},configurable:true});new Set([1,2]);new Set();return L.join()})",
  "T(()=>{Map.prototype.set=1;return new Map([[1,2]])})", "T(()=>{Map.prototype.set=1;return new Map().size})", "T(()=>{Set.prototype.add=undefined;return new Set([1])})",
  "T(()=>{Set.prototype.add=undefined;return new Set().size})", "T(()=>{WeakMap.prototype.set=null;return new WeakMap([[{},1]])})",
  "T(()=>{var L=[];var it={[Symbol.iterator](){return {next(){L.push('n');return {done:true}}}}};new Set(it);new Map(it);new WeakSet(it);new WeakMap(it);return L.join()})",
  "T(()=>{var L=[];Set.prototype.add=function(v){L.push(v);if(v===2)throw new RangeError('add');return this};try{new Set([1,2,3])}catch(e){L.push(e.name)}return L.join()})",
  "T(()=>{var L=[];var it={[Symbol.iterator](){return {next(){return {done:false,value:1}},return(){L.push('r');return {}}}}};try{new Map(it)}catch(e){L.push(e.name+': '+e.message)}return L.join()})",
  "T(()=>{var L=[];var it={[Symbol.iterator](){return {next(){return {done:false,value:[1,2]}},return(){L.push('r');return {}}}}};Set.prototype.add=function(){throw new RangeError('x')};try{new Set(it)}catch(e){L.push(e.name)}return L.join()})",
  "T(()=>{var L=[];var set={size:1,has(){L.push('has');return true},keys(){L.push('keys');return [2][Symbol.iterator]()}};return [S([...new Set([1]).union(set)]),L.join()]})",
  "T(()=>{var L=[];var set={size:1,has(){L.push('has');return true},keys(){L.push('keys');return {next(){L.push('next');return {done:true}},return(){L.push('ret');return {}}}}};return [S([...new Set([1]).isSupersetOf(set)?[1]:[0]]),L.join()]})",
  "T(()=>{var L=[];var set={size:5,has(){L.push('has');return false},keys(){L.push('keys');return {next(){L.push('next');return {done:false,value:1}},return(){L.push('ret');return {}}}}};return [new Set([1,2]).isSupersetOf(set),L.join()]})",
  "T(()=>{var L=[];var set={size:5,has(v){L.push('has'+v);return false},keys(){L.push('keys');return [].values()}};return [S([...new Set([1,2]).intersection(set)]),L.join()]})",
  "T(()=>Map.groupBy('abca',c=>c).size)", "T(()=>S(Object.groupBy('abca',c=>c)))", "T(()=>S(Object.groupBy(new Set([1,2,3]),v=>v%2?'o':'e')))",
  "T(()=>S([...Map.groupBy(new Map([[1,2],[3,4]]),([k,v])=>k>1)]))", "T(()=>Object.groupBy(1,x=>x))", "T(()=>Map.groupBy(null,x=>x))", "T(()=>Object.groupBy([1],1))",
  "T(()=>S(Object.fromEntries(new Map([[1,2],['a',3]]))))", "T(()=>S(Object.fromEntries('ab')))", "T(()=>Object.fromEntries([1]))", "T(()=>Object.fromEntries(1))",
  "T(()=>Object.fromEntries(null))", "T(()=>S(Object.fromEntries([['a',1],['a',2]])))", "T(()=>S(Object.fromEntries([[Symbol.iterator,1]])))", "T(()=>S(Object.fromEntries([[{toString(){return 'k'}},1]])))",
  "T(()=>S(Object.fromEntries(new Set([['x',1]]))))", "T(()=>S(Object.fromEntries([{0:'a',1:'b'}])))", "T(()=>S(Object.fromEntries((function*(){yield ['g',1]})())))",
  "T(()=>S(Object.fromEntries([['__proto__',1]])))",
);

// ---- 12. RegExp String Iterator.
const reStrings = ["'a1b22c333'", "''", "'abc'", "'\\ud83d\\ude00\\ud83d\\ude00'", "'x'", "'aXbX'", "'\\n\\n'"];
const regexes = ["/\\d+/g", "/\\d*/g", "/(?:)/g", "/./gu", "/./g", "/a|b/gi", "/(\\d)(\\w)?/g", "/\\d/gy", "/x/", "/\\d/", "/(?<n>\\d)/g", "/^/gm", "/\\b/g"];
const reConsumers = [
  "S([...s.matchAll(r)].map(m=>m[0]+'@'+m.index))", "S(Array.from(s.matchAll(r),m=>m.length))", "S(s.matchAll(r).next())",
  "(()=>{var it=s.matchAll(r);return S([it.next().done,it.next().done,it.next().done])})()", "S([...s.matchAll(r)].map(m=>Object.keys(m).join()+JSON.stringify(m.groups)))",
];
for (const s of reStrings) for (const r of regexes) for (const c of reConsumers) add(`T(()=>{var s=${s},r=${r};return ${c}})`);
const reThises = ["undefined", "null", "0", "'s'", "{}", "[]", "/x/g", "/x/", "{flags:'g',[Symbol.matchAll]:RegExp.prototype[Symbol.matchAll]}", "function(){}", "new Proxy(/x/g,{})"];
for (const t of reThises) {
  add(`T(()=>RegExp.prototype[Symbol.matchAll].call(${t},'xx').next())`, `T(()=>[...RegExp.prototype[Symbol.matchAll].call(${t},'axbx')])`);
}
const matchAllArgs = ["'a'", "'.'", "null", "undefined", "1", "{[Symbol.matchAll]:undefined}", "{[Symbol.matchAll](s){return 'custom'+s}}", "{flags:'g'}", "{flags:'',[Symbol.match]:true}", "/a/", "/a/g", "new RegExp('a','g')", "{toString(){return 'a'}}", "Symbol()"];
for (const a of matchAllArgs) add(`T(()=>S('aXa'.matchAll(${a})))`, `T(()=>S([...'aXa'.matchAll(${a})].map(m=>m[0])))`);
add(
  "T(()=>{var r=/\\d/g;r.lastIndex=2;var a=[...'1234'.matchAll(r)].map(m=>m.index);return [a,r.lastIndex]})",
  "T(()=>{var r=/\\d/g;var it='1234'.matchAll(r);r.lastIndex=3;return [S([...it].map(m=>m.index)),r.lastIndex]})",
  "T(()=>{var r=/\\d/g;var it='1234'.matchAll(r);it.next();r.lastIndex=0;return S([...it].map(m=>m.index))})",
  "T(()=>{var r=/\\d/g;var it='1234'.matchAll(r);it.next();it.next();return r.lastIndex})",
  "T(()=>{var r=/\\d/g;var it='1234'.matchAll(r);it.next();r.compile('x','g');return S(it.next())})",
  "T(()=>{var r=/(?:)/gu;return S([...'\\ud83d\\ude00'.matchAll(r)].map(m=>m.index))})", "T(()=>{var r=/(?:)/g;return S([...'\\ud83d\\ude00'.matchAll(r)].map(m=>m.index))})",
  "T(()=>{var L=[];class R extends RegExp{exec(s){L.push('exec'+this.lastIndex);return super.exec(s)}};return [S([...'a1b2'.matchAll(new R('\\\\d','g'))].map(m=>m[0])),L.join()]})",
  "T(()=>{var L=[];var r=/\\d/g;r.exec=function(s){L.push('exec');return null};return [S([...'1'.matchAll(r)].length),L.join()]})",
  "T(()=>{var r=/\\d/g;r.exec=function(s){return 1};return [...'1'.matchAll(r)]})", "T(()=>{var r=/\\d/g;r.exec=1;return S([...'1'.matchAll(r)].length)})",
  "T(()=>{var L=[];var r=/\\d/g;Object.defineProperty(r,'flags',{get(){L.push('flags');return 'g'}});var it='1'.matchAll(r);return L.join()+typeof it.next})",
  "T(()=>{var L=[];var r=/\\d/g;Object.defineProperty(r,'constructor',{get(){L.push('ctor');return RegExp}});'1'.matchAll(r);return L.join()})",
  "T(()=>{var r=/\\d/g;r.constructor={[Symbol.species]:function(p,f){this.lastIndex=0;this.exec=()=>null;this.flags=f;}};return S([...'1'.matchAll(r)].length)})",
  "T(()=>{var r=/\\d/g;Object.defineProperty(r,'lastIndex',{writable:false,value:0});return [...'1'.matchAll(r)]})",
  "T(()=>{var r=/\\d/g;Object.freeze(r);return S([...'12'.matchAll(r)].length)})",
  "T(()=>{var it='1'.matchAll(/\\d/g);return Object.getPrototypeOf(it)===Object.getPrototypeOf('2'.matchAll(/x/g))})",
  "T(()=>{var it='1'.matchAll(/\\d/g);return Object.getPrototypeOf(Object.getPrototypeOf(it))===Object.getPrototypeOf(Object.getPrototypeOf([].values()))})",
  "T(()=>{var m=[...'ab'.matchAll(/(?<x>a)|(?<y>b)/g)];return m.map(a=>Object.keys(a.groups).join()+JSON.stringify(a.groups)).join(' ')})",
  "T(()=>{var m='ab'.matchAll(/a/g).next().value;return [Reflect.ownKeys(m).join(),m.input,m.index,Array.isArray(m)]})",
  "T(()=>String.prototype.matchAll.length+String.prototype.matchAll.name)", "T(()=>RegExp.prototype[Symbol.matchAll].call(/x/g,{toString(){throw new RangeError('s')}}))",
  "T(()=>S([...RegExp.prototype[Symbol.matchAll].call(/x/g,123)]))", "T(()=>S([...RegExp.prototype[Symbol.matchAll].call(/a/g)]))",
);

// ---- 13. %IteratorPrototype% e %AsyncIteratorPrototype%.
const IP = "Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))";
const AIP = "Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf((async function*(){})())))";
const AIP2 = "Object.getPrototypeOf(Object.getPrototypeOf((async function*(){}).prototype))";
add(
  `T(()=>${IP}===Iterator.prototype)`, `T(()=>Reflect.ownKeys(${IP}).map(String).join())`, `T(()=>D(${IP},Symbol.iterator))`, `T(()=>D(${IP},Symbol.toStringTag))`,
  `T(()=>D(${IP},'constructor'))`, `T(()=>${IP}[Symbol.iterator].name+${IP}[Symbol.iterator].length)`, `T(()=>${IP}[Symbol.iterator].call(7))`,
  `T(()=>${IP}[Symbol.iterator].call(undefined))`, `T(()=>${IP}[Symbol.iterator].call(null))`, `T(()=>${IP}[Symbol.iterator].call('s'))`,
  `T(()=>{var o={};return ${IP}[Symbol.iterator].call(o)===o})`, `T(()=>Object.getPrototypeOf(${IP})===Object.prototype)`, `T(()=>Object.isExtensible(${IP})+','+Object.isFrozen(${IP}))`,
  `T(()=>new ${IP}[Symbol.iterator]())`, `T(()=>Object.prototype.toString.call(${IP}))`, `T(()=>String(${IP}[Symbol.toStringTag]))`,
  `T(()=>{var o=Object.create(${IP});return [o[Symbol.iterator]()===o,Object.prototype.toString.call(o)]})`,
  `T(()=>{var o=Object.create(${IP});return [...o]})`, `T(()=>{var o=Object.create(${IP});o.next=()=>({done:true});return [...o]})`,
  `T(()=>{var o=Object.create(${IP});var i=0;o.next=()=>({done:i++>1,value:i});return [...o]})`,
  `T(()=>{var o=Object.create(${IP});var i=0;o.next=()=>({done:i++>5,value:i});for(var v of o){break}return i})`,
  `T(()=>{var o=Object.create(${IP});var L=[];o.next=()=>({done:false,value:1});o.return=()=>{L.push('r');return {}};for(var v of o){break}return L})`,
  `T(()=>{class It extends Iterator{next(){return {done:true}}};return [new It()[Symbol.iterator]() instanceof It,[...new It()]]})`,
  `T(()=>{class It extends Iterator{};var i=new It();return [Object.prototype.toString.call(i),i instanceof Iterator,typeof i.next]})`,
  `T(()=>new Iterator())`, `T(()=>Iterator())`, `T(()=>Iterator.prototype.constructor===Iterator)`, `T(()=>Iterator.length+Iterator.name)`,
  `T(()=>Reflect.ownKeys(Iterator.prototype).map(String).join())`, `T(()=>Reflect.ownKeys(Iterator).map(String).join())`,
  `T(()=>{Iterator.prototype[Symbol.iterator]=function(){return {next(){return {done:true}}}};return [...[1,2].values()]})`,
  `T(()=>{Iterator.prototype[Symbol.iterator]=undefined;return [...[1,2].values()]})`, `T(()=>{delete Iterator.prototype[Symbol.iterator];return [[1,2].values()[Symbol.iterator]]})`,
  `T(()=>{Iterator.prototype[Symbol.toStringTag]='Q';return Object.prototype.toString.call([].values())})`,
  `T(()=>{Iterator.prototype.constructor=1;return Object.prototype.toString.call([].values())})`,
  `T(()=>{var o={};Iterator.prototype[Symbol.toStringTag]='Q';return [Object.prototype.toString.call(o),Object.getOwnPropertyNames(o)]})`,
  `T(()=>{var o=Object.create(Iterator.prototype);o[Symbol.toStringTag]='Own';return [Object.prototype.toString.call(o),Object.getOwnPropertyNames(o)]})`,
  `T(()=>{var o=Object.create(Iterator.prototype);Object.defineProperty(o,Symbol.toStringTag,{value:'Own'});return Object.prototype.toString.call(o)})`,
  `T(()=>{Object.getPrototypeOf([].values())[Symbol.toStringTag]='Z';return Object.prototype.toString.call([].values())})`,
  `T(()=>{Object.getPrototypeOf(new Map().keys())[Symbol.toStringTag]='Z';return Object.prototype.toString.call(new Map().values())})`,
  `T(()=>{var p=Object.getPrototypeOf([].values());p[Symbol.toStringTag]='Z';return D(p,Symbol.toStringTag)})`,
  // Forma de cada membro do Iterator.prototype: descritor, name, length, ordem de chaves, receptor errado, fechamento.
  ...["toArray", "forEach", "some", "every", "find", "reduce", "map", "filter", "take", "drop", "flatMap", "chunks", "windows", "includes", "join"].flatMap((m) => [
    `T(()=>D(Iterator.prototype,${JSON.stringify(m)}))`,
    `T(()=>Iterator.prototype[${JSON.stringify(m)}].name+':'+Iterator.prototype[${JSON.stringify(m)}].length)`,
    `T(()=>Iterator.prototype[${JSON.stringify(m)}].call(undefined,()=>1))`,
    `T(()=>Iterator.prototype[${JSON.stringify(m)}].call(1,()=>1))`,
    `T(()=>{var L=[];var o=Object.create(Iterator.prototype);o.next=()=>({done:false,value:1});o.return=()=>{L.push('r');return {}};try{Iterator.prototype[${JSON.stringify(m)}].call(o,'bad')}catch(e){L.push(e.name+': '+e.message)}return L})`,
    `T(()=>{var L=[];var o=Object.create(Iterator.prototype);o.next=()=>({done:false,value:1});o.return=()=>{L.push('r');return {}};try{Iterator.prototype[${JSON.stringify(m)}].call(o,-1,-1)}catch(e){L.push(e.name+': '+e.message)}return L})`,
  ]),
  `T(()=>Reflect.ownKeys(Iterator.prototype).map(String).join())`, `T(()=>D(Iterator.prototype,Symbol.dispose))`,
  `T(()=>Iterator.prototype[Symbol.dispose].name+':'+Iterator.prototype[Symbol.dispose].length)`,
  `T(()=>Iterator.prototype[Symbol.dispose].call(1))`, `T(()=>Iterator.prototype[Symbol.dispose].call({}))`,
  `T(()=>{var L=[];Iterator.prototype[Symbol.dispose].call({return(){L.push('r');return 1}});return L})`,
  ...["from", "concat", "zip", "zipKeyed"].flatMap((m) => [
    `T(()=>D(Iterator,${JSON.stringify(m)}))`, `T(()=>Iterator[${JSON.stringify(m)}].name+':'+Iterator[${JSON.stringify(m)}].length)`,
    `T(()=>new Iterator[${JSON.stringify(m)}]())`,
  ]),
  `T(()=>D(Iterator,'length')+'|'+D(Iterator,'name')+'|'+D(Iterator,'prototype'))`,
  `T(()=>{var g=Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor');return [g.get.name,g.get.length,g.set.name,g.set.length]})`,
  `T(()=>{var g=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return [g.get.name,g.get.length,g.set.name,g.set.length]})`,
  `T(()=>Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').set.call(1,2))`,
  `T(()=>Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').set.call(Iterator.prototype,2))`,
  `T(()=>Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).set.call(Iterator.prototype,'x'))`,
  `T(()=>{var p=Object.getPrototypeOf([].values());return Object.getPrototypeOf(p)===Object.getPrototypeOf(Object.getPrototypeOf(new Map().keys()))})`,
  `T(()=>{var a=Object.getPrototypeOf([].values()),b=Object.getPrototypeOf(new Map().keys()),c=Object.getPrototypeOf(new Set().keys()),d=Object.getPrototypeOf(''[Symbol.iterator]()),e=Object.getPrototypeOf(''.matchAll(/x/g));return [a===b,b===c,c===d,d===e,a===e]})`,
  `T(()=>{var a=Object.getPrototypeOf(new Map().keys()),b=Object.getPrototypeOf(new Map().entries()),c=Object.getPrototypeOf(new Map()[Symbol.iterator]());return [a===b,b===c]})`,
  `T(()=>{var a=Object.getPrototypeOf(new Set().keys()),b=Object.getPrototypeOf(new Set().entries());return a===b})`,
  `T(()=>{var a=Object.getPrototypeOf([].keys()),b=Object.getPrototypeOf(new Uint8Array(1).keys());return a===b})`,
  `T(()=>{var a=Object.getPrototypeOf([].values()),b=Object.getPrototypeOf((function(){return arguments[Symbol.iterator]()})());return a===b})`,
  `T(()=>${AIP2}===${AIP})`, `T(()=>Reflect.ownKeys(${AIP}).map(String).join())`, `T(()=>D(${AIP},Symbol.asyncIterator))`,
  `T(()=>${AIP}[Symbol.asyncIterator].name+${AIP}[Symbol.asyncIterator].length)`, `T(()=>${AIP}[Symbol.asyncIterator].call(7))`,
  `T(()=>${AIP}[Symbol.asyncIterator].call(undefined))`, `T(()=>{var o={};return ${AIP}[Symbol.asyncIterator].call(o)===o})`,
  `T(()=>Object.getPrototypeOf(${AIP})===Object.prototype)`, `T(()=>Object.prototype.toString.call(${AIP}))`, `T(()=>${AIP}[Symbol.toStringTag])`,
  `T(()=>Object.getPrototypeOf(${AIP})===${IP})`, `T(()=>${AIP}.hasOwnProperty(Symbol.iterator))`, `T(()=>new ${AIP}[Symbol.asyncIterator]())`,
  `T(()=>Object.prototype.toString.call((async function*(){})()))`, `T(()=>{var g=(async function*(){})();return [g[Symbol.asyncIterator]()===g,g[Symbol.iterator]]})`,
  `T(()=>{var g=(async function*(){})();return [typeof g.next,typeof g.return,typeof g.throw,Reflect.ownKeys(g).length]})`,
  `T(()=>{var p=Object.getPrototypeOf((async function*(){})());return [Reflect.ownKeys(p).map(String).join(),D(p,Symbol.toStringTag)]})`,
  `T(()=>{var p=Object.getPrototypeOf((async function*(){})());return [D(p,'next'),p.next.length,p.return.length,p.throw.length]})`,
  `T(()=>{var p=Object.getPrototypeOf((async function*(){})());return p.next.call(1) instanceof Promise})`,
  `T(()=>{var p=Object.getPrototypeOf((async function*(){})());var pr=p.next.call({});pr.catch(()=>{});return [pr instanceof Promise,Object.prototype.toString.call(pr)]})`,
  `T(()=>{var p=Object.getPrototypeOf((async function*(){})());var pr=p.return.call(undefined);pr.catch(()=>{});return pr instanceof Promise})`,
  `T(()=>{var p=Object.getPrototypeOf((async function*(){})());var pr=p.throw.call(null,1);pr.catch(()=>{});return pr instanceof Promise})`,
  `T(()=>{var p=Object.getPrototypeOf((function*(){})());return [Reflect.ownKeys(p).map(String).join(),D(p,Symbol.toStringTag)]})`,
  `T(()=>{var p=Object.getPrototypeOf((function*(){})());return [p.next.call(1)]})`, `T(()=>{var p=Object.getPrototypeOf((function*(){})());return [p.return.call({})]})`,
  `T(()=>{var p=Object.getPrototypeOf((function*(){})());return [p.throw.call(null,1)]})`, `T(()=>{var p=Object.getPrototypeOf((function*(){})());return [p.next.length,p.return.length,p.throw.length]})`,
  `T(()=>{var g=(function*(){yield 1})();return [g[Symbol.iterator]()===g,Object.prototype.toString.call(g),Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(g)))===${IP}]})`,
  `T(()=>{var g=function*(){};return [Object.getPrototypeOf(g)===Object.getPrototypeOf(function*(){}),Object.getPrototypeOf(g).constructor.name,Object.prototype.toString.call(g)]})`,
  `T(()=>{var g=function*(){};return Reflect.ownKeys(g).map(String).join()+'|'+Reflect.ownKeys(g.prototype).length})`,
  `T(()=>{var g=function*(){};return [g.prototype===Object.getPrototypeOf(g()),Object.getPrototypeOf(g.prototype)===Object.getPrototypeOf(function*(){}).prototype]})`,
  `T(()=>{var g=function*(){};g.prototype=null;return Object.getPrototypeOf(g())===Object.getPrototypeOf(function*(){}).prototype})`,
  `T(()=>{var g=function*(){};g.prototype=1;return Object.getPrototypeOf(g())===Object.getPrototypeOf(function*(){}).prototype})`,
  `T(()=>{var g=function*(){};g.prototype={next(){return {done:true}}};return [Object.prototype.toString.call(g()),[...g()]]})`,
  `T(()=>{var g=function*(){yield 1};g.prototype=Object.create(${IP});return [[...g()],Object.prototype.toString.call(g())]})`,
);
// Os métodos de IteratorPrototype com this inválido ou com iterador sem next.
for (const t of ["undefined", "null", "1", "'s'", "{}", "{next:1}", "{next(){return {done:true}}}", "[1].values()", "(function*(){yield 1})()"]) {
  for (const m of ["map(x=>x)", "filter(x=>1)", "take(1)", "drop(1)", "flatMap(x=>[x])", "toArray()", "forEach(x=>x)", "some(x=>x)", "every(x=>x)", "find(x=>x)", "reduce((a,b)=>a)"]) {
    add(`T(()=>Iterator.prototype.${m.split("(")[0]}.call(${t},${m.slice(m.indexOf("(") + 1, -1)}))`);
  }
}
for (const t of ["undefined", "null", "1", "'s'", "{}", "[1]", "new Set([1])", "{next(){return {done:true}}}", "{[Symbol.iterator](){return {next(){return {done:true}}}}}", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}", "{next:1,[Symbol.iterator]:undefined}"]) {
  add(`T(()=>Iterator.from(${t}))`, `T(()=>S([...Iterator.from(${t})]))`, `T(()=>Object.prototype.toString.call(Iterator.from(${t})))`);
}

// ---- Execução.
const bases = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  if (!/iter|gener|collection|array|string|typed|regexp|map|set|spread|destruct|buffer|arguments/i.test(file) || file === "builtin_iteration_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { bases.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const baseText = bases.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let dup = 0;
const jobs = [];
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const needsMk = /MK\(/.test(expr);
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + (needsMk ? MK : "") + `globalThis.R = ${expr}` });
}
const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => { child.kill("SIGKILL"); reject(new Error("tempo esgotado")); }, 20000);
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", status => { clearTimeout(timer); status === 0 ? resolve(decodeResult(out)) : reject(new Error(err || "filho falhou")); });
    child.stdin.end(source);
  });
}
async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const i = next++;
      try {
        // Processo fresco por programa: a reificação lazy das tabelas estáticas depende da ordem de acesso.
        results[i] = await runChild(jobs[i].source);
      } catch (e) {
        results[i] = null;
        process.stderr.write("erro de programa: " + JSON.stringify(jobs[i].expr).slice(0, 160) + " " + e + "\n");
      }
    }
  }
  await Promise.all(Array.from({ length: 12 }, worker));
  let kept = 0;
  let dropped = 0;
  const out = [];
  const outSeen = new Set();
  for (let i = 0; i < jobs.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || outSeen.has(jobs[i].source)) { dropped++; continue; }
    outSeen.add(jobs[i].source);
    kept++;
    out.push(JSON.stringify(jobs[i].source) + "\t" + JSON.stringify(results[i]));
  }
  process.stdout.write(emitFactoredLines("builtin_iteration", out));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
