// Gera tests/golden/generator_state_bun.tsv: generators síncronos em grade, medido no bun 1.4.2.
// Cobre next/return/throw em cada estado (suspendedStart, suspendedYield, executing com reentrância, completed),
// yield dentro de try/catch/finally com return() e throw() injetados, yield* para iteradores com e sem return/throw
// (o TypeError quando throw falta e o fechamento do iterador interno), argumentos de next, generator methods,
// computed e static, generator como construtor, prototype de generator function, `this`, `arguments`, recursão via
// yield* e as mensagens exatas ("Generator is already running" e companhia).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Um bun filho novo por programa, no máximo 8 em paralelo, timeout de 5 s por filho.
// Uso: bun scripts/gen-generator-state-golden.js > tests/golden/generator_state_bun.tsv
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
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":String(v);if(Array.isArray(v))return "["+v.map(S).join(",")+"]";' +
  'return "{"+Object.keys(v).map(k=>k+":"+S(v[k])).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'var it,L=[];function C(m,a){L.push(m+(arguments.length>1?"("+S(a)+")":"")+"="+T(()=>it[m](a)))}\n' +
  'function J(){var r=L.join(" | ");L=[];return r}\n';

// Cada caso: { body, sloppy }. O corpo é de uma arrow e devolve a string do resultado.
const cases = [];
const add = (...bodies) => bodies.forEach(body => cases.push({ body, sloppy: false }));
const addSloppy = (...bodies) => bodies.forEach(body => cases.push({ body, sloppy: true }));

// ---- 1. Grade de estados: gerador x preparo x operação, seguida de next, next e return(0).
const grid1Gens = [
  "function*(){}",
  "function*(){return 1}",
  "function*(){yield 1;yield 2;return 3}",
  "function*(){var x=yield 1;var y=yield x;return [x,y]}",
  "function*(){try{yield 1;yield 2}catch(e){yield 'c'+e;yield 'd'}return 9}",
  "function*(){try{yield 1;yield 2}finally{yield 'f';yield 'g'}return 9}",
  "function*(){try{yield 1}finally{return 'ov'}}",
  "function*(){try{yield 1}finally{throw 'ft'}}",
  "function*(){throw 5}",
  "function*(){yield* [1,2];return 4}",
  "function*(){it.next()}",
  "function*(){yield 1;it.next()}",
  "function*(){try{yield 1}catch(e){it.throw('x')}}",
  "function*(){while(true){try{yield 1}catch(e){yield 'caught'}}}",
  "function*(){try{try{yield 1}finally{yield 'in'}}finally{yield 'out'}}",
  "function*(){yield;yield undefined;yield yield 1}",
];
const preps = [
  [], ['C("next")'], ['C("next")', 'C("next")'], ['C("next")', 'C("next")', 'C("next")'],
  ['C("next")', 'C("next")', 'C("next")', 'C("next")', 'C("next")'], ['C("return")'], ['C("return",1)'], ['C("throw",2)'],
  ['C("next")', 'C("return",3)'], ['C("next")', 'C("throw",4)'], ['C("next")', 'C("next")', 'C("return",3)'], ['C("next")', 'C("next")', 'C("throw",4)'],
];
const ops = [
  'C("next")', 'C("next",7)', 'C("return")', 'C("return",8)', 'C("throw")', 'C("throw",9)', 'C("throw",new RangeError("r"))',
];
for (const g of grid1Gens) for (const p of preps) for (const o of ops) {
  add(`it=(${g})();${p.join(";")};${o};C("next");C("next");C("return",0);return J()`);
}

// ---- 2. yield dentro de try/catch/finally com return() e throw() injetados em cada ponto.
const tcfs = [
  "try{yield 1;yield 2}catch(e){yield 'c'}finally{yield 'f'}",
  "try{yield 1}catch(e){yield 'c'+e;yield 'c2'}finally{yield 'f'}",
  "try{yield 1}finally{yield 'f1';yield 'f2'}",
  "try{yield 1}catch(e){yield 'c'}",
  "try{try{yield 1}finally{yield 'in'}}catch(e){yield 'out'+e}",
  "try{try{yield 1}catch(e){yield 'in'+e}}finally{yield 'fin'}",
  "try{yield 1}finally{try{yield 'f'}finally{yield 'ff'}}",
  "try{yield 1}finally{return 'ov'}",
  "try{yield 1}finally{throw 'ft'}",
  "try{yield 1}catch(e){throw 'ct'+e}finally{yield 'f'}",
  "for(var i=0;i<3;i++){try{yield i}finally{yield 'f'+i}}",
  "for(var i=0;i<3;i++){try{yield i;continue}finally{yield 'f'+i}}",
  "a:for(var i=0;i<2;i++){try{yield i;break a}finally{yield 'f'+i}}",
  "try{yield 1}finally{for(var i=0;i<2;i++)yield 'l'+i}",
  "try{yield* [1,2]}finally{yield 'f'}",
  "try{yield 1}catch(e){}finally{yield 'f'}yield 'after'",
  "var r=0;try{r=yield 1}finally{L.push('fin:'+r)}yield r",
  "try{return yield 1}finally{yield 'f'}",
];
const injects = [
  'C("return",5)', 'C("return")', 'C("throw","E")', 'C("throw",new TypeError("t"))', 'C("next",9)', 'C("return",{a:1})', 'C("throw")',
];
for (const t of tcfs) for (let n = 0; n < 5; n++) for (const inj of injects) {
  add(`it=(function*(){${t};return 'end'})();${'C("next");'.repeat(n)}${inj};C("next");C("next");C("next");return J()`);
}

