// Gera tests/golden/error_edge_bun.tsv: borda da família Error medida no bun 1.4.2, no padrão de
// gen-object-edge-golden.js (cada programa roda num bun filho novo, sem APIs de host, resultado em `globalThis.R`).
// Cobre o que error_api, error_stack, errors, stack_format e dispose ainda não cobrem: AggregateError (errors
// iterável, cause, ordem de acesso e de chaves), `cause` em todos os construtores nativos, SuppressedError e as pilhas
// descartáveis, Error.captureStackTrace com constructorOpt, Error.stackTraceLimit, Error.prepareStackTrace com
// CallSite, e as mensagens de TypeError/RangeError/SyntaxError de APIs comuns com o sufixo `(evaluating ...)`.
// Programas cuja expressão já aparece nos goldens de erro, pilha e dispose são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). O arquivo do script é `x.js`.
// Uso: bun scripts/gen-error-edge-golden.js > tests/golden/error_edge_bun.tsv
const { emitFactoredLines } = require("./golden-prelude.js");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'if(v instanceof Error)return "E("+v.name+":"+v.message+")";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function K(o){return Reflect.ownKeys(o).filter(k=>!["originalLine","originalColumn","line","column","sourceURL"].includes(k))}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function M(f){try{f();return "no throw"}catch(e){return (e&&e.constructor&&e.constructor.name)+": "+(e&&e.message)}}\n' +
  'function N(s){return String(s).replace(/x\\.js:\\d+:\\d+/g,"x.js:L:C")}\n' +
  'function H(s){return N(s).split("\\n")[0]}\n' +
  // F: nomes dos frames do script (e dos nativos entre eles), cortando o que vem depois do último frame x.js.
  'function F(make){var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s};var s;try{s=make().stack}finally{Error.prepareStackTrace=o}' +
  'if(!Array.isArray(s))return "stack="+typeof s;var last=-1;s.forEach(function(c,i){if(c.getFileName()==="x.js")last=i});' +
  'return s.slice(0,last+1).map(function(c){return (c.isConstructor()?"new ":"")+(c.isNative()?"native ":"")+(c.getFunctionName()||"<anon>")}).join(">")}\n' +
  // P: roda fn com prepareStackTrace = hook e devolve o que o hook devolver para a pilha do erro criado por make.
  'function P(hook,make){var o=Error.prepareStackTrace;Error.prepareStackTrace=hook;try{return make().stack}finally{Error.prepareStackTrace=o}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. AggregateError: o iterável de errors.
const iterables = [
  "[]", "[1,2]", "[1,,3]", "new Set([1,1,2])", "new Map([[1,2],[3,4]])", "'abc'", "'\\ud83d\\ude00x'", "(function*(){yield 1;yield 2})()",
  "(function*(){yield 1;throw new RangeError('g')})()", "{length:2,0:'a',1:'b'}", "5", "null", "undefined", "true", "Symbol()", "{}", "1n",
  "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator](){return {next(){return 1}}}}",
  "{[Symbol.iterator](){return {next(){throw new EvalError('n')}}}}", "{[Symbol.iterator](){throw new URIError('i')}}",
  "new Uint8Array([7,8])", "(function(){return arguments})(1,2)", "Object.create(Array.prototype)", "new (class extends Array{})(3)",
  "[undefined,null,NaN]", "[[1],[2]]", "[new Error('a'),new TypeError('b')]", "new Proxy([1,2],{})", "new Proxy({},{})", "Object('s')",
  "{[Symbol.iterator]:null}", "{[Symbol.iterator]:undefined}", "new Set()", "'' ", "Array.from({length:3},(_, i)=>i)",
  "{[Symbol.iterator](){var i=0;return {next(){return i++<2?{value:i,done:false}:{done:true}},return(){return 7}}}}",
  "{[Symbol.iterator](){var i=0;return {next(){return {value:i++,done:i>3}}}}}",
];
for (const it of iterables) {
  add(`T(()=>{var e=new AggregateError(${it});return S(e.errors)+' '+Array.isArray(e.errors)+' '+D(e,'errors')})`);
  add(`T(()=>{var e=AggregateError(${it},'m');return S(e.errors)+' '+e.message+' '+S(K(e))})`);
  add(`M(()=>new AggregateError(${it},{toString(){throw new SyntaxError('msg')}}))`);
  add(`T(()=>{var e=new AggregateError(${it},undefined,{cause:3});return S(K(e))+' '+e.cause+' '+e.message+'|'})`);
}
add(
  "T(()=>{var log=[];var o=new Proxy({},{has(t,k){log.push('has '+String(k));return false},get(t,k){log.push('get '+String(k))}});new AggregateError([],{toString(){log.push('msg');return 'm'}},o);return log.join()})",
  "T(()=>{var log=[];new AggregateError({[Symbol.iterator](){log.push('iter');return [][Symbol.iterator]()}},{toString(){log.push('msg');return 'm'}},{get cause(){log.push('cause');return 1}});return log.join()})",
  "T(()=>{var log=[];try{new AggregateError({[Symbol.iterator](){log.push('iter');throw 1}},{toString(){log.push('msg');return 'm'}},{get cause(){log.push('cause');return 1}})}catch(e){log.push('caught '+e)}return log.join()})",
  "T(()=>{var log=[];try{new AggregateError([],{toString(){log.push('msg');throw 2}},{get cause(){log.push('cause');return 1}})}catch(e){log.push('caught '+e)}return log.join()})",
  "T(()=>{var a=[1,2];var e=new AggregateError(a);a.push(3);return S(e.errors)+' '+(e.errors===a)})",
  "T(()=>{var e=new AggregateError([1]);e.errors.push(2);return S(e.errors)})",
  "T(()=>{var e=new AggregateError([1]);e.errors=5;return S(e.errors)+' '+D(e,'errors')})",
  "T(()=>S(K(new AggregateError([],'m',{cause:1}))))", "T(()=>S(K(new AggregateError([]))))", "T(()=>S(K(new AggregateError([],''))))",
  "T(()=>S(K(new AggregateError([],undefined,{cause:undefined}))))", "T(()=>S(K(new AggregateError([],undefined,{}))))",
  "T(()=>String(new AggregateError([1,2],'boom')))", "T(()=>String(new AggregateError([1,2])))", "T(()=>String(new AggregateError([],'')))",
  "T(()=>AggregateError.length+AggregateError.name+Object.getPrototypeOf(AggregateError).name)", "T(()=>S(K(AggregateError)))", "T(()=>S(K(AggregateError.prototype)))",
  "T(()=>D(AggregateError.prototype,'name')+'|'+D(AggregateError.prototype,'message')+'|'+D(AggregateError.prototype,'constructor'))",
  "T(()=>D(AggregateError,'prototype')+'|'+D(AggregateError,'length')+'|'+D(AggregateError,'name'))",
  "T(()=>Object.prototype.toString.call(new AggregateError([])))", "T(()=>(new AggregateError([]) instanceof Error)+' '+(new AggregateError([]) instanceof AggregateError))",
  "T(()=>{class K extends AggregateError{};var e=new K([1],'m',{cause:2});return e.name+' '+e.constructor.name+' '+S(e.errors)+' '+e.cause+' '+String(e)})",
  "T(()=>{class K extends AggregateError{constructor(){super([9])}};return S(new K().errors)})", "T(()=>{class K extends AggregateError{get errors(){return 7}};return S(new K([1]).errors)})",
  "T(()=>{var e=new AggregateError([]);e.name='Custom';return String(e)+' '+H(e.stack)})", "T(()=>{var e=new AggregateError([],'zz');return H(e.stack)})",
  "T(()=>{var e=new AggregateError([],'a\\nb');return S(H(e.stack))+S(N(e.stack).split('\\n').length>2)})",
  "T(()=>Reflect.construct(AggregateError,[[1],'m'],Object).constructor===Object)", "T(()=>Object.getPrototypeOf(Reflect.construct(AggregateError,[[1]],Array))===Array.prototype)",
  "T(()=>{function F(){};F.prototype=null;var e=Reflect.construct(AggregateError,[[1]],F);return Object.getPrototypeOf(e)===AggregateError.prototype})",
  "T(()=>Promise.any.length+Promise.any.name)", "T(()=>{var r;Promise.any([]).catch(e=>{R=S(e.errors)+' '+e.name+' '+e.message+' '+(e instanceof AggregateError)});return 'pending'})",
  "T(()=>{Promise.any([Promise.reject(1),Promise.reject(2)]).catch(e=>{R=S(e.errors)+' '+e.message+' '+S(K(e))});return 'pending'})",
  "T(()=>{Promise.any([Promise.reject(2),Promise.reject(1),new Promise(function(_,j){j(3)})]).catch(e=>{R=S(e.errors)});return 'pending'})",
  "T(()=>{Promise.any(5).catch(e=>{R=e.name+': '+e.message});return 'pending'})", "T(()=>{Promise.any().catch(e=>{R=e.name+': '+e.message});return 'pending'})",
  "T(()=>{Promise.any([1,Promise.reject(1)]).then(v=>{R='v'+v});return 'pending'})",
  "T(()=>{var e=new AggregateError([new AggregateError([1],'in')],'out');return S(e.errors[0].errors)+e.errors[0].message+e.message})",
  "T(()=>{var e=new AggregateError([],'m',{cause:new AggregateError([2])});return S(e.cause.errors)})",
  "T(()=>{var e=new AggregateError([]);return K(e).join()})", "T(()=>{var e=new AggregateError([],'m',{cause:0});return K(e).join()})",
  "T(()=>JSON.stringify(new AggregateError([1],'m',{cause:1})))", "T(()=>K(new AggregateError([1],'m',{cause:1})).join())",
  "T(()=>{var e=new AggregateError([1],'m');return structuredCloneProbe})",
);

// ---- 2. `cause` em todos os construtores nativos, chamando com e sem new.
const ctors = ["Error", "TypeError", "RangeError", "EvalError", "URIError", "ReferenceError", "SyntaxError"];
const causeOptions = [
  "{cause:1}", "{cause:undefined}", "{}", "{cause:null}", "{cause:0,extra:1}", "{get cause(){return 'g'}}", "{get cause(){throw new RangeError('boom')}}",
  "null", "undefined", "1", "'str'", "true", "[]", "Object.create({cause:'inh'})", "new Proxy({cause:'p'},{})", "new Proxy({},{has(){return true},get(){return 'px'}})",
  "Object.defineProperty({},'cause',{value:5,enumerable:false})", "{cause:new Error('inner')}", "{cause:Symbol.iterator}", "{cause:{cause:{}}}", "function(){}",
  "Object.assign(()=>1,{cause:'fn'})", "new String('s')", "{CAUSE:1}", "{'cause ':1}",
];
for (const c of ctors) {
  for (const o of causeOptions) {
    add(`T(()=>{var e=new ${c}('m',${o});return D(e,'cause')+' '+S(K(e))+' '+e.message})`);
    add(`T(()=>{var e=${c}(undefined,${o});return D(e,'cause')+' '+S(K(e))+' '+Object.hasOwn(e,'message')})`);
  }
  add(`T(()=>${c}.length+' '+${c}.name+' '+D(${c}.prototype,'message')+' '+S(K(${c}.prototype)))`);
  add(`T(()=>{var e=new ${c}('m',{cause:1});e.cause=2;return e.cause+' '+D(e,'cause')})`);
  add(`T(()=>{var e=new ${c}('m',{cause:1});return H(e.stack)+'|'+String(e)})`);
  add(`T(()=>{class K extends ${c}{};var e=new K('m',{cause:'k'});return e.cause+' '+e.name+' '+String(e)+' '+(e instanceof ${c})})`);
  add(`T(()=>{var e=new ${c}({toString(){return 'ts'}},{cause:1});return e.message+' '+D(e,'message')})`);
  add(`T(()=>{var e=new ${c}(Symbol('s'),{cause:1})})`);
  add(`T(()=>{var e=new ${c}(1n,{cause:1});return typeof e.message+e.message})`);
  add(`T(()=>{var e=new ${c}('m',{cause:1},'extra');return S(K(e))})`);
  add(`T(()=>Reflect.construct(${c},['m',{cause:1}],Object).cause)`);
  add(`T(()=>{var log=[];new ${c}('m',new Proxy({cause:1},{has(t,k){log.push('has '+String(k));return k in t},get(t,k){log.push('get '+String(k));return t[k]},getOwnPropertyDescriptor(t,k){log.push('gopd');return Reflect.getOwnPropertyDescriptor(t,k)}}));return log.join()})`);
}
add(
  "T(()=>{var log=[];new Error({toString(){log.push('msg');return 'm'}},{get cause(){log.push('cause');return 1}});return log.join()})",
  "T(()=>{var log=[];try{new Error({toString(){log.push('msg');throw 1}},{get cause(){log.push('cause');return 1}})}catch(e){}return log.join()})",
  "T(()=>S(K(new Error('m',{cause:1}))))", "T(()=>S(K(new Error(undefined,{cause:1}))))",
  "T(()=>Error('m',{cause:1}).cause)", "T(()=>Error.prototype.hasOwnProperty('cause'))", "T(()=>'cause' in new Error('m'))", "T(()=>'cause' in new Error('m',{cause:undefined}))",
  "T(()=>{var e=new Error('a',{cause:new Error('b',{cause:new Error('c')})});return e.cause.cause.message})",
  "T(()=>{var e=new Error('a');e.cause=e;return String(e)+(e.cause===e)})",
  "T(()=>{var o={};o.cause=o;return String(new Error('x',o).cause===o)})",
);

