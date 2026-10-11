// Gera tests/golden/error_ctor_bun.tsv: construtores Error, medido no bun 1.4.2.
// Cobre os oito construtores (Error, TypeError, RangeError, SyntaxError, ReferenceError, EvalError, URIError,
// AggregateError) com e sem `new`, message de tipos variados (objeto com toString que loga, Symbol que lança), options
// com cause (getter que loga, has vs undefined, Proxy), AggregateError com iteráveis exóticos, subclasses,
// newTarget exótico, Error.prototype.toString com name e message exóticos e receptores inválidos, e descritores de
// message, cause, errors e dos próprios construtores. O resultado nunca inclui `stack` nem as propriedades de posição
// do bun (line, column, sourceURL, originalLine, originalColumn). Programas cuja expressão já aparece nos goldens
// error_*.tsv e stack_*.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-error-ctor-golden.js > tests/golden/error_ctor_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'var L=[];var X={stack:1,line:1,column:1,sourceURL:1,originalLine:1,originalColumn:1};' +
  'var N=["Error","TypeError","RangeError","SyntaxError","ReferenceError","EvalError","URIError","AggregateError"];\n' +
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);if(d>3)return "...";if(v instanceof Error)return "E("+v.name+":"+v.message+")";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return f()}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)+" L="+L.join()}}\n' +
  'function E(e){var s;try{s=e.toString()}catch(x){s="throw "+x.name}var p=Object.getPrototypeOf(e);' +
  'var pn=N.find(n=>globalThis[n].prototype===p)||(p===Object.prototype?"Object":p===null?"null":"other");' +
  'return "p="+pn+" n="+S(e.name)+" m="+S(e.message)+" s="+S(s)+" k="+Reflect.ownKeys(e).filter(k=>!X[k]).map(k=>S(k)+":"+D(e,k)).join(";")+" L="+L.join()}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

const SIMPLE = ["Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError"];
const ALL = [...SIMPLE, "AggregateError"];
const show = (e) => `T(()=>E(${e}))`;
const forms = (c, args) => [`${c}(${args})`, `new ${c}(${args})`];

const MESSAGES = [
  "undefined", "null", "0", "-0", "1", "1.5", "NaN", "Infinity", "true", "''", "'m'", "'a\\nb'", "'\\u00e9\\ud83d\\ude00'", "1n", "[]", "[1,[2]]", "{}", "{a:1}",
  "{toString(){L.push('ts');return 'x'}}", "{valueOf(){L.push('vo');return 'v'},toString:undefined}", "{valueOf(){L.push('vo');return 1}}",
  "{toString(){L.push('ts');return {}},valueOf(){L.push('vo');return 7}}", "{[Symbol.toPrimitive](h){L.push(h);return 'tp'}}",
  "{toString(){throw new RangeError('ts')}}", "{toString(){return {}},valueOf(){return {}}}", "Symbol('s')", "Symbol()", "Object(Symbol('o'))",
  "{[Symbol.toPrimitive](){return Symbol('q')}}", "new String('ns')", "Object.create(null)", "function f(){}", "new Error('inner')", "2**53", "1e21", "123456.789e-9",
  "new Proxy({},{get(t,k){L.push(String(k));return undefined}})", "new Proxy({},{get(t,k){L.push(String(k));return k===Symbol.toPrimitive?undefined:()=>'px'}})",
  "{toString:1,valueOf:2}", "{toString(){L.push('ts');throw 5}}", "new Number(3)", "new Boolean(false)", "[null]", "[undefined,undefined]", "['a','b']",
];

