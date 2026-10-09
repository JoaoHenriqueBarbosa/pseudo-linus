// Gera tests/golden/function_scope_bun.tsv: escopo e funções, medido no bun 1.4.2.
// Cobre hoisting de var/function/let/class (TDZ, switch, closures, parâmetros), declarações duplicadas por escopo
// (SyntaxError exato), parâmetros padrão com escopo próprio, `arguments` mapeado vs não mapeado (strict, default, rest),
// name/length de funções (expressões, métodos, getters, bound, anônimas atribuídas, builtins), funções em blocos
// (Annex B), new.target, closures em laços com let, IIFEs, recursão mútua e tail position, Function.prototype.toString
// de formas variadas, o construtor Function com parâmetros estranhos e mensagens exatas de erro via Function/eval.
// Cada programa roda num bun filho novo (a ordem de reificação de propriedades estáticas depende do processo), os
// filhos rodam em paralelo e a saída segue a ordem de geração. Programas cuja expressão já aparece em algum golden
// existente são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-function-scope-golden.js > tests/golden/function_scope_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactoredLines, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const J = JSON.stringify;
const exprs = [];
const add = (...list) => exprs.push(...list);
const wrap = (strict, body) => `T(()=>{${strict ? "'use strict';" : ""}${body}})`;
const both = (body) => { add(wrap(false, body)); add(wrap(true, body)); };

// ---- 1. Hoisting: declaração x lugar x forma de acesso, em sloppy e strict.
const decls = {
  var: "var x=1;", let: "let x=1;", const: "const x=1;", fn: "function x(){return 1}", cls: "class x{}",
  fnexpr: "var x=function(){return 1};", arrow: "var x=()=>1;", gen: "function* x(){}", asyncfn: "async function x(){}",
};
const reads = {
  read: "x", typeof: "typeof x", call: "x()", assign: "x=5", incr: "x++", member: "x.y", new: "new x", delete: "delete x", void: "void x", tmpl: "`${x}`",
};
const places = {
  top: (d, r) => `var out;try{out=S(${r})}catch(e){out=e.name+": "+e.message}${d}return out+"/"+typeof x`,
  block: (d, r) => `var out;{try{out=S(${r})}catch(e){out=e.name+": "+e.message}${d}}return out+"/"+typeof x`,
  outside: (d, r) => `var out;{${d}}try{out=S(${r})}catch(e){out=e.name+": "+e.message}return out`,
  closure: (d, r) => `var out;function g(){return ${r}}try{out=S(g())}catch(e){out=e.name+": "+e.message}${d}try{return out+"/"+S(g())}catch(e){return out+"/"+e.name+": "+e.message}`,
  switch: (d, r) => `var out;switch(1){case 0:${d}case 1:try{out=S(${r})}catch(e){out=e.name+": "+e.message}}return out`,
  loop: (d, r) => `var out;for(var i=0;i<1;i++){try{out=S(${r})}catch(e){out=e.name+": "+e.message}${d}}return out+"/"+typeof x`,
  catch: (d, r) => `var out;try{throw 1}catch(e){try{out=S(${r})}catch(e2){out=e2.name+": "+e2.message}${d}}return out`,
  param: (d, r) => `function g(p=${r}){${d}return S(p)}try{return g()}catch(e){return e.name+": "+e.message}`,
  paramOuter: (d, r) => `var x="outer";function g(p=${r}){${d}return S(p)}try{return g()}catch(e){return e.name+": "+e.message}`,
};
for (const [dn, d] of Object.entries(decls)) for (const [rn, r] of Object.entries(reads)) for (const [pn, p] of Object.entries(places)) both(p(d, r));
// Hoisting em cadeia e formas soltas.
const hoistMisc = [
  "var a=typeof f;function f(){}return a", "var a=typeof f;var f=function(){};return a", "var a=typeof f;var f=1;function f(){}return a+typeof f",
  "function f(){return 1}var f;return typeof f", "function f(){return 1}var f=2;return typeof f", "var f=2;function f(){return 1}return typeof f",
  "function f(){return 1}function f(){return 2}return f()", "return typeof f;function f(){}", "return typeof f;var f", "return typeof f;let f", "return typeof f;class f{}",
  "if(true){function f(){return 1}}else{function f(){return 2}}return f()", "if(false){function f(){return 1}}return typeof f", "var r=typeof f;if(false){function f(){}}return r+typeof f",
  "var r=typeof f;{function f(){}}return r+typeof f", "var r=[];for(var i=0;i<2;i++){r.push(typeof f);function f(){}}return r.join()", "var r=typeof f;try{throw 0}catch(e){function f(){}}return r+typeof f",
  "var r=typeof f;switch(0){case 0:function f(){}}return r+typeof f", "var r=typeof f;lbl:function f(){}return r+typeof f", "return typeof f;{function f(){}}",
  "let a=1;{let a=2;{let a=3;var s=a}}return s", "var a=1;{var a=2}return a", "let a=1;function g(){return a}{let a=2;return g()}", "let a=1;{function g(){return a}let a=2;return g()}",
  "return typeof typeof x", "return (function(){return typeof x;var x=1})()", "return (function(x){return typeof x;var x=1})(3)", "return (function(x){var x;return x})(3)", "return (function(x){var x=4;return x})(3)",
  "return (function(x){function x(){}return typeof x})(3)", "return (function(x){return typeof x;function x(){}})(3)", "return (function(x=1){var x;return x})(3)", "return (function(x=1){var x;return x})()",
  "return (function(x=1){var x=x;return x})()", "return (function(x=1){var x=x+1;return x})(5)", "return (function(x=1){function x(){}return typeof x})(5)", "return (function(x=1){return typeof x;function x(){}})(5)",
  "return (function(x=()=>y){var y=2;return x()})()", "return (function(x=()=>typeof y){var y=2;return x()})()", "var y='o';return (function(x=()=>y){var y=2;return x()})()", "var y='o';return (function(x=()=>y){return x()})()",
  "return (function(){return typeof g;{function g(){}}})()", "return (function(){'use strict';return typeof g;{function g(){}}})()", "class A{static f(){return typeof A}}return A.f()", "var A=class B{static f(){return typeof B}};return A.f()+typeof B",
  "var A=class B{static f(){return B=1}};return A.f()", "var A=class B{static f(){'use strict';return B=1}};return A.f()", "var A=function B(){return B=1};return typeof A()", "var A=function B(){'use strict';B=1};return A()",
  "var A=function B(){var B=5;return B};return A()", "var A=function B(B){return B};return A(7)", "var A=function B(){return typeof B};return A()+typeof B", "return typeof class{}.name", "const c=1;try{c=2}catch(e){return e.message}",
  "let a=a;", "let a=(()=>a)();", "const k=k+1;", "class C extends C{}", "class C{static x=C.y;static y=1}return S(C.x)", "class C{static x=this.y;static y=1}return S(C.x)", "var C=class{static a=C}",
  "var r=[];function f(){r.push(1)}f();function f(){r.push(2)}f();return r.join()", "var r=[];f();function f(){r.push(1)}f=function(){r.push(2)};f();return r.join()",
  "return (()=>{return typeof x;var x})()", "var x=1;function g(){return x;var x=2}return typeof g()", "var x=1;function g(){var r=x;var x=2;return r}return typeof g()", "var x=1;function g(){x=3;var x=2}g();return x",
  "var x=1;function g(){x=3;{let x=2}}g();return x", "var x=1;function g(){{let x=2;x=5}return x}return g()", "for(var i=0;i<2;i++){var v=i}return S([i,v])", "for(let i=0;i<2;i++){}return typeof i",
  "for(var [a,b]=[1,2];a<2;a++){}return S([a,b])", "for(var i in {a:1}){}return i", "for(var i of [7]){}return i", "try{throw 1}catch(e){var e=2}return typeof e", "try{throw 1}catch(e){var e=2;var r=e}return S([r,typeof e])",
  "try{throw 1}catch(e){function e(){}}return typeof e", "try{throw 1}catch(e){for(var e of [2]){}}return typeof e",
].forEach((b) => both(b));

// ---- 2. Declarações duplicadas: SyntaxError exato por par e contexto.
const dk = {
  var: "var a;", varinit: "var a=1;", let: "let a;", const: "const a=1;", fn: "function a(){}", cls: "class a{}", gen: "function* a(){}", asyncfn: "async function a(){}",
};
const ctxs = {
  fnbody: (s) => s, arrowbody: (s) => `(()=>{${s}})`, block: (s) => `{${s}}`, switch: (s) => `switch(0){case 0:${s}}`,
  twoCases: (s) => `switch(0){case 0:${s}}`, staticblock: (s) => `class C{static{${s}}}`, method: (s) => `({m(){${s}}})`,
  label: (s) => `L:{${s}}`, forbody: (s) => `for(;;){${s}break}`, ifblock: (s) => `if(1){${s}}`, trycatch: (s) => `try{${s}}catch(e){}`, nestedblocks: (s) => `{${s}{}}`,
};
for (const [n1, a] of Object.entries(dk)) for (const [n2, b] of Object.entries(dk)) for (const [cn, c] of Object.entries(ctxs)) {
  const src = c(a + b);
  add(`T(()=>{Function(${J(src)});return "ok"}) + T(()=>{Function(${J("'use strict';" + src)});return "ok"})`);
}
const paramCtx = [
  "function f(a){%}", "function f(a=1){%}", "function f(...a){%}", "function f([a]){%}", "function f({a}){%}", "function f(a,b=a){%}", "(a)=>{%}", "(a=0)=>{%}", "(...a)=>{%}", "({m(a){%}})",
  "try{}catch(a){%}", "try{}catch([a]){%}", "try{}catch({a}){%}", "try{}catch(a){{%}}", "for(let a of []){%}", "for(let a=0;;){%break}", "for(const a of []){%}", "for(let a in {}){%}",
  "class C{m(a){%}}", "class C{static m(a){%}}", "(function a(){%})", "(function(a,a){%})", "(function*(a){%})", "(async function(a){%})", "(async(a)=>{%})", "(class a{static{%}})",
];
for (const p of paramCtx) for (const [n, a] of Object.entries(dk)) {
  const src = p.replace("%", a);
  add(`T(()=>{Function(${J(src)});return "ok"}) + T(()=>{Function(${J("'use strict';" + src)});return "ok"})`);
}
[
  "function f(a,a){}", "function f(a,a=1){}", "function f(a,...a){}", "function f(a,[a]){}", "(a,a)=>1", "function f(a,a){'use strict'}", "function f(a,a){'use strict';}", "function f(a=1){'use strict'}",
  "function f([a]){'use strict'}", "function f(...a){'use strict'}", "(function(a,b,a){})", "({m(a,a){}})", "class C{m(a,a){}}", "function* g(a,a){}", "async function g(a,a){}", "async(a,a)=>1",
  "function f(eval){'use strict'}", "function f(arguments){'use strict'}", "function f(yield){'use strict'}", "function f(let){'use strict'}", "function f(static){'use strict'}", "function f(implements){'use strict'}",
  "function eval(){'use strict'}", "function arguments(){'use strict'}", "'use strict';function eval(){}", "'use strict';var eval", "'use strict';var arguments", "'use strict';eval=1", "'use strict';arguments=1", "'use strict';eval++",
  "let let", "let [let]=[]", "const let=1", "var let", "'use strict';var let", "'use strict';var static", "var yield", "'use strict';var yield", "function* g(){var yield}", "async function g(){var await}", "var await", "class C{static{var await}}",
  "let a,a", "const a=1,a=2", "var a;let a", "let a;var a", "{let a;var a}", "{var a;let a}", "{let a;{var a}}", "{{var a}let a}", "function f(){let a;{var a}}", "let a;{function a(){}}", "{function a(){}function a(){}}",
  "'use strict';{function a(){}function a(){}}", "{function a(){}var a}", "{var a;function a(){}}", "{function a(){}let a}", "{function* a(){}function a(){}}", "{async function a(){}function a(){}}", "{function a(){}class a{}}",
  "switch(0){case 0:function a(){}default:function a(){}}", "switch(0){case 0:let a;default:var a}", "switch(0){case 0:let a;default:let a}", "for(let a,a;;){}", "for(let [a,a]=[];;){}", "let [a,a]=[]", "const {a,a:a}={}", "var [a,a]=[]",
  "for(let a;;){var a}", "for(let a of []){var a}", "for(let a in {}){var a}", "for(const a of []){var a}", "for(let a of []){{var a}}", "for(let a of []){function a(){}}", "for(var a of []){let a}", "for(let a;;){let a;break}",
  "try{}catch(a){let a}", "try{}catch(a){var a}", "try{}catch(a){function a(){}}", "try{}catch(a){for(var a of []){}}", "try{}catch(a){for(var a in {}){}}", "try{}catch(a){for(var a;;){break}}", "try{}catch([a]){var a}", "try{}catch(a){{var a}}", "try{}catch(a,a){}",
  "try{}catch([a,a]){}", "class C{constructor(){}constructor(){}}", "class C{get constructor(){}}", "class C{static prototype(){}}", "class C{static prototype=1}", "class C{'constructor'(){}'constructor'(){}}", "class C{constructor=1}", "class C{#a;#a}", "class C{#a;get #a(){}}", "class C{get #a(){}set #a(v){}}", "class C{get #a(){}static set #a(v){}}",
  "class C{#constructor}", "class C{m(){this.#b}}", "class C{static{await 1}}", "class C{static{return}}", "class C{static{arguments}}", "class C{static{super()}}", "class C{x=arguments}", "class C{x=super()}", "class C{static x=arguments}",
  "label:label:;", "a:{a:;}", "a:function a(){}", "'use strict';a:function f(){}", "while(1)function f(){}", "if(1)function f(){}", "'use strict';if(1)function f(){}", "if(1)class C{}", "if(1)let a", "if(1)const a=1", "if(1)function* g(){}", "if(1)async function g(){}", "label:let\na", "do function f(){}while(0)", "for(;;)function f(){}", "with({})function f(){}",
].forEach((s) => add(`T(()=>{Function(${J(s)});return "ok"})`, `T(()=>{(0,eval)(${J(s)});return "ok"})`, `T(()=>{eval(${J("'use strict';" + s)});return "ok"})`));