// ---- 3. SuppressedError e as pilhas descartáveis.
const sargs = ["", "1", "1,2", "1,2,'m'", "undefined,undefined,undefined", "new Error('a'),new Error('b')", "null,null,null", "1,2,{toString(){return 'ts'}}", "1,2,Symbol()", "1,2,undefined,3",
  "1,2,'m',{cause:1}", "{},{}", "'e','s',''"];
for (const a of sargs) {
  add(`T(()=>{var e=new SuppressedError(${a});return S(K(e))+' '+S(e.error)+' '+S(e.suppressed)+' '+JSON.stringify(e.message)+' '+Object.hasOwn(e,'message')})`);
  add(`T(()=>{var e=SuppressedError(${a});return D(e,'error')+' '+D(e,'suppressed')+' '+String(e)})`);
}
add(
  "T(()=>SuppressedError.length+SuppressedError.name+(Object.getPrototypeOf(SuppressedError)===Error))", "T(()=>S(K(SuppressedError)))", "T(()=>S(K(SuppressedError.prototype)))",
  "T(()=>D(SuppressedError.prototype,'name')+'|'+D(SuppressedError.prototype,'message'))", "T(()=>(new SuppressedError(1,2) instanceof Error))",
  "T(()=>Object.prototype.toString.call(new SuppressedError(1,2)))", "T(()=>H(new SuppressedError(1,2,'mm').stack))", "T(()=>H(new SuppressedError(1,2).stack))",
  "T(()=>{class K extends SuppressedError{};var e=new K(1,2,'m');return e.name+e.constructor.name+e.error+e.suppressed})",
  "T(()=>{var log=[];new SuppressedError(1,2,{toString(){log.push('msg');return 'm'}});return log.join()})",
  "T(()=>JSON.stringify(new SuppressedError(1,2,'m')))", "T(()=>Object.keys(new SuppressedError(1,2,'m')).join())",
  "T(()=>typeof Symbol.dispose+typeof Symbol.asyncDispose+String(Symbol.dispose)+String(Symbol.asyncDispose))", "T(()=>D(Symbol,'dispose')+'|'+D(Symbol,'asyncDispose'))",
  "T(()=>DisposableStack.length+DisposableStack.name+S(K(DisposableStack.prototype)))", "T(()=>AsyncDisposableStack.length+AsyncDisposableStack.name+S(K(AsyncDisposableStack.prototype)))",
  "T(()=>M(()=>DisposableStack()))", "T(()=>M(()=>AsyncDisposableStack()))", "T(()=>M(()=>new DisposableStack(1)))",
  "T(()=>{var s=new DisposableStack();return s.disposed+' '+Object.prototype.toString.call(s)+' '+(s[Symbol.dispose]===s.dispose)})",
  "T(()=>{var s=new AsyncDisposableStack();return s.disposed+' '+Object.prototype.toString.call(s)+' '+(s[Symbol.asyncDispose]===s.disposeAsync)})",
  "T(()=>{var log=[];var s=new DisposableStack();s.defer(()=>log.push('a'));s.defer(()=>log.push('b'));s.dispose();return log.join()+s.disposed})",
  "T(()=>{var log=[];var s=new DisposableStack();s.dispose();s.dispose();return s.disposed})",
  "T(()=>M(()=>{var s=new DisposableStack();s.dispose();s.defer(()=>1)}))", "T(()=>M(()=>{var s=new DisposableStack();s.dispose();s.use({[Symbol.dispose](){}})}))",
  "T(()=>M(()=>{var s=new DisposableStack();s.dispose();s.adopt(1,()=>1)}))", "T(()=>M(()=>{var s=new DisposableStack();s.dispose();s.move()}))",
  "T(()=>M(()=>new DisposableStack().use(1)))", "T(()=>M(()=>new DisposableStack().use({})))", "T(()=>M(()=>new DisposableStack().use({[Symbol.dispose]:1})))",
  "T(()=>new DisposableStack().use(null)===null)", "T(()=>new DisposableStack().use(undefined)===undefined)", "T(()=>{var o={[Symbol.dispose](){}};return new DisposableStack().use(o)===o})",
  "T(()=>M(()=>new DisposableStack().defer(1)))", "T(()=>M(()=>new DisposableStack().defer()))", "T(()=>new DisposableStack().defer(()=>1))",
  "T(()=>M(()=>new DisposableStack().adopt(1,2)))", "T(()=>new DisposableStack().adopt(5,()=>1))", "T(()=>{var log=[];var s=new DisposableStack();s.adopt('v',v=>log.push(v));s.dispose();return log.join()})",
  "T(()=>{var s=new DisposableStack();s.defer(()=>1);var m=s.move();return s.disposed+' '+m.disposed+' '+(m instanceof DisposableStack)})",
  "T(()=>{var log=[];var s=new DisposableStack();s.defer(()=>{throw 'a'});s.defer(()=>{throw 'b'});try{s.dispose()}catch(e){return e.name+' '+e.error+' '+e.suppressed+' '+(e instanceof SuppressedError)+' '+JSON.stringify(e.message)}})",
  "T(()=>{var s=new DisposableStack();s.defer(()=>{throw 'a'});s.defer(()=>{throw 'b'});s.defer(()=>{throw 'c'});try{s.dispose()}catch(e){return S(e.error)+' '+S(e.suppressed.error)+' '+S(e.suppressed.suppressed)}})",
  "T(()=>{var s=new DisposableStack();s.defer(()=>{throw 'a'});s.defer(()=>1);try{s.dispose()}catch(e){return typeof e+e}})",
  "T(()=>{var s=new DisposableStack();s.defer(()=>1);s.defer(()=>{throw 'a'});try{s.dispose()}catch(e){return typeof e+e}})",
  "T(()=>{var s=new DisposableStack();s.defer(()=>{throw new RangeError('r')});try{s.dispose()}catch(e){return e.name+e.message+s.disposed}})",
  "T(()=>{var r=[];var s=new DisposableStack();s.use({[Symbol.dispose](){r.push(this===undefined?'u':typeof this)}});s.dispose();return r.join()})",
  "T(()=>{var r=[];var s=new DisposableStack();s.defer(function(){r.push(this===undefined?'u':typeof this)});s.dispose();return r.join()})",
  "T(()=>{var r=[];var s=new DisposableStack();s.defer(function(){r.push(arguments.length)});s.dispose();return r.join()})",
  "T(()=>{var r=[];var s=new DisposableStack();s.adopt(1,function(){r.push(arguments.length)});s.dispose();return r.join()})",
  "T(()=>{var s=new DisposableStack();var d=Object.getOwnPropertyDescriptor(DisposableStack.prototype,'disposed');return typeof d.get+typeof d.set+d.enumerable+d.configurable})",
  "T(()=>M(()=>Object.getOwnPropertyDescriptor(DisposableStack.prototype,'disposed').get.call({})))", "T(()=>M(()=>DisposableStack.prototype.dispose.call({})))",
  "T(()=>M(()=>DisposableStack.prototype.use.call(new AsyncDisposableStack(),null)))", "T(()=>M(()=>AsyncDisposableStack.prototype.disposeAsync.call({})))",
  "T(()=>{var p=new AsyncDisposableStack().disposeAsync();return Object.prototype.toString.call(p)})",
  "T(()=>{var p=AsyncDisposableStack.prototype.disposeAsync.call({});return Object.prototype.toString.call(p)})",
  "T(()=>{var log=[];var s=new AsyncDisposableStack();s.defer(async()=>{log.push('a')});s.defer(()=>{log.push('b')});s.disposeAsync().then(()=>{R=log.join()});return 'pending'})",
  "T(()=>{var s=new AsyncDisposableStack();s.defer(()=>{throw 'a'});s.defer(()=>{throw 'b'});s.disposeAsync().catch(e=>{R=e.name+e.error+e.suppressed});return 'pending'})",
  "T(()=>{var s=new AsyncDisposableStack();s.use({[Symbol.dispose](){R='sync'}});s.disposeAsync();return 'pending'})",
  "T(()=>{var s=new AsyncDisposableStack();s.use({[Symbol.asyncDispose](){R='async'}});s.disposeAsync();return 'pending'})",
  "T(()=>M(()=>new AsyncDisposableStack().use({})))", "T(()=>M(()=>new AsyncDisposableStack().use(1)))", "T(()=>new AsyncDisposableStack().use(null)===null)",
  "T(()=>{var s=new AsyncDisposableStack();s.defer(()=>1);var m=s.move();return s.disposed+' '+m.disposed})",
  "T(()=>{var s=new AsyncDisposableStack();var p1=s.disposeAsync();var p2=s.disposeAsync();return (p1===p2)+' '+s.disposed})",
  "T(()=>{var s=new AsyncDisposableStack();s.disposeAsync();return M(()=>s.defer(()=>1))})",
  "T(()=>S(K(DisposableStack.prototype).map(String)))",
);

// ---- 4. Error.captureStackTrace.
const targets = ["{}", "[]", "function(){}", "class{}", "Object.create(null)", "Object.freeze({})", "Object.seal({})", "Object.preventExtensions({})", "new Map", "new Proxy({},{})",
  "{stack:'old'}", "{message:'mm'}", "{name:'NN',message:'mm'}", "{name:'NN'}", "Object.create(Error.prototype)", "new Error('own')", "Object.defineProperty({},'stack',{value:1,configurable:true})",
  "Object.defineProperty({},'stack',{value:1})", "Object.defineProperty({},'stack',{get(){return 'g'},configurable:true})", "{toString(){return 'ts'}}", "{message:{toString(){return 'tm'}}}",
  "Object.assign(Object.create(Error.prototype),{name:'Q',message:'z'})", "{name:'',message:''}", "{name:'',message:'only'}", "{name:'only',message:''}", "new (class Foo extends Error{})('c')"];
