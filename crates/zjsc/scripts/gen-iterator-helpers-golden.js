// Gera tests/golden/iterator_helpers_bun.tsv: Iterator helpers síncronos em grade, medido no bun 1.4.2.
// Cobre Iterator.prototype.map/filter/flatMap/take/drop/reduce/toArray/forEach/some/every/find, Iterator.from,
// Iterator.concat, %WrapForValidIteratorPrototype% e %IteratorHelperPrototype%: receptores e argumentos inválidos
// (mensagens exatas), `return()` propagado ao subjacente, `next` lido uma vez, subjacente sem next/return, helpers
// encadeados (pares e trincas), reentrância, exaustão, throw no callback fechando o iterador, contadores de take/drop
// com NaN/Infinity/negativos/BigInt, flatMap rejeitando strings, Symbol.toStringTag e descritores.
// AsyncIterator não existe no bun 1.4.2 (`typeof AsyncIterator` é um ReferenceError), então não há parte assíncrona.
// Programas cuja fonte já aparece nos goldens iterator_bun, iterator_protocol_bun, collection_async_bun e esnext_bun
// são descartados. Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-iterator-helpers-golden.js > tests/golden/iterator_helpers_bun.tsv
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
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  // Iterador subjacente instrumentado: o getter de next e as chamadas vão para o log.
  'function It(log,vals,o){o=o||{};var i=0;var it={__proto__:o.bare?null:Iterator.prototype};' +
  'Object.defineProperty(it,"next",{get(){log.push("get next");return function(){log.push("next");if(o.nextThrow!==undefined&&i>=o.nextThrow)throw new RangeError("nt");' +
  'if(i>=vals.length)return {done:true};return {done:false,value:vals[i++]}}},configurable:true});' +
  'if(!o.noReturn)it.return=function(){log.push("return");if(o.retThrow)throw new EvalError("rt");return "retVal" in o?o.retVal:{}};return it}\n' +
  'function M(n,it){return Iterator.prototype[n].apply(it,Array.prototype.slice.call(arguments,2))}\n' +
  'function Dr(h,max){var o=[];for(var i=0;i<(max||12);i++){var r=h.next();o.push(r.done?"done":S(r.value));if(r.done)break}return o}\n' +
  'function Rn(log,f){var r;try{r=f()}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return S([r,log])}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const lazy = new Set(["map", "filter", "flatMap", "take", "drop"]);
const eager = ["reduce", "toArray", "forEach", "some", "every", "find"];
const cbMethods = ["map", "filter", "flatMap", "reduce", "forEach", "some", "every", "find"];
const allMethods = ["map", "filter", "flatMap", "take", "drop", "reduce", "toArray", "forEach", "some", "every", "find"];
// Chama o método e, se for preguiçoso, drena o helper.
const call = (name, recv, args) => (lazy.has(name) ? `Dr(M("${name}",${recv}${args ? "," + args : ""}))` : `M("${name}",${recv}${args ? "," + args : ""})`);
const validArg = n => (n === "take" || n === "drop" ? "2" : n === "toArray" ? "" : n === "flatMap" ? "x=>[x]" : "x=>x");

// ---- 1. Receptores inválidos em cada método, com argumento válido.
const receivers = ["undefined", "null", "1", "'abc'", "true", "Symbol()", "1n", "NaN", "Object.create(null)", "{}", "[]", "function(){}", "[1,2].values", "new Proxy({},{})"];
for (const n of allMethods) for (const r of receivers) add(`T(()=>M("${n}",${r}${validArg(n) ? "," + validArg(n) : ""}))`);
for (const n of allMethods) for (const r of ["{next(){return {done:true}}}", "{next:1}", "{next:undefined}", "[][Symbol.iterator]()", "new Set([1]).values()", "'ab'[Symbol.iterator]()"]) {
  add(`Rn([],()=>${call(n, r, validArg(n))})`);
}

// ---- 2. Argumentos inválidos: o argumento é validado antes de ler next, e o subjacente é fechado ou não.
const badCallbacks = ["undefined", "null", "1", "'x'", "{}", "[]", "Symbol()", "true", "class{}", "new Proxy({},{})", "{call(){}}", "1n", "NaN"];
for (const n of cbMethods) for (const a of badCallbacks) {
  add(`(()=>{var log=[];return Rn(log,()=>M("${n}",It(log,[1,2]),${a}))})()`);
  add(`(()=>{var log=[];return Rn(log,()=>M("${n}",It(log,[1,2],{bare:true,noReturn:true}),${a},${n === "reduce" ? "0" : "1"}))})()`);
}
const counts = ["NaN", "Infinity", "-Infinity", "-1", "-0", "0", "0.5", "1.9", "2", "3", "5", "'2'", "'x'", "undefined", "null", "true", "false", "{valueOf(){return 2}}", "{valueOf(){throw new EvalError('v')}}",
  "1n", "Symbol()", "2**53", "-0.5", "1e308", "''", "[]", "[3]", "{}", "4294967296", "-1e-7", "' 2 '", "'0x2'", "'Infinity'", "1e-320", "2**31", "-(2**31)", "new Number(2)", "{toString(){return '1'}}"];
for (const n of ["take", "drop"]) for (const c of counts) {
  add(`(()=>{var log=[];return Rn(log,()=>Dr(M("${n}",It(log,[1,2,3,4,5]),${c})))})()`);
  add(`(()=>{var log=[];return Rn(log,()=>M("${n}",It(log,[1,2,3],{noReturn:true}),${c}).toArray())})()`);
  add(`(()=>{var log=[];return Rn(log,()=>{var h=M("${n}",It(log,[1,2,3,4,5]),${c});h.return();return log.concat(Dr(h))})})()`);
  add(`Rn([],()=>M("${n}",undefined,${c}))`);
}
add(`T(()=>M("take",[1].values()))`, `T(()=>M("drop",[1].values()))`, `T(()=>[1,2,3].values().take(1,2,3).toArray())`, `T(()=>[1,2,3].values().drop().toArray())`);