// ---- 3. Parâmetros padrão: escopo próprio, closures sobre parâmetros, corpo com var.
const plist = [
  "a=1,b=a", "a=b,b=1", "a=1,b=()=>a", "a=()=>b,b=2", "a,b=()=>a,c=b()", "a=x", "a=typeof x", "a=arguments.length", "a=arguments[0]", "a=this===undefined", "a=(()=>arguments.length)()",
  "a=function(){return typeof a}", "a=eval('1')", "a=eval('var q=5;q')", "a=eval('var a2=3')", "a=1,{b}={b:a}", "[a]=[1],b=a", "a=a", "a=(b=2)", "a=1,b=(a=5,a)", "a,b=a+1,c=b+1", "a=()=>a",
  "a=1,b=()=>{a=9;return a}", "a=1,b=function(){return eval('a')}", "a=1,b=(()=>{var a=7;return a})()", "a=[1,2],[b,c]=a", "{a,b}={a:1,b:2}", "{a=1,b=a}={}", "a=1,...b", "a,b=arguments[0]",
];
const pbody = [
  "return S([typeof a,typeof b])", "var a;return S(a)", "var a=2;return S(a)", "var a=a;return S(a)", "function a(){}return typeof a", "a=9;return S(arguments.length)+S([typeof a])", "var q=1;return typeof q+typeof a",
  "return typeof b==='function'?S(b()):S(b)", "var b=7;return typeof b==='function'?'fn':S(b)", "{let a=3}return S(a)", "var a;a=3;return typeof b==='function'?S(b()):S(b)", "var b;return typeof b",
  "return S([...arguments])", "a=5;return S(arguments[0])", "var arguments;return S(arguments.length)", "let c=1;return S([a,c])", "return (()=>typeof q)()+typeof eval('typeof a')", "eval('var a=8');return S(a)", "eval('var zz=8');return typeof zz",
];
const pargs = ["", "5", "5,6", "undefined,undefined", "null", "{b:3}"];
for (const p of plist) for (const b of pbody) for (const a of pargs) both(`function f(${p}){${b}}return f(${a})`);

// ---- 4. `arguments` mapeado vs não mapeado.
const aparams = { simple: "a,b", defaultB: "a,b=2", rest: "a,...r", destr: "{a},b", dup: "a,a", defaultA: "a=1,b", one: "a" };
const aops = [
  "a=9;return arguments[0]", "arguments[0]=9;return a", "arguments[1]=9;return b", "delete arguments[0];arguments[0]=5;return S([a,arguments[0]])", "Object.defineProperty(arguments,'0',{writable:false});a=3;return S([a,arguments[0]])",
  "Object.defineProperty(arguments,'0',{value:7});return a", "Object.defineProperty(arguments,'0',{get(){return 1}});a=4;return S([a,arguments[0]])", "Object.defineProperty(arguments,'0',{enumerable:false});arguments[0]=8;return a",
  "arguments.length=0;return S([a,arguments[0],arguments.length])", "try{return typeof arguments.callee}catch(e){return e.name+': '+e.message}", "b=5;return S([].slice.call(arguments))", "var a;return S([a,arguments[0]])",
  "var arguments;return typeof arguments", "function arguments(){}return typeof arguments", "var arguments=3;return S(arguments)", "return (()=>arguments[0])()", "return Object.getOwnPropertyNames(arguments).join()",
  "return D(arguments,'0')", "arguments[0]=9;a=1;return S([...arguments])", "Object.freeze(arguments);a=7;return S([a,arguments[0]])", "a++;return arguments[0]", "return Object.prototype.toString.call(arguments)+arguments.length",
  "Object.defineProperty(arguments,'0',{writable:false,value:5});a=2;return S([a,arguments[0]])", "Object.defineProperty(arguments,'0',{configurable:false});delete arguments[0];a=2;return S([a,arguments[0]])",
  "Object.defineProperty(arguments,'0',{configurable:false});a=2;return S([a,arguments[0]])", "return D(arguments,'length')+D(arguments,Symbol.iterator)", "return arguments[Symbol.iterator]===[][Symbol.iterator]",
  "arguments[2]=1;return S([arguments.length,[...arguments]])", "return S(Object.keys(arguments))", "[a]=[5];return S(arguments[0])", "for(a of [3]);return S(arguments[0])", "for(var k in {z:1}){a=k}return S(arguments[0])",
  "return arguments.hasOwnProperty('callee')+','+Object.hasOwn(arguments,'callee')", "return typeof Object.getOwnPropertyDescriptor(arguments,'callee')&&D(arguments,'callee').replace(/fn/g,'F')",
];
const acalls = ["1", "1,2", "", "1,2,3"];
for (const [pn, p] of Object.entries(aparams)) for (const o of aops) for (const c of acalls) {
  add(wrap(false, `function f(${p}){${o}}return f(${c})`));
  if (pn !== "dup") add(wrap(true, `function f(${p}){${o}}return f(${c})`));
}
add(
  "T(()=>(function(a){'use strict';a=2;return arguments[0]})(1))", "T(()=>(function(a){a=2;return arguments[0]})(1))", "T(()=>((a)=>{try{return arguments[0]}catch(e){return e.name}})(1))",
  "T(()=>(function(){return (()=>arguments.length)()})(1,2,3))", "T(()=>(function(){return (()=>(()=>arguments[1])())()})(1,2,3))", "T(()=>(function(a){return eval('arguments[0]=5;a')})(1))", "T(()=>(function(a){return eval('a=5;arguments[0]')})(1))",
  "T(()=>(function(a){return eval('var arguments=1;arguments')})(1))", "T(()=>(function(a){'use strict';return eval('var arguments=1;arguments')})(1))", "T(()=>(function(){return typeof arguments})())",
  "T(()=>(function(){return arguments}).call(0,1).constructor===Object)", "T(()=>(function(){return Object.getPrototypeOf(arguments)===Object.prototype})())", "T(()=>(function(){return arguments.callee===undefined})())",
  "T(()=>(function(){'use strict';return arguments.callee})())", "T(()=>(function(){'use strict';return D(arguments,'callee').length>0})())", "T(()=>(function(){'use strict';var d=Object.getOwnPropertyDescriptor(arguments,'callee');return d.get===d.set&&typeof d.get})())",
  "T(()=>{var o={f(){return arguments.length}};return o.f(1,2)})", "T(()=>{var o={get g(){return arguments.length}};return o.g})", "T(()=>{var o={set g(v){this.n=arguments.length}};o.g=1;return o.n})", "T(()=>{class C{static m(){return arguments.length}}return C.m(1,2,3)})",
  "T(()=>{function f(){return arguments.length}return f.apply(null,{length:3})})", "T(()=>{function f(){return S([...arguments])}return f.apply(null,[1,,3])})", "T(()=>{function f(){return arguments.length}return f(...[1,2],...[3])})",
  "T(()=>{function f(){return arguments.length}return new f(1,2) instanceof f})", "T(()=>{function* g(){yield arguments.length}return g(1,2).next().value})", "T(()=>{async function f(){return arguments.length}return typeof f(1)})",
  "T(()=>{function f(){var a=arguments;return function(){return a===arguments}}return f()()})", "T(()=>{function f(){arguments=1;return arguments}return f()})", "T(()=>{function f(){'use strict';arguments=1}return f()})",
  "T(()=>{function f(){return delete arguments}return f()})", "T(()=>{function f(arguments){return arguments}return f(5)})", "T(()=>{function f(arguments=3){return arguments}return f()})", "T(()=>{function f(a=arguments){return a.length}return f(undefined,1)})",
  "T(()=>{function f(a=()=>arguments){return a().length}return f(undefined,1,2)})", "T(()=>{function f(a,b=arguments[0]){a=9;return b}return f(1)})", "T(()=>{function f(a,b=1){a=9;return arguments[0]}return f(1)})", "T(()=>{function f(...r){r[0]=9;return arguments[0]}return f(1)})",
  "T(()=>{function f(a,...r){a=9;return arguments[0]}return f(1)})", "T(()=>{function f({a}){a=9;return arguments[0].a}return f({a:1})})", "T(()=>{function f(a){return function(){a=9;return arguments.length}}return f(1)()})",
);