const OPTIONS = [
  "undefined", "null", "1", "'x'", "true", "Symbol()", "1n", "{}", "{cause:1}", "{cause:undefined}", "{cause:null}", "{cause:0}", "{cause:''}", "{cause:NaN}",
  "{get cause(){L.push('get');return 'c'}}", "{get cause(){throw new EvalError('g')}}", "Object.create({cause:'inh'})", "{__proto__:null,cause:2}",
  "new Proxy({cause:1},{has(t,k){L.push('has '+String(k));return k in t},get(t,k){L.push('get '+String(k));return t[k]}})",
  "new Proxy({},{has(t,k){L.push('has '+String(k));return false},get(t,k){L.push('get '+String(k))}})",
  "new Proxy({},{has(t,k){L.push('has '+String(k));return true},get(t,k){L.push('get '+String(k));return 'viaProxy'}})",
  "new Proxy({},{has(){throw new URIError('h')}})", "new Proxy({cause:1},{get(){throw new RangeError('gp')}})",
  "{cause:new Error('c')}", "{cause:{}}", "{cause:Symbol('c')}", "{cause:{a:1}}", "{cause:undefined,extra:1}", "{CAUSE:1}", "{'cause ':1}", "{[Symbol('cause')]:1}",
  "Object.defineProperty({},'cause',{value:3,enumerable:false})", "Object.defineProperty({},'cause',{get(){L.push('g');return 4},enumerable:false})",
  "[]", "[1]", "function(){}", "Object.assign(function(){},{cause:5})", "new String('abc')", "Object.assign([],{cause:6})",
  "(()=>{var r=Proxy.revocable({},{});r.revoke();return r.proxy})()", "Object.freeze({cause:7})", "{cause:{get x(){L.push('x');return 1}}}",
  "Object.create(null,{cause:{value:8}})", "Object.create({get cause(){L.push('pg');return 9}})",
];

// ---- 1. message de tipos variados, para os sete construtores simples e AggregateError (com errors vazio).
for (const c of SIMPLE) for (const m of MESSAGES) for (const f of forms(c, m)) add(show(f));
for (const c of SIMPLE) for (const f of forms(c, "")) add(show(f));
for (const m of MESSAGES) for (const f of forms("AggregateError", `[],${m}`)) add(show(f));

// ---- 2. options com cause, message ausente e presente.
for (const c of SIMPLE) for (const o of OPTIONS) {
  for (const f of forms(c, `'m',${o}`)) add(show(f));
  for (const f of forms(c, `undefined,${o}`)) add(show(f));
}
for (const o of OPTIONS) for (const f of forms("AggregateError", `[1],'m',${o}`)) add(show(f));
for (const o of OPTIONS) for (const f of forms("AggregateError", `[1],undefined,${o}`)) add(show(f));
for (const c of SIMPLE) for (const f of forms(c, "'m',{cause:1},'extra'")) add(show(f));

// ---- 3. message e options juntos (ordem de coerção e de acesso).
const crossMessages = [
  "{toString(){L.push('ts');return 'x'}}", "{toString(){throw new RangeError('ts')}}", "Symbol('s')", "undefined", "1n", "''", "{[Symbol.toPrimitive](h){L.push(h);return 'tp'}}", "'m'",
];
const crossOptions = [
  "{get cause(){L.push('get');return 'c'}}", "new Proxy({cause:1},{has(t,k){L.push('has '+String(k));return k in t},get(t,k){L.push('get '+String(k));return t[k]}})",
  "{cause:undefined}", "null", "{cause:1}", "{get cause(){throw new EvalError('g')}}", "new Proxy({},{has(){throw new URIError('h')}})", "1",
];
for (const c of SIMPLE) for (const m of crossMessages) for (const o of crossOptions) for (const f of forms(c, `${m},${o}`)) add(show(f));
for (const m of crossMessages) for (const o of crossOptions) add(show(`new AggregateError([L.push('it')&&1],${m},${o})`));

