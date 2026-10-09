// Gera tests/golden/tdz_grid_bun.tsv: grade de TDZ e escopo léxico medida no bun.
// Cobre o acesso antes da inicialização a let/const/class em cada posição (mesmo bloco, closure chamada antes, arrow,
// método, getter, inicializador de campo, bloco static, gerador, laço, try/finally, eval direto), em cada forma de
// acesso (leitura, escrita, typeof, delete, ++, atribuição lógica, chamada, new, template, desestruturação...), defaults de
// parâmetro referindo parâmetro posterior, for(let i in/of) com expressão referindo i, cabeçalhos de for com let/const,
// switch com case compartilhando bloco, class extends e computed key referindo o próprio nome, shadowing em catch(e) com
// var e, function declarations em bloco com let homônimo (SyntaxError exato) e global let vs var homônimo em programas
// separados avaliados com (0,eval) em sequência. As mensagens exatas de ReferenceError, TypeError e SyntaxError entram
// no resultado. O código de cada caso vai numa string avaliada por Function ou (0,eval), em modo sloppy e strict.
// Programas já presentes (linha idêntica) em qualquer outro golden são descartados. Cada programa roda num bun filho
// novo, no máximo 6 em paralelo, com timeout de 8 s.
// Colunas: sufixo do programa (JSON), valor da global `R` (JSON), índice do prelúdio (fatorado).
// Uso: bun scripts/gen-tdz-grid-golden.js > tests/golden/tdz_grid_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const { spawn } = require("child_process");

// O filho só avalia o programa; quem imprime `R` é o preload (ver `writeResultPreload` em golden-prelude.js).
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
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const J = JSON.stringify;
const STRICT = '"use strict";';
const exprs = [];
// Função: o código é o corpo de uma Function (precisa de return).
const viaFunction = (code) => `T(()=>Function(${J(code)})())`;
// Eval indireto: o valor é o completion value do código.
const viaEval = (code) => `T(()=>(0,eval)(${J(code)}))`;
const addFn = (code) => { exprs.push(viaFunction(code), viaFunction(STRICT + code)); };
const addEval = (code) => { exprs.push(viaEval(code), viaEval(STRICT + code)); };
const CATCH = 'catch(e){return e.name+": "+e.message}';

// ---- 1. Grade posição x forma de acesso x declaração.
const decls = ["let x=1;", "let x;", "const x=1;", "class x{};", "let [x]=[1];", "let {x}={x:1};"];
const accesses = [
  "x", "x=2", "x+=1", "x++", "++x", "x||=1", "x&&=1", "x??=1", "typeof x", "delete x", "void x", "x.y", "x?.y", "x()", "x?.()",
  "new x", "x`a`", "[x]", "({x})", "({[x]:1})", "({...x})", "[...x]", "-x", "!x", "x in {}", "x instanceof Object", "Object instanceof x",
  "x==1", "`${x}`", "([x]=[1])", "({x}={x:1})", "(x,1)", "x?1:2", "(()=>x)()", "((a=x)=>a)()", "eval('x')", "eval('typeof x')", "(0,eval)('typeof x')",
];
const positions = {
  same: (a, d) => `try{return [${a}]}${CATCH} ${d}`,
  block: (a, d) => `var r;{try{r=[${a}]}catch(e){r=e.name+": "+e.message} ${d}} return r`,
  nested: (a, d) => `var r;{{try{r=[${a}]}catch(e){r=e.name+": "+e.message}} ${d}} return r`,
  before: (a, d) => `function f(){return [${a}]} var r; try{r=f()}catch(e){r=e.name+": "+e.message} ${d} return [r]`,
  beforeAfter: (a, d) => `function f(){return [${a}]} var r; try{r=f()}catch(e){r=e.name+": "+e.message} ${d} var q; try{q=f()}${"catch(e){q=e.name+\": \"+e.message}"} return [r,q]`,
  arrow: (a, d) => `var f=()=>[${a}]; var r; try{r=f()}catch(e){r=e.name+": "+e.message} ${d} return r`,
  method: (a, d) => `var o={m(){return [${a}]}}; var r; try{r=o.m()}catch(e){r=e.name+": "+e.message} ${d} return r`,
  getter: (a, d) => `var o={get g(){return [${a}]}}; var r; try{r=o.g}catch(e){r=e.name+": "+e.message} ${d} return r`,
  field: (a, d) => `var k=()=>new (class{f=[${a}]}); var r; try{r=k().f}catch(e){r=e.name+": "+e.message} ${d} return r`,
  staticBlock: (a, d) => `var r; try{class K{static{r=[${a}]}}}catch(e){r=e.name+": "+e.message} ${d} return r`,
  generator: (a, d) => `function* g(){yield [${a}]} var r; try{r=g().next().value}catch(e){r=e.name+": "+e.message} ${d} return r`,
  loop: (a, d) => `var r=[]; for(var i=0;i<2;i++){try{r.push([${a}])}catch(e){r.push(e.name+": "+e.message)} ${d}} return r`,
  tryFinally: (a, d) => `var r; try{try{r=[${a}]}finally{${d}}}catch(e){r=e.name+": "+e.message} return r`,
  directEval: (a, d) => `var r; try{r=eval(${J("[" + a + "]")})}catch(e){r=e.name+": "+e.message} ${d} return r`,
  labeled: (a, d) => `var r; l:{try{r=[${a}]}catch(e){r=e.name+": "+e.message}; break l; ${d}} return r`,
  after: (a, d) => `${d} try{return [${a}]}${CATCH}`,
};
for (const [pos, make] of Object.entries(positions)) {
  for (const a of accesses) {
    for (const d of decls) {
      exprs.push(viaFunction(make(a, d)));
    }
    for (const d of [decls[0], decls[2], decls[3]]) exprs.push(viaFunction(STRICT + make(a, d)));
  }
}
// Posição de eval indireto: o topo do eval tem escopo léxico próprio.
for (const a of accesses) for (const d of decls) {
  addEval(`try{${a}}catch(e){e.name+": "+e.message} ${d}`);
  addEval(`${a}; ${d}`);
}

