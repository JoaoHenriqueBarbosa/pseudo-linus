// Gera tests/golden/error_message_bun.tsv: `name` e `message` dos erros de runtime do dia a dia, medidos no bun.
// Cobre chamar valor não chamável (com o `(evaluating '...')` para vários formatos de callee), ler e escrever
// propriedade de undefined/null, `in` e instanceof inválidos, spread e destructuring, JSON circular, BigInt misturado,
// Symbol convertido, const, classes, super, estouro de pilha, RangeError de números, URI e RegExp inválidos.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-error-message-golden.js > tests/golden/error_message_bun.tsv
const PRELUDE =
  'function T(f){try{var v=f();return "ok "+(typeof v==="string"?v:typeof v)}catch(e){return (e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const body = code => `T(()=>{${code}})`;

// ---- 1. Chamar valor não chamável, vários formatos de callee.
const badValues = ["undefined", "null", "1", "{}", "'s'", "true", "Symbol()"];
const callees = [
  v => `var a=${v};a()`, v => `var a={b:${v}};a.b()`, v => `var a={b:{c:${v}}};a.b.c()`, v => `var a={b:${v}},k="b";a[k]()`,
  v => `var a={b:${v}};a["b"]()`, v => `var a=[${v}];a[0]()`, v => `var f=${v};(0,f)()`, v => `var f=${v};(f)()`,
  v => `var a={b:${v}};new a.b()`, v => `var f=${v};new f`, v => `var f=${v};new f(1,2)`, v => `var f=${v};f\`x\``,
  v => `var a={b:${v}};a.b\`x\``, v => `var a={b:${v}};a?.b()`, v => `var a={b:${v}};a.b?.c()`, v => `var f=${v};f?.()`,
  v => `var a={b:${v}};a?.["b"]()`, v => `var f=${v};f.call(null)`, v => `var f=${v};f(...[1,2])`, v => `var a={b:${v}};a.b(...[1])`,
];
for (const v of badValues) for (const c of callees) add(body(c(v)));

add(body("undefinedFn()"), body("a.b.c.d()"), body("var o={};o.x.y()"), body("var o={};o.x()"), body("var o={};o['a b']()"),
  body("var o={};o[1]()"), body("var o={};o[Symbol('s')]()"), body("var s='x';s()"), body("var n=1;n()"), body("(function(){})()()"),
  body("(()=>1)()()"), body("[]()"), body("({})()"), body("(1)()"), body("'abc'()"), body("this.x()"), body("var o={};o.f.g.h()"),
  body("var o={m(){}};o.m()()"), body("var o={};new o.x.y()"), body("new (class{}).x"), body("new 1"), body("new (()=>{})"),
  body("new Math.max"), body("new parseInt"), body("new Symbol"), body("new BigInt(1)"), body("new Math"), body("new JSON"),
  body("var o={m(){}};new o.m"), body("var o={get m(){return 1}};new o.m"), body("var f=async function(){};new f"), body("var f=function*(){};new f"),
  body("var f=async()=>{};new f"), body("Function.prototype.call.call(1)"), body("Reflect.apply(1)"), body("Reflect.construct(1,[])"),
  body("[1].map(1)"), body("[1].forEach(undefined)"), body("[1].filter({})"), body("[1].reduce('x')"), body("[1].sort(1)"), body("[1].find(null)"),
  body("[].reduce((a,b)=>a)"), body("new Promise(1)"), body("new Promise()"), body("Promise.resolve().then.call(1)"),
  body("Object.defineProperty(1,'a',{})"), body("Object.defineProperty({}, 'a', 1)"), body("Object.create(1)"), body("Object.setPrototypeOf({}, 1)"),
  body("Object.assign(null)"), body("Object.keys(null)"), body("Object.entries(undefined)"), body("Object.fromEntries(1)"), body("Object.getPrototypeOf(null)"));

// ---- 2. Ler e escrever propriedade de undefined/null.
for (const v of ["undefined", "null"]) {
  add(body(`var a=${v};a.x`), body(`var a=${v};a.x.y`), body(`var a=${v};a["x"]`), body(`var a=${v};a[0]`), body(`var a=${v};var k="p";a[k]`),
    body(`var a=${v};a.x=1`), body(`var a=${v};a[0]=1`), body(`var a=${v};a.x+=1`), body(`var a=${v};a.x++`), body(`var a=${v};delete a.x`),
    body(`var a=${v};a[Symbol.iterator]`), body(`var a=${v};a.length`), body(`var a={};a.b.c`), body(`var a={b:${v}};a.b.c`),
    body(`var a={b:${v}};a.b.c=1`), body(`var a={b:${v}};a.b[0]`), body(`var a={b:${v}};a.b.c.d`), body(`var a=[${v}];a[0].x`),
    body(`var a=[${v}];a[0][1]`), body(`var {x}=${v}`), body(`var {x:{y}}={x:${v}}`), body(`var [x]=${v}`), body(`var {}=${v}`), body(`var [,]=${v}`),
    body(`var f=({a})=>a;f(${v})`), body(`var f=([a])=>a;f(${v})`), body(`var f=function({a}){};f(${v})`), body(`for(var {a} of [${v}]);`),
    body(`for(var [a] of [${v}]);`), body(`var a;({a}=${v})`), body(`var a;[a]=${v}`), body(`var a=${v};a?.b.c`), body(`var a=${v};a.b?.c`),
    body(`var a=${v};with(Object(1)){}a.x`), body(`var a=${v};a.toString()`), body(`var a=${v};a.constructor`), body(`var a=${v};a["__proto__"]`),
    body(`var a=${v};\`\${a.x}\``), body(`var a=${v};a.x.y.z`), body(`var a=${v};a[a]`), body(`var a=${v};a[1+1]`), body(`var a=${v};a[""]`),
    body(`var o={};o.a.b.c`), body(`var s=${v};s.charAt(0)`), body(`${v}.x`), body(`${v}[0]`), body(`${v}.x=1`),
    body(`Object.prototype.hasOwnProperty.call(${v},'a')`), body(`Array.prototype.map.call(${v},x=>x)`), body(`String.prototype.trim.call(${v})`),
    body(`Number.prototype.toFixed.call(${v})`), body(`Function.prototype.toString.call(${v})`), body(`Map.prototype.get.call(${v},1)`),
    body(`Set.prototype.add.call(${v},1)`), body(`Symbol.prototype.toString.call(${v})`), body(`Date.prototype.getTime.call(${v})`),
    body(`RegExp.prototype.test.call(${v},'a')`), body(`Array.from(${v})`), body(`new Map(${v}).size`), body(`Object.keys(${v})`), body(`JSON.parse(${v}).x`));
}
add(body("var a={};a.b.c=1"), body("var a=0;a.x.y"), body("var a='s';a.x.y"), body("var a=1;a.b.c"), body("var a=true;a.b.c"),
  body("'use strict';var a=1;a.x=1"), body("'use strict';var a='s';a.x=1"), body("'use strict';var a=Symbol();a.x=1"), body("'use strict';var a=1n;a.x=1"),
  body("'use strict';var a=Object.freeze({});a.x=1"), body("'use strict';var a=Object.freeze({x:1});a.x=2"), body("'use strict';var a=Object.freeze({x:1});delete a.x"),
  body("'use strict';var a=Object.freeze([]);a.push(1)"), body("'use strict';var a=Object.preventExtensions({});a.x=1"), body("'use strict';var a={get x(){return 1}};a.x=2"),
  body("'use strict';var a=Object.seal({x:1});delete a.x"), body("'use strict';'abc'.length=1"), body("'use strict';'abc'[0]='x'"), body("'use strict';undefined=1"),
  body("'use strict';NaN=1"), body("'use strict';Infinity=1"), body("'use strict';Math.PI=3"), body("'use strict';Object.defineProperty({}, 'x', {value:1}).x=2"),
  body("var o=Object.freeze([1]);'use strict';o.push(2)"), body("Object.freeze([1]).push(2)"), body("Object.freeze([1]).pop()"), body("Object.freeze({}).x=1;return 'ok'"),
  body("var u;u.x"), body("let u;u.x"), body("const u=null;u.x"), body("function f(a){return a.b}f()"), body("function f(a){return a.b.c}f({})"),
  body("function f({a}){return a}f()"), body("(function(){return this.x})()"), body("(function(){'use strict';return this.x})()"),
  body("var o={f(){return this.x}};var g=o.f;g()"), body("var o={f(){'use strict';return this.x}};var g=o.f;g()"), body("class A{m(){return this.x}};var m=new A().m;m()"));