// ---- 4. AggregateError com iteráveis exóticos.
const ITERABLES = [
  "[]", "[1,2]", "[,1]", "[1,,3]", "new Set([1,1,2])", "'ab'", "'\\ud83d\\ude00x'", "new Map([[1,2]])", "new Uint8Array(2)", "(function(){return arguments})(1,2)",
  "undefined", "null", "1", "true", "{}", "{length:1,0:'a'}", "Symbol()", "1n", "function(){}", "new String('xy')",
  "(function*(){L.push('g0');yield 1;L.push('g1');yield 2})()", "(function*(){yield 1;throw new RangeError('gen')})()",
  "(function*(){try{yield 1;yield 2}finally{L.push('fin')}})()",
  "{[Symbol.iterator](){L.push('it');return {next(){L.push('next');return {done:true}}}}}",
  "{[Symbol.iterator](){L.push('it');return {next(){L.push('next');throw new EvalError('n')}}}}",
  "{[Symbol.iterator](){L.push('it');return {next(){L.push('next');return 1}}}}",
  "{[Symbol.iterator](){L.push('it');return {next(){L.push('next');return {get done(){L.push('done');return true},get value(){L.push('value');return 1}}}}}}",
  "{[Symbol.iterator](){L.push('it');var i=0;return {next(){L.push('next');return {done:i++>1,value:i}},return(){L.push('return');return {}}}}}",
  "{[Symbol.iterator](){L.push('it');return {get next(){L.push('getnext');return function(){return {done:true}}}}}}",
  "{[Symbol.iterator](){L.push('it');return 1}}", "{[Symbol.iterator](){L.push('it');return {}}}", "{[Symbol.iterator]:undefined}", "{[Symbol.iterator]:null}", "{[Symbol.iterator]:1}",
  "{get [Symbol.iterator](){L.push('gi');throw new URIError('gi')}}", "{[Symbol.iterator](){throw new RangeError('it')}}",
  "Object.assign([1,2],{[Symbol.iterator]:function*(){yield 'custom'}})", "new Proxy([1,2],{get(t,k){L.push(String(k));return t[k]}})",
  "[new Error('a'),new TypeError('b')]", "[1,[2,[3]]]", "[undefined,null]", "Array.from({length:5},(_,i)=>i)", "[Symbol.iterator]", "[{a:1}]", "[NaN,-0]",
  "(()=>{var r=Proxy.revocable([],{});r.revoke();return r.proxy})()", "Object.create(Array.prototype)", "'x'.matchAll(/x/g)", "new Set([1]).values()", "[7].entries()",
];
for (const it of ITERABLES) {
  for (const f of forms("AggregateError", it)) add(show(f));
  for (const f of forms("AggregateError", `${it},'m'`)) add(show(f));
  for (const f of forms("AggregateError", `${it},{toString(){L.push('ts');return 'x'}},{get cause(){L.push('cause');return 1}}`)) add(show(f));
}
add(
  show("AggregateError()"), show("new AggregateError()"), show("new AggregateError(undefined,'m')"), show("new AggregateError([],undefined,{cause:1})"),
  "T(()=>{var a=[1];var e=new AggregateError(a);a.push(2);return S(e.errors)+(e.errors===a)})",
  "T(()=>{var a=[1];var e=new AggregateError(a);e.errors.push(3);return S(a)+S(e.errors)})",
  "T(()=>{var e=new AggregateError([1]);return D(e,'errors')})", "T(()=>{var e=new AggregateError([1],'m');return Reflect.ownKeys(e).filter(k=>!X[k]).map(String).join()})",
  "T(()=>{var e=new AggregateError([1],'m',{cause:2});return Reflect.ownKeys(e).filter(k=>!X[k]).map(String).join()})",
  "T(()=>{var e=new AggregateError([1],undefined,{cause:2});return Reflect.ownKeys(e).filter(k=>!X[k]).map(String).join()})",
  "T(()=>Array.isArray(new AggregateError([]).errors))", "T(()=>Object.getPrototypeOf(AggregateError)===Error)", "T(()=>Object.getPrototypeOf(AggregateError.prototype)===Error.prototype)",
  "T(()=>AggregateError.length+AggregateError.name)", "T(()=>D(AggregateError.prototype,'errors'))", "T(()=>D(AggregateError.prototype,'name')+'|'+D(AggregateError.prototype,'message'))",
  "T(()=>new AggregateError([]) instanceof AggregateError && new AggregateError([]) instanceof Error)", "T(()=>Object.prototype.toString.call(new AggregateError([])))",
  "T(()=>String(new AggregateError([],'hi')))", "T(()=>Object.getOwnPropertyNames(AggregateError.prototype).join())", "T(()=>Object.getOwnPropertyNames(AggregateError).join())",
  "T(()=>{var e=new AggregateError([1]);e.errors=5;return S(e.errors)})", "T(()=>{'use strict';var e=new AggregateError([1]);delete e.errors;return Reflect.ownKeys(e).filter(k=>!X[k]).length})",
);

