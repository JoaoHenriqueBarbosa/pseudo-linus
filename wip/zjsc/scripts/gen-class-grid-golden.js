// Gera tests/golden/class_grid_bun.tsv: grade de classes medida no bun 1.4.2.
// Cobre a ordem de inicialização de campos públicos, privados e estáticos e de blocos static (combinações de três
// elementos), computed keys com efeitos, extends de null/função/Proxy/bound/arrow/generator, super() duas vezes, nunca,
// em arrow e em eval, this antes do super, retorno de objeto ou primitivo do construtor derivado, new.target em cadeias,
// Symbol.species (Array, typed arrays, Promise, RegExp, ArrayBuffer), métodos privados, `#x in obj`, brand checks,
// accessors estáticos, herança de builtins e mensagens exatas de TypeError/ReferenceError/SyntaxError.
// Programas cuja fonte já aparece nos goldens class_edge, subclass_edge, ctor_this, class_builtin, class e proxy_class
// são descartados. Cada programa roda num bun filho novo, em paralelo.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-class-grid-golden.js > tests/golden/class_grid_bun.tsv
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

const exprs = [];
const add = (...list) => exprs.push(...list);
const J = JSON.stringify;
// Corpo que registra o log `l` e captura exceção como entrada final.
const LOGGED = (inner) => `T(()=>{var l=[];function L(x){l.push(String(x));return x}function K(x){l.push("k"+x);return x}class B{constructor(){L("B")}}try{${inner}}catch(e){L("!"+e.name+": "+e.message)}return l.join()})`;

// ---- 1. Ordem de inicialização: todas as trincas de elementos.
const els = [
  ["a=L('a')", null], ["#p=L('p')", "p"], ["static s=L('s')", null], ["static #q=L('q')", "q"], ["static{L('blk')}", null],
  ["[K('1')]=L('v1')", null], ["static [K('2')]=L('v2')", null], ["[K('3')](){}", null], ["static get [K('4')](){return 1}", null],
  ["#m(){}", "m"], ["static #sm(){}", "sm"], ["b=L(typeof this.a)", null], ["static t=L(this.name)", null], ["static{L(typeof this.s)}", null],
  ["x=L(new.target&&new.target.name)", null], ["#r=L(#r in this)", "r"],
];
let n = 0;
for (let i = 0; i < els.length; i++) for (let j = 0; j < els.length; j++) for (let k = 0; k < els.length; k++) {
  const idx = [i, j, k];
  const privs = idx.map(x => els[x][1]).filter(Boolean);
  if (new Set(privs).size !== privs.length) continue;
  const body = idx.map(x => els[x][0]).join(";");
  const mode = n++ % 4;
  const her = mode === 0 ? "" : "extends B";
  const ctor = mode === 2 ? "constructor(){super();L('C')}" : mode === 3 ? "constructor(){L('C0');super();L('C1')}" : "";
  const sub = n % 3 === 0 ? "var D=class D extends C{y=L('y');#z=L('z');static u=L('u');constructor(){L('D0');super();L('D1')}};L('def');new D;" : "var C2=C;L('def');new C;L('end');new C;";
  add(LOGGED(`var C=class C ${her}{${body};${ctor}};${sub}`));
}
// Pares com extends de efeito e múltiplas derivações.
for (const a of els) for (const b of els) {
  if (a[1] && a[1] === b[1]) continue;
  add(LOGGED(`class C extends (L('ext'),B){${a[0]};${b[0]}};class D extends C{${b[0].replace(/#/g, "#d").replace(/\(#dr/, "(#dr")};${a[0].replace(/#/g, "#e")}};L('def');new D`));
}

// ---- 2. Computed keys com efeitos.
const ckinds = ["[K('1')]=L('v1')", "static [K('2')]=L('v2')", "[K('3')](){}", "static [K('4')](){}", "get [K('5')](){return 1}", "static get [K('6')](){return 1}", "static{L('b')}", "a=L('a')", "static s=L('s')"];
for (const a of ckinds) for (const b of ckinds) for (const ext of ["", "extends (L('ext'),B)"]) {
  add(LOGGED(`class C ${ext}{${a};${b}};L('def');new C`));
}
const keyVals = ["1", "-0", "1.5", "1n", "Symbol.iterator", "Symbol('d')", "null", "undefined", "true", "[]", "[1,2]", "{}", "{toString(){L('ts');return 'z'}}", "{toString(){throw new RangeError('ts')}}", "{valueOf(){return 'v'},toString:undefined}", "{[Symbol.toPrimitive](h){L(h);return 'p'}}", "()=>1", "'constructor'", "'prototype'", "'__proto__'", "2**32", "0x10", "''"];
for (const v of keyVals) {
  add(
    LOGGED(`class C{[${v}](){}};L(Reflect.ownKeys(C.prototype).map(String).join("|"))`),
    LOGGED(`class C{static [${v}](){}};L(Reflect.ownKeys(C).map(String).join("|"))`),
    LOGGED(`class C{[${v}]=1};L(Reflect.ownKeys(new C).map(String).join("|"))`),
    LOGGED(`class C{static [${v}]=1};L(Reflect.ownKeys(C).map(String).join("|"))`),
    LOGGED(`class C{get [${v}](){return 1}set [${v}](x){}};L(D(C.prototype,Reflect.ownKeys(C.prototype)[1]))`),
    LOGGED(`var o={[${v}]:L('v')};L(Reflect.ownKeys(o).map(String).join("|"));class C{[${v}]=L('f');static [${v}]=L('s')};new C`),
    LOGGED(`class C{static [${v}](){return 1}};L(Object.getOwnPropertyNames(C).map(k=>k+":"+(typeof C[k]==="function"?C[k].name:'-')).join())`),
  );
}
add(
  LOGGED("class C{[C.name](){}}"), LOGGED("class C extends C{}"), LOGGED("class C{static x=C.name}; L(C.x)"), LOGGED("class C{static [C.name]=1}"),
  LOGGED("var C=class N{static x=N.name;static [N.name]=1};L(Object.keys(C))"), LOGGED("class C{[eval('this')]=1}"), LOGGED("class C{x=eval('this.y')};L(new C().x)"),
  LOGGED("class C{static x=eval('this.name')};L(C.x)"), LOGGED("class C{x=eval('new.target')};L(new C().x)"), LOGGED("class C{x=eval('arguments')};new C"),
  LOGGED("class C{static{eval('arguments')}}"), LOGGED("class C{x=eval('super.y')};L(new C().x)"), LOGGED("class C extends B{x=eval('super()')};new C"),
  LOGGED("class C{x=()=>this};L(new C().x()instanceof C)"), LOGGED("class C{static x=()=>this};L(C.x()===C)"), LOGGED("class C{x=function(){return this}};L(new C().x()===undefined)"),
  LOGGED("class C{static{var v=1;L(v)}static{L(typeof v)}}"), LOGGED("class C{static{this.z=1}static{L(this.z)}}"), LOGGED("class C{static{L(typeof C)}}"),
  LOGGED("var C=class N{static{L(typeof N);L(N===this)}};L(typeof N)"), LOGGED("class C{static{throw new URIError('blk')}}"), LOGGED("class C{static x=(()=>{throw new EvalError('f')})()}"),
  LOGGED("class C{x=(()=>{throw new EvalError('f')})()};L('def');new C"), LOGGED("var n=0;class C{static a=++n;static b=++n;c=++n;d=++n};new C;new C;L(n)"),
  LOGGED("class C{static a=this.b;static b=1};L(C.a+','+C.b)"), LOGGED("class C{a=this.b;b=1};var c=new C;L(c.a+','+c.b)"), LOGGED("class C{a=this.b;b=2;constructor(){L('ctor '+this.a+this.b)}};new C"),
  LOGGED("class C extends B{a=L('fa');constructor(){L('pre');super();L('post '+this.a)}};new C"), LOGGED("class C extends B{a=L('fa');constructor(){try{super();super()}catch(e){L(e.message)}}};new C"),
  LOGGED("class C extends B{a=L('fa')};var o=new C;L(Object.keys(o))"), LOGGED("class A{constructor(){return {}}}class C extends A{#p=1;static has(o){return #p in o}};L(C.has(new C));"),
  LOGGED("class A{constructor(){return o}}var o={};class C extends A{#p=1;static has(o){return #p in o}};new C;L(C.has(o));try{new C}catch(e){L(e.message)}"),
  LOGGED("class A{constructor(){return o}}var o={};class C extends A{x=L('x')};new C;new C;L(Object.keys(o))"),
  LOGGED("class A{constructor(){return o}}var o={};class C extends A{static{}x;};L(Object.getOwnPropertyDescriptor(new C,'x')&&D(new C,'x'))"),
  LOGGED("class C{static x;static y=1;z;w=1};L(Object.keys(C)+'|'+Object.keys(new C))"), LOGGED("class C{'a b'=1;1=2;[Symbol.iterator]=3};L(Reflect.ownKeys(new C).map(String))"),
  LOGGED("class C{static name=1};L(C.name+' '+D(C,'name'))"), LOGGED("class C{static length=1};L(C.length)"), LOGGED("class C{static prototype2=1;static ['prototype']=2}"),
  LOGGED("class C{static name(){}};L(typeof C.name)"), LOGGED("class C{static get name(){return 'g'}};L(C.name+D(C,'name'))"), LOGGED("class C{constructor(){}static constructor(){return 7}};L(C.constructor())"),
);