// ---- 3. `in`, instanceof.
for (const v of ["1", "'s'", "true", "undefined", "null", "Symbol()", "1n"]) add(body(`'a' in ${v}`), body(`0 in ${v}`), body(`Symbol.iterator in ${v}`));
for (const v of ["1", "'s'", "{}", "undefined", "null", "[]", "Symbol()", "true", "({})", "Math", "JSON", "(()=>1)", "{prototype:1}"]) {
  add(body(`({}) instanceof ${v}`), body(`1 instanceof ${v}`), body(`var o={};o instanceof ${v}`));
}
add(body("var F=function(){};F.prototype=1;({}) instanceof F"), body("var F=function(){};F.prototype=null;({}) instanceof F"), body("(()=>1) instanceof (()=>2)"),
  body("({}) instanceof {[Symbol.hasInstance]:1}"), body("({}) instanceof {[Symbol.hasInstance](){return 1}}"), body("({}) instanceof Object.create(Function.prototype)"),
  body("var F=function(){}.bind();({}) instanceof F"), body("({}) instanceof (class{})"), body("({}) instanceof Proxy"), body("({}) instanceof Symbol"),
  body("'a' in 'abc'"), body("'a' in []"), body("1 in [1]"), body("'a' in new Proxy({},{has(){throw new TypeError('h')}})"),
  body("#x in {}"), body("class A{#x;static t(o){return #x in o}};A.t(1)"), body("class A{#x;static t(o){return #x in o}};A.t({})"));

// ---- 4. Spread de não iterável, destructuring e iteração.
for (const v of ["1", "{}", "undefined", "null", "true", "Symbol()", "1n", "({a:1})", "(()=>1)", "new Date()", "Math", "{[Symbol.iterator]:1}",
  "{[Symbol.iterator](){return 1}}", "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator](){return {next(){return 1}}}}", "{[Symbol.iterator](){return {next:1}}}"]) {
  add(body(`[...${v}]`), body(`Math.max(...${v})`), body(`new Set(${v})`), body(`new Map(${v})`), body(`var [a]=${v}`), body(`for(var x of ${v});`),
    body(`Array.from(${v}).length`), body(`Promise.all(${v})`), body(`var f=(...a)=>a;f(...${v})`), body(`var o={...${v}};return Object.keys(o).length`),
    body(`function* g(){yield* ${v}}[...g()]`), body(`new WeakSet(${v})`), body(`Object.fromEntries(${v})`));
}
add(body("var [a]={}"), body("var [a,b]=[1]"), body("var [[a]]=[1]"), body("var [[a]]=[]"), body("var {a:[b]}={a:1}"), body("var {a:{b}}={a:1}"),
  body("new Map([1])"), body("new Map([[1,2],3])"), body("new Map(['ab'])"), body("new WeakMap([[1,2]])"), body("new WeakSet([1])"), body("new WeakMap().set(1,2)"),
  body("new WeakSet().add('x')"), body("new WeakRef(1)"), body("new FinalizationRegistry(1)"), body("Object.fromEntries([1])"), body("Object.fromEntries([[]])"),
  body("var it={[Symbol.iterator](){return {next(){return {done:false,value:1}},return(){throw new Error('r')}}}};for(var x of it){break}"),
  body("for(var x of 1);"), body("for(var x of {});"), body("for(var x in null){}return 'ok'"), body("for(var x of undefined);"), body("yield_ = [...'abc'].length"));

// ---- 5. JSON circular, BigInt misturado, Symbol em string.
add(body("var a={};a.a=a;JSON.stringify(a)"), body("var a=[];a.push(a);JSON.stringify(a)"), body("var a={b:{}};a.b.c=a;JSON.stringify(a)"),
  body("var a={b:[{}]};a.b[0].c=a.b;JSON.stringify(a)"), body("var a={};a.self=a;JSON.stringify({x:a})"), body("class A{constructor(){this.me=this}};JSON.stringify(new A)"),
  body("var a={toJSON(){return a}};JSON.stringify(a)"), body("JSON.stringify(1n)"), body("JSON.stringify({a:1n})"), body("JSON.stringify([1n])"),
  body("JSON.stringify(Symbol())"), body("JSON.parse('{')"), body("JSON.parse('')"), body("JSON.parse(undefined)"), body("JSON.parse('{a:1}')"),
  body("JSON.parse('[1,]')"), body("JSON.parse(\"{'a':1}\")"), body("JSON.parse('nul')"), body("JSON.parse('1 2')"), body("JSON.parse('\"abc')"),
  body("JSON.parse(null)"), body("JSON.parse({})"), body("JSON.parse(Symbol())"), body("JSON.parse('<html>')"), body("JSON.parse('[')"), body("JSON.parse('{\"a\":}')"),
  body("1n+1"), body("1+1n"), body("1n-1"), body("1n*2"), body("2*1n"), body("1n/1"), body("1n%1"), body("1n**1"), body("1n+'a'"), body("1n<1"), body("1n==1"),
  body("1n>>>1n"), body("+1n"), body("Math.abs(1n)"), body("Math.max(1n)"), body("Number(1n)"), body("parseInt(1n)"), body("BigInt(1.5)"), body("BigInt('a')"),
  body("BigInt(undefined)"), body("BigInt(null)"), body("BigInt(Symbol())"), body("BigInt(NaN)"), body("BigInt(Infinity)"), body("BigInt('1.5')"), body("BigInt({})"),
  body("1n/0n"), body("1n%0n"), body("2n**-1n"), body("1n<<(2n**64n)"), body("BigInt.asIntN(-1,1n)"), body("BigInt.asUintN(2**53,1n)"), body("BigInt.asIntN(1,1)"),
  body("(1n).toString(1)"), body("(1n).toString(37)"), body("BigInt.prototype.toString.call(1)"), body("new BigInt(1)"), body("Object(1n)+1"), body("1n+Object(1)"),
  body("[1n,2].sort().map(x=>x+1)"), body("new BigInt64Array([1])"), body("new BigInt64Array(1)[0]=1"), body("new Int8Array(1)[0]=1n"), body("var a=new BigInt64Array(1);a[0]=1"),
  body("var a=new Int8Array(1);a[0]=1n"), body("Atomics.add(new BigInt64Array(1),0,1)"), body("new DataView(new ArrayBuffer(8)).setBigInt64(0,1)"),
  body("Symbol()+''"), body("`${Symbol()}`"), body("Symbol()+1"), body("+Symbol()"), body("Number(Symbol())"), body("'a'+Symbol('x')"), body("String(Symbol('x'))"),
  body("Symbol('x')*2"), body("Symbol()<1"), body("Symbol()==1"), body("-Symbol()"), body("~Symbol()"), body("Symbol().toString()"), body("Math.abs(Symbol())"),
  body("parseInt(Symbol())"), body("[Symbol()].join()"), body("''.concat(Symbol())"), body("'a'.repeat(Symbol())"), body("isNaN(Symbol())"), body("new Symbol()"),
  body("Symbol.keyFor('x')"), body("Symbol.prototype.valueOf.call(1)"), body("Symbol().description='x';return 'ok'"), body("var o={};o[Symbol()]=1;JSON.stringify(o)"),
  body("`${{toString:null,valueOf:null}}`"), body("''+{toString(){return {}},valueOf(){return {}}}"), body("''+Object.create(null)"), body("`${Object.create(null)}`"),
  body("Number(Object.create(null))"), body("Object.create(null)+1"), body("var o={[Symbol.toPrimitive]:1};o+1"), body("var o={[Symbol.toPrimitive](){return {}}};o+1"),
  body("var o={[Symbol.toPrimitive](){return 1}};o+1"), body("String(Object.create(null))"));