// ---- 3. yield* para iteradores com e sem return/throw.
const rets = [
  "", "return(v){L.push('r:'+S(v));return {value:'R'+v,done:true}}", "return(v){L.push('r');return 1}",
  "return(v){L.push('r');return {done:false,value:'nd'}}", "return(v){L.push('r');throw 'rt'}", "return:null", "return:undefined", "return:5",
  "return(v){return {get done(){L.push('gd');return true},get value(){L.push('gv');return 'V'}}}",
];
const thrs = [
  "", "throw(v){L.push('t:'+S(v));return {value:'T'+v,done:false}}", "throw(v){L.push('t:'+S(v));return {value:'T'+v,done:true}}",
  "throw(v){L.push('t');return 1}", "throw(v){L.push('t');throw 'tt'}", "throw:null", "throw:5",
];
const nextOk = "next(v){L.push('n:'+S(v));k++;return {value:k,done:k>3}}";
const mkInner = (n, r, t) => `var k=0;var inner={[Symbol.iterator](){L.push('iter');return this},${[n, r, t].filter(Boolean).join(",")}};`;
const wraps = [
  b => `function*(){${b}var r=yield* inner;L.push('r='+S(r));return r}`,
  b => `function*(){${b}try{var r=yield* inner;return r}catch(e){L.push('catch '+S(e));yield 'afterCatch'}finally{L.push('fin')}}`,
];
const steps3 = [];
for (const n of [0, 1, 2]) for (const op of ['C("return",5)', 'C("throw",6)', 'C("next",7)', 'C("return")', 'C("throw")'])
  steps3.push(`${'C("next",1);'.repeat(n)}${op};C("next",2);C("next",3)`);
for (const r of rets) for (const t of thrs) for (const w of wraps) for (const s of steps3) {
  add(`${mkInner(nextOk, r, t)}it=(${w("")})();${s};return J()`);
}
const nexts = [
  "next(v){L.push('n');return 1}", "next(v){L.push('n');throw 'nt'}", "next(v){L.push('n');return {get done(){throw 'gd'},value:1}}",
  "next(v){L.push('n');return {done:false,get value(){L.push('gv');return 'V'}}}", "next:5", "next:undefined",
  "next(v){L.push('n');return {done:true,value:'last'}}", "next(v){L.push('n');return null}",
];
for (const n of nexts) for (const w of wraps) for (const s of steps3) {
  add(`var k=0;var inner={[Symbol.iterator](){return this},${n},return(v){L.push('r');return {}},throw(v){L.push('t');return {done:true}}};it=(${w("")})();${s};return J()`);
}
// Iteráveis de várias origens e Symbol.iterator quebrado.
const sources = [
  "[1,2,3]", "'ab'", "new Set([1,2])", "new Map([[1,2]])", "(function*(){yield 'a';return 'b'})()", "(function*(){try{yield 1;yield 2}finally{L.push('innerfin')}})()",
  "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){throw 'si'}}", "{}", "1", "null", "undefined",
  "{[Symbol.iterator](){return {next(){return {done:true,value:'z'}}}}}", "(function*(){return 'only'})()", "[][Symbol.iterator]()",
];
for (const src of sources) for (const s of steps3) {
  add(`it=(function*(){var inner=${src};try{var r=yield* inner;L.push('r='+S(r));return r}catch(e){L.push('catch '+(e&&e.name||e));throw e}finally{L.push('fin')}})();${s};return J()`);
}