// ---- 3. extends: heritage x operação.
const heritages = [
  "null", "undefined", "1", "'s'", "{}", "[]", "Symbol()", "true", "function(){}", "()=>{}", "function*(){}", "async function(){}", "async()=>{}", "async function*(){}", "(function(){}).bind()",
  "class{}.bind()", "new Proxy(function(){},{})", "new Proxy(class{},{})", "new Proxy({},{})", "new Proxy(function(){},{construct(){return {pc:1}}})", "new Proxy(function(){},{get(t,k){return k==='prototype'?null:t[k]}})",
  "class{}", "class extends Object{}", "({m(){}}).m", "({get g(){return 1}}).g", "Math.max", "Math", "Object", "Array", "Date", "Reflect", "Function", "Function.prototype", "Object.prototype", "Symbol", "BigInt",
  "Object.assign(function(){},{prototype:null})", "Object.assign(function(){},{prototype:1})", "Object.assign(function(){},{prototype:{}})", "Object.assign(function(){},{prototype:()=>{}})",
  "Object.assign(function(){},{prototype:undefined})", "(()=>{var f=function(){};Object.setPrototypeOf(f,null);return f})()", "(()=>{var p=Proxy.revocable(function(){},{});p.revoke();return p.proxy})()",
  "new Proxy(function(){},{get(){throw new RangeError('g')}})", "class{static get prototype2(){}}", "(function(){}).bind().bind()", "Object.defineProperty(function(){},'prototype',{get(){throw new EvalError('p')}})",
  "Object.defineProperty(function(){},'prototype',{value:null,writable:true})", "Promise", "Error", "Map", "RegExp", "String", "Number", "Boolean", "WeakRef", "Uint8Array", "ArrayBuffer",
];
const hops = [
  (h) => `class C extends (${h}){};return typeof C`,
  (h) => `class C extends (${h}){};return Object.getPrototypeOf(C)===Function.prototype`,
  (h) => `class C extends (${h}){};return S(Object.getPrototypeOf(C.prototype))`,
  (h) => `class C extends (${h}){};return Object.getPrototypeOf(C.prototype)===null`,
  (h) => `class C extends (${h}){};return S(new C)`,
  (h) => `class C extends (${h}){constructor(){super()}};return S(new C)`,
  (h) => `class C extends (${h}){constructor(){}};return S(new C)`,
  (h) => `class C extends (${h}){constructor(){return {}}};return S(new C)`,
  (h) => `class C extends (${h}){};return C()`,
  (h) => `class C extends (${h}){};return S(Reflect.construct(C,[],Object))`,
  (h) => `class C extends (${h}){};return C.name+C.length`,
  (h) => `var H=${h};class C extends H{};return Object.getPrototypeOf(C)===H`,
  (h) => `var H=${h};class C extends H{};return new C instanceof C`,
  (h) => `var H=${h};class C extends H{};return C.prototype.constructor===C`,
  (h) => `var C=class extends (${h}){};return C.name===""&&Object.getOwnPropertyNames(C).join()`,
  (h) => `class C extends (${h}){static s(){return super.s}};return typeof C.s()`,
  (h) => `class C extends (${h}){m(){return super.m}};return typeof C.prototype.m.call({})`,
  (h) => `class C extends (${h}){constructor(){var x=()=>super();x()}};return Object.keys(new C)`,
];
for (const h of heritages) for (const op of hops) add(`T(()=>{${op(h)}})`);
add(
  "T(()=>{class C extends (class A{static x=1}){};return C.x+Object.hasOwn(C,'x')})",
  "T(()=>{function F(){this.a=1}F.prototype.p=2;class C extends F{};var c=new C;return c.a+','+c.p+','+Object.keys(c)})",
  "T(()=>{function F(){return {r:1}}class C extends F{};return S(new C)})",
  "T(()=>{function F(){return 1}class C extends F{};return S(new C)})",
  "T(()=>{function F(){}F.prototype=null;class C extends F{};return Object.getPrototypeOf(C.prototype)})",
  "T(()=>{var order=[];class C extends (order.push('h'),Object){[(order.push('k'),'m')](){}};return order.join()})",
  "T(()=>{var order=[];try{class C extends (order.push('h'),1){[(order.push('k'),'m')](){}}}catch(e){order.push(e.name)}return order.join()})",
  "T(()=>{class A{}class C extends A{};A.prototype.z=1;return new C().z})",
  "T(()=>{class A{}class C extends A{};Object.setPrototypeOf(C,null);return new C})",
  "T(()=>{class A{constructor(){this.t='A'}}class B2{constructor(){this.t='B'}}class C extends A{};Object.setPrototypeOf(C,B2);return new C().t})",
  "T(()=>{class A{}class C extends A{constructor(){super()}};Object.setPrototypeOf(C,function(){this.z=1});return S(new C)})",
  "T(()=>{class A{}class C extends A{constructor(){super()}};Object.setPrototypeOf(C,()=>1);return S(new C)})",
  "T(()=>{class A{}class C extends A{constructor(){super()}};Object.setPrototypeOf(C,null);return S(new C)})",
  "T(()=>{class A{}class C extends A{constructor(){super()}};Object.setPrototypeOf(C,Object.setPrototypeOf(function(){},null));return S(new C)})",
);

// ---- 4. super() e retorno do construtor derivado.
const bases = {
  plain: "class B{constructor(){this.b=1}}",
  retObj: "class B{constructor(){return {o:1}}}",
  retPrim: "class B{constructor(){return 5}}",
  retNull: "class B{constructor(){return null}}",
  fn: "function B(){this.f=1}",
  throws: "class B{constructor(){throw new RangeError('b')}}",
  args: "class B{constructor(...a){this.n=a.length;this.a=a.join()}}",
  nt: "class B{constructor(){this.nt=new.target.name}}",
  retFn: "class B{constructor(){return function(){}}}",
  retSym: "class B{constructor(){return Symbol()}}",
};
const sbodies = [
  "super()", "super();super()", "", "return", "return {}", "return 1", "return undefined", "return null", "return 'x'", "return Symbol()", "return function(){}", "return []",
  "this.x=1;super()", "super();this.x=1", "(()=>super())()", "(()=>{super()})();this.z=1", "var f=()=>super();f();f()", "eval('super()')", "eval('super()');this.q=1", "eval('super();super()')",
  "(()=>this)()", "var a=()=>this;super();return a()", "var a=()=>this;try{a()}catch(e){this_e=e.message}super()", "try{super();super()}catch(e){this.e=e.message}", "try{this.x}catch(e){}super()",
  "super(1,2,3)", "super(...[1,2])", "super(...arguments)", "super.m", "super.x=1;super()", "typeof super.x", "delete super.x", "super();return 7", "super();return {r:1}", "super();return this",
  "return super()", "if(0)super()", "for(;;){super();break}", "super(),super()", "return (super(),1)", "var s=()=>{return super()};return s()", "var s=()=>super();s();return s", "new (class{constructor(){}})",
  "var t=this", "super(this)", "super(()=>this)", "super(new.target.name)", "this.constructor", "typeof this", "(function(){return this})()", "{super()}", "function g(){}super()", "label:{super();break label}",
  "Reflect.construct(B,[],new.target);super()", "super(Reflect.construct(B,[],new.target))", "return new.target", "return Object.getPrototypeOf(new.target)===B", "super();return new.target.name",
];
for (const [bn, bsrc] of Object.entries(bases)) for (const body of sbodies) {
  add(`T(()=>{var this_e;${bsrc};class C extends B{constructor(){${body}}};var r=new C;return S(r)+" "+(r instanceof C)+" "+(r instanceof B)})`);
}
add(
  "T(()=>{class B{};class C extends B{};return S(new C)+typeof C.prototype.constructor})", "T(()=>{class B{constructor(){this.n=arguments.length}};class C extends B{};return new C(1,2,3).n})",
  "T(()=>{class B{constructor(){this.n=arguments.length}};class C extends B{};return new C(...[1,2]).n})", "T(()=>{var o={};Object.defineProperty(Array.prototype,Symbol.iterator,{value:function*(){yield 9},configurable:true});class B{constructor(...a){this.a=a}};class C extends B{};var r=new C(1,2);delete Array.prototype[Symbol.iterator];return S(r.a)})",
  "T(()=>{class B{};class C extends B{constructor(){super();return}};return S(new C)})", "T(()=>{class B{};class C extends B{constructor(){return}};return S(new C)})",
  "T(()=>{class B{};class C extends B{constructor(){super();this.a=1;return undefined}};return S(new C)})", "T(()=>{class C{constructor(){return 1}};return S(new C)})",
  "T(()=>{class C{constructor(){return {z:1}}};return S(new C)})", "T(()=>{class C{constructor(){return null}};return S(new C)})", "T(()=>{class C{constructor(){return this}};return S(new C)})",
  "T(()=>{class C{constructor(){super.x}};return S(new C)})", "T(()=>{class C{constructor(){super.x=1}};return S(new C)})", "T(()=>{class C{m(){return super.toString===Object.prototype.toString}};return new C().m()})",
  "T(()=>{class B{constructor(){this.v=new.target===C}};class C extends B{};return new C().v})", "T(()=>{class B{static make(){return new this}};class C extends B{};return new C.make() instanceof C})",
  "T(()=>{class B{static make(){return new this}};class C extends B{};return C.make() instanceof C})", "T(()=>{class B{m(){return 'B'}};class C extends B{m(){return super.m()+'C'}};return new C().m()})",
  "T(()=>{class B{m(){return this.n}};class C extends B{n=5;m(){return super.m.call({n:6})}};return new C().m()})", "T(()=>{class B{};class C extends B{m(){super.z=1;return Object.keys(this)}};return new C().m()})",
  "T(()=>{class B{};Object.defineProperty(B.prototype,'z',{set(v){this.s=v}});class C extends B{m(){super.z=1;return Object.keys(this)}};return new C().m()})",
  "T(()=>{class B{};Object.defineProperty(B.prototype,'z',{value:1,writable:false});'use strict';class C extends B{m(){super.z=2}};return new C().m()})",
  "T(()=>{class B{static sm(){return 'bs'}};class C extends B{static sm(){return super.sm()+'c'}};return C.sm()})", "T(()=>{class C{static m(){return super.name}};return C.m()})",
  "T(()=>{class C{static x=super.toString===Function.prototype.toString};return C.x})", "T(()=>{class B{static x=1};class C extends B{static y=super.x};return C.y})",
  "T(()=>{var o={m(){return super.toString===Object.prototype.toString}};return o.m()})", "T(()=>{var o={__proto__:{x:1},m(){return super.x}};return o.m()})",
  "T(()=>{var o={__proto__:{x:1},m(){return ()=>super.x}};return o.m()()})", "T(()=>{var o={__proto__:{x:1},m:()=>1};return o.m()})",
  "T(()=>{class B{constructor(){this.k=1}};class C extends B{constructor(){var x=super();return x}};return S(new C)})", "T(()=>{class B{};class C extends B{constructor(){var x=super();this.same=x===this}};return S(new C)})",
);