// ---- 5. subclasses e newTarget exótico.
const subclasses = [
  "class K extends C{}", "class K extends C{constructor(...a){super(...a);this.extra=1}}", "class K extends C{constructor(m){super(m,{cause:'k'})}}",
  "class K extends C{get message(){return 'g'}}", "class K extends C{constructor(m){super(m);this.message='over'}}", "class K extends C{constructor(){super()}}",
  "class K extends C{constructor(m){super(String(m)+'!')}}", "class K extends C{get name(){return 'Named'}}", "class K extends C{static [Symbol.hasInstance](){return true}}",
  "class K extends C{constructor(m,o){super(m,o);delete this.message}}", "class K extends C{constructor(m,o){super(m,o);Object.defineProperty(this,'cause',{value:'late'})}}",
  "class K extends C{} K.prototype.name='KK'", "class K extends C{} Object.defineProperty(K.prototype,'name',{value:undefined})",
  "class K extends C{} K.prototype.message='proto-msg'", "class K extends C{constructor(){return {fake:1}}}", "class K extends C{constructor(){super();return super.constructor===C}}",
];
for (const c of ALL) {
  const a = c === "AggregateError" ? "[1]," : "";
  for (const s of subclasses) {
    const body = s.replace(/\bC\b/g, c);
    add(show(`(()=>{${body};return new K(${a}'m')})()`), show(`(()=>{${body};return new K(${a}undefined,{cause:1})})()`), `T(()=>{${body};try{return E(K(${a}'m'))}catch(e){return e.name+': '+e.message}})`);
  }
  const nts = [
    "Object", "function(){}", "Object.assign(function(){},{prototype:1})", "Object.assign(function(){},{prototype:null})", "function(){}.bind()", "Array", "Date", "Function.prototype",
    "new Proxy(function(){},{get(t,k){L.push(String(k));return k==='prototype'?Array.prototype:t[k]}})", "new Proxy(function(){},{get(t,k){L.push(String(k));throw new RangeError('nt')}})",
    "Object.assign(function(){},{prototype:Object.create(Error.prototype)})", "Object.assign(function(){},{prototype:TypeError.prototype})", "class{}", "Symbol", "Proxy", "TypeError", "AggregateError",
  ];
  for (const nt of nts) {
    add(show(`Reflect.construct(${c},[${a}'m'],${nt})`), show(`Reflect.construct(${c},[${a}undefined,{cause:2}],${nt})`));
  }
  add(
    show(`Reflect.construct(${c},[${a}'m'],1)`), show(`Reflect.construct(${c},[${a}'m'],null)`), show(`Reflect.construct(${c},[${a}'m'],()=>1)`),
    show(`Reflect.construct(${c},[${a}'m'],undefined)`), show(`Reflect.apply(${c},undefined,[${a}'m'])`), show(`Reflect.apply(${c},{},[${a}'m'])`),
    show(`${c}.call(1,${a}'m')`), show(`${c}.call(new ${c}(${a}'z'),${a}'m')`), show(`new (${c}.bind(null,${a}'bound'))('extra')`), show(`new (${c}.bind(null,${a}'bound'))`),
    show(`${c}.bind(null)(${a}'m')`), show(`Reflect.construct(${c},{length:${a ? 2 : 1},0:${a ? "[1]" : "'a'"},1:'b'})`),
  );
}