for (const t of targets) {
  add(`T(()=>{var o=${t};Error.captureStackTrace(o);return D(o,'stack').slice(0,2)+' '+typeof o.stack+' '+Object.hasOwn(o,'stack')+' '+(typeof o.stack==='string'?H(o.stack):'')})`);
  add(`T(()=>{var o=${t};Error.captureStackTrace(o);return S(K(o))+' '+Object.getOwnPropertyDescriptor(o,'stack').writable+Object.getOwnPropertyDescriptor(o,'stack').enumerable+Object.getOwnPropertyDescriptor(o,'stack').configurable})`);
}
const prims = ["undefined", "null", "1", "'s'", "true", "Symbol()", "1n"];
for (const p of prims) add(`M(()=>Error.captureStackTrace(${p}))`, `M(()=>Error.captureStackTrace({},${p}))`);
add(
  "M(()=>Error.captureStackTrace())", "T(()=>Error.captureStackTrace({})===undefined)", "T(()=>Error.captureStackTrace.length+Error.captureStackTrace.name+D(Error,'captureStackTrace'))",
  "T(()=>D(Error,'stackTraceLimit'))", "T(()=>S(K(Error)))", "T(()=>[Error,TypeError,RangeError,AggregateError,SuppressedError].map(c=>Object.hasOwn(c,'captureStackTrace')+':'+Object.hasOwn(c,'stackTraceLimit')).join())",
  "T(()=>TypeError.captureStackTrace===Error.captureStackTrace)", "T(()=>{TypeError.stackTraceLimit=1;var r=Error.stackTraceLimit+' '+Object.hasOwn(TypeError,'stackTraceLimit');delete TypeError.stackTraceLimit;return r})",
  "T(()=>{var o={};TypeError.captureStackTrace(o);return typeof o.stack})", "T(()=>{var f=Error.captureStackTrace;var o={};f(o);return typeof o.stack})",
  "T(()=>{var o={};Error.captureStackTrace.call(null,o);return typeof o.stack})", "T(()=>{var o={};Error.captureStackTrace(o,o);return typeof o.stack})",
  "T(()=>{var o={};Error.captureStackTrace(o,1);return typeof o.stack})", "T(()=>{var o={};Error.captureStackTrace(o,{});return typeof o.stack})",
);
// constructorOpt: quem some da pilha. A cadeia a>b>c>d cria o erro em d.
const opts = ["undefined", "null", "a", "b", "c", "d", "Math.max", "Array.prototype.map", "function nope(){}", "a.bind(null)", "class Z{}", "()=>1", "Error", "Object"];
for (const o of opts) {
  add(`T(()=>{function d(){var t={};Error.captureStackTrace(t,${o});return t}function c(){return d()}function b(){return c()}function a(){return b()}return F(()=>a())})`);
  add(`T(()=>{function a(){return b()}function b(){return c()}function c(){return d()}function d(){var t={};Error.captureStackTrace(t,${o});return t}return F(()=>a())})`);
}
add(
  "T(()=>{function mk(){var t={};Error.captureStackTrace(t,mk);return t}function w(){return mk()}return F(()=>w())})",
  "T(()=>{function mk(){var t={};Error.captureStackTrace(t,mk);return t}function w(){return mk()}return F(()=>[1].map(function inner(){return w()})[0])})",
  "T(()=>{function mk(){var t={};Error.captureStackTrace(t,Array.prototype.map);return t}return F(()=>[1].map(function inner(){return mk()})[0])})",
  "T(()=>{function mk(){var t={};Error.captureStackTrace(t,inner);return t}function inner(){return mk()}return F(()=>[1].map(inner)[0])})",
  "T(()=>{class K{constructor(){Error.captureStackTrace(this,K)}}function w(){return new K}return F(()=>w())})",
  "T(()=>{class K{constructor(){Error.captureStackTrace(this)}}function w(){return new K}return F(()=>w())})",
  "T(()=>{class K extends Error{constructor(){super();Error.captureStackTrace(this,K)}}function w(){return new K}return F(()=>w())})",
  "T(()=>{class K extends Error{constructor(){super('x');Error.captureStackTrace(this,new.target)}}class L extends K{}function w(){return new L}return F(()=>w())})",
  "T(()=>{function E(m){this.message=m;Error.captureStackTrace(this,E)}function w(){return new E('q')}return F(()=>w())+' '+H(w().stack)})",
  "T(()=>{function E(m){this.message=m;Error.captureStackTrace(this)}function w(){return new E('q')}return F(()=>w())})",
  "T(()=>{var e=new Error('first');var s1=e.stack;Error.captureStackTrace(e);return (s1===e.stack)+' '+H(e.stack)})",
  "T(()=>{var e=new Error('first');e.message='second';Error.captureStackTrace(e);return H(e.stack)})",
  "T(()=>{var e=new Error('first');e.message='second';return H(e.stack)})", "T(()=>{var e=new Error('first');e.name='Nm';return H(e.stack)})",
  "T(()=>{var o={name:'A',message:'b'};Error.captureStackTrace(o);o.message='changed';return H(o.stack)})",
  "T(()=>{var o={name:'A',message:'b'};Error.captureStackTrace(o);return H(o.stack)+'|'+N(o.stack).split('\\n').length})",
  "T(()=>{var o={};Error.captureStackTrace(o);return H(o.stack)})", "T(()=>{var o=[];Error.captureStackTrace(o);return H(o.stack)+o.length})",
  "T(()=>{var o=function nm(){};Error.captureStackTrace(o);return H(o.stack)})", "T(()=>{var o=Object.create(null);Error.captureStackTrace(o);return H(o.stack)})",
  "T(()=>{var o={name:{toString(){return 'ntm'}},message:'k'};Error.captureStackTrace(o);return H(o.stack)})",
  "T(()=>{var o={name:{toString(){throw new RangeError('nm')}},message:'k'};Error.captureStackTrace(o);return H(o.stack)})",
  "T(()=>{var o={get message(){throw new RangeError('gm')}};Error.captureStackTrace(o);return typeof o.stack})",
  "T(()=>{var o=new Proxy({},{defineProperty(){return false}});return M(()=>Error.captureStackTrace(o))})",
  "T(()=>{var log=[];var o=new Proxy({},{defineProperty(t,k,d){log.push('dp '+String(k));return Reflect.defineProperty(t,k,d)},get(t,k){log.push('get '+String(k));return t[k]},set(t,k,v){log.push('set '+String(k));t[k]=v;return true},has(t,k){log.push('has '+String(k));return k in t}});Error.captureStackTrace(o);return log.join()})",
  "T(()=>{var o=Object.freeze({});return M(()=>Error.captureStackTrace(o))})", "T(()=>{'use strict';var o=Object.freeze({});return M(()=>Error.captureStackTrace(o))})",
  "T(()=>{var o={};Object.defineProperty(o,'stack',{value:1,writable:false,configurable:false});return M(()=>Error.captureStackTrace(o))})",
  "T(()=>{var o={stack:'s'};Error.captureStackTrace(o);return typeof o.stack+Object.keys(o).join()})",
);

// ---- 5. Error.stackTraceLimit.
const limits = ["0", "1", "2", "3", "5", "6", "'2'", "2.9", "-1", "-0", "NaN", "Infinity", "-Infinity", "null", "undefined", "true", "false", "{valueOf(){return 2}}", "1e10", "2**32", "'abc'", "[]", "[3]", "Symbol", "1n"];
for (const l of limits) {
  add(`T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=${l};function c(){return new Error('e')}function b(){return c()}function a(){return b()}return F(()=>a())+'|'+(typeof a().stack==='string'?N(a().stack).split('\\n').length:'ns')}catch(e){return 'throw '+e.name}finally{Error.stackTraceLimit=old}})`);
  add(`T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=${l};var o={};function c(){Error.captureStackTrace(o);return o}function b(){return c()}return F(()=>b())}catch(e){return 'throw '+e.name}finally{Error.stackTraceLimit=old}})`);
  add(`T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=${l};return D(Error,'stackTraceLimit')}finally{Error.stackTraceLimit=old}})`);
  add(`T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=${l};function r(n){if(n===0)return new TypeError('deep');return r(n-1)}return F(()=>r(4))}finally{Error.stackTraceLimit=old}})`);
}
add(
  "T(()=>{var old=Error.stackTraceLimit;delete Error.stackTraceLimit;try{function c(){return new Error('e')}return typeof Error.stackTraceLimit+' '+F(()=>c())+' '+(typeof c().stack)+' '+Object.hasOwn(Error,'stackTraceLimit')}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;delete Error.stackTraceLimit;try{var o={};Error.captureStackTrace(o);return typeof o.stack}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;Object.defineProperty(Error,'stackTraceLimit',{get(){return 1}});try{function c(){return new Error('e')}function b(){return c()}return F(()=>b())}finally{Object.defineProperty(Error,'stackTraceLimit',{value:old,writable:true,configurable:true,enumerable:true})}})",
  "T(()=>{var old=Error.stackTraceLimit;Object.defineProperty(Error,'stackTraceLimit',{get(){throw new RangeError('lim')},configurable:true});try{return M(()=>new Error('e'))}finally{Object.defineProperty(Error,'stackTraceLimit',{value:old,writable:true,configurable:true,enumerable:true})}})",
  "T(()=>{var old=Error.stackTraceLimit;Object.defineProperty(Error,'stackTraceLimit',{value:1,writable:false,configurable:true});try{Error.stackTraceLimit=9;function c(){return new Error('e')}function b(){return c()}return Error.stackTraceLimit+' '+F(()=>b())}finally{Object.defineProperty(Error,'stackTraceLimit',{value:old,writable:true,configurable:true,enumerable:true})}})",
  "T(()=>{var old=Error.stackTraceLimit;try{function c(){return new TypeError('e')}function b(){return c()}TypeError.stackTraceLimit=1;var r=F(()=>b());delete TypeError.stackTraceLimit;return r}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{function c(){return new Error('e')}function b(){return c()}var e=b();Error.stackTraceLimit=1;var r=F(()=>b());return r}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=0;var e=new Error('m');return JSON.stringify(e.stack)+' '+D(e,'stack').slice(0,2)}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=0;var e=new Error('m');e.name='X';return JSON.stringify(e.stack)+Object.hasOwn(e,'stack')}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=0;var o={name:'A',message:'b'};Error.captureStackTrace(o);return JSON.stringify(o.stack)}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=0;return JSON.stringify(new TypeError('t',{cause:1}).stack)+JSON.stringify(new AggregateError([],'m').stack)}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=0;try{null.x}catch(e){return JSON.stringify(e.stack)}}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=1;function f(){null.x}try{f()}catch(e){return F(()=>e)}}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=2;function f(){return [1].map(function g(){return new Error('x')})[0]}return F(()=>f())}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=1;function f(){return [1].map(function g(){return new Error('x')})[0]}return F(()=>f())}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=2;function f(){return new Promise(function exec(){throw new Error('x')}).catch(e=>e)}return 'p'}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=3;function f(){return Reflect.construct(Error,['x'])}function g(){return f()}return F(()=>g())}finally{Error.stackTraceLimit=old}})",
  "T(()=>{var old=Error.stackTraceLimit;try{Error.stackTraceLimit=3;function f(){return new (class K extends Error{})('x')}function g(){return f()}return F(()=>g())}finally{Error.stackTraceLimit=old}})",
  "T(()=>typeof Error.stackTraceLimit+' '+(Error.stackTraceLimit>=10))", "T(()=>Error.stackTraceLimit)",
);

// ---- 6. Error.prepareStackTrace e CallSite.
const shapes = {
  fn: "function mk(){return new Error('e')}function w(){return mk()}",
  method: "var o={m(){return new Error('e')}};function w(){return o.m()}",
  ctor: "function K(){this.e=new Error('e')}function w(){return new K().e}",
  klass: "class K{constructor(){this.e=new Error('e')}}function w(){return new K().e}",
  arrow: "var mk=()=>new Error('e');function w(){return mk()}",
  nested: "function w(){return (function inner(){return new Error('e')})()}",
  anon: "function w(){return (function(){return new Error('e')})()}",
  getter: "var o={get g(){return new Error('e')}};function w(){return o.g}",
  staticm: "class K{static s(){return new Error('e')}}function w(){return K.s()}",
  proto: "function K(){}K.prototype.pm=function(){return new Error('e')};function w(){return new K().pm()}",
  native: "function w(){return [1].map(function cb(){return new Error('e')})[0]}",
  evalf: "function w(){return eval('new Error(\"e\")')}",
  evalfn: "function w(){return eval('(function ev(){return new Error(\"e\")})()')}",
  newfn: "function w(){return new Function('return new Error(\"e\")')()}",
  call: "function mk(){return new Error('e')}function w(){return mk.call({a:1})}",
  strict: "function mk(){'use strict';return new Error('e')}function w(){return mk.call(5)}",
  bound: "function mk(){return new Error('e')}var b=mk.bind(null);function w(){return b()}",
  computed: "var o={['c'+'m'](){return new Error('e')}};function w(){return o.cm()}",
  symkey: "var s=Symbol('sk');var o={[s](){return new Error('e')}};function w(){return o[s]()}",
  accessset: "var o={set v(x){this.e=new Error('e')}};function w(){o.v=1;return o.e}",
  gen: "function* g(){yield new Error('e')}function w(){return g().next().value}",
  top: "function w(){return new Error('e')}",
  reflect: "function mk(){return new Error('e')}function w(){return Reflect.apply(mk,null,[])}",
  thisobj: "var o={name:'on',m:function nm(){return new Error('e')}};function w(){return o.m()}",
};
const methods = ["getFileName", "getLineNumber", "getColumnNumber", "getFunctionName", "getTypeName", "getMethodName", "isNative", "isEval", "isToplevel", "isConstructor", "isAsync",
  "isPromiseAll", "toString", "getEvalOrigin", "getScriptNameOrSourceURL", "getPromiseIndex", "getPosition", "getEnclosingLineNumber", "getEnclosingColumnNumber", "getScriptHash"];
