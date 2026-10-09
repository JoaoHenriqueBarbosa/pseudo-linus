// Gera tests/golden/class_builtin_bun.tsv: matrizes de herança de built-ins e de classes, medidas no bun 1.4.2.
// Cobre class extends de Array/Error/Promise/Map/Set/RegExp/Function/Boolean/Number/String/Date/TypedArray/ArrayBuffer
// com super() e new.target, Reflect.construct com newTarget (classe, função, bound, Proxy, prototype inválido, outro
// realm via ShadowRealm), formas de construtor derivado, campos e métodos estáticos com `this`, static blocks,
// `accessor` (o bun 1.4.2 ainda não tem auto-accessors, então só os erros de sintaxe e o uso como nome), métodos e
// acessores privados com `#x in o` (inclusive carimbo por retorno de objeto), `extends null` e não construtores,
// getters estáticos herdados, Symbol.species em subclasses e Function.prototype.toString de classes e membros.
// Programas cuja expressão já aparece nos goldens vizinhos (class, class_edge, ctor_this, subclass_edge, brand,
// proxy_class, symbol_species, reflect, arguments_super) são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-class-builtin-golden.js > tests/golden/class_builtin_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
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
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);

// ---- 1. Built-ins x newTarget: o protótipo do resultado vem do newTarget, com fallback para o intrínseco do realm.
const builtins = [
  ["Array", "[]"], ["Array", "[3]"], ["Error", "['m']"], ["TypeError", "['m']"], ["AggregateError", "[[1],'m']"], ["Promise", "[function(){}]"],
  ["Map", "[]"], ["Set", "[]"], ["WeakMap", "[]"], ["WeakSet", "[]"], ["RegExp", "['a','g']"], ["Function", "['return 1']"], ["Boolean", "[1]"],
  ["Number", "['5']"], ["String", "['ab']"], ["Date", "[0]"], ["Uint8Array", "[2]"], ["Float64Array", "[1]"], ["BigInt64Array", "[1]"],
  ["ArrayBuffer", "[4]"], ["DataView", "[new ArrayBuffer(2)]"], ["Object", "[]"], ["WeakRef", "[{}]"], ["FinalizationRegistry", "[function(){}]"],
  ["SharedArrayBuffer", "[2]"],
];
const targets = [
  ["class N{}", "N", "N.prototype"],
  ["function N(){}", "N", "N.prototype"],
  ["function N(){}", "N.bind(null)", "B.prototype"],
  ["function N(){}", "new Proxy(N,{})", "N.prototype"],
  ["function N(){}", "new Proxy(N,{get(t,k){return k==='prototype'?Array.prototype:t[k]}})", "Array.prototype"],
  ["function N(){}; N.prototype=null", "N", "B.prototype"],
  ["function N(){}; N.prototype=1", "N", "B.prototype"],
  ["function N(){}; N.prototype='s'", "N", "B.prototype"],
  ["function N(){}; N.prototype=Object.create(null)", "N", "N.prototype"],
  ["function N(){}; Object.defineProperty(N,'prototype',{get(){throw new RangeError('gp')}})", "N", "0"],
  ["class N extends B{}", "N", "N.prototype"],
  ["var N=function(){}; Object.defineProperty(N,'prototype',{value:Math})", "N", "Math"],
  ["var N=(()=>1)", "N", "0"],
  ["var N=async function(){}", "N", "0"],
  ["var N=function*(){}", "N", "0"],
  ["var N={m(){}}.m", "N", "0"],
  ["var N=Math.max", "N", "0"],
  ["var N=new ShadowRealm().evaluate('function N(){}; N')", "N", "0"],
  ["var N=new ShadowRealm().evaluate('(function(){}).bind()')", "N", "0"],
];
for (const [B, args] of builtins) for (const [decl, nt, proto] of targets) {
  add(`T(()=>{var B=${B};${decl};var r=Reflect.construct(B,${args},${nt});return [Object.prototype.toString.call(r),Object.getPrototypeOf(r)===(${proto}),r instanceof B].join()})`);
}

// ---- 2. Formas de construtor derivado sobre built-ins.
const derivedBases = ["Array", "Error", "Promise", "Map", "Set", "RegExp", "Function", "Boolean", "Number", "String", "Date", "Uint8Array", "ArrayBuffer", "WeakMap", "Object"];
const baseArgs = { Array: "2", Error: "'m'", Promise: "function(){}", Map: "", Set: "", RegExp: "'a'", Function: "'return 7'", Boolean: "1", Number: "3", String: "'s'", Date: "5", Uint8Array: "1", ArrayBuffer: "3", WeakMap: "", Object: "" };
const shapes = [
  (a) => ["constructor(){super(" + a + ")}", "new D()"],
  (a) => ["constructor(){super(" + a + ");return {z:1}}", "new D()"],
  (a) => ["constructor(){super(" + a + ");return 1}", "new D()"],
  (a) => ["constructor(){super(" + a + ");return undefined}", "new D()"],
  (a) => ["constructor(){return {z:2}}", "new D()"],
  (a) => ["constructor(){}", "new D()"],
  (a) => ["constructor(){super(" + a + ");super(" + a + ")}", "new D()"],
  (a) => ["constructor(){this.k=1;super(" + a + ")}", "new D()"],
  (a) => ["constructor(){(()=>super(" + a + "))()}", "new D()"],
  (a) => ["constructor(){var f=()=>super(" + a + ");f();f()}", "new D()"],
  (a) => ["constructor(){super(" + a + ");this.nt=new.target===D}", "new D()"],
  (a) => ["constructor(){eval('super(" + a.replace(/'/g, "\\'") + ")')}", "new D()"],
  (a) => ["constructor(...x){super(...x)}", "new D(" + a + ")"],
  (a) => ["constructor(){super(" + a + ")} static get [Symbol.species](){return Object}", "new D()"],
  (a) => ["constructor(){super(" + a + ");return this}", "D.call({})"],
  (a) => ["", "D()"],
  (a) => ["x=1", "new D(" + a + ")"],
  (a) => ["static x=new.target", "D.x"],
];
for (const B of derivedBases) for (const shape of shapes) {
  const [body, use] = shape(baseArgs[B]);
  add(`T(()=>{class D extends ${B}{${body}}var r=${use};return [typeof r,Object.prototype.toString.call(r),r instanceof D,r instanceof ${B},Object.keys(r)]})`);
}

// ---- 3. new.target em contextos.
const ntCases = [
  "function f(){return new.target} [f(),new f()===undefined,typeof new f()]", "function f(){return new.target} f.call({})", "function f(){return new.target===f} new f()instanceof f",
  "function f(){return ()=>new.target} [f()(),typeof new f()()]", "function f(){return eval('new.target')} [f(),typeof new f()]",
  "function f(){return new Function('return new.target')()} f()", "function f(){return (()=>(()=>new.target)())()} new f()===undefined",
  "class A{constructor(){this.t=new.target}} class B extends A{} [new A().t===A,new B().t===B]", "class A{constructor(){this.t=new.target.name}} class B extends A{} new B().t",
  "class A{static s(){return new.target}} [A.s(),new A.s()===undefined]", "class A{m(){return new.target}} [new A().m()]", "class A{static{globalThis.q=new.target}} globalThis.q",
  "class A{x=new.target} new A().x", "class A{static x=new.target} A.x", "class A{x=()=>new.target} new A().x()", "class A{[eval('new.target')]=1} Object.keys(new A())",
  "class A{constructor(){return Reflect.construct(Array,[],new.target)}} class B extends A{} Object.getPrototypeOf(new B())===B.prototype",
  "class A{constructor(){this.a=new.target}} Reflect.construct(A,[],Array).a===Array", "class A{constructor(){this.a=new.target}} Reflect.construct(A,[],function(){}).a.name",
  "function f(){return new.target} Reflect.construct(f,[],Object)===Object", "function f(){return new.target} Reflect.construct(f,[],class{})===undefined",
  "function f(){return new.target} Reflect.apply(f,null,[])", "function f(){return new.target} Reflect.construct(f,[]).constructor===f",
  "var o={m(){return new.target}}; o.m()", "var o={get g(){return new.target}}; o.g", "var o={f:function(){return new.target}}; [o.f(),typeof new o.f()]",
  "var f=function(){return new.target}.bind(null); [f(),typeof new f()]", "function f(){return new.target} var g=f.bind(null); new g()===f",
  "function f(){return new.target} var g=f.bind(null); var h=g.bind(null); new h()===f", "async function f(){return new.target} f().constructor===Promise",
  "function f(a=new.target){return a} [f(),f(1)]", "function f(a=new.target){return a} typeof new f()", "function f({a=new.target}={}){return a} [f(),typeof new f()]",
  "function* g(){yield new.target} g().next().value", "var f=function(){return new.target}; Reflect.construct(f,[],Date).constructor===Date",
  "class A{constructor(){return new.target.prototype}} class B extends A{} new B()===B.prototype", "class A extends null{constructor(){return Object.create(new.target.prototype)}} new A() instanceof A",
  "class A{constructor(){return {t:new.target}}} Reflect.construct(A,[],Array).t===Array", "function f(){new.target=1} 1",
  "class A{constructor(){this.f=function(){return new.target}}} new A().f()", "class A{constructor(){this.f=()=>new.target}} new A().f()===A",
  "class A{constructor(){this.f=()=>new.target}} class B extends A{} new B().f()===B", "class B extends Array{constructor(){super();this.t=new.target}} new B().t===B",
  "class A{constructor(){this.s=Object.getPrototypeOf(new.target)===Function.prototype}} class B extends A{} class C extends B{} [new A().s,new B().s,new C().s]",
];
for (const e of ntCases) add(wrapBody(e));

function wrapBody(e) {
  // Última expressão separada por " " depois do último `}` ou `;` vira o return.
  const m = e.match(/^(.*[;}])\s+([^;}]+)$/s);
  return m ? `T(()=>{${m[1]} return ${m[2]}})` : `T(()=>${e})`;
}