// ---- 2. Parâmetros: default referindo parâmetro posterior.
const paramLists = [
  "a=b,b", "a=a", "a,b=a", "a=()=>b,b", "a=b,b=1", "{a=b},b", "[a=b],b", "a=eval('b'),b", "a=typeof b,b", "a=b=1,b", "a=(b,1),b",
  "...a", "a=1,b=a+1", "a=function(){return b},b", "a=class{static v=b},b", "a=new.target,b", "a=arguments.length,b", "a=this,b",
  "a=x,b", "a=(()=>b)(),b", "{a}={a:b},b", "a=[b],b", "a=`${b}`,b", "a=b?.c,b", "a=delete b,b", "b=a,a", "a=c,b=1,c=2", "a=b,a2=a,b",
  "a=class{[b](){}},b", "a=class{x=b},b",
];
const bodies = ["return [a,b]", "var b;return [a,b]", "var a;return [a]", "let c=1;return [a,c]", "return (()=>a)()", "function a(){}return a", "var a=5;return [a]", "return typeof b"];
const forms = [
  (p, b) => `function f(${p}){${b}}`,
  (p, b) => `var f=(${p})=>{${b}};`,
  (p, b) => `var f=({m(${p}){${b}}}).m;`,
  (p, b) => `function* f(${p}){${b}}`,
  (p, b) => `var f=class{constructor(${p}){${b.replace("return ", "this.v=")}}}; var g=f; f=function(...q){return new g(...q).v};`,
];
const calls = ["f()", "f(1)", "f(undefined,2)", "f(1,2)"];
let n = 0;
for (const p of paramLists) for (const b of bodies) {
  for (let fi = 0; fi < forms.length; fi++) {
    const form = forms[fi](p, b);
    const call = calls[n++ % calls.length];
    const ret = fi === 3 ? "f(" + call.slice(2, -1) + ").next().value" : call;
    addFn(`${form} return ${ret}`);
  }
}
// Parâmetros duplicados e conflitos com o corpo, só pelo texto do erro.
for (const src of [
  "function f(a,a){} return 1", "function f(a=1,a){}", "(a,a)=>1", "function f(a){let a}", "function f(a){const a=1}", "function f(a){class a{}}",
  "function f({a},a){}", "function f(a,[a]){}", "function f(a){var a;let b;var b}", "function f(...a){let a}", "function f(a=1){'use strict'}",
  "function f(){'use strict';var a;let a}", "var f=function(a=1){'use strict'}", "({m(a=1){'use strict'}})", "function f(a,a){'use strict'}",
]) { addFn(`try{${src.startsWith("function") || src.startsWith("var") ? src : "(" + src + ")"}; return "ok"}${CATCH}`); addFn(`try{Function(${J(src)});return "ok"}${CATCH}`); }

