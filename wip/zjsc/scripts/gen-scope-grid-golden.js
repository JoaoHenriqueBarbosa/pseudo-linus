// Gera tests/golden/scope_grid_bun.tsv: grade de escopo e closures medida no bun 1.4.2. Cobre captura de variáveis em
// laços (for let/var/const, for-in/of, closures por iteração, mutação do contador, continue), let/const em switch,
// funções em blocos (Annex B) em sloppy e strict, shadowing de parâmetros por var/function, parâmetros com default e
// escopo próprio, `arguments` em defaults, catch com destructuring, binding interno de class/função, atribuição em NFE,
// eval com var em função com parâmetros, `with` com closures, TDZ via closures e typeof, delete de var/let, globais
// let/var (propriedade de globalThis ou não) e shadowing de built-ins globais.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num processo bun novo, como script do JSC puro (`vm.runInThisContext`), sem APIs de host no
// programa; SyntaxError de compilação deixa `R` indefinido ("<undefined>"). Programas já presentes nos goldens de
// escopo existentes são descartados.
// Uso: bun scripts/gen-scope-grid-golden.js > tests/golden/scope_grid_bun.tsv
const { emitFactoredLines } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  const source = fs.readFileSync(0, "utf8");
  try { require("node:vm").runInThisContext(source); } catch (e) {}
  const r = globalThis.R;
  process.stdout.write(r === undefined ? "<undefined>" : String(r));
  process.exit(0);
}

const programs = [];
const add = body => programs.push(body);
const q = JSON.stringify;
const CATCH = "}catch(e){globalThis.R=e.name+': '+e.message}";
// Corpo de função (IIFE) em sloppy ou strict; o valor devolvido vira R.
const fn = (code, strict) => `try{globalThis.R=String((function(){${strict ? '"use strict";' : ""}${code}\n}).call(undefined))${CATCH}`;
const both = code => { add(fn(code, false)); add(fn(code, true)); };
// Compila o corpo por `new Function` para que SyntaxError vire mensagem.
const viaFunction = (code, strict) => fn(`return new Function(${q((strict ? '"use strict";' : "") + code)})()`, false);
const bothFunction = code => { add(viaFunction(code, false)); add(viaFunction(code, true)); };
// Script de topo: declarações fora de try, sonda dentro.
const top = (decl, probe, strict) => `${strict ? '"use strict";\n' : ""}${decl}\ntry{globalThis.R=String(${probe})${CATCH}`;

// ---- 1. Captura em laços.
const loopKinds = [
  "for(let i=0;i<3;i++)", "for(var i=0;i<3;i++)", "for(const i of [0,1,2])", "for(let i of [0,1,2])", "for(var i of [0,1,2])",
  "for(const i in {a:1,b:1,c:1})", "for(let i in {a:1,b:1,c:1})", "for(var i in {a:1,b:1,c:1})", "for(let i=0,j=10;i<3;i++,j--)",
  "for(let [i]=[0];i<3;i++)", "for(let {i}={i:0};i<3;i++)", "for(const [i] of [[0],[1],[2]])", "for(var [i] of [[0],[1],[2]])",
  "for(let i=3;i>0;i--)",
];
const loopBodies = [
  "fs.push(()=>i);", "fs.push(()=>i);i++;", "fs.push(()=>i=i+10);", "if(i==1)continue;fs.push(()=>i);",
  "fs.push(()=>i);if(i==1)continue;i+=0;", "fs.push(()=>i);if(i==1)break;", "fs.push(()=>i++);", "let k=i;fs.push(()=>k);",
  "fs.push(function(){return i});", "fs.push(()=>()=>i);", "var k=i;fs.push(()=>k);", "fs.push(()=>i);i=i;",
];
const loopTails = ["", "+'|'+typeof i"];
for (const kind of loopKinds) for (const body of loopBodies) for (const tail of loopTails) {
  both(`var fs=[];${kind}{${body}}var a=fs.map(f=>typeof f()==='function'?'fn':f()).join();var b=fs.map(f=>typeof f()==='function'?'fn':f()).join();return a+'/'+b${tail}`);
}
for (const body of [
  "var fs=[];for(let i=0;i<3;fs.push(()=>i),i++);return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;fs.push(()=>i),i<3;i++);return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0,g=()=>i;i<3;i++)fs.push(g);return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0,g=()=>i;i<3;i++){fs.push(g);g=()=>i}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0,g=()=>i;i<2;i++,g=()=>i)fs.push(g);return fs.map(f=>f()).join()",
  "var fs=[];for(var i=0,g=()=>i;i<3;i++)fs.push(g);return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;fs.length<3;i++)fs.push(()=>i++);return fs.map(f=>f()).join()+'|'+fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);i++}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<4;i++){if(i%2)continue;fs.push(()=>i)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);for(let i=10;i<11;i++)fs.push(()=>i)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){var i2=i;fs.push(()=>i2)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);var i;}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<2;i++){let i=5;fs.push(()=>i)}return fs.map(f=>f()).join()",
  "var fs=[];for(const x of [1,2]){for(const y of [10,20])fs.push(()=>x+y)}return fs.map(f=>f()).join()",
  "var fs=[];for(let x of [1,2]){x*=2;fs.push(()=>x)}return fs.map(f=>f()).join()",
  "var fs=[];for(let x in {a:1,b:2}){x+='!';fs.push(()=>x)}return fs.map(f=>f()).join()",
  "var fs=[];l:for(let i=0;i<3;i++){for(let j=0;j<3;j++){if(j==1)continue l;fs.push(()=>i+''+j)}}return fs.map(f=>f()).join()",
  "var fs=[];l:for(let i=0;i<3;i++){for(let j=0;j<3;j++){if(j==2)break l;fs.push(()=>i+''+j)}}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){try{if(i==1)continue;fs.push(()=>i)}finally{fs.push(()=>-i)}}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){try{throw i}catch(i){fs.push(()=>i)}}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){switch(i){case 1:continue;default:fs.push(()=>i)}}return fs.map(f=>f()).join()",
  "var fs=[];for(let i of [1,2,3]){fs.push(()=>i);if(i==2)break}return fs.map(f=>f()).join()",
  "var fs=[];var it={[Symbol.iterator](){var n=0;return{next(){return{done:n>=3,value:n++}},return(){fs.push(()=>'ret');return{}}}}};for(let v of it){fs.push(()=>v);if(v==1)break}return fs.map(f=>f()).join()",
  "var fs=[];for(let [a,b=()=>a] of [[1],[2]])fs.push(b);return fs.map(f=>f()).join()",
  "var fs=[];for(let {a,b=()=>a} of [{a:1},{a:2}])fs.push(b);return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++)fs.push(()=>i);return fs[0]()+','+fs[1]()+','+fs[2]()",
  "var fs=[];for(let i=(fs.push(()=>i),0);i<2;i++);return fs[0]()",
  "var fs=[];for(let x of [(()=>typeof x)()]);return 1",
  "var fs=[];for(let x of [x]);return 1",
  "var fs=[];for(let x in {x});return 1",
  "var fs=[];for(let i=i;i<1;i++);return 1",
  "var fs=[];for(var i=0;i<3;i++){fs.push(()=>i)}var r=fs.map(f=>f()).join();i=9;return r+'|'+fs[0]()",
  "var fs=[];var n=0;while(n<3){let k=n++;fs.push(()=>k)}return fs.map(f=>f()).join()",
  "var fs=[];var n=0;while(n<3){const k=n++;fs.push(()=>k+n)}return fs.map(f=>f()).join()",
  "var fs=[];var n=0;do{let k=n;fs.push(()=>k);k++}while(++n<3);return fs.map(f=>f()).join()",
  "var fs=[];var n=0;do{var k=n;fs.push(()=>k)}while(++n<3);return fs.map(f=>f()).join()",
  "var fs=[];var n=0;while(n<3){let k=n++;if(k==1)continue;fs.push(()=>k)}return fs.map(f=>f()).join()",
  "var fs=[];for(;;){let k=fs.length;fs.push(()=>k);if(fs.length==3)break}return fs.map(f=>f()).join()",
  "var fs=[];{let i=0;fs.push(()=>i);i=5}{let i=1;fs.push(()=>i)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>i)}var r=[];for(let f of fs){r.push(f())}return r.join()",
  "function mk(){var fs=[];for(let i=0;i<3;i++)fs.push(()=>i);return fs}var a=mk(),b=mk();return a[1]()+','+b[2]()",
  "var fs=[];for(let i=0;i<3;i++)fs.push(eval('()=>i'));return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++)fs.push(new Function('return typeof i'));return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>eval('i'))}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){eval('var j=i');fs.push(()=>j)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){eval('let j=i');fs.push(()=>typeof j)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(()=>this===undefined)}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){fs.push(function(){return arguments.length})}return fs.map(f=>f(1,2)).join()",
]) both(body);