// ---- 4. Estáticos com this, herança de getters estáticos, static blocks.
const staticMembers = [
  ["static x=this", "x"], ["static x=this.name", "x"], ["static x=()=>this", "x()"], ["static x(){return this}", "x()"], ["static get x(){return this}", "x"],
  ["static get x(){return this.name}", "x"], ["static set x(v){this._x=v}", "x=5"], ["static x=1;static y=this.x+1", "y"], ["static #p=this;static get x(){return A.#p}", "x"],
  ["static #m(){return this}static get x(){return A.#m()}", "x"], ["static async x(){return this}", "x()"], ["static *x(){yield this}", "x().next().value"],
  ["static [Symbol.iterator](){return this}", "[Symbol.iterator]()"], ["static ['x'+1]=this", "x1"], ["static x=new.target", "x"],
  ["static x=super.toString", "x===Function.prototype.toString"], ["static x=super.constructor", "x===Function"], ["static{this.x=this}", "x"],
  ["static{var t=this;this.x=()=>t}", "x()"], ["static x=class{static y=this}", "x.y"], ["static x=function(){return this}", "x()"],
  ["static x=(()=>{try{return this}catch(e){return e.name}})()", "x"], ["static get x(){return super.name}", "x"], ["static set x(v){super.y=v}", "x=1;y"],
];
const dForms = [
  (m, A, B) => `class A{${A}}class B extends A{}return S(B.${B})`,
  (m, A, B) => `class A{${A}}class B extends A{}var d=Object.getOwnPropertyDescriptor(B,'x');return typeof d`,
  (m, A, B) => `class A{${A}}class B extends A{}return Object.getOwnPropertyNames(B).join()`,
  (m, A, B) => `class A{${A}}class B extends A{static x=1}return Object.getOwnPropertyNames(B).join()`,
  (m, A, B) => `class A{${A}}return S(A.${B})`,
];
for (const [member, use] of staticMembers) for (const f of dForms) {
  add(`T(()=>{${f(null, member, use)}})`);
}
add(
  "T(()=>{var o=[];class A{static{o.push(1)}static x=o.push(2);static{o.push(3)}static y=o.push(4)}return o.join()})",
  "T(()=>{var o=[];class A{static x=o.push('x');[(o.push('k'),'y')]=1;static [(o.push('sk'),'z')]=o.push('z')}return o.join()})",
  "T(()=>{class A{static{return}}})", "T(()=>{class A{static{await 1}}})", "T(()=>{class A{static{arguments}}})", "T(()=>{class A{static{var x=1}static{return x}}})",
  "T(()=>{class A{static{var x=1}static y=typeof x}return A.y})", "T(()=>{class A{static{let x=1;this.g=()=>x}static{let x=2;this.h=()=>x}}return [A.g(),A.h()]})",
  "T(()=>{class A{static{break}}})", "T(()=>{x:{class A{static{break x}}}})", "T(()=>{class A{static{super()}}})", "T(()=>{class A{static{this.a=super.constructor}}return A.a===Function})",
  "T(()=>{class A{static{yield}}})", "T(()=>{class A{static{var await}}})", "T(()=>{class A{static{function await(){}}}})", "T(()=>{class A{static{class await{}}}})",
  "T(()=>{class A{static{(function(){return arguments})()}}return 1})", "T(()=>{class A{static{(()=>arguments)()}}})", "T(()=>{var C=class{static{this.n=C}};return typeof C.n})",
  "T(()=>{class A{static{this.n=A.name}}return A.n})", "T(()=>{var A=class{static{this.n=this.name}};return A.n})", "T(()=>{class A{static{throw new RangeError('sb')}}})",
  "T(()=>{try{class A{static{throw 1}}}catch(e){return e}})", "T(()=>{class A{static x=A.y;static y=1}return A.x})", "T(()=>{class A{static x=B;}var B=1;return A.x})",
  "T(()=>{class A{static x=(()=>{try{return B}catch(e){return e.name}})()}class B{}return A.x})", "T(()=>{class A{static x=A}return A.x===A})",
  "T(()=>{var B=class A{static x=A};return B.x===B})", "T(()=>{var B=class A{static x=()=>A};var C=B;B=null;return C.x()===C})",
  "T(()=>{class A{static x=this.y;static y=2}return A.x})", "T(()=>{class A{static get [Symbol.species](){return this}}class B extends A{}return B[Symbol.species]===B})",
  "T(()=>{class A{static get g(){return this.v}}class B extends A{static v=9}return [A.g,B.g]})", "T(()=>{class A{static get g(){return this.v}}class B extends A{static set g(x){}}return B.g})",
  "T(()=>{class A{static get g(){return 1}static set g(x){this._=x}}class B extends A{}B.g=4;return [B.g,Object.getOwnPropertyNames(B).join(),A._]})",
  "T(()=>{class A{static get g(){return 1}}class B extends A{}B.g=4;return B.g})", "T(()=>{'use strict';class A{static get g(){return 1}}class B extends A{}B.g=4})",
  "T(()=>{class A{static get g(){return 1}}class B extends A{}Object.defineProperty(B,'g',{value:5});return [A.g,B.g]})",
  "T(()=>{class A{static x=1}class B extends A{}B.x=2;return [A.x,B.x,Object.hasOwn(B,'x')]})", "T(()=>{class A{static x=1}class B extends A{}delete B.x;return [A.x,B.x]})",
  "T(()=>{class A{static m(){return 'A'}}class B extends A{static m(){return super.m()+'B'}}class C extends B{static m(){return super.m()+'C'}}return C.m()})",
  "T(()=>{class A{static m(){return this.name}}class B extends A{static m(){return super.m()}}return B.m()})", "T(()=>{class A{static m(){return this.name}}class B extends A{static m(){return super.m.call(A)}}return B.m()})",
  "T(()=>{class A{static m(){return this}}var m=A.m;return m()})", "T(()=>{class A{static m(){return this}}var {m}=A;return m()})", "T(()=>{class A{static m(){return typeof this}}return [A.m.call(1),A.m.call('s'),A.m.call(null)]})",
  "T(()=>{class A{static m(){return this}}return A.m.call(1)})", "T(()=>{class A{m(){return this}}return [A.prototype.m.call(1),typeof A.prototype.m.call('s')]})",
  "T(()=>{class A{static m(){return super.x}}Object.setPrototypeOf(A,{x:5});return A.m()})", "T(()=>{class A{static m(){return super.x}}var o={m:A.m,__proto__:{x:6}};return o.m()})",
  "T(()=>{class A{static m(){super.x=1;return Object.hasOwn(this,'x')}}return A.m()})", "T(()=>{class A{static m(){super.x=1;return Object.hasOwn(this,'x')}}return A.m.call({})})",
  "T(()=>{class A{static m(){delete super.x}}A.m()})", "T(()=>{class A{static m(){return super.x++}}Object.setPrototypeOf(A,{x:1});A.m();return [A.x,Object.getPrototypeOf(A).x]})",
  "T(()=>{class A{static m(){return super['toS'+'tring']===Function.prototype.toString}}return A.m()})", "T(()=>{class A{static m(){return super.x}}Object.setPrototypeOf(A,null);return A.m()})",
);