// ---- 5. new.target em cadeias.
const NTC = "var l=[];class A{constructor(){l.push('A:'+(new.target&&new.target.name))}}class B extends A{constructor(){super();l.push('B:'+new.target.name)}}class C extends B{constructor(){super();l.push('C:'+new.target.name)}}";
const ntTargets = ["A", "B", "C", "Object", "function X(){}", "class Y{}", "new Proxy(C,{})", "C.bind()", "Object.assign(function Z(){},{prototype:A.prototype})", "Object.assign(function W(){},{prototype:null})", "Object.assign(function V(){},{prototype:1})", "(()=>1)", "function*g(){}", "Math.max", "1", "null", "undefined"];
for (const start of ["A", "B", "C"]) for (const t of ntTargets) {
  add(`T(()=>{${NTC};try{var r=Reflect.construct(${start},[],${t});l.push(Object.getPrototypeOf(r)===${start}.prototype)}catch(e){l.push(e.name+': '+e.message)}return l.join()})`);
}
add(
  `T(()=>{${NTC};new C;new B;new A;return l.join()})`, `T(()=>{${NTC};Reflect.construct(A,[],C);return l.join()})`, `T(()=>{${NTC};A.call({});return l.join()})`,
  "T(()=>{function F(){return new.target}return [F()===undefined,new F()instanceof F,Reflect.construct(F,[],Object)===Object.prototype]})",
  "T(()=>{function F(){return new.target}return F.call({})})", "T(()=>{function F(){return new.target}return Reflect.apply(F,{},[])})", "T(()=>{function F(){return new.target}return new F===undefined})",
  "T(()=>{function F(){return ()=>new.target}return new F()===undefined})", "T(()=>{function F(){this.t=(()=>new.target)()}return new F().t===F})", "T(()=>{function F(){this.t=eval('new.target')}return new F().t===F})",
  "T(()=>{function F(){return eval('new.target')}return F()})", "T(()=>{function F(){return (()=>eval('new.target'))()}return typeof F()})",
  "T(()=>{class C{static x=new.target};return C.x})", "T(()=>{class C{static{this.x=new.target}};return C.x})", "T(()=>{class C{x=new.target};return new C().x})", "T(()=>{class C{m(){return new.target}};return new C().m()})",
  "T(()=>{class C{get g(){return new.target}};return new C().g})", "T(()=>{class C{static m(){return new.target}};return C.m()})", "T(()=>{var o={m(){return new.target}};return o.m()})",
  "T(()=>{class C{constructor(){this.t=new.target===C}};return new C().t})", "T(()=>{class C{constructor(){this.t=new.target}};class D extends C{};return new D().t===D})",
  "T(()=>{class C{constructor(){return new.target}};return typeof new C})", "T(()=>{class C{constructor(){this.t=Object.getPrototypeOf(new.target.prototype)===Object.prototype}};return new C().t})",
  "T(()=>{class C{constructor(){this.n=new.target.name}};return new (class Q extends C{})().n})", "T(()=>{class C{constructor(){this.n=new.target.name}};var Q=class extends C{};return new Q().n})",
  "T(()=>{class C{constructor(){this.n=new.target.name}};return new (class extends C{})().n===''})", "T(()=>{function F(){this.n=new.target.name}return new F().n})",
  "T(()=>{function F(){this.n=new.target.name}var G=F.bind();return new G().n})", "T(()=>{function F(){this.t=new.target===F}var G=F.bind();return new G().t})", "T(()=>{function F(){this.t=new.target}var G=F.bind();return new G().t===F})",
  "T(()=>{class C{constructor(){this.t=new.target}};var G=C.bind();return new G().t===C})", "T(()=>{var P=new Proxy(class C{constructor(){this.t=new.target===P}},{});return new P().t})",
  "T(()=>{var P=new Proxy(class C{constructor(){this.t=new.target}},{construct(t,a,nt){return Reflect.construct(t,a,nt)}});return new P().t===P})",
  "T(()=>{var P=new Proxy(class C{constructor(){this.t=new.target}},{construct(t,a,nt){return Reflect.construct(t,a,t)}});return new P().t===P})",
  "T(()=>{var P=new Proxy(function(){},{construct(t,a,nt){return nt}});return new P()===P})", "T(()=>{var P=new Proxy(function(){},{construct(t,a,nt){return 1}});return new P()})",
);