// ---- 2. let/const/class/function em switch.
const switchDecls = ["let x=1;", "const x=1;", "class x{}", "function x(){return 1}", "var x=1;", "let x;"];
const switchAccess = ["typeof x", "x", "(()=>typeof x)()", "(()=>x)()"];
for (const decl of switchDecls) for (const entry of [1, 2, 3, 4]) for (const acc of switchAccess) {
  both(`switch(${entry}){case 1:${decl}break;case 2:return ${acc};case 3:${decl}return ${acc};default:return ${acc}}return 'end'`);
}
bothFunction("switch(1){case 1:let a;case 2:let a}");
bothFunction("switch(1){case 1:let a;break;case 2:var a}");
bothFunction("switch(1){case 1:function a(){}break;case 2:function a(){}}");
bothFunction("switch(1){case 1:const a=1;break;default:function a(){}}");
bothFunction("switch(1){case 1:class a{}break;case 2:class a{}}");
bothFunction("switch(1){case 1:let a=1;break;case 2:{let a=2}}return 'ok'");
for (const body of [
  "var fs=[];switch(2){case 1:let x=1;fs.push(()=>x);case 2:fs.push(()=>typeof x);}return fs.map(f=>f()).join()",
  "var fs=[];for(var i=0;i<3;i++){switch(i){case 0:let x=i;fs.push(()=>x);break;case 1:fs.push(()=>typeof x);break;default:fs.push(()=>1)}}return fs.map(f=>{try{return f()}catch(e){return e.name}}).join()",
  "var r=[];for(var i=0;i<2;i++){switch(1){case 1:let x;r.push(x);x=5;}}return r.join()",
  "var r=[];switch(0){case (()=>{r.push(typeof x);return 0})():let x=1;}return r.join()",
  "var r=[];switch(typeof x){case 'undefined':r.push(1);case 'x':let x}return r.join()",
  "switch(1){case 1:let x=1;{var y=x}}return y",
  "var r=[];switch(1){default:r.push(typeof f);function f(){}}return r.join()+typeof f",
  "var r=[];switch(2){case 1:function f(){return 1}case 2:r.push(typeof f)}return r.join()+typeof f",
  "var r=[];r.push(typeof f);switch(0){case 1:function f(){}}return r.join()+typeof f",
  "var r=[];switch(1){case 1:{let x=2;r.push(x)}case 2:r.push(typeof x)}return r.join()",
]) both(body);

// ---- 3. Funções em blocos (Annex B) sloppy x strict.
const abPrior = ["", "var f=0;", "let f=0;", "function f(){return 2}", "f=5;", "const f=0;", "class f{}", "var f;"];
const abContexts = [
  b => `{${b}}`, b => `if(1)${b}`, b => `switch(1){case 1:${b}}`, b => `if(0){}else{${b}}`, b => `try{${b}}catch(e){}`, b => `L:{${b}}`,
  b => `for(var z=0;z<1;z++){${b}}`, b => `{{${b}}}`,
];
const abProbes = [
  "typeof f", "[typeof f,(function(){return typeof f})()].join()", "(typeof f==='function'?f():f)", "f===undefined",
];
for (const prior of abPrior) abContexts.forEach(ctx => abProbes.forEach(probe => {
  const block = ctx("function f(){return 1}");
  both(`var before=typeof f;${prior}${block}return before+'|'+${probe}`);
}));
for (const body of [
  "{function f(){return 1}function f(){return 2}}return f()",
  "{function f(){return 1}}{function f(){return 2}}return f()",
  "{function f(){return 1}f=3}return typeof f",
  "{function f(){return 1}f=3;}return typeof f",
  "{f=3;function f(){return 1}}return typeof f",
  "{function f(){return 1}var g=f}return typeof f+typeof g",
  "var r=[];{r.push(typeof f);function f(){}}r.push(typeof f);return r.join()",
  "var r=[];if(1)function f(){return 1}r.push(typeof f);return r.join()",
  "var r=[];if(0)function f(){return 1}r.push(typeof f);return r.join()",
  "var r=[];{function f(){return 1}}{let f=2;r.push(f)}r.push(typeof f);return r.join()",
  "var r=[];function g(f){{function f(){}}r.push(typeof f)}g(1);return r.join()",
  "var r=[];function g(f){{function f(){}}r.push(typeof f)}g(1);r.push(typeof f);return r.join()",
  "var r=[];function g(){let f=1;{function f(){}}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){{function f(){return 1}}{function f(){return 2}}r.push(f())}g();return r.join()",
  "var r=[];function g(){{function arguments(){}}r.push(typeof arguments)}g();return r.join()",
  "var r=[];function g(a=1){{function a(){}}r.push(typeof a)}g();return r.join()",
  "var r=[];function g(a){{function a(){}}r.push(typeof a)}g(1);return r.join()",
  "var r=[];function g(){var f=1;{function f(){}}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){try{throw 1}catch(f){{function f(){}}r.push(typeof f)}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){for(let f of [1]){{function f(){}}}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){switch(1){case 1:function f(){return 1};case 2:function f(){return 2}}r.push(f())}g();return r.join()",
  "var r=[];function g(){L:function f(){}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){{L:function f(){}}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){(function(){{function h(){}}r.push(typeof h)})();r.push(typeof h)}g();return r.join()",
  "var r=[];function g(){{function* f(){}}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){{async function f(){}}r.push(typeof f)}g();return r.join()",
  "var r=[];function g(){{function f(){}}r.push(typeof f)}g();r.push(typeof g.f);return r.join()",
  "var r=[];{function f(){}}r.push(typeof globalThis.f);return r.join()",
  "var r=[];{function f(){return 1}}var o=f;{function f(){return 2}}r.push(o()+f());return r.join()",
  "var r=[];eval('{function f(){}}');r.push(typeof f);return r.join()",
  "var r=[];eval('var q=1;{function f(){}}');r.push(typeof f,typeof q);return r.join()",
  "var r=[];(0,eval)('{function ff(){}}');r.push(typeof ff);return r.join()",
  "var r=[];(0,eval)('r2=typeof ff;{function ff(){}}');r.push(typeof ff,typeof globalThis.r2);return r.join()",
]) both(body);
bothFunction("let f;{function f(){}}return 1");
bothFunction("{let f;{function f(){}}}return 1");
bothFunction("{function f(){}let f}return 1");
bothFunction("{function f(){}var f}return 1");
bothFunction("{var f;function f(){}}return 1");
bothFunction("if(1)function f(){}else function g(){}return typeof f+typeof g");
bothFunction("while(0)function f(){}return 1");
bothFunction("for(;0;)function f(){}return 1");
bothFunction("if(1)class f{}return 1");
bothFunction("if(1)let f;return 1");
bothFunction("if(1)function* f(){}return 1");
bothFunction("L:function* f(){}return 1");
bothFunction("do function f(){};while(0);return 1");
bothFunction("with({})function f(){}return 1");