// ---- 5. accessor (não suportado no bun 1.4.2) e o nome `accessor` como campo, método ou identificador.
const accessorSrcs = [
  "class A{accessor x=1}", "class A{static accessor x=1}", "class A{accessor #x=1}", "class A{accessor x}", "class A{accessor 'x'=1}", "class A{accessor [k]=1}", "class A{accessor get x(){}}",
  "class A{accessor\nx=1}", "class A{accessor=1}", "class A{accessor;}", "class A{accessor(){}}", "class A{static accessor=1}", "class A{static accessor(){}}", "class A{get accessor(){return 1}}",
  "class A{accessor\n}", "class A{accessor\nx}", "class A{async accessor(){}}", "class A{*accessor(){}}", "class A{#accessor=1}", "class A{static accessor\nx}", "class A{accessor accessor=1}",
  "class A{accessor async x(){}}", "class A{accessor static x}", "class A{static static accessor=1}", "var accessor=1;class A{[accessor]=2}", "var o={accessor:1}", "var o={accessor(){}}",
  "var o={accessor x(){}}", "var accessor=1;accessor", "accessor\nx", "var o={get accessor(){return 3}};o.accessor", "(class{accessor})", "(class{accessor=1;x=accessor})",
];
for (const s of accessorSrcs) {
  add(`T(()=>eval(${JSON.stringify(s)}))`);
  add(`T(()=>{var A=eval(${JSON.stringify("(" + s + ")")});return Object.getOwnPropertyNames(A.prototype||A).join()})`);
  add(`T(()=>new Function(${JSON.stringify(s)})())`);
}
for (const d of ["@d class A{}", "class A{@d m(){}}", "class A{@d x=1}", "@d export class A{}", "class A{@d accessor x=1}", "class A{static @d m(){}}", "var f=@d class{}", "class A{@(d()) m(){}}"]) {
  add(`T(()=>eval(${JSON.stringify(d)}))`);
}