// ---- 6. Symbol.species.
const sp = ["undefined", "null", "Array", "MA", "function(n){return {length:0,x:n}}", "1", "{}", "Object", "function(){throw new RangeError('sp')}", "class{constructor(n){this.n=n}}", "()=>[]", "function(){return null}", "function(){return 1}", "Map", "Proxy"];
const amethods = ["map(x=>x)", "filter(()=>true)", "slice()", "splice(0,1)", "concat()", "flat()", "flatMap(x=>[x])", "slice(0,0)", "map(x=>x).map(x=>x)"];
for (const s of sp) for (const m of amethods) {
  add(`T(()=>{class MA extends Array{static get [Symbol.species](){return ${s}}};var r=new MA(1,2,3).${m};return (r&&r.constructor&&r.constructor.name)+" "+Array.isArray(r)+" "+S(r)})`);
  add(`T(()=>{class MA extends Array{};var a=new MA(1,2,3);a.constructor={[Symbol.species]:${s}};var r=a.${m};return (r&&r.constructor&&r.constructor.name)+" "+Array.isArray(r)+" "+S(r)})`);
  add(`T(()=>{var a=[1,2,3];a.constructor=${s.includes("MA") ? "Array" : s};var r=a.${m};return Object.getPrototypeOf(r)===Array.prototype})`);
}
const tas = ["Uint8Array", "Float64Array", "BigInt64Array"];
for (const ta of tas) for (const s of ["undefined", "null", ta, "Uint8Array", "Int16Array", "function(n){return new " + ta + "(2)}", "function(n){return new " + ta + "(0)}", "1", "{}", "function(){throw new EvalError('s')}"]) {
  for (const m of ["map(x=>x)", "filter(()=>true)", "slice()", "subarray(0)", "slice(0,1)"]) {
    add(`T(()=>{class M extends ${ta}{static get [Symbol.species](){return ${s}}};var r=new M(3).${m};return (r&&r.constructor&&r.constructor.name)+" "+(r instanceof M)+" "+S(Array.from(r||[]))})`);
  }
}
for (const s of ["undefined", "null", "Promise", "MP", "function(ex){ex(()=>{},()=>{});return {then(){}}}", "function(ex){ex(()=>{},()=>{})}", "1", "{}", "Object", "function(){throw new SyntaxError('sp')}", "class{constructor(ex){this.ex=typeof ex}}"]) {
  for (const m of ["then(x=>x)", "then()", "catch(x=>x)", "finally(()=>{})"]) {
    add(`T(()=>{class MP extends Promise{static get [Symbol.species](){return ${s}}};var r=MP.resolve(1).${m};return (r&&r.constructor&&r.constructor.name)+" "+(r instanceof MP)+" "+(r instanceof Promise)})`);
  }
  add(`T(()=>{class MP extends Promise{static get [Symbol.species](){return ${s}}};var r=MP.resolve(1);return (r instanceof MP)+" "+(MP.resolve(r)===r)+" "+(Promise.resolve(r)===r)})`);
}
for (const s of ["undefined", "null", "RegExp", "MR", "function(re,fl){return new RegExp(re,fl)}", "function(re,fl){return {exec(){return null},flags:fl}}", "1", "{}", "function(){throw new EvalError('sp')}"]) {
  for (const call of ["'a,b,c'.split(new MR(','))", "'a,b,c'.split(new MR(','),2)", "'abc'.split(new MR(''))", "'abc'.split(new MR('b'))", "[...'a1b2'.matchAll(new MR('\\\\d','g'))].length", "'a1b2'.replace(new MR('\\\\d','g'),'x')", "'a1b2'.search(new MR('\\\\d'))"]) {
    add(`T(()=>{class MR extends RegExp{static get [Symbol.species](){return ${s}}};return ${call}})`);
  }
}
for (const s of ["undefined", "null", "ArrayBuffer", "MAB", "function(n){return new ArrayBuffer(n)}", "function(n){return new ArrayBuffer(1)}", "function(n){return {}}", "1", "{}", "SharedArrayBuffer"]) {
  add(`T(()=>{class MAB extends ArrayBuffer{static get [Symbol.species](){return ${s}}};var r=new MAB(8).slice(2,6);return (r&&r.constructor&&r.constructor.name)+" "+(r&&r.byteLength)+" "+(r instanceof MAB)})`);
}
add(
  "T(()=>Array[Symbol.species]===Array)", "T(()=>{class M extends Map{};return M[Symbol.species]===M})", "T(()=>{class M extends Set{};return M[Symbol.species]===M})", "T(()=>{class M extends Promise{};return M[Symbol.species]===M})",
  "T(()=>{class M extends RegExp{};return M[Symbol.species]===M})", "T(()=>{class M extends ArrayBuffer{};return M[Symbol.species]===M})", "T(()=>{class M extends Uint8Array{};return M[Symbol.species]===M})",
  "T(()=>D(Array,Symbol.species))", "T(()=>D(Map,Symbol.species))", "T(()=>D(Promise,Symbol.species))", "T(()=>D(RegExp,Symbol.species))", "T(()=>D(ArrayBuffer,Symbol.species))", "T(()=>D(Object.getPrototypeOf(Int8Array),Symbol.species))",
  "T(()=>Object.getOwnPropertyDescriptor(Array,Symbol.species).get.name)", "T(()=>Object.getOwnPropertyDescriptor(Map,Symbol.species).get.call(5))", "T(()=>Object.getOwnPropertyDescriptor(Array,Symbol.species).get.call(undefined))",
  "T(()=>Object.getOwnPropertyDescriptor(Set,Symbol.species).get.length)", "T(()=>{class A extends Array{};return Object.getOwnPropertyNames(A).join()})", "T(()=>{class A extends Array{};return Reflect.ownKeys(A).length})",
  "T(()=>{class A extends Array{};var a=A.from([1,2]);return a instanceof A})", "T(()=>{class A extends Array{};var a=A.of(1,2);return a instanceof A&&a.length})", "T(()=>{class A extends Array{};return A.from({length:2},(_, i)=>i) instanceof A})",
  "T(()=>{class A extends Array{};return new A(3).length+','+new A('3').length+','+new A(1,2).length})", "T(()=>{class A extends Array{};var a=new A;a[2]=1;return a.length})", "T(()=>{class A extends Array{};var a=new A(1,2,3);a.length=1;return S(a)})",
  "T(()=>{class A extends Array{};return Array.isArray(new A)})", "T(()=>{class A extends Array{};return JSON.stringify(new A(1,2))})", "T(()=>{class A extends Array{};return Object.prototype.toString.call(new A)})",
  "T(()=>{class A extends Array{};return [].concat(new A(1,2)).length})", "T(()=>{class A extends Array{get [Symbol.isConcatSpreadable](){return false}};return [].concat(new A(1,2)).length})",
);

// ---- 7. Métodos privados, `#x in obj`, brand checks.
const PRIV = "class K{#f=1;static #sf=2;#m(){return 'm'}static #sm(){return 'sm'}get #g(){return 'g'}set #s(v){}get #gs(){return 'gs'}set #gs(v){}static get #sg(){return 'sg'}static set #ss(v){}static t(o){return EXPR}}";
const pmembers = ["#f", "#sf", "#m", "#sm", "#g", "#s", "#gs", "#sg", "#ss"];
const pops = [(x) => `o.${x}`, (x) => `o.${x}=5`, (x) => `o.${x}()`, (x) => `${x} in o`, (x) => `o.${x}++`, (x) => `o.${x}+=1`, (x) => `o.${x}??=3`, (x) => `[o.${x}]=[1]`, (x) => `({a:o.${x}}={a:1})`, (x) => `o?.${x}`, (x) => `new o.${x}`, (x) => `typeof o.${x}`, (x) => `o.${x}.y`];
const ptargets = ["new K", "K", "{}", "null", "undefined", "1", "new Proxy(new K,{})", "K.prototype", "new (class extends K{})", "Object.create(new K)", "function(){}", "'s'", "Symbol()"];
for (const m of pmembers) for (const op of pops) for (const t of ptargets) {
  add(`T(()=>{${PRIV.replace("EXPR", op(m))};return K.t(${t})})`);
}
add(
  "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}var o={};new C(o);return C.g(o)})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1}var o={};new C(o);new C(o)})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{#m(){}}var o={};new C(o);new C(o)})", "T(()=>{class A{constructor(o){return o}}class C extends A{get #g(){return 1}}var o={};new C(o);new C(o)})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{static #p=1;static g(o){return #p in o}}return C.g(new A({}))})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return #p in o}}return C.g(new C(new A(1)))})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return #p in o}}return C.g(new C(function(){}))})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return #p in o}}return C.g(new C(new Proxy({},{})))})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}return C.g(new C(new Proxy({},{})))})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}var p=new Proxy({},{});new C(p);return C.g(p)})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}var t={};var p=new Proxy(t,{});new C(t);return C.g(p)})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}return C.g(new C(Object.freeze({})))})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}return C.g(new C(Object.preventExtensions({})))})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}return C.g(new C(1))})",
  "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}return C.g(new C(null))})", "T(()=>{class A{constructor(o){return o}}class C extends A{#p=1;static g(o){return o.#p}}return C.g(new C(undefined))})",
  "T(()=>{class C{#p=1;static g(o){return #p in o}}return [1,'s',null,undefined,Symbol(),1n,true].map(v=>{try{return C.g(v)}catch(e){return e.name+': '+e.message}}).join('|')})",
  "T(()=>{class C{#p=1;static g(o){return #p in o}}return C.g(new C)&&C.g(Object.create(new C))})", "T(()=>{class C{#p=1;static g(o){return #p in o}}return C.g(C)})",
  "T(()=>{class C{#p=1;static g(o){return #p in o}}class D extends C{};return C.g(new D)})", "T(()=>{class C{#p=1;static g(o){return #p in o}}return C.g(new Proxy(new C,{}))})",
  "T(()=>{class C{#p=1;static g(){return #p in 1}}return C.g()})", "T(()=>{class C{#p=1;#q=2;static g(o){return (#p in o)+','+(#q in o)}}return C.g(new C)})",
  "T(()=>{class C{static #p=1;static g(o){return #p in o}}class D extends C{};return C.g(C)+','+C.g(D)})", "T(()=>{class C{static #p=1;static g(){return this.#p}}class D extends C{};return D.g()})",
  "T(()=>{class C{static #p=1;static g(){return this.#p}}class D extends C{};return C.g()})", "T(()=>{class C{static #m(){return 1}static g(){return this.#m()}}class D extends C{};return D.g()})",
  "T(()=>{class C{#m(){return 1}g(){return this.#m()}}class D extends C{};return new D().g()})", "T(()=>{class C{#m(){return 1}g(){return this.#m}}var c=new C;return c.g()===c.g()})",
  "T(()=>{class C{#m(){return 1}g(){return this.#m}}return new C().g()===new C().g()})", "T(()=>{class C{#m(){return this}g(){return this.#m.call(5)}}return typeof new C().g()})",
  "T(()=>{class C{#m(){}g(){return this.#m.name}}return new C().g()})", "T(()=>{class C{get #g(){return 1}h(){return Object.getOwnPropertyNames(this).length}}return new C().h()})",
  "T(()=>{class C{#p=1;h(){return Object.getOwnPropertyNames(this).length+Reflect.ownKeys(this).length}}return new C().h()})", "T(()=>{class C{#p=1;h(){return JSON.stringify(this)}}return new C().h()})",
  "T(()=>{class C{#p=1}return Object.keys(Object.getOwnPropertyDescriptors(new C)).length})", "T(()=>{class C{#p=1;h(){return delete this.p}}return new C().h()})",
  "T(()=>{class C{#p=1;static s(o){return o.#p}}var c=new C;var p=new Proxy(c,{get(t,k){return t[k]}});return C.s(p)})", "T(()=>{class C{#p=1;h(){return this.#p}}var c=new C;var h=c.h;return h()})",
  "T(()=>{class C{#p=1;h(){return this.#p}}var c=new C;return c.h.call(Object.create(c))})", "T(()=>{class C{#p=1;h(){return this.#p}}var c=new C;return c.h.call({})})", "T(()=>{class C{#p=1;h(){return ()=>this.#p}}return new C().h()()})",
  "T(()=>{class C{#p=1;h(){return eval('this.#p')}}return new C().h()})", "T(()=>{class C{#p=1;h(){return eval('#p in this')}}return new C().h()})", "T(()=>{class C{#p=1;h(){return new Function('o','return o.#p')}}return new C().h()})",
  "T(()=>{class C{#p=1;static h(o){return (0,eval)('o.#p')}}return C.h(new C)})", "T(()=>{class C{#p=1;h(){return (0,eval)('this.#p')}}return new C().h()})", "T(()=>{class C{#p=1;h(){return eval('1;this.#q')}}return new C().h()})",
  "T(()=>{class C{#p=1;h(){var o={m(){return 7}};return o.m()+this.#p}}return new C().h()})", "T(()=>{var order=[];class C{#a=order.push('a');#b=order.push('b');constructor(){order.push('c')}}new C;return order.join()})",
  "T(()=>{class C{constructor(){this.#p}#p=1}return 'ok'})", "T(()=>{class C{constructor(){return this.#p}#p=1}return new C})", "T(()=>{class C{#a=this.#b;#b=1}return new C})", "T(()=>{class C{#a=this.#m();#m(){return 1}}return S(new C)})",
  "T(()=>{class C{#a=this.#g;get #g(){return 1}}return S(new C)})", "T(()=>{class C{static #a=C.#b;static #b=1}return 0})", "T(()=>{class C{static #a=this.#m();static #m(){return 1}}return 'ok'})",
  "T(()=>{class C{static #a=this.#b;static #b=1}return 'ok'})", "T(()=>{class C{static #b=1;static #a=this.#b;static g(){return C.#a}}return C.g()})",
  "T(()=>{class C{#x=1;static eq(a,b){return a.#x===b.#x}}return C.eq(new C,new C)})", "T(()=>{class C{#x=1;equals(o){return #x in o&&o.#x===this.#x}}return new C().equals(new C)})",
  "T(()=>{class C{#x;static of(o){try{return o.#x}catch(e){return e.constructor===TypeError}}}return C.of({})})", "T(()=>{class C{#x;static of(o){return o.#x}}return C.of(new C)})",
  "T(()=>{class C{#x;static set(o){o.#x=1;return o.#x}}return C.set(new C)})", "T(()=>{class C{#x;static set(o){o.#x=1}}return C.set({})})", "T(()=>{class C{get #x(){return 1}static set(o){o.#x=1}}return C.set(new C)})",
  "T(()=>{class C{set #x(v){}static get(o){return o.#x}}return C.get(new C)})", "T(()=>{class C{#m(){}static set(o){o.#m=1}}return C.set(new C)})", "T(()=>{class C{#m(){}static set(o){[o.#m]=[1]}}return C.set(new C)})",
  "T(()=>{class C{#m(){}static set(o){for(o.#m of [1]);}}return C.set(new C)})", "T(()=>{class C{#x=0;static set(o){for(o.#x of [1,2]);return o.#x}}return C.set(new C)})", "T(()=>{class C{#x=0;static set(o){for(o.#x in {a:1,b:2});return o.#x}}return C.set(new C)})",
  "T(()=>{class C{#x=0;static set(o){({a:o.#x=9}={});return o.#x}}return C.set(new C)})", "T(()=>{class C{#x=0;static set(o){o.#x**=2;o.#x<<=1;o.#x||=7;o.#x&&=8;return o.#x}}return C.set(new C)})",
  "T(()=>{class C{#x=1;static f(o){return o.#x++ + ++o.#x}}return C.f(new C)})", "T(()=>{class C{#x=1n;static f(o){return o.#x++ + ++o.#x}}return C.f(new C)})", "T(()=>{class C{#x=1;static f(o){return `${o.#x}`+(o.#x=2)}}return C.f(new C)})",
  "T(()=>{class C{#x=()=>this;static f(o){return o.#x()===o}}return C.f(new C)})", "T(()=>{class C{#x(){return this}static f(o){return o.#x()===o}}return C.f(new C)})", "T(()=>{class C{static #x(){return this}static f(){return C.#x()===C}}return C.f()})",
  "T(()=>{class C{static #x(){return this}static f(){return (0,C.#x)()}}return C.f()})", "T(()=>{class C{static #x(){return this}static f(){return C.#x`a`===C}}return C.f()})", "T(()=>{class C{#x(){return this}static f(o){return o.#x`a`===o}}return C.f(new C)})",
  "T(()=>{class C{#x(){return this}static f(o){return o?.#x()===o}}return C.f(new C)})", "T(()=>{class C{#x(){return this}static f(o){return o?.#x()}}return C.f(null)})", "T(()=>{class C{#x(){return this}static f(o){return (o?.#x)()===o}}return C.f(new C)})",
  "T(()=>{class C{#x(){return this}static f(o){return (o.#x)()===o}}return C.f(new C)})", "T(()=>{class C{#x(){return this}static f(o){return o.#x.call(1)}}return typeof C.f(new C)})",
  "T(()=>{class C{static #x=1;static f(o){return o.#x}}class D extends C{}return D.f(D)})", "T(()=>{class C{static #x=1;static f(o){return o.#x}}return C.f(Object.create(C))})", "T(()=>{class C{static #x=1;static f(){return C.#x}}var D=class extends C{};return D.f()})",
  "T(()=>{var C=class N{#x=1;static f(o){return o.#x}};return C.f(new C)})", "T(()=>{class C{#x=1;static f(o){return o.#x}}var D=C;C=null;return D.f(new D)})", "T(()=>{var o={};class C{#x=1;static f(o){return o.#x}};return C.f(o)})",
  "T(()=>{function mk(){return class{#x=1;static f(o){return o.#x}}}var A=mk(),B2=mk();return A.f(new B2)})", "T(()=>{function mk(){return class{#x=1;static f(o){return #x in o}}}var A=mk(),B2=mk();return A.f(new B2)+','+A.f(new A)})",
  "T(()=>{function mk(){return class{#x=1;static f(o){return o.#x}}}var A=mk();return A.f(new A)})", "T(()=>{function mk(){return class{#m(){}static f(o){return o.#m===o.#m}}}var A=mk();return A.f(new A)})",
  "T(()=>{function mk(){return class{#m(){}get(){return this.#m}}}var A=mk(),B2=mk();return new A().get()===new B2().get()})",
);