// ---- 4. Argumentos de next e formas de yield como expressão.
const yieldForms = [
  "var a=yield 1;L.push('a='+S(a));var b=yield 2;L.push('b='+S(b));return [a,b]",
  "var a=yield;return a", "return yield yield 1", "return [yield 1,yield 2,yield 3]", "return {a:yield 1,b:yield 2}",
  "return (yield 1)+(yield 2)", "return `${yield 1}-${yield 2}`", "var o={};o[yield 1]=yield 2;return o",
  "return f(yield 1,yield 2);function f(a,b){return [a,b]}", "var [x,y]=[yield 1,yield 2];return x+y",
  "return (yield 1)?(yield 'a'):(yield 'b')", "return (yield 1)||(yield 2)", "return (yield 1)&&(yield 2)", "return (yield 1)??(yield 2)",
  "var {a=yield 'd'}={};return a", "return typeof (yield 1)", "return !(yield 1)", "return [...(yield 1)]", "return new Array(yield 1,yield 2)",
  "switch(yield 1){case (yield 2):return 'm';default:return 'd'}", "for(var i=yield 1;i<(yield 2);i++)L.push('i'+i);return i",
  "if(yield 1)return yield 2;return yield 3", "while(yield 1)L.push('w');return 'out'", "do{L.push('d')}while(yield 1);return 'out'",
  "var x=1;x+=yield 1;return x", "var o={};o.p??=yield 1;return o", "return yield* [yield 1]", "return (yield 1)**(yield 2)",
];
const argSeqs = [
  [], [undefined], [1], [1, 2], [1, 2, 3], [0, '', null], [NaN, -0, 1n === 1n], [[1, 2], [3]], [{ a: 1 }, 'x'],
];
for (const f of yieldForms) for (const a of argSeqs) {
  const calls = ['C("next",111)'].concat(a.map(v => `C("next",${JSON.stringify(v) === undefined ? "undefined" : Object.is(v, -0) ? "-0" : typeof v === "number" && v !== v ? "NaN" : JSON.stringify(v)})`), ['C("next",5)']);
  add(`it=(function*(){${f}})();${calls.join(";")};return J()`);
}

// ---- 5. Generator methods, computed, static, e prototype de generator function.
const targets = [
  "function*(){}", "function*f(a,b){}", "({*m(a){}}).m", "(class{static *s(a,b,c){}}).s", "({*[Symbol.iterator](){}})[Symbol.iterator]", "({*['x'+1](){}}).x1",
  "(class{*i(){}}).prototype.i", "({async *a(){}}).a", "(function*(a,b=1,c){}) ", "(function*(...r){})", "({*'s t'(){}})['s t']", "({*5(){}})[5]",
];
const reflOps = [
  "typeof g", "g.name", "g.length", "String(Object.getOwnPropertyNames(g))", "String(Reflect.ownKeys(g).map(String))",
  "S(Object.getOwnPropertyDescriptor(g,'prototype'))", "Object.hasOwn(g,'prototype')", "typeof g.prototype", "Object.getPrototypeOf(g.prototype)===Object.getPrototypeOf(function*(){}).prototype",
  "Object.hasOwn(g.prototype,'constructor')", "Object.getOwnPropertyNames(g.prototype).length", "Object.prototype.toString.call(g)",
  "Object.prototype.toString.call(g())", "g()[Symbol.toStringTag]", "g()[Symbol.iterator]()===g()", "(x=>x[Symbol.iterator]()===x)(g())",
  "Object.getPrototypeOf(g())===g.prototype", "g() instanceof g", "Function.prototype.toString.call(g).length>0",
  "(function(){var f=g;f.prototype=null;return Object.getPrototypeOf(f())===Object.getPrototypeOf(function*(){}).prototype})()",
  "(function(){var f=g;f.prototype=5;return Object.getPrototypeOf(f())===Object.getPrototypeOf(function*(){}).prototype})()",
  "(function(){var f=g;var p={x:1};f.prototype=p;return Object.getPrototypeOf(f())===p&&f().x})()",
  "(function(){var f=g;f.prototype={next(){return 'mine'}};return f().next()})()",
  "(function(){var f=g;f.prototype=Object.create(Object.getPrototypeOf(function*(){}).prototype);var i=f();return S(i.next())})()",
  "(function(){var f=g;delete f.prototype;return Object.getPrototypeOf(f())===Object.getPrototypeOf(function*(){}).prototype})()",
  "Object.getPrototypeOf(g)===Object.getPrototypeOf(function*(){})", "Object.getPrototypeOf(g)===Function.prototype",
  "g.hasOwnProperty('caller')||g.hasOwnProperty('arguments')", "(function(){try{return g.caller}catch(e){return e.name+': '+e.message}})()",
  "S(Object.getOwnPropertyDescriptor(g,'name'))", "S(Object.getOwnPropertyDescriptor(g,'length'))", "g.bind().name", "typeof g.call", "g.constructor.name",
];
for (const t of targets) for (const o of reflOps) {
  add(`var g=${t.trim()};return ${o}`);
}
const GF = "Object.getPrototypeOf(function*(){}).constructor";
const GP = "Object.getPrototypeOf(function*(){})";
const GEN = "Object.getPrototypeOf(function*(){}).prototype";
add(
  `return ${GF}.name`, `return ${GF}.length`, `return typeof ${GF}`, `return ${GF}===Function`, `return Object.getPrototypeOf(${GF})===Function`,
  `return ${GF}.prototype===${GP}`, `return ${GP}[Symbol.toStringTag]`, `return String(Reflect.ownKeys(${GP}).map(String))`,
  `return String(Reflect.ownKeys(${GEN}).map(String))`, `return ${GEN}[Symbol.toStringTag]`, `return ${GP}.prototype===${GEN}`,
  `return S(Object.getOwnPropertyDescriptor(${GP},'prototype'))`, `return S(Object.getOwnPropertyDescriptor(${GEN},'constructor'))`,
  `return S(Object.getOwnPropertyDescriptor(${GP},'constructor'))`, `return S(Object.getOwnPropertyDescriptor(${GEN},Symbol.toStringTag))`,
  `return S(Object.getOwnPropertyDescriptor(${GEN},'next'))`, `return S(Object.getOwnPropertyDescriptor(${GEN},'return'))`, `return S(Object.getOwnPropertyDescriptor(${GEN},'throw'))`,
  `return ${GEN}.next.length+${GEN}.return.length+${GEN}.throw.length`, `return ${GEN}.next.name+${GEN}.return.name+${GEN}.throw.name`,
  `return Object.getPrototypeOf(${GEN})===Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))`,
  `return typeof Object.getPrototypeOf(${GEN})[Symbol.iterator]`, `return Object.getPrototypeOf(${GEN})[Symbol.iterator].call(5)`,
  `var g=new ${GF}('a','b','yield a;yield b;return a+b');return S([...g(1,2)])+g.name+g.length`, `var g=${GF}('yield 1');return S([...g()])`,
  `return new ${GF}('yield 1').toString()`, `return ${GF}('a,b','yield a').length`, `return ${GF}('a','b','return 1').toString()`,
  `return new ${GF}('yield*')`, `return new ${GF}('var yield')`, `return new ${GF}('yield=1')`, `return ${GF}('yield\\n1')().next().value`,
  `return ${GF}('}function*(){')`, `return ${GF}('a=yield','return a')`, `return ${GF}('yield','return 1')`, `return ${GF}('...r','yield r.length')(1,2,3).next().value`,
  `var o=Object.create(${GEN});return o.next()`, `return ${GEN}.next()`, `return ${GEN}.return(1)`, `return ${GEN}.throw(1)`,
  `return Object.prototype.toString.call(${GEN})`, `return Object.prototype.toString.call(${GP})`, `return String(${GP})`, `return String(function*(a){yield a})`,
  `return String(function*  g ( ) { } )`, `return String(({*m(){}}).m)`, `return String((class{static*s(){}}).s)`, `return String(({*[1+1](){}})[2])`,
);