// ---- 3. Comportamento dos callbacks em cada método, com subjacente comum e sem return.
const cbs = [
  "x=>x", "(x,i)=>i", "x=>{throw new RangeError('cb')}", "(x,i)=>{if(i==2)throw new TypeError('cb2');return x}", "function(){return this}", "function(){'use strict';return this}",
  "()=>undefined", "()=>null", "(...a)=>a.length", "x=>x*2", "x=>x%2", "x=>({})", "x=>'s'", "x=>[x,x]", "x=>[]", "x=>new Set([x])", "x=>new String('ab')", "x=>'ab'",
  "x=>({next(){return {done:true}}})", "x=>({[Symbol.iterator]:1})", "x=>({[Symbol.iterator](){return {next(){return {done:true}}}}})", "x=>1", "x=>Symbol()", "x=>0n",
  "x=>({[Symbol.iterator](){return 1}})", "x=>({[Symbol.iterator](){return {}}})", "x=>({[Symbol.iterator](){return [x].values()}})", "x=>function*(){yield x}()", "x=>[x].values()",
  "x=>x>1?[x]:'str'", "(x,i)=>i<1?[x]:[]", "x=>({next(){return {done:false,value:x}}})",
];
for (const n of cbMethods.filter(n => n !== "reduce")) for (const cb of cbs) {
  add(`(()=>{var log=[];return Rn(log,()=>${call(n, "It(log,[1,2,3])", cb)})})()`);
  add(`(()=>{var log=[];return Rn(log,()=>${call(n, "It(log,[1,2,3],{noReturn:true})", cb)})})()`);
}
const reducers = ["(a,x)=>a+x", "(a,x,i)=>a+i", "()=>{throw new RangeError('r')}", "a=>a", "(a,x)=>[a,x]", "(a,x,i,z)=>z", "function(){return this}", "(a,x)=>x", "(a,x,i)=>{if(i==2)throw 5;return a}"];
const inits = ["", ",undefined", ",0", ",'z'", ",null"];
for (const r of reducers) for (const ini of inits) for (const items of ["[]", "[1]", "[1,2,3]"]) {
  add(`(()=>{var log=[];return Rn(log,()=>M("reduce",It(log,${items}),${r}${ini}))})()`);
}
add(`T(()=>[].values().reduce((a,b)=>a))`, `T(()=>[].values().reduce((a,b)=>a,undefined))`, `T(()=>[1].values().reduce((a,b)=>a))`, `T(()=>[].values().toArray())`,
  `T(()=>[,,1].values().toArray())`, `T(()=>[1,2,3].values().forEach(x=>x))`, `T(()=>[].values().some(x=>1))`, `T(()=>[].values().every(x=>0))`, `T(()=>[].values().find(x=>1))`);

// ---- 4. next lido uma vez, protocolo do resultado de next.
add(
  `(()=>{var log=[];var h=It(log,[1,2,3]).map(x=>x);Dr(h);return S(log)})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).filter(x=>1);Dr(h);return S(log)})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).take(2);Dr(h);return S(log)})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).drop(1);Dr(h);return S(log)})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).flatMap(x=>[x]);Dr(h);return S(log)})()`,
  `(()=>{var log=[];It(log,[1,2,3]).reduce((a,b)=>a+b);return S(log)})()`,
  `(()=>{var log=[];It(log,[1,2,3]).toArray();return S(log)})()`,
  `(()=>{var log=[];It(log,[1,2,3]).forEach(x=>x);return S(log)})()`,
  `(()=>{var log=[];It(log,[1,2,3]).some(x=>x==2);return S(log)})()`,
  `(()=>{var log=[];It(log,[1,2,3]).every(x=>x==2);return S(log)})()`,
  `(()=>{var log=[];It(log,[1,2,3]).find(x=>x==2);return S(log)})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).map(x=>x);return S(log)})()`,
  `(()=>{var log=[];var o=It(log,[1,2,3]);var h=o.map(x=>x);Object.defineProperty(o,"next",{value(){log.push("swapped");return {done:true}}});return S([Dr(h),log])})()`,
  `(()=>{var log=[];var o=It(log,[1,2,3]);var h=o.take(5);delete o.next;return Rn(log,()=>Dr(h))})()`,
  `(()=>{var log=[];var o=It(log,[1,2,3]);o.return=undefined;var h=o.take(1);return Rn(log,()=>Dr(h))})()`,
);
const nextResults = [
  "1", "null", "undefined", "'str'", "true", "{}", "{done:1}", "{done:0,value:1}", "{done:'',value:1}", "{done:'x',value:1}", "{done:null,value:1}", "{done:true,value:1}", "{value:1}", "[]", "function(){}",
  "{get done(){log.push('done');return false},get value(){log.push('value');return 7}}", "{get done(){throw new EvalError('d')}}", "{done:false,get value(){throw new EvalError('v')}}", "new Proxy({done:false,value:3},{})",
  "{done:NaN,value:1}", "{done:{},value:1}", "{done:Symbol(),value:1}", "{done:0n,value:1}",
];
for (const r of nextResults) for (const n of ["map", "filter", "take", "drop", "flatMap"]) {
  add(`(()=>{var log=[];var u={__proto__:Iterator.prototype,next(){log.push('next');return ${r}},return(){log.push('return');return {}}};return Rn(log,()=>S(Dr(M("${n}",u,${n === "take" || n === "drop" ? "3" : n === "flatMap" ? "x=>[x]" : "x=>1"}),3)))})()`);
}
for (const r of nextResults) for (const n of ["toArray", "some", "find", "reduce"]) {
  add(`(()=>{var log=[];var c=0;var u={__proto__:Iterator.prototype,next(){log.push('next');return c++<2?${r}:{done:true}},return(){log.push('return');return {}}};return Rn(log,()=>${n === "toArray" ? "u.toArray()" : n === "reduce" ? "u.reduce((a,b)=>a,0)" : `u.${n}(x=>0)`})})()`);
}
// next que não é função, ausente ou getter lançando, em cada método.
const nexts = ["undefined", "null", "1", "'f'", "{}", "Symbol()", "class{}"];
for (const n of allMethods) for (const nx of nexts) {
  add(`(()=>{var log=[];var u={__proto__:Iterator.prototype,next:${nx},return(){log.push('return');return {}}};return Rn(log,()=>${call(n, "u", validArg(n))})})()`);
}
for (const n of allMethods) {
  add(`(()=>{var log=[];var u={__proto__:Iterator.prototype,get next(){throw new EvalError('gn')},return(){log.push('return');return {}}};return Rn(log,()=>${call(n, "u", validArg(n))})})()`);
  add(`(()=>{var log=[];var u={__proto__:Iterator.prototype,return(){log.push('return');return {}}};return Rn(log,()=>${call(n, "u", validArg(n))})})()`);
  add(`(()=>{var log=[];var u={__proto__:null,next(){return {done:true}}};return Rn(log,()=>${call(n, "u", validArg(n))})})()`);
}