// ---- 6. const, let, TDZ, referências.
add(body("const a=1;a=2"), body("const a=1;a++"), body("const a=1;a+=1"), body("const a={};a={}"), body("const a=1;[a]=[2]"), body("const a=1;({a}={a:2})"),
  body("const a=1;for(a of [1]);"), body("const a=1;for(a in {x:1});"), body("for(const i=0;i<2;i++);"), body("for(const i of [1]){i=2}"), body("const a=1;a||=2;a&&=3"),
  body("const a=1;a??=2;a=3"), body("const f=function g(){g=1;return typeof g};return f()"), body("'use strict';var f=function g(){g=1};f()"), body("'use strict';(function g(){g=1})()"),
  body("x;let x=1"), body("let x=x"), body("const x=x"), body("typeof x;let x"), body("function f(){return x;let x}f()"), body("class A extends A{}"), body("new A;class A{}"),
  body("function f(a=b,b){}f()"), body("function f(a=a){}f()"), body("(({a=b,b})=>a)({})"), body("let [a=b,b]=[]"), body("switch(1){case 0:let x;case 1:x}"),
  body("notDefined"), body("notDefined=1;return 'ok'"), body("'use strict';notDefined2=1"), body("typeof notDefined"), body("notDefined++"), body("notDefined.x"),
  body("delete notDefined"), body("'use strict';delete Object.prototype"), body("'use strict';var o={};Object.defineProperty(o,'x',{});delete o.x"),
  body("'use strict';arguments=1"), body("'use strict';eval=1"), body("(function(){'use strict';arguments.callee})()"), body("(function(){'use strict';return arguments.caller})()"),
  body("(function(){'use strict';}).caller"), body("(()=>{}).caller"), body("(function(){}).arguments"), body("'use strict';(function(){}).caller"), body("(class{}).caller"),
  body("Function.prototype.caller"), body("Function.prototype.arguments"), body("(async function(){}).caller"), body("(function*(){}).arguments"),
  body("this.x.y"), body("globalThis.nope.x"), body("window"), body("document.title"), body("undefinedVar.nope.x"), body("nope('x')"),body("module.exports"));

// ---- 7. Classes, super, new.target.
add(body("class A{};A()"), body("class A{constructor(){}};A()"), body("class A extends Object{};A()"), body("var A=class{};A()"), body("class A{};A.call({})"),
  body("class A{static m(){}};A.m.call()"), body("class A{};new A().constructor()"), body("class A{};Reflect.apply(A,{},[])"), body("class A{};A.apply(null)"),
  body("class A{constructor(){this.x}};class B extends A{constructor(){this.y=1;super()}};new B"), body("class A{};class B extends A{constructor(){}};new B"),
  body("class A{};class B extends A{constructor(){super();super()}};new B"), body("class A{};class B extends A{constructor(){return 1}};new B"),
  body("class A{};class B extends A{constructor(){return undefined}};new B"), body("class A{};class B extends A{constructor(){super();return 1}};new B"),
  body("class A{};class B extends A{constructor(){return {}}};typeof new B"), body("class A{};class B extends A{constructor(){let f=()=>this;f();super()}};new B"),
  body("class A extends null{};new A"), body("class A extends null{constructor(){super()}};new A"), body("class A extends 1{}"), body("class A extends {}{}"), body("class A extends undefined{}"),
  body("class A extends (()=>1){}"), body("class A extends Symbol(){}"), body("function F(){};F.prototype=1;class A extends F{}"), body("function F(){};F.prototype=undefined;class A extends F{}"),
  body("class A extends (function*(){}){}"), body("class A extends (async function(){}){}"), body("class A extends Math.max{}"), body("class A extends Math.max.bind(){}"),
  body("var f=()=>{};class A extends f{}"), body("class A{#x=1;static g(o){return o.#x}};A.g({})"), body("class A{#x=1;static s(o){o.#x=2}};A.s({})"),
  body("class A{#m(){};static c(o){o.#m()}};A.c({})"), body("class A{static #p=1;static g(o){return o.#p}};A.g(class{})"), body("class A{#x;constructor(o){return o}};class B extends A{#y;constructor(o){super(o)}};var o={};new B(o);new B(o)"),
  body("class A{get x(){return 1}};var a=new A;'use strict';a.x=2"), body("class A{static x=1};'use strict';A.prototype=1"), body("class A{m(){}};new (new A().m)"),
  body("class A{constructor(){new.target()}};new A"), body("function f(){return new.target}typeof f()"), body("function f(){new.target()}new f"),
  body("class A{static{this.x.y}};"), body("class A{x=this.y.z};new A"), body("class A{[x]=1};"), body("class A{static x=A.y.z};"), body("var o={m(){return super.x.y}};o.m()"),
  body("var o={__proto__:null,m(){return super.x()}};o.m()"), body("class A{m(){return super.x()}};new A().m()"), body("class A{m(){return super.x}};new A().m()"),
  body("class A{static m(){return super.x()}};A.m()"), body("var o={m(){super.x()}};o.m()"), body("class A{};class B extends A{m(){super.nope()}};new B().m()"),
  body("class A{};class B extends A{constructor(){super.x;super()}};new B"), body("class A{constructor(){throw new RangeError('boom')}};class B extends A{};new B"),
  body("Reflect.construct(function(){},[],1)"), body("Reflect.construct(function(){},[],()=>1)"), body("Reflect.construct(1,[])"), body("Reflect.construct(function(){},1)"),
  body("Object.defineProperty(class{},'prototype',{value:1})"), body("class A{};Object.setPrototypeOf(A.prototype,1)"), body("Object.setPrototypeOf(Object.prototype,{})"),
  body("var a={};var b=Object.create(a);Object.setPrototypeOf(a,b)"), body("var a={};a.__proto__=a"), body("Object.setPrototypeOf(Object.freeze({}),{})"),
  body("Object.setPrototypeOf(undefined,{})"), body("Object.setPrototypeOf({},undefined)"), body("({}).__proto__=1;return 'ok'"), body("Object.prototype.__proto__=1"),
  body("Object.prototype.__proto__={}"), body("Object.prototype.__proto__=Object.create(null)"), body("Object.prototype.__proto__=null;return 'ok'"));