// ---- 4. Shadowing de parâmetros por var/function.
const pParams = ["a", "a=1", "a,b", "a,b=a", "...a", "[a]", "{a}", "a=()=>a", "a,a"];
const pBodies = ["", "var a;", "var a=2;", "function a(){}", "var a;function a(){}", "{function a(){}}", "var a=a+1;", "a=9;", "let b;", "var arguments;", "function arguments(){}"];
const pRets = ["typeof a", "String(a)", "typeof arguments[0]", "arguments.length"];
for (const params of pParams) for (const body of pBodies) for (const ret of pRets) {
  const fnSrc = `function f(${params}){${body}return ${ret}}`;
  both(`${fnSrc}var c=function(){try{return String(f.apply(null,arguments))}catch(e){return e.name}};return c(1)+'|'+c([5])+'|'+c()`);
}
for (const src of ["function f(a){let a}", "function f(a){const a=1}", "function f(a){class a{}}", "function f(a=1){let a}", "function f([a]){let a}", "function f(...a){let a}",
  "function f(a){{let a}}", "function f(a){var a;let b;{let a}}", "function f(){let a;var a}", "function f(){var a;let a}", "function f(){let a;function a(){}}",
  "function f(){function a(){}let a}", "function f(){function a(){}var a;function a(){}}", "function f(){let a;{var a}}", "function f(){{var a}let a}",
  "(a)=>{let a}", "(a)=>{var a}", "(a=1)=>{var a}", "function f(a,a){}", "(a,a)=>1", "function f(a,a=1){}", "function f(a,[a]){}", "function f(a,...a){}",
  "'use strict';function f(a,a){}", "function f(a,a){'use strict'}", "function f(a=1){'use strict'}", "function f(a){'use strict';var a}"]) {
  bothFunction(`${src};return 'ok'`);
}

// ---- 5. Parâmetros com default: escopo separado do corpo.
const dParams = [
  "a=1,g=()=>a", "g=()=>a,a=1", "g=()=>x", "g=()=>typeof arguments", "b=arguments.length", "b=eval('1')", "b=eval('var z=3;z')",
  "g=()=>z", "a=arguments[0],g=()=>a", "a=1,b=a+1,g=()=>b",
];
const dBodies = ["var a=2;", "var a;", "a=2;", "var z=7;", "function a(){}", "var arguments;", "var arguments=5;", "let q=1;", "var x=5;", "var b=9;"];
const dRets = ["[typeof a,typeof g==='function'?g():typeof g,typeof b].join()", "[a,typeof z,typeof x].join()", "[typeof g==='function'?g():g,typeof arguments,arguments.length].join()"];
for (const params of dParams) for (const body of dBodies) for (const ret of dRets) {
  const src = `var x='out';function f(${params}){${body}return ${ret}}`;
  both(`${src};var c=function(){try{return String(f.apply(null,arguments))}catch(e){return e.name+':'+e.message}};return c()+'|'+c(7)`);
}
for (const body of [
  "var x=1;function f(a=x){var x=2;return a}return f()",
  "var x=1;function f(a=x){let x=2;return a}return f()",
  "var x=1;function f(a=()=>x){var x=2;return a()}return f()",
  "var x=1;function f(a=()=>x){x=2;return a()}return f()+','+x",
  "function f(a=()=>b,b=1){return a()}return f()",
  "function f(a=b,b=1){return a}return f()",
  "function f(a=a){return a}return f()",
  "function f(a=typeof a){return a}return f()",
  "function f(a=()=>typeof a){return a()}return f()",
  "function f(a=1,b=()=>{a=5}){var a;b();return a}return f()",
  "function f(a=1,b=()=>{a=5}){b();return a}return f()",
  "function f(a=1,b=()=>{a=5}){var a=a;b();return a}return f()",
  "function f(a=1,b=()=>a){var a=3;return b()}return f()",
  "function f(a=1,b=()=>a){a=3;return b()}return f()",
  "function f(a,b=()=>a){var a=3;return b()}return f(1)",
  "function f(a,b=()=>a){var a;return a+','+b()}return f(1)",
  "function f(a,b=()=>a){var a;a=4;return a+','+b()}return f(1)",
  "function f(a=this){return a===undefined}return f()",
  "function f(a=this){return typeof a}return f.call(1)",
  "function f(a=new.target){return typeof a}return f()",
  "function f(a=arguments){return a===arguments}return f()",
  "function f(a=arguments){return a.length}return f(undefined,2,3)",
  "function f(a=arguments[1]){return a}return f(undefined,2,3)",
  "function f(a=1){arguments[0]=5;return a}return f(2)",
  "function f(a){arguments[0]=5;return a}return f(2)",
  "function f(a){a=5;return arguments[0]}return f(2)",
  "function f(a=0){a=5;return arguments[0]}return f(2)",
  "function f(a,b){a=5;return arguments[0]+','+arguments.length}return f(2)",
  "function f(a,b){b=5;return arguments[1]+','+arguments.length}return f(2)",
  "function f(a){'use strict';a=5;return arguments[0]}return f(2)",
  "function f(...a){a[0]=5;return arguments[0]}return f(2)",
  "function f(a=1,...r){return arguments.length+','+r.length}return f(undefined,2,3)",
  "function f(a,b=eval('a')){return b}return f(4)",
  "function f(a,b=eval('var a=9;a')){return a+','+b}return f(4)",
  "function f(a,b=eval('var c=9;c')){return typeof c+','+b}return f(4)",
  "function f(b=eval('var c=9;c')){return typeof c+','+b}return f()",
  "function f(b=eval('var c=9;c')){var c;return typeof c+','+b+c}return f()",
  "function f(b=eval('var c=9;c'),d=()=>c){return d()}return f()",
  "function f(b=eval('var c=9;c')){return eval('typeof c')}return f()",
  "function f(a=eval('var arguments=1')){}return f()",
  "function f(a=eval('arguments')){return typeof a}return f()",
  "var f=(a,b=()=>a)=>{var a=2;return b()};return f(1)",
  "var f=(a=()=>b)=>{var b=2;return typeof a()};return f()",
  "var f=({a},b=()=>a)=>{var a=2;return b()+','+a};return f({a:1})",
  "var f=([a],b=()=>a)=>{var a;return b()+','+a};return f([1])",
  "var o={m(a=()=>super.x){return a()}};o.__proto__={x:5};return o.m()",
  "class C{constructor(a=()=>this){this.f=a}}var c=new C();return c.f()===c",
  "class B{}class C extends B{constructor(a=()=>this){super();this.f=a}}var c=new C();return c.f()===c",
  "class B{}class C extends B{constructor(a=this){super()}}return new C()",
  "function f(a=1){function a(){}return typeof a}return f()",
  "function f(a=1){var a;return typeof a}return f()",
  "function f(a=1){{function a(){}}return typeof a}return f()",
  "function f(a=()=>1){function a(){return 2}return a()}return f()",
  "function f(g=()=>a){var a=1;return g()}return f()",
  "var a='out';function f(g=()=>a){var a=1;return g()}return f()",
]) both(body);