// ---- 5. return() propagado: helper x estágio x comportamento do return do subjacente.
const helpers = {
  map: "x=>x", filter: "x=>1", take2: null, drop1: null, flatMapArr: null, flatMapIt: null,
};
const mkHelper = (kind, u) => ({
  map: `M("map",${u},x=>x)`, filter: `M("filter",${u},x=>1)`, take: `M("take",${u},2)`, drop: `M("drop",${u},1)`,
  flatArr: `M("flatMap",${u},x=>[x,x])`, flatIt: `M("flatMap",${u},x=>It(log,[9,8]))`,
}[kind]);
const stages = {
  fresh: "h.return()", started: "h.next();h.return()", exhausted: "Dr(h);h.return()", twice: "h.next();h.return();h.return()",
  thenNext: "h.next();h.return();h.next()", retNext: "h.return();h.next()", retThenRet: "h.return();h.return();h.next()", mid: "h.next();h.next();h.return()",
};
const behaviors = {
  normal: "{}", noReturn: "{noReturn:true}", throws: "{retThrow:true}", prim: "{retVal:1}", undef: "{retVal:undefined}", nul: "{retVal:null}", str: "{retVal:'s'}",
  bare: "{bare:true}", bareNoReturn: "{bare:true,noReturn:true}",
};
for (const kind of ["map", "filter", "take", "drop", "flatArr", "flatIt"]) for (const [sn, st] of Object.entries(stages)) for (const [bn, bv] of Object.entries(behaviors)) {
  add(`(()=>{var log=[];var h=${mkHelper(kind, `It(log,[1,2,3,4],${bv})`)};return Rn(log,()=>{var r=(()=>{${st.replace(/(^|;)([^;]*)$/, "$1return $2")}})();return S([r,log])})})()`);
}
// return do helper: valores de retorno e propriedades.
for (const kind of ["map", "filter", "take", "drop", "flatArr"]) {
  add(`(()=>{var log=[];var h=${mkHelper(kind, "It(log,[1,2,3])")};var r=h.return();return S([r,Object.getPrototypeOf(r)===Object.prototype,Reflect.ownKeys(r),log])})()`);
  add(`(()=>{var log=[];var h=${mkHelper(kind, "It(log,[1,2,3])")};h.next();var r=h.next();return S([r,Reflect.ownKeys(r),log])})()`);
  add(`(()=>{var log=[];var h=${mkHelper(kind, "It(log,[1,2,3])")};return S([h.return(5),h.next(),h.return(),log])})()`);
  add(`(()=>{var log=[];var h=${mkHelper(kind, "It(log,[1,2,3])")};return S([h.next(1),h.next(2),log])})()`);
}

// ---- 6. Helpers encadeados: pares e trincas de operações preguiçosas.
const ops = ["map(x=>x+1)", "filter(x=>x%2)", "take(3)", "drop(1)", "flatMap(x=>[x,x])", "take(0)", "drop(100)", "take(100)"];
for (const a of ops) for (const b of ops) {
  add(`(()=>{var log=[];return Rn(log,()=>Dr(It(log,[1,2,3,4,5]).${a}.${b},20))})()`);
  add(`(()=>{var log=[];return Rn(log,()=>Dr(It(log,[1,2],{noReturn:true}).${a}.${b},20))})()`);
}
for (const a of ops) for (const b of ops) for (const c of ops) {
  add(`(()=>{var log=[];return Rn(log,()=>Dr(It(log,[1,2,3,4,5]).${a}.${b}.${c},20))})()`);
}
for (const a of ops) for (const e of ["toArray()", "reduce((a,b)=>a+b,0)", "some(x=>x>2)", "every(x=>x<3)", "find(x=>x==3)", "forEach(x=>{})"]) {
  add(`(()=>{var log=[];return Rn(log,()=>S(It(log,[1,2,3,4,5]).${a}.${e}))})()`);
}
// Encadear e fechar no meio: o return atravessa a cadeia inteira.
for (const a of ops.slice(0, 5)) for (const b of ops.slice(0, 5)) {
  add(`(()=>{var log=[];var h=It(log,[1,2,3,4]).${a}.${b};h.next();h.return();return Rn(log,()=>S([Dr(h),log]))})()`);
}