// ---- 8. Accessors estáticos e herança estática.
const sdefs = {
  getter: "static get x(){return 'g:'+this.name}", setter: "static set x(v){L('set '+this.name+' '+v)}", both: "static get x(){return 'g:'+this.name}static set x(v){L('set '+this.name+' '+v)}",
  field: "static x=1", method: "static x(){return 'm:'+this.name}", priv: "static #x=1;static get x(){return this.#x}static set x(v){this.#x=v}", frozenField: "static x=Object.freeze({a:1})",
  sym: "static get [Symbol.iterator](){return function*(){yield 1}}",
};
const sacts = [
  "L(A.x)", "L(B.x)", "B.x=5", "A.x=5", "L('x' in B)", "L(Object.hasOwn(B,'x'))", "L(D(A,'x'))", "L(D(B,'x'))", "L(Reflect.get(A,'x',{name:'rcv'}))", "L(Reflect.set(A,'x',3,{name:'rcv'}))", "L(Reflect.set(B,'x',3,B))",
  "L(Object.keys(A)+'|'+Object.getOwnPropertyNames(A))", "L(delete B.x)", "L(delete A.x)", "'use strict';B.x=6;L(D(B,'x'))", "L(Reflect.defineProperty(B,'x',{value:9}));L(D(B,'x'))", "L(typeof Object.getOwnPropertyDescriptor(A,'x')?.get)",
  "L(Object.getOwnPropertyDescriptor(A,'x')?.get?.name)", "L(Object.getOwnPropertyDescriptor(A,'x')?.get?.hasOwnProperty('prototype'))", "var o=Object.create(B);o.x=7;L(Object.keys(o))", "L(JSON.stringify(Object.entries(Object.getOwnPropertyDescriptors(A))))",
];
const sextras = ["", "static x=2", "static get x(){return 'bg'}", "static set x(v){L('bset')}", "static x(){}", "static y=this.x"];
for (const [dn, d] of Object.entries(sdefs)) for (const act of sacts) for (const ex of sextras) {
  if (ex && !/^static/.test(ex)) continue;
  add(LOGGED(`'use strict';class A{${d}};class B extends A{${ex}};${act}`));
}
add(
  LOGGED("class A{static get x(){return 1}static x=2};L(D(A,'x'))"), LOGGED("class A{static x=2;static get x(){return 1}};L(D(A,'x'))"), LOGGED("class A{static get x(){return 1}static set x(v){}};L(D(A,'x'))"),
  LOGGED("class A{static get x(){return 1}static x(){}};L(D(A,'x'))"), LOGGED("class A{static set x(v){}static get x(){return 1}};L(D(A,'x'))"), LOGGED("class A{get x(){return 1}static get x(){return 2}};L(new A().x+','+A.x)"),
  LOGGED("class A{static get ['x'](){return 1}static get x(){return 2}};L(A.x)"), LOGGED("class A{static get 'x'(){return 1}static get 1(){return 2}static get 1n(){return 3}};L(Reflect.ownKeys(A).map(String))"),
  LOGGED("class A{static get x(){return this}};L(A.x===A);var o={x:Object.getOwnPropertyDescriptor(A,'x').get};L(o.x===o)"), LOGGED("class A{static get x(){return super.y}};Object.setPrototypeOf(A,{y:'sup'});L(A.x)"),
  LOGGED("class A{static set x(v){super.y=v}};Object.setPrototypeOf(A,{set y(v){L('sy '+v+' '+(this===A))}});A.x=1"), LOGGED("class A{static x(){return super.name}};L(A.x())"),
  LOGGED("class A{static get x(){return 1}};class B extends A{static get x(){return super.x+1}};class C extends B{static get x(){return super.x+1}};L(C.x)"),
  LOGGED("class A{static set x(v){L('A '+v)}};class B extends A{static set x(v){super.x=v+1}};B.x=1"), LOGGED("class A{static set x(v){L('A '+v)}};class B extends A{static set x(v){L('B '+v);super.x=v+1}};class C extends B{};C.x=1"),
  LOGGED("class A{static get x(){return 1}};class B extends A{static x=super.x+1};L(B.x)"), LOGGED("class A{static get x(){return 1}};class B extends A{static{this.y=super.x}};L(B.y)"),
  LOGGED("class A{static get x(){throw new RangeError('gx')}};L(A.x)"), LOGGED("class A{static get x(){return 1}};'use strict';A.x=2"), LOGGED("'use strict';class A{static get x(){return 1}};A.x=2"),
  LOGGED("'use strict';class A{static set x(v){}};L(A.x)"), LOGGED("'use strict';class A{static get x(){return 1}};delete A.x"), LOGGED("'use strict';class A{static x=1};delete A.x"),
  LOGGED("'use strict';class A{};A.prototype=1"), LOGGED("'use strict';class A{};delete A.prototype"), LOGGED("'use strict';class A{};A.name='n'"), LOGGED("'use strict';class A{};A.length=2"),
  LOGGED("class A{};L(D(A,'prototype')+'|'+D(A,'name')+'|'+D(A,'length'))"), LOGGED("class A{static m(){}};L(D(A,'m'))"), LOGGED("class A{m(){}};L(D(A.prototype,'m'))"), LOGGED("class A{get g(){return 1}};L(D(A.prototype,'g'))"),
  LOGGED("class A{static m(){}};L(A.m.hasOwnProperty('prototype'))"), LOGGED("class A{m(){}};L(A.prototype.m.hasOwnProperty('prototype'))"), LOGGED("class A{m(){}};new A().m.call(null)"), LOGGED("class A{m(){}};new (A.prototype.m)"),
  LOGGED("class A{static m(){}};new A.m"), LOGGED("class A{get g(){return 1}};new (Object.getOwnPropertyDescriptor(A.prototype,'g').get)"), LOGGED("class A{static *g(){}};new A.g"), LOGGED("class A{static async m(){}};new A.m"),
  LOGGED("class A{};A()"), LOGGED("class A{};A.call({})"), LOGGED("class A{};A.apply()"), LOGGED("class A{};Reflect.apply(A,{},[])"), LOGGED("class A{};L(typeof A+Object.prototype.toString.call(A))"), LOGGED("class A{};L(String(A))"),
  LOGGED("class A{constructor(){}m(){}};L(String(A))"), LOGGED("L(String(class{}))"), LOGGED("L(String(class  X  extends  Object { }))"), LOGGED("class A{m(a,b){return a}};L(String(A.prototype.m))"), LOGGED("class A{static async *g(){}};L(String(A.g))"),
  LOGGED("class A{get g(){return 1}};L(String(Object.getOwnPropertyDescriptor(A.prototype,'g').get))"), LOGGED("class A{[Symbol.iterator](){}};L(A.prototype[Symbol.iterator].name)"), LOGGED("class A{static [Symbol('d')](){}};L(Object.getOwnPropertySymbols(A).length+A[Object.getOwnPropertySymbols(A)[0]].name)"),
  LOGGED("class A{static get x(){}};L(Object.getOwnPropertyDescriptor(A,'x').get.name+'|'+Object.getOwnPropertyDescriptor(A,'x').get.length)"), LOGGED("class A{static set x(v){}};L(Object.getOwnPropertyDescriptor(A,'x').set.name+'|'+Object.getOwnPropertyDescriptor(A,'x').set.length)"),
  LOGGED("class A{static #p(){}static g(){return this.#p.name}};L(A.g())"), LOGGED("class A{static get #p(){return 1}static g(){return Object.keys(A).length}};L(A.g())"),
);