// ---- 6. Parâmetro de catch com destructuring e redeclaração.
const catchParams = [
  ["e", "5"], ["{e}", "{e:1,a:2,b:{b:7},c:3}"], ["{a,b}", "{e:1,a:2,b:{b:7},c:3}"], ["[a,b]", "[1,2]"], ["{a=1}", "{}"], ["[a=5]", "[]"],
  ["{a:{b}}", "{a:{b:7}}"], ["{...a}", "{x:1,y:2}"], ["[...a]", "[1,2,3]"],
];
const catchBodies = [
  "", "var e=2;", "var a=9;", "function a(){}", "e=3;", "{var e=4}", "for(var e in {k:1});", "var b=1;", "a=8;", "{let a=4}", "let c=1;", "var e;",
  "for(var e of [1]);", "try{throw 0}catch(e){var e=6}", "(()=>{e=7})();", "function e(){}", "{function a(){}}",
];
for (const [param, thrown] of catchParams) for (const body of catchBodies) {
  bothFunction(`var out=[];try{throw ${thrown}}catch(${param}){${body}out.push(typeof e,typeof a,typeof b)}out.push(typeof e,typeof a,typeof b);return out.join()`);
}
for (const body of [
  "var r=[];try{throw 1}catch(e){var e=2;r.push(e)}r.push(e);return r.join()",
  "var r=[];try{throw 1}catch(e){var e;r.push(e)}r.push(e);return r.join()",
  "var r=[];try{throw 1}catch(e){e=2;var e=3;r.push(e)}r.push(e);return r.join()",
  "var r=[];try{throw 1}catch(e){(function(){var e=5})();r.push(e)}return r.join()",
  "var r=[];try{throw 1}catch(e){eval('var e=5');r.push(e)}r.push(e);return r.join()",
  "var r=[];try{throw 1}catch(e){eval('var z=5');r.push(z)}r.push(z);return r.join()",
  "var r=[];try{throw 1}catch(e){eval('e=5');r.push(e)}return r.join()",
  "var r=[];try{throw 1}catch(e){eval('let e=5')}return r.join()",
  "var r=[];try{throw 1}catch({e}){}return r.join()",
  "var r=[];try{throw {e:1}}catch({e}){var f=()=>e;r.push(f())}r.push(typeof e);return r.join()",
  "var fs=[];for(var i=0;i<3;i++){try{throw i}catch(e){fs.push(()=>e)}}return fs.map(f=>f()).join()",
  "var fs=[];for(var i=0;i<3;i++){try{throw i}catch(e){fs.push(()=>e);e=e*2}}return fs.map(f=>f()).join()",
  "var fs=[];for(let i=0;i<3;i++){try{throw [i]}catch([e]){fs.push(()=>e)}}return fs.map(f=>f()).join()",
  "var r=[];try{throw 1}catch(e){try{throw 2}catch(e){r.push(e)}r.push(e)}return r.join()",
  "var r=[];try{throw 1}catch(e){try{throw 2}catch(f){r.push(e,f)}}return r.join()",
  "var r=[];try{throw 1}catch(e){var g=()=>e}try{throw 2}catch(e){r.push(g())}return r.join()",
  "var r=[];try{throw 1}catch(e){r.push(typeof e)}r.push(typeof e);return r.join()",
  "var r=[];try{throw undefined}catch(e){r.push(String(e))}return r.join()",
  "var r=[];try{throw 1}catch{r.push(typeof e)}return r.join()",
  "var r=[];try{throw {a:1}}catch({a=(()=>b)(),b=2}){r.push(a)}return r.join()",
  "var r=[];try{throw {}}catch({a=b,b=2}){r.push(a)}return r.join()",
  "var r=[];try{throw {}}catch({a=(()=>a)()}){r.push(a)}return r.join()",
  "var r=[];try{throw {}}catch({a,b=()=>a}){var a2=a;r.push(typeof b())}return r.join()",
  "var r=[];try{throw null}catch({a}){r.push(a)}return r.join()",
  "var r=[];try{try{throw null}catch({a}){r.push(a)}}catch(e2){r.push(e2.name)}return r.join()",
  "var r=[];try{throw 1}catch(e){r.push((function(){return typeof e})())}return r.join()",
  "var r=[];try{throw 1}catch(e){r.push((function(e){return typeof e})())}return r.join()",
  "var r=[];try{throw 1}catch(e){let x=e;r.push(x)}return r.join()+typeof x",
  "var r=[];try{throw 1}catch(e){function g(){return e}r.push(g())}r.push(typeof g);return r.join()",
]) both(body);