// ---- 5. name e length.
const fexprs = [
  "function(){}", "function n(){}", "()=>0", "async()=>0", "async function(){}", "function*(){}", "async function*(){}", "class{}", "class N{}", "class{static name='z'}", "class{static name(){}}", "class{static length=3}", "(function(){})", "((function(){}))",
  "(0,function(){})", "function(a,b=1,c){}", "function(...r){}", "(a,b)=>0", "function(a,{b},c){}", "(a=1)=>0", "function(a,b,){}", "class{constructor(a,b){}}", "class extends Object{}", "function(){}.bind()", "(function(){}).bind()", "(function g(){}).bind()",
  "Function()", "new Function('a','b','')", "Math.max", "Math.max.bind()", "(function(){}).bind().bind(null,1)", "async(a,b)=>0", "function*(a,b){}", "class{constructor(...a){}}", "class{constructor(a=1,b){}}", "class A{static f(){}}.f", "(a,[b,c],d)=>0", "(...a)=>0",
  "Symbol", "Symbol.prototype.toString", "[].map", "Object.getOwnPropertyDescriptor(Map.prototype,'size').get", "new Proxy(function(a,b){},{})", "new Proxy(class{},{})", "eval", "parseInt", "BigInt",
];
const nctx = {
  var: (e) => `var f=${e};return f`, let: (e) => `let f=${e};return f`, assign: (e) => `var f;f=${e};return f`, obj: (e) => `var o={f:${e}};return o.f`, objc: (e) => `var o={['f'+1]:${e}};return o.f1`, objsym: (e) => `var s=Symbol('s');var o={[s]:${e}};return o[s]`,
  objsym0: (e) => `var s=Symbol();var o={[s]:${e}};return o[s]`, destrdef: (e) => `var {f=${e}}={};return f`, arrdef: (e) => `var [f=${e}]=[];return f`, param: (e) => `function g(f=${e}){return f}return g()`,
  static: (e) => `class C{static f=${e}}return C.f`, field: (e) => `class C{f=${e}}return new C().f`, orassign: (e) => `var f;f||=${e};return f`, nullish: (e) => `var f;f??=${e};return f`, andassign: (e) => `var f=1;f&&=${e};return f`,
  member: (e) => `var o={};o.f=${e};return o.f`, paren: (e) => `var f=(${e});return f`, comma: (e) => `var f=(0,${e});return f`, cond: (e) => `var f=true?${e}:0;return f`, destrassign: (e) => `var f;({f=${e}}={});return f`, defaultexport: (e) => `var o={f:(${e})};return o.f`,
};
for (const e of fexprs) for (const [cn, c] of Object.entries(nctx)) both(`var q=(function(){${c(e)}})();return S([q.name,q.length,D(q,'name'),D(q,'length')])`);
const methods = [
  "{m(){}}", "{get m(){}}", "{set m(v){}}", "{*m(){}}", "{async m(){}}", "{async *m(){}}", "{['c'+'d'](){}}", "{[Symbol('d')](){}}", "{[Symbol()](){}}", "{1(){}}", "{'a b'(){}}", "{get 1(){}}", "{get [Symbol('q')](){}}", "{set [Symbol.iterator](v){}}",
  "{__proto__(){}}", "{m:function(){}}", "{m:()=>{}}", "{m(a,b){}}", "{m(a,b=1,c){}}", "{set m([a]){}}", "{get ['x'+1](){}}", "{async [Symbol('as')](){}}", "{*[Symbol.iterator](){}}", "{m:class{}}", "{m:function*(){}}", "{0.5(){}}", "{1e3(){}}", "{0x10(){}}", "{1n(){}}",
];
for (const m of methods) {
  add(wrap(false, `var o=${m};var k=Reflect.ownKeys(o)[0];var d=Object.getOwnPropertyDescriptor(o,k);var f=d.value||d.get||d.set;return S([f.name,f.length,Object.hasOwn(f,'prototype'),typeof f.prototype])`));
  add(wrap(false, `var o=${m};var k=Reflect.ownKeys(o)[0];var d=Object.getOwnPropertyDescriptor(o,k);var f=d.value||d.get||d.set;return S([D(f,'name'),D(f,'length')])`));
  add(wrap(false, `var o=${m};var k=Reflect.ownKeys(o)[0];var d=Object.getOwnPropertyDescriptor(o,k);var f=d.value||d.get||d.set;var b=f.bind();return S([b.name,b.length,Object.hasOwn(b,'prototype')])`));
}
const cmethods = [
  "m(){}", "static m(){}", "get m(){}", "static get m(){}", "set m(v){}", "static set m(v){}", "*m(){}", "static async m(){}", "['c'+'d'](){}", "static [Symbol('d')](){}", "[Symbol()](){}", "static 1(){}", "'a b'(){}", "static async *m(){}", "get [Symbol.toStringTag](){return 'X'}",
  "static m=function(){}", "static m=()=>{}", "m=function(){}", "m=()=>{}", "static ['x'+1]=function(){}", "static [Symbol('z')]=()=>1", "static m=class{}", "static m=(0,function(){})",
];
for (const m of cmethods) add(wrap(false, `class C{${m}}var T1=/^static /.test(${J(m)})?C:C.prototype;var k=Reflect.ownKeys(T1).filter(x=>!['length','name','prototype','constructor'].includes(x))[0];var d=Object.getOwnPropertyDescriptor(T1,k);var f=d.value||d.get||d.set;if(${J(m)}.indexOf('m=')>=0||${J(m)}.indexOf("1=")>=0||${J(m)}.indexOf('z')>=0){f=(T1===C?C:new C())[k]}return S([typeof k==='symbol'?String(k):k,f.name,f.length])`));
[
  "class C{static #p(){}static g(){return C.#p}}return S([C.g().name,C.g().length])", "class C{#p(){}g(){return this.#p}}return S([new C().g().name])", "class C{static get #p(){return 1}static g(){return Object.getOwnPropertyDescriptor}}return 1",
  "class C{#p=function(){};g(){return this.#p}}return new C().g().name", "class C{#p=()=>1;g(){return this.#p}}return new C().g().name", "class C{static #p=class{};static g(){return C.#p}}return C.g().name", "class C{*#p(){}g(){return this.#p}}return new C().g().name",
  "class C{async #p(a){}g(){return this.#p}}return S([new C().g().name,new C().g().length])", "return S([Object.getOwnPropertyDescriptor(Map.prototype,'size').get.name,Object.getOwnPropertyDescriptor(Map.prototype,'size').get.length])",
  "return S([Object.getOwnPropertyDescriptor(RegExp.prototype,'flags').get.name,Object.getOwnPropertyDescriptor(RegExp.prototype,'global').get.length])", "return S([Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set.name,Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').get.name])",
  "return Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get.name", "return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get.name", "return Object.getOwnPropertyDescriptor(Uint8Array.prototype.__proto__,Symbol.toStringTag).get.name",
  "return Object.getOwnPropertyDescriptor(Array,Symbol.species).get.name", "return Object.getOwnPropertyDescriptor(Map,Symbol.species).get.name", "return Symbol.prototype[Symbol.toPrimitive].name+Symbol.prototype[Symbol.toPrimitive].length", "return Array.prototype[Symbol.iterator].name+Array.prototype[Symbol.iterator].length",
  "return Array.prototype.values===Array.prototype[Symbol.iterator]", "return Map.prototype.entries===Map.prototype[Symbol.iterator]", "return RegExp.prototype[Symbol.match].name+RegExp.prototype[Symbol.replace].length", "return Function.prototype[Symbol.hasInstance].name+Function.prototype[Symbol.hasInstance].length",
  "return D(Function.prototype,Symbol.hasInstance)", "return Date.prototype[Symbol.toPrimitive].name+Date.prototype[Symbol.toPrimitive].length", "return String.prototype[Symbol.iterator].name", "return S([Promise.resolve.name,Promise.prototype.then.length,Promise.all.name,Promise.name,Promise.length])",
  "return S([Function.prototype.name,Function.prototype.length,typeof Function.prototype,Function.prototype()])", "return S([Function.name,Function.length,Function.prototype.constructor===Function])", "return S([Object.getPrototypeOf(function*(){}).constructor.name,Object.getPrototypeOf(async function(){}).constructor.name,Object.getPrototypeOf(async function*(){}).constructor.name])",
  "return S([Object.getPrototypeOf(function*(){}).constructor.length])", "return S([Math.max.name,Math.max.length,Math.min.length,Math.hypot.length,Math.atan2.length,Math.pow.length])", "return S([parseInt.length,parseFloat.length,isNaN.length,encodeURIComponent.length])",
  "return S([[].concat.length,[].push.length,[].splice.length,[].slice.length,[].indexOf.length,[].reduce.length,[].fill.length,[].copyWithin.length,[].at.length,[].flat.length])", "return S([''.padStart.length,''.replace.length,''.concat.length,''.slice.length,''.localeCompare.length,''.normalize.length,''.split.length])",
  "return S([Object.assign.length,Object.defineProperty.length,Object.entries.length,Object.create.length,Reflect.apply.length,Reflect.construct.length,Reflect.set.length])", "return S([JSON.stringify.length,JSON.parse.length,Date.UTC.length,Date.length,Date.prototype.setHours.length])",
  "return S([Array.from.length,Array.of.length,Array.length,Array.isArray.length,Map.length,Set.length,WeakMap.length,Proxy.length,Proxy.revocable.length,Symbol.length,Symbol.for.length,BigInt.length,BigInt.asIntN.length])",
  "return S([Number.length,String.length,String.raw.length,String.fromCharCode.length,Boolean.length,RegExp.length,Error.length,TypeError.length,AggregateError.length,Error.captureStackTrace&&Error.captureStackTrace.length])",
  "var b=function f(a,b,c){}.bind(null,1);return S([b.name,b.length])", "var b=function f(a,b,c){}.bind(null,1,2,3,4);return S([b.name,b.length])", "var b=function f(a,b,c){}.bind().bind().bind();return S([b.name,b.length])", "var f=function(){};Object.defineProperty(f,'name',{value:5});return S([f.bind().name])",
  "var f=function(){};Object.defineProperty(f,'name',{value:Symbol('q')});return S([f.bind().name])", "var f=function(){};delete f.name;return S([f.name,f.bind().name])", "var f=function(){};Object.defineProperty(f,'length',{value:'3'});return S([f.bind().length])", "var f=function(a,b){};Object.defineProperty(f,'length',{value:Infinity});return S([f.bind(null,1).length])",
  "var f=function(a,b){};Object.defineProperty(f,'length',{value:-Infinity});return S([f.bind(null,1).length])", "var f=function(a,b){};Object.defineProperty(f,'length',{value:2.7});return S([f.bind(null,1).length])", "var f=function(a,b){};Object.defineProperty(f,'length',{value:-1});return S([f.bind().length])", "var f=function(a,b){};delete f.length;return S([f.length,f.bind().length])",
  "var f=function(){};f.name='z';return f.name", "'use strict';var f=function(){};f.name='z';return f.name", "var f=function(){};Object.defineProperty(f,'name',{value:'z'});return D(f,'name')", "var f=function(){};Object.defineProperty(f,'name',{writable:true});f.name='q';return f.name", "var f=function(){};return delete f.name",
  "var f=function(){};delete f.name;f.name='q';return D(f,'name')", "var f=function(){};delete f.name;return Object.getPrototypeOf(f).name===''&&f.name===''", "var f=function(){};return Object.getOwnPropertyNames(f).join()", "var f=class{};return Object.getOwnPropertyNames(f).join()", "var f=class{static x=1;static m(){}};return Object.getOwnPropertyNames(f).join()",
  "var f=()=>1;return Object.getOwnPropertyNames(f).join()", "var f=async function(){};return Object.getOwnPropertyNames(f).join()", "var f=function*(){};return Object.getOwnPropertyNames(f).join()", "var f=({m(){}}).m;return Object.getOwnPropertyNames(f).join()", "var f=function(){}.bind();return Object.getOwnPropertyNames(f).join()",
  "function f(){}return Object.getOwnPropertyNames(f).join()", "function f(){'use strict'}return Object.getOwnPropertyNames(f).join()", "var f=function(){'use strict'};return D(f,'prototype')", "return D(function(){},'prototype')", "return D(class{},'prototype')", "return D(function*(){},'prototype')", "return D(async function(){},'prototype')",
  "var g=function*(){};return Object.getPrototypeOf(g.prototype)===Object.getPrototypeOf(function*(){}).prototype", "var s=Symbol('desc');var f={[s]:function(){}}[s];return f.name", "var s=Symbol('desc');var f={get [s](){}};return Object.getOwnPropertyDescriptor(f,s).get.name", "var f={get 'x y'(){}};return Object.getOwnPropertyDescriptor(f,'x y').get.name",
  "var f=function(){}.bind();return f.name", "var a={b:function(){}.bind()};return a.b.name", "var f=function(){};f.x=function(){};return f.x.name", "var o={};o.x=function(){};return o.x.name", "var o={};o['x']=()=>0;return o.x.name", "var o={};o[1]=()=>0;return o[1].name", "var o={};var f;f=o.q=function(){};return f.name",
  "var f;(f)=function(){};return f.name", "var f;(f=function(){});return f.name", "var f=(function(){});return f.name", "var f=(0,function(){});return f.name", "var f=function(){}||0;return f.name", "var f=0||function(){};return f.name", "var f=1&&function(){};return f.name", "var f=null??function(){};return f.name", "var f=(1,2,function(){});return f.name",
  "var f=async()=>{};return f.name", "var f=class{static m(){}};return f.name", "var f=class{static name(){}};return typeof f.name", "var f=class{static get name(){return 'g'}};return f.name", "var {f}={f:function(){}};return f.name", "var [f]=[function(){}];return f.name", "var f=[function(){}][0];return f.name", "var f=new Function;return f.name",
  "var f=new (class extends Function{});return f.name", "var f=Function.prototype;return S([f.name,f.length])", "var f=function anon(){}.bind().bind();return f.name", "var f={m(){}}.m.bind();return f.name", "var f=function(){}.call.bind(function(){});return f.name",
].forEach((b) => both(b));

// ---- 6. Annex B: funções em blocos.
const abPrefix = [
  ["", ""], ["", "var f=0;"], ["", "let f=0;"], ["", "const f=0;"], ["f", ""], ["f=3", ""], ["", "function f(){return 0}"], ["", "var f='v';"], ["...f", ""], ["{f}", ""], ["", "class f{}"], ["", "try{throw 0}catch(f){"], ["", "arguments;"],
];
const abBlocks = [
  "{function f(){return 1}}", "if(true){function f(){return 1}}", "if(false){function f(){return 1}}", "if(true){function f(){return 1}}else{function f(){return 2}}", "if(0){}else{function f(){return 2}}", "{{function f(){return 1}}}", "{function f(){return 1}function f(){return 2}}",
  "{f=5;function f(){return 1}}", "{function f(){return 1}f=3}", "{function f(){return 1}}f=4", "{var r=typeof f;function f(){}}", "switch(1){case 1:function f(){return 1}}", "switch(0){case 0:function f(){return 1}case 1:function f(){return 2}}", "L:{function f(){return 1}}",
  "for(var i=0;i<1;i++){function f(){return 1}}", "for(var i of [1]){function f(){return i}}", "while(false){function f(){}}", "{function* f(){}}", "{async function f(){}}", "{class f{}}", "{let f=1}", "{function f(){}let g}", "{let f;{function f(){}}}", "{function f(){return 1}{let f=2}}",
  "if(1)function f(){return 1}", "if(0)function f(){return 1}else function f(){return 2}", "L:function f(){return 1}", "{function f(){f=2;return 1}}", "try{function f(){return 1}}catch(e){}", "try{throw 0}catch(e){function f(){return 1}}", "try{}finally{function f(){return 1}}",
  "{function f(){return typeof f}}", "{function f(){return 1}var f=2}", "{var f=2;function f(){return 1}}", "do{function f(){return 1}}while(false)", "with({}){function f(){return 1}}",
];
const abProbe = ["return typeof f", "return typeof f==='function'?S(f()):S(f)", "var r=typeof f;return r+','+typeof f"];
for (const [pa, pb] of abPrefix) for (const b of abBlocks) for (const pr of abProbe) {
  const close = pb.startsWith("try{throw") ? "}" : "";
  both(`function g(${pa}){${pb}${b}${pr}${close}}return g(7)`);
}
[
  "return Function('{function f(){}}return typeof f')()", "return (0,eval)('{function f(){}}typeof f')", "return eval('{function f(){}}typeof f')", "return eval('typeof f;{function f(){}}')", "(0,eval)('{function qq(){}}');return typeof qq", "(0,eval)('var r=typeof qq;{function qq(){}}');return r+typeof qq",
  "(0,eval)('let qq=1;{function qq(){}}');return typeof qq", "eval('{function zz(){}}');return typeof zz", "var zz=1;eval('{function zz(){return 2}}');return typeof zz", "let zz=1;eval('{function zz(){return 2}}');return typeof zz", "{let zz=1;{eval('{function zz(){}}')}return typeof zz}",
  "function f(){return typeof zz}eval('{function zz(){}}');return f()", "function g(a=eval('{function zz(){}}')){return typeof zz}return g()", "function g(a=eval('var zz=1')){return typeof zz}return g()", "function g(a=1){eval('var zz=1');return typeof zz}return g()", "function g(a=1){eval('{function zz(){}}');return typeof zz}return g()",
  "var f=1;{function f(){}}return typeof f", "var f=1;{f=2;function f(){}}return typeof f", "var f=1;{function f(){}f=3}return f", "function f(){return 'outer'}{function f(){return 'inner'}}return f()", "function f(){return 'outer'}{function f(){return 'inner'}f=null}return typeof f",
  "var log=[];{log.push(typeof f);function f(){}log.push(typeof f)}log.push(typeof f);return log.join()", "var log=[];log.push(typeof f);{function f(){}}log.push(typeof f);return log.join()", "var log=[];if(true){log.push(typeof f);function f(){}}return log.join()+typeof f",
  "var f=function(){return 1};{function f(){return 2}}return f()", "{function f(){return 1}}var f;return typeof f", "{function f(){return 1}}function f(){return 2}return f()", "function f(){return 2}{function f(){return 1}}return f()", "{function f(){return 1}}var f=5;return f",
  "for(let i=0;i<1;i++){function f(){return 1}}return typeof f", "for(const i of [0]){function f(){return 1}}return typeof f", "for(let i=0;i<1;i++){function f(){return i}}return f()", "switch(0){case 0:let a;function f(){return 1}}return typeof f",
  "{function f(){return 1}}return f.name+f.length", "{function f(a,b){return 1}}return f.name+f.length", "var fns=[];{function f(){return 1}fns.push(f)}fns.push(f);return fns[0]===fns[1]", "var fns=[];for(var i=0;i<2;i++){function f(){return 1}fns.push(f)}return fns[0]===fns[1]", "{function f(){return 1}}return D(globalThis,'f')",
].forEach((b) => both(b));