// ---- 6. Error.prototype.toString: name e message exóticos, receptores inválidos.
const NAMES = [
  "undefined", "null", "''", "'N'", "'a b'", "1", "true", "{toString(){L.push('nts');return 'ON'}}", "{toString(){throw new RangeError('nthrow')}}", "Symbol('n')", "'\\n'", "[]", "['x','y']", "-0", "1n",
];
const MSGS2 = [
  "undefined", "null", "''", "'M'", "'a b'", "1", "false", "{toString(){L.push('mts');return 'OM'}}", "{toString(){throw new EvalError('mthrow')}}", "Symbol('m')", "'\\n'", "[]", "0", "1n",
];
for (const n of NAMES) for (const m of MSGS2) add(`T(()=>S(Error.prototype.toString.call({name:${n},message:${m}})))`);
for (const n of NAMES) {
  add(`T(()=>{var e=new TypeError('m');e.name=${n};return S(String(e))})`, `T(()=>{var e=new Error();e.name=${n};return S(String(e))})`, `T(()=>{var o={message:'only'};o.name=${n};return S(Error.prototype.toString.call(o))})`);
}
for (const m of MSGS2) {
  add(`T(()=>{var e=new Error();e.message=${m};return S(String(e))})`, `T(()=>{var e=new RangeError();e.message=${m};return S(String(e))})`, `T(()=>S(Error.prototype.toString.call({name:'only',message:${m}})))`);
}
const RECEIVERS = [
  "undefined", "null", "1", "'str'", "true", "1n", "Symbol('r')", "function(){}", "()=>1", "class{}", "[]", "[1,2]", "{}", "Object.create(null)", "Object.create({name:'inh',message:'inh'})",
  "{get name(){L.push('gname');return 'GN'},get message(){L.push('gmsg');return 'GM'}}", "{get name(){throw new RangeError('gn')},message:'m'}", "{name:'n',get message(){throw new RangeError('gm')}}",
  "new Proxy({},{get(t,k){L.push(String(k));return undefined}})", "new Proxy({name:'P',message:'Q'},{})", "(()=>{var r=Proxy.revocable({},{});r.revoke();return r.proxy})()",
  "new Error('e')", "new TypeError('t')", "new AggregateError([],'ag')", "Error.prototype", "TypeError.prototype", "AggregateError.prototype", "Object.assign(Object.create(Error.prototype),{message:'x'})",
  "Object(1)", "new String('s')", "new Date(NaN)", "/re/", "new Map", "globalThis", "Math", "JSON", "Reflect", "new Number(2)", "new Boolean(true)", "Object(Symbol())", "Object(1n)",
  "{name:undefined,message:undefined}", "{name:null,message:null}", "{name:'',message:''}", "{name:'',message:'m'}", "{name:'n',message:''}", "{name:{toString(){return 'x'}},message:{toString(){return 'y'}}}",
];
for (const r of RECEIVERS) add(`T(()=>S(Error.prototype.toString.call(${r})))`, `T(()=>S(TypeError.prototype.toString.call(${r})))`, `T(()=>S(AggregateError.prototype.toString.call(${r}))+" L="+L.join())`);
add(
  "T(()=>Error.prototype.toString===TypeError.prototype.toString)", "T(()=>Error.prototype.toString.length+Error.prototype.toString.name)", "T(()=>D(Error.prototype,'toString'))",
  "T(()=>new Error.prototype.toString)", "T(()=>Error.prototype.toString())", "T(()=>S(Error.prototype.toString()))", "T(()=>S(String(Error.prototype)))", "T(()=>S(String(TypeError.prototype)))",
  "T(()=>S(Error.prototype+''))", "T(()=>S(`${new RangeError('t')}`))", "T(()=>S(''+new URIError))", "T(()=>S([new Error('a'),new TypeError('b')].join()))", "T(()=>S(String([new Error('a')])))",
  "T(()=>S(JSON.stringify(new Error('m'))))", "T(()=>S(JSON.stringify({e:new TypeError('m',{cause:1})})))", "T(()=>S(Object.prototype.toString.call(new Error)))",
  "T(()=>{var e=new Error('m');e.toString=null;return S(String(e))})", "T(()=>{var e=new Error('m');e.toString=()=>'custom';return S(e+'')})",
  "T(()=>{var e=new Error('m');e[Symbol.toPrimitive]=()=>'prim';return S(e+'')})",
);