// ---- 6. Generator como construtor e receptores inválidos de next/return/throw.
const ctorBodies = [];
for (const t of ["function*(){}", "function*(){yield 1}", "({*m(){}}).m", "(class{static *s(){}}).s", "(function*(){}).bind()", "Object.getPrototypeOf(function*(){}).constructor('yield 1')"]) {
  ctorBodies.push(
    `var g=${t};return new g`, `var g=${t};return new g()`, `var g=${t};return Reflect.construct(g,[])`, `var g=${t};return Reflect.construct(function(){},[],g)`,
    `var g=${t};class C extends g{};return new C`, `var g=${t};return Reflect.construct(g,[],function(){})`, `var g=${t};return new g.prototype.constructor`,
    `var g=${t};return typeof Reflect.construct(Object,[],g)`, `var g=${t};return new new Proxy(g,{})`, `var g=${t};return Reflect.getPrototypeOf(g)===Function.prototype`,
    `var g=${t};return new (g.bind())`, `var g=${t};return new (g.bind(null,1))`, `var g=${t};return g.call({}).next().done`, `var g=${t};return g.apply(null,[]).toString()`,
    `var g=${t};return class extends g{}`, `var g=${t};class C{static m=g};return new C.m`,
  );
}
add(...ctorBodies);
const recv = ["undefined", "null", "1", "'s'", "{}", "[]", "function*(){}", "(function*(){})", "Object.create(" + GEN + ")", "(async function*(){})()", "new Proxy({}, {})", "Symbol()", "(function*(){})().next", "[][Symbol.iterator]()", "new Map().entries()"];
for (const r of recv) for (const m of ["next", "return", "throw"]) for (const a of ["", "1"]) {
  add(`return ${GEN}.${m}.call(${r}${a ? "," + a : ""})`, `return ${GEN}.${m}.apply(${r},[${a}])`, `return Reflect.apply(${GEN}.${m},${r},[${a}])`);
}
add(
  "var g=(function*(){yield 1})();var n=g.next;return n.call((function*(){yield 2})()).value",
  "var g=(function*(){yield 1})();return g.next.call(g).value+g.next.call(g).done",
  "var g=(function*(){yield 1})();g.next=5;return S(g.next)",
  "var g=(function*(){yield 1})();g.next=function(){return 'own'};return g.next()",
  "var g=(function*(){yield 1})();Object.freeze(g);return S(g.next())",
  "var g=(function*(){yield 1})();g.x=1;return Object.keys(g).join()+Object.getOwnPropertyNames(g).length",
  "var g=(function*(){yield 1})();return Reflect.ownKeys(g).length",
  "var g=(function*(){yield 1})();return JSON.stringify(g)",
  "var g=(function*(){yield 1})();return String(g)",
  "var g=(function*(){yield 1})();return `${g}`",
  "var g=(function*(){yield 1})();return g+''",
  "var g=(function*(){yield 1})();return typeof g+Object.isExtensible(g)",
  "var g=(function*(){yield 1})();return Object.getPrototypeOf(Object.getPrototypeOf(g))===" + GEN,
);