for (const [name, body] of Object.entries(shapes)) {
  for (const m of methods) {
    add(`T(()=>{${body}return P(function(e,s){return s},()=>w()).slice(0,3).map(function(c){try{var v=c.${m}();return typeof v==='string'?N(v):v===undefined?'undef':v===null?'null':typeof v==='number'?'num':v}catch(x){return 'throws '+x.name}}).join('|')})`);
  }
  add(`T(()=>{${body}return F(()=>w())})`);
  add(`T(()=>{${body}var s=P(function(e,s){return s},()=>w());return typeof s.length+' '+s.length+' '+(typeof s[0].getThis())+' '+(typeof s[0].getFunction())})`);
}
add(
  "T(()=>{var s=P(function(e,s){return s},()=>new Error('e'));return Array.isArray(s)+' '+Object.getPrototypeOf(s[0])===Object.getPrototypeOf(s[1])})",
  "T(()=>{var s=P(function(e,s){return s},()=>new Error('e'));return S(K(Object.getPrototypeOf(s[0])))})",
  "T(()=>{var s=P(function(e,s){return s},()=>new Error('e'));return Object.prototype.toString.call(s[0])+' '+s[0].constructor.name+' '+Object.getOwnPropertyNames(s[0]).length})",
  "T(()=>{var s=P(function(e,s){return s},()=>new Error('e'));return typeof s[0].toString+' '+String(s[0]===s[0])+' '+(String(s[0])===s[0].toString())})",
  "T(()=>{var seen;P(function(e,s){seen=[typeof e,e instanceof Error,typeof s,Array.isArray(s),this===undefined?'u':typeof this];return 'x'},()=>new Error('e'));return seen.join()})",
  "T(()=>{var seen=[];P(function(e,s){seen.push(arguments.length,e.message);return 'x'},()=>new TypeError('tt'));return seen.join()})",
  "T(()=>P(function(){return 42},()=>new Error('e')))", "T(()=>P(function(){return undefined},()=>new Error('e')))", "T(()=>P(function(){return null},()=>new Error('e')))",
  "T(()=>S(P(function(){return {a:1}},()=>new Error('e'))))", "T(()=>S(P(function(){return [1,2]},()=>new Error('e'))))", "T(()=>typeof P(function(){return function(){}},()=>new Error('e')))",
  "T(()=>typeof P(function(){return Symbol()},()=>new Error('e')))", "T(()=>P(function(){return 1n},()=>new Error('e'))+'')",
  "T(()=>M(()=>P(function(){throw new RangeError('hook')},()=>new Error('e'))))", "T(()=>{var e=P(function(){throw new RangeError('hook')},()=>({stack:0}));return 1})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){throw new RangeError('hook')};try{var e=new Error('x');try{e.stack}catch(x){return 'stack threw '+x.name}return 'no throw '+typeof e.stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){throw new RangeError('hook')};try{return M(()=>new Error('x'))}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return 'once'};try{var e=new Error('x');var a=e.stack;Error.prepareStackTrace=function(){return 'twice'};return a+' '+e.stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;var n=0;Error.prepareStackTrace=function(e,s){n++;return 'c'+n};try{var e=new Error('x');var r=n;e.stack;e.stack;return r+' '+n+' '+e.stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;var n=0;Error.prepareStackTrace=function(e,s){n++;return 'c'+n};try{var e=new Error('x');e.stack;var f=new Error('y');return n+' '+f.stack+' '+n}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=1;try{return typeof new Error('x').stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace={};try{return H(new Error('x').stack)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=null;try{return H(new Error('x').stack)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=undefined;try{return H(new Error('x').stack)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=class{};try{return typeof new Error('x').stack}catch(e){return e.name}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=async function(){return 'a'};try{return Object.prototype.toString.call(new Error('x').stack)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function*(){};try{return Object.prototype.toString.call(new Error('x').stack)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e){return e.message+'!'};try{var t={message:'tm'};Error.captureStackTrace(t);return t.stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;var seen;Error.prepareStackTrace=function(e,s){seen=Object.prototype.toString.call(e);return 's'};try{var t=[];Error.captureStackTrace(t);t.stack;return seen}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.length};try{function a(){return new Error('x')}return typeof a().stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.length};var old=Error.stackTraceLimit;Error.stackTraceLimit=1;try{return new Error('x').stack}finally{Error.prepareStackTrace=o;Error.stackTraceLimit=old}})",
  "T(()=>{var o=Error.prepareStackTrace;var old=Error.stackTraceLimit;Error.prepareStackTrace=function(e,s){return s.length};Error.stackTraceLimit=0;try{return new Error('x').stack}finally{Error.prepareStackTrace=o;Error.stackTraceLimit=old}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.length};var old=Error.stackTraceLimit;Error.stackTraceLimit=2;try{function a(){return b()}function b(){return c()}function c(){return new Error('x')}return a().stack}finally{Error.prepareStackTrace=o;Error.stackTraceLimit=old}})",
  "T(()=>{var o=Error.prepareStackTrace;var seen;Error.prepareStackTrace=function(e,s){seen=e===undefined;return 'q'};try{var t=Object.create(null);Error.captureStackTrace(t);t.stack;return seen}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return e.name+'/'+e.message};try{var e=new RangeError('rr');return e.stack+' '+new SuppressedError(1,2,'ss').stack+' '+new AggregateError([],'aa').stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return 'custom'};try{var e=new Error('x');var d=Object.getOwnPropertyDescriptor(e,'stack');return D(e,'stack')+' '+S(K(e))}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return 'custom'};try{var e=new Error('x');e.stack='manual';return e.stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return 'custom'};try{var e=new Error('x');delete e.stack;return typeof e.stack+Object.hasOwn(e,'stack')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.getFunctionName()).join()};try{function aa(){return new Error('x')}function bb(){return aa()}return F(()=>bb())+' '+P(Error.prepareStackTrace,()=>bb()).split(',').slice(0,2)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.getFunctionName()+':'+c.getTypeName()).join()};try{var ob={m(){return new Error('x')}};return ob.m().stack.split(',')[0]}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{function aa(){return new Error('x')}return N(aa().stack).split('\\n')[0]}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s[0].getFunction()};try{function aa(){'use strict';return new Error('x')}return typeof aa().stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s[0].getThis()};try{function aa(){return new Error('x')}return typeof aa.call(5).stack+' '+typeof aa.call({}).stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s[0].getThis()};try{function aa(){'use strict';return new Error('x')}return typeof aa.call(5).stack+' '+typeof aa.call({}).stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s[0].getTypeName()};try{var a=[];a.m=function(){return new Error('x')};var d=new Date(0);d.m=a.m;var r=/x/;r.m=a.m;var f=function(){};f.m=a.m;return [a,d,r,f,Object.create(null),Object.assign(Object.create(null),{m:a.m}),5,'s'].map(v=>{try{return (v.m||a.m).call(v).stack}catch(x){return x.name}}).join()}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s[0].getMethodName()};try{var ob={a(){return new Error('x')}};ob.b=ob.a;return ob.a().stack+' '+ob.b().stack}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s[0].getMethodName()};try{var ob={a(){return new Error('x')}};ob.b=ob.a;var f=ob.a;return String(f().stack)}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.isNative()+''+c.getFunctionName()).join()};try{return [1].map(function cb(){return new Error('x')})[0].stack.split(',').slice(0,2).join()}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{var r=[1].forEach(function cb(){throw new Error('x')})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{[1].forEach(function cb(){throw new Error('x')})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{JSON.parse('{\"a\":1}',function rv(){throw new Error('x')})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{[3,1].sort(function cmp(){throw new Error('x')})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{'a'.replace(/a/,function rp(){throw new Error('x')})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{new Proxy({},{get(){throw new Error('x')}}).k}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{({valueOf(){throw new Error('x')}})+1}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{new (class A{constructor(){throw new Error('x')}})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{new (class A extends Object{constructor(){super();throw new Error('x')}})}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{eval('throw new Error(\"x\")')}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
  "T(()=>{var o=Error.prepareStackTrace;Error.prepareStackTrace=function(e,s){return s.map(c=>c.toString()).join('\\n')};try{new Function('throw new Error(\"x\")')()}catch(e){return N(e.stack).split('\\n').slice(0,2).join('|')}finally{Error.prepareStackTrace=o}})",
);