// ---- 7. Reentrância e exaustão.
add(
  `(()=>{var h;h=[1,2,3].values().map(x=>h.next());return T(()=>h.next())})()`,
  `(()=>{var h;h=[1,2,3].values().filter(x=>h.next());return T(()=>h.next())})()`,
  `(()=>{var h;h=[1,2,3].values().flatMap(x=>h.next());return T(()=>h.next())})()`,
  `(()=>{var h;h=[1,2,3].values().map(x=>{h.return();return x});return T(()=>[h.next(),h.next()])})()`,
  `(()=>{var h;h=[1,2,3].values().map(x=>{try{h.next()}catch(e){return e.name+":"+e.message}return x});return T(()=>[h.next().value,h.next().value])})()`,
  `(()=>{var h,log=[];h=It(log,[1,2,3]).map(x=>{try{h.return()}catch(e){return e.name+":"+e.message}return x});return Rn(log,()=>S([h.next().value,h.next().value,log]))})()`,
  `(()=>{var h,log=[];h=It(log,[1,2,3]).take(2);var r=h.next();return Rn(log,()=>S([r,h.next(),h.next(),h.next(),log]))})()`,
  `(()=>{var u;var log=[];u=It(log,[1,2,3]);var o=Object.getPrototypeOf(u);var h=u.map(x=>x);u.next=function(){log.push('reenter');return {done:true}};return Rn(log,()=>S(Dr(h)))})()`,
  `(()=>{var h;var u={__proto__:Iterator.prototype,next(){return h.next()}};h=u.map(x=>x);return T(()=>h.next())})()`,
  `(()=>{var h;var u={__proto__:Iterator.prototype,next(){return {done:false,value:1}},return(){h.next();return {}}};h=u.map(x=>x);h.next();return T(()=>h.return())})()`,
  `(()=>{var h;var u={__proto__:Iterator.prototype,next(){return {done:false,value:1}},return(){return h.return()}};h=u.map(x=>x);h.next();return T(()=>h.return())})()`,
  `(()=>{var h;var u={__proto__:Iterator.prototype,next(){return {done:false,value:1}},return(){return {}}};h=u.take(1);h.next();return T(()=>[h.next(),h.return()])})()`,
  `(()=>{var h;h=[1,2].values().flatMap(x=>({[Symbol.iterator](){return {next(){return h.next()}}}}));return T(()=>h.next())})()`,
  `(()=>{var log=[];var h=[1,2,3].values().flatMap(x=>({[Symbol.iterator](){log.push('iter');return {next(){log.push('inner');return {done:true}}}}}));return T(()=>[Dr(h),log])})()`,
  `(()=>{var log=[];var inner={__proto__:Iterator.prototype,next(){log.push('in');return {done:false,value:1}},return(){log.push('inret');return {}}};var h=It(log,[1,2]).flatMap(x=>inner);h.next();h.return();return S(log)})()`,
  `(()=>{var log=[];var inner={__proto__:Iterator.prototype,next(){log.push('in');return {done:false,value:1}},return(){log.push('inret');throw new EvalError('ir')}};var h=It(log,[1,2]).flatMap(x=>inner);h.next();return Rn(log,()=>h.return())})()`,
  `(()=>{var log=[];var inner={__proto__:Iterator.prototype,next(){log.push('in');throw new EvalError('in')},return(){log.push('inret');return {}}};var h=It(log,[1,2]).flatMap(x=>inner);return Rn(log,()=>h.next())})()`,
  `(()=>{var log=[];var h=It(log,[1,2]).flatMap(x=>{throw new EvalError('cb')});return Rn(log,()=>h.next())})()`,
  `(()=>{var log=[];var h=It(log,[1,2],{retThrow:true}).map(x=>{throw new RangeError('cb')});return Rn(log,()=>h.next())})()`,
  `(()=>{var log=[];var h=It(log,[1,2],{retThrow:true}).map(x=>x);h.next();return Rn(log,()=>h.return())})()`,
  `(()=>{var log=[];var h=It(log,[1,2],{nextThrow:1}).map(x=>x);return Rn(log,()=>S([h.next(),h.next(),h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2],{nextThrow:0}).take(1);return Rn(log,()=>S([h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2],{nextThrow:1}).drop(1);return Rn(log,()=>S([h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).take(0);return S([h.next(),log,h.next(),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).take(1);return S([h.next(),log,h.next(),log,h.next(),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).take(3);return S([Dr(h),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).take(5);return S([Dr(h),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).drop(5);return S([Dr(h),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).drop(0);return S([Dr(h),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3]).drop(Infinity);return S([Dr(h),log])})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3],{noReturn:true}).take(1);return Rn(log,()=>S([h.next(),h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3],{retThrow:true}).take(1);return Rn(log,()=>S([h.next(),h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3],{retVal:1}).take(1);return Rn(log,()=>S([h.next(),h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3],{retVal:1}).take(0);return Rn(log,()=>S([h.next(),log]))})()`,
  `(()=>{var log=[];var h=It(log,[1,2,3],{retThrow:true}).take(0);return Rn(log,()=>S([h.next(),log]))})()`,
  `(()=>{var log=[];var r=It(log,[1,2,3],{retThrow:true}).some(x=>true);return S([r,log])})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{retThrow:true}).some(x=>{throw new RangeError('c')}))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{retVal:5}).every(x=>false))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{retVal:5}).find(x=>true))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{retVal:5}).forEach(x=>{throw 1}))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{retVal:5}).reduce((a,b)=>{throw 1},0))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{retThrow:true}).reduce((a,b)=>{throw 1},0))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{noReturn:true}).forEach(x=>{throw 1}))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{nextThrow:2}).toArray())})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{nextThrow:2}).forEach(x=>x))})()`,
  `(()=>{var log=[];return Rn(log,()=>It(log,[1,2,3],{nextThrow:2}).some(x=>false))})()`,
);