// ---- 6. Privados e brand checks (`#x in o`), carimbo por retorno de objeto.
const priv = [
  ["#x=1", "#x"], ["#m(){return 1}", "#m"], ["get #g(){return 1}", "#g"], ["set #s(v){}", "#s"], ["static #sx=1", "#sx"], ["static #sm(){}", "#sm"],
  ["get #ga(){return 1}set #ga(v){}", "#ga"], ["#x", "#x"], ["static get #sg(){return 1}", "#sg"], ["async #am(){}", "#am"], ["*#gm(){}", "#gm"],
];
const subjects = [
  "new A()", "new B()", "Object.create(new A())", "new Proxy(new A(),{})", "A.prototype", "A", "B", "Object.create(A.prototype)", "{}", "[]", "function(){}",
  "new C()", "1", "'s'", "Symbol()", "null", "undefined", "Object.freeze(new A())", "Object.seal(new A())", "new (class extends A{})()", "new (A.bind())()",
];
for (const [decl, name] of priv) for (const s of subjects) {
  add(`T(()=>{class A{${decl}static t(o){return ${name} in o}}class B extends A{}class C{${decl}}return A.t(${s})})`);
}
add(
  "T(()=>{class A{#x=1;static t(o){return #x in o}}return A.t(1)})", "T(()=>{class A{#x=1;static t(){return #x in 1}}return A.t()})", "T(()=>{class A{#x=1;static t(){return #y in {}}}})",
  "T(()=>{class A{static t(){return #x in {}}}})", "T(()=>{class A{#x;static t(o){return !(#x in o)}}return A.t({})})", "T(()=>{class A{#x;static t(o){return (#x in o)===true}}return A.t(new A())})",
  "T(()=>{class A{#x;static t(o){return #x in o in o}}return A.t(new A())})", "T(()=>{class A{#x;static t(o){return #x in #x in o}}})", "T(()=>{class A{#x;static t(o){return 1+#x in o}}})",
  "T(()=>{class A{#x;static t(o){return (#x) in o}}})", "T(()=>{class A{#x;static t(o){return o.#x}}return A.t({})})", "T(()=>{class A{#x;static t(o){o.#x=1}}return A.t({})})",
  "T(()=>{class A{#m(){}static t(o){o.#m=1}}return A.t(new A())})", "T(()=>{class A{#m(){}static t(o){o.#m()}}return A.t({})})", "T(()=>{class A{get #g(){return 1}static t(o){o.#g=1}}return A.t(new A())})",
  "T(()=>{class A{set #s(v){}static t(o){return o.#s}}return A.t(new A())})", "T(()=>{class A{#x=1;static t(o){return delete o.#x}}})", "T(()=>{class A{#x=1;static t(o){return o?.#x}}return [A.t(null),A.t(new A())]})",
  "T(()=>{class A{#x=1;static t(o){return o?.#x}}return A.t({})})", "T(()=>{class A{#x=1;static t(o){return o.#x++}}return [A.t(new A())]})", "T(()=>{class A{#x=1;static t(o){return o.#x??=5}}return A.t(new A())})",
  "T(()=>{class A{#x=1;static t(o){return o.#x**=2}}return A.t(new A())})", "T(()=>{class A{#x=1;static t(o){[o.#x]=[7];return o.#x}}return A.t(new A())})", "T(()=>{class A{#x=1;static t(o){({a:o.#x}={a:8});return o.#x}}return A.t(new A())})",
  "T(()=>{class A{#x=1;static t(o){for(o.#x of [3,4]);return o.#x}}return A.t(new A())})", "T(()=>{class A{#x=1;static t(o){return eval('o.#x')}}return A.t(new A())})", "T(()=>{class A{#x=1;static t(o){return eval('#x in o')}}return A.t(new A())})",
  "T(()=>{class A{#x=1;static t(o){return new Function('o','return o.#x')(o)}}return A.t(new A())})", "T(()=>{class A{#x=1;static t(o){return (0,eval)('o.#x')}}return A.t(new A())})",
  "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1;static t(o){return #x in o}}var o={};new B(o);return [B.t(o),B.t({})]})",
  "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1}var o={};new B(o);new B(o)})", "T(()=>{class A{constructor(o){return o}}class B extends A{#m(){}}var o={};new B(o);new B(o)})",
  "T(()=>{class A{constructor(o){return o}}class B extends A{x=1}var o={};new B(o);new B(o);return o.x})", "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1;g(){return this.#x}}var o=Object.freeze({});new B(o);return B.prototype.g.call(o)})",
  "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1;static g(o){return o.#x}}var p=new Proxy({},{});new B(p);return B.g(p)})", "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1;static g(o){return o.#x}}var t={};var p=new Proxy(t,{});new B(t);return B.g(p)})",
  "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1;static g(o){return o.#x}}var t={};var p=new Proxy(t,{});new B(p);return B.g(t)})", "T(()=>{class A{constructor(o){return o}}class B extends A{static #s=1;static g(o){return #s in o}}return [B.g(B),B.g(A)]})",
  "T(()=>{class A{constructor(o){return o}}class B extends A{#x=1;static g(o){return #x in o}}return B.g(new B(Object.preventExtensions({})))})", "T(()=>{class A{constructor(){return Object.preventExtensions({})}}class B extends A{x=1}return new B()})",
  "T(()=>{class A{constructor(){return Object.preventExtensions({})}}class B extends A{#x=1}new B()})", "T(()=>{class A{constructor(){return Object.preventExtensions({})}}class B extends A{x=1}new B()})",
  "T(()=>{class A{constructor(){return Object.freeze({})}}class B extends A{x=1}new B()})", "T(()=>{class A{constructor(){return Object.freeze({x:0})}}class B extends A{x=1}new B()})",
  "T(()=>{class A{#x=1;static g(o){return o.#x}}class B extends A{}return A.g(new B())})", "T(()=>{class A{#x=1;g(){return this.#x}}class B extends A{}return new B().g()})", "T(()=>{class A{#x=1;g(){return this.#x}}return A.prototype.g.call(Object.create(new A()))})",
  "T(()=>{class A{#x=1;g(){return this.#x}}return new Proxy(new A(),{}).g()})", "T(()=>{class A{#m(){return 1}g(){return this.#m()}}return new Proxy(new A(),{}).g()})", "T(()=>{class A{#m(){return 1}g(){return this.#m}}var a=new A();return a.g()===new A().g()})",
  "T(()=>{class A{#m(){return 1}get m(){return this.#m}}return typeof new A().m})", "T(()=>{class A{#m(){}static t(o){return o.#m.name}}return A.t(new A())})", "T(()=>{class A{get #g(){return 1}static t(o){return o.#g}}return A.t(new A())})",
  "T(()=>{class A{#x=1;#y=this.#x+1;g(){return this.#y}}return new A().g()})", "T(()=>{class A{#y=this.#x+1;#x=1}new A()})", "T(()=>{class A{#y=this.#m();#m(){return 5}g(){return this.#y}}return new A().g()})",
  "T(()=>{class A{x=this.#m();#m(){return 5}}return new A().x})", "T(()=>{class A{x=this.#f;#f=2}return new A().x})", "T(()=>{class A{static x=A.#s;static #s=2}})", "T(()=>{class A{static #s=2;static x=A.#s}return A.x})",
  "T(()=>{class A{static #s=2;static g(){return this.#s}}class B extends A{}return [A.g(),B.g()]})", "T(()=>{class A{static #s=2;static g(){return this.#s}}class B extends A{}return B.g()})",
  "T(()=>{class A{static #m(){return 3}static g(){return this.#m()}}class B extends A{}return B.g()})", "T(()=>{class A{static #m(){return 3}static g(){return this.#m()}}return A.g.call({})})",
  "T(()=>{class A{#x;constructor(){this.#x=1;this.#x=2}get x(){return this.#x}}return new A().x})", "T(()=>{class A{#x;constructor(){delete this.x}}return typeof new A()})",
  "T(()=>{class A{#x=1;static eq(a,b){return a.#x===b.#x}}return A.eq(new A(),new A())})", "T(()=>{class A{#a=1;#b=2;static s(o){return Object.getOwnPropertyNames(o).length+Reflect.ownKeys(o).length}}return A.s(new A())})",
  "T(()=>{class A{#a=1;static s(o){return JSON.stringify(o)+Object.keys(o).length}}return A.s(new A())})", "T(()=>{class A{#a=1}return Object.getOwnPropertyDescriptors(new A())})",
  "T(()=>{class A{#a=1}var a=new A();Object.freeze(a);return Object.isFrozen(a)})", "T(()=>{class A{#a=1;set(v){this.#a=v;return this.#a}}var a=Object.freeze(new A());return a.set(9)})",
  "T(()=>{class A{#a=1;static #b=2;static h(o){return [#a in o,#b in o]}}return [A.h(new A()),A.h(A)]})", "T(()=>{class A{#a=1;m(){return class{static h(o){return #a in o}}}}var H=new A().m();return [H.h(new A()),H.h({})]})",
  "T(()=>{class A{#a=1;m(){return o=>#a in o}}return new A().m()(new A())})", "T(()=>{class A{#a=1;m(){return {h(o){return #a in o}}}}return new A().m().h(new A())})", "T(()=>{class A{#a=1;m(){return function(o){return o.#a}}}return new A().m()(new A())})",
  "T(()=>{class A{#a=1;g(){return class B extends A{h(){return this.#a}}}}var B=new A().g();return new B().h()})", "T(()=>{var C;class A{#a=1;static{C=(o=>o.#a)}}return [C(new A()),typeof C]})",
  "T(()=>{class A{#a=1;static f(){return class{#a=2;static g(o){return o.#a}}}}var X=A.f();return X.g(new X())})", "T(()=>{class A{#a=1;static f(){return class{#a=2;static g(o){return o.#a}}}}var X=A.f();return X.g(new A())})",
  "T(()=>{class A{#a=1;static f(){return class{static g(o){return o.#a}}}}var X=A.f();return X.g(new A())})", "T(()=>{class A{#a=1;static f(){return class{static g(o){return #a in o}}}}var X=A.f();return [X.g(new A()),X.g(new X())]})",
  "T(()=>{class A{#a=1;static f(){return class{static g(o){return o.#a}}}}var X=A.f();return X.g(new X())})", "T(()=>{class A{#a}class A2{#a;static h(o){return o.#a}}return A2.h(new A())})",
  "T(()=>{class A{#a=1;static h(o){return #a in o}}var A2=A;return A2.h(new A())})", "T(()=>{function mk(){return class{#a=1;static h(o){return #a in o}}}var X=mk(),Y=mk();return [X.h(new X()),X.h(new Y())]})",
  "T(()=>{function mk(){return class{#a=1;static h(o){return o.#a}}}var X=mk(),Y=mk();return X.h(new Y())})", "T(()=>{function mk(){return class{static #a=1;static h(o){return #a in o}}}var X=mk(),Y=mk();return [X.h(X),X.h(Y)]})",
);