// ---- 3. Laços: for(let i in/of) com expressão referindo i, cabeçalhos, closures por iteração.
const loopExprs = ["i", "[i]", "[1,2]", "(()=>i)()", "{a:i}", "eval('i')", "typeof i", "(i,[1])", "[1].map(()=>i)", "i=[1]", "[...i]", "{[i]:1}", "i??[1]", "i?.a", "[0,1].concat(i)"];
const loopBinds = ["let i", "const i", "let [i]", "const [i]", "let {i}", "const {i}", "let [i=1]", "let {a:i}", "let [j=i]"];
for (const e of loopExprs) for (const bind of loopBinds) for (const kw of ["of", "in"]) {
  addFn(`var r=[]; for(${bind} ${kw} ${e}){r.push(typeof i)} return r`);
}
const forHeads = [
  "let i=0;i<2;i++", "let i=i;;", "let i=0,j=i;j<1;j++", "let i=j,j=0;;", "const i=1;i<2;", "const i=0;i<2;i++", "let i=0;i<2;i+=i", "let i=()=>i;;",
  "let i=typeof i;;", "let i=0,i2=()=>i;i<2;i++", "let [i]=[i];;", "let {i}={i};;", "let i=0;i<2;)", "let i=eval('i');;", "let i=(()=>i)();;",
];
for (const h of forHeads) {
  const head = h.endsWith(")") ? h.slice(0, -1) + ";" : h;
  for (const body of ["r.push(i);break", "fs.push(()=>i);if(fs.length>2)break;", "r.push(typeof i);break", "i=9;break"]) {
    addFn(`var r=[],fs=[]; for(${head}){${body}} return [r,fs.map(f=>f())]`);
  }
}
for (const bind of ["let i", "const i", "var i"]) for (const kw of ["of", "in"]) for (const body of ["fs.push(()=>i)", "i=3", "var i=2", "let i=2", "{let i=2;fs.push(i)}", "function i(){}"]) {
  const iter = kw === "of" ? "[1,2]" : "{a:1,b:2}";
  addFn(`var fs=[]; try{ for(${bind} ${kw} ${iter}){${body}} }${"catch(e){fs.push(e.name+\": \"+e.message)}"} return fs.map(f=>typeof f==="function"?f():f)`);
}

// ---- 4. switch: case compartilhando o bloco do corpo.
const switchBodies = [
  "case 0: let x=1; case 1: r=[x]; break;", "case 0: r=[x]; break; case 1: let x=1;", "case 0: let x=1; break; case 1: x=2; r=[x]; break;",
  "case 0: const x=1; break; case 1: r=[typeof x]; break;", "case 0: class x{}; break; case 1: r=[x]; break;", "case 1: let x=1; r=[x]; break;",
  "case 1: r=[()=>x]; break; case 0: let x=1;", "case 0: let x=1; case 1: function f(){return x} r=[f()]; break;", "case 1: function f(){return x} r=[f()]; break; case 0: let x=1;",
  "default: r=[x]; case 3: let x=1;", "case 0: let x; break; default: r=[x];", "case x: break; case 1: let x=1;", "case 0: let x=1; break; case (x=3): r=[x];",
  "case 1: { r=[x] } break; case 0: let x=1;", "case 0: let x=1; case 1: let x=2;", "case 0: var x=1; case 1: let x=2;", "case 0: let x=1; case 1: function x(){}",
  "case 0: function x(){} case 1: let x=2;", "case 0: function x(){} case 1: function x(){} r=[typeof x];", "case 1: r=[typeof x]; break; case 0: let x=1;",
  "case 1: r=[delete x]; break; case 0: let x=1;", "case 1: x++; break; case 0: let x=1;", "case 1: try{x}catch(e){r=[e.name]} break; case 0: const x=1;",
];
for (const body of switchBodies) for (const disc of [0, 1, 2]) {
  addFn(`var r; switch(${disc}){${body}} return r`);
}