// ---- 8. Iterator.from e %WrapForValidIteratorPrototype%.
const fromArgs = [
  "undefined", "null", "1", "1n", "true", "Symbol()", "NaN", "{}", "[]", "[1,2]", "'ab'", "''", "new String('ab')", "new Number(1)", "function(){}", "class{}", "[].values()", "new Set([1]).values()", "new Map([[1,2]])",
  "(function*(){yield 1})()", "{next(){return {done:true}}}", "{next:1}", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator](){return {next(){return {done:true}}}}}",
  "{[Symbol.iterator]:undefined,next(){return {done:true}}}", "{[Symbol.iterator]:null,next(){return {done:true}}}", "{[Symbol.iterator]:null}", "{get [Symbol.iterator](){throw new EvalError('g')}}",
  "{[Symbol.iterator](){throw new EvalError('c')}}", "Object.create(Iterator.prototype)", "{__proto__:Iterator.prototype,next(){return {done:true}}}", "new (class extends Iterator{})", "new Proxy([].values(),{})",
  "Object.create(null)", "{next(){return {done:true}},return(){return {}}}", "{get next(){throw new EvalError('n')}}", "new Proxy({},{})", "Symbol.iterator", "{[Symbol.iterator](){return [1,2].values()}}",
  "{[Symbol.iterator]:function*(){yield 1}}", "{[Symbol.iterator]:[][Symbol.iterator]}", "[1,2][Symbol.iterator]", "arguments", "new Uint8Array(2)", "new Int8Array(0)",
];
for (const a of fromArgs) {
  add(`Rn([],()=>{var w=Iterator.from(${a});return S([typeof w,w===null?0:Object.getPrototypeOf(w)===Iterator.prototype,Reflect.ownKeys(w)])})`);
  add(`Rn([],()=>{var w=Iterator.from(${a});return Dr(w,6)})`);
  add(`Rn([],()=>{var o=${a};var w=Iterator.from(o);return S([w===o,w instanceof Iterator,Object.prototype.toString.call(w),w[Symbol.toStringTag],typeof w.next,typeof w.return,w[Symbol.iterator]()===w])})`);
  add(`Rn([],()=>{var w=Iterator.from(${a});return S([w.return(),w.return(5),w.next()])})`);
  add(`Rn([],()=>{var w=Iterator.from(${a});return w.map(x=>x).toArray()})`);
}
add(
  `T(()=>Iterator.from())`, `T(()=>Iterator.from.length)`, `T(()=>Iterator.from.name)`, `T(()=>new Iterator.from({}))`, `T(()=>Iterator.from.call(undefined,[1]).toArray())`,
  `T(()=>Iterator.from.call(1,[1]).toArray())`, `T(()=>D(Iterator,'from'))`, `T(()=>D(Iterator,'concat'))`, `T(()=>D(Iterator,'zip'))`,
  `(()=>{var log=[];var o={next(){log.push('next');return {done:true}},return(){log.push('return');return {}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return S([Reflect.ownKeys(P),Object.getPrototypeOf(P)===Iterator.prototype,D(P,'next'),D(P,'return'),P[Symbol.toStringTag],log])})()`,
  `(()=>{var log=[];var o={next(){log.push('next');return {done:true}}};var w=Iterator.from(o);return Rn(log,()=>S([w.return(),log]))})()`,
  `(()=>{var log=[];var o={get next(){log.push('get');return function(){log.push('call');return {done:false,value:1}}}};var w=Iterator.from(o);w.next();w.next();return S(log)})()`,
  `(()=>{var log=[];var o={next(){log.push('call:'+arguments.length+':'+(this===o));return {done:false,value:1}}};var w=Iterator.from(o);w.next(1,2);return S(log)})()`,
  `(()=>{var log=[];var o={next(){return 5},return(){log.push('ret:'+arguments.length+':'+(this===o));return 7}};var w=Iterator.from(o);return Rn(log,()=>S([w.next(),w.return(1,2),log]))})()`,
  `(()=>{var log=[];var o={next(){return {done:true}},return(){return 7}};var w=Iterator.from(o);return Rn(log,()=>S(w.return()))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return Rn([],()=>P.next.call({}))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return Rn([],()=>P.return.call({}))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return Rn([],()=>P.next.call(undefined))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return Rn([],()=>P.return.call(1))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return Rn([],()=>P.next.call(Iterator.from({next(){return {done:true}}})))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return Rn([],()=>P.next.call(w))})()`,
  `(()=>{var o={next(){return {done:true}}};var w=Iterator.from(o);var P=Object.getPrototypeOf(w);return S([P.next.name,P.next.length,P.return.name,P.return.length,Object.getPrototypeOf(P.next)===Function.prototype])})()`,
  `(()=>{var a=Iterator.from({next(){return {done:true}}});var b=Iterator.from({next(){return {done:true}},return(){}});return S([Object.getPrototypeOf(a)===Object.getPrototypeOf(b),a.next===b.next])})()`,
  `(()=>{var P=Object.getPrototypeOf(Iterator.from({next(){}}));return Rn([],()=>{P.x=1;return S([Object.isExtensible(P),Object.isFrozen(P),Object.getPrototypeOf(P)===Iterator.prototype])})})()`,
  `(()=>{var log=[];var o={next(){return {done:false,value:1}}};var w=Iterator.from(o);var h=w.take(1);Dr(h);return Rn(log,()=>S(log))})()`,
  `(()=>{var log=[];var o={next(){log.push('n');return {done:false,value:1}},return(){log.push('r');return {}}};var h=Iterator.from(o).take(1);Dr(h);return S(log)})()`,
  `(()=>{var log=[];var o={next(){log.push('n');return {done:false,value:1}},return(){log.push('r');return {}}};var h=Iterator.from(o).map(x=>x);h.next();h.return();return S(log)})()`,
  `(()=>{var o=[1,2].values();return S([Iterator.from(o)===o,Iterator.from(Iterator.from('ab')).next()])})()`,
  `(()=>{class A extends Iterator{next(){return {done:true}}};var a=new A();return S([Iterator.from(a)===a])})()`,
  `(()=>{var o={[Symbol.iterator](){return {next(){return {done:false,value:1}}}}};var w=Iterator.from(o);return S([Object.getPrototypeOf(w)===Iterator.prototype,w.next(),typeof w.return])})()`,
  `(()=>{var o={[Symbol.iterator](){return {__proto__:Iterator.prototype,next(){return {done:true}}}}};var it=o[Symbol.iterator]();var w=Iterator.from(o);return S([w===it,Object.getPrototypeOf(w)===Iterator.prototype,w.next()])})()`,
  `(()=>{var log=[];var o={get [Symbol.iterator](){log.push('gi');return function(){log.push('ci');return {next(){return {done:true}}}}},get next(){log.push('gn');return function(){return {done:true}}}};Iterator.from(o);return S(log)})()`,
  `(()=>{var log=[];var o={[Symbol.iterator]:undefined,get next(){log.push('gn');return function(){return {done:true}}}};var w=Iterator.from(o);w.next();w.next();return S(log)})()`,
  `T(()=>Iterator.from('ab').toArray())`, `T(()=>Iterator.from(new String('ab')).toArray())`, `T(()=>Iterator.from({length:2,0:1,1:2}))`,
  `T(()=>Iterator.from([1,2,3]).take(2).toArray())`, `T(()=>Iterator.from(new Set([1,2])).map(x=>x*2).toArray())`, `T(()=>Iterator.from(new Map([[1,2]])).toArray())`,
);

// ---- 9. Iterator.concat.
const concatArgs = [
  "", "[]", "[1],[2]", "[1],[2,3],[]", "'ab'", "'ab',[1]", "[1],'ab'", "1", "undefined", "null", "[1],undefined", "[1],1", "{}", "[1],{}", "new Set([1,2]),[3]", "[1].values()", "[1].values(),[2].values()",
  "{[Symbol.iterator](){return {next(){return {done:true}}}}}", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator]:null}", "{[Symbol.iterator]:undefined}",
  "{get [Symbol.iterator](){throw new EvalError('g')}}", "{[Symbol.iterator](){throw new EvalError('c')}}", "(function*(){yield 1;yield 2})()", "(function*(){yield 1;yield 2})(),[3]", "new String('ab')", "new Map([[1,2]])",
  "[1],[2],[3],[4],[5]", "Symbol()", "true", "function(){}", "new Proxy([1],{})", "new Uint8Array(2)", "{next(){return {done:true}}}", "Iterator.from([1,2])",
];
for (const a of concatArgs) {
  add(`(()=>{var log=[];return Rn(log,()=>S([Dr(Iterator.concat(${a}),10),log]))})()`);
  add(`Rn([],()=>{var h=Iterator.concat(${a});return S([typeof h,Object.getPrototypeOf(h)===Iterator.prototype.map.call([].values(),x=>x).__proto__,h[Symbol.toStringTag],typeof h.return,h[Symbol.iterator]()===h])})`);
  add(`Rn([],()=>{var h=Iterator.concat(${a});return S([h.return(),h.next(),h.return()])})`);
  add(`Rn([],()=>Iterator.concat(${a}).map(x=>x).take(2).toArray())`);
}
add(
  `T(()=>Iterator.concat.length)`, `T(()=>Iterator.concat.name)`, `T(()=>new Iterator.concat([]))`, `T(()=>Iterator.concat.call(undefined,[1]).toArray())`,
  `(()=>{var log=[];var mk=n=>({[Symbol.iterator](){log.push('open'+n);return {next(){log.push('next'+n);return {done:true}},return(){log.push('ret'+n);return {}}}}});var h=Iterator.concat(mk(1),mk(2));return S([log.slice(),Dr(h),log])})()`,
  `(()=>{var log=[];var mk=n=>({[Symbol.iterator](){log.push('open'+n);return {i:0,next(){log.push('next'+n);return this.i++<2?{done:false,value:n}:{done:true}},return(){log.push('ret'+n);return {}}}}});var h=Iterator.concat(mk(1),mk(2));h.next();h.return();return S([log,h.next()])})()`,
  `(()=>{var log=[];var mk=n=>({[Symbol.iterator](){log.push('open'+n);return {next(){log.push('next'+n);return {done:false,value:n}},return(){log.push('ret'+n);throw new EvalError('r')}}}});var h=Iterator.concat(mk(1),mk(2));h.next();return Rn(log,()=>S([h.return(),log]))})()`,
  `(()=>{var log=[];var mk=n=>({[Symbol.iterator](){log.push('open'+n);return {next(){log.push('next'+n);return 5}}}});var h=Iterator.concat(mk(1));return Rn(log,()=>S([h.next(),log]))})()`,
  `(()=>{var log=[];var h;h=Iterator.concat({[Symbol.iterator](){return {next(){return h.next()}}}});return Rn(log,()=>h.next())})()`,
  `(()=>{var log=[];var o={get [Symbol.iterator](){log.push('get');return function(){log.push('call');return {get next(){log.push('gn');return function(){return {done:true}}}}}}};var h=Iterator.concat(o);log.push('made');h.next();h.next();return S(log)})()`,
  `(()=>{var log=[];var o={[Symbol.iterator](){log.push('call');return {next(){return {done:true}}}}};var h=Iterator.concat(o,o);h.next();return S(log)})()`,
  `(()=>{var a=[1,2];var h=Iterator.concat(a);a.push(3);return S(Dr(h))})()`,
  `(()=>{var a=[1,2];var h=Iterator.concat(a,a);return S(Dr(h))})()`,
);