// ---- 8. Estouro de pilha.
add(body("function f(){return f()+1}f()"), body("function f(){f()+1}f()"), body("var f=()=>1+f();f()"), body("var o={get x(){return this.x}};o.x"),
  body("var o={set x(v){this.x=v}};o.x=1"), body("function f(){new f}new f"), body("var a=[];a[0]=a;a.toString()"), body("var a=[];a.push(a);a.join()"),
  body("var o={toString(){return ''+this}};''+o"), body("var o={valueOf(){return +this}};+o"), body("function f(){return [f()]}f()"),
  body("var p=new Proxy({},{get(t,k,r){return r[k]}});p.x"), body("var p=new Proxy({},{});Object.setPrototypeOf(p,p)"), body("class A{constructor(){new A}};new A"),
  body("function f(n){return n?f(n-1):0}f(1e6)"), body("function f(n){return n?1+f(n-1):0}f(1e7)"), body("JSON.stringify((function n(d){return d?{a:n(d-1)}:{}})(1e5))"),
  body("var s='[';for(var i=0;i<1e5;i++)s+='[';JSON.parse(s)"), body("var o={};for(var i=0;i<1e5;i++)o={o};JSON.stringify(o)"), body("eval('('.repeat(1e5))"),
  body("new Function('return '+'('.repeat(1e5)+'1'+')'.repeat(1e5))()"), body("/(?:a|b)*/.test('a'.repeat(1e6))"), body("function f(){try{f()}finally{}}f()"),
  body("function f(){try{f()}catch(e){throw e}}f()"), body("var r=function f(){try{return f()}catch(e){return e.name}};r()"), body("[].concat.apply([],new Array(1e6).fill(1)).length"),
  body("Math.max.apply(null,new Array(1e6).fill(1))"), body("String.fromCharCode.apply(null,new Array(1e6).fill(65)).length"), body("Math.max(...new Array(1e6).fill(1))"));

// ---- 9. RangeError de números, arrays, strings.
add(body("new Array(-1)"), body("new Array(1.5)"), body("new Array(2**32)"), body("new Array(2**32-1).length"), body("new Array(NaN)"), body("new Array(Infinity)"),
  body("new Array('a').length"), body("[].length=-1"), body("[].length=1.5"), body("[].length=2**32"), body("var a=[];a.length='x'"), body("var a=[];a.length=NaN"), body("var a=[];a.length={}"),
  body("'use strict';var a=Object.freeze([]);a.length=1"), body("Array(2**32)"), body("Array.from({length:-1})"), 
body("var a=[];a[2**32-1]=1;a.push(1)"), body("var a=[];a.length=2**32-1;a.push(1,2)"), body("'a'.repeat(-1)"), body("'a'.repeat(Infinity)"), body("'a'.repeat(2**30)"), body("'a'.repeat(2**31)"), body("'a'.padStart(2**31)"),
  body("'a'.padEnd(2**30,'bb')"), body("'abc'.normalize('x')"), body("'a'.localeCompare('b','xx-invalid-')"), body("String.fromCodePoint(-1)"), body("String.fromCodePoint(1.5)"),
  body("String.fromCodePoint(0x110000)"), body("String.fromCodePoint(NaN)"), body("String.fromCodePoint('a')"), body("String.fromCodePoint(Infinity)"), body("'a'.at(Infinity)"),
  body("(1).toFixed(101)"), body("(1).toFixed(-1)"), body("(1).toFixed(100)"), body("(1).toFixed(0)"), body("(1).toFixed(NaN)"), body("(1).toFixed(Infinity)"),
  body("(1).toPrecision(0)"), body("(1).toPrecision(101)"), body("(1).toPrecision(100)"), body("(1).toPrecision(1)"), body("(1).toExponential(-1)"), body("(1).toExponential(101)"),
  body("(1).toExponential(100)"), body("(1).toString(1)"), body("(1).toString(37)"), body("(1).toString(0)"), body("(1).toString(NaN)"), body("(1).toString(Infinity)"),
  body("(1).toString(2.5)"), body("(1).toString(36)"), body("Number.prototype.toString.call('1')"), body("Number.prototype.toFixed.call('1')"), body("Number.prototype.valueOf.call({})"),
  body("Number.prototype.toLocaleString.call(1,'xx-invalid-')"), body("(1).toLocaleString('en',{style:'currency'})"), body("(1).toLocaleString('en',{style:'currency',currency:'XX'})"),
  body("(1).toLocaleString('en',{minimumFractionDigits:101})"), body("(1).toLocaleString('en',{minimumFractionDigits:5,maximumFractionDigits:2})"),
  body("new Intl.NumberFormat('en',{style:'bogus'})"), body("new Intl.DateTimeFormat('en',{timeZone:'Nowhere/City'})"), body("new Intl.DateTimeFormat('xx-invalid-')"),
  body("new Date(NaN).toISOString()"), body("new Date('x').toISOString()"), body("new Date(8.64e15+1).toISOString()"), body("new Date(Infinity).toISOString()"),
  body("new Date(NaN).toJSON()"), body("new Date(NaN).toString()"), body("Date.prototype.toISOString.call({})"), body("Date.prototype.getTime.call({})"), body("Date.prototype.getTime.call(1)"),
  body("new Date(1).toLocaleDateString('en',{timeZone:'Bad/Zone'})"), body("new Date(1).toLocaleString('xx-invalid-')"), body("new Date(2**53).toISOString()"),
  body("new ArrayBuffer(-1)"), body("new ArrayBuffer(2**53)"), body("new ArrayBuffer(1e12)"), body("new ArrayBuffer(NaN).byteLength"), body("new ArrayBuffer(8,{maxByteLength:4})"),
  body("new Uint8Array(-1)"), body("new Uint8Array(2**53)"), body("new Uint8Array(new ArrayBuffer(8),9)"), body("new Uint16Array(new ArrayBuffer(8),1)"), body("new Uint16Array(new ArrayBuffer(3))"),
  body("new Uint8Array(new ArrayBuffer(8),0,9)"), body("new Float64Array(new ArrayBuffer(8),4)"), body("new Uint8Array(1).set([1,2])"), body("new Uint8Array(1).set([1],2)"),
  body("new Uint8Array(1).set([1],-1)"), body("new Uint8Array(4).subarray(5).length"), body("new Uint8Array([1]).fill(1,0,5).length"), body("new Uint8Array(1).with(2,1)"),
  body("new Uint8Array(1).at(Infinity)"), body("new Uint8Array(1)[0]=1;Uint8Array.from(1)"), body("Uint8Array.of(1).map(1)"), body("Uint8Array.prototype.length"), body("Uint8Array.prototype.map.call([],x=>x)"),
  body("Uint8Array()"), body("Uint8Array.from.call({},[])"), body("new DataView(1)"), body("new DataView(new ArrayBuffer(1),2)"), body("new DataView(new ArrayBuffer(1)).getInt16(0)"),
  body("new DataView(new ArrayBuffer(1)).getInt8(-1)"), body("new DataView(new ArrayBuffer(1)).setInt8(1,1)"), body("new DataView(new ArrayBuffer(8),0,9)"),
  body("new ArrayBuffer(8).slice(1,2).resize(1)"), body("new ArrayBuffer(8).resize(1)"), body("new ArrayBuffer(8,{maxByteLength:16}).resize(17)"),
  body("structuredClone(()=>1)"), body("structuredClone(Symbol())"), body("structuredClone({f(){}})"), body("structuredClone()"),
  body("Math.max.call(null,1n)"), body("Math.hypot(1n)"), body("Math.round(Symbol())"), body("isFinite(1n)"), body("Number.isFinite(1n)"), body("Math.imul(1n,1)"), body("Math.floor(Object(Symbol()))"),
  body("parseFloat(Symbol())"), body("(123.456).toFixed(2.9)"), body("(0.000001234).toExponential(2)"), body("(1e21).toFixed(2)"), body("(-1.5).toFixed(0)"));