// ---- 5. class extends / computed key referindo o próprio nome.
const classBits = [
  "class C extends C{}", "class C extends (C,Object){}", "class C extends (()=>C)(){}", "class C extends (class{}){}", "class C extends Object{static x=C}",
  "class C{static x=C.name}", "class C{static x=typeof C}", "class C{[C](){}}", "class C{[typeof C](){}}", "class C{static [C]=1}", "class C{[C]=1}",
  "class C{static{C}}", "class C{static{r=[typeof C]}}", "class C{static [C.name]=1}", "class C{[(C,'a')](){}}", "class C{x=C;static s=new C().x===C}",
  "class C{static m(){return C}}; r=[C.m()===C]", "class C{static [(()=>C)()](){}}", "class C{static [eval('C')](){}}", "class C{static [eval('typeof C')](){}}",
  "class C{constructor(){C=1}}; try{new C}catch(e){r=[e.name+': '+e.message]}", "class C{static m(){C=1}}; try{C.m()}catch(e){r=[e.name+': '+e.message]}",
  "class C{static x=(C=1)}", "class C extends (class D{static x=C}){}", "class C extends Object{[C.x](){}}",
  "let D=class extends D{}", "let D=class{[D](){}}", "let D=class{static x=D}", "const D=class E{static x=E;static y=typeof D}", "const D=class E{[typeof D](){}}",
  "const D=class E extends E{}", "const D=class E extends D{}", "var D=class E{static x=E.name;static [E.name]=1}", "var D=class E{[E](){}}",
  "let D=class{static{D}}", "const D=class{static x=D.name}", "let D=(class{static x=typeof D})", "const D=class{static [D]=1}",
  "let D=class D2{static x=()=>D2}; r=[D.x()===D]", "(class C{[C](){}})", "(class C extends C{})", "(class C{static x=C})", "(class{[typeof C](){}})",
  "class C{static x=D}; class D{}", "class C{static x=new D}; class D{}", "class D{}; class C extends D{static x=D}", "class C extends D{}; class D{}",
  "new C; class C{}", "typeof C; class C{}", "C=1; class C{}", "class C{}; C=1", "class C{}; class C{}", "class C{}; let C", "class C{}; var C", "var C; class C{}", "let C; class C{}",
  "class C{}; { class C{} }; r=[typeof C]", "{ class C{} } r=[typeof C]", "r=[typeof C]; { class C{} }", "class C{}; function C(){}", "function C(){}; class C{}", "class C{ #p=C; static q(){return typeof C} }",
];
for (const bit of classBits) {
  addFn(`var r; try{ ${bit} }${"catch(e){r=[e.name+': '+e.message]}"} return r`);
  addEval(`try{ ${bit} }catch(e){e.name+': '+e.message}`);
}

// ---- 6. catch(e) com var e e variações.
const catchBodies = [
  "try{throw 1}catch(e){var e=2; r=[e]} r.push(e)", "try{throw 1}catch(e){var e; r=[e]} r.push(e)", "try{throw 1}catch(e){var e=2} r=[e]",
  "try{throw 1}catch(e){e=3; var e; r=[e]} r.push(e)", "try{throw 1}catch(e){{var e=4} r=[e]} r.push(e)", "try{throw 1}catch(e){let e}",
  "try{throw 1}catch(e){const e=1}", "try{throw 1}catch(e){class e{}}", "try{throw 1}catch(e){function e(){}}", "try{throw 1}catch([e]){var e}",
  "try{throw [1]}catch([e]){var e}", "try{throw {e:1}}catch({e}){var e}", "try{throw 1}catch([e]){let e}", "try{throw 1}catch(e){for(var e of [1]);}",
  "try{throw 1}catch(e){for(var e in {a:1});r=[e]} r.push(e)", "try{throw 1}catch(e){for(var e;;)break; r=[e]}", "try{throw 1}catch(e){for(var e=5;;)break; r=[e]} r.push(e)",
  "try{throw 1}catch{var e=2} r=[e]", "try{throw 1}catch(e){var f=()=>e; var e=2; r=[f()]} r.push(typeof e)", "try{throw 1}catch(e){r=[typeof e]} r.push(typeof e)",
  "var e=0; try{throw 1}catch(e){var e=2} r=[e]", "let e=0; try{throw 1}catch(e){r=[e]} r.push(e)", "let e=0; try{throw 1}catch(e){var e}", "const e=0; try{throw 1}catch(e){var e}",
  "try{throw 1}catch(e){try{throw 2}catch(e){var e=3; r=[e]} r.push(e)} r.push(e)", "try{throw 1}catch(e){try{throw 2}catch(e){let f=e; r=[f]}}",
  "try{throw 1}catch(e){{let e=5; r=[e]} r.push(e)}", "try{throw 1}catch(e){{function e(){}} r=[typeof e]}", "try{throw 1}catch(e){if(1)function e(){} r=[typeof e]}",
  "try{throw 1}catch(e){eval('var e=9'); r=[e]} r.push(e)", "try{throw 1}catch(e){eval('let e=9'); r=[e]}", "try{throw 1}catch(e){(function(){var e=3})(); r=[e]}",
  "try{throw 1}catch(e){(0,eval)('var e=7'); r=[e]} r.push(e)", "try{throw 1}catch(e=2){}", "try{throw 1}catch(e,f){}", "try{throw 1}catch(e){var [e]=[8]}",
  "try{throw 1}catch(e){var {e}={e:8}}", "try{throw 1}catch({e}){var e}", "try{throw {}}catch({e=5}){r=[e]}", "try{throw 1}catch(e){label:var e=2;}",
  "try{throw 1}catch(e){r=[(()=>{try{return e}finally{var e=3}})()]}", "try{throw 1}catch(e){r=[(()=>{var e=3;return e})(), e]}", "try{throw 1}catch(e){let f=e; var e2=f; r=[e2]}",
  "try{throw 1}catch(e){function g(){return e} r=[g()]}", "try{throw 1}catch(e){function g(){var e=2; return e} r=[g(), e]}", "try{throw 1}catch(e){var h=function e(){return typeof e}; r=[h()]}",
];
for (const body of catchBodies) {
  addFn(`var r; try{ ${body} }${"catch(err){r=[err.name+': '+err.message]}"} return r`);
  addFn(`${body}; return r`);
  addEval(`var r; ${body}; r`);
}

