// Gera tests/golden/object_literal_bun.tsv: literais de objeto em grade, medido no bun 1.4.2.
// Cobre ordem de chaves (inteiras, strings, símbolos, computed, numéricas como "1"/"01"/"-0"/"1e3"/"4294967295"),
// chaves repetidas (data/getter/setter/método/spread misturados), `__proto__` em todas as formas (com SyntaxError
// exato nas duplicatas), spread com getters e proxies (ordem de efeitos), métodos com super e home object, nomes de
// funções em chaves computadas e símbolos, getters/setters com nomes numéricos e computed, JSON versus literal.
// Programas cuja fonte já aparece (igual) em algum tests/golden/*.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-object-literal-golden.js > tests/golden/object_literal_bun.tsv
const fs = require("fs");
const { knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

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
  'function T(f){try{return f()}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function E(s){return T(()=>S((0,eval)(s)))}\n' +
  'function DD(o){return Reflect.ownKeys(o).map(k=>S(k)+" "+D(o,k)).join(";")}\n' +
  'function P(o){var p=Object.getPrototypeOf(o);return S(Reflect.ownKeys(o))+" proto="+(p===Object.prototype?"OP":p===null?"null":p===Array.prototype?"AP":p===Function.prototype?"FP":"other")+" z="+o.z}\n' +
  'function N(o){return Reflect.ownKeys(o).map(k=>{var d=Object.getOwnPropertyDescriptor(o,k),f=d.get||d.set||d.value;return typeof f==="function"?D(f,"name")+" "+D(f,"length")+" "+Reflect.ownKeys(f).map(String):"-"}).join(";")}\n' +
  'function J(s){return T(()=>S(JSON.parse(s)))}\n' +
  'var sym=Symbol("s"),L=[];\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = JSON.stringify;

// ---- 1. Ordem de chaves: formas de escrever uma chave, sozinhas, em pares e em trios.
const pool = [
  "1", '"1"', "[1]", "0x1", "1.0", "1n", '"01"', "01", '"-0"', "[-0]", '"1e3"', "1e3", "4294967295", '"4294967295"', "4294967294",
  "4294967296", ".5", '"1.5"', "1e21", "1e-7", "0b11", "0o7", "1_0", "a", '"a"', "['a']", "[sym]", "[Symbol.iterator]", '""', "[1+1]",
  "[null]", "[undefined]", "[true]", "[[1,2]]", "[NaN]", "0", "010", "08", "[Infinity]", "[-1]", '"-1"', '"0x1"', '" 1"', '"1 "', "[`${1}`]",
  "[{}]", "[1n]", '"2147483648"', "2147483647", "9007199254740991", "9007199254740993", '"9007199254740991"', "[4294967295]", "[4294967296]",
];
for (const k of pool) add(`T(()=>S({${k}:1}))`, `T(()=>Reflect.ownKeys({${k}:1}).map(String).join())`);
const pairPool = pool.slice(0, 30);
pairPool.forEach((a, i) => pairPool.forEach((b, j) => add(`T(()=>S({${a}:${i},${b}:${j}}))`)));
const triPool = ["2", '"01"', '"-0"', "1e3", "4294967295", '"b"', "[sym]", "0"];
for (const a of triPool) for (const b of triPool) for (const c of triPool) add(`T(()=>S({${a}:1,${b}:2,${c}:3}))`);
// Ordem com chaves inteiras depois de ser criado o objeto e mutado.
for (const k of ["2", "'2'", "'b'", "sym", "'01'", "4294967295", "4294967294", "'-0'"])
  add(`T(()=>{var o={z:1,5:1,[sym]:1,a:1};o[${k}]=9;return S(o)})`, `T(()=>{var o={z:1,5:1,[sym]:1,a:1};delete o.z;o[${k}]=9;return S(o)})`, `T(()=>{var o={[${k}]:0,z:1};delete o[${k}];o[${k}]=1;return S(o)})`);

// ---- 2. Chaves repetidas: tipos de definição em pares e trios.
const keys = [
  { src: "k", ex: '"k"' }, { src: "1", ex: "1" }, { src: "[sym]", ex: "sym" }, { src: '"x y"', ex: '"x y"' },
];
const kinds = [
  { n: "data", f: (k, n) => `${k.src}:${n}` },
  { n: "get", f: (k, n) => `get ${k.src}(){return "g${n}"}` },
  { n: "set", f: (k, n) => `set ${k.src}(v){}` },
  { n: "method", f: (k, n) => `${k.src}(){return ${n}}` },
  { n: "getset", f: (k, n) => `get ${k.src}(){return "g${n}"},set ${k.src}(v){}` },
  { n: "spread", f: (k, n) => `...{[${k.ex}]:${n}}` },
  { n: "computed", f: (k, n) => `[${k.ex}]:${n}` },
  { n: "async", f: (k, n) => `async ${k.src}(){}` },
  { n: "gen", f: (k, n) => `*${k.src}(){}` },
  { n: "arrow", f: (k, n) => `${k.src}:()=>${n}` },
];
for (const k of keys) for (const a of kinds) for (const b of kinds)
  add(`T(()=>{var o={${a.f(k, 1)},${b.f(k, 2)}};return DD(o)})`);
const triKinds = [kinds[0], kinds[1], kinds[2], kinds[3], kinds[5], kinds[4]];
for (const k of keys.slice(0, 1)) for (const a of triKinds) for (const b of triKinds) for (const c of triKinds)
  add(`T(()=>{var o={${a.f(k, 1)},${b.f(k, 2)},${c.f(k, 3)}};return DD(o)})`);
for (const b of kinds) add(`T(()=>{var k=7;var o={k,${b.f(keys[0], 2)}};return DD(o)})`, `T(()=>{var k=7;var o={${b.f(keys[0], 2)},k};return DD(o)})`);
// Setter e getter observados depois da definição duplicada.
for (const a of triKinds) for (const b of triKinds)
  add(`T(()=>{var o={${a.f(keys[0], 1)},${b.f(keys[0], 2)}};var r=[];o.k=5;r.push(S(o.k));r.push(Object.hasOwn(o,"k"));return r.join()+DD(o)})`);

// ---- 3. __proto__ em todas as formas.
const protoValues = ["{z:1}", "null", "1", '"s"', "undefined", "true", "[]", "function(){}", "Symbol()", "Object.create(null)", "new Proxy({z:2},{})", "1n", "{z:1,__proto__:null}", "Object.prototype", "class{}", "-0"];
const protoForms = [
  (v) => `__proto__:${v}`, (v) => `"__proto__":${v}`, (v) => `['__proto__']:${v}`, (v) => `["__pro"+"to__"]:${v}`, (v) => `'__proto__':${v}`,
  (v) => `__proto__(){return ${v}}`, (v) => `get __proto__(){return ${v}}`, (v) => `...{__proto__:${v}}`, (v) => `__proto__:${v},z:5`, (v) => `z:5,__proto__:${v}`,
  (v) => `\\u005f_proto__:${v}`, (v) => `__proto__:${v}`.replace("__proto__", "__proto__ ") , (v) => `__proto__:__proto__:${v}`.replace(":__proto__", ""),
  (v) => `async __proto__(){}`.replace("async", `[1]:${v},async`), (v) => `__proto__:${v},__proto__(){}`,
];
for (const f of protoForms) for (const v of protoValues) add(`E(${q(`P({${f(v)}})`)})`);
for (const v of protoValues) {
  add(`T(()=>{var __proto__=${v};return P({__proto__})})`, `T(()=>{var __proto__=${v};return P({__proto__,z:3})})`, `T(()=>{var __proto__=${v};return P({__proto__:{z:9},__proto__})})`);
  add(`T(()=>{var __proto__=${v};return P({__proto__,__proto__:{z:9}})})`, `T(()=>{var __proto__=${v};return P({__proto__,__proto__})})`);
  add(`T(()=>{var o={__proto__:${v}};return Object.hasOwn(o,"__proto__")+" "+(o.__proto__===Object.getPrototypeOf(o))})`);
  add(`T(()=>{var o={["__proto__"]:${v}};return Object.hasOwn(o,"__proto__")+" "+D(o,"__proto__")})`);
  add(`T(()=>{var o={"__proto__":${v}};o.__proto__=null;return P(o)})`, `T(()=>{var o={__proto__:${v}};return Object.prototype.hasOwnProperty.call(o,"z")})`);
  add(`T(()=>{var o=JSON.parse(${q(`{"__proto__":${/^[\[\{"0-9tfn-]/.test(v) && !/^(null|true|-0)$/.test(v) && !/function|class|Object|new|undefined|Symbol|1n|^-/.test(v) ? v.replace(/(\w+):/g, '"$1":') : "1"}}`)});return P(o)+" "+D(o,"__proto__")})`);
}
// Duplicatas: SyntaxError exato versus válido.
const dupForms = [
  (v) => `__proto__:${v}`, (v) => `"__proto__":${v}`, (v) => `['__proto__']:${v}`, (v) => `__proto__`, (v) => `__proto__(){}`,
  (v) => `get __proto__(){return ${v}}`, (v) => `'__proto__':${v}`, (v) => `__proto__:${v}`.replace("__proto__", "\\u005f_proto__"), (v) => `...{__proto__:${v}}`,
  (v) => `set __proto__(x){}`, (v) => `async __proto__(){}`, (v) => `*__proto__(){}`,
];
const dupVals = [["{z:1}", "null"], ["1", "2"], ["{}", "{z:2}"]];
for (const fa of dupForms) for (const fb of dupForms) for (const [x, y] of dupVals) {
  const lit = `{${fa(x)},${fb(y)}}`;
  add(`E(${q(`var __proto__=3;P(${lit})`)})`);
}
const dupContexts = [
  (l) => `(${l})`, (l) => `"use strict";(${l})`, (l) => `(${l}={})`, (l) => `(${l})=>1`, (l) => `var f=function(${l}){};f`, (l) => `[${l}]`,
  (l) => `({a:${l}})`, (l) => `(function(){return ${l}})()`, (l) => `(${l}=>1)`, (l) => `new (class{static x=${l}})`, (l) => `for(${l} of []);1`,
  (l) => `({...${l}})`, (l) => `async(${l})=>1`, (l) => `(${l})=2`, (l) => `({a=1,__proto__:1,__proto__:2})`, (l) => `({__proto__:1,__proto__:2,a=1})`,
];
for (const ctx of dupContexts) for (const lit of ["{__proto__:1,__proto__:2}", "{__proto__:a,__proto__:b}", '{"__proto__":a,__proto__:b}', "{__proto__,__proto__}", "{__proto__:a,__proto__}", "{__proto__:{}, ['__proto__']:1}", "{__proto__:1,__proto__(){}}"])
  add(`E(${q(`var a,b,__proto__;${ctx(lit)}`)})`, `E(${q(ctx(lit))})`);
add(
  `J('{"__proto__":1,"__proto__":2}')`, `J('{"__proto__":{"z":1}}')+E('P(JSON.parse(\\'{"__proto__":{"z":1}}\\'))')`, `E('P(JSON.parse(\\'{"__proto__":null}\\'))')`,
  `T(()=>{var o=JSON.parse('{"__proto__":{"z":1}}');return P(o)+Object.getPrototypeOf(o).z})`, `T(()=>{var o=JSON.parse('{"a":1,"__proto__":null,"b":2}');return DD(o)})`,
  `T(()=>P({...JSON.parse('{"__proto__":{"z":1}}')}))`, `T(()=>P(Object.assign({},JSON.parse('{"__proto__":{"z":1}}'))))`,
  `T(()=>P(Object.fromEntries([["__proto__",{z:1}]])))`, `T(()=>{var o={};o["__proto__"]={z:1};return P(o)})`, `T(()=>{var o={};Object.defineProperty(o,"__proto__",{value:{z:1},enumerable:true});return P(o)})`,
  `T(()=>{var o={__proto__:{z:1}};var c={...o};return P(c)})`, `T(()=>{var o={__proto__:{z:1}};return JSON.stringify(o)+Object.keys(o)})`, `T(()=>{var o={["__proto__"]:{z:1}};return JSON.stringify(o)+P({...o})})`,
  `T(()=>{var {__proto__:p}={z:1};return S(p)})`, `T(()=>{var {__proto__:p}={__proto__:{z:1}};return S(p)})`, `T(()=>{var {["__proto__"]:p}={__proto__:{z:1}};return S(p)})`,
  `T(()=>Object.getOwnPropertyDescriptor(Object.prototype,"__proto__").get.name)`, `T(()=>Object.getOwnPropertyDescriptor(Object.prototype,"__proto__").set.length)`,
  `T(()=>{var o={__proto__:Array.prototype};return Array.isArray(o)+" "+(o instanceof Array)+" "+o.length})`,
  `T(()=>{var o={__proto__:{get x(){return this.y}},y:4};return o.x})`, `T(()=>{var o={__proto__:{set x(v){this.w=v}}};o.x=3;return DD(o)})`,
  `T(()=>{var p={x:1};var o={__proto__:p,x:2};delete o.x;return o.x})`, `T(()=>{var o={__proto__:{a:1}};var r=[];for(var k in o)r.push(k);return r.join()})`,
  `T(()=>{var o={a:1,__proto__:{b:2}};var r=[];for(var k in o)r.push(k);return r.join()})`, `T(()=>{var o={__proto__:new Proxy({},{get(t,k){return "P"+String(k)}})};return o.foo})`,
  `T(()=>{var p=Object.freeze({});var o={__proto__:p};o.x=1;return o.x})`, `T(()=>{var p=Object.freeze({x:1});var o={__proto__:p};o.x=2;return o.x+" "+Object.hasOwn(o,"x")})`,
  `T(()=>{"use strict";var p=Object.freeze({x:1});var o={__proto__:p};o.x=2})`, `T(()=>{var o=Object.preventExtensions({});return Object.getPrototypeOf({__proto__:o})===o})`,
  `T(()=>{var a={};var b={__proto__:a};Object.setPrototypeOf(a,b)})`,);

// ---- 4. Spread com getters e proxies: prefixo, fonte e sufixo.
const spreadSources = [
  '{get a(){L.push("a");return 1},get b(){L.push("b");return 2}}', "{b:1,a:2}", "[1,2]", '"ab"', "null", "undefined", "5", "true", "Symbol()",
  "{[sym]:1,x:2,1:3}",
  'new Proxy({a:1,b:2},{ownKeys(t){L.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){L.push("gopd "+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){L.push("get "+String(k));return t[k]}})',
  'Object.create({inherited:1},{own:{value:1,enumerable:true},hidden:{value:2}})', "new (class{constructor(){this.i=1}m(){}})", "function(){}", "new Map([[1,2]])",
  '{get a(){throw new RangeError("boom")}}', "1n", 'new String("xy")', "[,1]", "{a:1,get a(){return 2}}", "{a:1,b:2,get c(){L.push('c');return 3},d:4}",
  '{get [sym](){L.push("sym");return 1},get 2(){L.push("2");return 1},get a(){L.push("a");return 1}}', "Object.defineProperty({a:1},'a',{enumerable:false})",
  "new Proxy({a:1},{ownKeys(){return ['a','a']}})", "new Proxy([1,2],{})", "new Uint8Array([1,2])", "new Set([1])", "Object.freeze({a:1})",
];
const spreadPrefix = ["", "a:0,", "b:0,", 'get a(){L.push("pa");return 0},', 'set a(v){L.push("set")},', "[sym]:0,", "0:0,", "x:0,y:0,"];
const spreadSuffix = ["", ",a:9"];
for (const pre of spreadPrefix) for (const src of spreadSources) for (const suf of spreadSuffix)
  add(`T(()=>{L=[];var o={${pre}...${src}${suf}};return L.join()+"|"+DD(o)})`);
add(
  `T(()=>{L=[];var o={...{get a(){L.push("get");return 1}},...{get a(){L.push("get2");return 2}}};return L.join()+DD(o)})`,
  `T(()=>{var s=0;var o={...{a:++s},b:++s,...{c:++s}};return DD(o)})`, `T(()=>{var o={...(L.push("1"),{a:1}),...(L.push("2"),{b:2})};return L.join()+S(o)})`,
  `T(()=>{var o={a:1,...{a:2},a:3};return DD(o)})`, `T(()=>{var o={...{a:1,b:2},...{b:3,a:4}};return S(o)})`, `T(()=>{var o={...[1,2,3],length:1};return S(o)})`,
  `T(()=>{var o={..."abc",...[9]};return S(o)})`, `T(()=>{var o={...{get __proto__(){return 1}}};return P(o)})`, `T(()=>{var o={...JSON.parse('{"__proto__":{"z":1}}')};return P(o)})`,
  `T(()=>{var x={__proto__:{inh:1},own:2};var o={...x};return S(o)})`, `T(()=>{var o={...{a:1},__proto__:{z:1}};return P(o)})`, `T(()=>{var o={...null,...undefined};return S(o)})`,
  `T(()=>{var o={...{a:1}};Object.freeze(o);return Object.isFrozen(o)+" "+DD(o)})`,
);

// ---- 5. Métodos com super e home object.
const protoSetups = [
  { n: "lit", def: (m) => `{__proto__:{x:1,f(){return "pf"+this.tag},get g(){return this.tag+"g"}},tag:"T",${m}}` },
  { n: "late", def: (m) => `{tag:"T",${m}}`, post: `Object.setPrototypeOf(o,{x:2,f(){return "lf"+this.tag},get g(){return this.tag+"g"}});` },
  { n: "null", def: (m) => `{__proto__:null,tag:"T",${m}}` },
  { n: "none", def: (m) => `{tag:"T",${m}}` },
  { n: "fnproto", def: (m) => `{__proto__:Array.prototype,tag:"T",${m}}` },
];
const superKinds = [
  { d: "m(){return super.x}", c: (O) => `${O}.m()` }, { d: "get m(){return super.x}", c: (O) => `${O}.m` },
  { d: "set m(v){super.x=v}", c: (O) => `(${O}.m=5,[${O}.x,Object.hasOwn(${O},"x")].join())` },
  { d: "async m(){return super.x}", c: (O) => `typeof ${O}.m().then` }, { d: "*m(){yield super.x}", c: (O) => `[...${O}.m()].join()` },
  { d: "m(){return (()=>super.x)()}", c: (O) => `${O}.m()` }, { d: "m(){return super.f()}", c: (O) => `${O}.m()` },
  { d: "m(){return super.f.call(this)}", c: (O) => `${O}.m()` }, { d: 'm(){return super["x"]}', c: (O) => `${O}.m()` },
  { d: '["m"](){return super.x}', c: (O) => `${O}.m()` }, { d: "m(){return super.x?.y}", c: (O) => `${O}.m()` },
  { d: "m(){return super.constructor===Object}", c: (O) => `${O}.m()` }, { d: "m(){return delete super.x}", c: (O) => `${O}.m()` },
  { d: "m(){super.x++;return this.x}", c: (O) => `${O}.m()` }, { d: "m(){return super.f`t`}", c: (O) => `${O}.m()` },
  { d: "m(){return super.toString===Object.prototype.toString}", c: (O) => `${O}.m()` }, { d: "m(){return super.g}", c: (O) => `${O}.m()` },
  { d: "m(){super.y=1;return Object.hasOwn(this,'y')}", c: (O) => `${O}.m()` }, { d: "m(){return super[Symbol.iterator]===undefined}", c: (O) => `${O}.m()` },
  { d: "m(){return super.x+super.x}", c: (O) => `${O}.m()` },
];
for (const sk of superKinds) for (const ps of protoSetups) {
  const base = `var o=${ps.def(sk.d)};${ps.post || ""}`;
  add(`T(()=>{${base}return S(${sk.c("o")})})`);
  add(`T(()=>{${base}var q={x:9,tag:"Q",f(){return "qf"+this.tag}};Object.defineProperty(q,"m",Object.getOwnPropertyDescriptor(o,"m"));return S(${sk.c("q")})})`);
}
const superSyntax = [
  "({a:()=>super.x})", "({a:function(){return super.x}})", "({m(){function f(){super.x}}})", "({m(){return super()}})", "({get a(){return super.x}})", "({a:super.x})",
  "({m(){super}})", "({m(){return super?.x}})", "({async *m(){return super.x}})", "({m(){return new super.x}})", "({m(){return new super}})",
  "({['a']:class{m(){return super.x}}})", "({m(){return class extends (super.x){}}})", "({m(){return {n(){return super.x}}}})", "({m(){return {n:()=>super.x}}})",
  "({m(){return eval('super.x')}})", "({m(){return (0,eval)('super.x')}})", "({m(){return new Function('return super.x')}})", "({m(a=super.x){return a}})",
  "({m(){super.x=1;super.y=2}})", "({m(){return super.#x}})", "({static m(){}})", "({m(){return [super.x]}})", "({m(){super.x\n.y}})", "({m(){return super.x`a`}})",
  "({m(){return typeof super.x}})", "({m(){for(super.x of []);}})", "({m(){[super.x]=[1]}})", "({m(){({a:super.x}={a:1})}})", "({m(){super.x=>1}})",
];
for (const s of superSyntax) add(`E(${q(s)})`, `E(${q('"use strict";' + s)})`);
add(
  `T(()=>{var o={m(){return super.x},__proto__:{x:1}};return o.m()})`, `T(()=>{var a={x:1};var b={x:2};var o={__proto__:a,m(){return super.x}};Object.setPrototypeOf(o,b);return o.m()})`,
  `T(()=>{var o={__proto__:{x:1},m(){return super.x}};var f=o.m;return f.call({x:5})})`, `T(()=>{var o={__proto__:{x:1},m(){return super.x}};var f=o.m;return f()})`,
  `T(()=>{var o={__proto__:{get x(){return this.t}},t:1,m(){return super.x}};return o.m.call({t:2})})`,
  `T(()=>{var o={__proto__:{set x(v){this.set=v}},m(){super.x=3;return S(this)}};return o.m()})`,
  `T(()=>{var o={__proto__:Object.freeze({x:1}),m(){super.x=3;return this.x}};return o.m()})`,
  `T(()=>{"use strict";var o={__proto__:Object.freeze({x:1}),m(){super.x=3;return this.x}};return o.m()})`,
  `T(()=>{"use strict";var o={m(){super.x=3;return this.x}};return o.m.call(1)})`, `T(()=>{var o={m(){super.x=3;return this.x}};return typeof o.m.call(1)})`,
  `T(()=>{var o={m(){return super.x},__proto__:{x:1}};return o.m.call(null)})`, `T(()=>{"use strict";var o={m(){return super.x},__proto__:{x:1}};return o.m.call(undefined)})`,
  `T(()=>{var o={m(){return super.x},__proto__:{x:1}};return Object.hasOwn(o.m,"prototype")+" "+T(()=>new o.m)})`,
  `T(()=>{var o={m(){return super.x}};return Reflect.ownKeys(o.m).map(String).join()})`, `T(()=>{var o={a:function(){}};return Reflect.ownKeys(o.a).map(String).join()})`,
  `T(()=>{var o={__proto__:{x:1},m:function(){return typeof super_}};return o.m()})`,
  `T(()=>{var o={__proto__:{f(){return "P"}},m(){return {n(){return super.f()}}.n()}};return o.m()})`,
  `T(()=>{var o={__proto__:{f(){return "P"}},m(){return (()=>(()=>super.f())())()}};return o.m()})`,
  `T(()=>{var o={__proto__:{f(){return "P"}},m(){return class{static s(){return super.name}}.s()}};return o.m()})`,
  `T(()=>{var o={__proto__:{f(){return "P"}},get m(){return super.f()}};return o.m})`, `T(()=>{var o={__proto__:{f(){return "P"}},["m"+1](){return super.f()}};return o.m1()})`,
  `T(()=>{var o={__proto__:{f(){return "P"}},m(){return super.f()+super.f}};return o.m()})`, `T(()=>{var o={__proto__:{f(){return this===o}},m(){return super.f()}};return o.m()})`,
  `T(()=>{var o={__proto__:{f(){return "P"}},m(){return super.f?.()}};return o.m()})`, `T(()=>{var o={__proto__:{},m(){return super.f?.()}};return o.m()})`, `T(()=>{var o={__proto__:{},m(){return super.f()}};return o.m()})`,
  `T(()=>{var o={__proto__:null,m(){return super.f}};return o.m()})`, `T(()=>{var o={__proto__:null,m(){super.f=1;return this.f}};return o.m()})`,
);

// ---- 6. Nomes de funções em chaves, símbolos e computed; getters/setters com nomes numéricos.
const nameKeys = [
  "a", '"b c"', "1", "1.5", "[sym]", "[Symbol()]", "[Symbol.iterator]", '["x"+1]', "[1n]", "[null]", "0x10", "1e21", '""', '[Symbol("")]', "1n", ".5", '["__proto__"]',
  "[{toString(){return 'ts'}}]", "[Symbol.for('reg')]", "[-0]", "get", "set", "async", "static", "constructor", "0b11", '"1e3"', "1e3",
];
const nameKinds = [
  (k) => `${k}:function(){}`, (k) => `${k}:()=>1`, (k) => `${k}:class{}`, (k) => `${k}(){}`, (k) => `get ${k}(){}`, (k) => `set ${k}(v){}`, (k) => `async ${k}(){}`,
  (k) => `*${k}(){}`, (k) => `async *${k}(){}`, (k) => `${k}:function g(){}`, (k) => `${k}:(0,function(){})`, (k) => `${k}:class{static name="x"}`,
  (k) => `${k}:class{static name(){}}`, (k) => `${k}:async()=>1`, (k) => `${k}:function*(){}`, (k) => `${k}:(function(){})`, (k) => `${k}:class C{}`,
  (k) => `${k}:function(a,b=1,c){}`, (k) => `${k}:(a,...r)=>1`, (k) => `set ${k}([a,b]){}`, (k) => `set ${k}({a}=1){}`,
];
for (const k of nameKeys) for (const f of nameKinds) add(`T(()=>{var o={${f(k)}};return N(o)})`);
add(
  `T(()=>{var o={};o.a=function(){};o["b"]=()=>1;o[sym]=class{};return N(o)})`, `T(()=>{var {a=function(){}}={};return a.name})`, `T(()=>{var [a=function(){}]=[];return a.name})`,
  `T(()=>{var o={a:function(){}}.a;return o.name})`, `T(()=>{var o;o={a:1,b:function(){}};return N(o)})`, `T(()=>{var f;f=function(){};return f.name})`, `T(()=>{var f=function g(){};return f.name})`,
  `T(()=>{var o={f:(function(){})};return o.f.name})`, `T(()=>{var o={f:(1,function(){})};return o.f.name})`, `T(()=>{var o={f:true&&function(){}};return o.f.name})`,
  `T(()=>{var o={f:(0||function(){})};return o.f.name})`, `T(()=>{var o={f:null??function(){}};return o.f.name})`, `T(()=>{var o={f:true?function(){}:0};return o.f.name})`,
  `T(()=>{var o={[sym]:function(){}};return o[sym].name})`, `T(()=>{var o={[Symbol("x y")]:()=>1};return Reflect.ownKeys(o).map(k=>o[k].name)})`,
  `T(()=>{var o={get [sym](){}};return Object.getOwnPropertyDescriptor(o,sym).get.name})`, `T(()=>{var o={set [Symbol()](v){}};return JSON.stringify(Object.getOwnPropertyDescriptor(o,Reflect.ownKeys(o)[0]).set.name)})`,
  `T(()=>{var o={get 1(){return 1},set 1(v){}};return DD(o)+N(o)})`, `T(()=>{var o={get 0x10(){return 16}};return DD(o)+Object.keys(o)})`, `T(()=>{var o={get 1e3(){return 1}};return Object.keys(o)+N(o)})`,
  `T(()=>{var o={get .5(){return 1}};return Object.keys(o)+N(o)})`, `T(()=>{var o={get 1n(){return 1}};return Object.keys(o)+N(o)})`, `T(()=>{var o={get 'a b'(){return 1}};return Object.keys(o)+N(o)})`,
  `T(()=>{var o={get [1+1](){return 2}};return Object.keys(o)+N(o)})`, `T(()=>{var o={get ['x'](){return 1},set ['x'](v){}};return DD(o)})`,
  `T(()=>{var o={get 01(){return 1}};return Object.keys(o)})`, `T(()=>{var o={get 4294967295(){return 1},get 4294967294(){return 2}};return Object.keys(o)})`,
  `T(()=>{var o={set 2(v){this.s=v}};o[2]=7;return S(o)})`, `T(()=>{var o={get 2(){return 2}};o[2]=7;return S(o[2])})`,
  `T(()=>{"use strict";var o={get 2(){return 2}};o[2]=7})`, `T(()=>{"use strict";var o={set 2(v){}};return o[2]})`,
  `T(()=>{var o={a:class{static x=this.name}};return o.a.x})`, `T(()=>{var o={a:class{static x=this.name;static name="n"}};return o.a.x+o.a.name})`,
);
const accSyntax = [
  "({get a(x){}})", "({set a(){}})", "({set a(x,y){}})", "({set a(...x){}})", "({set a(x=1){}})", "({set a([x]){}})", "({set a({x}){}})", "({get a(){},get a(){}})",
  "({get(){}})", "({set(){}})", "({get:1})", "({get,set})", "({get a})", "({get a:1})", "({async get a(){}})", "({get async a(){}})", "({get *a(){}})", "({async\na(){}})",
  "({get\na(){}})", "({get 1(){},get '1'(){}})", "({get a(){},a:1,set a(v){}})", "({set a(v){'use strict'}})", "({set a(eval){'use strict'}})", "({get a(){'use strict';return 010}})",
  "({set a(a){let a}})", "({set a(a){var a}})", "({a(a,a){}})", "({a(a,a){'use strict'}})", "({a(a=1,a){}})", "({a(){let a;var a}})", "({*a(){yield}})", "({*a(yield){}})",
  "({async a(await){}})", "({async a(){await}})", "({a:1,})", "({,})", "({a:1,,})", "({a b})", "({1 a(){}})", "({'a'})", "({1})", "({[a]})", "({a=1})", "({a=1}={})",
  "({a:1}=1)", "({...a,})", "({...a,b}={})", "({...{}}={})", "({...a}={}) ", "({a(){}.b})", "({a(){}}.a)", "({a:1}.a)", "({a:1}\n.a)", "{a:1}", "{a:1,b:2}", "{a:1;b:2}", "({a:1;b:2})",
  "({\\u0061:1})", "({\\u{61}:1})", "({\\u0061(){}})", "({v\\u0061r:1})", "({g\\u0065t a(){}})", "({\\u0067et a(){}})", "({async\\u0020a(){}})", "({1.:1})", "({1.e1:1})", "({0n:1})", "({00:1})", "({09:1})", "({0_1:1})", "({1__0:1})", "({1_:1})",
  "({'\\08':1})", "({'\\u{110000}':1})", "({'\\x4':1})", "({'a\nb':1})", "({'a b':1})", "({`a`:1})", "({[`a`]:1})", "({[a,b]:1})", "({[(a,b)]:1})", "({[yield]:1})", "({[await]:1})",
  "({if:1,for:2,class:3,new:4,this:5,null:6,true:7,typeof:8,let:9,static:10,yield:11,await:12,enum:13})", "({if,for})", "({yield})", "({await})", "({let})", "({static})", "({enum})", "({null})", "({this})",
  "({eval})", "({arguments})", '"use strict";({eval})', '"use strict";({yield})', '"use strict";({let})', '"use strict";({static})', '"use strict";({implements})', '"use strict";({a:eval}=1)', '"use strict";({eval}=1)',
  '"use strict";({a:010})', '"use strict";({010:1})', '"use strict";({"\\01":1})', '"use strict";({a:"\\01"})', '"use strict";({a:1,a:2})', "({a:1,a:2})", "({a:1,get a(){}})",
];
for (const s of accSyntax) add(`E(${q(s)})`);

// ---- 7. JSON versus literal.
const jsonTexts = [
  '{"__proto__":1}', '{"1":1,"a":2,"0":3}', '{"a":1,"a":2}', '{"__proto__":null}', '{"-0":1}', '{"01":1}', '{"a":1,}', "{'a':1}", "{a:1}", '{"a":01}', '{"\\u0061":1}',
  '{"a":1,"b":{"__proto__":{"c":1}}}', '{"b":1,"a":2,"1":3,"0":4}', '{"4294967295":1,"4294967294":2,"4294967296":3}', '{"1e3":1,"1000":2}', '{"":1}', '{"a b":1}',
  '{"\\ud800":1}', '{"\\u2028":1}', '{"a":1e999}', '{"a":-0}', '{"a":0.1}', '{"a":1.0}', '{"a":"\\n"}', '{"a":[1,{"b":2}]}', '{"constructor":1,"toString":2}',
  '{"__proto__":{"__proto__":{}}}', '{"a":NaN}', '{"a":undefined}', '{"a":+1}', '{"a":.5}', '{"a":1.}', '{"a":0x1}', '{"a":\'x\'}', '{"a":"\\x41"}', '{"a":"\t"}',
  '{ "a" : 1 , "b" : 2 }', '{"a":1}{', '{"a":1} ', ' {"a":1}', '{"a" 1}', '{"a":}', '{:1}', '{"a":1 "b":2}', '{"a":1,,"b":2}', '[1,2,]', '{"a":{"b":{"c":{}}}}',
  '{"9007199254740991":1,"9007199254740992":2}', '{"-1":1,"1":2}', '{"1.5":1,"1":2}', '{"+1":1}', '{"1":1,"01":2,"001":3}', '{"b":1,"a":2,"B":3,"A":4}',
];
for (const s of jsonTexts) add(`T(()=>{var s=${q(s)};return J(s)+"|"+E("P("+s+")")+"|"+E("("+s+")")})`);
const jsonPool = pool.filter((k) => !k.startsWith("[{") && !k.startsWith("[[") && k !== "[Symbol.iterator]").slice(0, 20);
jsonPool.forEach((a, i) => jsonPool.forEach((b, j) => add(`T(()=>JSON.stringify({${a}:${i},${b}:${j % 3 === 0 ? "undefined" : j % 3 === 1 ? '"s"' : "[1]"}}))`)));
for (const a of jsonPool.slice(0, 10)) for (const b of jsonPool.slice(0, 10))
  add(`T(()=>{L=[];var r=JSON.stringify({${a}:1,${b}:{n:2}},(k,v)=>(L.push(k),v));return r+"|"+L.join()})`, `T(()=>{L=[];var r=JSON.parse('{"x":1}',function(k,v){L.push(k);return v});var o={${a}:1,${b}:2};return JSON.stringify(o,Reflect.ownKeys(o).filter(k=>typeof k==="string").reverse())+L.join()})`);
add(
  `T(()=>JSON.stringify({a:1,toJSON(){return 5}}))`, `T(()=>JSON.stringify({get a(){return 1},set b(v){},c(){}}))`, `T(()=>JSON.stringify({[sym]:1,a:undefined,b:()=>1,c:Symbol()}))`,
  `T(()=>JSON.stringify({a:1n}))`, `T(()=>JSON.stringify({1:1,b:2,0:0,[Symbol.iterator]:3}))`, `T(()=>JSON.stringify({a:new Date(0),b:new String("s"),c:Object(1),d:Object(true)}))`,
  `T(()=>JSON.stringify({__proto__:{x:1},y:2}))`, `T(()=>JSON.stringify({...{get a(){return 1}}}))`, `T(()=>JSON.stringify({a:{b:{c:1}}},null,2))`, `T(()=>JSON.stringify({a:[],b:{}},null,"--"))`,
  `T(()=>JSON.stringify({a:1,b:2},["b","a","b"]))`, `T(()=>JSON.stringify({1:1,2:2},[2,1]))`, `T(()=>JSON.stringify({a:1},[{}]))`, `T(()=>{var o={};o.o=o;return JSON.stringify(o)})`,
  `T(()=>JSON.stringify({a:1e21,b:-0,c:NaN,d:Infinity,e:1e-7}))`, `T(()=>JSON.stringify({"\\u2028":"\\u2028","\\ud800":"\\ud800"}))`, `T(()=>JSON.stringify({a:"\\u007f\\u0000\\u001f"}))`,
);

// ---- Execução.
const baseSet = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const program of knownPrograms("object_literal_bun.tsv", (file) => !(!file.endsWith(".tsv") || file === "object_literal_bun.tsv"))) baseSet.add(JSON.stringify(program));
const seen = new Set();
const programs = [];
let dup = 0;
for (const expr of exprs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  const source = PRELUDE + `globalThis.R = ${expr}`;
  if (baseSet.has(q(source))) { dup++; continue; }
  programs.push({ expr, source });
}

const runChild = (source) =>
  new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    let done = false;
    const finish = (value) => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => { child.kill("SIGKILL"); finish({ error: "timeout" }); }, 5000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => {
      const decoded = code === 0 ? decodeResult(out) : null;
      finish(decoded !== null ? { out: decoded } : { error: err || (code === 0 ? "filho sem resultado" : "status " + code) });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  const worker = async () => {
    while (next < programs.length) {
      const i = next++;
      results[i] = await runChild(programs[i].source);
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  programs.forEach((p, i) => {
    const r = results[i];
    if (r.error) { dropped++; process.stderr.write("erro de programa: " + q(p.expr).slice(0, 160) + " " + r.error.slice(0, 200) + "\n"); return; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; process.stderr.write("caminho ou marca no resultado: " + q(p.expr).slice(0, 160) + "\n"); return; }
    kept++;
    lines.push(q(p.source) + "\t" + q(r.out));
  });
  process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