// ---- 7. new.target.
const nt = {
  fn: "function f(){return new.target}", arrow: "function f(){return (()=>new.target)()}", evalnt: "function f(){return eval('new.target')}", deflt: "function f(a=new.target){return a}", nested: "function f(){return function(){return new.target}()}",
  store: "function f(){this.t=new.target;}", eq: "function f(){return new.target===f}", typeof: "function f(){return typeof new.target}", cls: "class f{constructor(){this.t=new.target}}", derived: "class B{constructor(){this.t=new.target}}class f extends B{}",
  derived2: "class B{constructor(){this.t=new.target}}class f extends B{constructor(){super();this.u=new.target}}", getter: "var f=function(){return new.target}", method: "var o={m(){try{return eval('new.target')}catch(e){return e.name}}};var f=function(){return o.m()}",
  fnctor: "var f=Function('return new.target')", bound: "function f0(){return new.target}var f=f0.bind(null)", gen: "function* f(){yield new.target}", async: "async function f(){return new.target}",
};
const ntCall = {
  call: "f()", new: "new f()", reflect: "Reflect.construct(f,[],G)", apply: "Reflect.apply(f,null,[])", callm: "f.call({})", newbound: "new (f.bind())()", reflect2: "Reflect.construct(f,[],f)", viaProxy: "new (new Proxy(f,{}))()", viaProxyCtor: "new (new Proxy(f,{construct(t,a,n){return Reflect.construct(t,a,G)}}))()",
};
for (const [dn, d] of Object.entries(nt)) for (const [cn, c] of Object.entries(ntCall)) both(`function G(){}${d};var r;try{r=${c}}catch(e){return e.name+': '+e.message}return r&&typeof r==='object'&&!(r instanceof Function)&&'t' in r?S([r.t===undefined,r.t===f,r.t===G,r.u===f]):typeof r==='function'?S([r===f,r===G]):S(r)`);
[
  "return Function('return new.target')()", "return new (Function('this.n=new.target;'))().n===undefined", "(0,eval)('new.target')", "eval('new.target')", "return eval('new.target')", "function f(){return eval('new.target')}return f()", "function f(){return eval('new.target')}return typeof new f()",
  "function f(){return new Function('return new.target')()}return f()", "return (()=>{try{return eval('new.target')}catch(e){return e.name+e.message}})()", "return Function('return ()=>new.target')()()", "return Function('return eval(\"new.target\")')()", "function f(){return (()=>eval('new.target'))()}return f()",
  "function f(){return Reflect.construct(function(){return new.target},[],f)===f}return f()", "var t=Reflect.construct(function(){this.nt=new.target},[],Array);return t.nt===Array&&Object.getPrototypeOf(t)===Array.prototype", "function F(){this.n=new.target}function G(){}G.prototype={g:1};var o=Reflect.construct(F,[],G);return S([o.n===G,o.g])",
  "class A{constructor(){this.n=new.target.name}}class B extends A{}return new B().n", "class A{constructor(){this.n=new.target.name}}return Reflect.construct(A,[],class Z{}).n", "class A{static create(){return new this()}constructor(){this.n=new.target.name}}class B extends A{}return B.create().n",
  "class A{constructor(){this.n=new.target.prototype.constructor.name}}class B extends A{}return new B().n", "class A{constructor(){return {nt:new.target===A}}}return new A().nt", "class A{constructor(){return {nt:new.target===B}}}class B extends A{}return new B().nt", "class A{constructor(){}}try{A()}catch(e){return e.name+': '+e.message}",
  "class A{x=new.target}return S(new A().x)", "class A{static x=new.target}return S(A.x)", "class A{static{this.y=typeof new.target}}return A.y", "class A{x=()=>new.target}return S(new A().x())", "var o={m(){return typeof eval('new.target')}};return o.m()", "var o={get g(){return eval('new.target')}};return S(o.g)",
  "function f(){return new.target}return S(f.call(new f()))", "function f(){return new.target}return S(typeof new f())", "function f(){return new.target}return new f()===f", "function f(){return new.target}return typeof f.call()", "var r=[];function f(){r.push(new.target===undefined)}f();new f();f.call({});return r.join()",
  "var r=[];function f(){if(!new.target)return new f();r.push(1);return this}f();return r.length", "function f(){if(!new.target)throw new TypeError('use new');this.a=1}try{f()}catch(e){return e.message}", "function f(){return this instanceof f}return S([f(),new f()===undefined])",
  "async function f(){return new.target}return typeof f()", "function* f(){yield new.target}return S(f().next().value)", "function* f(){yield new.target}try{new f()}catch(e){return e.name+': '+e.message}", "async function f(){}try{new f()}catch(e){return e.name+': '+e.message}", "var f=()=>1;try{new f()}catch(e){return e.name+': '+e.message}",
  "var o={m(){}};try{new o.m()}catch(e){return e.name+': '+e.message}", "var o={get g(){return 1}};try{new (Object.getOwnPropertyDescriptor(o,'g').get)}catch(e){return e.name+': '+e.message}", "try{new Math.max}catch(e){return e.name+': '+e.message}", "try{new (class A{static m(){}}).m}catch(e){return e.name+': '+e.message}",
  "try{new Symbol}catch(e){return e.name+': '+e.message}", "try{new BigInt(1)}catch(e){return e.name+': '+e.message}", "try{new parseInt}catch(e){return e.name+': '+e.message}", "try{new (()=>{})}catch(e){return e.name+': '+e.message}", "try{new 1}catch(e){return e.name+': '+e.message}", "try{new undefined}catch(e){return e.name+': '+e.message}",
  "var u;try{new u}catch(e){return e.name+': '+e.message}", "var o={};try{new o.x}catch(e){return e.name+': '+e.message}", "var o={x:1};try{new o.x}catch(e){return e.name+': '+e.message}", "try{new new Object}catch(e){return e.name+': '+e.message}", "try{new (function(){}.bind())}catch(e){return e.name}", "try{new (async function(){}).bind()}catch(e){return e.name+': '+e.message}",
].forEach((b) => both(b));
["new.target", "()=>new.target", "function f(){new.target=1}", "function f(){new.target++}", "function f(){for(new.target of []);}", "function f(){delete new.target}", "function f(){new.target?.x}", "function f(){new.target()}", "function f(){new new.target}", "function f(){new.foo}", "function f(){new . target}", "function f(){new.\\u0074arget}", "function f(){n\\u0065w.target}", "({m(){new.target}})", "class C{x=new.target}", "function f(){[new.target]=[]}", "function f(){({a:new.target}={})}", "function f(){new.target=>1}", "async function f(){await new.target}"].forEach((s) => add(`T(()=>{Function(${J(s)});return "ok"})`, `T(()=>{(0,eval)(${J(s)});return "ok"})`));

// ---- 8. Closures em laços com let.
const heads = {
  letfor: "for(let i=0;i<3;i++)", varfor: "for(var i=0;i<3;i++)", letpair: "for(let i=0,j=10;i<3;i++,j--)", constof: "for(const i of [0,1,2])", letof: "for(let i of [0,1,2])", varof: "for(var i of [0,1,2])", constin: "for(const i in {0:1,1:1,2:1})",
  letin: "for(let i in {0:1,1:1,2:1})", closeinit: "for(let i=0,g=()=>i;i<3;i++)", closetest: "for(let i=0;(fs.push(()=>i),i<3);i++)", closeupdate: "for(let i=0;i<3;fs.push(()=>i),i++)", destr: "for(let [i]of [[0],[1],[2]])", destro: "for(let {i} of [{i:0},{i:1},{i:2}])",
  whilelet: "var n=0;while(n++<3)", dowhile: "var n=0;do", forinit: "for(let i=0;i<3;i++)", labeled: "L:for(let i=0;i<3;i++)", nested: "for(let k=0;k<1;k++)for(let i=0;i<3;i++)", genof: "for(let i of (function*(){yield 0;yield 1;yield 2})())",
};
const caps = {
  arrow: "{fs.push(()=>i)}", fnexpr: "{fs.push(function(){return i})}", incr: "{fs.push(()=>i++)}", nested: "{fs.push(()=>()=>i)}", evalc: "{fs.push(()=>eval('i'))}", getter: "{fs.push(()=>({get v(){return i}}).v)}", cls: "{fs.push(()=>new class{v=i}().v)}",
  gen: "{fs.push(()=>(function*(){yield i})().next().value)}", bodymod: "{i+=0;fs.push(()=>i);if(typeof i==='number')i++}", bodylet: "{let j=i*2;fs.push(()=>[i,j])}", cont: "{if(i===1)continue;fs.push(()=>i)}", brk: "{if(i===2)break;fs.push(()=>i)}",
  block: "{{let i2=i;fs.push(()=>[i,i2])}}", assignInside: "{fs.push(()=>{i=i+10;return i})}", fnDecl: "{function h(){return i}fs.push(h)}", shadow: "{let i=5;fs.push(()=>i)}",
};
for (const [hn, h] of Object.entries(heads)) for (const [cn, c] of Object.entries(caps)) {
  const loopBody = hn === "dowhile" ? `${c}while(n++<3)` : c;
  const tail = hn === "dowhile" ? "" : "";
  const lh = hn === "dowhile" ? "var n=0;do" : h;
  const body = hn === "dowhile" ? `${c.replace(/\bi\b/g, "n")}while(n++<3)` : `${h}${c}`;
  both(`var fs=[];${hn === "dowhile" ? `var n=0;do${c.replace(/\bi\b/g, "n")}while(n++<3);` : body + ";"}var r=[];for(var f of fs){try{r.push(S(f()))}catch(e){r.push(e.name)}}for(var f of fs){try{r.push(S(f()))}catch(e){r.push(e.name)}}return r.join("|")`);
}
[
  "var fs=[];for(let i=0;i<3;i++){setTimeoutLike(()=>fs.push(i))}function setTimeoutLike(f){f()}return fs.join()", "var fs=[];for(var i=0;i<3;i++){fs.push((function(j){return ()=>j})(i))}return fs.map(f=>f()).join()", "var fs=[];for(var i=0;i<3;i++){(function(j){fs.push(()=>j)})(i)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);i++}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<5;i++){fs.push(()=>i);i++}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);fs[0]()}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i++)}return [fs[0](),fs[0](),fs[1]()].join()",
  "var fs=[];let i=0;for(i=0;i<3;i++){fs.push(()=>i)}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<2;i++){for(let j=0;j<2;j++){fs.push(()=>[i,j].join(':'))}}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<2;i++){fs.push(()=>i);for(let i=5;i<6;i++)fs.push(()=>i)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0,f=()=>i;i<2;i++){fs.push(f)}return fs.map(f=>f()).join()", "var fs=[];for(let i=0;i<2;i++){let i=7;fs.push(()=>i)}return fs.map(f=>f()).join()", "try{for(let i=0;i<2;i++){var i}}catch(e){return e.name}", "try{eval('for(let i=0;i<2;i++){var i}')}catch(e){return e.name+': '+e.message}",
  "var r=[];for(let i of [1,2]){r.push(()=>i)}for(let i of [3]){r.push(()=>i)}return r.map(f=>f()).join()", "var r=[];for(var i of [1,2]){r.push(()=>i)}return r.map(f=>f()).join()", "var r=[];for(const k in {a:1,b:2}){r.push(()=>k)}return r.map(f=>f()).join()", "var r=[];for(let i=0;i<3;i++){r.push(()=>i);if(i==1){continue}}return r.map(f=>f()).join()",
  "var r=[];L:for(let i=0;i<3;i++){for(let j=0;j<3;j++){r.push(()=>[i,j].join(''));continue L}}return r.map(f=>f()).join()", "var r=[];for(let i=0;i<3;i++){switch(i){case 1:r.push(()=>i);break;default:let q=i;r.push(()=>q)}}return r.map(f=>f()).join()", "var r=[];let i=10;for(let i=0;i<2;i++){r.push(()=>i)}r.push(()=>i);return r.map(f=>f()).join()",
  "var r=[];for(let i=0;i<2;i++){r.push(function(){return typeof i})}return r.map(f=>f()).join()", "var r=[];for(let [i,j]=[0,5];i<2;i++,j++){r.push(()=>i+j)}return r.map(f=>f()).join()", "var r=[];for(let {i}={i:0};i<2;i++){r.push(()=>i)}return r.map(f=>f()).join()", "var r=[];for(let i=0;i<2;i++){r.push(class{static v=i})}return r.map(c=>c.v).join()",
  "var r=[];for(let i=0;i<2;i++){r.push({m(){return i}})}return r.map(o=>o.m()).join()", "var r=[];for(let i=0;i<2;i++){r.push(async()=>i)}return r.length", "var r=[];for(let i=0;i<2;i++){r.push(()=>arguments.length)}return 1", "let r=[];for(let i=0;i<3;i++){r.push(()=>i);i=i}return r.map(f=>f()).join()",
  "var r=[];for(let i=0;i<3;i++){r.push(()=>i);var i2=i}return r.map(f=>f()).join()+i2", "var r=[];for(let i=0;i<3;r.push(i),i++){}return r.join()", "var r=[];for(let i=0;r.push(i)&&i<2;i++){}return r.join()", "var r=[];for(let i=0;i<2;i++){r.push(eval('()=>i'))}return r.map(f=>f()).join()",
  "var r=[];for(let i=0;i<2;i++){r.push(new Function('return typeof i'))}return r.map(f=>f()).join()", "var r=[];for(let i=0;i<2;i++){eval('var v'+i+'=i')}return typeof v0+typeof v1", "var r=[];for(let i=0;i<2;i++){eval('function e'+i+'(){return i}')}return e0()+e1()",
].forEach((b) => both(b));