// ---- 7. this, arguments, new.target, parâmetros.
const thisBodies = [
  "function* g(){yield this}var r=g.call(5).next().value;return typeof r+S(r)",
  "function* g(){yield typeof this}return g.call(5).next().value",
  "function* g(){yield this===undefined}return g.call(undefined).next().value",
  "function* g(){yield this===globalThis}return g.call(undefined).next().value",
  "function* g(){yield this===globalThis}return g().next().value",
  "function* g(){yield this}var o={g};return o.g().next().value===o",
  "function* g(){yield this}var o={g};var it=o.g();return it.next.call(it).value===o",
  "function* g(){yield this.x;yield this.x}var o={g,x:1};var i=o.g();var a=i.next().value;o.x=2;return a+','+i.next().value",
  "function* g(){yield ()=>this}var o={g};return o.g().next().value()===o",
  "function* g(){var f=function(){return this};yield f()===globalThis}return g.call({}).next().value",
  "function* g(){yield eval('this')}var o={};return g.call(o).next().value===o",
  "var o={*m(){yield super.toString===Object.prototype.toString}};return o.m().next().value",
  "class A{m(){return 'A'}}class B extends A{*g(){yield super.m();yield super.m}}return S([...new B().g()].map(String))",
  "class A{static s(){return 'S'}}class B extends A{static *g(){yield super.s()}}return [...B.g()].join()",
  "function* g(){yield new.target}return String(g().next().value)",
  "function* g(){yield arguments.length;yield arguments[0];yield typeof arguments}return S([...g(7,8)])",
  "function* g(a){arguments[0]=9;yield a;a=3;yield arguments[0]}return S([...g(1)])",
  "function* g(a){'use strict';arguments[0]=9;yield a;a=3;yield arguments[0]}return S([...g(1)])",
  "function* g(a){yield arguments;}var i=g(1,2);var a=i.next().value;return Object.prototype.toString.call(a)+a.length",
  "function* g(a,b=2){yield [a,b,arguments.length]}return S(g(1).next().value)",
  "function* g(){yield arguments.callee}return typeof g().next().value",
  "function* g(a=L.push('param'),b=L.push('param2')){L.push('body');yield 1}var it=g();L.push('after call');it.next();return J()",
  "function* g(a=(()=>{throw 'pe'})()){yield 1}var r;try{g()}catch(e){r='threw '+e}return r",
  "function* g({a}){yield a}var r;try{g()}catch(e){r=e.name}return r",
  "function* g(...r){yield r}return S(g(1,2,3).next().value)",
  "function* g(a){yield a;var a;yield a}return S([...g(1)])",
  "function* g(a){yield ()=>a;a=2}var i=g(1);var f=i.next().value;i.next();return f()",
  "function* g(){yield g}return g().next().value===g",
  "var g=function*h(){yield h};return g().next().value===g",
  "var g=function*h(){h=1;yield typeof h};return g().next().value",
  "var g=function*h(){'use strict';h=1;yield typeof h};return g().next().value",
  "function* g(){yield 1;return 2}var i=g();return S([i.next(),i.next(),i.next()])",
  "function* g(){let x=1;{let x=2;yield x}yield x}return S([...g()])",
  "function* g(){var f=()=>{try{yield}catch(e){}}}",
  "function* yield_(){}return typeof yield_",
  "function* g(){var yield_=1;yield yield_}return S([...g()])",
  "var o={yield:5};function* g(){yield o.yield}return S([...g()])",
  "function* g(){yield\n5}return S([...g()])",
  "function* g(){yield* [1,2]\n+3}return S([...g()])",
  "function* g(){return}return S(g().next())",
  "function* g(){return\n5}return S(g().next())",
  "function* g(){yield /x/g}return String(g().next().value)",
  "function* g(){yield\n/x/g}return String(g().next().value)",
  "function* g(){yield [1]}return S(g().next().value)",
  "function* g(){yield {}}return S(g().next().value)",
  "function* g(){yield function(){}}return typeof g().next().value",
  "function* g(){yield class{}}return typeof g().next().value",
  "function* g(){yield `t${yield 1}`}var i=g();i.next();return i.next('x').value",
  "var g=function*(){yield 1}.bind({});return S([...g()])",
  "function* g(){yield 1}g.prototype.extra=function(){return 'e'};return g().extra()",
  "function* g(){yield 1}var i=g();return Object.getPrototypeOf(i)===g.prototype&&i.hasOwnProperty('extra')",
  "async function* ag(){}return Object.prototype.toString.call(ag())",
];
add(...thisBodies);
addSloppy(
  "function* g(){yield this===globalThis}return g.call(undefined).next().value",
  "function* g(){yield typeof this}return g.call(5).next().value",
  "function* g(){yield this}var r=g.call('s').next().value;return typeof r",
  "function* g(){yield this===globalThis}return g.call(null).next().value",
  "function* g(){yield this===globalThis}return g().next().value",
  "function* g(a){arguments[0]=9;yield a;a=3;yield arguments[0]}return S([...g(1)])",
  "function* g(a){yield arguments.length;arguments.length=0;yield arguments.length}return S([...g(1,2)])",
  "function* g(a,b){yield [a,b];arguments[1]=7;yield [a,b]}return S([...g(1)])",
  "function* g(a,b){yield [a,b];arguments[1]=7;yield [a,b]}return S([...g(1,2)])",
  "function* g(){yield typeof arguments.callee}return g().next().value",
  "function* g(){yield arguments.callee===g}return g().next().value",
  "function* g(){yield g.caller}return g().next().value",
  "function* g(){var yield_=1;yield yield_}return S([...g()])",
  "function* g(){eval('var ev=5');yield typeof ev}return g().next().value",
  "function* g(){yield eval('typeof arguments')}return g(1).next().value",
  "function* g(){with({a:1}){yield a}}return g().next().value",
  "function* g(){with({a:1}){yield a;yield a+1}}return S([...g()])",
  "var yield=3;function* g(){yield 1}return S([...g()])+yield",
  "function g(){var yield=2;return yield}return g()",
  "function* g(){yield 1}return typeof g.arguments+typeof g.caller",
);