// ---- 10. URI malformado.
for (const s of ["%", "%E0%A4%A", "%FF", "%C0%AF", "%ED%A0%80", "%zz", "%1", "%E0%A4", "%F0%90%80", "%80", "%C3", "%C3%28", "%F8%88%80%80%80", "%E0%80%80"]) {
  add(body(`decodeURIComponent(${JSON.stringify(s)})`), body(`decodeURI(${JSON.stringify(s)})`));
}
for (const s of ["\\uD800", "\\uDC00", "\\uD800a", "a\\uDBFF", "\\uDC00\\uD800", "\\uD83D\\uD83D\\uDE00"]) {
  add(body(`encodeURIComponent("${s}")`), body(`encodeURI("${s}")`), body(`"${s}".normalize()`), body(`"${s}".toWellFormed()`));
}
add(body("decodeURIComponent(Symbol())"), body("encodeURI(Symbol())"), body("decodeURI(1n)"), body("escape('\\uD800')"), body("unescape('%u')"), body("unescape('%zz')"),
  body("new URL('x')"), body("new URL('')"), body("new URL('http://')"), body("new URL('http://[::1')"), body("new URL('//x')"), body("new URL('/x','y')"), body("new URL(undefined)"),
  body("new URL('http://a b/')"), body("new URL('http://exa mple.com')"), body("new URL('https://x:99999')"),
  body("atob('a')"), body("atob('***')"), body("atob('!')"), body("btoa('\\u0100')"), body("btoa(Symbol())"), body("atob()"), body("btoa()"));

// ---- 11. RegExp inválido.
for (const p of ["(", ")", "[", "]", "a**", "*", "+", "?", "a{2,1}", "{1}", "\\", "(?<n>a)(?<n>b)", "(?<>a)", "(?<1a>x)", "\\k<n>", "(?<n>a)\\k<m>", "[b-a]", "(?", "(?x)", "(?<=a",
  "(?<!a", "a(?=", "\\u{110000}", "\\p{Foo}", "\\p{Lu", "\\P{", "[\\d-z]", "\\c", "(?:", "x{99999999999}", "\\1(a)\\2", "a|*", "^*", "$+", "(?=a)*", "\\-", "\\a", "[a-\\d]", "(?i:a)(?i:", "\\u{", "\\p"]) {
  const lit = JSON.stringify(p);
  add(body(`new RegExp(${lit})`), body(`new RegExp(${lit},"u")`), body(`new RegExp(${lit},"v")`));
}
for (const f of ["x", "gg", "uv", "ii", "gimsuyd", "dd", "G", " ", "gu v", "vv", "yy", "msx", "\\u0067"]) add(body(`new RegExp("a",${JSON.stringify(f)})`), body(`/a/.compile("a",${JSON.stringify(f)})`));
add(body("new RegExp(Symbol())"), body("RegExp.prototype.exec.call({},'a')"), body("RegExp.prototype.test.call(1,'a')"), body("RegExp.prototype.global"), body("RegExp.prototype.source"),
  body("RegExp.prototype.flags"), body("RegExp.prototype.toString.call(1)"), body("/a/[Symbol.replace].call(1)"), body("'a'.replaceAll(/a/,'b')"), body("'a'.matchAll(/a/)"), body("'a'.matchAll('a','x')"),
  body("'a'.replace(/a/,Symbol())"), body("'a'.match(/a/y,1);RegExp.prototype[Symbol.match].call({},'a')"), body("'x'.startsWith(/a/)"), body("'x'.endsWith(/a/)"), body("'x'.includes(/a/)"),
  body("String.prototype.at.call(null)"), body("String.prototype.trim.call(undefined)"), body("String.prototype.toUpperCase.call(null)"), body("''.split.call(undefined,'')"),
  body("String.raw()"), body("String.raw({})"), body("String.raw(null)"), body("'a'.localeCompare()"), body("new String(Symbol())"), body("String(Symbol('a')).length"),
  body("'a'.search(Symbol())"), body("var r=/a/g;r.lastIndex=Symbol();r.test('a')"), body("/a/y.lastIndex=1;'use strict';Object.freeze(/a/g).test('a')"),
  body("'use strict';var r=/a/g;Object.freeze(r);r.test('a')"), body("/(?<n>a)/.exec('a').groups.n.x.y"));

// ---- 12. Erros lançados pelo usuário e propriedades de Error.
add(body("throw new Error('m')"), body("throw new TypeError()"), body("throw new RangeError('r',{cause:1})"), body("throw new AggregateError([],'m')"), body("throw new AggregateError(1)"),
  body("new AggregateError()"), body("new Error('a',{cause:'c'}).cause"), body("Error('x') instanceof Error"),  body("Error.prototype.toString.call(1)"), body("Error.prototype.toString.call({name:'N',message:'M'})"), body("Error.prototype.toString.call({})"), body("Error.prototype.toString.call(null)"),
  body("Error.prototype.message"), body("Error.prototype.name"), body("Error.prototype.stack"), body("String(new Error('a'))"), body("String(new TypeError('a'))"),
  body("var e=new Error('a');e.name='X';String(e)"), body("var e=new Error('a');e.message='';e.name='';String(e)"), body("var e=new Error('a');e.name='';String(e)"),
  body("var e=new Error('a');e.message='';String(e)"), body("Object.prototype.toString.call(new Error)"),
  body("new Error(Symbol())"), body("new Error({toString(){throw new Error('t')}})"),
  body("throw null"), body("throw undefined"), body("throw 1"), body("try{throw {name:'N',message:'M'}}catch(e){return e.name+e.message}"),
  body("try{null.x}catch(e){return Object.prototype.toString.call(e)+Object.getPrototypeOf(e).constructor.name}"),
  body("try{undefined()}catch(e){return Object.keys(e).length+','+Object.getOwnPropertyNames(e).sort().join()}"),
  body("try{null.x}catch(e){return JSON.stringify(Object.getOwnPropertyDescriptor(e,'message'))}"),
  body("try{null.x}catch(e){return typeof e.stack+(e.stack.indexOf(e.message)>=0)}"),
  body("try{JSON.parse('{')}catch(e){return e.constructor===SyntaxError}"), body("try{decodeURI('%')}catch(e){return e.constructor===URIError}"),
  body("try{new Array(-1)}catch(e){return e.constructor===RangeError}"), body("try{undefined()}catch(e){return e.constructor===TypeError}"),
  body("try{x}catch(e){return e.constructor===ReferenceError}"), body("try{eval('1+')}catch(e){return e.constructor===SyntaxError}"),
  body("eval('1+')"), body("eval('var')"), body("eval('}')"), body("eval('a b')"), body("eval('let let')"), body("eval('{')"), body("eval('1 = 2')"), body("eval('for(;;')"),
  body("eval('return 1')"), body("eval('break')"), body("eval('continue')"), body("eval('yield 1')"), body("eval('await 1')"), body("eval('super()')"), body("eval('new.target')"),
  body("eval('\"use strict\";with(a){}')"), body("eval('\"use strict\";var eval')"), body("eval('\"use strict\";010')"), body("eval('class A{constructor(){}constructor(){}}')"),
  body("eval('`${')"), body("eval('\"abc')"), body("eval('/a')"), body("eval('1..a.')"), body("eval('a?.b=1')"), body("eval('async function f(){await}')"), body("eval('({a:1}=1)')"),
  body("new Function('a b')"), body("new Function('}','')"), body("new Function('return }')"), body("Function('x','y','return x+')"), body("new Function('a','a','\"use strict\";')"),
  body("(0,eval)('x1')"), body("eval('x2')"), body("new Function('return x3')()"), body("(function(){'use strict';eval('var a=1');return typeof a})()"));