// ---- 9. IIFEs, recursão mútua, tail position.
[
  "return (function(){return 1})()", "return (function(){return 1}())", "return (()=>1)()", "return !function(){return 1}()", "return +function(){return 1}()", "return void function(){return 1}()", "return new function(){this.a=1}", "return new function(){this.a=1}().a", "return S(new function(){return {b:2}})", "return S(new function(){return 3})",
  "return (function f(n){return n?n*f(n-1):1})(5)", "return (function f(){return typeof f})()", "return (function f(){f=1;return typeof f})()", "return (function f(){'use strict';try{f=1}catch(e){return e.message}})()", "return (function f(f){return f})(2)", "return (function f(){var f=3;return f})()",
  "return (function f(){function f(){return 9}return f()})()", "return (function(){return this===globalThis})()", "return (function(){'use strict';return this})()", "return (()=>this===globalThis)()", "return (function(){return (()=>this)()}).call(5) instanceof Number", "return (function(){'use strict';return (()=>this)()}).call(5)",
  "return typeof (function(){return typeof this}).call(5)", "return (function(){'use strict';return typeof this}).call(5)", "return (function(){return this}).call(null)===globalThis", "return (function(){return this}).call(undefined)===globalThis", "return (function(){'use strict';return this}).call(null)", "return typeof (function(){return this}).call('s')",
  "return (function(a,b){return a+b})(1,2)", "return (function(a,b){return a+b})(1)", "return (function(a,b){return arguments.length})(1,2,3)", "return (async function(){return 1})() instanceof Promise", "return (function*(){yield 1})().next().value", "return (function(){return function(){return 1}})()()",
  "var x=1;return (function(){var x=2;return (function(){return x})()})()", "var x=1;(function(){x=2})();return x", "var x=1;(function(x){x=2})(x);return x", "var x=1;(function(){var x=x;return typeof x})();return (function(){var x=x;return typeof x})()", "var o={v:1,f(){return (function(){return typeof this.v})()}};return o.f()",
  "var o={v:1,f(){return (()=>this.v)()}};return o.f()", "var o={v:1,f:function(){return (function(){return this===globalThis})()}};return o.f()", "var r=(function(){return arguments})(1,2);return r.length", "var r=(function(){return (()=>arguments)()})(1,2);return r.length", "return ((a,b)=>a+b)(1,2)", "return ((a,b=2)=>a+b)(1)",
  "return (({a},[b])=>a+b)({a:1},[2])", "return ((...r)=>r.length)(1,2,3)", "return (function(){return eval('1+1')})()", "return (function(){var a=1;return eval('a+1')})()", "return (function(){var a=1;return (0,eval)('typeof a')})()", "return (function(){var a=1;return (eval)('typeof a')})()", "return (function(){var a=1;var e=eval;return e('typeof a')})()",
  "return (function(){var a=1;return eval?.('typeof a')})()", "return (function(){var a=1;return window===undefined})", "var e=eval;return (function(){var a=1;return e('typeof a')})()", "return (function(){return eval('var a=1;a')+typeof a})()", "return (function(){'use strict';return eval('var a=1;a')+typeof a})()",
  "function isEven(n){return n===0?true:isOdd(n-1)}function isOdd(n){return n===0?false:isEven(n-1)}return S([isEven(10),isOdd(7),isEven(7)])", "var even=n=>n===0||odd(n-1),odd=n=>n!==0&&even(n-1);return S([even(10),odd(10)])", "var o={e(n){return n===0||this.o(n-1)},o(n){return n!==0&&this.e(n-1)}};return S([o.e(9),o.o(9)])",
  "function a(n){return n<=0?'a':b(n-1)}function b(n){return n<=0?'b':c(n-1)}function c(n){return n<=0?'c':a(n-1)}return a(7)+a(8)+a(9)", "function f(n){return n<=0?0:1+f(n-1)}return f(1000)", "function f(n){return n<=0?0:1+f(n-1)}return f(5000)", "var f=(n)=>n<=0?0:1+f(n-1);return f(2000)",
  "function f(n){return n<=0?'done':f(n-1)}return f(1000)", "function f(n){return n<=0?'done':f(n-1)}return f(100000)", "function f(n){'use strict';return n<=0?'done':f(n-1)}return f(100000)", "function f(n){'use strict';return n<=0?'done':f(n-1)}return f(1000000)",
  "function f(n,a){'use strict';return n<=0?a:f(n-1,a+1)}return f(500000,0)", "function f(n,a){'use strict';if(n<=0)return a;return f(n-1,a+1)}return f(500000,0)", "function f(n){'use strict';if(n<=0)return 'ok';return g(n-1)}function g(n){'use strict';return f(n)}return f(500000)", "function f(n){'use strict';return n<=0?'ok':(0,f)(n-1)}return f(500000)",
  "function f(n){'use strict';return n<=0?'ok':f.call(null,n-1)}return f(500000)", "function f(n){'use strict';return n<=0?'ok':f.apply(null,[n-1])}return f(500000)", "function f(n){'use strict';return n<=0?'ok':(()=>f(n-1))()}return f(100000)", "function f(n){'use strict';try{return n<=0?'ok':f(n-1)}finally{}}try{return f(500000)}catch(e){return e.name}",
  "function f(n){'use strict';try{return n<=0?'ok':f(n-1)}catch(e){}}try{return f(500000)}catch(e){return e.name}", "function f(n){'use strict';return n<=0?'ok':1&&f(n-1)}return f(500000)", "function f(n){'use strict';return n<=0?'ok':0||f(n-1)}return f(500000)", "function f(n){'use strict';return n<=0?'ok':(f(n-1),f(0))}try{return f(1000)}catch(e){return e.name}",
  "function f(n){'use strict';return n<=0?'ok':f(n-1)+''}try{return f(500000)}catch(e){return e.name+': '+e.message}", "function f(n){'use strict';return n<=0?'ok':new f(n-1)}try{return typeof f(5)}catch(e){return e.name}", "function f(n){'use strict';if(n<=0)return 'ok';return f(n-1)}return f(3000000)", "var f=n=>{'use strict';return n<=0?'ok':f(n-1)};return f(500000)",
  "var o={f(n){'use strict';return n<=0?'ok':this.f(n-1)}};return o.f(500000)", "class C{static f(n){return n<=0?'ok':C.f(n-1)}}return C.f(500000)", "function* g(n){if(n>0)yield* g(n-1);yield n}return [...g(5)].join()", "function f(){return f()}try{f()}catch(e){return e.name+': '+e.message}", "function f(){'use strict';return f()}return typeof f", "var f=()=>f();try{f()}catch(e){return e instanceof RangeError}",
  "function f(){f()}try{f()}catch(e){return e.constructor===RangeError&&e.message}", "function f(){return [f()]}try{f()}catch(e){return e.message}", "function f(n){return n?f(n-1)+1:0}try{return f(1e6)}catch(e){return e.name}", "var d=0;function f(){d++;f()}try{f()}catch(e){}return d>1000",
].forEach((b) => both(b));

// ---- 10. Function.prototype.toString de formas variadas.
const tsForms = [
  "function f(){}", "function f( a , b ){ return a+b }", "function /*a*/ f /*b*/ ( /*c*/ a /*d*/ ) /*e*/ { /*f*/ }", "function f(a,\n  b)\n{\n  return 1;\n}", "function*   g  ( ) { }", "async   function  h ( ) { }", "async function* i(){}", "(a,b)=>a+b", "a=>a", "async a=>a", "async (a)=>{}", "( a ) => { return a }",
  "()=>({})", "class A{}", "class A extends Object{ constructor(){ super() } }", "class A { static x = 1; m(){} get g(){return 1} set g(v){} static async *s(){} }", "class   A  {  }", "(class{})", "(class extends Object{})", "function f(a=')',b=\"(\"){}", "function f(a=`)`){return `x${a}`}", "function f(){return /)/g}", "function f(){ // c\n}",
  "function f(){/* ) */}", "function \\u0061(){}", "function f(\\u0061){}", "function f(a,b,){}", "function f(...r){}", "function f({a,b},[c]){}", "function f(a=function(){}){}", "function f(){return function(){}}", "function f(){ 'use strict' }", "function f(){;}", "function f(){}\n", "function f(){} ;", "function  f ( ) { } /* trailing */",
  "var f=function(){}", "var f=function  named  (){}", "var f=()=>{}", "var f=async()=>{}", "var f=class{}", "var o={m(){}}.m", "var o={ m ( a ) { } }.m", "var o={ get g ( ) { return 1 } }", "var o={ async m(){} }.m", "var o={ *m(){} }.m", "var o={ async *m(){} }.m", "var o={ ['a'+'b'](){} }.ab", "var o={ 'q r'(){} }['q r']", "var o={ 1(){} }[1]", "var o={m:function(){}}.m",
  "var o={m:()=>{}}.m", "var o={ [Symbol.iterator](){} }[Symbol.iterator]", "var o={ get [Symbol.iterator](){return 1} }", "class A{ static m ( ) { } }.m", "class A{ m ( ) { } }.prototype.m", "class A{ constructor ( ) { } }", "class A{ #p(){} static g(a){return a.#p} }.g", "class A{ static { } }", "class A{ x = 1 }", "class A{ 'x' = 1; static y }",
  "function f(){ return 'é' }", "function f(){ return '\\n' }", "function f(){ return \"\\u{1F600}\" }", "function f(){\r\n}", "function f(){\u2028}", "function f(a /* , */ , b){}", "function f(a // c\n,b){}", "function f(){ return a\n+b }", "function f(){ if(1){function g(){}} }", "(function(){})", "(function(){}).bind()", "(function(){}).bind().bind()",
  "async function f(){ await 1 }", "async function f(){ for await(x of y); }", "function* f(){ yield* g }", "()=>{}", "x=>x", "(x,y)=>(x,y)", "async x=>await x", "async()=>{}", "function f(a, a){}", "function f(eval){}", "function eval(){}", "function yield(){}", "function await(){}", "function async(){}", "function get(){}", "function static(){}",
];
for (const s of tsForms) {
  const val = /^(var|class\s+A\s*\{.*\}\.|class A\{.*\}\.|\w)/.test(s) && /^var /.test(s) ? s.replace(/^var f=/, "f=") : s;
  const prog = /^var /.test(s) ? `${s.replace(/^var /, "var ")};` : null;
  const mk = (expr, extra) => `var __v=(${expr});return ${extra}`;
  const expr = /^var /.test(s) ? null : s;
  if (/^var /.test(s)) {
    const m = s.match(/^var (\w+)=(.*)$/s);
    const pick = m[1] === "o" ? m[2] : m[2];
    both(`var f=${pick.replace(/\.(\w+|\[.*\])$/s, "")};return S(String(${m[2].includes("}.") || m[2].includes("}[") ? m[2] : "f"}))`);
  } else if (/\}\.\w+$/.test(s) || /\}\.prototype\.\w+$/.test(s) || /\}\.g$/.test(s)) {
    both(`var f=(${s});return S(f.toString())`);
  } else {
    both(`var f=(${s});return typeof f==='function'?S(f.toString())+"|"+S(String(f))+"|"+S(Function.prototype.toString.call(f))+"|"+S(\`\${f}\`)+"|"+S(f.toString===Function.prototype.toString):S(f)`);
    if (/^(function|async function|class)\s/.test(s) || /^\(/.test(s) || /=>/.test(s)) both(`return S(Function.prototype.toString.call((${s}).bind()))+S(Function.prototype.toString.call(new Proxy((${s}),{})))`);
  }
}
[
  "return Function.prototype.toString.call(Math.max)", "return Function.prototype.toString.call(Symbol)", "return Function.prototype.toString.call(class{static m(){}}.m)", "return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Map.prototype,'size').get)", "return Function.prototype.toString.call(Array.prototype[Symbol.iterator])",
  "return Function.prototype.toString.call(Function.prototype)", "return Function.prototype.toString.call(Function)", "return Function.prototype.toString.call(function(){}.bind())", "return Function.prototype.toString.call(Object.getPrototypeOf(function*(){}).constructor)", "return Function.prototype.toString.call(Proxy)",
  "return Function.prototype.toString.call(new Proxy(class{},{}))", "return Function.prototype.toString.call(new Proxy(function f(){},{}))", "return Function.prototype.toString.call(Reflect.get)", "return Function.prototype.toString.call(Symbol.prototype[Symbol.toPrimitive])", "return Function.prototype.toString.call(RegExp.prototype[Symbol.match])",
  "return Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Object.prototype,'__proto__').set)", "return Function.prototype.toString.call(parseInt)", "return Function.prototype.toString.call(eval)", "return Function.prototype.toString.call(Promise.resolve)", "return Function.prototype.toString.call(Date.prototype[Symbol.toPrimitive])",
  "return Function.prototype.toString.call({})", "return Function.prototype.toString.call(1)", "return Function.prototype.toString.call(null)", "return Function.prototype.toString.call(undefined)", "return Function.prototype.toString.call('f')", "return Function.prototype.toString.call(Symbol())", "return Function.prototype.toString.call([])", "return Function.prototype.toString.call(/x/)",
  "return Function.prototype.toString.call(new Proxy({},{}))", "return Function.prototype.toString.call(Object.create(Function.prototype))", "return Function.prototype.toString.call(class{}.prototype)", "return Function.prototype.toString.call()", "return Function.prototype.toString.length+Function.prototype.toString.name",
  "return String(Math.max)+String(Math.max.bind())+String(function(){}.bind(null,1))", "return (function(){}).toString.call(()=>1)", "var f=function(){};f.toString=()=>'x';return String(f)+Function.prototype.toString.call(f)", "var f=function(){};f.toString=null;return `${f}`.length>0", "var f=function(){};Object.setPrototypeOf(f,null);return typeof Function.prototype.toString.call(f)",
  "var f=function(){};Object.defineProperty(f,'name',{value:'zz'});return String(f)", "var f=function g(){};f.name;return String(f)", "var f=function(){return 1};return String(f)===String(f)&&f.toString()===f+''", "var f=eval('(function  (  ) {  })');return String(f)", "var f=eval('(function(){})\\n//c');return String(f)", "var f=eval('/*a*/function f(){}/*b*/');return String(f)",
  "var f=eval('(/*a*/ function /*b*/ ( ) /*c*/ { } /*d*/)');return String(f)", "var f=eval('/*x*/ (a) /*y*/ => /*z*/ a /*w*/');return String(f)", "var f=eval('({ /*a*/ m /*b*/ ( ) /*c*/ { } /*d*/ }).m');return String(f)", "var f=eval('({ /*a*/ get /*b*/ g /*c*/ ( ) /*d*/ { } /*e*/ })');return String(Object.getOwnPropertyDescriptor(f,'g').get)",
  "var f=eval('class /*a*/ A /*b*/ { /*c*/ static /*d*/ m /*e*/ ( ) /*f*/ { } /*g*/ }');return String(f)+'|'+String(f.m)", "var f=eval('class A{ /*a*/ constructor /*b*/ ( ) /*c*/ { } }');return String(f)", "var f=eval('class A extends /*a*/ Object /*b*/ { }');return String(f)", "var f=eval('(async /*a*/ function /*b*/ * /*c*/ g ( ) { })');return String(f)",
  "var f=eval('(async /*a*/ ( ) /*b*/ => /*c*/ 1)');return String(f)", "var f=eval('function f(){}\\n;');return String(f)", "var f=eval('var q=function(){}\\n,w=1;q');return String(f)", "var f=(0,eval)('(function(){ \"use strict\" })');return String(f)", "var f=new Function('a','b','return a+b');return String(f)", "var f=new Function('a,b','return a+b');return f.toString()",
  "var f=new Function();return f.toString()", "var f=new Function('');return f.toString()", "var f=new Function('a','');return f.toString()", "var f=new Function('','a','');return f.toString()", "var f=new Function('/*c*/','');return f.toString()", "var f=new Function('a //c','');return f.toString()", "var f=new Function('a','//c');return f.toString()", "var f=new Function('a=1','b=2','return a+b');return f.toString()+f()",
  "var f=new Function('...a','return a.length');return f.toString()+f(1,2)", "var G=Object.getPrototypeOf(function*(){}).constructor;return new G('a','yield a').toString()", "var A=Object.getPrototypeOf(async function(){}).constructor;return new A('a','return a').toString()", "var AG=Object.getPrototypeOf(async function*(){}).constructor;return new AG('a','yield a').toString()",
  "return S([Function('return 1').name,new Function().name,Function('a','return 1').length,Function('a,b','c','').length,Function('a=1','').length,Function('...a','').length,Function('{a}','').length,Function('a','b=1','c','').length])", "return Function.prototype.toString.call(function(){}.constructor)", "return Function.prototype.toString.call(class A{}.constructor)",
  "return Function('return typeof this')()", "return Function('\"use strict\";return typeof this')()", "return Function('return this===globalThis')()", "return Function('return arguments.length')(1,2)", "return Function('a','a','return a')(1,2)", "return Function('a','a','\"use strict\";return a')(1,2)", "return Function('var a=1;return typeof a')()", "return typeof Function('return typeof x')",
  "var x='g';return Function('return typeof x')()", "var x='g';(0,eval)('var x2=1');return Function('return typeof x2')()", "return Function('return Function.prototype.toString.call(arguments.callee)')()", "return Function('return arguments.callee.name')()", "return Function('return new.target')()", "return Function('return super.x')", "return Function('super()')",
].forEach((b) => both(b));