// ---- 7. Binding interno de class e função.
const classForms = [
  s => `class C{static m(){${s}}}var K=C;`, s => `var K=class C{static m(){${s}}};`, s => `var K=class C{static m(){${s}}};var C=0;`,
  s => `var K=class C{static x=(()=>{${s}})();static m(){return 1}};K=K;K.m=()=>C.x;var C0=1;`,
];
const funcForms = [
  s => `function C(){${s}}var K=C;`, s => `var K=function C(){${s}};`, s => `var K=function C(){${s}};var C=0;`,
  s => `var K=function*C(){${s}};`, s => `var K=function C(C){${s}};`, s => `var K=function C(a=C){${s}};`, s => `var K=function C(){var C=7;${s}};`,
  s => `var K=function C(){let C=7;${s}};`, s => `var K=function C(){function C(){}${s}};`,
];
const innerStmts = [
  "C=1;return typeof C", "C=1;return C===K", "var C=1;return typeof C", "let C=2;return C", "return typeof C", "try{C=1}catch(e){return e.name}return typeof C",
  "(()=>{C=1})();return typeof C", "return (()=>C)()===K", "{let C=3;return C}", "return eval('typeof C')", "eval('C=1');return typeof C",
  "eval('var C=1');return typeof C", "return typeof C+typeof arguments.callee", "C++;return typeof C", "return delete C", "return [C===K,C===undefined].join()",
];
classForms.forEach(form => innerStmts.forEach(stmt => both(`${form(stmt)}try{return String(typeof K.m==='function'&&K.m!==undefined&&!(K.m.length>9)?K.m():K())}catch(e){return e.name+': '+e.message}`)));
funcForms.forEach(form => innerStmts.forEach(stmt => both(`${form(stmt)}var res=K();try{var g=K;res=typeof res==='object'&&res!==null&&typeof res.next==='function'?res.next().value:res;return String(res)}catch(e){return e.name+': '+e.message}`)));
for (const body of [
  "class C{static x=C;static m(){return C}}return C.x===C&&C.m()===C",
  "class C{static x=typeof C}return C.x",
  "var C=1;class C2{static x=C}return C2.x",
  "try{class C{static x=C2;}class C2{}}catch(e){return e.name}return 'no'",
  "try{class C extends C{}}catch(e){return e.name}return 'no'",
  "try{class C{[C](){}}}catch(e){return e.name}return 'no'",
  "try{class C{static [C]=1}}catch(e){return e.name}return 'no'",
  "var C=class{};C=1;return typeof C",
  "class C{}C=1;return typeof C",
  "const D=class C{static m(){return typeof C}};return D.m()+typeof C",
  "class C{static{C=1}}return typeof C",
  "class C{static{var C=1}}return typeof C",
  "class C{static m(){C=1}}try{C.m()}catch(e){return e.name+typeof C}return typeof C",
  "class C{static m(){C=1}}var D=C;C=null;try{D.m()}catch(e){return e.name+typeof C}return typeof C",
  "var D=class C{static m(){C=1}};try{D.m()}catch(e){return e.name+typeof C}return typeof C",
  "var D=class C{m(){return C}};var d=new D();return d.m()===D",
  "var D=class C{m(){return C}};var C=5;return new D().m()===D",
  "class C{m(){return typeof C}}var D=C;C=1;return new D().m()",
  "class C{m(){return typeof C}}var D=C;C=undefined;return new D().m()",
  "class C{static f=()=>C}var D=C;C=1;return typeof D.f()",
  "let D;{class C{static f=()=>C}D=C}return typeof D.f()",
  "let D;{class C{}D=C;}return typeof C+typeof D",
  "class C{}{class C{static m(){return 1}}}return typeof C.m",
  "return typeof (class C{}).name+(class C{}).name",
  "var o={C:class{}};return o.C.name",
  "var C=class{};return C.name",
  "var C=class D{};return C.name+typeof D",
  "var C=function(){};return C.name",
  "var C=function D(){};return C.name+typeof D",
  "var C;C=function(){};return C.name",
  "var C;C=class{};return C.name",
  "var o={};o.C=function(){};return JSON.stringify(o.C.name)",
  "var [C=function(){}]=[];return C.name",
  "var {C=class{}}={};return C.name",
  "function f(C=function(){}){return C.name}return f()",
  "var C=(0,function(){});return JSON.stringify(C.name)",
  "var C=(function(){});return C.name",
  "var C=(class{});return C.name",
  "var C=(1,class{});return JSON.stringify(C.name)",
  "var C=class{static name='x'};return C.name",
  "var C=class{static name(){}};return typeof C.name",
  "var C=class{static x=1};return Object.getOwnPropertyNames(C).join()",
  "var C=function(){};return Object.getOwnPropertyNames(C).join()",
  "var C=function D(){};return Object.getOwnPropertyNames(C).join()",
  "var C=()=>1;return Object.getOwnPropertyNames(C).join()",
  "var C=class D{};return Object.getOwnPropertyNames(C).join()",
]) both(body);

// ---- 8. Atribuição ao nome de uma named function expression.
const nfeForms = [
  s => `var K=function C(){${s}};`, s => `var K=function C(C){${s}};`, s => `var K=function C(a=C){${s}};`, s => `var K=function C(){var C;${s}};`,
  s => `var K=function C(){function C(){}${s}};`, s => `var K=function*C(){${s}};`, s => `var K=(function C(){${s}});`, s => `var K={m:function C(){${s}}}.m;`,
  s => `var K={m(){${s}}}.m;var C=0;`, s => `var K=function C(){${s}};var C=0;`, s => `var K=function C(){${s}};K=function(){};`,
];
const nfeStmts = [
  "C=1;return typeof C", "C=1;return C===K", "return typeof C", "C+=1;return typeof C", "C++;return typeof C", "C=1;return delete C",
  "[C]=[1];return typeof C", "({C}={C:1});return typeof C", "for(C of [1]);return typeof C", "for(C in {a:1});return typeof C",
  "C&&=1;return typeof C", "C??=1;return typeof C", "var a=C=2;return typeof C+a",
];
nfeForms.forEach(form => nfeStmts.forEach(stmt => both(`${form(stmt)}var res=K();try{if(res&&typeof res==='object'&&typeof res.next==='function')res=res.next().value;return String(res)}catch(e){return e.name+': '+e.message}`)));

// ---- 9. eval com var em função com parâmetros.
const evParams = ["", "a", "a=1", "a,b=()=>a", "a=eval('var a2=2')"];
const evCode = ["var a=5", "var a", "var b=5", "var a;a=7", "function a(){}", "let a=5", "'use strict';var a=5", "var arguments=3", "var arguments", "eval('var a=8')"];
const evRets = ["a", "typeof a", "typeof b", "(()=>a)()", "arguments[0]"];
for (const params of evParams) for (const code of evCode) for (const ret of evRets) {
  both(`function f(${params}){eval(${q(code)});return String(${ret})}var c=function(){try{return f.apply(null,arguments)}catch(e){return e.name}};return c(1)+'|'+c()`);
}
for (const body of [
  "function f(a=eval('var a=2'),b=()=>a){return b()}return f()",
  "function f(a,b=eval('var a=2')){return a}return f(1)",
  "function f(a,b=eval('var c=2')){return typeof c}return f(1)",
  "function f(a,b=eval('var c=2')){var c;return c}return f(1)",
  "function f(a,b=eval('var c=2')){var c=3;return c}return f(1)",
  "function f(a,b=eval('var c=2'),d=()=>c){return d()}return f(1)",
  "function f(a,b=eval('var c=2')){return eval('c')}return f(1)",
  "function f(a,b=eval('var c=2')){eval('var c=4');return c}return f(1)",
  "function f(){eval('var x=1');return (()=>x)()}return f()",
  "function f(){eval('var x=1');return delete x}return f()+typeof x",
  "function f(){var x=0;eval('var x=1');return x}return f()",
  "function f(){var x=0;eval('var x;');return x}return f()",
  "function f(){let x=0;eval('var x=1');return x}return f()",
  "function f(){{let x=0;eval('var x=1')}return x}return f()",
  "function f(){{let x=0;{eval('var x=1')}}}return f()",
  "function f(){eval('function g(){return 1}');return g()}return f()",
  "function f(){eval('function g(){return 1}');return delete g}return f()",
  "function f(){eval('var arguments=1');return typeof arguments}return f()",
  "function f(){eval('var arguments');return typeof arguments}return f()",
  "function f(){eval('arguments=1');return arguments}return f(5)",
  "function f(){eval('var f=1');return typeof f}return f()",
  "function f(){eval('f=1');return typeof f}return f()",
  "var f=function g(){eval('var g=1');return typeof g};return f()",
  "var f=function g(){eval('g=1');return typeof g};return f()",
  "function f(a){eval('a=2');return arguments[0]}return f(1)",
  "function f(a){eval('arguments[0]=2');return a}return f(1)",
  "function f(a){'use strict';eval('var a=2');return a}return f(1)",
  "function f(a){'use strict';eval('var z=2');return typeof z}return f(1)",
  "function f(a){eval('\"use strict\";var z=2');return typeof z}return f(1)",
  "function f(a){return (0,eval)('typeof a')}return f(1)",
  "function f(a){var e=eval;e('var gz=1');return typeof gz}var r=f(1);return r+typeof globalThis.gz",
  "function f(a){return eval('(function(){return a})')()}return f(1)",
  "function f(a){return eval('(()=>this)')()===this}return f.call(1)===false",
  "var x='out';function f(){eval('var x=1');return x}return f()+x",
  "var x='out';function f(){{eval('var x=1')}return x}return f()+x",
  "var x='out';function f(){return eval('var x=1;x')+x}return f()+x",
  "var x='out';function f(){var g=()=>x;eval('var x=1');return g()}return f()",
  "var x='out';function f(){var g=()=>x;eval('var x=1');return g()+x}return f()",
  "var x='out';function f(a=()=>x){eval('var x=1');return a()}return f()",
  "var x='out';function f(a=()=>x){var x;eval('x=1');return a()+x}return f()",
  "function f(a=1){eval('var a');return a}return f()",
  "function f(a=1){eval('var a=2');return a}return f()",
  "function f(a=1){eval('var b=2');return typeof b}return f()",
  "function f(a=1,g=()=>b){eval('var b=2');return typeof g()}return f()",
  "function f(...r){eval('var r=1');return typeof r}return f()",
  "function f({a}){eval('var a=1');return a}return f({a:5})",
  "var o={m(){eval('var x=1');return x}};return o.m()",
  "var f=()=>{eval('var x=1');return x};return f()",
  "var f=()=>{eval('var x=1')};f();return typeof x",
  "class C{m(){eval('var x=1');return x}}return new C().m()",
  "class C{m(){return eval('super.constructor')}}return typeof new C().m()",
  "class C{static x=eval('1')}return C.x",
  "class C{static x=eval('typeof C')}return C.x",
  "class C{static x=eval('this===C')}return C.x",
  "class C{x=eval('this')}return typeof new C().x",
  "function* g(){eval('var x=1');yield x}return g().next().value",
  "function f(){var r=[];for(let i=0;i<2;i++){eval('var j=i');r.push(()=>j)}return r.map(f=>f()).join()}return f()",
]) both(body);