// ---- 7. Mensagens de erro de APIs comuns.
add(
  "M(()=>Array(-1))", "M(()=>new Array(-1))", "M(()=>new Array(2**32))", "M(()=>new Array(2**32-1).length)", "M(()=>Array(1.5))", "M(()=>Array(NaN))", "M(()=>Array(Infinity))", "M(()=>Array(-0))",
  "M(()=>'x'.repeat(-1))", "M(()=>'x'.repeat(Infinity))", "M(()=>'x'.repeat(2**30))", "M(()=>''.repeat(2**40))", "M(()=>'x'.repeat(NaN))", "M(()=>'ab'.repeat(2**31))", "M(()=>'x'.repeat(-0.5))",
  "M(()=>BigInt(1.5))", "M(()=>BigInt(NaN))", "M(()=>BigInt(Infinity))", "M(()=>BigInt('x'))", "M(()=>BigInt(''))", "M(()=>BigInt(undefined))", "M(()=>BigInt(null))", "M(()=>BigInt(Symbol()))",
  "M(()=>BigInt({}))", "M(()=>BigInt('1.5'))", "M(()=>BigInt('0x'))", "M(()=>BigInt('1n'))", "M(()=>new BigInt(1))", "M(()=>BigInt(1e300))", "M(()=>BigInt(2**53+0.5))",
  "M(()=>1n/0n)", "M(()=>1n%0n)", "M(()=>1n**-1n)", "M(()=>1n+1)", "M(()=>1n*1.5)", "M(()=>+1n)", "M(()=>1n>>>0n)", "M(()=>BigInt.asIntN(-1,1n))", "M(()=>BigInt.asUintN(2**53,1n))", "M(()=>BigInt.asIntN(1,1))",
  "M(()=>2n**(2n**40n))", "M(()=>1n<<(2n**40n))", "M(()=>Math.max(1n))", "M(()=>Math.abs(1n))", "M(()=>Number(Symbol()))", "M(()=>Symbol()+'')", "M(()=>`${Symbol()}`)", "M(()=>+Symbol())", "M(()=>Symbol()*1)",
  "M(()=>new Symbol())", "M(()=>Symbol.keyFor('x'))", "M(()=>Symbol().description.x)", "M(()=>Object(Symbol())+1)",
  "M(()=>JSON.parse(''))", "M(()=>JSON.parse())", "M(()=>JSON.parse(undefined))", "M(()=>JSON.parse('{'))", "M(()=>JSON.parse('}'))", "M(()=>JSON.parse('['))", "M(()=>JSON.parse('[1,]'))", "M(()=>JSON.parse('{\"a\":1,}'))",
  "M(()=>JSON.parse('{a:1}'))", "M(()=>JSON.parse(\"{'a':1}\"))", "M(()=>JSON.parse('01'))", "M(()=>JSON.parse('1.'))", "M(()=>JSON.parse('.5'))", "M(()=>JSON.parse('+1'))", "M(()=>JSON.parse('NaN'))", "M(()=>JSON.parse('Infinity'))",
  "M(()=>JSON.parse('undefined'))", "M(()=>JSON.parse('nul'))", "M(()=>JSON.parse('tru'))", "M(()=>JSON.parse('\"abc'))", "M(()=>JSON.parse('\"\\\\x\"'))", "M(()=>JSON.parse('\"\\\\u12\"'))", "M(()=>JSON.parse('\"\\n\"'))",
  "M(()=>JSON.parse('1 2'))", "M(()=>JSON.parse('{} x'))", "M(()=>JSON.parse('[1 2]'))", "M(()=>JSON.parse('{\"a\" 1}'))", "M(()=>JSON.parse('{\"a\":}'))", "M(()=>JSON.parse('{\"a\"}'))", "M(()=>JSON.parse('//c'))", "M(()=>JSON.parse('/*c*/1'))",
  "M(()=>JSON.parse(' '))", "M(()=>JSON.parse('\\u00a01'))", "M(()=>JSON.parse('[1,2,3,'))", "M(()=>JSON.parse('{\"a\":[1,{\"b\":}]}'))", "M(()=>JSON.parse('-'))", "M(()=>JSON.parse('1e'))", "M(()=>JSON.parse('1e+'))", "M(()=>JSON.parse('0x10'))",
  "M(()=>JSON.parse('\\'a\\''))", "M(()=>JSON.parse(Symbol()))", "M(()=>JSON.parse(1n))", "M(()=>JSON.parse({toString(){return '{'}}))", "M(()=>JSON.parse('{\"__proto__\":1,}'))", "M(()=>JSON.parse('\"\\ud800\"'))", "M(()=>JSON.parse('[\\u0000]'))",
  "M(()=>JSON.stringify(1n))", "M(()=>JSON.stringify({a:1n}))", "M(()=>JSON.stringify([1n]))", "M(()=>{var a={};a.a=a;JSON.stringify(a)})", "M(()=>{var a=[];a[0]=a;JSON.stringify(a)})", "M(()=>{var a={b:{c:{}}};a.b.c.d=a;JSON.stringify(a)})",
  "M(()=>JSON.stringify({toJSON(){throw new RangeError('tj')}}))", "M(()=>JSON.stringify({a:1},null,{toString(){throw 1}}))", "M(()=>JSON.stringify(Object(1n)))",
  "M(()=>null.x)", "M(()=>undefined.x)", "M(()=>null[0])", "M(()=>undefined['k'])", "M(()=>{var o={};o.a.b})", "M(()=>{var o={};o.a.b.c})", "M(()=>{var o={a:{}};o.a.b.c})", "M(()=>{var o={};o.a.b=1})", "M(()=>{var o;o.a})",
  "M(()=>{var o=null;o.a=1})", "M(()=>{var o=null;o[Symbol.iterator]})", "M(()=>{var o=null;delete o.a})", "M(()=>{var o=null;o.a++})", "M(()=>{var o=null;o.a+=1})", "M(()=>{var o=null;o.a()})", "M(()=>{var o=null;o['a']()})",
  "M(()=>undefined())", "M(()=>null())", "M(()=>(void 0)())", "M(()=>{var f;f()})", "M(()=>{var f=1;f()})", "M(()=>{var f={};f()})", "M(()=>{var f={};f.g()})", "M(()=>{var f={g:1};f.g()})", "M(()=>{var f={};f['g']()})",
  "M(()=>{var f={};var k='g';f[k]()})", "M(()=>{var f={};f[1]()})", "M(()=>{var f=[];f[0]()})", "M(()=>{var f=[];f.x.y()})", "M(()=>[]())", "M(()=>({})())", "M(()=>(1)())", "M(()=>'s'())", "M(()=>(()=>1)()())",
  "M(()=>(function(){})()())", "M(()=>{function f(){}f()()})", "M(()=>{var o={f(){}};o.f()()})", "M(()=>{var o={f(){}};o.f().g})", "M(()=>{var o={f(){}};o.f().g()})", "M(()=>[].map.call(null))", "M(()=>[].map(1))", "M(()=>[].map())",
  "M(()=>[].forEach({}))", "M(()=>[].filter('s'))", "M(()=>[].reduce((a,b)=>a))", "M(()=>[].reduce(1))", "M(()=>[].sort(1))", "M(()=>[3,1].sort('x'))", "M(()=>[].find(null))", "M(()=>[].flatMap())", "M(()=>[].reduceRight((a,b)=>a))",
  "M(()=>Array.from(null))", "M(()=>Array.from(undefined))", "M(()=>Array.from({length:-1}))", "M(()=>Array.from({length:2**32}))", "M(()=>Array.from(1,2))", "M(()=>Array.from([],{}))", "M(()=>Array.of.call(1))",
  "M(()=>[].concat.call(null))", "M(()=>[].push.call(Object.freeze([]),1))", "M(()=>Object.freeze([1]).push(2))", "M(()=>Object.freeze([1]).pop())", "M(()=>Object.freeze([1]).shift())", "M(()=>Object.freeze([1]).unshift(1))",
  "M(()=>Object.freeze([1]).splice(0,1))", "M(()=>Object.freeze([1,2]).reverse())", "M(()=>Object.freeze([2,1]).sort())", "M(()=>Object.freeze([1]).fill(0))", "M(()=>Object.freeze([1]).copyWithin(0,0))", "M(()=>{'use strict';Object.freeze([1]).length=0})",
  "M(()=>{'use strict';Object.freeze([1])[0]=2})", "M(()=>{'use strict';Object.freeze({a:1}).a=2})", "M(()=>{'use strict';Object.freeze({a:1}).b=2})", "M(()=>{'use strict';delete Object.freeze({a:1}).a})", "M(()=>{'use strict';undefinedVar=1})",
  "M(()=>{'use strict';var o={get a(){return 1}};o.a=2})", "M(()=>{'use strict';Object.defineProperty({},'a',{value:1}).a=2})", "M(()=>{'use strict';'s'.x=1})", "M(()=>{'use strict';(1).x=1})", "M(()=>{'use strict';'s'[0]='t'})", "M(()=>{'use strict';'s'.length=1})",
  "M(()=>{'use strict';NaN=1})", "M(()=>{'use strict';undefined=1})", "M(()=>{'use strict';Math.PI=3})", "M(()=>{'use strict';delete Math.PI})", "M(()=>{'use strict';delete Object.prototype})", "M(()=>{'use strict';Object.preventExtensions([]).push(1)})",
  "M(()=>{'use strict';Object.preventExtensions({}).x=1})", "M(()=>{'use strict';Object.seal({a:1}).b=1})", "M(()=>{'use strict';delete Object.seal({a:1}).a})", "M(()=>{'use strict';var f=function(){};f.name='x'})", "M(()=>{'use strict';(function(){}).length=1})",
  "M(()=>{'use strict';arguments.callee})", "M(()=>{'use strict';(function(){}).caller})", "M(()=>(function(){'use strict';return arguments.callee})())", "M(()=>(function(){'use strict';return (function(){}).caller})())",
  "M(()=>new 1)", "M(()=>new null)", "M(()=>new undefined)", "M(()=>new 'x')", "M(()=>new true)", "M(()=>new {})", "M(()=>new [])", "M(()=>new (()=>1))", "M(()=>new Math.max)", "M(()=>new Math)", "M(()=>new JSON)", "M(()=>new Reflect)",
  "M(()=>new (async function(){}))", "M(()=>new (function*(){}))", "M(()=>new ({m(){}}).m)", "M(()=>new ({get g(){return 1}}).g)", "M(()=>new Symbol)", "M(()=>new BigInt)", "M(()=>new parseInt)", "M(()=>new Array.prototype.map)", "M(()=>new (class{}).prototype.constructor.bind())",
  "M(()=>{var o={};new o.f})", "M(()=>{var o={};new o.f()})", "M(()=>{var o={f:1};new o.f})", "M(()=>{var x;new x})", "M(()=>{var x=1;new x()})", "M(()=>{var o={a:{}};new o.a.b})", "M(()=>{var o={a:{}};new o.a.b()})", "M(()=>{var o={};new o['f']})",
  "M(()=>{class A{};A()})", "M(()=>{class A{};A.call({})})", "M(()=>{class A{};A.apply()})", "M(()=>{class A{static s(){}};new A.s})", "M(()=>{class A extends null{};new A})", "M(()=>{class A extends 1{}})", "M(()=>{class A extends {}{}})", "M(()=>{class A extends (()=>1){}})",
  "M(()=>{function F(){};F.prototype=1;class A extends F{}})", "M(()=>{var F=function(){}.bind();class A extends F{}})", "M(()=>{class A{constructor(){this.x}};class B extends A{constructor(){this.x;super()}};new B})", "M(()=>{class B extends Object{constructor(){}};new B})",
  "M(()=>{class B extends Object{constructor(){super();super()}};new B})", "M(()=>{class B extends Object{constructor(){return 1}};new B})", "M(()=>{class A{#p;static t(o){return o.#p}};A.t({})})", "M(()=>{class A{#p;static t(o){o.#p=1}};A.t({})})",
  "M(()=>{class A{#m(){};static t(o){o.#m()}};A.t({})})", "M(()=>{class A{static #p=1;static t(o){return o.#p}};A.t(class{})})", "M(()=>{class A{#p;static t(o){return #p in o}};A.t(1)})", "M(()=>{class A{constructor(o){return o}};class B extends A{#q=1;constructor(o){super(o)}};var o={};new B(o);new B(o)})",
  "M(()=>1 instanceof 1)", "M(()=>1 instanceof {})", "M(()=>({}) instanceof (()=>1))", "M(()=>({}) instanceof null)", "M(()=>({}) instanceof undefined)", "M(()=>({}) instanceof {[Symbol.hasInstance]:1})", "M(()=>({}) instanceof function(){}.bind())",
  "M(()=>{function F(){};F.prototype=1;({}) instanceof F})", "M(()=>'a' in 'b')", "M(()=>'a' in 1)", "M(()=>'a' in null)", "M(()=>'a' in undefined)", "M(()=>Symbol() in 1)", "M(()=>1 in true)", "M(()=>{var o;'x' in o})", "M(()=>{var o={};'x' in o.a})",
  "M(()=>{var [a]=null})", "M(()=>{var [a]=undefined})", "M(()=>{var [a]=1})", "M(()=>{var [a]={}})", "M(()=>{var [a]=NaN})", "M(()=>{var {a}=null})", "M(()=>{var {a}=undefined})", "M(()=>{var {a:{b}}={}})", "M(()=>{var {a:{b}}={a:null}})",
  "M(()=>{var [[a]]=[1]})", "M(()=>{var [a,[b]]=[1]})", "M(()=>{var {...r}=null})", "M(()=>{var [...r]=1})", "M(()=>{(function({a}){})()})", "M(()=>{(function([a]){})()})", "M(()=>{(function({a}){})(null)})", "M(()=>{(({a})=>1)(undefined)})",
  "M(()=>{(function(a,{b}){})(1)})", "M(()=>{for(var x of 1);})", "M(()=>{for(var x of null);})", "M(()=>{for(var x of {});})", "M(()=>{for(var x of undefined);})", "M(()=>{for(var [x] of [1]);})", "M(()=>{for(var {x} of [null]);})",
  "M(()=>{[...1]})", "M(()=>{[...null]})", "M(()=>{[...{}]})", "M(()=>{Math.max(...1)})", "M(()=>{Math.max(...null)})", "M(()=>{Math.max(...{})})", "M(()=>{new Set(1)})", "M(()=>{new Set({})})", "M(()=>{new Map(1)})", "M(()=>{new Map([1])})", "M(()=>{new Map([[1,2],3])})",
  "M(()=>{new WeakMap([[1,2]])})", "M(()=>{new WeakSet([1])})", "M(()=>{new WeakMap().set(1,1)})", "M(()=>{new WeakSet().add('s')})", "M(()=>{new WeakRef(1)})", "M(()=>{new WeakRef()})", "M(()=>{new WeakRef(Symbol.for('x'))})", "M(()=>{new FinalizationRegistry(1)})",
  "M(()=>{new FinalizationRegistry(()=>{}).register(1)})", "M(()=>{var o={};new FinalizationRegistry(()=>{}).register(o,o)})", "M(()=>{Map()})", "M(()=>{Set()})", "M(()=>{WeakMap()})", "M(()=>{Promise()})", "M(()=>{new Promise()})", "M(()=>{new Promise(1)})", "M(()=>{Promise.resolve.call(1)})",
  "M(()=>{Promise.prototype.then.call(1)})", "M(()=>{Promise.all.call(1)})", "M(()=>{Map.prototype.get.call({},1)})", "M(()=>{Map.prototype.set.call(new Set,1,2)})", "M(()=>{Set.prototype.add.call(new Map,1)})", "M(()=>{Map.prototype.size})", "M(()=>{Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call({})})",
  "M(()=>{Date.prototype.getTime.call({})})", "M(()=>{Date.prototype.toISOString.call(1)})", "M(()=>{new Date(NaN).toISOString()})", "M(()=>{new Date(8.64e15+1).toISOString()})", "M(()=>{new Date('x').toISOString()})", "M(()=>{Date.prototype.toJSON.call(null)})",
  "M(()=>{Date.prototype[Symbol.toPrimitive].call(new Date,'x')})", "M(()=>{Date.prototype[Symbol.toPrimitive].call(1,'number')})", "M(()=>{new Date(Symbol())})", "M(()=>{new Date(1n)})", "M(()=>{Date.UTC(Symbol())})",
  "M(()=>{RegExp.prototype.exec.call({},'')})", "M(()=>{RegExp.prototype.test.call(1,'')})", "M(()=>{new RegExp('(')})", "M(()=>{new RegExp('[')})", "M(()=>{new RegExp('a','gg')})", "M(()=>{new RegExp('a','z')})", "M(()=>{new RegExp('*')})", "M(()=>{new RegExp('a{2,1}')})", "M(()=>{new RegExp('\\\\')})",
  "M(()=>{new RegExp('(?<n>a)(?<n>b)')})", "M(()=>{new RegExp('\\\\k<x>','u')})", "M(()=>{new RegExp('\\\\p{Foo}','u')})", "M(()=>{new RegExp('(?<=a)+')})", "M(()=>{new RegExp('[b-a]')})", "M(()=>{new RegExp('a','uv')})", "M(()=>{RegExp.prototype.global})", "M(()=>{RegExp.prototype.flags})",
  "M(()=>{Object.getOwnPropertyDescriptor(RegExp.prototype,'global').get.call({})})", "M(()=>{RegExp.prototype.toString.call(1)})", "M(()=>{'a'.matchAll(/a/)})", "M(()=>{'a'.replaceAll(/a/,'b')})", "M(()=>{'a'.match({[Symbol.match]:1})})", "M(()=>{'a'.startsWith(/a/)})", "M(()=>{'a'.includes(/a/)})", "M(()=>{'a'.endsWith(/a/)})",
  "M(()=>{String.prototype.trim.call(null)})", "M(()=>{String.prototype.toString.call(1)})", "M(()=>{String.prototype.valueOf.call({})})", "M(()=>{'a'.normalize('x')})", "M(()=>{'a'.localeCompare('b','xx-invalid-')})", "M(()=>{String.fromCodePoint(-1)})", "M(()=>{String.fromCodePoint(1.5)})", "M(()=>{String.fromCodePoint(0x110000)})",
  "M(()=>{String.fromCodePoint('x')})", "M(()=>{'a'.at(Symbol())})", "M(()=>{'a'.padStart(2**31,'x')})", "M(()=>{'a'.padEnd(2**30,'xy')})", "M(()=>{'a'.codePointAt(1n)})", "M(()=>{'\\ud800'.toWellFormed().isWellFormed.call(null)})", "M(()=>{'a'.isWellFormed.call(null)})", "M(()=>{String.raw()})", "M(()=>{String.raw(null)})",
  "M(()=>{(1).toFixed(101)})", "M(()=>{(1).toFixed(-1)})", "M(()=>{(1).toPrecision(0)})", "M(()=>{(1).toPrecision(101)})", "M(()=>{(1).toExponential(-1)})", "M(()=>{(1).toExponential(101)})", "M(()=>{(1).toString(1)})", "M(()=>{(1).toString(37)})", "M(()=>{(1).toString(Infinity)})",
  "M(()=>{Number.prototype.toString.call('1')})", "M(()=>{Number.prototype.valueOf.call({})})", "M(()=>{Number.prototype.toFixed.call(null)})", "M(()=>{(1).toLocaleString('xx-invalid-')})", "M(()=>{(1).toLocaleString('en',{style:'currency'})})", "M(()=>{(1).toLocaleString('en',{minimumFractionDigits:101})})",
  "M(()=>{new Intl.NumberFormat('en',{style:'bogus'})})", "M(()=>{new Intl.DateTimeFormat('en',{timeZone:'Nope/Zone'})})", "M(()=>{new Intl.DateTimeFormat('en',{dateStyle:'x'})})", "M(()=>{Intl.getCanonicalLocales('x_y')})", "M(()=>{new Intl.Locale()})", "M(()=>{new Intl.Locale('x')})", "M(()=>{Intl.NumberFormat.prototype.format})",
  "M(()=>{new Uint8Array(-1)})", "M(()=>{new Uint8Array(2**53)})", "M(()=>{new Uint8Array(1.5)})", "M(()=>{new Uint8Array({length:-1})})", "M(()=>{new Uint16Array(new ArrayBuffer(3))})", "M(()=>{new Uint16Array(new ArrayBuffer(4),1)})", "M(()=>{new Uint16Array(new ArrayBuffer(4),6)})",
  "M(()=>{new Uint16Array(new ArrayBuffer(4),0,3)})", "M(()=>{new Float64Array(new ArrayBuffer(7))})", "M(()=>{Uint8Array(1)})", "M(()=>{new (Object.getPrototypeOf(Uint8Array))})", "M(()=>{Object.getPrototypeOf(Uint8Array)()})", "M(()=>{Uint8Array.from(null)})", "M(()=>{Uint8Array.of.call(1)})",
  "M(()=>{new Uint8Array(1).set([1,2])})", "M(()=>{new Uint8Array(1).set([1],1)})", "M(()=>{new Uint8Array(1).set([1],-1)})", "M(()=>{new Uint8Array(1).set(null)})", "M(()=>{new Uint8Array(1).fill(1n)})", "M(()=>{new BigInt64Array(1).fill(1)})", "M(()=>{new BigInt64Array([1])})", "M(()=>{new Uint8Array([1n])})",
  "M(()=>{new Uint8Array(1).subarray.call([])})", "M(()=>{Uint8Array.prototype.length})", "M(()=>{Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),'length').get.call([])})", "M(()=>{new Uint8Array(1).with(5,1)})", "M(()=>{new Uint8Array(1).at.call(1)})",
  "M(()=>{var t=new Uint8Array(1);structuredCloneProbe})", "M(()=>{new DataView(new ArrayBuffer(1)).getInt32(0)})", "M(()=>{new DataView(new ArrayBuffer(1)).setInt8(1,0)})", "M(()=>{new DataView(new ArrayBuffer(1)).getInt8(-1)})", "M(()=>{new DataView(1)})", "M(()=>{new DataView(new ArrayBuffer(1),2)})",
  "M(()=>{new DataView(new ArrayBuffer(1),0,2)})", "M(()=>{DataView(new ArrayBuffer(1))})", "M(()=>{new DataView(new ArrayBuffer(8)).getBigInt64(0).x.y})", "M(()=>{new DataView(new ArrayBuffer(8)).setBigInt64(0,1)})",
  "M(()=>{new ArrayBuffer(-1)})", "M(()=>{new ArrayBuffer(2**53)})", "M(()=>{ArrayBuffer(1)})", "M(()=>{new ArrayBuffer(1,{maxByteLength:0})})", "M(()=>{new ArrayBuffer(8,{maxByteLength:4})})", "M(()=>{new ArrayBuffer(1).resize(2)})", "M(()=>{new ArrayBuffer(1,{maxByteLength:2}).resize(3)})",
  "M(()=>{ArrayBuffer.prototype.slice.call({})})", "M(()=>{Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get.call({})})", "M(()=>{new ArrayBuffer(1).transfer(-1)})", "M(()=>{var b=new ArrayBuffer(1);b.transfer();b.slice(0)})", "M(()=>{var b=new ArrayBuffer(1);b.transfer();new Uint8Array(b)})",
  "M(()=>{new SharedArrayBuffer(-1)})", "M(()=>{SharedArrayBuffer(1)})", "M(()=>{Atomics.add(new Float64Array(1),0,1)})", "M(()=>{Atomics.add(new Int32Array(1),1,1)})", "M(()=>{Atomics.wait(new Int32Array(1),0,0)})", "M(()=>{Atomics.notify(new Int32Array(1),0)})", "M(()=>{Atomics.add([],0,1)})",
  "M(()=>{Object.defineProperty(1,'a',{})})", "M(()=>{Object.defineProperty({},'a',1)})", "M(()=>{Object.defineProperty({},'a',{get:1})})", "M(()=>{Object.defineProperty({},'a',{get(){},value:1})})", "M(()=>{Object.defineProperty(Object.freeze({}),'a',{value:1})})", "M(()=>{Object.defineProperty(Object.freeze({a:1}),'a',{value:2})})",
  "M(()=>{Object.defineProperties({},null)})", "M(()=>{Object.defineProperties({},{a:1})})", "M(()=>{Object.create(1)})", "M(()=>{Object.create()})", "M(()=>{Object.create({},null)})", "M(()=>{Object.setPrototypeOf({},1)})", "M(()=>{Object.setPrototypeOf(null,{})})", "M(()=>{Object.setPrototypeOf(Object.freeze({}),{})})",
  "M(()=>{var a={};var b=Object.create(a);Object.setPrototypeOf(a,b)})", "M(()=>{Object.setPrototypeOf(Object.prototype,{})})", "M(()=>{Object.prototype.__proto__=1;return 1})", "M(()=>{var a={};a.__proto__=a})", "M(()=>{Object.getPrototypeOf(null)})", "M(()=>{Object.getPrototypeOf(undefined)})",
  "M(()=>{Object.keys(null)})", "M(()=>{Object.entries(undefined)})", "M(()=>{Object.values(null)})", "M(()=>{Object.assign(null)})", "M(()=>{Object.assign({},null,undefined,1)})", "M(()=>{Object.assign(Object.freeze({a:1}),{a:2})})", "M(()=>{Object.fromEntries(1)})", "M(()=>{Object.fromEntries([1])})", "M(()=>{Object.fromEntries(null)})",
  "M(()=>{Object.groupBy(1,x=>x)})", "M(()=>{Object.groupBy([],1)})", "M(()=>{Map.groupBy(null,x=>x)})", "M(()=>{Object.freeze(Symbol())})", "M(()=>{Object.getOwnPropertyNames(null)})", "M(()=>{Object.getOwnPropertyDescriptor(null,'a')})", "M(()=>{Object.getOwnPropertyDescriptors(undefined)})",
  "M(()=>{Object.is.call()})", "M(()=>{Object.hasOwn(null,'a')})", "M(()=>{Object.prototype.hasOwnProperty.call(null,'a')})", "M(()=>{Object.prototype.toString.call()})", "M(()=>{Object.prototype.valueOf.call(null)})", "M(()=>{Object.prototype.__lookupGetter__.call(null,'a')})",
  "M(()=>{Object.prototype.__defineGetter__.call({},'a',1)})", "M(()=>{Object.prototype.__defineSetter__.call(null,'a',()=>1)})", "M(()=>{Object.prototype.isPrototypeOf.call(null,{})})", "M(()=>{Object.prototype.propertyIsEnumerable.call(undefined,'a')})", "M(()=>{Object.prototype.toLocaleString.call(null)})",
  "M(()=>{Function.prototype.call.call(1)})", "M(()=>{Function.prototype.apply.call(1)})", "M(()=>{Function.prototype.bind.call(1)})", "M(()=>{Function.prototype.toString.call({})})", "M(()=>{Function.prototype.toString.call(class{}.prototype)})", "M(()=>{(function(){}).apply(null,1)})", "M(()=>{(function(){}).apply(null,'s')})",
  "M(()=>{(function(){}).apply(null,{length:2**32})})", "M(()=>{(function(){}).apply(null,{length:1e9})})", "M(()=>{Reflect.apply(1)})", "M(()=>{Reflect.apply(()=>1,null)})", "M(()=>{Reflect.apply(()=>1,null,1)})", "M(()=>{Reflect.construct(1,[])})", "M(()=>{Reflect.construct(()=>1,[])})", "M(()=>{Reflect.construct(function(){},[],1)})",
  "M(()=>{Reflect.construct(function(){},[],()=>1)})", "M(()=>{Reflect.construct(function(){})})", "M(()=>{Reflect.get(1,'a')})", "M(()=>{Reflect.set(null,'a',1)})", "M(()=>{Reflect.has(1,'a')})", "M(()=>{K(1)})", "M(()=>{Reflect.getPrototypeOf(1)})", "M(()=>{Reflect.setPrototypeOf({},1)})",
  "M(()=>{Reflect.defineProperty({},'a',1)})", "M(()=>{Reflect.deleteProperty(1,'a')})", "M(()=>{Reflect.isExtensible(1)})", "M(()=>{Reflect.preventExtensions(1)})", "M(()=>{Reflect.getOwnPropertyDescriptor(1,'a')})",
  "M(()=>{new Proxy()})", "M(()=>{new Proxy({})})", "M(()=>{new Proxy(1,{})})", "M(()=>{new Proxy({},1)})", "M(()=>{Proxy({},{})})", "M(()=>{Proxy.revocable(1,{})})", "M(()=>{var r=Proxy.revocable({},{});r.revoke();r.proxy.a})", "M(()=>{var r=Proxy.revocable({},{});r.revoke();r.proxy.a=1})",
  "M(()=>{var r=Proxy.revocable({},{});r.revoke();'a' in r.proxy})", "M(()=>{var r=Proxy.revocable({},{});r.revoke();Object.keys(r.proxy)})", "M(()=>{var r=Proxy.revocable(function(){},{});r.revoke();r.proxy()})", "M(()=>{var r=Proxy.revocable(function(){},{});r.revoke();new r.proxy})",
  "M(()=>{var r=Proxy.revocable({},{});r.revoke();new Proxy(r.proxy,{})})", "M(()=>{var r=Proxy.revocable({},{});r.revoke();Object.getPrototypeOf(r.proxy)})", "M(()=>{var r=Proxy.revocable([],{});r.revoke();Array.isArray(r.proxy)})", "M(()=>{var r=Proxy.revocable({},{});r.revoke();delete r.proxy.a})",
  "M(()=>{new Proxy({},{get:1}).a})", "M(()=>{new Proxy({},{get:null}).a})", "M(()=>{new Proxy({},{has:1});'a' in new Proxy({},{has:1})})", "M(()=>{new Proxy({},{ownKeys(){return 1}});Object.keys(new Proxy({},{ownKeys(){return 1}}))})", "M(()=>{Object.keys(new Proxy({},{ownKeys(){return [1]}}))})",
  "M(()=>{Object.keys(new Proxy({},{ownKeys(){return ['a','a']}}))})", "M(()=>{Object.keys(new Proxy(Object.freeze({a:1}),{ownKeys(){return []}}))})", "M(()=>{Object.keys(new Proxy(Object.preventExtensions({a:1}),{ownKeys(){return ['a','b']}}))})", "M(()=>{new Proxy({},{getPrototypeOf(){return 1}}).__proto__})",
  "M(()=>{Object.getPrototypeOf(new Proxy({},{getPrototypeOf(){return 1}}))})", "M(()=>{Object.getPrototypeOf(new Proxy(Object.preventExtensions({}),{getPrototypeOf(){return Array.prototype}}))})", "M(()=>{Object.defineProperty(new Proxy({},{defineProperty(){return false}}),'a',{value:1})})",
  "M(()=>{Reflect.defineProperty(new Proxy({},{defineProperty(){return true}}),'a',{value:1,configurable:false})})", "M(()=>{new Proxy(Object.freeze({a:1}),{get(){return 2}}).a})", "M(()=>{'a' in new Proxy(Object.freeze({a:1}),{has(){return false}})})", "M(()=>{delete new Proxy(Object.freeze({a:1}),{deleteProperty(){return true}}).a})",
  "M(()=>{'use strict';new Proxy({},{set(){return false}}).a=1})", "M(()=>{'use strict';delete new Proxy({a:1},{deleteProperty(){return false}}).a})", "M(()=>{Object.getOwnPropertyDescriptor(new Proxy({},{getOwnPropertyDescriptor(){return 1}}),'a')})",
  "M(()=>{Object.getOwnPropertyDescriptor(new Proxy({},{getOwnPropertyDescriptor(){return {value:1,configurable:false}}}),'a')})", "M(()=>{Object.isExtensible(new Proxy({},{isExtensible(){return false}}))})", "M(()=>{Object.preventExtensions(new Proxy({},{preventExtensions(){return true}}))})",
  "M(()=>{Object.setPrototypeOf(new Proxy({},{setPrototypeOf(){return false}}),null)})", "M(()=>{new (new Proxy(function(){},{construct(){return 1}}))})", "M(()=>{new (new Proxy(()=>1,{}))})", "M(()=>{new Proxy({},{}).call()})",
  "M(()=>{eval('}')})", "M(()=>{eval('var 1')})", "M(()=>{eval('(')})", "M(()=>{eval('a b')})", "M(()=>{eval('if')})", "M(()=>{eval('let let=1')})", "M(()=>{eval('\"')})", "M(()=>{eval('/')})", "M(()=>{eval('`')})", "M(()=>{eval('1++')})", "M(()=>{eval('for(;;')})", "M(()=>{eval('class')})", "M(()=>{eval('function')})",
  "M(()=>{eval('a=>')})", "M(()=>{eval('({a:1')})", "M(()=>{eval('[1,')})", "M(()=>{eval('x = ')})", "M(()=>{eval('return 1')})", "M(()=>{eval('break')})", "M(()=>{eval('continue')})", "M(()=>{eval('yield 1')})", "M(()=>{eval('await 1')})", "M(()=>{eval('new.target')})", "M(()=>{eval('super.x')})", "M(()=>{eval('import x from \"y\"')})",
  "M(()=>{eval('export var a')})", "M(()=>{eval('with(1){}\"use strict\"')})", "M(()=>{eval('\"use strict\";with(1){}')})", "M(()=>{eval('\"use strict\";var eval')})", "M(()=>{eval('\"use strict\";010')})", "M(()=>{eval('\"use strict\";delete x')})", "M(()=>{eval('let a;let a')})", "M(()=>{eval('const a')})", "M(()=>{eval('const a=1;a=2')})",
  "M(()=>{eval('a:a:1')})", "M(()=>{eval('1=2')})", "M(()=>{eval('f()=1')})", "M(()=>{eval('++1')})", "M(()=>{eval('for(1 of []);')})", "M(()=>{eval('(a,a)=>1')})", "M(()=>{eval('({a=1})')})", "M(()=>{eval('class A{constructor(){}constructor(){}}')})", "M(()=>{eval('class A{get constructor(){}}')})",
  "M(()=>{eval('x?.y=1')})", "M(()=>{eval('new x?.y')})", "M(()=>{eval('a??b||c')})", "M(()=>{eval('-1**2')})", "M(()=>{eval('async function f(){var await}')})", "M(()=>{eval('function*g(){var yield}')})", "M(()=>{eval('/(/')})", "M(()=>{eval('/a/gg')})", "M(()=>{eval('\"\\\\u{110000}\"')})", "M(()=>{eval('1_')})", "M(()=>{eval('1__0')})", "M(()=>{eval('0b2')})", "M(()=>{eval('1n.5')})", "M(()=>{eval('1.5n')})",
  "M(()=>{new Function('a b','')})", "M(()=>{new Function('}','')})", "M(()=>{new Function('return }')})", "M(()=>{new Function('a','a','\"use strict\"')})", "M(()=>{Function('...a,b','')})", "M(()=>{Function(')','')})", "M(()=>{Function('/*','*/){')})", "M(()=>{new (Object.getPrototypeOf(async function(){}).constructor)('await','')})",
  "M(()=>{decodeURIComponent('%')})", "M(()=>{decodeURIComponent('%E0%A4%A')})", "M(()=>{decodeURIComponent('%C0%80')})", "M(()=>{decodeURI('%ZZ')})", "M(()=>{encodeURIComponent('\\ud800')})", "M(()=>{encodeURI('\\udc00')})", "M(()=>{unescape.call()})", "M(()=>{decodeURIComponent(Symbol())})", "M(()=>{decodeURIComponent('%ff')})",
  "M(()=>{atobProbe})", "M(()=>{undefinedVariable})", "M(()=>{undefinedVariable.x})", "M(()=>{undefinedFn()})", "M(()=>{typeof undefinedVariable;undefinedVariable2=1;return 1})", "M(()=>{let a=a})", "M(()=>{a;let a})", "M(()=>{a;const a=1})", "M(()=>{a;class a{}})", "M(()=>{class A extends A{}})", "M(()=>{var f=()=>g;let g=1;f();})",
  "M(()=>{(function(){x;let x})()})", "M(()=>{{x;let x}})", "M(()=>{switch(1){case 0:let x;case 1:x}})", "M(()=>{for(let i=i;;);})", "M(()=>{for(let x of [x]);})", "M(()=>{var {a=b,b}={}})", "M(()=>{(function(a=b,b){})()})", "M(()=>{(function(a=a){})()})",
  "M(()=>{class A{static x=y;};let y})", "M(()=>{new (class A{x=this.y.z})})", "M(()=>{new (class A{static{this.q.r}})})", "M(()=>{super_probe})", "M(()=>{({m(){super.x.y}}).m()})", "M(()=>{({m(){super.x()}}).m()})", "M(()=>{class A{m(){super.q()}};new A().m()})",
  "M(()=>{function* g(){yield 1};var it=g();it.next();it.throw(new RangeError('t'))})", "M(()=>{function* g(){it.next()};var it=g();it.next()})", "M(()=>{function* g(){yield 1};g.prototype.next.call({})})", "M(()=>{Object.getPrototypeOf(function*(){}).prototype.next.call({})})", "M(()=>{new (function*(){})})",
  "M(()=>{[][Symbol.iterator]().next.call({})})", "M(()=>{[][Symbol.iterator]().next.call(new Map().keys())})", "M(()=>{new Map().keys().next.call([].values())})", "M(()=>{new Set().values().next.call({})})", "M(()=>{''[Symbol.iterator]().next.call({})})", "M(()=>{[].values.call(null)})",
  "M(()=>{Iterator.prototype.map.call(1,x=>x)})", "M(()=>{Iterator.from(1)})", "M(()=>{Iterator.from({})})", "M(()=>{[].values().map(1)})", "M(()=>{[].values().take(-1)})", "M(()=>{[].values().take(NaN)})", "M(()=>{[].values().drop(-1)})", "M(()=>{[].values().flatMap(()=>1).next()})", "M(()=>{[1].values().flatMap(()=>1).next()})",
  "M(()=>{[1].values().reduce()})", "M(()=>{[].values().reduce((a,b)=>a)})", "M(()=>{new Iterator})", "M(()=>{Iterator()})", "M(()=>{class I extends Iterator{};new I;return 1})", "M(()=>{Iterator.prototype.toArray.call(null)})",
  "M(async ()=>{await null})", "M(()=>{(async function(){})().then.call(1)})", "M(()=>{async function f(){};new f})", "M(()=>{Promise.reject.call(1,2)})", "M(()=>{Promise.withResolvers.call(1)})", "M(()=>{Promise.prototype.finally.call(1)})", "M(()=>{Promise.prototype.catch.call({})})",
  "M(()=>{Promise.allSettled.call(function(){})})", "M(()=>{Promise.race.call({})})", "M(()=>{Promise.try.call(1,()=>1)})", "M(()=>{Promise.try()})",
  "M(()=>{globalThis.globalThis=1;return 1})", "M(()=>{'use strict';globalThis.NaN=1})", "M(()=>{'use strict';globalThis.Infinity=1})", "M(()=>{'use strict';undefined=3})", "M(()=>{Object.defineProperty(globalThis,'NaN',{value:1})})", "M(()=>{eval('var NaN')})", "M(()=>{eval('function NaN(){}')})", "M(()=>{eval('let NaN')})",
  "M(()=>{Date.prototype.setHours.call(new Date,Symbol())})", "M(()=>{Math.round(Symbol())})", "M(()=>{Math.hypot(1n)})", "M(()=>{Math.sign(1n)})", "M(()=>{Math.max(Symbol())})", "M(()=>{Math.random.call(1)})", "M(()=>{Math.imul(1n,1)})", "M(()=>{Math.sumPrecise})",
  "M(()=>{Array.prototype.at.call(null)})", "M(()=>{Array.prototype.includes.call(undefined)})", "M(()=>{Array.prototype.join.call({length:2**32})})", "M(()=>{Array.prototype.push.call({length:2**53-1},1)})", "M(()=>{Array.prototype.unshift.call({length:2**53-1},1)})", "M(()=>{Array.prototype.splice.call({length:2**53-1},0,0,1)})",
  "M(()=>{Array.prototype.concat.call({length:2**53-1,[Symbol.isConcatSpreadable]:true},[1])})", "M(()=>{[].concat({length:2**53,[Symbol.isConcatSpreadable]:true})})", "M(()=>{var a=[];a.length=2**32})", "M(()=>{var a=[];a.length=-1})", "M(()=>{var a=[];a.length=1.5})", "M(()=>{var a=[];a.length='x'})", "M(()=>{var a=[];a.length={valueOf(){return -1}}})",
  "M(()=>{[].toSorted(1)})", "M(()=>{[].with(0,1)})", "M(()=>{[1].with(-2,1)})", "M(()=>{[1].toSpliced.call(null)})", "M(()=>{[].findLast(1)})", "M(()=>{[].flat.call(null)})", "M(()=>{[].copyWithin.call(null)})", "M(()=>{Array.prototype[Symbol.iterator].call(null)})", "M(()=>{[].entries.call(undefined)})",
  "M(()=>{Array.prototype.toString.call(null)})", "M(()=>{Array.prototype.toLocaleString.call(null)})", "M(()=>{[null].toLocaleString()})", "M(()=>{[{toLocaleString:1}].toLocaleString()})", "M(()=>{[{toString:1,valueOf:1}].join()})", "M(()=>{String({toString:1,valueOf:1})})", "M(()=>{({toString(){return {}},valueOf(){return {}}})+1})",
  "M(()=>{`${{toString(){return {}},valueOf(){return {}}}}`})", "M(()=>{({[Symbol.toPrimitive]:1})+1})", "M(()=>{({[Symbol.toPrimitive](){return {}}})+1})", "M(()=>{({[Symbol.toPrimitive](){return {}}})*1})", "M(()=>{Object.create(null)+''})", "M(()=>{String(Object.create(null))})", "M(()=>{Number(Object.create(null))})", "M(()=>{`${Object.create(null)}`})",
  "M(()=>{[Object.create(null)].join()})", "M(()=>{Object.create(null)>1})", "M(()=>{new Date(Object.create(null))})", "M(()=>{parseInt(Object.create(null))})", "M(()=>{isNaN(Symbol())})", "M(()=>{isFinite(1n)})", "M(()=>{parseFloat(Symbol())})", "M(()=>{Number.parseInt(Symbol())})",
  "M(()=>{Number(1n)+1;return 1})", "M(()=>{Number('1n')})", "M(()=>{var a=1n;a++;a+=1})", "M(()=>{var a=1n;a>>>=1n})", "M(()=>{1n<2})", "M(()=>{1n==1})", "M(()=>{1n+'a'})", "M(()=>{[1n].sort()})", "M(()=>{[1n,1].sort((a,b)=>a-b)})", "M(()=>{new Intl.PluralRules('en').select(1n)})",
  "M(()=>{structuredClone})", "M(()=>{Symbol.prototype.toString.call(1)})", "M(()=>{Symbol.prototype.valueOf.call({})})", "M(()=>{Symbol.prototype.description})", "M(()=>{Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get.call(1)})", "M(()=>{Symbol.for(Symbol())})", "M(()=>{Symbol.keyFor(1)})",
  "M(()=>{Boolean.prototype.toString.call(1)})", "M(()=>{Boolean.prototype.valueOf.call({})})", "M(()=>{BigInt.prototype.toString.call(1)})", "M(()=>{BigInt.prototype.valueOf.call({})})", "M(()=>{1n.toString(1)})", "M(()=>{1n.toString(37)})", "M(()=>{BigInt.prototype.toLocaleString.call(1)})",
  "M(()=>{Error.prototype.toString.call(1)})", "M(()=>{Error.prototype.toString.call(null)})", "M(()=>{Error.prototype.toString.call(undefined)})", "M(()=>{Error.prototype.toString.call('s')})", "M(()=>{Error.prototype.toString.call({name:Symbol()})})", "M(()=>{Error.prototype.toString.call({message:Symbol()})})",
  "M(()=>{Error.prototype.toString.call(Object.create(null))})", "M(()=>{Error.prototype.toString.call({get name(){throw new RangeError('n')}})})", "M(()=>{Error.captureStackTrace.call()})", "M(()=>{Error.call()})", "M(()=>{new Error(Symbol())})", "M(()=>{new Error({toString(){throw new RangeError('ts')}})})", "M(()=>{TypeError.prototype.name.x.y})",
  "M(()=>{throw 1})", "M(()=>{throw undefined})", "M(()=>{throw null})", "M(()=>{throw {}})", "M(()=>{throw new Error('e')})", "M(()=>{throw Object.create(null)})", "M(()=>{throw {message:'obj'}})", "M(()=>{throw {name:'N',message:'obj'}})", "M(()=>{throw 'str'})",
);