// ---- 11. Construtor Function com parâmetros e corpos estranhos.
const fparams = ["", "a", "a,b", "a, b", "a=1", "...a", "{a}", "[a]", "a /*c*/", "a //c", "/*x*/", "a\n", "a,", "a,a", "eval", "arguments", "a=this", "a=arguments", "yield", "await", "let", "static", "a b", "a;b", "1", "'a'", "a=", "a,,b", "...a,b", "(a)", "a)", "{a},{a}", "a=(", "\\u0061", "a\\u0020", "\u00e9", "a=1,a", "async", "get", "enum", "super", "new.target"];
const fbodies = ["", "return 1", "//c", "/*", "}{", "return a", "'use strict';return this", "return arguments.length", "return new.target", "let a;", "return typeof a", "var a;return 1", "}); (function(){", "super()", "yield 1", "await 1", "-->", "'use strict';var a=010", "return a=>a", "a:a:;", "return `${1}`", "<!--", "\nreturn 1\n", "return\n1", "'use strict'", "'use strict';with(a){}", "eval=1", "'use strict';eval=1", "var arguments", "break", "return 1;}", "function(){}", "return function(){}", "return this"];
for (const p of fparams) for (const b of fbodies) {
  add(`T(()=>{var f=Function(${J(p)},${J(b)});var r;try{r=S(f())}catch(e){r="call "+e.name}return S([f.toString(),f.length,f.name])+r})`);
}
for (const p of fparams) add(`T(()=>{var f=Function(${J(p)},"return 1");return S([f.toString(),f.length])})`, `T(()=>{var f=Function(${J(p)},"'use strict';return 1");return S([f.toString(),f.length])})`);
const GENS = [["Gen", "Object.getPrototypeOf(function*(){}).constructor"], ["Async", "Object.getPrototypeOf(async function(){}).constructor"], ["AsyncGen", "Object.getPrototypeOf(async function*(){}).constructor"]];
for (const [gn, g] of GENS) for (const p of ["", "a", "a,b", "a=1", "...a", "yield", "await", "a=yield", "a=await 1", "{a}", "a,a"]) for (const b of ["", "return 1", "yield 1", "await 1", "yield*[]", "var yield", "var await", "for await(a of [])0", "return arguments.length", "}{"]) {
  add(`T(()=>{var f=${g}(${J(p)},${J(b)});return S([f.toString(),f.length,f.name,Object.getPrototypeOf(f)===${g}.prototype,typeof f.prototype])})`);
}
add(
  "T(()=>S([Function('a','b','return a+b')(1,2),Function('a,b','return a+b')(1,2),Function('a','b,c','return a+b+c')(1,2,3),Function('a=1','b=a+1','return b')(),Function('a','return typeof a')()]))",
  "T(()=>Function('a','b','c','return [a,b,c].join()')(1,2,3))", "T(()=>new Function('a','b','return a*b')(3,4))", "T(()=>Function.call(null,'a','return a')(7))", "T(()=>Function.apply(null,['a','return a'])(8))", "T(()=>Reflect.construct(Function,['return 1'])())", "T(()=>Reflect.construct(Function,['return 1'],Array) instanceof Array)",
  "T(()=>Function({toString(){return 'a'}},{toString(){return 'return a'}})(5))", "T(()=>Function(1,2))", "T(()=>Function(null,'return 1').length)", "T(()=>Function(undefined,'return 1').length)", "T(()=>Function(Symbol(),'')", "T(()=>Function('return 1',undefined))", "T(()=>Function(['a','b'],'return a+b')(1,2))",
  "T(()=>Function('a,b','c','return a+b+c').length)", "T(()=>Function('a','b')())", "T(()=>Function('...a','b','')())", "T(()=>Function('a','...b','return b.length')(1,2,3))", "T(()=>Function('a=1','a','')())", "T(()=>Function('a','a=1','')())", "T(()=>Function(`a\nb`,'return 1'))", "T(()=>Function('a','return 1\n//'))", "T(()=>Function('a','return 1\n//').toString())",
  "T(()=>Function('/*','*/){')())", "T(()=>Function('a){','}')())", "T(()=>Function('','}), (function(){')())", "T(()=>Function('a','})')())", "T(()=>Function('){return 1}//','')())", "T(()=>Function('a','return 1}//').toString())", "T(()=>Function.prototype.constructor===Function)", "T(()=>Object.getPrototypeOf(Function('')) === Function.prototype)",
  "T(()=>Function('').prototype.constructor.name)", "T(()=>Object.getOwnPropertyNames(Function('')).join())", "T(()=>typeof Function('')())", "T(()=>Function('return this')()===globalThis)", "T(()=>Function('return function(){return this}()')()===globalThis)", "T(()=>{var o={};return Function('return this').call(o)===globalThis})",
  "T(()=>{var x='local';return Function('return typeof x')()})", "T(()=>{globalThis.gx=1;return Function('return gx')()})", "T(()=>{Function('globalThis.gz=5')();return gz})", "T(()=>{Function('var gq=5')();return typeof gq})", "T(()=>{Function('gw=5')();return gw})", "T(()=>{'use strict';Function('gw2=5')();return gw2})", "T(()=>{'use strict';return Function('return this')()===globalThis})",
);