// ---- 7. descritores e forma dos construtores e protótipos.
for (const c of ALL) {
  add(
    `T(()=>D(${c},'prototype'))`, `T(()=>D(${c},'name'))`, `T(()=>D(${c},'length'))`, `T(()=>D(${c}.prototype,'name'))`, `T(()=>D(${c}.prototype,'message'))`, `T(()=>D(${c}.prototype,'constructor'))`,
    `T(()=>D(${c}.prototype,'toString'))`, `T(()=>D(${c}.prototype,'cause'))`, `T(()=>D(${c}.prototype,'stack'))`, `T(()=>D(globalThis,${JSON.stringify(c)}))`,
    `T(()=>Object.getPrototypeOf(${c})===${c === "Error" ? "Function.prototype" : "Error"})`, `T(()=>Object.getPrototypeOf(${c}.prototype)===${c === "Error" ? "Object.prototype" : "Error.prototype"})`,
    `T(()=>${c}.prototype.constructor===${c})`, `T(()=>${c}.name+${c}.length)`, `T(()=>Object.getOwnPropertyNames(${c}.prototype).sort().join())`, `T(()=>S(String(${c}.prototype)))`,
    `T(()=>Object.prototype.toString.call(${c}.prototype))`, `T(()=>Object.prototype.toString.call(new ${c}(${c === "AggregateError" ? "[]" : ""})))`, `T(()=>typeof ${c}.prototype)`,
    `T(()=>${c}.prototype instanceof Error)`, `T(()=>${c}.prototype instanceof ${c})`, `T(()=>Object.isExtensible(${c})+","+Object.isFrozen(${c}.prototype))`, `T(()=>Object.getOwnPropertySymbols(${c}).length)`,
    show(`new ${c}(${c === "AggregateError" ? "[]," : ""}'m')`), `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');return D(e,'message')})`,
    `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m',{cause:1});return D(e,'cause')})`, `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""});return D(e,'message')+"|"+("message" in e)})`,
    `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}undefined);return Object.hasOwn(e,'message')})`, `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'');return Object.hasOwn(e,'message')})`,
    `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m',{cause:undefined});return Object.hasOwn(e,'cause')})`, `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m',{});return Object.hasOwn(e,'cause')})`,
    `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');e.message='z';e.name='NN';return S(String(e))})`, `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');delete e.message;return S(e.message)})`,
    `T(()=>{'use strict';var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');Object.freeze(e);e.message='z'})`, `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');return Object.keys(e).length+","+JSON.stringify(Object.keys(e))})`,
    `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m',{cause:1});return Object.keys(e).length+","+JSON.stringify(Object.keys(e))})`, `T(()=>{var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');return S(Object.getOwnPropertyNames(e).filter(k=>!X[k]))})`,
    `T(()=>${c}.prototype.hasOwnProperty('cause')+","+('cause' in ${c}.prototype))`, `T(()=>{${c}.prototype.cause='pc';var e=new ${c}(${c === "AggregateError" ? "[]," : ""}'m');return S(e.cause)+Object.hasOwn(e,'cause')})`,
    `T(()=>{${c}.prototype.message='pm';var e=new ${c}(${c === "AggregateError" ? "[]," : ""});return S(e.message)+Object.hasOwn(e,'message')+S(String(e))})`,
    `T(()=>{${c}.prototype.name='pn';return S(String(new ${c}(${c === "AggregateError" ? "[]," : ""}'m')))})`,
    `T(()=>{var o=Object.create(${c}.prototype);return S(String(o))+(o instanceof Error)+Object.prototype.toString.call(o)})`,
    `T(()=>{var e=Object.setPrototypeOf({},${c}.prototype);return S(e.message)+S(e.name)})`,
  );
}
add(
  "T(()=>D(Error,'stackTraceLimit')===undefined)", "T(()=>typeof Error.captureStackTrace===typeof Error.prepareStackTrace)", "T(()=>Object.getOwnPropertyNames(Error.prototype).sort().join())",
  "T(()=>Object.getOwnPropertyNames(TypeError).sort().join())", "T(()=>Object.getOwnPropertyNames(Error.prototype).map(k=>k+':'+D(Error.prototype,k)).sort().join('|'))",
  "T(()=>Error.prototype.hasOwnProperty('message')+','+Error.prototype.hasOwnProperty('cause'))", "T(()=>S(Error.prototype.message)+S(Error.prototype.name))",
  "T(()=>[TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError].map(c=>Object.getPrototypeOf(c)===Error).join())",
  "T(()=>[TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError].map(c=>c.prototype.name).join())", "T(()=>[TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError].map(c=>S(c.prototype.message)).join())",
  "T(()=>[Error,TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError,AggregateError].map(c=>c.length).join())",
  "T(()=>[Error,TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError,AggregateError].map(c=>Object.getOwnPropertyNames(c.prototype).join('+')).join())",
  "T(()=>Error.prototype.constructor===Error)", "T(()=>Function.prototype.toString.call(Error).replace(/\\s+/g,' '))", "T(()=>Function.prototype.toString.call(AggregateError).replace(/\\s+/g,' '))",
  "T(()=>Function.prototype.toString.call(Error.prototype.toString).replace(/\\s+/g,' '))", "T(()=>new Error('a') instanceof TypeError)", "T(()=>new TypeError('a') instanceof Error)",
  "T(()=>Error('a') instanceof Error)", "T(()=>Error.call({},'a') instanceof Error)", "T(()=>Error.apply(null,['a','b']).message)", "T(()=>Reflect.ownKeys(Error('m')).filter(k=>!X[k]).map(String).join())",
  "T(()=>Error.prototype.isPrototypeOf(new RangeError))", "T(()=>RangeError.prototype.isPrototypeOf(new Error))", "T(()=>Object.getPrototypeOf(Object.getPrototypeOf(new URIError))===Error.prototype)",
);