// ---- 7. extends null e não construtores.
const heritages = [
  "null", "undefined", "1", "'s'", "true", "{}", "[]", "Symbol()", "1n", "(()=>{})", "async function(){}", "function*(){}", "async function*(){}", "{m(){}}.m", "Math.max", "Math", "JSON", "Reflect", "globalThis",
  "function(){}.bind()", "new Proxy(function(){},{})", "new Proxy({},{})", "new Proxy(()=>{},{})", "Object.assign(function(){},{prototype:1})", "Object.assign(function(){},{prototype:undefined})",
  "Object.assign(function(){},{prototype:null})", "Object.assign(function(){},{prototype:Object.create(null)})", "Object.assign(function(){},{prototype:function(){}})", "Object.assign(function(){},{prototype:[]})",
  "Object.defineProperty(function(){},'prototype',{get(){throw new RangeError('hp')}})", "(0,function(){throw new RangeError('hf')})", "Object", "Function", "Symbol", "Proxy", "BigInt", "class{}", "class extends null{}",
  "new (class{})", "(class{}).bind()", "Date.now", "Array.prototype.push", "Object.create(Function.prototype)", "Object.setPrototypeOf(function(){},null)", "Object.setPrototypeOf(function(){},Array)",
];
for (const h of heritages) {
  add(`T(()=>{class A extends (${h}){}return [typeof A,Object.getPrototypeOf(A)===Function.prototype,Object.getPrototypeOf(A.prototype)===null,A.length]})`);
  add(`T(()=>{class A extends (${h}){}return new A()})`);
  add(`T(()=>{class A extends (${h}){constructor(){super()}}return new A()})`);
  add(`T(()=>{class A extends (${h}){constructor(){return Object.create(A.prototype)}}var a=new A();return [a instanceof A,Object.getPrototypeOf(a)===A.prototype]})`);
  add(`T(()=>{var A=class extends (${h}){};return A.name+Object.getPrototypeOf(A.prototype)})`);
  add(`T(()=>{class A extends (${h}){static m(){return super.m}}return A.m()})`);
}
add(
  "T(()=>{class A extends null{}return Reflect.ownKeys(A.prototype).join()})", "T(()=>{class A extends null{}return Reflect.construct(function(){},[],A) instanceof A})", "T(()=>{class A extends null{}return Object.getPrototypeOf(Reflect.construct(Object,[],A))===A.prototype})",
  "T(()=>{class A extends null{}class B extends A{}return new B()})", "T(()=>{class A extends null{constructor(){return {}}}class B extends A{constructor(){super()}}return Object.getPrototypeOf(new B())===Object.prototype})",
  "T(()=>{class A extends null{constructor(){return Object.create(null)}}return Object.getPrototypeOf(new A())})", "T(()=>{class A extends null{constructor(){super.x}}return new A()})", "T(()=>{class A extends null{constructor(){return 1}}return new A()})",
  "T(()=>{class A extends null{m(){return super.x}}return new (class extends A{constructor(){return {__proto__:A.prototype}}})().m()})", "T(()=>{class A extends null{static m(){return super.name}}return A.m()})",
  "T(()=>{class A extends null{}return A.prototype.constructor===A})", "T(()=>{class A extends null{}return A.prototype.toString})", "T(()=>{class A extends null{}return String(A).slice(0,9)})",
  "T(()=>{class A extends null{}return A.prototype instanceof Object})", "T(()=>{class A extends null{}return Object.prototype.toString.call(A.prototype)})", "T(()=>{class A extends null{}return A instanceof Function})",
  "T(()=>{class A extends null{}return typeof A.call})", "T(()=>{class A extends null{}A.prototype.x=1;return Reflect.getPrototypeOf(A.prototype)})", "T(()=>{class A extends null{}return Object.setPrototypeOf(A.prototype,Object.prototype)===A.prototype})",
  "T(()=>{class A extends null{}A()})", "T(()=>{class A extends null{}A.call({})})", "T(()=>{function F(){}class A extends null{}Object.setPrototypeOf(A,F);return new A() instanceof A})",
  "T(()=>{function F(){this.f=1}class A extends F{}F.prototype=null;return Object.getPrototypeOf(new A())===A.prototype})", "T(()=>{function F(){}class A extends F{}F.prototype=null;return Object.getPrototypeOf(A.prototype)===F.prototype})",
  "T(()=>{function F(){}F.prototype=1;class A extends F{}})", "T(()=>{function F(){}class A extends F{}return Object.getPrototypeOf(A.prototype)===F.prototype})", "T(()=>{var h=0;class A extends (h++,Object){}return h})",
  "T(()=>{var l=[];class A extends (l.push('h'),Object){[(l.push('k'),'a')](){}static [(l.push('s'),'b')](){}}return l.join()})", "T(()=>{class A extends A{}})", "T(()=>{class A extends (A,Object){}})",
  "T(()=>{var B=class A extends A{}})", "T(()=>{class A extends (()=>A)(){}})", "T(()=>{class A extends B{}class B{}})", "T(()=>{class A extends (class B extends A{}){}})", "T(()=>{class A extends Object,Array{}})",
  "T(()=>{class A extends (Object,Array){}return Object.getPrototypeOf(A)===Array})", "T(()=>{class A extends a=>1{}})", "T(()=>{class A extends async x{}})", "T(()=>{class A extends new Object{}})", "T(()=>{class A extends new Function{}return typeof A})",
  "T(()=>{class A extends Object?.constructor{}return typeof A})", "T(()=>{class A extends {a:Object}.a{}return Object.getPrototypeOf(A)===Object})", "T(()=>{class A extends [Object][0]{}return Object.getPrototypeOf(A)===Object})",
  "T(()=>{class A extends (void 0){}})", "T(()=>{class A extends !Object{}})", "T(()=>{class A extends typeof Object{}})", "T(()=>{class A extends Object++{}})", "T(()=>{class A extends class{}{}return typeof A})",
  "T(()=>{class A extends function(){}.bind(){}return typeof A})", "T(()=>{class A extends Object `x`{}})", "T(()=>{class A extends ((x)=>x)(Object){}return Object.getPrototypeOf(A)===Object})", "T(()=>{class A extends this?.x{}})",
);