// ---- 7. function declaration em bloco com let homônimo.
const fnBlock = [
  "{let f; function f(){}}", "{function f(){} let f}", "{function f(){} var f}", "{var f; function f(){}}", "{function f(){} function f(){}}",
  "{let f} function f(){}", "let f; { function f(){} } r=[typeof f]", "function f(){} let f", "let f; function f(){}", "var f; let f", "let f; var f",
  "function g(){let f; function f(){}}", "function g(){{let f} function f(){}} r=[typeof g]", "switch(1){case 1: let f; case 2: function f(){}}",
  "switch(1){case 1: function f(){} case 2: function f(){}} r=[typeof f]", "switch(1){case 1: function f(){} case 2: let f}", "{async function f(){} function f(){}}",
  "{function* f(){} function f(){}}", "{function f(){} async function f(){}}", "{function f(){} class f{}}", "{class f{} function f(){}}", "{const f=1; function f(){}}",
  "{function f(){} const f=1}", "if(1) function f(){} r=[typeof f]", "if(0) function f(){} r=[typeof f]", "if(1) function f(){} else function f(){}", "if(1) let f=1",
  "if(1) const f=1", "if(1) class f{}", "while(0) function f(){}", "for(;0;) function f(){}", "l: function f(){} r=[typeof f]", "l: let f=1", "l: const f=1",
  "if(1) l: function f(){}", "{l: function f(){}}", "{function f(){} l: function f(){}}", "r=[typeof f]; { function f(){} } r.push(typeof f)",
  "{ function f(){return 1} } { function f(){return 2} } r=[f()]", "{ function f(){} let g; } r=[typeof f, typeof g]", "let f=1; { function f(){} } r=[f]",
  "var f=1; { function f(){} } r=[typeof f]", "{ let f=1; { function f(){} } r=[f] }", "{ let f=1; { var f } }", "{ var f; { let f } }", "{ { var f } let f }",
  "for(let f of [1]){ var f }", "for(let f of [1]){ function f(){} }", "for(let f of [1]){ { function f(){} } r=[f] }", "for(let f of [1]){ let f=2; r=[f] }",
  "for(let f=0;;){ let f=2; r=[f]; break }", "for(let f=0;;){ var f; break }", "for(const f of [1]) var f", "(function(f){ { function f(){} } r=[typeof f] })(1)",
  "(function(f){ let f })(1)", "(function(f){ var f; r=[f] })(1)", "(function(){ let f=1; { var f } })()", "(function(){ function f(){} var f; r=[typeof f] })()",
  "(function(){ var f=1; function f(){} r=[f] })()", "(function f(){ var f=1; r=[typeof f] })()", "(function f(){ let f=1; r=[f] })()", "(function f(){ f=2; r=[typeof f] })()",
  "(function f(){ 'use strict'; f=2 })()", "(function(){ r=[typeof f]; { function f(){} } })()", "(function(){ r=[typeof f]; if(1) function f(){} })()",
  "(function(){ { function f(){} } { let f=2; r=[f] } r.push(typeof f) })()", "(function(){ { function f(){return 1} f=2; r=[f] } r.push(typeof f) })()",
  "(function(){ { f=2; function f(){} } r=[typeof f] })()", "(function(){ f=0; { function f(){} } r=[typeof f] })()",
];
for (const body of fnBlock) {
  addFn(`var r; try{ ${body} }${"catch(err){r=[err.name+': '+err.message]}"} return r`);
  addFn(`try{ Function(${J(body)})(); return "ok" }${CATCH}`);
  addEval(`try{ ${body}; typeof r==="undefined"?"u":r }catch(err){err.name+': '+err.message}`);
}