// ---- 8. Error.isError, quando existe no bun, sobre valores variados.
const IS_ERROR_VALUES = [
  "new Error", "new TypeError", "new AggregateError([])", "Error.prototype", "TypeError.prototype", "Object.create(Error.prototype)", "{name:'Error',message:''}", "undefined", "null", "1", "'Error'",
  "Symbol()", "[]", "{}", "function(){}", "class extends Error{}", "new (class extends Error{})", "new Proxy(new Error,{})", "new Proxy({},{})", "(()=>{var r=Proxy.revocable(new Error,{});r.revoke();return r.proxy})()",
  "Object.setPrototypeOf(new Error,null)", "Object.setPrototypeOf({},Error.prototype)", "Reflect.construct(Error,[],Object)", "Reflect.construct(Object,[],Error)", "new DOMException ? 1 : 0",
];
add(
  "T(()=>typeof Error.isError)", "T(()=>typeof Error.isError==='function'?Error.isError.length+Error.isError.name:'absent')", "T(()=>typeof Error.isError==='function'?D(Error,'isError'):'absent')",
  "T(()=>typeof Error.isError==='function'?String(Error.isError()):'absent')",
);
for (const v of IS_ERROR_VALUES) add(`T(()=>typeof Error.isError==='function'?String(Error.isError(${v})):'absent')`);