// ---- 10. Descritores, toStringTag, construtor e protótipos.
const protoHelperExpr = "Object.getPrototypeOf([].values().map(x=>x))";
for (const n of allMethods) {
  add(`T(()=>D(Iterator.prototype,'${n}'))`, `T(()=>[Iterator.prototype.${n}.name,Iterator.prototype.${n}.length,typeof Iterator.prototype.${n}.prototype,Object.getPrototypeOf(Iterator.prototype.${n})===Function.prototype])`,
    `T(()=>new Iterator.prototype.${n}())`, `T(()=>Reflect.ownKeys(Iterator.prototype.${n}))`, `T(()=>Iterator.prototype.${n}.call())`,
    `T(()=>Iterator.prototype.${n}.toString().replace(/\\s+/g,' '))`);
}
for (const k of ["next", "return", "constructor", "Symbol.toStringTag", "Symbol.iterator"]) {
  const kk = k.startsWith("Symbol.") ? k : `'${k}'`;
  add(`T(()=>D(${protoHelperExpr},${kk}))`, `T(()=>D(Iterator.prototype,${kk}))`, `T(()=>D(Object.getPrototypeOf(Iterator.from({next(){}})),${kk}))`);
}
add(
  `T(()=>Reflect.ownKeys(${protoHelperExpr}))`, `T(()=>Reflect.ownKeys(Iterator.prototype))`, `T(()=>Reflect.ownKeys(Iterator))`, `T(()=>Reflect.ownKeys(Object.getPrototypeOf(Iterator.from({next(){}}))))`,
  `T(()=>Object.getPrototypeOf(${protoHelperExpr})===Iterator.prototype)`, `T(()=>Object.getPrototypeOf(Iterator.prototype)===Object.prototype)`,
  `T(()=>[typeof Iterator,Iterator.name,Iterator.length,D(globalThis,'Iterator')])`, `T(()=>Iterator())`, `T(()=>new Iterator())`, `T(()=>Reflect.construct(Iterator,[],Object))`,
  `T(()=>{class A extends Iterator{};return [new A() instanceof Iterator,Object.getPrototypeOf(new A())===A.prototype,Object.prototype.toString.call(new A()),new A()[Symbol.toStringTag]]})`,
  `T(()=>Reflect.construct(Iterator,[],function(){}).constructor===Iterator)`, `T(()=>Reflect.construct(Iterator,[],Iterator)===undefined)`,
  `T(()=>D(Iterator,'prototype'))`, `T(()=>Iterator.prototype.constructor===Iterator)`, `T(()=>Iterator.prototype[Symbol.toStringTag])`,
  `T(()=>{Iterator.prototype[Symbol.toStringTag]='x'})`, `T(()=>{var o=Object.create(Iterator.prototype);o[Symbol.toStringTag]='x';return D(o,Symbol.toStringTag)})`,
  `T(()=>{'use strict';var o=Object.create(Iterator.prototype);o[Symbol.toStringTag]='x';return [D(o,Symbol.toStringTag),o[Symbol.toStringTag]]})`,
  `T(()=>{'use strict';Iterator.prototype[Symbol.toStringTag]='x'})`, `T(()=>{'use strict';Iterator.prototype.constructor=1})`,
  `T(()=>{'use strict';var o=Object.create(Iterator.prototype);o.constructor=1;return [D(o,'constructor'),o.constructor===Iterator]})`,
  `T(()=>{'use strict';var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return [d.set.call(Object.create(Iterator.prototype),'q')]})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return d.get.call(undefined)})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return d.get.call({})})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return [d.set.name,d.get.name,d.set.length,d.get.length]})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return d.set.call(undefined,1)})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);return d.set.call(Iterator.prototype,1)})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag);var o={};d.set.call(o,'z');return D(o,Symbol.toStringTag)})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor');var o={};d.set.call(o,'z');return D(o,'constructor')})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor');return [d.get.call(undefined)===Iterator,d.get.call({})===Iterator]})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor');return d.set.call(Iterator.prototype,1)})`,
  `T(()=>{var d=Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor');return d.set.call(undefined,1)})`,
  `T(()=>{var h=[].values().map(x=>x);return [Object.prototype.toString.call(h),h[Symbol.toStringTag],String(h),h+''===String(h)]})`,
  `T(()=>{var h=[].values().map(x=>x);return [h[Symbol.iterator]===Iterator.prototype[Symbol.iterator],h[Symbol.iterator]()===h,Reflect.ownKeys(h),Object.isExtensible(h),Object.isFrozen(h)]})`,
  `T(()=>{var P=${protoHelperExpr};return [P.next.name,P.next.length,P.return.name,P.return.length,Object.getPrototypeOf(P.next)===Function.prototype,Object.hasOwn(P.next,'prototype')]})`,
  `T(()=>{var P=${protoHelperExpr};return P.next.call({})})`, `T(()=>{var P=${protoHelperExpr};return P.return.call({})})`, `T(()=>{var P=${protoHelperExpr};return P.next.call(undefined)})`,
  `T(()=>{var P=${protoHelperExpr};return P.return.call(1)})`, `T(()=>{var P=${protoHelperExpr};return P.next.call([].values())})`, `T(()=>{var P=${protoHelperExpr};return P.return.call([].values())})`,
  `T(()=>{var P=${protoHelperExpr};return P.next.call(Iterator.from({next(){}}))})`, `T(()=>{var P=${protoHelperExpr};return P.next.call(P)})`, `T(()=>{var P=${protoHelperExpr};return P.next.call(Object.create(P))})`,
  `T(()=>{var P=${protoHelperExpr};var h=[1].values().map(x=>x);var g=[1].values().filter(x=>1);return [P.next.call(h).value,P.next.call(g).value]})`,
  `T(()=>{var P=${protoHelperExpr};var h=[1].values().map(x=>x);return [P.return.call(h),h.next()]})`,
  `T(()=>{var P=${protoHelperExpr};var h=[1].values().map(x=>x);return P.next.call.call(P.next,h)})`,
  `T(()=>{var P=${protoHelperExpr};var h=[1,2].values().map(x=>x*10);P.x=1;return [h.x,Object.getPrototypeOf(h)===P]})`,
  `T(()=>{var a=[].values().map(x=>x),b=[].values().filter(x=>x),c=[].values().take(1),d=[].values().drop(1),e=[].values().flatMap(x=>[]);var P=Object.getPrototypeOf(a);return [P===Object.getPrototypeOf(b),P===Object.getPrototypeOf(c),P===Object.getPrototypeOf(d),P===Object.getPrototypeOf(e),P===Object.getPrototypeOf(Iterator.concat())]})`,
  `T(()=>{var G=Object.getPrototypeOf(function*(){}.prototype);return [Object.getPrototypeOf(${protoHelperExpr})===G,Object.getPrototypeOf(G)===Iterator.prototype]})`,
  `T(()=>{var AP=Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));return [AP===Iterator.prototype,Object.getPrototypeOf(Object.getPrototypeOf(new Map().entries()))===Iterator.prototype]})`,
  `T(()=>[typeof Iterator.zip,typeof Iterator.zipKeyed,typeof Iterator.range,typeof Iterator.prototype.chunks,typeof Iterator.prototype.windows,typeof Iterator.prototype.flat])`,
  `T(()=>Object.getOwnPropertyNames(Iterator).sort())`, `T(()=>Object.getOwnPropertyNames(Iterator.prototype).sort())`,
);