// ---- 9. Herança de builtins.
const builtins = {
  Array: ["", "3", "1,2", "'a'"], Map: ["", "[[1,2]]", "1"], Set: ["", "[1,1,2]", "'ab'"], WeakMap: ["", "[[{},1]]"], WeakSet: [""], Error: ["", "'m'", "'m',{cause:1}"], TypeError: ["'t'"], RangeError: [""],
  AggregateError: ["[1],'m'", "[]"], Promise: ["", "()=>{}", "r=>r(1)", "1"], RegExp: ["", "'a','g'", "/x/y"], Uint8Array: ["", "2", "[1,2]", "new ArrayBuffer(4)"], Float64Array: ["2"], BigInt64Array: ["1"],
  Date: ["0", "NaN"], ArrayBuffer: ["4"], DataView: ["new ArrayBuffer(2)"], String: ["'ab'", ""], Number: ["5", ""], Boolean: ["0", ""], Object: ["", "1"], Function: ["'return 1'"], Symbol: [""], BigInt: ["1"],
  Proxy: ["{},{}"], WeakRef: ["{}"], Boolean2: [], SharedArrayBuffer: ["2"],
};
const bops = [
  (B, a) => `class X extends ${B}{};var o=new X(${a});return [Object.prototype.toString.call(o),o instanceof X,o instanceof ${B},Object.getPrototypeOf(o)===X.prototype,X.name,Object.getPrototypeOf(X)===${B}].join()`,
  (B, a) => `class X extends ${B}{};return X(${a})`,
  (B, a) => `class X extends ${B}{};var o=Reflect.construct(${B},[${a}],X);return Object.getPrototypeOf(o)===X.prototype`,
  (B, a) => `class X extends ${B}{constructor(...a){super(...a);this.z=1}};var o=new X(${a});return Object.getOwnPropertyNames(o).join()`,
  (B, a) => `class X extends ${B}{constructor(){super(${a});this.z=1}};var o=new X;return S(o.z)+Object.prototype.toString.call(o)`,
  (B, a) => `class X extends ${B}{constructor(){this.z=1;super(${a})}};return new X`,
  (B, a) => `class X extends ${B}{constructor(){}};return new X`,
  (B, a) => `function F(){};F.prototype=1;var o=Reflect.construct(${B},[${a}],F);var g=Object.getPrototypeOf(o);return g===${B}.prototype||g===Object.prototype||typeof g`,
  (B, a) => `var F=function(){};F.prototype=null;var o=Reflect.construct(${B},[${a}],F);var g=Object.getPrototypeOf(o);return g===${B}.prototype||g===Object.prototype||String(g)`,
  (B, a) => `class X extends ${B}{static s(){return super.name}};return X.s()+X.length`,
  (B, a) => `class X extends ${B}{};return Reflect.ownKeys(new X(${a})).map(String).join()`,
  (B, a) => `class X extends ${B}{};var o=new X(${a});return Object.prototype.hasOwnProperty.call(X.prototype,'constructor')+","+(o.constructor===X)`,
  (B, a) => `class X extends ${B}{get [Symbol.toStringTag](){return 'Q'}};return Object.prototype.toString.call(new X(${a}))`,
  (B, a) => `class X extends ${B}{};class Y extends X{};var o=new Y(${a});return [o instanceof X,o instanceof Y,o instanceof ${B},Object.getPrototypeOf(Object.getPrototypeOf(o))===X.prototype].join()`,
  (B, a) => `var o=new (class X extends ${B}{})(${a});return String(Object.getPrototypeOf(o).constructor.name)+Object.getPrototypeOf(Object.getPrototypeOf(o))===${B}.prototype`,
];
for (const [B, argl] of Object.entries(builtins)) {
  if (B === "Boolean2") continue;
  for (const a of argl) for (const op of bops) add(`T(()=>{${op(B, a)}})`);
}
add(
  "T(()=>{class E extends Error{};var e=new E('m');return [e.message,e.name,String(e),Object.prototype.toString.call(e),Object.keys(e).length,e instanceof Error,e.constructor.name].join('|')})",
  "T(()=>{class E extends Error{constructor(m){super(m);this.name='E'}};var e=new E('m');return String(e)+Object.keys(e)})", "T(()=>{class E extends Error{get name(){return 'G'}};return String(new E('m'))})",
  "T(()=>{class E extends Error{};E.prototype.name='Pn';return String(new E('m'))})", "T(()=>{class E extends Error{};return D(new E('m'),'message')})", "T(()=>{class E extends Error{};return D(new E,'message')})",
  "T(()=>{class E extends Error{};return D(new E('a',{cause:2}),'cause')})", "T(()=>{class E extends Error{};return 'cause' in new E('a',{})})", "T(()=>{class E extends Error{};return Object.hasOwn(new E('a',{cause:undefined}),'cause')})",
  "T(()=>{class E extends Error{constructor(){super('x',{cause:{c:1}})}};return S(new E().cause)})", "T(()=>{class E extends TypeError{};return [new E('m') instanceof TypeError,new E('m') instanceof RangeError,String(new E('m'))].join()})",
  "T(()=>{class E extends AggregateError{};var e=new E([1,2],'m');return S(e.errors)+e.message+e.name})", "T(()=>{class E extends Error{};return Object.getPrototypeOf(E)===Error&&Object.getPrototypeOf(E.prototype)===Error.prototype})",
  "T(()=>{class E extends Error{};return typeof new E().stack})", "T(()=>{class E extends Error{};return Error.captureStackTrace.length})", "T(()=>{class E extends Error{};return Object.getOwnPropertyNames(new E('x')).sort().join()})",
  "T(()=>{class M extends Map{set(k,v){return super.set(k,v*2)}};var m=new M([[1,1]]);m.set(2,2);return S([...m])})", "T(()=>{class M extends Map{set(k,v){L=1;return super.set(k,v)}};var L=0;new M([[1,1]]);return L})",
  "T(()=>{var n=0;class M extends Map{set(k,v){n++;return super.set(k,v)}};new M([[1,1],[2,2]]);return n})", "T(()=>{var n=0;class S2 extends Set{add(v){n++;return super.add(v)}};new S2([1,2,2]);return n})",
  "T(()=>{class M extends Map{get(k){return 'ov'}};return new M([[1,2]]).get(1)+Map.prototype.get.call(new M([[1,2]]),1)})", "T(()=>{class M extends Map{};var m=new M;return Map.prototype.has.call(m,1)})",
  "T(()=>{class M extends Map{};return Map.prototype.has.call({},1)})", "T(()=>{class M extends Map{};return M.prototype.has.call(new Set,1)})", "T(()=>{class M extends Map{};return M.groupBy([1,2],x=>x%2) instanceof Map})", "T(()=>{class M extends Map{};return M.groupBy([1,2],x=>x%2) instanceof M})",
  "T(()=>{class P extends Promise{};return [P.resolve(1) instanceof P,P.reject(1).catch(()=>{}) instanceof P,P.all([]) instanceof P,P.race([]) instanceof P,P.allSettled([]) instanceof P,P.any([1]) instanceof P].join()})",
  "T(()=>{class P extends Promise{};return P.withResolvers().promise instanceof P})", "T(()=>{class P extends Promise{constructor(ex){super(ex);this.tag=1}};return P.resolve(1).tag})", "T(()=>{class P extends Promise{constructor(ex){super(ex);this.tag=1}};return P.resolve(1).then(x=>x).tag})",
  "T(()=>{class P extends Promise{constructor(){super(()=>{})}};return P.resolve(1)})", "T(()=>{class P extends Promise{constructor(ex){ex=undefined;super(ex)}};return 1})", "T(()=>{class P extends Promise{constructor(ex){super(ex)}};return new P(1)})",
  "T(()=>{class P extends Promise{constructor(ex){super((a,b)=>ex(a,b));}};return P.resolve(1) instanceof P})", "T(()=>{class P extends Promise{};return Promise.resolve.call(P,1) instanceof P})", "T(()=>{class P extends Promise{};return Promise.resolve.call({},1)})",
  "T(()=>{class P extends Promise{};return Promise.resolve.call(function(){},1)})", "T(()=>{class P extends Promise{};return Promise.all.call(1,[])})", "T(()=>{class P extends Promise{};return Promise.all.call(function(ex){ex(()=>{},()=>{})},[])})",
  "T(()=>{class R extends RegExp{};var r=new R('a','g');return [r.global,r.source,r.flags,String(r),r.lastIndex,r instanceof RegExp].join()})", "T(()=>{class R extends RegExp{exec(s){return null}};return new R('a').test('a')})",
  "T(()=>{class R extends RegExp{exec(s){return {0:'x',index:0,length:1}}};return new R('a').test('b')+','+'b'.replace(new R('a'),'z')})", "T(()=>{class R extends RegExp{};return RegExp.prototype.exec.call(new R('a'),'a').index})",
  "T(()=>{class R extends RegExp{};return RegExp.prototype.exec.call({},'a')})", "T(()=>{class R extends RegExp{get flags(){return 'i'}};return String(new R('a'))})", "T(()=>{class R extends RegExp{};return new R(/a/g).flags+new R(/a/g,'i').flags})",
  "T(()=>{class R extends RegExp{};return R(/a/g) instanceof R})", "T(()=>{class R extends RegExp{};var r=/a/;return new R(r) instanceof R})", "T(()=>{var r=/a/;return RegExp(r)===r})", "T(()=>{class R extends RegExp{};var r=new R('a');return RegExp(r)===r})",
  "T(()=>{class U extends Uint8Array{};var u=new U([1,2,3]);return [u.length,u[1],Object.prototype.toString.call(u),u.map(x=>x*2) instanceof U,u.slice(1) instanceof U,u.subarray(1) instanceof U,u.filter(x=>x>1).length].join()})",
  "T(()=>{class U extends Uint8Array{};return [U.BYTES_PER_ELEMENT,U.from([1]) instanceof U,U.of(1) instanceof U,U.name,U.length].join()})", "T(()=>{class U extends Uint8Array{constructor(){super(2)}};return U.from([1,2,3]).length})",
  "T(()=>{class U extends Uint8Array{constructor(){super(2)}};return new U().map(x=>1).length})", "T(()=>{class U extends Uint8Array{constructor(n){super(typeof n==='number'?n+1:n)}};return new U(2).length+new U(2).slice(0).length})",
  "T(()=>{class U extends Float32Array{};var u=new U(2);u[0]=1.5;return S([...u])+u.constructor.name})", "T(()=>{class U extends Uint8Array{};return Object.getPrototypeOf(U)===Uint8Array&&Object.getPrototypeOf(Uint8Array)===Object.getPrototypeOf(Int8Array)})",
  "T(()=>{class A extends Array{constructor(...a){super(...a);this.tag=1}};var a=new A(1,2);return [a.tag,a.map(x=>x).tag,a.length,a.slice(1).tag].join()})", "T(()=>{class A extends Array{constructor(){super(3)}};return new A().length+','+new A().map(x=>x).length})",
  "T(()=>{class A extends Array{constructor(n){super(n)}};return new A(3).map(x=>x).length+','+new A(3).filter(x=>1).length})", "T(()=>{class A extends Array{constructor(n){super();this.n=n}};return new A(5).map(x=>x).n})",
  "T(()=>{class A extends Array{};var a=new A;a.push(1,2);return [a.length,a instanceof A,JSON.stringify(a),a.concat([3]) instanceof A,A.from('ab') instanceof A,[].concat(a) instanceof A].join()})",
  "T(()=>{class A extends Array{};var a=new A(1,2,3);a.length=0;return a.length+','+Object.keys(a).length})", "T(()=>{class A extends Array{};return Array.prototype.map.call(new A(1,2),x=>x) instanceof A})",
  "T(()=>{class D2 extends Date{};var d=new D2(0);return [d.getTime(),d instanceof Date,Object.prototype.toString.call(d),D2.UTC(1970,0,1),D2.now()>0].join()})", "T(()=>{class D2 extends Date{};return Date.prototype.getTime.call(new D2(5))})",
  "T(()=>{class D2 extends Date{};return Date.prototype.getTime.call({})})", "T(()=>{class D2 extends Date{[Symbol.toPrimitive](h){return h}};return `${new D2(0)}`+(new D2(0)+1)+(new D2(0)*1)})", "T(()=>{class D2 extends Date{};return typeof D2(0)})",
  "T(()=>{class S2 extends String{};var s=new S2('ab');return [s.length,s[1],typeof s,s instanceof String,Object.keys(s).join(),String(s),Object.prototype.toString.call(s)].join()})", "T(()=>{class N extends Number{};var n=new N(5);return [n+1,typeof n,n.toFixed(1),Object.keys(n).length].join()})",
  "T(()=>{class Bo extends Boolean{};var b=new Bo(0);return [b?1:2,typeof b,String(b),b.valueOf()].join()})", "T(()=>{class O extends Object{};var o=new O(1);return [typeof o,o instanceof O,o instanceof Number].join()})",
  "T(()=>{class O extends Object{constructor(){super(1)}};var o=new O;return [typeof o,o instanceof O,o instanceof Number].join()})", "T(()=>{class O extends Object{constructor(){super(1)}};return Reflect.construct(O,[],Array) instanceof Array})",
  "T(()=>{class F extends Function{};var f=new F('return 7');return [f(),f instanceof F,typeof f,f.name,f.length,Object.getPrototypeOf(f)===F.prototype].join()})", "T(()=>{class F extends Function{};var f=new F('a','b','return a+b');return f(1,2)+f.length+String(f)})",
  "T(()=>{class F extends Function{constructor(){super('return this.x');}};var f=new F();return f.call({x:3})})", "T(()=>{class F extends Function{};var f=new F('return new.target');return typeof f()})", "T(()=>{class F extends Function{};return F('return 1') instanceof F})",
  "T(()=>{class A extends Object{};return A.prototype.constructor===A&&Object.getPrototypeOf(A)===Object})", "T(()=>{class A extends Function.prototype{};return 0})", "T(()=>{class A extends Symbol{};return 0})", "T(()=>{class A extends Symbol{};return new A})",
  "T(()=>{class A extends BigInt{};return new A(1)})", "T(()=>{class A extends BigInt{};return A(1)})", "T(()=>{class A extends Proxy{};return 0})", "T(()=>{class A extends Proxy{};return new A({},{})})", "T(()=>{class A extends Math{};return 0})",
  "T(()=>{class A extends JSON{};return 0})", "T(()=>{class A extends Reflect{};return 0})", "T(()=>{class A extends Atomics{};return 0})", "T(()=>{class A extends WeakRef{};var w=new A({});return typeof w.deref()+Object.prototype.toString.call(w)})",
  "T(()=>{class A extends WeakMap{};var k={};var w=new A([[k,1]]);return w.get(k)+Object.prototype.toString.call(w)})", "T(()=>{class A extends Intl.NumberFormat{};return new A('en').format(1234.5)})", "T(()=>{class A extends Intl.DateTimeFormat{};return typeof new A('en').format})",
  "T(()=>{class A extends ArrayBuffer{};var a=new A(4);return [a.byteLength,a instanceof ArrayBuffer,a.slice(1) instanceof A,Object.prototype.toString.call(a)].join()})", "T(()=>{class A extends DataView{};var a=new A(new ArrayBuffer(4));return [a.byteLength,a.getInt8(0),Object.prototype.toString.call(a)].join()})",
  "T(()=>{class A extends Array{static get [Symbol.species](){return Array}};var a=new A(1,2);return [a.map(x=>x) instanceof A,a.map(x=>x) instanceof Array,a.slice() instanceof A].join()})",
  "T(()=>{class A extends Array{};var a=new A(1,2,3);return [Array.isArray(a),a.toString(),a.toSorted() instanceof A,a.toReversed() instanceof A,a.with(0,1) instanceof A,a.toSpliced(0,1) instanceof A,a.at(-1),a.findLast(x=>1)].join()})",
);