// ---- 9. cause: interação de has/get e ordem relativa à message.
add(
  "T(()=>{var e=new Error('m',{cause:Symbol.for('s')});return S(e.cause)})", "T(()=>{var e=new Error('m',{cause:new Error('c')});return S(e.cause)+(e.cause instanceof Error)})",
  "T(()=>{var o={cause:1};var e=new Error('m',o);o.cause=2;return S(e.cause)})", "T(()=>{var e=new Error('m',{cause:1});e.cause=2;return S(e.cause)+D(e,'cause')})",
  "T(()=>{var e=new Error('m',{cause:1});delete e.cause;return Object.hasOwn(e,'cause')})", "T(()=>{var e=new Error('m',{cause:1});return Object.getOwnPropertyNames(e).filter(k=>!X[k]).join()})",
  "T(()=>{var e=new Error('m',{cause:1});return Reflect.ownKeys(e).filter(k=>!X[k]).map(String).join()})", "T(()=>{var e=new Error(undefined,{cause:1});return Reflect.ownKeys(e).filter(k=>!X[k]).map(String).join()})",
  "T(()=>{var e=new Error('m',{cause:1});return JSON.stringify(Object.getOwnPropertyDescriptors(Object.fromEntries(Object.entries(Object.getOwnPropertyDescriptors(e)).filter(([k])=>!X[k]))))})",
  "T(()=>{var log=[];var o=new Proxy({cause:1},{has(t,k){log.push('has');return true},get(t,k){log.push('get');return 1}});new Error({toString(){log.push('msg');return 'x'}},o);return log.join()})",
  "T(()=>{var log=[];new Error({toString(){log.push('msg');return 'x'}},{get cause(){log.push('cause');return 1}});return log.join()})",
  "T(()=>{var log=[];try{new Error({toString(){log.push('msg');throw 1}},{get cause(){log.push('cause');return 1}})}catch(e){log.push('caught')}return log.join()})",
  "T(()=>{var log=[];new AggregateError({[Symbol.iterator](){log.push('iter');return [][Symbol.iterator]()}},{toString(){log.push('msg');return 'x'}},{get cause(){log.push('cause');return 1}});return log.join()})",
  "T(()=>{var log=[];try{new AggregateError({[Symbol.iterator](){log.push('iter');throw 1}},{toString(){log.push('msg');return 'x'}},{get cause(){log.push('cause');return 1}})}catch(e){log.push('caught')}return log.join()})",
  "T(()=>{var log=[];try{new AggregateError([],{toString(){log.push('msg');throw 1}},{get cause(){log.push('cause');return 1}})}catch(e){log.push('caught')}return log.join()})",
  "T(()=>{var e=new Error('m',{cause:undefined});return S(e.cause)+Object.hasOwn(e,'cause')+('cause' in e)})", "T(()=>{var e=new Error('m',Object.create({cause:'p'}));return S(e.cause)+Object.hasOwn(e,'cause')})",
  "T(()=>{var e=new Error('m',{get cause(){return this===undefined}});return S(e.cause)})", "T(()=>{var o={get cause(){'use strict';return this}};var e=new Error('m',o);return e.cause===o})",
);

// ---- Execução.
const baseSources = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  // O próprio golden nunca conta como repetido (senão a regeneração o esvazia).
  if (file === "error_ctor_bun.tsv" || !/^(error|stack|errors|function_error)[a-z_]*\.tsv$/.test(file)) continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) { baseSources.push(line.split("\t")[0]); }
  }
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}` });
}

// Processo fresco por programa (a ordem de reificação das tabelas estáticas depende do que rodou antes), no máximo 8
// em paralelo, cada um com timeout de 5 s.
const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    let done = false;
    const finish = (value) => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => { child.kill("SIGKILL"); finish({ error: "timeout" }); }, 5000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("error", (e) => finish({ error: String(e) }));
    child.on("close", (code) => {
      const decoded = code === 0 ? decodeResult(out) : null;
      finish(decoded !== null ? { out: decoded } : { error: err || (code === 0 ? "sem resultado" : "status " + code) });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i].source);
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  jobs.forEach((job, i) => {
    const r = results[i];
    if (r.error) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + " " + r.error.slice(0, 120) + "\n"); return; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || r.out.includes("\u2014") || r.out.includes("\u2013")) {
      dropped++;
      process.stderr.write("caminho, marca ou travessão no resultado: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(JSON.stringify(job.source) + "\t" + JSON.stringify(r.out));
  });
  process.stdout.write(emitFactoredLines("error_ctor", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