// ---- 11. Receptores variados com helpers: geradores, Map/Set/String, proxies, subclasses, arrays com buracos.
const sources = [
  "[1,2,3].values()", "[1,,3].values()", "'ab'[Symbol.iterator]()", "new Set([1,2,3]).values()", "new Map([[1,'a'],[2,'b']]).entries()", "(function*(){yield 1;yield 2;yield 3})()", "[].values()", "new Uint8Array([1,2,3]).values()",
  "'a😀b'[Symbol.iterator]()", "[1,2,3].keys()", "[1,2,3].entries()", "'a1b2'.matchAll(/\\d/g)", "new (class extends Iterator{#i=0;next(){return this.#i<3?{done:false,value:this.#i++}:{done:true}}})", "new Proxy([1,2,3].values(),{})",
  "(function*(){try{yield 1;yield 2}finally{globalThis.fin=(globalThis.fin||0)+1}})()",
];
for (const s of sources) for (const n of allMethods) {
  add(`(()=>{globalThis.fin=0;return Rn([],()=>S([${call(n, s, validArg(n))},globalThis.fin]))})()`);
}
for (const s of sources) {
  add(`Rn([],()=>{var h=${s}.take(1);return S([h.next(),h.next(),globalThis.fin])})`);
  add(`Rn([],()=>{var h=${s}.map(x=>x);h.next();return S([h.return(),h.next(),globalThis.fin])})`);
}
// Gerador subjacente: finally roda no return, não roda quando já terminou.
add(
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}var h=g().take(1);return S([Dr(h),log])})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}var h=g().map(x=>x);h.next();h.return();return S([h.next(),log])})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}return Rn(log,()=>S([g().find(x=>x==2),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}return Rn(log,()=>S([g().toArray(),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}return Rn(log,()=>S([g().some(x=>x==9),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}return Rn(log,()=>S([g().every(x=>x==1),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin')}}return Rn(log,()=>S([g().forEach(x=>{if(x==2)throw 9}),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin');yield 99}}var h=g().take(1);return Rn(log,()=>S([Dr(h),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin');throw new EvalError('f')}}var h=g().take(1);return Rn(log,()=>S([Dr(h),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2;yield 3}finally{log.push('fin');throw new EvalError('f')}}var h=g().map(x=>{throw new RangeError('c')});return Rn(log,()=>S([Dr(h),log]))})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2}finally{log.push('fin')}}var h=g().flatMap(x=>[x,x]);h.next();h.return();return S([h.next(),log])})()`,
  `(()=>{var log=[];function*g(){try{yield 1;yield 2}finally{log.push('fin')}}var h=g().flatMap(x=>{throw 1});return Rn(log,()=>h.next())})()`,
  `(()=>{var log=[];function*g(){var r=yield 1;log.push('r:'+r);r=yield 2;log.push('r:'+r)}var h=g().map(x=>x);h.next('a');h.next('b');h.next('c');return S(log)})()`,
  `(()=>{var log=[];function*g(){try{yield 1}catch(e){log.push('c')}}var h=g().map(x=>x);h.next();h.return();return S([log,h.next()])})()`,
);

// ---- 12. flatMap rejeita strings e não iteráveis; aceita objetos-iteradores e String objects.
const flatValues = [
  "'ab'", "''", "new String('ab')", "[1,2]", "[]", "new Set([1,2])", "new Map([[1,2]])", "1", "null", "undefined", "true", "Symbol()", "1n", "{}", "{length:1,0:'a'}", "function(){}", "[1,2].values()",
  "{next(){return {done:true}}}", "{next(){return {done:true}},[Symbol.iterator]:undefined}", "{[Symbol.iterator]:null}", "{[Symbol.iterator]:null,next(){return {done:true}}}", "{[Symbol.iterator]:1,next(){return {done:true}}}",
  "{__proto__:Iterator.prototype,next(){return {done:true}}}", "function*(){yield 1}()", "{[Symbol.iterator](){return {next(){return {done:false,value:5}}}}}", "new Proxy([1,2],{})", "new Uint8Array([7,8])", "Object('abc')",
  "{[Symbol.iterator](){return null}}", "{get [Symbol.iterator](){throw new EvalError('g')}}", "{[Symbol.iterator](){throw new EvalError('c')}}", "{[Symbol.iterator]:Array.prototype.values}", "Iterator.from('ab')", "Iterator.concat([1],[2])",
  "new String('')", "Object(Symbol())", "Object(1n)", "Object(true)", "[[1,2],[3]]", "{[Symbol.iterator]:function*(){yield 1;yield 2}}",
];
for (const v of flatValues) {
  add(`(()=>{var log=[];return Rn(log,()=>S([Dr(It(log,[1,2]).flatMap(x=>${v}),8),log]))})()`);
  add(`(()=>{var log=[];return Rn(log,()=>S([Dr(It(log,[1,2],{noReturn:true}).flatMap(x=>${v}),8),log]))})()`);
  add(`(()=>{var log=[];var h=It(log,[1,2]).flatMap(x=>${v});try{h.next()}catch(e){}return Rn(log,()=>S([h.next(),log]))})()`);
}

// ---- 13. Índice, contador e argumentos recebidos por cada callback.
for (const n of ["map", "filter", "flatMap", "forEach", "some", "every", "find"]) {
  add(`(()=>{var log=[];var f=function(){log.push(Array.prototype.slice.call(arguments).join('/')+':'+(this===undefined)+':'+arguments.length);return ${n === "flatMap" ? "[]" : "0"}};${lazy.has(n) ? `Dr(M("${n}",[5,6,7].values(),f))` : `M("${n}",[5,6,7].values(),f)`};return S(log)})()`);
  add(`(()=>{var log=[];var f=function(){'use strict';log.push(typeof this);return ${n === "flatMap" ? "[]" : "0"}};${lazy.has(n) ? `Dr(M("${n}",[5].values(),f))` : `M("${n}",[5].values(),f)`};return S(log)})()`);
  add(`(()=>{var log=[];var o={__proto__:Iterator.prototype,next(){log.push('n:'+arguments.length+':'+(this===o));return {done:true}}};M("${n}",o,${validArg(n)});${lazy.has(n) ? "" : ""}return S(log)})()`);
  add(`(()=>{var log=[];var o={__proto__:Iterator.prototype,next(){log.push('n:'+arguments.length+':'+(this===o));return {done:true}}};${call(n, "o", validArg(n))};return S(log)})()`);
}
add(
  `(()=>{var log=[];var o={__proto__:Iterator.prototype,next(){log.push('n:'+arguments.length+':'+(this===o));return {done:false,value:1}},return(){log.push('r:'+arguments.length+':'+(this===o));return {}}};var h=o.take(1);h.next(1,2,3);h.next();h.return(4);return S(log)})()`,
  `(()=>{var log=[];var o={__proto__:Iterator.prototype,next(){return {done:false,value:1}},return(){log.push('r:'+arguments.length+':'+(this===o));return {}}};var h=o.map(x=>x);h.next();h.return(4,5);return S(log)})()`,
  `T(()=>[1,2,3].values().reduce((a,b,i)=>a+':'+b+':'+i,'s'))`, `T(()=>[1,2,3].values().reduce((a,b,i)=>a+':'+b+':'+i))`,
  `T(()=>{var c=0;[1,2,3].values().map(x=>x).next();return c})`,
  `T(()=>{var f=x=>x;var h=[1,2].values().map(f);return [h.next().value,h.next().value,h.next().done]})`,
  `T(()=>{var h=[1,2,3].values().map(x=>x*2);return [h.next(),h.next(),h.next(),h.next(),h.next()]})`,
  `T(()=>{var h=[1,2,3].values().filter(x=>x!==2);return [h.next(),h.next(),h.next(),h.next()]})`,
  `T(()=>{var h=[1,2,3].values().take(2);return [h.next(),h.next(),h.next(),h.next()]})`,
  `T(()=>{var h=[1,2,3].values().drop(2);return [h.next(),h.next(),h.next()]})`,
  `T(()=>{var h=[1,2,3].values().flatMap(x=>[x,-x]);return [h.next(),h.next(),h.next(),h.next(),h.next(),h.next(),h.next()]})`,
  `T(()=>[1,2,3].values().toArray().concat([1,2,3].values().toArray()))`,
  `T(()=>{var a=[1,2,3];var h=a.values().map(x=>x);a.push(4);return h.toArray()})`,
  `T(()=>{var a=[1,2,3];var h=a.values().map(x=>{if(x==1)a.length=1;return x});return h.toArray()})`,
  `T(()=>{var s=new Set([1,2]);var h=s.values().map(x=>{if(x<5)s.add(x+2);return x});return h.take(6).toArray()})`,
  `T(()=>{var it=[1,2,3].values();var h=it.map(x=>x);it.next();return h.toArray()})`,
  `T(()=>{var it=[1,2,3].values();var h=it.take(1);h.next();return [it.next(),h.next(),it.next()]})`,
  `T(()=>{var it=[1,2,3].values();var a=it.map(x=>x),b=it.map(x=>x*10);return [a.next(),b.next(),a.next(),b.next()]})`,
  `T(()=>{var it=[1,2,3].values();var a=it.take(1);a.next();a.next();return it.next()})`,
  `T(()=>{var it=[1,2,3].values();var a=it.drop(1);a.next();return it.next()})`,
  `T(()=>{var it=[1,2,3].values();it.find(x=>x==1);return it.next()})`,
  `T(()=>{var it=[1,2,3].values();it.some(x=>x==1);return it.next()})`,
  `T(()=>{var it=[1,2,3].values();it.every(x=>x==2);return it.next()})`,
  `T(()=>{var it=[1,2,3].values();it.forEach(x=>x);return it.next()})`,
  `T(()=>{var it=[1,2,3].values();it.toArray();return it.next()})`,
  `T(()=>{var it=[1,2,3].values();it.reduce((a,b)=>a);return it.next()})`,
  `T(()=>{var it=[1,2,3].values();var h=it.map(x=>x);h.return();return it.next()})`,
  `T(()=>{var it=[1,2,3].values();var h=it.map(x=>x);h.return();return h.next()})`,
);

// ---- Dedup por fonte contra os goldens existentes e geração.
const baseSources = [];
baseSources.push(...knownPrograms("iterator_helpers_bun.tsv", ["iterator_bun.tsv", "iterator_protocol_bun.tsv", "collection_async_bun.tsv", "esnext_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e) && (!process.env.ONLY_STAGES || /return \$|;return h\.|var r=\(\(\)=>/.test(e)));
let kept = 0;
const PRELOAD = writeResultPreload();
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida.
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${/^\(\(\)=>\{/.test(expr) || /^(Rn|T)\(/.test(expr) ? expr : "T(()=>" + expr + ")"}`;
  let result;
  try {
    // Processo fresco por programa: o JSC reifica as tabelas estáticas por ordem de acesso.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 5000 });
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