// ---- 13. Geradores, async, Promise, Proxy, Reflect.
add(body("function* g(){yield 1}var i=g();i.next();i.next.call({})"), body("function* g(){var x=yield;i.next()}var i=g();i.next();i.next()"),
  body("function* g(){i.next()}var i=g();i.next()"), body("function* g(){}g.prototype.next.call(1)"), body("function* g(){yield}new g"),
  body("var g=function*(){};g.call().next.call(function*(){}())"), body("function* g(){yield}g().throw(new RangeError('t'))"), body("function* g(){yield}var i=g();i.next();i.return(5).value"),
  body("function* g(){yield* 1}[...g()]"), body("async function f(){}f.call().then.call(1)"), body("Promise.prototype.then.call(1)"), body("Promise.resolve.call(1)"), body("Promise.all.call(1,[])"),
  body("Promise.resolve().finally.call(1)"), body("Promise.prototype.catch.call(null)"), body("new Promise(()=>{}).then.call({})"), body("Promise()"), body("new Promise.resolve()"),
  body("Promise.withResolvers.call(1)"), body("Promise.any([]).constructor.name"), body("Promise.race.call(function(){},[])"), body("Promise.reject.call(1)"),
  body("var p=new Proxy({},{get:1});p.x"), body("var p=new Proxy({},{get(){return 1}});Object.defineProperty(p,'x',{value:2});p.x"),
  body("var t={};Object.defineProperty(t,'x',{value:1});new Proxy(t,{get(){return 2}}).x"), body("new Proxy(1,{})"), body("new Proxy({},1)"), body("Proxy({},{})"),
  body("var r=Proxy.revocable({},{});r.revoke();r.proxy.x"), body("var r=Proxy.revocable({},{});r.revoke();r.proxy.x=1"), body("var r=Proxy.revocable(()=>1,{});r.revoke();r.proxy()"),
  body("var r=Proxy.revocable({},{});r.revoke();Object.keys(r.proxy)"), body("var r=Proxy.revocable({},{});r.revoke();'a' in r.proxy"), body("var r=Proxy.revocable([],{});r.revoke();Array.isArray(r.proxy)"),
  body("new Proxy({},{ownKeys(){return [1]}});Object.keys(new Proxy({},{ownKeys(){return [1]}}))"), body("Object.keys(new Proxy({},{ownKeys(){return ['a','a']}}))"),
  body("Object.keys(new Proxy({},{ownKeys(){return 1}}))"), body("Object.getPrototypeOf(new Proxy({},{getPrototypeOf(){return 1}}))"), body("new Proxy({},{has(){throw new URIError('u')}}).x;'a' in new Proxy({},{has(){throw new URIError('u')}})"),
  body("Object.defineProperty(new Proxy({},{defineProperty(){return false}}),'a',{})"), body("'use strict';new Proxy({},{set(){return false}}).a=1"), body("'use strict';delete new Proxy({},{deleteProperty(){return false}}).a"),
  body("new Proxy(function(){},{construct(){return 1}}).x;new (new Proxy(function(){},{construct(){return 1}}))"), body("Reflect.get(1,'a')"), body("Reflect.set(1,'a',1)"), body("Reflect.has(1,'a')"),
  body("Reflect.ownKeys(1)"), body("Reflect.getPrototypeOf(1)"), body("Reflect.defineProperty(1,'a',{})"), body("Reflect.apply(()=>1,null)"), body("Reflect.apply(()=>1,null,1)"),
  body("Reflect.setPrototypeOf({},1)"), body("Reflect.deleteProperty(1,'a')"), body("Reflect.isExtensible(1)"), body("Reflect()"), body("new Reflect"),
  body("Object.defineProperty({}, 'a', {get(){}, value:1})"), body("Object.defineProperty({}, 'a', {get:1})"), body("Object.defineProperty({}, 'a', {set:'x'})"),
  body("Object.defineProperty(Object.freeze({}),'a',{value:1})"), body("var o={};Object.defineProperty(o,'a',{value:1});Object.defineProperty(o,'a',{value:2})"),
  body("var o={};Object.defineProperty(o,'a',{get(){}});Object.defineProperty(o,'a',{value:2})"), body("Object.defineProperties({}, {a:1})"), body("Object.defineProperties({}, null)"),
  body("Object.defineProperty([],'length',{value:-1})"), body("Object.defineProperty([],'length',{get(){}})"), body("var a=[];Object.defineProperty(a,'length',{writable:false});a.push(1)"),
  body("Object.freeze(1);Object.freeze(new Uint8Array(1))"), body("Object.seal(new Uint8Array(1));Object.freeze(new Uint8Array(1))"), body("Object.getOwnPropertyDescriptor(null,'a')"),
  body("Object.getOwnPropertyNames(undefined)"), body("Object.entries(null)"), body("Object.values(undefined)"), body("Object.groupBy(1,x=>x)"), body("Object.groupBy([1],1)"),
  body("Object.prototype.toString.call(undefined)"), body("Object.prototype.valueOf.call(null)"), body("Object.prototype.toLocaleString.call(null)"), body("Object.prototype.isPrototypeOf.call(null,{})"),
  body("Object.prototype.propertyIsEnumerable.call(null,'a')"), body("Object.prototype.__lookupGetter__.call(null,'a')"), body("Object.prototype.__defineGetter__.call({}, 'a', 1)"),
  body("Object.hasOwn(null,'a')"), body("Object.is.call()"), body("Object.prototype.hasOwnProperty.call(undefined,'a')"), body("({}).hasOwnProperty.call(null,'a')"));