// ---- 12. Mensagens exatas de ReferenceError/TypeError/SyntaxError via Function/eval.
const snippets = [
  "undefinedVar", "undefinedVar=1", "typeof undefinedVar", "undefinedVar++", "delete undefinedVar", "undefinedVar()", "undefinedVar.x", "var o;o.x", "var o=null;o.x", "var o;o.x=1", "var o=null;o.x=1", "var o;o[0]", "var o;o[0]=1", "var o;o()", "var o;o.m()", "var o={};o.m()", "var o={};o.a.b", "var o={};o.a.b=1", "var o={};o.a.b()",
  "var o={};o.a.b.c()", "var o={a:{}};o.a.b()", "var o={a:{}};o.a.b.c", "var o={a:1};o.a()", "var o={a:'s'};o.a()", "var o={a:{}};o.a()", "(void 0)()", "null()", "(1)()", "'s'()", "({})()", "[]()", "(()=>1)()()", "new 1", "new ({})", "new null", "new undefined", "new (()=>1)", "new (function(){}.bind())", "new Math.max", "new (class{static m(){}}).m",
  "null.x", "undefined[0]", "null[Symbol.iterator]", "var {a}=null", "var {a}=undefined", "var [a]=null", "var [a]=undefined", "var [a]={}", "var [a]=1", "var {a:{b}}={}", "var {a:[b]}={}", "(({a})=>a)()", "(([a])=>a)()", "(({a})=>a)(null)", "for(var a of 1);", "for(var a of null);", "for(var a of {});", "[...1]", "[...null]", "[...{}]", "Math.max(...1)", "Math.max(...null)", "new Array(...{})",
  "let x=x", "const c=1;c=2", "const c=1;c++", "const c=1;[c]=[2]", "const c=1;({c}={c:2})", "const c=1;for(c of [1]);", "const c=1;c+=1", "const c=1;delete c", "let x;{x;let x}", "x;let x", "typeof x;let x", "class A extends A{}", "class A{static x=A.y.z}", "var A=class B extends B{}", "function f(a=b,b){}f()", "function f(a=a){}f()", "function f(a=()=>b,b){}f()()", "(function(a=b,b){})()", "((a=b,b)=>1)()",
  "class A extends null{constructor(){super()}};new A", "class A extends null{};new A", "class A{constructor(){this.x}};A()", "class A{};A()", "class A extends Object{constructor(){this.x}};new A", "class A extends Object{constructor(){}};new A", "class A extends Object{constructor(){super();super()}};new A", "class A extends Object{constructor(){return 1}};new A", "class A{constructor(){return 1}};typeof new A", "class A extends Object{constructor(){return undefined}};new A", "class A extends 1{}", "class A extends (()=>1){}", "class A extends function(){}.bind(){}", "function F(){};F.prototype=1;class A extends F{}", "function F(){};F.prototype=null;class A extends F{};typeof new A", "class A extends Math.max{}",
  "class A{#p;static t(o){return o.#p}};A.t({})", "class A{#p;static t(o){o.#p=1}};A.t({})", "class A{#p;static t(o){return #p in o}};A.t(1)", "class A{#m(){};static t(o){o.#m()}};A.t({})", "class A{#m(){};static t(o){o.#m=1}};A.t(new A)", "class A{get #g(){return 1};static t(o){o.#g=1}};A.t(new A)", "class A{set #s(v){};static t(o){return o.#s}};A.t(new A)", "class A{#p;constructor(){return o}};var o={};new A;new A", "class B{constructor(o){return o}}class A extends B{#p=1}var o={};new A(o);new A(o)", "class A{static #p;static t(o){return o.#p}};class B extends A{};B.t(B)", "class A{#p=1;static t(o){return o.#p}};A.t(Object.create(new A))",
  "1 instanceof 1", "({}) instanceof ({})", "({}) instanceof (()=>1)", "({}) instanceof Math.max", "({}) instanceof null", "({}) instanceof {[Symbol.hasInstance]:1}", "function F(){};F.prototype=1;({}) instanceof F", "({}) instanceof (function(){}.bind())", "'a' in 1", "'a' in 's'", "'a' in null", "1 in {}", "Symbol() in 1", "#a in {}", "({}) in ({}) in 1",
  "Symbol()+''", "Symbol()+1", "`${Symbol()}`", "+Symbol()", "Symbol()*2", "String(Symbol())+''", "new Symbol", "Symbol.keyFor(1)", "Symbol.prototype.toString.call(1)", "Symbol().description=1", "BigInt(1.5)", "BigInt('x')", "BigInt(undefined)", "BigInt(null)", "BigInt(Symbol())", "1n+1", "1n*1.5", "+1n", "1n>>>0n", "1n/0n", "1n**-1n", "BigInt.asUintN(-1,1n)", "Math.max(1n)", "1n<1", "Number(1n)+1n", "JSON.stringify(1n)", "BigInt(1e400)", "BigInt(NaN)", "0n.toString(1)", "(1n).toString(37)",
  "[].reduce((a,b)=>a)", "[].reduceRight((a,b)=>a)", "[1].reduce(1)", "[1].map(1)", "[1].forEach()", "[1].filter(null)", "[1].find({})", "[1].sort(1)", "[].at.call(null)", "Array.prototype.map.call(null,x=>x)", "Array.from(1,2)", "new Array(-1)", "new Array(1.5)", "new Array(2**32)", "[].length=-1", "[].length=2**32", "Array(2**32-1).push(1)", "[].concat.call(null)", "[].flat(Infinity,1)", "Array.prototype.join.call({length:2**32})", "[].with(0,1)", "[1].with(5,1)", "[1].toSpliced(0,1,...{})", "[].toSorted(1)",
  "'x'.repeat(-1)", "'x'.repeat(Infinity)", "'x'.repeat(2**31)", "'x'.padStart(2**31)", "''.normalize('x')", "String.prototype.trim.call(null)", "String.prototype.at.call(undefined)", "String.fromCodePoint(-1)", "String.fromCodePoint(1.5)", "String.fromCodePoint(0x110000)", "'a'.localeCompare('b','xx-invalid-')", "'a'.replaceAll(/a/,'b')", "'a'.matchAll(/a/)", "'a'.startsWith(/a/)", "'a'.includes(/a/)", "'a'.endsWith(/a/)", "String.raw()", "'a'.toWellFormed.call(null)", "new String(Symbol())", "'abc'.substring(Symbol())",
  "JSON.parse('{')", "JSON.parse('')", "JSON.parse('[1,]')", "JSON.parse(\"{'a':1}\")", "JSON.parse('{\"a\":1,}')", "JSON.parse('undefined')", "JSON.parse('01')", "JSON.parse('1 2')", "JSON.parse('\"\\n\"')", "JSON.parse('nul')", "JSON.parse('[')", "JSON.parse('{\"a\"}')", "JSON.parse('{\"a\":}')", "JSON.parse('\\u0000')", "JSON.stringify(Symbol())+JSON.stringify({a:1n})", "var a={};a.a=a;JSON.stringify(a)", "var a=[];a[0]=a;JSON.stringify(a)", "JSON.stringify({toJSON(){throw new RangeError('t')}})",
  "decodeURI('%')", "decodeURIComponent('%E0%A4%A')", "decodeURI('%FF')", "encodeURI('\\uD800')", "encodeURIComponent('\\uDC00')", "escape()+unescape('%u')", "atob('*')", "new RegExp('[')", "new RegExp('(')", "new RegExp('a','gg')", "new RegExp('a','x')", "new RegExp('\\\\')", "new RegExp('(?<a>x)(?<a>y)')", "new RegExp('\\\\k<a>','u')", "new RegExp('{1}','u')", "new RegExp('a{2,1}')", "new RegExp('(?<=a)+')", "/a/.exec.call({})", "RegExp.prototype.test.call(1)", "RegExp.prototype.flags", "Object.getOwnPropertyDescriptor(RegExp.prototype,'global').get.call({})", "/a/[Symbol.replace].call(1)", "/(/",
  "Object.defineProperty(1,'a',{})", "Object.defineProperty({},'a',1)", "Object.defineProperty({},'a',{get:1})", "Object.defineProperty({},'a',{get(){},value:1})", "Object.defineProperty(Object.freeze({}),'a',{value:1})", "Object.defineProperty(Object.freeze({a:1}),'a',{value:2})", "Object.setPrototypeOf(Object.freeze({}),{})", "Object.setPrototypeOf({},1)", "Object.setPrototypeOf(null,{})", "var a={};Object.setPrototypeOf(a,Object.create(a))", "Object.setPrototypeOf(Object.prototype,{})", "Object.create(1)", "Object.create(undefined)", "Object.assign(null)", "Object.keys(null)", "Object.entries(undefined)", "Object.fromEntries(1)", "Object.fromEntries([1])", "Object.fromEntries([[]])+Object.fromEntries([1])", "Object.groupBy(1,x=>x)", "Object.groupBy([],1)", "Object.getPrototypeOf(null)", "Object.getOwnPropertyNames(undefined)", "Object.freeze(Object.freeze([1])).push(1)", "'use strict';Object.freeze({a:1}).a=2", "'use strict';Object.freeze([1])[0]=2", "'use strict';Object.freeze([1]).length=0", "'use strict';Object.preventExtensions({}).a=1", "'use strict';Object.seal({a:1}).b=1", "'use strict';delete Object.freeze({a:1}).a", "'use strict';delete Object.seal({a:1}).a", "'use strict';delete [].length", "'use strict';({get a(){return 1}}).a=2", "'use strict';var o={};Object.defineProperty(o,'a',{value:1});o.a=2", "'use strict';'s'.x=1", "'use strict';'s'[0]=1", "'use strict';'s'.length=1", "'use strict';(1).x=1", "'use strict';Symbol().x=1", "'use strict';true.x=1", "'use strict';NaN=1", "'use strict';undefined=1", "'use strict';Infinity=1", "'use strict';Math.PI=1", "'use strict';delete Math.PI", "'use strict';(function(){}).name='x'", "'use strict';(function(){}).length=1", "'use strict';(function(){}).caller", "'use strict';(function(){}).arguments", "'use strict';(function(){}).caller=1", "(function(){'use strict';return arguments.callee})()", "(function(){'use strict';return arguments.caller})()", "(function(){'use strict';arguments.callee=1})()", "(()=>{'use strict';return (function f(){return f.caller})()})()",
  "Reflect.construct(1)", "Reflect.construct(function(){},1)", "Reflect.construct(function(){},[],1)", "Reflect.construct(()=>1,[])", "Reflect.apply(1)", "Reflect.apply(function(){},null,1)", "Reflect.ownKeys(1)", "Reflect.get(1,'a')", "Reflect.defineProperty(1,'a',{})", "Reflect.getPrototypeOf(1)", "Reflect.setPrototypeOf({},1)", "new Proxy(1,{})", "new Proxy({},1)", "Proxy({},{})", "new Proxy(new Proxy({},{}),{}).x", "var r=Proxy.revocable({},{});r.revoke();r.proxy.x", "var r=Proxy.revocable({},{});r.revoke();new Proxy(r.proxy,{})", "new Proxy({},{get:1}).x", "new Proxy({a:1},{get(){return 2}}).a", "var o={};Object.defineProperty(o,'a',{value:1});new Proxy(o,{get(){return 2}}).a", "new Proxy({},{ownKeys(){return [1]}}) && Object.keys(new Proxy({},{ownKeys(){return [1]}}))", "Object.keys(new Proxy({},{ownKeys(){return ['a','a']}}))", "Object.keys(new Proxy(Object.freeze({a:1}),{ownKeys(){return []}}))", "Object.getPrototypeOf(new Proxy({},{getPrototypeOf(){return 1}}))", "Object.getPrototypeOf(new Proxy(Object.preventExtensions({}),{getPrototypeOf(){return null}}))", "new Proxy({},{has(){throw new TypeError('h')}}) && ('a' in new Proxy({},{has(){throw new TypeError('h')}}))", "Object.isExtensible(new Proxy({},{isExtensible(){return false}}))", "Object.preventExtensions(new Proxy({},{preventExtensions(){return false}}))", "Object.defineProperty(new Proxy({},{defineProperty(){return false}}),'a',{})", "delete new Proxy({},{deleteProperty(){return false}}).a", "'use strict';delete new Proxy({},{deleteProperty(){return false}}).a", "'use strict';new Proxy({},{set(){return false}}).a=1", "new Proxy(function(){},{apply:1})()", "new Proxy(function(){},{construct(){return 1}})",
  "new Map(1)", "new Map([1])", "new Map([[]]).size+new Map(1)", "new Set(1)", "new WeakMap().set(1,1)", "new WeakSet().add(1)", "new WeakMap().set(Symbol.for('a'),1)", "new WeakRef(1)", "new FinalizationRegistry(1)", "Map()", "Set()", "Map.prototype.get.call({})", "Set.prototype.add.call(new Map,1)", "new Map().forEach(1)", "new Set().forEach()", "Map.groupBy(1,x=>x)", "new Map([[1,2]]).get.call(null)", "Map.prototype.size", "Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call({})",
  "new Promise()", "new Promise(1)", "Promise()", "Promise.resolve.call(1)", "Promise.all.call(1,[])", "Promise.prototype.then.call(1)", "new Promise(()=>{}).then.call({})", "Promise.race.call(function(){},[])", "Promise.withResolvers.call(1)", "Promise.try.call(1,()=>1)", "new Promise(function(){}).constructor.resolve()", "Promise.allSettled.call(undefined,[])", "Promise.any.call(1,[])",
  "new ArrayBuffer(-1)", "new ArrayBuffer(2**53)", "new ArrayBuffer(1,{maxByteLength:0})", "ArrayBuffer(1)", "new Uint8Array(-1)", "new Uint8Array(2**53)", "new Uint8Array(new ArrayBuffer(3),1,3)", "new Uint16Array(new ArrayBuffer(3))", "new Uint16Array(new ArrayBuffer(4),1)", "new Uint8Array(new ArrayBuffer(4),5)", "new Uint8Array({length:-1})", "new Uint8Array(1.5)", "new Uint8Array(1n)", "new BigInt64Array(1).fill(1)", "new Uint8Array(1).fill(1n)", "Uint8Array(1)", "Uint8Array.from(1)", "Uint8Array.of.call(1)", "new Uint8Array(1).set([1,2])", "new Uint8Array(1).set([1],-1)", "new Uint8Array(1).subarray.call({})", "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),'length').get.call({})", "new DataView(1)", "new DataView(new ArrayBuffer(1),2)", "new DataView(new ArrayBuffer(1)).getUint16(0)", "new DataView(new ArrayBuffer(1)).setInt8(-1,0)", "new DataView(new ArrayBuffer(1)).getBigInt64(0)", "DataView(new ArrayBuffer(1))", "new SharedArrayBuffer(-1)", "Atomics.add(new Float64Array(1),0,1)", "Atomics.wait(new Int32Array(1),0,0)", "Atomics.notify(new Float32Array(1),0)", "Atomics.load(1,0)",
  "new Date(Symbol())", "new Date(1n)", "new Date(NaN).toISOString()", "new Date(8.64e15+1).toISOString()", "Date.prototype.getTime.call({})", "Date.prototype.toISOString.call(1)", "Date.prototype[Symbol.toPrimitive].call(1)", "new Date()[Symbol.toPrimitive]('x')", "new Date()[Symbol.toPrimitive]()", "new Date().toJSON.call({toISOString:1})", "Date.prototype.toString.call(1)", "Date()+1", "Date.UTC(Symbol())", "new Intl.DateTimeFormat('xx-invalid-')", "(1).toFixed(101)", "(1).toFixed(-1)", "(1).toPrecision(0)", "(1).toPrecision(101)", "(1).toExponential(-1)", "(1).toString(1)", "(1).toString(37)", "Number.prototype.toString.call('1')", "Number.prototype.valueOf.call('1')", "Number.prototype.toFixed.call({})", "Number('1n')+Number(Symbol())", "parseInt(Symbol())", "parseFloat(1n)+parseInt(Symbol())", "isNaN(Symbol())", "isNaN(1n)", "Math.abs(Symbol())", "Math.round(1n)", "Math.max({valueOf(){throw new EvalError('v')}})", "Math.sqrt(1n)", "Number.isInteger.call()", "Number.parseFloat === parseFloat",
  "function*g(){}g().next.call({})", "function*g(){yield g().next()}var i=g();i.next()", "function*g(){var i=yield;}var i=g();i.next();i.return(1);i.next.call(1)", "var i=(function*(){i.next()})();i.next()", "(function*(){yield 1})().throw(new RangeError('t'))", "(function*(){})().return.call(1)", "new (function*(){})", "async function*g(){}g().next.call(1)", "(async function*(){}).prototype.next", "[][Symbol.iterator]().next.call({})", "new Map()[Symbol.iterator]().next.call({})", "''[Symbol.iterator]().next.call({})", "[][Symbol.iterator].call(null)", "Array.prototype[Symbol.iterator].call(undefined)", "var it={[Symbol.iterator](){return 1}};[...it]", "var it={[Symbol.iterator](){return {}}};[...it]", "var it={[Symbol.iterator](){return {next(){return 1}}}};[...it]", "var it={[Symbol.iterator](){return {next(){return {done:false}},return(){return 1}}}};for(var a of it)break", "var it={[Symbol.iterator](){return {next(){return {done:false}},return:1}}};for(var a of it)break", "var it={[Symbol.iterator]:1};[...it]", "var it={[Symbol.iterator]:null};[...it]", "for(var a of {[Symbol.iterator]:()=>({})});", "var [a]={[Symbol.iterator](){return {}}}", "var [a]={[Symbol.iterator]:2}", "Array.from({[Symbol.iterator]:1})", "new Map({[Symbol.iterator]:1})", "Promise.all({[Symbol.iterator]:1})", "Object.fromEntries({[Symbol.iterator]:1})", "new Set({[Symbol.iterator](){return {next:1}}})", "Array.from({length:1,[Symbol.iterator]:undefined})", "yield*[]",
  "a b", "var 1", "var a=", "var a b", "let let", "let [", "let {", "let a=", "const a", "const a;", "const [a]", "const {a}", "var [a]", "var {a}", "for(const a;;);", "for(let a,b of []);", "for(let a=1 of []);", "for(var a=1 of []);", "for(var a=1 in {});", "'use strict';for(var a=1 in {});", "for(let a=1 in {});", "for(let of []);", "for(let.x of []);", "for(async of []);", "for(async of=>1;;)break", "for(let in {});", "for(let=0;;)break", "for((a)of[]);", "for(a()of[]);", "for(a()in{});", "for(1 of []);", "for(a+b of []);", "for((a,b)of []);", "for(;;", "for(;;;)", "for(a in b of c);", "for await(a of b);", "async function f(){for await(a in b);}", "async function f(){for await(;;);}", "async function f(){for await(a of b,c);}", "function f(){for await(a of b);}",
  "function(){}", "function f(", "function f(a", "function f(a,", "function f(a,)", "function f(,a){}", "function f(a,,b){}", "function f(...a,){}", "function f(...a=1){}", "function f(...[a]=[]){}", "function f(...a,b){}", "(...a,)=>1", "(a,...b,)=>1", "(...a=1)=>1", "(,)=>1", "()=>", "()", "(a,b)", "(a,b)=", "(a,b)=>", "a=>{", "a=>{}()", "a=>{}.x", "a=>{}+1", "(a=>1)()", "a=>1()", "a\n=>1", "()\n=>1", "(a)\n=>1", "async a\n=>1", "async\na=>1", "async (a)\n=>1", "async\n(a)=>1", "async(a)\n", "async(...a,)", "async(a,...b,)=>1", "async(await)=>1", "async await=>1", "async function f(await){}", "async function await(){}", "(async function await(){})", "async()=>await", "async()=>{await}", "async function f(){await}", "function*g(){yield\n*1}", "function*g(){yield*}", "function*g(yield){}", "function*g(a=yield){}", "function*g(){(yield)=>1}", "function*g(){(a=yield)=>1}", "async function f(){(a=await 1)=>1}", "async function f(a=await 1){}", "async(a=await 1)=>1", "function*g(){function yield(){}}", "function*g(){var yield}", "function*g(){yield:1}", "async function f(){await:1}", "yield:1", "await:1", "'use strict';yield:1", "function*g(){yield=1}", "function*g(){yield++}", "function*g(){++yield}", "async function f(){await=1}", "async function f(){await++}",
  "return", "break", "continue", "break a", "continue a", "a:{continue a}", "a:{break b}", "a:while(1){continue b}", "a:a:;", "while(1){function f(){break}}", "switch(1){case 1:continue}", "if(1)break", "{break}", "x:{(function(){break x})}", "x:while(1)(()=>{continue x})", "var a;a:{var b}a:;", "do continue;while(0)", "try{break}finally{}", "super.x", "super()", "()=>super.x", "function f(){super.x}", "({m(){super()}})", "({m(){super.x}})", "({m:function(){super.x}})", "({get m(){super()}})", "class A{m(){super()}}", "class A extends B{m(){super()}}", "class A extends B{constructor(){()=>super()}}", "class A extends B{constructor(){function f(){super()}}}", "class A{constructor(){super()}}", "class A extends B{static m(){super()}}", "class A extends B{x=super()}", "class A extends B{x=super.y}", "class A{static{super.x}}", "new super.x", "new super", "super", "super?.x", "super.#x", "class A extends B{m(){super.#x}}", "class A{#x;m(){delete this.#x}}", "class A{#x;m(){delete this?.#x}}", "class A{#x;m(){delete (this.#x)}}", "class A{m(){this.#x}}", "this.#x", "class A{#x}this.#x", "class A{#x;m(){return this?.#x}}", "class A{#x;m(){return this?.a.#x}}", "class A{#x;m(){return a?.#x=1}}", "class A{#x;m(){return a?.b=1}}", "a?.b=1", "a?.b++", "a?.[0]=1", "a?.()=1", "a?.b`t`", "a?.`t`", "new a?.b", "new a?.()", "a?.b.c=1", "(a?.b).c=1", "a?.5:1", "a?.5", "a ?. b", "a?. 5:1", "a??b||c", "a||b??c", "a&&b??c", "a??b&&c", "(a??b)||c", "a**b**c", "-a**b", "+a**b", "!a**b", "typeof a**b", "(-a)**b", "a++**b", "++a**b", "await a**b", "delete a**b", "void a**b", "~a**b", "-1**2",
  "'use strict';with(a){}", "'use strict';delete x", "'use strict';delete (x)", "'use strict';delete ((x))", "'use strict';010", "'use strict';08", "'use strict';09.5", "'use strict';'\\01'", "'use strict';'\\8'", "'use strict';'\\08'", "'\\08'", "`\\08`", "`\\01`", "`\\u{110000}`", "'\\u{110000}'", "'\\u{}'", "'\\u12'", "'\\x1'", "'\\xg1'", "`\\u12`", "`\\xg`", "tag`\\u12`", "tag`\\xg`", "'a\nb'", "'a\u2028b'", "\"a\\\nb\"", "`a${`", "`a${}`", "`${`", "`${1`", "`${1}", "`", "'", "\"", "/*", "/a", "/a/gg", "/a/x", "/[/", "/(/", "/\\/", "/a/\\u0067", "/a/g\\u0067", "/(?<a>x)(?<a>y)/", "/(?<a>x)\\k<b>/", "/\\k<a>/u", "/\\p{x}/u", "/\\p{L}/", "/\\u{1F600}/", "/\\u{1F600}/u", "/[b-a]/", "/a{2,1}/", "/(?<=a)+/", "/(?=a)+/u", "/(?=a)+/", "/\\1/u", "/\\2(a)/u", "/{/u", "/}/u", "/]/u", "/a**/", "/+/", "/?/", "/*/", "/(?:/", "/(?<>x)/", "/(?<1a>x)/", "/(?<a-b>x)/", "/\\c/u", "/[\\c]/u", "/\\-/u", "/[\\-]/u", "/[\\d-a]/u", "/[\\d-a]/", "/\\u{110000}/u", "/\\ud83d\\ude00/u.test('\\u{1F600}')", "/./v", "/[a&&&b]/v", "/[(]/v", "/[a--b]/v", "/\\p{RGI_Emoji}/v", "/\\p{RGI_Emoji}/u", "/[^\\p{RGI_Emoji}]/v", "/a/uv", "/(?i:a)/", "/(?i-i:a)/", "/(?ii:a)/", "/(?-:a)/", "/a/dgimsuy", "/a/dgimsuyv", "/a/gdd",
  "1=1", "a++ ++", "++a++", "a++++", "(a,b)=1", "(a=1)=2", "([a])=1", "({a})=1", "[a]=1", "[a+b]=[]", "[(a)]=[]", "[(a=1)]=[]", "[...a,]=[]", "[...a,b]=[]", "[...a=1]=[]", "({...a,b}={})", "({...{a}}={})", "({...[a]}={})", "({...(a)}={})", "({...a.b}={})", "({a:1}={})", "({a:b+c}={})", "({a=1})", "({a=1}).x", "({a=1},{b=2})", "({a=1})=>1", "({a=1}={})", "({'a'=1}={})", "({[a]=1}={})", "({get a=1}={})", "({a(){}}={})", "({async a=1}={})", "({a:b=1}={})", "({a:(b=1)}={})", "({a:(b)}={})", "({a:((b))}={})", "({a:(b.c)}={})", "({a:(b[0])}={})", "({a:(b())}={})", "({a:(b,c)}={})", "({a:this}={})", "({a:new.target}={})", "(this)=1", "this=1", "this++", "new.target=1", "import.meta", "import('a','b','c')", "import()", "import(...a)", "import(a,)", "import(a,b,)", "import(a,b,c,)", "new import('a')", "import.x", "import", "export", "export default 1", "export var a", "import a from 'b'", "import 'a'", "await import('a')", "(import('a'))", "typeof import('a')", "delete import('a')", "import('a')=1", "import('a')++", "import('a')`x`", "import('a').then",
  "if(1)", "if(1){}else", "if(1)else", "if()", "if", "while", "while()", "while(1)", "do;while", "do;while(1)", "do;while(1)x", "do;while(1);x", "do{}while(1)", "do x;while(1)", "do function f(){}while(0)", "switch", "switch(1)", "switch(1){", "switch(1){case}", "switch(1){case 1}", "switch(1){default:default:}", "switch(1){x}", "switch(1){case 1:case:}", "try", "try{}", "try{}catch", "try{}catch(){}", "try{}catch(a,b){}", "try{}catch(a=1){}", "try{}catch(...a){}", "try{}catch([a]=[]){}", "try{}catch{}", "try{}catch{}finally", "try{}finally", "try{}finally{}", "try{}catch(a){}finally", "try catch", "throw", "throw\n1", "throw 1", "with", "with(a)", "with(a)b", "debugger", "debugger;", "debugger x", "label:", "label: label2:", "label:;", "label:function f(){}", "label:function*f(){}", "label:async function f(){}", "label:class A{}", "label:let a", "label:const a=1", "label:let\na", "label:\nlet a", "lbl:let[a]=[]", "lbl:let\n[a]=[]", "if(1)let\n[a]=[]", "if(1)let a", "while(1)let\na", "while(1)let[a]=[]", "{let\na}", "let\nlet", "let\nyield", "let\nawait", "let\nasync", "let\nof", "let\nstatic", "let\nlet=1", "async function f(){let\nawait}",
  "class", "class A", "class A{", "class A extends", "class A extends{}", "class A extends B,C{}", "class A extends (B,C){}", "class A extends B=C{}", "class A extends a?b:c{}", "class A extends a+b{}", "class A extends -b{}", "class A extends a=>1{}", "class A extends (a=>1){}", "class A extends async()=>1{}", "class A extends new B{}", "class A extends new B(){}", "class A extends B.C{}", "class A extends B()(){}", "class A extends B`c`{}", "class A extends class{}{}", "class A extends function(){}{}", "class A extends{}.x{}", "class A extends []{}", "class A extends 1{}", "class A extends 'a'{}", "class A extends /a/{}", "class A extends null{}", "class A extends undefined{}", "class A extends void 0{}", "class A extends typeof B{}", "class A extends delete a.b{}", "class A extends this{}", "class A extends super.b{}", "class A extends yield{}", "class A extends await{}", "class A extends async{}", "class A extends let{}", "class A extends static{}", "class A extends of{}", "class A extends get{}", "class A extends set{}", "class A extends eval{}", "class A extends arguments{}", "class eval{}", "class arguments{}", "class let{}", "class static{}", "class yield{}", "class await{}", "class async{}", "class of{}", "class get{}", "class set{}", "class implements{}", "class interface{}", "class package{}", "class private{}", "class protected{}", "class public{}", "class enum{}", "class null{}", "class true{}", "class this{}", "class new{}", "class A{;}", "class A{;;}", "class A{a;b}", "class A{a b}", "class A{a\nb}", "class A{a=1\nb=2}", "class A{a=1 b=2}", "class A{a=1,b=2}", "class A{a,b}", "class A{static}", "class A{static;}", "class A{static=1}", "class A{static static}", "class A{static static(){}}", "class A{static static=1}", "class A{static\nstatic}", "class A{static\nm(){}}", "class A{get\nm(){}}", "class A{get;}", "class A{get=1}", "class A{get(){}}", "class A{get\n}", "class A{get\nx}", "class A{get x(){}get x(){}}", "class A{get x(){}set x(v){}}", "class A{get x(){}x(){}}", "class A{x(){}get x(){}}", "class A{static get x(){}get x(){}}", "class A{get x(v){}}", "class A{set x(){}}", "class A{set x(a,b){}}", "class A{set x(...a){}}", "class A{set x([a]){}}", "class A{set x(a=1){}}", "class A{async\nm(){}}", "class A{async;}", "class A{async=1}", "class A{async(){}}", "class A{async *m(){}}", "class A{async\n*m(){}}", "class A{*async(){}}", "class A{*get x(){}}", "class A{*static m(){}}", "class A{static*m(){}}", "class A{static async*m(){}}", "class A{async get x(){}}", "class A{get async x(){}}", "class A{static get static(){}}", "class A{static static static(){}}", "class A{constructor(){}static constructor(){}}", "class A{constructor(){}['constructor'](){}}", "class A{async constructor(){}}", "class A{*constructor(){}}", "class A{get constructor(){}}", "class A{set constructor(v){}}", "class A{static get prototype(){}}", "class A{static async prototype(){}}", "class A{static ['prototype'](){}}", "class A{'prototype'(){}}", "class A{static 'prototype'=1}", "class A{static ['prototype']=1}", "class A{static prototype}", "class A{constructor}", "class A{constructor=1}", "class A{'constructor'}", "class A{['constructor']=1}", "class A{static constructor=1}", "class A{static constructor}", "class A{#constructor}", "class A{static #constructor(){}}", "class A{#a;#a}", "class A{#a;static #a}", "class A{#a;#a(){}}", "class A{#a(){}#a(){}}", "class A{get #a(){}get #a(){}}", "class A{get #a(){}set #a(v){}}", "class A{static get #a(){}set #a(v){}}", "class A{static get #a(){}static set #a(v){}}", "class A{get #a(){}static set #a(v){}}", "class A{# a}", "class A{#\\u0061;m(){this.#a}}", "class A{#a;m(){this.# a}}", "class A{#a;m(){this.#\\u0061}}", "class A{#a;m(){this.#b}}", "class A{#a;m(){delete this.#a}}", "class A{#a;m(){#a}}", "class A{#a;m(){#a in this}}", "class A{#a;m(){(#a) in this}}", "class A{#a;m(){1+#a in this}}", "class A{#a;m(){#a in #a in this}}", "class A{#a;m(){#a+1}}", "class A{#a;m(){#a=1}}", "class A{#a;m(){this.#a++}}", "class A{#a;m(){[this.#a]=[]}}", "class A{#a;m(){({a:this.#a}={})}}", "class A{#a;m(){for(this.#a of []);}}", "class A{#a;m(){this?.#a}}", "class A{#a;m(){new this.#a}}", "class A{#a;m(){this.#a`x`}}", "class A{#a;m(){super.#a}}", "class A{#a;static{this.#a}}", "class A{static{var a;let a}}", "class A{static{var a;var a}}", "class A{static{let a;let a}}", "class A{static{await}}", "class A{static{yield}}", "class A{static{arguments}}", "class A{static{return}}", "class A{static{break}}", "class A{static{super()}}", "class A{static{super.x}}", "class A{static{this}}", "class A{static{new.target}}", "class A{static{function f(){arguments}}}", "class A{static{()=>arguments}}", "class A{static{async function f(){await 1}}}", "class A{static{(await)=>1}}", "class A{static{async()=>await 1}}", "class A{static{function*g(){yield}}}", "class A{static{label:while(1)break label}}", "class A{static{a:{break a}}}", "class A{static{await:1}}", "class A{static\n{}}", "class A{static async{}}", "class A{static{};static{}}", "class A{static{}static{}}", "class A{static{} static x}", "class A{static {} static}", "class A{static *{}}", "class A{static()}", "class A{static(){}}", "class A{static=1}", "class A{static\n=1}", "class A{static\n;}", "class A{x=arguments}", "class A{x=()=>arguments}", "class A{x=function(){arguments}}", "class A{x=eval('arguments')}", "class A{x=await}", "class A{x=yield}", "class A{x=super()}", "class A{x=super.y}", "class A{x=new.target}", "class A{x=this}", "class A{static x=this}", "class A{static x=super.y}", "class A{static x=arguments}", "class A{[arguments]=1}", "class A{[await]=1}", "class A{[yield]=1}", "class A{x=1,y=2}", "class A{x=1;y}", "class A{x=1\ny=2}", "class A{x\n=1}", "class A{x\n=1\n}", "class A{x\ny}", "class A{x;y}", "class A{x\n*y(){}}", "class A{x\n[y]=1}", "class A{x\n(y)=1}", "class A{x=1\n*y(){}}", "class A{x=1\n[y]=1}", "class A{x=1\n(y){}}", "class A{x\nget y(){}}", "class A{x\nset y(v){}}", "class A{x\nstatic y(){}}", "class A{x\nasync y(){}}", "class A{get\nstatic x(){}}", "class A{static\nasync y(){}}", "class A{x\nin}", "class A{in}", "class A{in=1}", "class A{x in y}", "class A{x instanceof y}", "class A{x of y}", "class A{if(){}}", "class A{if=1}", "class A{class(){}}", "class A{class=1}", "class A{new(){}}", "class A{function(){}}", "class A{null(){}}", "class A{true=1}", "class A{this(){}}", "class A{let(){}}", "class A{yield(){}}", "class A{await(){}}", "class A{enum(){}}", "class A{static let(){}}", "class A{static yield(){}}", "class A{static await(){}}", "class A{async await(){}}", "class A{async yield(){}}", "class A{*yield(){}}", "class A{*await(){}}", "class A{get yield(){}}", "class A{get await(){}}", "class A{static async *await(){}}", "class A{static async *yield(){}}",
];
const seenSnip = new Set();
const uniqSnip = snippets.filter((s) => !seenSnip.has(s) && seenSnip.add(s));
for (const s of uniqSnip) {
  add(`T(()=>{return (0,eval)(${J(s)})})`, `T(()=>{return Function(${J(s)})()})`, `T(()=>{'use strict';return eval(${J(s)})})`);
}