// ---- 10. Sintaxe e mensagens de erro de classes (via eval para capturar SyntaxError).
const syn = [
  "class C{#a;#a}", "class C{#constructor}", "class C{constructor=1}", "class C{'constructor'=1}", "class C{static prototype=1}", "class C{static prototype(){}}", "class C{static ['prototype']=1}", "class C{static 'prototype'(){}}",
  "class C{constructor(){}constructor(){}}", "class C{get constructor(){}}", "class C{set constructor(v){}}", "class C{*constructor(){}}", "class C{async constructor(){}}", "class C{static constructor(){}}", "class C{static constructor=1}",
  "class C{x=arguments}", "class C{static x=arguments}", "class C{static{arguments}}", "class C{x=()=>arguments}", "class C{x=function(){return arguments}}", "class C{static{await 1}}", "class C{static{return}}", "class C{static{var await}}",
  "class C{static{yield}}", "async function f(){class C{static{await}}}", "async function f(){class C{x=await 1}}", "function*g(){class C{x=yield 1}}", "function*g(){class C{[yield 1](){}}}", "async function f(){class C{[await 1](){}}}",
  "class C{m(){super()}}", "class C{constructor(){super()}}", "class C extends Object{m(){super()}}", "class C extends Object{constructor(){function f(){super()}}}", "class C extends Object{constructor(){super.x;super()}}", "function f(){super.x}", "function f(){super()}",
  "var o={m(){super()}}", "var o={m:function(){super.x}}", "var o={m(){super.x}}", "var o={get m(){return super.x}}", "class C{x=super.y}", "class C{x=super()}", "class C extends Object{x=super()}", "class C{static x=super.y}", "class C{static{super.y}}", "class C{static{super()}}",
  "new.target", "function f(){new.target}", "()=>new.target", "class C{x=new.target}", "class C{static{new.target}}", "class C extends 1{}", "class C extends {}", "class C extends a,b{}", "class C extends (a,b){}", "class C extends a=>1{}", "class C extends a?b:c{}",
  "class C{#x;m(){delete this.#x}}", "class C{#x;m(){delete this?.#x}}", "class C{#x;m(){delete (this.#x)}}", "class C{#x;m(){delete this.a.#x}}", "class C{m(){this.#y}}", "class C{m(){#y in this}}", "class C{#x;m(){#x}}", "class C{#x;m(){#x+1}}", "class C{#x;m(){1+#x in this}}",
  "class C{#x;m(){#x in #x in this}}", "class C{#x;m(){(#x) in this}}", "class C{#x;m(){#x in this in this}}", "class C{#x;m(){#x = 1}}", "class C{#x;m(){this?.#x=1}}", "class C{#x;m(){this?.#x++}}", "class C{#x;m(){[this?.#x]=[1]}}", "class C{#x;m(){({a:this?.#x}={})}}",
  "class C{#x;m(){for(this?.#x of []);}}", "class C{#x;m(){this.# x}}", "class C{# x}", "class C{#\\u0078;m(){this.#x}}", "class C{#x;m(){this.#\\u0078}}", "class C{#1}", "class C{#@}", "class C{#\\u{1d7d8}}", "class C{#a\\u{1d7d8};m(){return this.#a\\u{1d7d8}}}",
  "class C{static #x;static m(){return C.#x}}", "class C{get #x(){}set #x(v){}}", "class C{get #x(){}get #x(){}}", "class C{static get #x(){}set #x(v){}}", "class C{get #x(){}static set #x(v){}}", "class C{#x;#x(){}}", "class C{#x(){}#x(){}}", "class C{static #x;get #x(){}}",
  "class C{x;y}", "class C{x\ny}", "class C{x=1 y=2}", "class C{x y}", "class C{get\nx(){}}", "class C{static\nx}", "class C{static\nx(){}}", "class C{get;set;static;async}", "class C{get=1;set=2;static=3;async=4}", "class C{async\nx(){}}", "class C{*\nx(){}}", "class C{static async\nx(){}}",
  "class C{x=1,y=2}", "class C{;;;}", "class C{static;static}", "class C{static static(){}}", "class C{static static static}", "class C{get get(){}set set(v){}}", "class C{'use strict'(){}}", "class C{async *[Symbol.asyncIterator](){}}", "class C{static async *#g(){}}", "class C{constructor(){}'constructor'(){}}",
  "class C{['constructor'](){}}", "class C{['constructor']=1}", "class C{static ['constructor'](){}}", "class let{}", "class yield{}", "class await{}", "class static{}", "class implements{}", "class C{m(){var let}}", "class C{m(){with(1);}}", "class C{m(){arguments=1}}", "class C{m(eval){}}", "class C{m(a,a){}}", "class C{m(a=1){'use strict'}}",
  "(class C{}).x=1", "(class{}) = 1", "class C{};class C{}", "let C;class C{}", "class C{};var C", "{class C{};function C(){}}", "class C{[a,b](){}}", "class C{[a=1]=1}", "class C{[a,b]=1}", "class C{static async x}", "class C{async x=1}", "class C{get x=1}", "class C{*x=1}",
  "class C extends B{constructor(){super()}constructor(){super()}}", "class C extends B{constructor(){(super)()}}", "class C extends B{constructor(){super?.()}}", "class C extends B{constructor(){new super()}}", "class C extends B{constructor(){new super.x}}", "class C extends B{constructor(){super`a`}}", "class C extends B{m(){super`a`}}", "class C extends B{m(){super[]}}", "class C extends B{m(){super.#x}}",
  "class C extends B{constructor(){super(...[])}}", "class C extends B{constructor(){super(1,)}}", "class C extends B{constructor(){super(,)}}", "class C extends B{constructor(){super(...)}}", "class C extends B{constructor(){x=super()}}", "class C extends B{constructor(){super()=1}}", "class C extends B{constructor(){super().x}}", "class C extends B{constructor(){delete super.x}}", "class C extends B{constructor(){delete super[0]}}",
  "class C extends B{m(){return class{[super.x](){}}}}", "class C extends B{m(){return class extends super.x{}}}", "class C extends B{m(){return class{x=super.y}}}", "class C extends B{m(){return class{constructor(){super()}}}}", "class C extends B{static m(){return class extends B{constructor(){super()}}}}",
  "class C{constructor(){return}}", "class C extends B{constructor(){return}}", "class C{static{}static{}}", "class C{static{;}}", "class C{static{let x;let x}}", "class C{static{var x;let x}}", "class C{static{function f(){}function f(){}}}", "class C{static{label:{break label}}}", "class C{static{break}}", "class C{static{continue}}", "class C{static{this}}",
  "class C{static{x:x:1}}", "class C{static{async function f(){await 1}}}", "class C{static{async()=>await 1}}", "class C{static{(await)=>1}}", "class C{static{function await(){}}}", "class C{static{class await{}}}", "class C{static{({await})}}", "class C{static{({await:1})}}", "class C{static{await:1}}",
];
for (const s of syn) {
  add(`T(()=>{eval(${J(s)});return 'ok'})`, `T(()=>{'use strict';eval(${J(s)});return 'ok'})`, `T(()=>{new Function(${J(s)});return 'ok'})`);
}

// ---- Execução.
const baseSources = [];
for (const file of ["class_edge_bun.tsv", "subclass_edge_bun.tsv", "ctor_this_bun.tsv", "class_builtin_bun.tsv", "class_bun.tsv", "proxy_class_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
      if (!line) continue;
      try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
    }
  } catch (e) {}
}
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let dropped = 0;
let dup = 0;
const jobs = [];
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  jobs.push('"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`);
}
const DASH = /[\u2013\u2014]/;
const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 20000);
    child.stdout.on("data", d => { out += d; });
    child.stderr.on("data", d => { err += d; });
    child.on("close", code => { clearTimeout(timer); resolve(code === 0 ? (decodeResult(out) === null ? { err: "filho sem resultado" } : { out: decodeResult(out) }) : { err: err || "filho falhou" }); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}
async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.err !== undefined) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(jobs[i].slice(PRELUDE.length + 20, PRELUDE.length + 200)) + " " + r.err.slice(0, 120) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || DASH.test(r.out) || DASH.test(jobs[i])) { dropped++; continue; }
    kept++;
    lines.push(JSON.stringify(jobs[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("class_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