// ---- 14. Map, Set, WeakMap, Array e Function misc.
add(body("Map()"), body("Set()"), body("WeakMap()"), body("new Map(1)"), body("new Set(1)"), body("Map.prototype.get.call({},1)"), body("Map.prototype.size"), body("Set.prototype.has.call(new Map,1)"),
  body("Map.prototype.set.call(new Set,1,2)"), body("Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call({})"), body("new Map().forEach(1)"), body("new Set().forEach()"),
  body("new Set([1]).union(1)"), body("new Set([1]).union([1])"), body("new Set([1]).intersection({})"), body("new Set([1]).union({size:NaN,has(){},keys(){}})"), body("new Set([1]).union({size:1,has:1,keys(){}})"),
  body("Map.groupBy(1,x=>x)"), body("Map.groupBy([1],1)"), body("new WeakMap().get(1)"), body("new WeakMap().set({},1).set(1,1)"), body("new WeakSet().add(Symbol.for('x'))"), body("new WeakRef({}).deref.call(1)"),
  body("Array.prototype.map.call(1,1)"), body("Array.prototype.forEach.call(null,x=>x)"), body("[].forEach.call(undefined,x=>x)"), body("Array.from(1,2)"), body("Array.from([],1)"), body("Array.of.call(1)"),
  body("[1,2].sort(1)"), body("[1,2].sort(null)"), body("[1,2].toSorted(1)"), body("[].at.call(null)"), body("[1].flatMap(1)"), body("[1].findLast()"), body("[1].some('x')"), body("[1].every({})"),
  body("[1].reduceRight()"), body("[].reduceRight((a,b)=>a)"), body("[].reduce(1)"), body("[,].reduce((a,b)=>a)"), body("[1].join(Symbol())"), body("[Symbol()].join()"), body("[Symbol()]+''"),
  body("[1].with(5,1)"), body("[1].with(-5,1)"), body("[1].toSpliced(0,0,...[1,2],3).length"), body("[1].copyWithin.call(null)"), body("[].fill.call(Object.freeze([1]),1)"),
  body("Array.prototype.concat.call(null)"), body("[].push.call({length:2**53-1},1)"),
  body("Array.isArray.call()"), body("new Array(3).map(x=>x).length"), body("Array.prototype.length=1;return Array.prototype.length"), body("Array.apply(null,{length:-1})"),
  body("Function.prototype.call.call(undefined)"), body("Function.prototype.apply.call(1)"), body("Function.prototype.bind.call(1)"), body("(function(){}).apply(null,1)"), body("(function(){}).apply(null,'abc')"),
  body("(function(){}).bind().prototype.x"), body("Function.prototype.toString.call({})"), body("Function.prototype.toString.call(class{})"), body("Function.prototype()"), body("new Function.prototype"),
  body("Function.prototype[Symbol.hasInstance].call(1,{})"), body("Function.call(1,'}')"), body("Function('a,','return a')"), body("var f=function(){};f.bind(1).call.call(1)"),
  body("Symbol.for(Symbol())"), body("Symbol.for()"), body("Symbol.keyFor(1)"), body("Symbol.prototype.description"), body("Symbol.prototype[Symbol.toPrimitive].call(1)"),
  body("Date.prototype.toString.call(1)"), body("Date.prototype[Symbol.toPrimitive].call(new Date,'bad')"), body("Date.prototype[Symbol.toPrimitive].call(1,'number')"), body("new Date(Symbol())"),
  body("new Date(1n)"), body("Date.UTC(Symbol())"), body("Date(1n)"), body("Date.prototype.setTime.call({},1)"), body("new Date().setHours(Symbol())"),
  body("Intl.DateTimeFormat.prototype.format"), body("Intl.NumberFormat.prototype.format.call({})"), body("Intl.Collator.prototype.compare.call(1)"), body("Intl()"), body("new Intl.PluralRules('en',{type:'x'})"),
  body("new Intl.ListFormat('en',{type:'x'})"), body("new Intl.RelativeTimeFormat('en').format(1,'x')"), body("new Intl.Locale()"), body("new Intl.Locale('x_y')"), body("Intl.getCanonicalLocales('x_y')"),
  body("Intl.DateTimeFormat.supportedLocalesOf(1)"), body("Intl.DisplayNames"), body("new Intl.DisplayNames('en')"), body("new Intl.DisplayNames('en',{type:'x'})"), body("new Intl.Segmenter('en',{granularity:'x'})"),
  body("globalThis.x.y"), body("globalThis.Symbol()"), body("globalThis()"), body("new globalThis"), body("Atomics.wait(new Int32Array(4),0,0)"), body("Atomics.add(1,0,1)"), body("Atomics.add(new Float64Array(1),0,1)"),
  body("Atomics.load(new Int32Array(1),5)"), body("Atomics.notify(new Int32Array(1),0)"), body("new SharedArrayBuffer(-1)"), body("WebAssembly.Module()"), body("new WebAssembly.Module(new Uint8Array(1))"),
  body("new WebAssembly.Instance(1)"), body("new WebAssembly.Memory({})"), body("new WebAssembly.Module()"), body("WebAssembly.validate(1)"), body("WebAssembly.compile(1)"),
  body("nope(1)"), body("nope()"),
  body("new TextDecoder('xx')"), body("new TextDecoder('utf-8',{fatal:true}).decode(new Uint8Array([255]))"), body("new TextDecoder().decode(1)"), body("new TextEncoder().encode(Symbol())"),
  body("new AbortController().signal.throwIfAborted()"), body("var c=new AbortController();c.abort();c.signal.throwIfAborted()"));
// Fora de propósito: URLSearchParams, DOMException, AbortSignal, Event, EventTarget, Blob, Headers, Request, Response
// e a mensagem `invalid_argument` de Error.captureStackTrace são do Bun (WebCore e embedder), não do JavaScriptCore.

// ---- 15. Formatos de callee adicionais: nomes compostos e números, propagação do trecho `(evaluating '...')`.
const chains = ["a.b", "a.b.c", "a[0]", "a['x-y']", "a.b[0]", "a[0][1]", "a.b.c.d", "a?.b", "a.b?.c", "a[b]", "a[b.c]", "a[b[0]]", "a[1+2]", "a[`k`]", "a[b()]", "this.a", "this.a.b", "super_a", "a.b.c.d.e.f"];
for (const c of chains) {
  add(body(`var a={},b=0;${c}()`), body(`var a={},b=0;new ${c}`), body(`var a={},b=0;new ${c}()`), body(`var a={},b=0;${c}\`t\``), body(`var a={},b=0;${c}(1,2,3)`));
}
add(body("var a={b(){return 1}};a.b()()"), body("var a={b(){return {}}};a.b().c()"), body("var f=()=>({});f().x()"), body("var f=()=>()=>1;f()()()"), body("(function(){return 1})()()"),
  body("[1,2].map(x=>x).x()"), body("'abc'.split('').foo()"), body("'abc'.length()"), body("[].length()"), body("(1).x()"), body("(1.5).toFixed()()"), body("1..x()"), body("true.x()"),
  body("Math.nope()"), body("Math.PI()"), body("JSON.nope()"), body("Object.nope()"), body("Array.nope()"), body("Promise.nope()"), body("undefinedVar.nope()"),body("Date.nope()"), body("new Date().nope()"),
  body("new Map().nope()"), body("new Set().nope()"), body("[].nope()"), body("({}).nope()"), body("''.nope()"), body("(()=>{}).nope()"), body("Symbol.nope()"), body("Reflect.nope()"),
  body("var x=1;x.y.z()"), body("var s=Symbol('q');({})[s]()"), body("var o={};o[Symbol.iterator]()"), body("var o={};o[Symbol.for('k')]()"), body("var o={};o[1n]()"), body("var o={};o[{}]()"), body("var o={};o[[1]]()"),
  body("var o={};o[null]()"), body("var o={};o[undefined]()"), body("var o={};o[true]()"), body("var o={};o[-1]()"), body("var o={};o[1.5]()"), body("var o={};o['']()"), body("var o={};o[' ']()"),
  body("var o={};o['a.b']()"), body("var o={};o['a\\'b']()"), body("var o={};o[\"a\\\"b\"]()"), body("var o={};o['\\n']()"), body("var o={};o['é']()"), body("var o={};o['😀']()"), body("var o={};o['a'.repeat(200)]()"),
  body("var é=undefined;é()"), body("var $=undefined;$()"), body("var _=undefined;_()"), body("var o={é:1};o.é()"), body("var o={a:{}};o.a.b.c.d()"),
  body("async function f(){await undefined()}f()"), body("async function f(){await null.x}return typeof f()"), body("async function f(){null.x}f().then"), body("(async()=>{null.x})().catch(e=>e)"),
  body("new Promise(r=>r()).then(undefined()).x"), body("Promise.resolve().then(null)"), body("var p=Promise.resolve();p.then.call(p,1,2)"), body("[...[1,2]].x()"), body("[1,2,3].map(undefined)"),
  body("`${undefined()}`"), body("(undefined)()"), body("(null)()"), body("(void 0)()"), body("(1,undefined)()"), body("(a=undefined)()"), body("var a;(a=a)()"), body("var a;(a||a)()"), body("var a;(a&&a)()"),
  body("var a;(a??a)()"), body("var a;(a?a:a)()"), body("var a;(await_=a)()"), body("var a;(!a)()"), body("var a;(-a)()"), body("var a;(typeof a)()"), body("var a;(a+1)()"), body("var a;(a,a)()"),
  body("var a;new (a||a)"), body("var a;new (a,a)"), body("var a;new (a?.b)"), body("var a={};new (a.b||a.c)"), body("new (function(){return 1}())"), body("new ((()=>1)())"));