// ---- 8. Recursão via yield*.
const rec = [];
for (let n = 0; n <= 6; n++) {
  rec.push(
    `function* r(n){if(n>0){yield n;yield* r(n-1)}}return S([...r(${n})])`,
    `function* r(n){if(n==0)return 0;var v=yield* r(n-1);yield n;return v+n}var i=r(${n});var o=[];var x;while(!(x=i.next()).done)o.push(x.value);return S(o)+x.value`,
    `function* f(n){if(n<2){yield n;return n}var a=yield* f(n-1);var b=yield* f(n-2);return a+b}var i=f(${n});var x,c=0;while(!(x=i.next()).done)c++;return c+':'+x.value`,
    `function* p(a,k){if(k==a.length){yield a.join('');return}for(var i=k;i<a.length;i++){[a[k],a[i]]=[a[i],a[k]];yield* p(a,k+1);[a[k],a[i]]=[a[i],a[k]]}}return S([...p([1,2,3,4,5,6].slice(0,${n}),0)].length)`,
    `function* r(n){try{if(n>0)yield* r(n-1);yield 'v'+n}finally{L.push('f'+n)}}var i=r(${n});C("next");C("return",1);return J()`,
    `function* r(n){try{if(n>0)yield* r(n-1);else yield 'base'}finally{L.push('f'+n)}}it=r(${n});C("next");C("throw","x");C("next");return J()`,
    `function* r(n){try{if(n>0)yield* r(n-1);else throw 'deep'}catch(e){yield 'c'+n+e}}return S([...r(${n})])`,
    `function* tree(t){if(t===null)return;yield* tree(t.l);yield t.v;yield* tree(t.r)}function mk(n){return n==0?null:{l:mk(n-1),v:n,r:mk(n-1)}}return S([...tree(mk(${Math.min(n, 4)}))])`,
    `function* fl(a){for(var x of a){if(Array.isArray(x))yield* fl(x);else yield x}}function nest(n){return n==0?[0]:[n,nest(n-1),n]}return S([...fl(nest(${n}))])`,
    `function* r(n){if(n==0){var x=yield 'leaf';return x}return yield* r(n-1)}it=r(${n});C("next");C("next","back");return J()`,
    `function* a(n){yield 'a'+n;if(n>0)yield* b(n-1)}function* b(n){yield 'b'+n;if(n>0)yield* a(n-1)}return S([...a(${n})])`,
    `var depth=0;function* r(n){depth++;if(n>0)yield* r(n-1);yield depth}return S([...r(${n})])`,
    `function* r(n){if(n==0){it.next();return}yield* r(n-1)}it=r(${n});C("next");C("next");return J()`,
    `function* r(n){yield n;if(n>0)return yield* r(n-1)}it=r(${n});for(var i=0;i<${n}+3;i++)C("next",i);return J()`,
  );
}
add(...rec);
// Recursão profunda (estouro de pilha vira RangeError) e delegação em cadeia longa.
add(
  "function* r(n){if(n>0)yield* r(n-1);else yield 'end'}var c=0;for(var v of r(200))c++;return c",
  "function* r(n){if(n>0)yield* r(n-1);else yield 'end'}return S([...r(1000)])",
  "function* r(){yield* r()}try{r().next()}catch(e){return e.name}return 'ok'",
  "function* r(){yield* r()}try{r().next()}catch(e){return e instanceof RangeError}return 'ok'",
  "function* r(n){return n==0?'z':yield* r(n-1)}var i=r(50);return S(i.next())",
  "function* r(n){if(n>0)yield* r(n-1);yield n}var s=0;for(var v of r(100))s+=v;return s",
);