// ---- 10. with e closures (só sloppy).
const withObjs = [
  "{x:1}", "{x:1,[Symbol.unscopables]:{x:true}}", "{}", "{get x(){return 7}}", "{x:1,y:2}", "Object.create({x:1})", "[1,2]",
  "{x:1,[Symbol.unscopables]:{x:false}}", "{get [Symbol.unscopables](){return {x:true}},x:1}", "'str'",
];
const withBodies = [
  "fs.push(()=>x);", "var x=2;fs.push(()=>x);", "x=3;fs.push(()=>x);", "function g(){return x}fs.push(g);", "delete o.x;fs.push(()=>x);",
  "fs.push(()=>x);delete o.x;", "var y=5;fs.push(()=>y);", "let x=9;fs.push(()=>x);", "fs.push(()=>typeof x);", "fs.push(()=>(x=4));",
  "x++;fs.push(()=>x);", "fs.push(()=>length);", "fs.push(()=>typeof y);y=1;", "var x;fs.push(()=>x);",
];
const withAfter = ["", "o.x=9;", "delete o.x;", "o.y=1;"];
for (const obj of withObjs) for (const body of withBodies) for (const after of withAfter) {
  add(`try{globalThis.R=String((function(){var x='out',y='outy',o=${obj},fs=[];with(o){${body}}${after}var r=fs.map(f=>{try{return String(f())}catch(e){return e.name}});return r.join()+'|'+x+'|'+y+'|'+(typeof o==='object'?Object.keys(o).join():'')}).call(undefined))${CATCH}`);
}
for (const body of [
  "var log=[];var o=new Proxy({x:1},{has(t,k){log.push(typeof k==='symbol'?'sym':k);return k in t}});with(o){x}return log.join()",
  "var log=[];var o=new Proxy({x:1},{has(t,k){log.push(typeof k==='symbol'?'sym':k);return k in t},get(t,k){log.push('get '+(typeof k==='symbol'?'sym':k));return t[k]}});with(o){x}return log.join()",
  "var log=[];var o=new Proxy({x:1},{has(t,k){log.push(typeof k==='symbol'?'sym':k);return k in t},set(t,k,v){log.push('set '+k);t[k]=v;return true}});with(o){x=2}return log.join()",
  "var log=[];var o=new Proxy({x:1},{has(t,k){log.push(typeof k==='symbol'?'sym':k);return k in t}});with(o){typeof x;typeof zz}return log.join()",
  "var log=[];var o=new Proxy({},{has(t,k){log.push(typeof k==='symbol'?'sym':k);return false}});with(o){var x=1}return log.join()+x",
  "var o={f(){return this===o}};with(o){return f()}",
  "var o={f(){return this===o}};with(o){return (f)()}",
  "var o={f(){return this===o}};with(o){return (0,f)()}",
  "var o={f(){return typeof this}};with(o){return eval('f()')}",
  "var o={x:1};with(o){return eval('x')}",
  "var o={x:1};with(o){eval('var x=2')}return o.x",
  "var o={};with(o){eval('var x=2')}return o.x+','+x",
  "var o={x:1};with(o){var f=function(){return x}}o.x=5;return f()",
  "var o={x:1};with(o){var f=()=>x}delete o.x;return typeof f()",
  "var x='out';var o={x:1};with(o){var f=()=>x}delete o.x;return f()",
  "var o={x:1};var f;with(o){f=function g(){return x}}return f()",
  "var o={x:1},r=[];with(o){for(var i=0;i<2;i++)r.push(()=>x+i)}return r.map(f=>f()).join()",
  "var o={x:1},r=[];with(o){for(let i=0;i<2;i++)r.push(()=>x+i)}return r.map(f=>f()).join()",
  "var o={x:1},r=[];for(let i=0;i<2;i++){with(o){r.push(()=>x+i)}o.x++}return r.map(f=>f()).join()",
  "var o={x:1};with(o){with({x:2}){return x}}",
  "var o={x:1};with(o){with({y:2}){return x}}",
  "var o={x:1};with(o){with({y:2}){x=5}}return o.x",
  "var o={x:1};with(o){return (function(){return x})()}",
  "var o={x:1};with(o){return (function(x){return x})(9)}",
  "var o={x:1};with(o){return (function(){var x=9;return x})()+','+o.x}",
  "var o={x:1};with(o){return (function(){var x=9;return x})()}",
  "var o={a:1};with(o){var a=2}return o.a+','+typeof a",
  "var o={};with(o){var a=2}return typeof o.a+','+a",
  "var o={a:1};with(o){function a(){}}return typeof o.a+','+typeof a",
  "var o={a:1};with(o){{function a(){}}}return typeof o.a+','+typeof a",
  "var o={a:1};with(o){let a=2;return a+','+o.a}",
  "var o={get a(){return 1},set a(v){this.b=v}};with(o){a=5}return o.b",
  "var o={a:1};Object.freeze(o);with(o){a=2}return o.a",
  "'use strict';with({}){}",
  "var o={a:1};with(o)a=2,b=3;return o.a+','+typeof b+','+typeof globalThis.b",
  "var o={a:1};with(o){a=(delete o.a,2)}return o.a+','+typeof globalThis.a",
  "var o={a:1};with(o){a=(o.a=7,2)}return o.a",
  "var o={a:1};with(o){var r=a+(delete o.a,a)}return r",
  "var o={a:1};with(o){return typeof a+typeof arguments+typeof this}",
  "var o={arguments:5};with(o){return arguments}",
  "var o={arguments:5};with(o){return typeof arguments}",
  "var o={undefined:5};with(o){return undefined}",
  "var o={eval:function(){return 'fake'}};with(o){return eval('1')}",
  "var o={Array:1};with(o){return typeof Array}",
]) add(fn(body, false));