// Combinação valor x forma: cada forma é avaliada com `v` ligado a um valor.
const VALS = ["null", "undefined", "1", "'str'", "true", "({})", "[]", "Symbol('s')", "1n", "function fn(){}", "NaN", "0", "new Date(0)", "/r/", "class K{}", "Object.create(null)", "new Map", "-0", "1.5", "''"];
const SHAPES = [
  "v.x", "v[0]", "v['k-1']", "v[1+1]", "o.a.x", "o.a[0]", "f().x", "f()()", "f().m()", "v.m()", "v()", "v(1,2)", "o.a()", "o.a.b()", "new v", "new v()", "new o.a", "new o.a()", "new f()", "new (f())", "new v.x", "v.x=1", "v[0]=1",
  "o.a.x=1", "o.a[0]=1", "v.x++", "v.x+=1", "delete v.x", "[...v]", "[].concat(...v)", "Math.max(...v)", "for(var q of v);", "for(var q in v);", "var {a}=v", "var [a]=v", "var {a:{b}}={a:v}", "var [[a]]=[v]", "var {...r}=v", "(({a})=>a)(v)", "(([a])=>a)(v)",
  "v in o", "'x' in v", "o instanceof v", "v instanceof Object", "class C extends v{}", "[1].map(v)", "[1].forEach(v)", "[3,1].sort(v)", "Object.keys(v)", "Object.entries(v)", "Object.setPrototypeOf({},v)", "Object.setPrototypeOf(v,{})", "Object.create(v)",
  "Object.defineProperty(v,'a',{})", "Object.defineProperty({},'a',v)", "Object.freeze(v)", "Object.assign(v,{a:1})", "Reflect.get(v,'x')", "K(v)", "new Proxy(v,{})", "new Proxy({},v)", "new WeakMap().set(v,1)", "new WeakSet().add(v)", "new WeakRef(v)",
  "Symbol.keyFor(v)", "v.x?.y", "v?.x.y", "v?.x()", "v?.[0].y", "`${v}`", "v+''", "v+1", "+v", "-v", "v*1", "1n+v", "v**2", "~v", "v|0", "v<1", "v>>>1", "v++", "--v", "Number(v)", "String(v)", "BigInt(v)", "parseInt(v)", "new Array(v)", "Array(v)", "'x'.repeat(v)",
  "new Uint8Array(v)", "new ArrayBuffer(v)", "'a'.padStart(v)", "(1).toFixed(v)", "(1).toString(v)", "new Date(v).toISOString()", "Math.max(v)", "Math.abs(v)", "JSON.stringify(v)", "JSON.stringify({a:v})", "JSON.parse(v)", "Array.from(v)", "new Set(v)", "new Map(v)",
  "Promise.resolve.call(v)", "new Promise(v)", "Array.prototype.map.call(v,fn2)", "Array.prototype.push.call(v,1)", "String.prototype.trim.call(v)", "Function.prototype.call.call(v)", "Function.prototype.apply.call(f,null,v)", "Reflect.apply(v,null,[])", "Reflect.construct(v,[])",
  "Object.getPrototypeOf(v)", "Object.getOwnPropertyDescriptor(v,'a')", "v.constructor.name", "v.length", "v.toString()", "v.valueOf()", "v.toString.call(v)", "v[Symbol.iterator]()", "v[Symbol.toPrimitive]()", "v.hasOwnProperty('a')", "Object.prototype.hasOwnProperty.call(v,'a')",
  "[v].join()", "[v].toString()", "[v].toLocaleString()", "[v].sort()", "[v,1].sort((a,b)=>a-b)", "[v].includes(1)", "Object.groupBy(v,x=>x)", "Object.fromEntries(v)", "Object.fromEntries([v])", "new Map([v])", "new Map([[v,1]])", "Array.from({length:v})", "new Intl.NumberFormat(v)", "new Intl.DateTimeFormat(v)",
  "new Date(2020,v)", "new RegExp(v)", "new RegExp('a',v)", "'a'.replace(v,'b')", "'a'.split(v)", "'a'.match(v)", "'a'.search(v)", "'a'.indexOf(v)", "'a'.concat(v)", "'a'.localeCompare(v)", "'a'.at(v)", "[1].at(v)", "[1].slice(v)", "[1].fill(v)", "[1].indexOf(v)", "[1].join(v)", "[1].flat(v)",
  "label: for(var q of v) break label", "(function(){return v.x})()", "(()=>v())()", "(async()=>{await v.x})()", "(function*(){yield* v})().next()", "(function*(){yield* v.x})().next()", "({...v}).x.y", "({[v]:1})", "({a:1})[v]", "({}).x[v]", "o[v]", "o[v].x", "o.a[v]",
];
for (const v of VALS) {
  for (const s of SHAPES) {
    add(`M(()=>{var v=${v};var o={a:v,b:{}};var f=()=>v;var fn2=function(){};${s}})`);
  }
}
// As mesmas formas em modo estrito (muda mensagem de atribuição e de delete).
for (const v of ["null", "undefined", "1", "'str'", "Symbol('s')", "Object.freeze({a:1})", "Object.freeze([1])"]) {
  for (const s of ["v.x=1", "v[0]=1", "v.a=2", "v.length=0", "delete v.a", "delete v[0]", "v.a++", "v.a+=1", "v.x=v", "o.a.x=1", "o.a.a=1", "v[Symbol.iterator]=1", "v['k-1']=1", "v.push(1)", "Object.defineProperty(v,'a',{value:3})"]) {
    add(`M(()=>{'use strict';var v=${v};var o={a:v};${s}})`);
  }
}