// ---- 8. Symbol.species em subclasses.
const speciesVals = [
  ["undefined", "undefined"], ["null", "null"], ["own", "this"], ["Object", "Object"], ["Array", "Array"], ["other", "class O extends Array{}"], ["number", "1"], ["string", "'s'"], ["throws", "(()=>{throw new RangeError('sp')})()"],
  ["arrow", "()=>({})"], ["fn", "function(n){return {len:n}}"], ["bound", "Array.bind(null)"], ["proxy", "new Proxy(Array,{})"], ["gen", "function*(){}"],
];
for (const [label, v] of speciesVals) {
  const sp = v.startsWith("class ") ? `static get [Symbol.species](){return (${v.replace(/^class O extends Array\{\}/, "class O extends Array{}")})}` : `static get [Symbol.species](){return ${v}}`;
  for (const m of ["map(x=>x)", "filter(x=>true)", "slice()", "splice(0,1)", "concat([4])", "flat()", "flatMap(x=>[x])"]) {
    add(`T(()=>{class A extends Array{${sp}}var a=A.from([1,2,3]);var r=a.${m};return [Object.getPrototypeOf(r)===A.prototype,Array.isArray(r),r.constructor===Array,S(r)]})`);
  }
}
for (const [label, v] of speciesVals) {
  const sp = `static get [Symbol.species](){return ${v}}`;
  for (const m of ["map(x=>x)", "filter(x=>true)", "slice()", "subarray(1)"]) for (const TA of ["Uint8Array", "Float32Array", "BigInt64Array"]) {
    const init = TA === "BigInt64Array" ? "[1n,2n,3n]" : "[1,2,3]";
    const fn = m.replace("x=>x", TA === "BigInt64Array" ? "x=>x" : "x=>x");
    add(`T(()=>{class A extends ${TA}{${sp}}var a=new A(${init});var r=a.${fn};return [Object.getPrototypeOf(r)===A.prototype,r.constructor===${TA},Object.prototype.toString.call(r),r.length]})`);
  }
  add(`T(()=>{class A extends ArrayBuffer{${sp}}var a=new A(4);var r=a.slice(1);return [Object.getPrototypeOf(r)===A.prototype,r.byteLength,r.constructor===ArrayBuffer]})`);
  add(`T(()=>{class A extends Promise{${sp}}var a=A.resolve(1);var r=a.then(x=>x);return [Object.getPrototypeOf(r)===A.prototype,r instanceof Promise]})`);
  add(`T(()=>{class A extends Promise{${sp}}var a=A.resolve(1);var r=a.finally(()=>1);return [Object.getPrototypeOf(r)===A.prototype,r instanceof Promise]})`);
  add(`T(()=>{class A extends Promise{${sp}}var r=A.resolve(1);return r.constructor===A})`);
  add(`T(()=>{class A extends RegExp{${sp}}var r=new A('a','g');var o=r[Symbol.split]('bab');return S(o)})`);
  add(`T(()=>{class A extends RegExp{${sp}}var r=new A('a','g');var o=r[Symbol.matchAll]('aba');return Object.prototype.toString.call(o)+typeof o.next})`);
  add(`T(()=>{class A extends RegExp{${sp}}return new A('a','g')[Symbol.replace]('aba','x')})`);
  add(`T(()=>{class A extends Map{${sp}}return [new A() instanceof Map,A[Symbol.species]===undefined]})`);
  add(`T(()=>{class A extends Array{${sp}}return Object.getOwnPropertyDescriptor(A,Symbol.species).set===undefined})`);
}
add(
  "T(()=>{var a=[1,2];a.constructor=undefined;return Object.getPrototypeOf(a.map(x=>x))===Array.prototype})", "T(()=>{var a=[1,2];a.constructor=1;return a.map(x=>x)})", "T(()=>{var a=[1,2];a.constructor=null;return a.map(x=>x)})",
  "T(()=>{var a=[1,2];a.constructor={};return Array.isArray(a.map(x=>x))})", "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:null};return Array.isArray(a.map(x=>x))})", "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:function(n){return {n}}};return S(a.map(x=>x))})",
  "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:function(n){return {n}}};return S(a.filter(x=>true))})", "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:function(n){return {n}}};return S(a.slice())})",
  "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:function(n){return Object.freeze({n})}}};return S(a.map(x=>x))})", "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:function(n){return Object.freeze({})}};return S(a.map(x=>x))})",
  "T(()=>{var a=[1,2];a.constructor={[Symbol.species]:function(n){return {get 0(){return 1},set 0(v){throw new EvalError('s0')}}}};return S(a.map(x=>x))})",
  "T(()=>{var a=[1,2];var l=[];a.constructor={[Symbol.species]:function(n){l.push('c'+n);return {}}};a.map(x=>x);a.filter(x=>x);a.slice(1);a.splice(0,1);return l.join()})",
  "T(()=>{var a=[1,2];var l=[];a.constructor={[Symbol.species]:function(n){l.push('c'+n);return {}}};a.concat([3]);a.flat();a.flatMap(x=>x);return l.join()})",
  "T(()=>{var a=new Array(2**32-1).fill?1:0;return a})", "T(()=>{class A extends Array{}return [A.of(1,2) instanceof A,A.from('ab') instanceof A,Array.of.call(A,1) instanceof A,Array.from.call(Object,[1]) instanceof Object]})",
  "T(()=>{class A extends Array{}var a=new A(1,2,3);return [a.length,a.map(x=>x) instanceof A,[].concat(a) instanceof A,a.concat([]) instanceof A,Array.prototype.concat.call(a) instanceof A]})",
  "T(()=>{class A extends Array{}var a=new A(3);return [a.length,Object.keys(a).length,a.map(x=>x).length]})", "T(()=>{class A extends Array{constructor(...x){super(...x);this.k=1}}var a=new A(1,2);return [a.k,a.length,a.map(x=>x).k,a.slice().k]})",
  "T(()=>{class A extends Array{constructor(n){super(n);this.n=n}}return new A(3).map(x=>x).n})", "T(()=>{class A extends Array{constructor(n){super();this.n=n}}return new A(3).filter(x=>x).n})",
  "T(()=>{class A extends Array{constructor(){throw new RangeError('ac')}}var a=Array.from([1]);a.constructor=A;return a.map(x=>x)})", "T(()=>{class A extends Array{}A.prototype.constructor=undefined;return new A(1).map(x=>x) instanceof A})",
  "T(()=>{class A extends Array{}A.prototype.constructor=Array;return new A(1).map(x=>x) instanceof A})", "T(()=>{class A extends Array{}Object.defineProperty(A.prototype,'constructor',{get(){throw new EvalError('cg')}});return new A(1).map(x=>x)})",
  "T(()=>{class A extends Array{}var a=new A(1);a.constructor=Object;return a.map(x=>x) instanceof Array})", "T(()=>{var a=[1];a.constructor=Object;return Object.getPrototypeOf(a.map(x=>x))===Array.prototype})",
  "T(()=>{var a=[1];a.constructor=Object;return a.map(x=>x) instanceof Array})", "T(()=>{var a=[1];a.constructor=new ShadowRealm().evaluate('Array');})",
  "T(()=>{var A=Array;var r=new ShadowRealm();var X=r.evaluate('Array');return [typeof X,X===A]})", "T(()=>{var X=new ShadowRealm().evaluate('class X extends Array{}; X');return typeof X})",
  "T(()=>{class A extends Array{}return [A[Symbol.species]===A,Array[Symbol.species]===Array,Promise[Symbol.species]===Promise,RegExp[Symbol.species]===RegExp,Map[Symbol.species]===Map,Set[Symbol.species]===Set,ArrayBuffer[Symbol.species]===ArrayBuffer]})",
  "T(()=>{var T=Object.getPrototypeOf(Uint8Array);class A extends Uint8Array{}return [T[Symbol.species]===T,A[Symbol.species]===A,Object.getOwnPropertyDescriptor(T,Symbol.species).get.name]})",
  "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return Uint16Array}}return new A([1,2]).map(x=>x+1).constructor===Uint16Array})", "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return Array}}return new A([1,2]).map(x=>x+1)})",
  "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return function(n){return new Uint8Array(1)}}}return new A([1,2]).map(x=>x+1)})", "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return function(n){return new Uint8Array(5)}}}return new A([1,2]).map(x=>x+1).length})",
  "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return function(n){return new Uint8Array(5)}}}return new A([1,2]).filter(x=>true).length})", "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return function(){return new Uint8Array(1).buffer}}}return new A([1,2]).slice()})",
  "T(()=>{class A extends Uint8Array{static get [Symbol.species](){return BigInt64Array}}return new A([1,2]).map(x=>x)})", "T(()=>{class A extends BigInt64Array{static get [Symbol.species](){return Uint8Array}}return new A([1n]).map(x=>x)})",
);