// ---- Saída.
const fs = require("fs");
process.on("unhandledRejection", () => {});
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
// ---- 3. toObject e requireObjectCoercible de nativos em undefined/null (o sufixo `(evaluating '...')` só aparece
// quando o nativo usa `createNotAnObjectError`; os demais têm mensagem própria).
for (const v of ["undefined", "null"]) {
  add(body(`Object.keys(${v})`), body(`Object.values(${v})`), body(`Object.entries(${v})`), body(`Object.assign(${v})`),
    body(`Object.getPrototypeOf(${v})`), body(`Object.getOwnPropertyNames(${v})`), body(`Object.getOwnPropertySymbols(${v})`),
    body(`Object.getOwnPropertyDescriptor(${v},'a')`), body(`Object.getOwnPropertyDescriptors(${v})`), body(`Object.isFrozen(${v})`),
    body(`Object.freeze(${v})`), body(`Object.setPrototypeOf(${v},{})`), body(`Object.defineProperties(${v},{})`),
    body(`Object.fromEntries(${v})`), body(`Object.groupBy(${v},x=>x)`), body(`Object.prototype.toString.call(${v})`),
    body(`Object.prototype.valueOf.call(${v})`), body(`Object.prototype.hasOwnProperty.call(${v},'a')`),
    body(`Object.prototype.isPrototypeOf.call(${v},{})`), body(`Object.prototype.propertyIsEnumerable.call(${v},'a')`),
    body(`Object.prototype.toLocaleString.call(${v})`), body(`String.prototype.trim.call(${v})`), body(`String.prototype.at.call(${v})`),
    body(`String.prototype.padStart.call(${v},2)`), body(`String.prototype.toUpperCase.call(${v})`), body(`String.prototype.slice.call(${v})`),
    body(`String.prototype.split.call(${v},'a')`), body(`String.prototype.replace.call(${v},'a','b')`), body(`String.prototype.localeCompare.call(${v},'a')`),
    body(`String.prototype.normalize.call(${v})`), body(`String.prototype.repeat.call(${v},2)`), body(`String.prototype.includes.call(${v},'a')`),
    body(`Number.prototype.toString.call(${v})`), body(`Number.prototype.valueOf.call(${v})`), body(`Number.prototype.toFixed.call(${v},1)`),
    body(`Boolean.prototype.valueOf.call(${v})`), body(`BigInt.prototype.toString.call(${v})`), body(`Function.prototype.call.call(${v})`),
    body(`Function.prototype.apply.call(${v})`), body(`Function.prototype.bind.call(${v})`), body(`Reflect.ownKeys(${v})`),
    body(`Reflect.getPrototypeOf(${v})`), body(`Reflect.defineProperty(${v},'a',{})`), body(`Reflect.apply(${v})`),
    body(`new Map(${v})`), body(`new Set(${v})`), body(`new WeakMap(${v})`), body(`new Map([${v}])`), body(`new Map([1])`),
    body(`Array.prototype.concat.call(${v})`), body(`Array.prototype.join.call(${v})`), body(`Array.from.call(${v},[])`), body(`Array.of.call(${v})`));
}
add(body("Object.freeze()"), body("Reflect.ownKeys(1)"), body("Reflect.ownKeys('s')"), body("Object.setPrototypeOf(1)"), body("Object.fromEntries(1)"),
  body("Object.getPrototypeOf(1)"), body("Object.keys(1)"), body("Object.defineProperties(1,{})"), body("Object.defineProperties({},null)"),
  body("Object.create({},null)"), body("Object.create(undefined)"), body("Object.assign({}, null, undefined)"), body("Number.prototype.toString.call({})"),
  body("Number.prototype.toString.call('s')"), body("Number.prototype.toString.call(1n)"), body("Boolean.prototype.valueOf.call(1)"),
  body("String.prototype.valueOf.call(1)"), body("Symbol.prototype.valueOf.call(1)"), body("BigInt.prototype.valueOf.call(1)"));

// ---- 7. Texto-fonte de `(In '...')`, `(evaluating '...')` e `(near '...')` com o programa no topo de script (eval indireto),
// dentro de função, com chamada de membro, `new`, argumentos e linhas além da primeira (PLAN.md item 6).
// Cada caso tem nomes próprios porque os `var` do eval indireto vazam para o global do gerador.
let topN = 0;
const top = src => body(`(0,eval)(${JSON.stringify(src.replaceAll("@", String(topN++)))})`);
add(top("g@(); var g@ = function(){}"), top("\n\n   g@(); var g@ = function(){}"), top("  \n\n  var a@;\n  a@()"),
  top("g@();\nvar g@;\n"), top("/* c */ g@(); var g@"), top("x@ = 1; x@()"), top("var a@ = 1; a@(\n1,\n2)"),
  top("var o@ = {}; o@.m(\n)"), top("var o@ = {}; o@.f()"), top("var o@ = {f: 1}; o@.f()"), top("var o@ = {a:{}}; o@.a.b()"),
  top("var o@ = {}; o@.a.b()"), top("var o@ = {}; o@['x']()"), top("var o@ = {}; o@[\n'k']()"), top("var o@ = {}; o@.b(...[1])"),
  top("var o@ = {}; (0, o@.b)()"), top("this.zz@()"), top("var X@; new X@()"), top("var X@; new X@(1, 2)"), top("var o@ = {}; new o@.k()"),
  top("var a@ = 1; new a@()"), top("var s@ = 'str'; s@()"), top("null()"), top("var a@ = 1; a@`x`"),
  top("\n\nvar y@;\n\n  y@(1)"), top("var z@ = 1;\nz@();"), top("g@(1, 'a'); var g@"), top("var u@ = undefined; u@()"),
  top("var q@; ;;; q@()"), top("var q@; q@?.(); q@()"), top("if (1) {\n  h@();\n}\nvar h@"),
  top("(function(){ g@(); var g@ = function(){} })()"), top("function f@(){ \n  var x = 3;\n  x(); }\nf@()"),
  top("function f@(){\n   g(1,\n 2);\n var g = 5 }\nf@()"), top("(function(){ var o = {}; o.m(1) })()"),
  top("(function(){ var X; new X(1) })()"), top("(function(){ 'use strict'; var a; a() })()"),
  top("[1].map(function(){ g@(); var g@; })"), top("var a@; eval('a@()')"), top("eval('  g@(); var g@ = 1')"),
  top("eval('var o@ = {}; o@\\n.m()')"), top("eval('\\n\\n  var o = {};\\n  o.p.q()')"));

const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const outPath = process.argv[2];
const lines = [];
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const expr of unique) {
  if (HOST.test(expr)) continue;
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    globalThis.R = undefined;
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    dropped++;
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    continue;
  }
  // Resultados que dependem do ambiente (caminho, hora, pilha, memória) não são determinísticos entre motores.
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|node_modules/.test(result) || result === "undefined") {
    dropped++;
    process.stderr.write("não determinístico: " + JSON.stringify(expr).slice(0, 160) + " => " + result.slice(0, 80) + "\n");
    continue;
  }
  lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
if (outPath) fs.writeFileSync(outPath, lines.join("\n") + "\n");
else console.log(lines.join("\n"));
process.stderr.write(`mantidos ${lines.length}, descartados ${dropped}\n`);