// ---- 9. Reentrância: o estado executing e a mensagem "already running".
const reLocs = [
  "it.%M%", "yield it.%M%", "try{it.%M%}catch(e){L.push('c:'+e.name+':'+e.message)}yield 'after'", "try{yield 1}finally{it.%M%}",
  "try{yield 1}catch(e){it.%M%}", "var x={get p(){return it.%M%}};x.p", "[1].forEach(()=>it.%M%)", "yield* {[Symbol.iterator](){return this},next(){return it.%M%}}",
  "yield* {[Symbol.iterator](){it.%M%;return [][Symbol.iterator]()}}", "for(var v of {[Symbol.iterator](){return {next(){return it.%M%}}}})break",
  "var g=function*(){it.%M%};g().next()", "L.push(S([...{[Symbol.iterator](){it.%M%;return [][Symbol.iterator]()}}]))", "String({toString(){return it.%M%}})",
  "new Promise(r=>r(it.%M%))", "Array.from({length:1,get 0(){return it.%M%}})", "JSON.stringify({toJSON(){return it.%M%}})",
  "({[{toString(){return it.%M%}}]:1})", "`${{toString(){return it.%M%}}}`", "it.%M%;it.%M%",
];
const reMs = ["next()", "next(1)", "return()", "return(1)", "throw()", "throw(1)", "throw(new Error('e'))"];
for (const loc of reLocs) for (const m of reMs) {
  const code = loc.replace(/%M%/g, m);
  add(`it=(function*(){${code};return 'done'})();C("next");C("next");C("next");return J()`);
  add(`it=(function*(){yield 0;${code};return 'done'})();C("next");C("next");C("throw","T");C("next");return J()`);
}
add(
  "it=(function*(){yield it.next()})();return T(()=>it.next())",
  "it=(function*(){try{it.next()}catch(e){yield e.constructor===TypeError}})();return S(it.next())",
  "it=(function*(){try{it.next()}catch(e){yield e.message}})();return S(it.next())",
  "it=(function*(){try{it.return(1)}catch(e){yield e.message}})();return S(it.next())",
  "it=(function*(){try{it.throw(1)}catch(e){yield e.message}})();return S(it.next())",
  "it=(function*(){try{it.next()}finally{L.push('fin')}})();C('next');C('next');return J()",
  "it=(function*(){it.next()})();C('next');C('next');C('next');return J()",
  "var a=(function*(){yield b.next()})();var b=(function*(){yield a.next()})();return T(()=>a.next())",
  "var a=(function*(){yield 1;yield b.next()})();var b=(function*(){yield a.next()})();a.next();return T(()=>a.next())",
  "var a=(function*(){yield* b})();var b=(function*(){yield a.next()})();return T(()=>a.next())",
  "var a=(function*(){yield* b})();var b=(function*(){yield 1;yield a.next()})();return S(a.next())+T(()=>a.next())",
  "var a=(function*(){yield* a})();return T(()=>a.next())",
  "it=(function*(){yield* it})();return T(()=>it.next())",
  "it=(function*(){yield* it})();return T(()=>it.return(1))",
  "it=(function*(){yield* it})();return T(()=>it.throw(1))",
  "it=(function*(){yield 1;yield* it})();it.next();return T(()=>it.next())+T(()=>it.next())",
  "it=(function*(){try{yield 1}finally{it.next()}})();it.next();return T(()=>it.return(1))+T(()=>it.next())",
  "it=(function*(){for(var x of it){}})();return T(()=>it.next())",
  "it=(function*(){yield [...it]})();return T(()=>it.next())",
  "it=(function*(){var [a]=it})();return T(()=>it.next())",
  "it=(function*(){yield Array.from(it)})();return T(()=>it.next())",
  "it=(function*(){yield Math.max(...it)})();return T(()=>it.next())",
  "it=(function*(){yield new Set(it)})();return T(()=>it.next())",
  "it=(function*(){yield Object.fromEntries(it)})();return T(()=>it.next())",
  "it=(function*(){yield Promise.all(it)})();return T(()=>it.next())",
);