// ---- 9. toString de classes e membros.
const srcs = [
  "class A{}", "class  A  {  }", "class A extends Object{}", "class A{constructor(){}}", "class A{ /*c*/ m(){} // t\n}", "class A{static m(){}}", "class A{static async *m(){}}", "class A{get x(){return 1}set x(v){}}",
  "class A{[`k${1}`](){}}", "class A{#p=1;static #q(){}}", "class A{x=1;y;static z=2;static{this.w=3}}", "class A{'s'(){}1(){}[Symbol.iterator](){}}", "class A{async m(){}*g(){}async*ag(){}}", "(class{})", "(class B{})",
  "(class extends Array{})", "class A{\n\tconstructor(a,\tb){ this.a=a }\n}", "class A{static x=class B{}}", "class A{m(){return class{}}}", "class A{static [(()=>'k')()](){}}", "class A{}", "class A{ ; ; m(){} ; }",
  "class A{static\nm(){}}", "class A{get\nx(){return 1}}", "class A{static get x(){return 1}static set x(v){}}", "class A{'use strict'(){}}", "class A{constructor(){super}}",
];
for (const s of srcs) {
  const lit = JSON.stringify(s);
  const decl = /^\(/.test(s) ? s : `(${s})`;
  add(`T(()=>{var C=eval(${JSON.stringify(decl)});return String(C)})`);
  add(`T(()=>{var C=eval(${JSON.stringify(decl)});return C.toString===Function.prototype.toString?String(C)===${lit.replace(/^"\(|\)"$/g, '"')}||String(C).length:0})`);
  add(`T(()=>{var C=eval(${JSON.stringify(decl)});return Function.prototype.toString.call(C).length+','+String(C.prototype&&C.prototype.constructor===C)})`);
  add(`T(()=>{var C=eval(${JSON.stringify(decl)});return String(C.bind(null))})`);
  add(`T(()=>{var C=eval(${JSON.stringify(decl)});return String(new Proxy(C,{}))})`);
  add(`T(()=>{var C=eval(${JSON.stringify(decl)});return C.name+'|'+C.length+'|'+Object.getOwnPropertyNames(C).join()})`);
}
const memberSrcs = [
  "m(){}", "static m(){}", "async m(){}", "*m(){}", "async*m(){}", "get m(){return 1}", "set m(v){}", "static get m(){return 1}", "[`m`](){}", "'m'(){}", "1(){}", "#m(){}", "static #m(){}", "get #m(){return 1}",
  "m( a , b ){ return a }", "m(){/*c*/}", "static async*m(){}", "m=()=>1", "m=function(){}", "m=function n(){}", "static m=class{}", "static m=async()=>{}",
];
for (const m of memberSrcs) {
  const name = /#/.test(m) ? "#m" : /^static\s+m=|^m=/.test(m) ? "m" : "m";
  const access = /#m/.test(m) ? "A.t()" : /=/.test(m) ? (/static/.test(m) ? "A.m" : "new A().m") : /^(\w+ )*(static )/.test(m) && !/\bstatic\b/.test(m) ? "A.prototype.m" : /static/.test(m) ? (/get /.test(m) ? "Object.getOwnPropertyDescriptor(A,'m').get" : "A.m") : /get /.test(m) ? "Object.getOwnPropertyDescriptor(A.prototype,'m').get" : /set /.test(m) ? "Object.getOwnPropertyDescriptor(A.prototype,'m').set" : /\[`m`\]|'m'|1\(/.test(m) ? "Object.values(Object.getOwnPropertyDescriptors(A.prototype)).filter(d=>d.value&&d.value.length>=0).pop().value" : "A.prototype.m";
  const tHelper = /#m/.test(m) ? (/get /.test(m) ? "static t(){return String(Object.getOwnPropertyDescriptor(A.prototype,'constructor').value)}" : /static/.test(m) ? "static t(){return String(A.#m)}" : "static t(){return String(new A().#m)}") : "";
  if (/#m/.test(m)) {
    if (/get /.test(m)) continue;
    add(`T(()=>{class A{${m}${tHelper}}return A.t()})`);
  } else {
    add(`T(()=>{class A{${m}}var f=${access};return typeof f==='function'?String(f):typeof f})`);
    add(`T(()=>{class A{${m}}var f=${access};return typeof f==='function'?f.name+'|'+f.length+'|'+Object.getOwnPropertyNames(f).join():typeof f})`);
  }
}
add(
  "T(()=>String(class{static name(){}}))", "T(()=>(class{static name(){}}).name)", "T(()=>typeof (class{static name='x'}).name)", "T(()=>(class{static name='x'}).name)", "T(()=>Object.getOwnPropertyNames(class{static name(){}}).join())",
  "T(()=>Object.getOwnPropertyNames(class{static length=3}).join())", "T(()=>(class{static length=3}).length)", "T(()=>Object.getOwnPropertyNames(class{static prototype(){}}))", "T(()=>Object.getOwnPropertyNames(class{static ['prototype'](){}}))",
  "T(()=>Object.getOwnPropertyNames(class{static get prototype(){return 1}}))", "T(()=>Object.getOwnPropertyNames(class{static prototype=1}))", "T(()=>Object.getOwnPropertyNames(class{static constructor(){}}).join())",
  "T(()=>Object.getOwnPropertyNames(class{static ['constructor'](){}}).join())", "T(()=>Object.getOwnPropertyNames(class{['constructor'](){}}.prototype).join())", "T(()=>Object.getOwnPropertyNames(class{constructor(){}}.prototype).join())",
  "T(()=>Object.getOwnPropertyNames(class{get constructor(){}}))", "T(()=>Object.getOwnPropertyNames(class{*constructor(){}}))", "T(()=>Object.getOwnPropertyNames(class{async constructor(){}}))", "T(()=>Object.getOwnPropertyNames(class{constructor(){}constructor(){}}))",
  "T(()=>Object.getOwnPropertyNames(class{constructor=1}))", "T(()=>Object.getOwnPropertyNames(class{'constructor'=1}))", "T(()=>Object.getOwnPropertyNames(class{#constructor=1}))", "T(()=>Object.getOwnPropertyNames(class{static constructor=1}))",
  "T(()=>Object.getOwnPropertyDescriptor(class A{},'prototype'))", "T(()=>Object.getOwnPropertyDescriptor(class A{m(){}}.prototype,'m'))", "T(()=>Object.getOwnPropertyDescriptor(class A{},'name'))", "T(()=>Object.getOwnPropertyDescriptor(class A{},'length'))",
  "T(()=>Object.getOwnPropertyDescriptor(class A{static x=1},'x'))", "T(()=>Object.getOwnPropertyDescriptor(class A{x=1;static m(){}},'m'))", "T(()=>Object.getOwnPropertyDescriptor(class A{get x(){return 1}}.prototype,'x').get.name)",
  "T(()=>Object.getOwnPropertyDescriptor(class A{set x(v){}}.prototype,'x').set.name)", "T(()=>Object.getOwnPropertyDescriptor(class A{static get [Symbol.iterator](){return 1}},Symbol.iterator).get.name)", "T(()=>(class{[Symbol.iterator](){}}).prototype[Symbol.iterator].name)",
  "T(()=>(class{[Symbol()](){}}).prototype)", "T(()=>Reflect.ownKeys(class{static [Symbol()](){}}).length)", "T(()=>Object.getPrototypeOf(class{})===Function.prototype)", "T(()=>(class{}).prototype.constructor.name)",
  "T(()=>class A{}.prototype.constructor===undefined)", "T(()=>typeof class{}.prototype)", "T(()=>Object.prototype.toString.call(class{}))", "T(()=>Object.prototype.toString.call(class{static [Symbol.toStringTag]='X'}))",
  "T(()=>Object.prototype.toString.call(new (class{get [Symbol.toStringTag](){return 'Y'}})))", "T(()=>String(new (class{})))", "T(()=>String(new (class{toString(){return 'ts'}})))", "T(()=>`${new (class{[Symbol.toPrimitive](){return 'tp'}})}`)",
  "T(()=>String(new (class extends Error{})('m')))", "T(()=>String(new (class X extends Error{})('m')))", "T(()=>{class X extends Error{}X.prototype.name='XE';return String(new X('m'))})", "T(()=>{class X extends Error{get name(){return 'GE'}}return String(new X('m'))})",
  "T(()=>{class X extends Error{constructor(){super('c');this.name='SelfE'}}return String(new X())})", "T(()=>{class X extends Error{}return Object.getOwnPropertyNames(new X('m')).join()})", "T(()=>{class X extends Error{}return Object.prototype.toString.call(new X())})",
  "T(()=>{class X extends TypeError{}return [new X() instanceof TypeError,new X() instanceof Error,new X().name,X.name,Object.getPrototypeOf(X)===TypeError]})", "T(()=>{class X extends Error{}return new X('m',{cause:1}).cause})",
  "T(()=>{class X extends Error{}return Object.getOwnPropertyNames(new X('m',{cause:1})).join()})", "T(()=>{class X extends AggregateError{}return [new X([1],'m').errors.length,String(new X([1],'m'))]})",
  "T(()=>{class X extends Error{constructor(m){super(m);Error.captureStackTrace(this,X)}}return typeof new X('m').stack})", "T(()=>{class X extends Error{}return new X('m').stack.split('\\n')[0]})",
  "T(()=>{class X extends Error{}var e=new X('m');return Object.getOwnPropertyDescriptor(e,'message').enumerable})", "T(()=>{class X extends Error{message='fm'}return new X('m').message})",
  "T(()=>{class X extends Error{}return Error.prototype.toString.call(new X(undefined))})", "T(()=>{class X extends Error{}X.prototype.message='pm';return String(new X())+'|'+Object.hasOwn(new X(),'message')})",
);

// ---- 10. Reflect.construct: validação de argumentos por tipo de alvo e de newTarget.
const rcTargets = ["function(){}", "class{}", "()=>1", "async function(){}", "function*(){}", "{m(){}}.m", "Math.max", "Symbol", "Proxy", "Object", "Array", "new Proxy(function(){},{})", "new Proxy(()=>1,{})", "function(){}.bind()", "(()=>1).bind()", "1", "null", "{}"];
const rcArgs = ["[]", "[1]", "{length:1,0:5}", "undefined", "null", "1", "'ab'", "{}", "new Proxy([],{})", "{get length(){throw new RangeError('l')}}", "function(){}"];
for (const t of rcTargets) for (const a of rcArgs.slice(0, 5)) add(`T(()=>{var r=Reflect.construct(${t},${a});return typeof r})`);
for (const t of rcTargets.slice(0, 6)) for (const a of rcArgs) add(`T(()=>{var r=Reflect.construct(function(){this.a=arguments.length},${a},${t});return r.a+','+typeof r})`);
add(
  "T(()=>Reflect.construct())", "T(()=>Reflect.construct(function(){}))", "T(()=>Reflect.construct(class{},[],undefined))", "T(()=>Reflect.construct(class{},[],null))", "T(()=>Reflect.construct(class{},[],1))",
  "T(()=>{var l=[];var nt=new Proxy(function(){},{get(t,k){l.push(String(k));return t[k]}});Reflect.construct(class{},[],nt);return l.join()})",
  "T(()=>{var l=[];var nt=new Proxy(function(){},{get(t,k){l.push(String(k));return t[k]}});Reflect.construct(Array,[],nt);return l.join()})",
  "T(()=>{var l=[];var nt=new Proxy(function(){},{get(t,k){l.push(String(k));return t[k]}});Reflect.construct(Error,['m'],nt);return l.join()})",
  "T(()=>{var l=[];var nt=new Proxy(function(){},{get(t,k){l.push(String(k));return t[k]}});Reflect.construct(Map,[],nt);return l.join()})",
  "T(()=>{var l=[];var nt=new Proxy(function(){},{get(t,k){l.push(String(k));return t[k]}});Reflect.construct(function(){},[],nt);return l.join()})",
  "T(()=>{var l=[];var nt=new Proxy(class{},{get(t,k){l.push(String(k));return t[k]}});Reflect.construct(class extends Array{},[],nt);return l.join()})",
  "T(()=>{var l=[];var f=new Proxy(function(){},{construct(t,a,n){l.push('c'+a.length+(n===f));return {}}});new f(1,2);Reflect.construct(f,[1],Array);return l.join()})",
  "T(()=>{var f=new Proxy(function(){},{construct(){return 1}});new f()})", "T(()=>{var f=new Proxy(function(){},{construct(){return {}}});return typeof new f()})", "T(()=>{var f=new Proxy(function(){},{construct:null});return typeof new f()})",
  "T(()=>{var f=new Proxy(function(){},{construct:1});new f()})", "T(()=>{var f=new Proxy(()=>1,{construct(){return {}}});new f()})", "T(()=>{var f=new Proxy(class{},{apply(){return 1}});return f()})",
  "T(()=>{var f=new Proxy(class{},{});return f()})", "T(()=>{var f=new Proxy(class{},{get(t,k){return k==='prototype'?Array.prototype:t[k]}});return new f() instanceof Array})", "T(()=>{class A{}var f=new Proxy(A,{});return new f() instanceof A})",
  "T(()=>{class A{constructor(){this.t=new.target}}var f=new Proxy(A,{});return new f().t===f})", "T(()=>{class A{constructor(){this.t=new.target}}var f=new Proxy(A,{});return Reflect.construct(A,[],f).t===f})",
  "T(()=>{class A{}class B extends new Proxy(A,{}){}return new B() instanceof A})", "T(()=>{class A{constructor(){this.t=new.target}}class B extends new Proxy(A,{}){}return new B().t===B})",
  "T(()=>{var l=[];class A{}class B extends new Proxy(A,{get(t,k){l.push(String(k));return t[k]}}){}new B();return l.join()})", "T(()=>{var l=[];class A{}var P=new Proxy(A,{get(t,k){l.push(String(k));return t[k]}});class B extends P{}return l.join()})",
  "T(()=>{var l=[];var P=new Proxy(Array,{get(t,k){l.push(String(k));return t[k]}});class B extends P{}new B();return l.join()})", "T(()=>{var l=[];var P=new Proxy(Array,{construct(t,a,n){l.push('c');return Reflect.construct(t,a,n)}});class B extends P{}var b=new B(3);return l.join()+b.length+(b instanceof B)})",
  "T(()=>{var l=[];var P=new Proxy(Map,{construct(t,a,n){l.push(n.name);return Reflect.construct(t,a,n)}});class B extends P{}new B();return l.join()})", "T(()=>{var P=new Proxy(Map,{construct(){return {}}});class B extends P{}return new B() instanceof B})",
  "T(()=>{var P=new Proxy(Map,{construct(){return {}}});class B extends P{constructor(){super();this.z=1}}return Object.keys(new B()).join()})", "T(()=>{var P=new Proxy(Map,{construct(){return 1}});class B extends P{}new B()})",
  "T(()=>{var P=new Proxy(Error,{construct(t,a,n){var e=Reflect.construct(t,a,n);e.m='p';return e}});class B extends P{}return new B('x').m})", "T(()=>{class A{constructor(){return new Proxy({},{get(t,k){return 'px'}})}}class B extends A{}return new B().anything})",
  "T(()=>{class A{constructor(){return new Proxy({},{})}}class B extends A{#x=1;static h(o){return #x in o}}var b=new B();return B.h(b)})", "T(()=>{class A{constructor(){return new Proxy({},{set(){return false}})}}class B extends A{x=1}return new B()})",
  "T(()=>{class A{constructor(){return new Proxy({},{defineProperty(){return false}})}}class B extends A{x=1}new B()})", "T(()=>{class A{constructor(){return new Proxy({},{defineProperty(t,k,d){return Reflect.defineProperty(t,k,d)}})}}class B extends A{x=1}return Object.keys(new B()).join()})",
  "T(()=>{var l=[];class A{constructor(){return new Proxy({},{defineProperty(t,k,d){l.push(k+':'+d.writable+d.enumerable+d.configurable);return Reflect.defineProperty(t,k,d)}})}}class B extends A{x=1;y=2}new B();return l.join()})",
  "T(()=>{var l=[];class A{constructor(){return new Proxy({},{set(t,k,v){l.push('set '+k);return true},defineProperty(t,k,d){l.push('def '+k);return true}})}}class B extends A{x=1}new B();return l.join()})",
  "T(()=>{class A{}class B extends A{constructor(){super();this.x=new.target}}var b=Reflect.construct(B,[],Array);return [Object.getPrototypeOf(b)===Array.prototype,b.x===Array]})",
  "T(()=>{class A{}class B extends A{}var b=Reflect.construct(B,[],A);return [Object.getPrototypeOf(b)===A.prototype,b instanceof B]})", "T(()=>{class A{}class B extends A{}var b=Reflect.construct(A,[],B);return [Object.getPrototypeOf(b)===B.prototype,b instanceof B]})",
  "T(()=>{var r=new ShadowRealm();var F=r.evaluate('(function(){this.x=1})');return typeof new F()})", "T(()=>{var r=new ShadowRealm();var F=r.evaluate('(class{})');return typeof F})",
  "T(()=>{var r=new ShadowRealm();var F=r.evaluate('Array');return Reflect.construct(Array,[],F)})", "T(()=>{var r=new ShadowRealm();var F=r.evaluate('(class{})');return Object.getPrototypeOf(Reflect.construct(Array,[],F))===Array.prototype})",
  "T(()=>{var r=new ShadowRealm();return r.evaluate('class A extends Array{}; new A(2).length')})", "T(()=>{var r=new ShadowRealm();return r.evaluate('class A extends Error{}; new A(\"m\").message')})",
  "T(()=>{var r=new ShadowRealm();return r.evaluate('class A{static #s=1;static h(o){return #s in o}}; String(A.h(A))')})", "T(()=>{var r=new ShadowRealm();return r.evaluate('(class{static x=this.name}).x')})",
  "T(()=>{var r=new ShadowRealm();return r.evaluate('Reflect.construct(Map,[],Array) instanceof Map?\"m\":\"n\"')})", "T(()=>{var r=new ShadowRealm();return r.evaluate('new.target')})",
);

// ---- Execução.
const baseSources = [];
baseSources.push(...knownPrograms("class_builtin_bun.tsv", ["class_bun.tsv", "class_edge_bun.tsv", "ctor_this_bun.tsv", "subclass_edge_bun.tsv", "brand_bun.tsv", "proxy_class_bun.tsv", "symbol_species_bun.tsv", "reflect_bun.tsv", "arguments_super_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const PRELOAD = writeResultPreload();
let kept = 0;
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  let source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    // Processo fresco por programa: a ordem de reificação das tabelas estáticas do JSC depende do que rodou antes.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26 });
    if (child.status !== 0) {
      // Erro de sintaxe do programa inteiro: o caso testa o erro de parse, então refaz com o corpo dentro de eval direto.
      if (!/^T\(\(\)=>/.test(expr)) throw new Error(child.stderr || "filho falhou");
      const inner = expr.slice(6, -1);
      source = '"use strict";\n' + PRELUDE + `globalThis.R = T(()=>eval(${JSON.stringify("(()=>" + inner + ")()")}))`;
      const retry = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26 });
      if (retry.status !== 0) throw new Error(retry.stderr || "filho falhou");
      result = decodeResult(retry.stdout);
    } else result = decodeResult(child.stdout);
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
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("class_builtin", rows));