// ---- 8. global let vs var homônimo em programas separados avaliados em sequência com (0,eval).
const globalForms = ["var x=1", "let x=1", "const x=1", "class x{}", "function x(){}", "var x", "let x", "async function x(){}", "function* x(){}", "x=1"];
for (const a of globalForms) for (const b of globalForms) {
  const probe = "typeof x";
  exprs.push(`T(()=>{var r=[];for(const s of [${J(a)},${J(b)},${J(probe)}]){try{r.push((0,eval)(s))}catch(e){r.push(e.name+": "+e.message)}}delete globalThis.x;return r})`);
  exprs.push(`T(()=>{var r=[];for(const s of [${J(STRICT + a)},${J(STRICT + b)},${J(probe)}]){try{r.push((0,eval)(s))}catch(e){r.push(e.name+": "+e.message)}}delete globalThis.x;return r})`);
  exprs.push(`T(()=>{(0,eval)(${J(a + ";" + b)});return typeof x})`);
  exprs.push(`T(()=>{(0,eval)(${J(a)});Function(${J(b)})();return typeof x})`);
  exprs.push(`T(()=>{(0,eval)(${J(a + ";{" + b + "}")});return typeof x})`);
  exprs.push(`T(()=>{(0,eval)(${J("{" + a + "}" + b)});return typeof x})`);
  exprs.push(`T(()=>{(0,eval)(${J("(function(){" + a + ";" + b + "})()")});return typeof x})`);
  exprs.push(`T(()=>{(0,eval)(${J("(function(x){" + b + "})(1)")});return typeof x})`);
}
for (const pre of ["globalThis.x=1", "Object.defineProperty(globalThis,'x',{value:1,configurable:false,writable:false})", "Object.defineProperty(globalThis,'x',{get(){return 1},configurable:true})", "Object.preventExtensions(globalThis)", "Object.freeze(globalThis)"]) {
  for (const f of ["var x=2", "let x=2", "const x=2", "class x{}", "function x(){}", "x=2", "var x", "let x"]) {
    exprs.push(`T(()=>{var r=[];try{${pre}}catch(e){r.push("p "+e.name)}try{r.push((0,eval)(${J(f + ";typeof x")}))}catch(e){r.push(e.name+": "+e.message)}try{r.push((0,eval)("typeof x"))}catch(e){r.push(e.name+": "+e.message)}return r})`);
  }
}
for (const decl of ["let", "const", "var"]) for (const probe of ["x", "typeof x", "x=1", "delete x", "globalThis.x", "this.x"]) {
  exprs.push(`T(()=>{return (0,eval)(${J(probe + ";" + decl + " x=5;" + probe)})})`);
  exprs.push(`T(()=>{return (0,eval)(${J(decl + " x=5;" + probe + ";" + decl + " y=x;y")})})`);
}

// ---- Dedup: linha idêntica já presente em qualquer outro golden é descartada.
const knownLines = new Set();
for (const program of knownPrograms("tdz_grid_bun.tsv", () => true)) {
  for (const line of program.split("\n")) knownLines.add(line);
}
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let dup = 0;
const jobs = [];
for (const expr of unique) {
  const line = `globalThis.R = ${expr}`;
  if (knownLines.has(line) || knownLines.has(expr)) { dup++; continue; }
  jobs.push(STRICT + "\n" + PRELUDE + line);
}
const DASH = new RegExp("[" + String.fromCharCode(0x2013) + String.fromCharCode(0x2014) + "]");
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", (d) => { err += d; });
    child.on("close", (code) => {
      clearTimeout(timer);
      const decoded = code === 0 ? decodeResult(out) : null;
      resolve(decoded !== null ? { out: decoded } : { err: err || "filho falhou" });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}
async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.err !== undefined) { dropped++; process.stderr.write("erro de programa: " + J(jobs[i].slice(PRELUDE.length + 20, PRELUDE.length + 200)) + " " + r.err.slice(0, 120) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || DASH.test(r.out) || DASH.test(jobs[i])) { dropped++; continue; }
    kept++;
    lines.push(J(jobs[i]) + "\t" + J(r.out));
  }
  process.stdout.write(emitFactoredLines("tdz_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