// ---- 10. Consumidores (for-of, spread, desestruturação) e o fechamento do gerador.
const consGens = [
  "function*(){try{yield 1;yield 2;yield 3}finally{L.push('closed')}}",
  "function*(){try{yield 1;yield 2;yield 3}finally{L.push('closed');yield 'extra'}}",
  "function*(){try{yield 1;yield 2;yield 3}finally{throw 'fin'}}",
  "function*(){try{yield 1;yield 2;yield 3}catch(e){L.push('caught '+e)}finally{L.push('closed')}}",
  "function*(){yield 1;yield 2;yield 3}",
  "function*(){yield 1;throw 'boom'}",
  "function*(){yield 1;return 'ret'}",
  "function*(){yield* [1,2,3]}",
  "function*(){try{yield* (function*(){try{yield 1;yield 2}finally{L.push('inner')}})()}finally{L.push('outer')}}",
];
const consumers = [
  "for(var v of g){L.push(v)}", "for(var v of g){L.push(v);break}", "for(var v of g){L.push(v);throw 'body'}", "for(var v of g){L.push(v);continue}",
  "a:for(var v of g){for(var w of [1]){L.push(v);break a}}", "(function(){for(var v of g){return v}})()", "L.push(S([...g]))", "var [a]=g;L.push(a)", "var [a,b]=g;L.push(a+','+b)",
  "var [a,b,c,d]=g;L.push(S([a,b,c,d]))", "var [,,c]=g;L.push(c)", "var [...r]=g;L.push(S(r))", "var [a=(L.push('dflt'),1)]=g;L.push(a)", "var [a,,b]=g;L.push(a+','+b)",
  "L.push(S(Array.from(g)))", "L.push(S(Array.from(g,x=>x*2)))", "L.push(String(new Set(g).size))", "L.push(String(new Map(g.map?[]:[]).size))", "L.push(Math.max(...g))",
  "Promise.resolve(g)", "L.push(S(Object.fromEntries((function*(){yield ['a',1]})())))", "var o={};for(o.x of g){break}L.push(S(o))", "for(var v of g){L.push(v);g.return(9)}",
  "for(var v of g){L.push(v);g.next()}", "L.push(S(Array.prototype.concat.call([],...g)))", "for(var [k] of [g]){L.push('k')}", "var it2=g[Symbol.iterator]();L.push(it2===g)",
  "L.push(S([...g]));L.push(S([...g]))", "for(var v of g){break}for(var w of g){L.push(w)}", "for(var v of g){L.push(v)}L.push(S(g.next()))",
];
for (const gg of consGens) for (const c of consumers) {
  add(`var g=(${gg})();var r;try{${c}}catch(e){L.push('threw '+S(e))}return J()`);
}

// ---- 11. Completar o estado: return/throw sobre completed e suspendedStart, valores exóticos.
const exotic = ["undefined", "null", "0", "-0", "NaN", "''", "'s'", "1n", "Symbol.iterator", "[]", "{}", "{done:true}", "{then(){}}", "function(){}", "new Error('x')", "Promise.resolve(1)", "globalThis", "it"];
for (const v of exotic) {
  for (const m of ["next", "return", "throw"]) {
    add(
      `it=(function*(){yield 1})();C("${m}",${v});C("next");C("next");return J()`,
      `it=(function*(){yield 1})();C("next");C("${m}",${v});C("next");return J()`,
      `it=(function*(){yield 1})();C("next");C("next");C("${m}",${v});C("${m}",${v});return J()`,
      `it=(function*(){try{yield 1}finally{yield 2}})();C("next");C("${m}",${v});C("next");C("next");return J()`,
      `it=(function*(){try{yield 1}catch(e){yield e}})();C("next");C("${m}",${v});C("next");return J()`,
      `it=(function*(){return yield* (function*(){return yield 1})()})();C("next");C("${m}",${v});C("next");return J()`,
    );
  }
}
// Valores de yield e return de tipos variados refletidos no objeto de resultado.
for (const v of exotic.filter(v => v !== "it")) {
  add(
    `it=(function*(){yield ${v};return ${v}})();C("next");C("next");C("next");return J()`,
    `it=(function*(){var x=yield ${v};return x})();C("next");C("next",${v});return J()`,
    `var r=(function*(){yield ${v}})().next();return S(Object.getOwnPropertyNames(r))+Object.getPrototypeOf(r===r?r:r)===Object.prototype+S(Object.getOwnPropertyDescriptor(r,'value'))`,
  );
}

// ---- Execução.
const seen = new Set();
const baseSources = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "generator_state_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    const tab = line.indexOf("\t");
    if (tab < 0) continue;
    baseSources.add(line.slice(0, tab));
  }
}

const programs = [];
let dup = 0;
for (const c of cases) {
  const source = (c.sloppy ? "" : '"use strict";\n') + PRELUDE + `globalThis.R = T(()=>{${c.body}})`;
  const key = JSON.stringify(source);
  if (seen.has(key)) { dup++; continue; }
  seen.add(key);
  if (baseSources.has(key)) { dup++; continue; }
  programs.push({ source, key, body: c.body });
}

function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 5000);
    child.stdout.on("data", d => { out += d; });
    child.stderr.on("data", d => { err += d; });
    child.stdin.on("error", () => {});
    child.on("close", code => { clearTimeout(timer); resolve({ code: code === 0 && decodeResult(out) === null ? -1 : code, out: code === 0 ? decodeResult(out) : null, err: code === 0 && decodeResult(out) === null ? "filho sem resultado" : err, timedOut }); });
    child.stdin.end(source);
  });
}

async function main() {
  const results = new Array(programs.length);
  let next = 0;
  async function worker() {
    while (next < programs.length) {
      const index = next++;
      results[index] = await runChild(programs[index].source);
    }
  }
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  programs.forEach((program, index) => {
    const r = results[index];
    if (r.timedOut || r.code !== 0) {
      dropped++;
      process.stderr.write("filho falhou: " + JSON.stringify(program.body).slice(0, 160) + " " + (r.timedOut ? "timeout" : r.err.slice(0, 120)) + "\n");
      return;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(program.body).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(program.key + "\t" + JSON.stringify(r.out));
  });
  process.stdout.write(emitFactoredLines("generator_state", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos ${dup}\n`);
}
main();