// ---- 11. TDZ via closures chamadas cedo e typeof.
const tdzDecls = ["let x=1;", "const x=1;", "class x{}", "let x;", "var y=0;let x=y;", "const x=(()=>1)();"];
const tdzAcc = [
  "x", "typeof x", "x=2", "(()=>x)()", "eval('x')", "eval('typeof x')", "[x]=[1]", "x++", "x in {}", "({x})", "x||1", "(x,1)", "void x", "!x",
  "`${x}`", "[...[x]]", "x?1:2", "delete x", "x.y", "new x", "x()", "typeof x===typeof x", "x=x", "x+=1",
];
const tdzWrap = [
  a => `function g(){return ${a}}`, a => `var g=()=>${a};`, a => `function g(p=${a}){return 1}`, a => `var g=function(){return ${a}};`,
];
for (const decl of tdzDecls) for (const acc of tdzAcc) for (const wrap of tdzWrap) {
  both(`var r=[];${wrap(acc)}\ntry{r.push(String(g()))}catch(e){r.push(e.name)}\n${decl}\ntry{r.push(String(g()))}catch(e){r.push(e.name)}return r.join()`);
}
for (const body of [
  "var r=[];{r.push(typeof x);let x}return r.join()",
  "var r=[];try{{r.push(typeof x);let x}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{{r.push(typeof x);const x=1}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{{r.push(typeof x);class x{}}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{{r.push(typeof x);var x}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{{r.push(typeof x);function x(){}}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof x)}catch(e){r.push(e.name)}let x;return r.join()",
  "var r=[];try{r.push(typeof x(()=>x))}catch(e){r.push(e.name)}let x;return r.join()",
  "var r=[];try{r.push(typeof undeclared)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof (undeclared))}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof (0,undeclared))}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof undeclared.x)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof typeof undeclared)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof [undeclared])}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(typeof (undeclared,1))}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{undeclared=1;r.push(typeof undeclared)}catch(e){r.push(e.name)}return r.join()+typeof globalThis.undeclared",
  "var r=[];try{undeclared++}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{undeclared+=1}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{undeclared||=1;r.push(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{undeclared??=1;r.push(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{[undeclared]=[1];r.push(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(undeclared of [1]);r.push(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(undeclared in {a:1});r.push(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{undeclared=undeclared}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{delete undeclared;r.push(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{r.push(delete globalThis.undeclared)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let x=x}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let x=typeof x}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let x=()=>x;r.push(typeof x())}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let [x=x]=[]}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let [x,y=x]=[1];r.push(y)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let [y=x,x]=[]}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let {x=x}={}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{let {x:y=x}={}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{const x=class{static y=x}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{const x=class{y=x}; new x();r.push('ok')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{const x=class{static y=()=>x};r.push(typeof x.y())}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{class x extends x{}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{const x=class extends x{}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{class x{static [x.name]=1}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{class x{[typeof x](){}}r.push('ok')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];function f(){return x}try{f()}catch(e){r.push(e.name)}var x=1;r.push(f());return r.join()",
  "var r=[];function f(){return x}try{f()}catch(e){r.push(e.name)}let x=1;r.push(f());return r.join()",
  "var r=[];function f(){return typeof x}r.push(f());let x=1;r.push(f());return r.join()",
  "var r=[];function f(){return typeof x}try{r.push(f())}catch(e){r.push(e.name)}const x=1;r.push(f());return r.join()",
  "var r=[];for(let i=0;i<2;i++){try{r.push(typeof x)}catch(e){r.push(e.name)}let x=i}return r.join()",
  "var r=[];for(let i=0;i<2;i++){var f=()=>x;try{r.push(f())}catch(e){r.push(e.name)}let x=i;r.push(f())}return r.join()",
  "var r=[];for(let x of [typeof x]){r.push(x)}return r.join()",
  "var r=[];try{for(let x of [x]);}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(let x in {x});r.push('ok')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(let x in x);}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(let x=x;;);}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(const x=typeof x;;)break;r.push('ok')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(let i=0,j=i;i<1;i++)r.push(j)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{for(let i=j,j=0;i<1;i++);}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function(a=b,b){})()}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function(a=b,b){})(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function(a=b,b){r.push(a)})(1,2)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function(a,b=a){r.push(b)})(1)}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function(a=typeof a){r.push(a)})()}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function({a=a}){})({})}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(function([a=b,b]){})([])}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{((a=b,b)=>1)()}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(async function(a=b,b){})().catch(e=>0);r.push('promise')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{function*g(a=b,b){}g();r.push('no throw')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{function*g(){r.push(typeof x);let x}g().next()}catch(e){r.push(e.name)}return r.join()",
  "var r=[];switch(1){case 0:let x;case 1:try{x}catch(e){r.push(e.name)}}return r.join()",
  "var r=[];switch(1){case 0:let x;case 1:try{x=1}catch(e){r.push(e.name)}}return r.join()",
  "var r=[];switch(1){case 0:let x;case 1:r.push(typeof x)}return r.join()",
  "var r=[];try{label:{break label;let x}r.push('ok')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{if(1){r.push(typeof x);const x=1}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{try{throw 1}catch(e){r.push(typeof z);let z}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{try{throw 1}finally{r.push(typeof z);let z}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{with({}){r.push(typeof z);let z}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{with({z:1}){r.push(typeof z);let z}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{with({z:1}){r.push(z);let z}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{eval('r.push(typeof z);let z')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{eval('var f=()=>z;f();let z')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];let z=1;try{eval('r.push(z);let z=2')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];let z=1;try{{r.push(z);let z=2}}catch(e){r.push(e.name)}return r.join()",
  "var r=[];let z=1;{let z=2;r.push(z)}r.push(z);return r.join()",
  "var r=[];let z=1;{r.push((()=>z)());{let z=2}}return r.join()",
  "var r=[];try{new Function('x','let x')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(0,eval)('r.push(typeof qq);let qq')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];try{(0,eval)('var qq;let qq')}catch(e){r.push(e.name)}return r.join()",
  "var r=[];(0,eval)('let zz=1');r.push(typeof zz);return r.join()",
  "var r=[];(0,eval)('var zz=1');r.push(typeof zz);return r.join()",
  "var r=[];(0,eval)('const zz=1;var yy=zz');r.push(typeof zz,typeof yy);return r.join()",
  "var r=[];(0,eval)('class zz{}');r.push(typeof zz);return r.join()",
  "var r=[];(0,eval)('function zz(){}');r.push(typeof zz,typeof globalThis.zz);return r.join()",
]) both(body);