// ---- Execução.
const baseSources = [];
for (const file of ["error_api_bun.tsv", "error_bun.tsv", "error_message_bun.tsv", "errors_bun.tsv", "error_stack_bun.tsv", "dispose_bun.tsv", "function_error_bun.tsv", "stack_bun.tsv", "stack_more_bun.tsv", "stack_format_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
      if (!line) continue;
      try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
    }
  } catch (e) {}
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "error-edge-golden-"));
const runner = path.join(dir, "runner.js");
fs.writeFileSync(
  runner,
  `const fs=require("fs");try{require("node:vm").runInThisContext(fs.readFileSync(process.argv[2],"utf8"),{filename:"x.js"})}catch(e){}\n` +
    `process.on("exit",()=>{process.stdout.write("\\u0001"+JSON.stringify(globalThis.R===undefined?"<undefined>":String(globalThis.R))+"\\n")})\n`,
);

function runOne(index, source) {
  return new Promise(resolve => {
    const file = path.join(dir, `case${index}.js`);
    fs.writeFileSync(file, source);
    const child = spawn(process.execPath, [runner, file], { cwd: dir });
    let out = "";
    child.stdout.on("data", chunk => (out += chunk));
    child.stderr.on("data", () => {});
    const timer = setTimeout(() => child.kill("SIGKILL"), 10000);
    child.on("close", () => {
      clearTimeout(timer);
      fs.rmSync(file, { force: true });
      const marked = out.split("\n").find(line => line.startsWith("\u0001"));
      resolve(marked ? JSON.parse(marked.slice(1)) : null);
    });
  });
}

(async () => {
  let kept = 0;
  let dropped = 0;
  let dup = 0;
  const jobs = [];
  for (const expr of unique) {
    if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
    jobs.push({ expr, source: PRELUDE + `globalThis.R = ${expr};` });
  }
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 8 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runOne(i, jobs[i].source);
    }
  });
  await Promise.all(workers);
  const lines = [];
  jobs.forEach((job, i) => {
    const result = results[i];
    if (result === null) { dropped++; process.stderr.write("sem resultado: " + JSON.stringify(job.expr).slice(0, 140) + "\n"); return; }
    if (result === "<undefined>" && /^T\(.*Probe/.test(job.expr)) { dropped++; return; }
    if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\/|\bbun\b/i.test(result)) { dropped++; process.stderr.write("caminho ou marca: " + JSON.stringify(job.expr).slice(0, 140) + "\n"); return; }
    kept++;
    lines.push(JSON.stringify(job.source) + "\t" + JSON.stringify(result));
  });
  process.stdout.write(emitFactoredLines("error_edge", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
  fs.rmSync(dir, { recursive: true, force: true });
})();