// ---- Execução.
const baseSources = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
baseSources.push(...knownPrograms("function_scope_bun.tsv", (file) => !(!file.endsWith(".tsv") || file === "function_scope_bun.tsv")));
const baseSet = new Set(baseSources);
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let dup = 0;
const jobs = [];
for (const expr of unique) {
  // A fonte exata já aparecer num golden existente conta como repetida.
  const source = PRELUDE + `globalThis.R = ${expr}`;
  if (baseSet.has(source)) { dup++; continue; }
  jobs.push({ expr, source });
}

// Amostra determinística por hash (sampleByHash): ZJSC_STRIDE=n mantém cerca de um job em cada n (padrão 1, tudo).
const stride = Number(process.env.ZJSC_STRIDE || 1);
if (stride > 1) {
  const kept = sampleByHash(jobs, Math.ceil(jobs.length / stride), (job) => job.source);
  jobs.length = 0;
  jobs.push(...kept);
}
process.stderr.write(`jobs ${jobs.length}\n`);

function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 5000, killSignal: "SIGKILL", env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => resolve({ code, out, err }));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: Math.min(8, Math.max(4, os.cpus().length)) }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i].source);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const { expr, source } = jobs[i];
    const r = results[i];
    if (r.code !== 0) {
      dropped++;
      process.stderr.write("erro de programa: " + J(expr).slice(0, 160) + " " + r.err.split("\n")[0] + "\n");
      continue;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun|\u2013|\u2014/i.test(r.out) || /\u2013|\u2014/.test(source)) {
      dropped++;
      process.stderr.write("caminho, marca ou travessão no resultado: " + J(expr).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    lines.push(J(source) + "\t" + J(r.out));
  }
  process.stdout.write(emitFactoredLines("function_scope", lines)); // grava também tests/golden/function_scope.preludes.json
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