// ---- 12. Globais: let/var/const/class/function no topo, propriedade de globalThis ou não, delete.
const gDecls = [
  "var x=1;", "let x=1;", "const x=1;", "class x{}", "function x(){}", "x=1;", "globalThis.x=1;",
  "Object.defineProperty(globalThis,'x',{value:1,configurable:true});", "var x;", "let x;", "var x=1;var x=2;", "var x=1;function x(){}",
  "var x=1;{function x(){}}", "{function x(){}}", "if(1){function x(){}}", "eval('var x=1');", "(0,eval)('var x=1');", "(0,eval)('let x=1');",
];
const gProbes = [
  "delete x", "delete globalThis.x", "typeof globalThis.x", "'x' in globalThis", "q(Object.getOwnPropertyDescriptor(globalThis,'x'))", "this.x",
  "Object.keys(globalThis).includes('x')", "globalThis.hasOwnProperty('x')", "(0,eval)('typeof x')", "(0,eval)('var x=9;x')+','+x", "(0,eval)('let x=9;x')+','+x",
  "x", "typeof x", "(function(){return x})()", "(function(){x=5;return x})()+','+typeof globalThis.x+','+typeof x", "(()=>{globalThis.x=5;return x})()",
  "Reflect.deleteProperty(globalThis,'x')", "Object.getOwnPropertyNames(globalThis).includes('x')", "this===globalThis&&typeof this.x",
  "(function(){return typeof this.x}).call(globalThis)", "(function(){'use strict';return typeof this})()", "x===globalThis.x",
];
for (const decl of gDecls) for (const probe of gProbes) for (const strict of [false, true]) {
  if (strict && (/^delete x$/.test(probe) || /^x=1;$|eval\('var|eval\('let|eval\('function/.test(decl))) continue;
  add(top(decl.replace(/;?$/, ";"), probe.replace(/\bq\(/g, "JSON.stringify("), strict));
}
// Duas declarações no mesmo script e redeclarações entre scripts por eval indireto.
for (const [a, b] of [["let x;", "var x;"], ["var x;", "let x;"], ["let x;", "let x;"], ["const x=1;", "var x;"], ["class x{}", "var x;"], ["function x(){}", "let x;"], ["let x;", "function x(){}"],
  ["var x;", "function x(){}"], ["function x(){}", "function x(){}"], ["class x{}", "class x{}"], ["const x=1;", "function x(){}"]]) {
  add(`${a}\n${b}\nglobalThis.R='ok'`);
  add(`${a}\ntry{(0,eval)(${q(b)});globalThis.R='ok'}catch(e){globalThis.R=e.name+': '+e.message}`);
  add(`${a}\ntry{globalThis.R=String(new Function(${q(b + ";return typeof x")})())}catch(e){globalThis.R=e.name+': '+e.message}`);
  add(`${a}\n{${b}}\nglobalThis.R='ok'`);
  add(`${a}\n(function(){${b}})();globalThis.R='ok'`);
  add(`${a}\ntry{eval(${q(b)});globalThis.R='ok'}catch(e){globalThis.R=e.name+': '+e.message}`);
}

// ---- 13. Shadowing de built-ins globais.
const builtinNames = ["Array", "Object", "undefined", "NaN", "Infinity", "Math", "JSON", "Symbol", "Promise", "globalThis", "Function", "String", "parseInt", "eval", "Reflect", "Error"];
const shDecls = ["var N=1;", "let N=1;", "const N=1;", "class N{};", "function N(){};", "N=1;", "var N;", "{function N(){}}"];
const shProbes = ["typeof N", "typeof globalThis.N", "globalThis.N===N", "JSON.stringify(Object.getOwnPropertyDescriptor(globalThis,'N')&&Object.getOwnPropertyDescriptor(globalThis,'N').configurable)", "delete globalThis.N"];
for (const name of builtinNames) for (const decl of shDecls) for (const probe of shProbes) {
  for (const strict of [false, true]) {
    if (strict && /^N=1;$/.test(decl)) continue;
    const d = decl.split("N").join(name);
    const p = probe.split("N").join(name).replace("JSON.stringify(Object", name === "JSON" ? "(0,globalThis.JSON).stringify(Object" : "JSON.stringify(Object");
    if (name === "JSON" && !/^\(0,globalThis/.test(p) && /JSON\.stringify/.test(p)) continue;
    add(top(d, p, strict));
  }
}
const localForms = [
  n => `function f(){var ${n}=1;return typeof ${n}}`, n => `function f(${n}){return typeof ${n}}`, n => `function f(){let ${n}=1;return typeof ${n}}`,
  n => `function f(){function ${n}(){}return typeof ${n}}`, n => `function f(){return typeof ${n}}`, n => `function f(){var ${n};return typeof ${n}}`,
  n => `function f(${n}=2){return typeof ${n}}`, n => `function f(){try{throw 1}catch(${n}){return typeof ${n}}}`, n => `function f(){class ${n}{}return typeof ${n}}`,
  n => `function f(){eval('var ${n}=1');return typeof ${n}}`, n => `function f(){{function ${n}(){}}return typeof ${n}}`, n => `function f(){var ${n}=1;return typeof globalThis.${n}}`,
  n => `function f(){var ${n}=1;return (0,eval)('typeof ${n}')}`, n => `function f(){${n}=1;return typeof globalThis.${n}}`,
];
for (const name of builtinNames) for (const form of localForms) both(`${form(name)}return String(f(2))`);

// ---- Execução.
const baseDir = path.join(__dirname, "..", "tests", "golden");
let baseText = "";
for (const file of ["scope_bun.tsv", "sloppy_bun.tsv", "eval_scope_bun.tsv", "eval_forin_bun.tsv", "completion_order_bun.tsv", "annexb_bun.tsv", "sloppy_syntax_bun.tsv"]) {
  try {
    for (const line of fs.readFileSync(path.join(baseDir, file), "utf8").split("\n")) {
      if (!line) continue;
      try { baseText += JSON.parse(line.split("\t")[0]) + "\n\u0000\n"; } catch (e) {}
    }
  } catch (e) {}
}
const seen = new Set();
const unique = programs.filter(p => !seen.has(p) && seen.add(p));
const runOne = source => new Promise(resolve => {
  const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
  let out = "";
  const timer = setTimeout(() => { child.kill("SIGKILL"); }, 10000);
  child.stdout.on("data", d => { out += d; });
  child.on("close", code => { clearTimeout(timer); resolve(code === 0 ? out : null); });
  child.stdin.end(source);
});
(async () => {
  const results = new Array(unique.length);
  let next = 0;
  let dup = 0;
  const worker = async () => {
    while (true) {
      const i = next++;
      if (i >= unique.length) return;
      const body = unique[i];
      if (body.length > 24 && baseText.includes(body)) { dup++; results[i] = undefined; continue; }
      results[i] = await runOne(body);
      if (results[i] === null) process.stderr.write("sem resultado: " + q(body).slice(0, 160) + "\n");
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  let kept = 0, dropped = 0;
  const lines = [];
  unique.forEach((body, i) => {
    const result = results[i];
    if (result === undefined) return;
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; return; }
    kept++;
    lines.push(q(body) + "\t" + q(result));
  });
  process.stdout.write(emitFactoredLines("scope_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
