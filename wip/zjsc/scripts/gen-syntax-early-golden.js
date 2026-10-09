// Gera tests/golden/syntax_early_bun.tsv: SyntaxError e early errors em grade sistemática, mensagem exata medida no bun.
// Cobre redeclaração (let/const/var/function/class em bloco, switch, catch, cabeça de for, parâmetros), "use strict" com
// parâmetros não simples, labels, break/continue/return fora de contexto, palavras reservadas e contextuais como
// identificador (sloppy, strict, generator, async), new.target e super fora de contexto, regex literais inválidos com
// flags, números (separadores, octal legado, `08`, `0b2`), escapes de string inválidos, delete em strict, alvos de
// atribuição inválidos, destructuring inválido, optional chaining com template, import/export fora de módulo, await
// top-level em script e membros de classe (duplicados, constructor especial, #privado não declarado).
// Cada programa analisa a fonte via `new Function` (ou `eval` indireto numa lista curta de fontes sem efeito) dentro de
// try/catch e grava `name: message` em `R` (ou "ok"). Programas cuja fonte já aparece em syntax-errors.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-syntax-early-golden.js > tests/golden/syntax_early_bun.tsv
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { writeResultPreload, decodeResult } = require("./golden-prelude.js");

// O filho só avalia o programa; quem imprime `R` é o preload (ver `writeResultPreload` em golden-prelude.js).
if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const progs = [];
const seen = new Set();
// kind: F Function, G GeneratorFunction, A AsyncFunction, AG AsyncGeneratorFunction, E eval indireto.
const CTOR = {
  F: "Function",
  G: "Object.getPrototypeOf(function*(){}).constructor",
  A: "Object.getPrototypeOf(async function(){}).constructor",
  AG: "Object.getPrototypeOf(async function*(){}).constructor",
};
function add(src, kind = "F") {
  const key = kind + "\0" + src;
  if (seen.has(key)) return;
  seen.add(key);
  const body = kind === "E" ? `(0,eval)(${JSON.stringify(src)})` : `new (${CTOR[kind]})(${JSON.stringify(src)})`;
  progs.push({ src, kind, prog: `R="ok";try{${body}}catch(e){R=e.name+": "+e.message}` });
}
// Sloppy e strict para a mesma fonte.
function both(src, kind = "F") {
  add(src, kind);
  add("'use strict';" + src, kind);
}

// ---- 1. Redeclaração: A seguido de B em vários contextos.
const decls = [
  "var x", "let x", "const x=1", "function x(){}", "class x{}", "async function x(){}", "function* x(){}", "async function* x(){}",
  "var x=1", "let x=1", "var [x]=[]", "let {x}={}", "const {x}={}", "let [x]=[]",
];
const ctxs = [
  (a, b) => `${a};${b}`,
  (a, b) => `{${a};${b}}`,
  (a, b) => `{${a}}${b}`,
  (a, b) => `{${b}}${a}`,
  (a, b) => `{${a};{${b}}}`,
  (a, b) => `{${a};{${b}}}${b}`,
  (a, b) => `switch(1){case 1:${a};case 2:${b}}`,
  (a, b) => `switch(1){case 1:${a};default:{${b}}}`,
  (a, b) => `switch(1){case 1:${a}}${b}`,
  (a, b) => `try{}catch(x){${a}}`,
  (a, b) => `try{}catch(x){${b};${a}}`,
  (a, b) => `try{}catch([x]){${a}}`,
  (a, b) => `try{}catch({x}){${a}}`,
  (a, b) => `try{}catch(e){${a};${b}}`,
  (a, b) => `try{${a}}catch(x){${b}}`,
  (a, b) => `try{${a}}finally{${b}}`,
  (a, b) => `for(let x;;){${a};break}`,
  (a, b) => `for(let x of []){${a}}`,
  (a, b) => `for(const x in {}){${a}}`,
  (a, b) => `for(var x;;){${a};break}`,
  (a, b) => `for(let x;;){var x;break}`,
  (a, b) => `for(let x of []){{${a}}}`,
  (a, b) => `for(var x of []){${a};${b}}`,
  (a, b) => `function f(x){${a}}`,
  (a, b) => `function f(x){${a};${b}}`,
  (a, b) => `function f(x){{${a}}}`,
  (a, b) => `function f(){${a};${b}}`,
  (a, b) => `(function(x){${a}})`,
  (a, b) => `(x)=>{${a}}`,
  (a, b) => `(x,y)=>{${a};${b}}`,
  (a, b) => `({m(x){${a}}})`,
  (a, b) => `class C{m(x){${a}}}`,
  (a, b) => `function f(x=1){${a}}`,
  (a, b) => `function f([x]){${a}}`,
  (a, b) => `label:${a};${b}`,
  (a, b) => `if(1){${a}}else{${b}}${a}`,
  (a, b) => `if(1)${b};${a}`,
];
for (const a of decls) for (const b of decls) for (const c of ctxs) both(c(a, b));
// Variações de função em bloco (Annex B) e escopo de parâmetros.
for (const n of ["x", "a", "arguments", "eval", "f"]) {
  both(`{function ${n}(){}function ${n}(){}}`);
  both(`{function ${n}(){}var ${n}}`);
  both(`{var ${n};function ${n}(){}}`);
  both(`{function ${n}(){}let ${n}}`);
  both(`{async function ${n}(){}function ${n}(){}}`);
  both(`{function* ${n}(){}function ${n}(){}}`);
  both(`switch(1){case 1:function ${n}(){}case 2:function ${n}(){}}`);
  both(`switch(1){case 1:function ${n}(){}case 2:var ${n}}`);
  both(`function f(${n}){let ${n}}`);
  both(`function f(${n}){const ${n}=1}`);
  both(`function f(${n}){var ${n}}`);
  both(`function f(${n}){function ${n}(){}}`);
  both(`function f(${n}){class ${n}{}}`);
  both(`function f(${n},${n}){}`);
  both(`function f(${n},${n}=1){}`);
  both(`function f(${n}=1,${n}){}`);
  both(`function f({${n}},${n}){}`);
  both(`function f(...${n},${n}){}`);
  both(`function f(...${n}){var ${n}}`);
  both(`function f(...${n}){let ${n}}`);
  both(`(${n},${n})=>1`);
  both(`(${n},[${n}])=>1`);
  both(`(${n},${n}=1)=>1`);
  both(`(...${n},${n})=>1`);
  both(`({m(${n},${n}){}})`);
  both(`({m(${n},${n}){}})`);
  both(`class C{m(${n},${n}){}}`);
  both(`({set p(${n}){let ${n}}})`);
  both(`({set p(${n}){var ${n}}})`);
  both(`(function ${n}(${n}){let ${n}})`);
  both(`for(let ${n},${n};;);`);
  both(`for(let [${n},${n}] of []);`);
  both(`for(const ${n}=1,${n}=2;;);`);
  both(`let ${n},${n}`);
  both(`const ${n}=1,${n}=2`);
  both(`var ${n};let ${n}`);
  both(`let ${n};var ${n}`);
  both(`let [${n},${n}]=[]`);
  both(`const {${n},${n}}={}`);
  both(`let {${n}:${n},y:${n}}={}`);
  both(`try{}catch(${n}){let ${n}}`);
  both(`try{}catch(${n}){var ${n}}`);
  both(`try{}catch([${n}]){var ${n}}`);
  both(`try{}catch(${n}){function ${n}(){}}`);
  both(`try{}catch(${n}){for(var ${n} of []);}`);
  both(`try{}catch(${n}){for(var ${n} in {});}`);
  both(`try{}catch(${n}){for(var ${n};;);}`);
  both(`try{}catch(${n}){{var ${n}}}`);
  both(`try{}catch(${n},${n}){}`);
  both(`try{}catch(${n}=1){}`);
  both(`try{}catch([${n},${n}]){}`);
  both(`try{}catch{let ${n};var ${n}}`);
  both(`class ${n}{}class ${n}{}`);
  both(`class ${n}{}var ${n}`);
  both(`class ${n}{}function ${n}(){}`);
  both(`function ${n}(){}class ${n}{}`);
  both(`let ${n};class ${n}{}`);
  both(`class ${n}{static ${n}(){}}`);
  both(`(class ${n}{${n}(){let ${n}}})`);
  both(`label:{var ${n}}let ${n}`);
  both(`{let ${n};{var ${n}}}`);
  both(`{{var ${n}}let ${n}}`);
  both(`let ${n};{var ${n}}`);
  both(`for(let ${n} of [])var ${n};`);
  both(`for(let ${n};;)var ${n};`);
  both(`for(let ${n} in {}){var ${n}}`);
  both(`for(let ${n} of []){let ${n}}`);
  both(`for(const ${n} of []){function ${n}(){}}`);
  both(`for(var ${n} of [])let ${n};`);
  both(`let ${n}=${n}`);
  both(`const ${n}=${n}`);
  both(`let [${n}=${n}]=[]`);
  both(`var ${n};var ${n}`);
  both(`function ${n}(){}function ${n}(){}`);
  both(`function ${n}(){}var ${n}`);
  both(`function ${n}(){}let ${n}`);
  both(`function ${n}(){}const ${n}=1`);
}
for (const kw of ["let", "const", "var", "class", "function", "import", "export", "default", "case"]) {
  both(`let ${kw}=1`);
  both(`const ${kw}=1`);
  both(`var ${kw}=1`);
  both(`function ${kw}(){}`);
  both(`(${kw})=>1`);
  both(`class ${kw}{}`);
}
both("let let");
both("const let=1");
both("let [let]=[]");
both("let {let}={}");
both("for(let let of []);");
both("for(const let in {});");
both("for(let in {});");
both("for(let of []);");
both("for(let.x of []);");
both("for(let of=1;;);");
both("for(let\nof=1;;);");
both("let\nx");
both("let\n[x]=[]");
both("if(1)let\nx");
both("if(1)let x");
both("if(1)const x=1");
both("if(1)class C{}");
both("if(1)function f(){}");
both("if(1)async function f(){}");
both("if(1)function* f(){}");
both("while(1)function f(){}");
both("while(1)let x");
both("do function f(){}while(0)");
both("for(;;)function f(){}");
both("for(;;)class C{}");
both("for(x in y)function f(){}");
both("label:class C{}");
both("label:let x");
both("label:const x=1");
both("label:function* g(){}");
both("label:async function f(){}");
both("label:function f(){}");
both("a:b:function f(){}");
both("if(1)label:function f(){}");
both("while(1)label:function f(){}");
both("with(a)function f(){}");

// ---- 2. "use strict" com parâmetros não simples.
const params = [
  "a=1", "...a", "[a]", "{a}", "a,b=2", "a,...b", "{a},b", "[a],[b]", "a,{b}", "a=1,b", "({a}={})".slice(1, -1), "[a=1]", "{a=1}", "...[a]", "...{a}",
];
const fnForms = [
  (p, b) => `function f(${p}){${b}}`,
  (p, b) => `(function(${p}){${b}})`,
  (p, b) => `(function* (${p}){${b}})`,
  (p, b) => `(async function(${p}){${b}})`,
  (p, b) => `(async function*(${p}){${b}})`,
  (p, b) => `(${p})=>{${b}}`,
  (p, b) => `async (${p})=>{${b}}`,
  (p, b) => `({m(${p}){${b}}})`,
  (p, b) => `({*m(${p}){${b}}})`,
  (p, b) => `({async m(${p}){${b}}})`,
  (p, b) => `({set p(${p.split(",")[0]}){${b}}})`,
  (p, b) => `class C{m(${p}){${b}}}`,
  (p, b) => `class C{constructor(${p}){${b}}}`,
  (p, b) => `class C{static m(${p}){${b}}}`,
  (p, b) => `new Function(${JSON.stringify(p)},${JSON.stringify(b)})`.replace(/^new Function/, "(function(){}).constructor"),
];
for (const p of params) for (const f of fnForms) {
  add(f(p, "'use strict'"));
  add(f(p, "'use strict';"));
  add(f(p, "\"use strict\""));
  add(f(p, "'use\\x20strict'"));
  add(f(p, "'use strict';return 1"));
  add(f(p, "return 1;'use strict'"));
}
for (const f of fnForms) {
  add(f("a,a", "'use strict'"));
  add(f("eval", "'use strict'"));
  add(f("arguments", "'use strict'"));
  add(f("yield", "'use strict'"));
  add(f("let", "'use strict'"));
  add(f("static", "'use strict'"));
  add(f("implements", "'use strict'"));
  add(f("a,a", ""));
  add(f("eval", ""));
}
add("function eval(){'use strict'}");
add("function arguments(){'use strict'}");
add("function yield(){'use strict'}");
add("function static(){'use strict'}");
add("(function eval(){'use strict'})");
add("(function arguments(){'use strict'})");
add("function f(){'use strict';function eval(){}}");
add("function f(){'use strict';var eval}");
add("function f(){'use strict';eval=1}");
add("function f(){'use strict';arguments=1}");
add("function f(){'use strict';eval++}");
add("function f(){'use strict';--arguments}");
add("function f(){'use strict';[eval]=[]}");
add("function f(){'use strict';({eval}={})}");
add("function f(){'use strict';({a:eval}={})}");
add("function f(){'use strict';for(eval of []);}");
add("function f(){'use strict';for(eval in {});}");
add("function f(){'use strict';try{}catch(eval){}}");
add("function f(){'use strict';let eval}");
add("function f(){'use strict';class eval{}}");
add("function f(){'use strict';(eval)=>1}");
add("function f(){'use strict';eval=>1}");
add("function f(){'use strict';arguments=>1}");
add("function f(){'use strict';with(a);}");
add("function f(){'use strict';010}");
add("function f(){'use strict';'\\01'}");
add("function f(){'use strict';'\\8'}");
add("function f(){'use strict';'\\08'}");
add("function f(){'use strict';delete a}");
add("function f(){'use strict';delete (a)}");
add("function f(){'use strict';delete ((a))}");
add("function f(){'use strict';delete (a,b)}");
add("function f(){'use strict';delete a.b}");
add("function f(){'use strict';delete a?.b}");
add("function f(){'use strict';delete this}");
add("function f(){'use strict';delete 1}");
add("function f(){'use strict';delete (0,a)}");
add("function f(){'use strict';delete (a=1)}");
add("function f(){'use strict';delete -a}");
add("function f(){'use strict';delete typeof a}");
add("function f(){'use strict';delete void a}");
add("function f(){'use strict';delete delete a}");
add("function f(){'use strict';delete a['b']}");
add("function f(){'use strict';delete a.#b}");
add("class C{#b;m(){delete this.#b}}");
add("class C{#b;m(){delete this?.#b}}");
add("class C{#b;m(){delete (this.#b)}}");
add("class C{#b;m(){delete ((this.#b))}}");
add("class C{#b;m(){delete this.a.#b}}");
add("class C{#b;m(){delete this?.a.#b}}");
for (const d of ["a", "(a)", "((a))", "a.b", "a?.b", "a[0]", "(a.b)", "(a,b)", "(a,b.c)", "this", "1", "typeof a", "!a", "+a", "a++", "++a", "new a", "a()", "a=1", "[a]", "({a})", "async()=>1", "()=>1", "function(){}", "class{}", "`x`", "a?.()", "a?.[0]", "super.x", "new.target", "import.meta", "eval", "arguments", "undefined", "NaN", "void 0", "yield", "await", "await 1"]) {
  both(`delete ${d}`);
  both(`(function(){delete ${d}})`);
  both(`(function(){return delete ${d}})`);
  both(`(function(){var x=delete ${d}})`);
  both(`delete ${d}.x`);
}

// ---- 3. Labels, break, continue, return.
for (const s of [
  "a:a:1", "a:{a:1}", "a:b:a:1", "a:while(1)a:break", "a:function a(){}", "a:{function f(){a:1}}", "a:(function(){a:1})", "a:()=>{a:1}", "a:class C{m(){a:1}}",
  "a:1;a:2", "a:{}a:{}", "a:{a:{}}", "a:{b:{a:{}}}", "a:{b:{b:{}}}", "a:b:c:a:1", "a:if(1)a:1", "a:try{a:1}catch(e){}", "a:for(;;){a:break}", "a:switch(1){case 1:a:1}",
  "break", "break a", "continue", "continue a", "a:break a", "a:break b", "a:continue a", "a:continue", "a:{continue a}", "a:{continue}", "a:{break}",
  "a:{break a}", "a:while(1)break a", "a:while(1)continue a", "a:while(1)continue b", "a:while(1){b:continue a}", "a:b:while(1)continue a", "a:b:while(1)continue b",
  "a:b:{while(1)continue a}", "a:{b:while(1)continue a}", "a:{while(1)break a}", "a:{while(1)continue a}", "a:if(1)continue a", "a:if(1)break a", "a:if(1){while(1)continue a}",
  "while(1){break}", "while(1){continue}", "while(1)break", "while(1)continue", "do break;while(0)", "do continue;while(0)", "for(;;)break", "for(;;)continue", "for(x in y)break",
  "for(x of y)continue", "for(x of y)continue b", "switch(1){case 1:break}", "switch(1){case 1:continue}", "switch(1){case 1:break a}", "switch(1){default:continue}", "a:switch(1){case 1:continue a}",
  "while(1){switch(1){case 1:continue}}", "while(1){function f(){break}}", "while(1){function f(){continue}}", "while(1){(function(){break})}", "while(1){()=>{continue}}", "while(1){({m(){break}})}",
  "while(1){class C{m(){continue}}}", "while(1){class C{static{break}}}", "while(1){class C{static{continue}}}", "a:{class C{static{break a}}}", "a:while(1){class C{static{continue a}}}",
  "a:{(function(){break a})}", "a:{(()=>{break a})}", "a:{(()=>{continue a})}", "a:{({m(){break a}})}", "a:while(1){(function(){continue a})}", "a:while(1){(function(){break})}",
  "try{break}finally{}", "while(1){try{break}finally{}}", "while(1){try{}finally{break}}", "while(1){try{continue}catch(e){}}", "x:{try{break x}finally{}}", "x:try{break x}finally{}",
  "break\na", "a:while(1){break\na}", "a:while(1){continue\na}", "a:while(1){break /*\n*/a}", "yield:1", "await:1", "async:1", "let:1", "static:1", "implements:1", "eval:1", "arguments:1",
  "of:1", "get:1", "set:1", "if:1", "this:1", "null:1", "true:1", "enum:1", "class:1", "a\\u0020:1", "\\u0061:\\u0061:1", "a:\\u0061:1", "\\u{61}:a:1", "v\\u0061r:1", "\\u0069f:1",
  "a:;a:;", "a:{};a:{}", "a:a", "a:function*g(){}", "a:async function f(){}", "a:class C{}", "a:let\nx", "a:let x", "a:let;", "a:const x=1", "a:var x", "a: b: function f(){}", "a:\nfunction f(){}", "a:if(1)function f(){}",
  "return", "return 1", "return;", "return\n1", "{return}", "if(1)return", "while(0)return", "return a=>1", "return(1)", "return(", "return,", "()=>{return}", "class C{static{return}}",
  "class C{static{(function(){return})}}", "class C{static{()=>{return}}}", "class C{static{({m(){return}})}}", "class C{x=function(){return}}", "class C{x=()=>{return 1}}",
  "class C{static{await}}", "class C{static{await 1}}", "class C{static{var await}}", "class C{static{let await}}", "class C{static{await:1}}", "class C{static{function await(){}}}", "class C{static{class await{}}}",
  "class C{static{({await})}}", "class C{static{(await)=>1}}", "class C{static{async function f(){await 1}}}", "class C{static{(function await(){})}}", "class C{static{(function(await){})}}",
  "class C{static{arguments}}", "class C{static{()=>arguments}}", "class C{static{(function(){arguments})}}", "class C{static{yield}}", "class C{static{var yield}}",
  "class C{static{this}}", "class C{static{super.x}}", "class C{static{super()}}", "class C{static{new.target}}", "class C{static{var x;var x}}", "class C{static{let x;var x}}", "class C{static{function f(){}function f(){}}}",
  "class C{static{}static{}}", "class C{static{;}}", "class C{static async{}}", "class C{static\n{}}", "class C{static{}", "class C{static {let a}static{let a}}",
]) both(s);

// ---- 4. Palavras reservadas e contextuais como identificador, em cada modo.
const words = [
  "yield", "await", "async", "let", "static", "implements", "interface", "package", "private", "protected", "public", "enum", "eval", "arguments",
  "of", "get", "set", "target", "meta", "as", "from", "null", "true", "false", "this", "new", "super", "typeof", "void", "delete", "in", "instanceof",
  "with", "do", "if", "else", "try", "catch", "finally", "throw", "switch", "case", "default", "debugger", "undefined", "constructor", "accessor", "using",
];
const positions = [
  (w) => `var ${w}`,
  (w) => `let ${w}`,
  (w) => `const ${w}=1`,
  (w) => `function ${w}(){}`,
  (w) => `(function ${w}(){})`,
  (w) => `function f(${w}){}`,
  (w) => `function f(${w}=1){}`,
  (w) => `function f({${w}}){}`,
  (w) => `function f([${w}]){}`,
  (w) => `function f(...${w}){}`,
  (w) => `(${w})=>1`,
  (w) => `${w}=>1`,
  (w) => `async ${w}=>1`,
  (w) => `async(${w})=>1`,
  (w) => `${w}:1`,
  (w) => `${w}=1`,
  (w) => `${w}++`,
  (w) => `++${w}`,
  (w) => `[${w}]=[]`,
  (w) => `({${w}}={})`,
  (w) => `({${w}=1}={})`,
  (w) => `({a:${w}}={})`,
  (w) => `({${w}})`,
  (w) => `({${w}:1})`,
  (w) => `({${w}(){}})`,
  (w) => `({get ${w}(){return 1}})`,
  (w) => `({*${w}(){}})`,
  (w) => `({async ${w}(){}})`,
  (w) => `class ${w}{}`,
  (w) => `(class ${w}{})`,
  (w) => `class C extends ${w}{}`,
  (w) => `class C{${w}(){}}`,
  (w) => `class C{${w}=1}`,
  (w) => `class C{static ${w}(){}}`,
  (w) => `class C{get ${w}(){return 1}}`,
  (w) => `try{}catch(${w}){}`,
  (w) => `for(var ${w} of []);`,
  (w) => `for(${w} in {});`,
  (w) => `for(${w} of []);`,
  (w) => `for(let ${w} of []);`,
  (w) => `${w}.x`,
  (w) => `x.${w}`,
  (w) => `x?.${w}`,
  (w) => `${w}()`,
  (w) => `new ${w}`,
  (w) => `typeof ${w}`,
  (w) => `(${w})`,
  (w) => `[${w}]`,
  (w) => `${w}\n1`,
  (w) => `${w} 1`,
  (w) => `x=${w}`,
  (w) => `x=${w}=>1`,
  (w) => `\\u0061${w}`.slice(6),
  (w) => `${w.slice(0, 1) === "" ? w : "\\u{" + w.charCodeAt(0).toString(16) + "}" + w.slice(1)}`,
  (w) => `var ${"\\u{" + w.charCodeAt(0).toString(16) + "}" + w.slice(1)}`,
  (w) => `({${"\\u{" + w.charCodeAt(0).toString(16) + "}" + w.slice(1)}})`,
  (w) => `x.${"\\u{" + w.charCodeAt(0).toString(16) + "}" + w.slice(1)}`,
];
const posIndexCore = positions.length;
for (const w of words) for (let i = 0; i < posIndexCore; i++) {
  const src = positions[i](w);
  add(src, "F");
  add("'use strict';" + src, "F");
  add(src, "G");
  add(src, "A");
  if (i < 20) add(src, "AG");
}
// Dentro de funções internas (o contexto vem da função, não do construtor).
for (const w of ["yield", "await", "async", "let", "static"]) {
  for (const ctx of [
    (s) => `function* g(){${s}}`, (s) => `async function f(){${s}}`, (s) => `async function* h(){${s}}`, (s) => `(function*(){${s}})`,
    (s) => `(async function(){${s}})`, (s) => `async()=>{${s}}`, (s) => `({*m(){${s}}})`, (s) => `({async m(){${s}}})`, (s) => `class C{m(){${s}}}`, (s) => `class C extends (${s}){}`,
    (s) => `function* g(){function f(){${s}}}`, (s) => `async function f(){function g(){${s}}}`, (s) => `function* g(){()=>{${s}}}`, (s) => `async function f(){()=>{${s}}}`,
    (s) => `function* g(x=${s}){}`, (s) => `async function f(x=${s}){}`, (s) => `function* ${w}(){${s}}`, (s) => `async function ${w}(){${s}}`,
    (s) => `(function* ${w}(){${s}})`, (s) => `(async function ${w}(){${s}})`, (s) => `class C{static{${s}}}`, (s) => `class C{x=${s}}`, (s) => `class C{[${s}](){}}`,
  ]) {
    for (const st of [`var ${w}`, `${w}=1`, `${w}`, `${w}()`, `${w} 1`, `${w}\n1`, `${w}.x`, `(${w})`, `${w}=>1`, `(${w})=>1`, `async ${w}=>1`, `let ${w}`, `function ${w}(){}`, `({${w}})`, `[${w}]=[]`, `x=${w}`, `${w}:1`, `class ${w}{}`, `try{}catch(${w}){}`, `${w}\n=>1`]) {
      add(ctx(st), "F");
      add("'use strict';" + ctx(st), "F");
    }
  }
}
// yield e await como expressões e com operandos inválidos.
for (const e of [
  "yield", "yield 1", "yield\n1", "yield*", "yield* 1", "yield\n*1", "yield?1:2", "yield ? 1 : 2", "yield in x", "yield=1", "yield++", "yield+1", "yield /x/", "yield /x/g", "yield => 1", "(yield)=>1",
  "(x=yield)=>1", "(x=yield 1)=>1", "x=yield", "x=yield 1", "yield yield 1", "yield await 1", "await yield", "await yield 1", "yield, yield", "[yield]", "[...yield]", "({yield})", "({yield:1})", "({yield=1}={})",
  "({[yield]:1})", "`${yield}`", "yield`x`", "yield.x", "yield[0]", "yield()", "new yield", "new yield()", "typeof yield", "void yield", "!yield", "-yield", "yield ** 2", "2 ** yield", "yield ?? 1", "yield || 1", "1 + yield", "1 + yield 1",
  "1 || yield", "1 ?? yield", "a ? yield : yield", "a ? yield 1 : yield 2", "if(yield)yield", "for(yield;;);", "for(yield in x);", "for(yield of x);", "for(x of yield);", "for(var x of yield 1);",
  "var x=yield", "var x=yield 1", "var [x=yield]=[]", "var {x=yield}={}", "let x=yield 1", "x(yield)", "x(yield 1)", "x(...yield)", "x(yield, 1)", "x(1, yield 1)", "yield\n=>1", "async yield=>1",
  "await", "await 1", "await\n1", "await x", "await await 1", "await (1)", "await(1)", "await=1", "await++", "await in x", "await /x/", "await => 1", "(await)=>1", "(x=await 1)=>1", "(x=await)=>1", "x=await 1",
  "[await 1]", "({await})", "({await:1})", "({[await 1]:1})", "`${await 1}`", "await`x`", "await.x", "await[0]", "await()", "new await", "new await 1", "typeof await 1", "void await 1", "!await 1", "-await 1", "await 1 ** 2", "(await 1) ** 2", "2 ** await 1",
  "await 1 ?? 2", "1 + await 1", "if(await 1);", "for(await 1;;);", "for await(x of y);", "for await(var x of y);", "for await(let x of y);", "for await(x in y);", "for await(;;);", "for await(x of y,z);", "for await(async of y);", "for await(let of y);",
  "for await(x of y)break", "for await (const [a,b] of y);", "for await (x of y) function f(){}", "for await\n(x of y);", "for await(x=1 of y);", "for await(var x=1 of y);",
  "async function f(){for await(x of y);}", "async function f(){for await(x in y);}", "async function f(){for await(;;);}", "async function f(){for await(let of y);}", "async function f(){for await(async of y);}", "async function f(){for await(var x=1 of y);}",
  "async function f(){await}", "async function f(){await 1 = 2}", "async function f(){await x=1}", "async function f(){(await x)=1}", "async function f(){await x++}", "async function f(){-await x ** 2}", "async function f(){(-await x) ** 2}",
  "async function f(){var await}", "async function f(){let await}", "async function f(){await:1}", "async function f(){function await(){}}", "async function f(){(await)=>1}", "async function f(){({await})}", "async function f(){({await}=x)}",
  "async function f(){[await]=x}", "async function f(){x.await}", "async function f(){({await(){}})}", "async function f(){({await:1})}", "async function f(a=await 1){}", "async function f(await){}", "async function f(...await){}", "async function f({await}){}",
  "async function await(){}", "(async function await(){})", "async function f(){function g(a=await 1){}}", "async function f(){function g(){await 1}}", "async function f(){()=>await 1}", "async function f(){async()=>await 1}",
  "async function f(){(a=await 1)=>1}", "async function f(){async(a=await 1)=>1}", "async function f(){async(await)=>1}", "async function f(){async await=>1}", "async function f(){class C{[await 1](){}}}", "async function f(){class C extends (await 1){}}",
  "async function f(){class C{x=await 1}}", "async function f(){class C{static{await 1}}}", "async function f(){new.target}", "async function f(){super.x}", "async function f(){arguments}", "async function f(){import.meta}", "async function f(){import(await 1)}",
  "async()=>await", "async()=>await 1", "async(x=await 1)=>1", "async(await)=>1", "async await=>1", "async(...await)=>1", "async({await})=>1", "async([await])=>1", "async(x=await)=>1", "async x=>await 1", "async x\n=>1", "async\nx=>1", "async\n(x)=>1", "async (x)\n=>1",
  "async function* g(){yield await 1}", "async function* g(){await yield 1}", "async function* g(){yield*1}", "async function* g(){yield\n*1}", "async function* g(){var yield}", "async function* g(){var await}", "async function* g(a=yield){}", "async function* g(a=await 1){}",
  "function* g(){yield\n*1}", "function* g(a=yield){}", "function* g(a=yield 1){}", "function* g(yield){}", "function* g(...yield){}", "function* g({yield}){}", "function* g([yield]){}", "function* g(){function f(a=yield){}}", "function* g(){(a=yield)=>1}", "function* g(){(yield)=>1}",
  "function* g(){yield=>1}", "function* g(){({yield})}", "function* g(){({yield:1})}", "function* g(){({yield}=x)}", "function* g(){[yield]=x}", "function* g(){x.yield}", "function* g(){({*yield(){}})}", "function* g(){class C{yield(){}}}", "function* g(){class yield{}}", "function* g(){label:yield}", "function* g(){yield:1}",
  "function* g(){yield\n}", "function* g(){yield}", "function* g(){yield /x/}", "function* g(){yield /x/g}", "function* g(){yield\n/x/}", "function* g(){yield++}", "function* g(){yield ++x}", "function* g(){yield in x}", "function* g(){(yield) in x}", "function* g(){yield instanceof x}", "function* g(){yield ? 1 : 2}", "function* g(){yield ?? 1}", "function* g(){1 ?? yield}",
  "function* g(){yield = 1}", "function* g(){(yield) = 1}", "function* g(){yield++}", "function* g(){(yield)++}", "function* g(){yield.x}", "function* g(){yield\n.x}", "function* g(){(yield).x}", "function* g(){yield `x`}", "function* g(){yield\n`x`}", "function* g(){new yield}", "function* g(){new (yield)}", "function* g(){new yield 1}",
  "function* g(){-yield}", "function* g(){-yield 1}", "function* g(){typeof yield}", "function* g(){1 + yield}", "function* g(){1 + yield 1}", "function* g(){1 ** yield}", "function* g(){yield ** 1}", "function* g(){yield\n** 1}", "function* g(){(yield) ** 1}", "function* g(){-(yield) ** 1}",
  "function* g(){for(yield in x);}", "function* g(){for(yield of x);}", "function* g(){for(yield;;);}", "function* g(){for(x of yield);}", "function* g(){for(x in yield);}", "function* g(){for(var x of yield 1);}", "function* g(){for(var x = yield in y;;);}", "function* g(){for(let x = yield;;);}", "function* g(){for(var yield of x);}",
  "function* g(){for(var x=yield in y);}", "function* g(){var [yield]=[]}", "function* g(){var {yield}={}}", "function* g(){var {a:yield}={}}", "function* g(){let yield}", "function* g(){const yield=1}", "function* g(){try{}catch(yield){}}", "function* g(){function yield(){}}", "function* g(){(function yield(){})}", "function* g(){(function* yield(){})}",
  "function* yield(){}", "(function* yield(){})", "function* g(){function* yield(){}}", "function* g(){async function yield(){}}", "function* g(){async function* yield(){}}", "function* g(){(async function yield(){})}", "function* g(){(async function* yield(){})}", "function* g(){yield\n}", "function* g(){({a=yield}={})}", "function* g(){({a=yield 1}={})}",
]) {
  both(e);
  add(e, "G");
  add(e, "A");
  add(e, "AG");
}

// ---- 5. new.target, super, import.meta, import() fora de contexto.
for (const s of [
  "new.target", "new.target()", "new.target=1", "new.target++", "new.target.x", "new . target", "new.\ntarget", "new.targe", "new.targets", "new.t", "new.", "new.target`x`", "new.target?.x", "new.\\u0074arget", "new.t\\u0061rget", "n\\u0065w.target", "new.target in x", "typeof new.target", "delete new.target",
  "function f(){new.target}", "function f(){new.target=1}", "function f(){new.target++}", "function f(){[new.target]=[]}", "function f(){({a:new.target}={})}", "function f(){for(new.target of []);}", "function f(){for(new.target in {});}", "function f(){new.target()}", "function f(){new new.target}",
  "()=>new.target", "(()=>new.target)", "function f(){()=>new.target}", "function f(){(()=>{new.target})}", "function f(){class C{x=new.target}}", "function f(){class C{static x=new.target}}", "class C{x=new.target}", "class C{static x=new.target}", "class C{static{new.target}}", "class C{[new.target](){}}", "class C extends new.target{}",
  "class C{m(){new.target}}", "class C{constructor(){new.target}}", "({m(){new.target}})", "({get a(){return new.target}})", "({set a(v){new.target}})", "({a:function(){new.target}})", "({a:()=>new.target})", "function f(a=new.target){}", "(a=new.target)=>1", "function f(){function g(a=new.target){}}",
  "async function f(){new.target}", "function* g(){new.target}", "async function* g(){new.target}", "async()=>new.target", "function f(){eval('new.target')}", "()=>eval('new.target')", "function f(){return new.target}", "new new.target", "new.target.name", "function f(){new.target.name}",
  "super", "super.x", "super[x]", "super()", "super.x()", "super.x=1", "super.x++", "super?.x", "super.#x", "super`x`", "super.x`y`", "new super", "new super()", "new super.x", "new super.x()", "typeof super", "typeof super.x", "delete super.x", "delete super[x]", "delete (super.x)", "super in x", "super ? 1 : 2", "super\n.x", "super . x", "super.\nx", "super.",
  "function f(){super.x}", "function f(){super()}", "function f(){super}", "()=>super.x", "()=>super()", "function f(){()=>super.x}", "function f(){()=>super()}", "({m(){super.x}})", "({m(){super()}})", "({m(){super}})", "({m(){super[x]}})", "({m(){super.x=1}})", "({m(){super.x++}})", "({m(){delete super.x}})", "({m(){delete super[x]}})", "({m(){new super.x}})", "({m(){new super}})",
  "({m(){()=>super.x}})", "({m(){()=>super()}})", "({m(){function f(){super.x}}})", "({m(){(function(){super.x})}})", "({m(){({n(){super.x}})}})", "({m(){class C{n(){super.x}}}})", "({get m(){return super.x}})", "({set m(v){super.x=v}})", "({*m(){super.x}})", "({async m(){super.x}})", "({async *m(){super.x}})", "({m:function(){super.x}})", "({m:()=>super.x})", "({m(a=super.x){}})", "({m(a=super()){}})",
  "({[super.x]:1})", "({a:super.x})", "({a:super()})", "({...super.x})", "({m(){super.x`y`}})", "({m(){super.x?.y}})", "({m(){super?.x}})", "({m(){super.x?.()}})", "({m(){super.#x}})", "({m(){super\n.x}})", "({m(){super[]}})", "({m(){super[1,2]}})", "({m(){super.x.y}})", "({m(){super.x.y=1}})", "({m(){super.x[y]}})", "({m(){[super.x]=[]}})", "({m(){({a:super.x}={})}})", "({m(){for(super.x of []);}})", "({m(){for(super.x in {});}})", "({m(){super.x\n=1}})",
  "class C{m(){super.x}}", "class C{m(){super()}}", "class C{constructor(){super()}}", "class C{constructor(){super.x}}", "class C{constructor(){()=>super()}}", "class C{constructor(){function f(){super()}}}", "class C{constructor(){(function(){super()})}}", "class C{constructor(){({m(){super()}})}}", "class C{static m(){super.x}}", "class C{static m(){super()}}", "class C{get m(){return super.x}}",
  "class C extends D{constructor(){super()}}", "class C extends D{constructor(){super();super()}}", "class C extends D{constructor(){()=>super()}}", "class C extends D{constructor(){super.x}}", "class C extends D{constructor(){function f(){super()}}}", "class C extends D{constructor(){(function(){super()})}}", "class C extends D{constructor(){({m(){super()}})}}", "class C extends D{constructor(){class E{constructor(){super()}}}}", "class C extends D{constructor(){class E extends F{constructor(){super()}}}}",
  "class C extends D{m(){super()}}", "class C extends D{static m(){super()}}", "class C extends D{get m(){super()}}", "class C extends D{set m(v){super()}}", "class C extends D{*m(){super()}}", "class C extends D{async m(){super()}}", "class C extends D{x=super()}", "class C extends D{x=super.y}", "class C extends D{static x=super.y}", "class C extends D{static x=super()}", "class C extends D{static{super.x}}", "class C extends D{static{super()}}", "class C extends D{[super.x](){}}", "class C extends D{[super()](){}}", "class C extends super.x{}", "class C extends (super.x){}", "class C extends D{constructor(a=super()){}}", "class C extends D{constructor(a=super.x){}}", "class C extends D{constructor(){super`x`}}", "class C extends D{constructor(){super?.()}}", "class C extends D{constructor(){new super()}}", "class C extends D{constructor(){super=1}}", "class C extends D{constructor(){super()=1}}", "class C extends D{constructor(){super()++}}", "class C extends D{constructor(){[super()]=[]}}", "class C extends D{constructor(){for(super() of []);}}", "class C extends D{constructor(){delete super()}}", "class C extends D{constructor(){typeof super()}}", "class C extends D{constructor(){super().x}}", "class C extends D{constructor(){super(...a)}}", "class C extends D{constructor(){super(...a,)}}", "class C extends D{constructor(){super(,)}}", "class C extends D{constructor(){super(a,,b)}}", "class C extends D{constructor(){super(...)}}", "class C extends D{constructor(){super.x()}}", "class C extends D{constructor(){super[x]()}}",
  "class C extends null{constructor(){super()}}", "class C extends (D,E){}", "class C extends D,E{}", "class C extends D=E{}", "class C extends D?E:F{}", "class C extends a?.b{}", "class C extends a.b{}", "class C extends a()(){}", "class C extends new D{}", "class C extends new D(){}", "class C extends async function(){}{}", "class C extends function(){}{}", "class C extends class{}{}", "class C extends{}{}", "class C extends []{}", "class C extends 1{}", "class C extends -1{}", "class C extends !D{}", "class C extends typeof D{}", "class C extends D++{}", "class C extends ++D{}", "class C extends yield{}", "class C extends await{}", "class C extends this{}", "class C extends null{}", "class C extends{}.x{}", "class C extends D.#x{}", "class C extends\nD{}", "class C\nextends D{}", "class C extends{", "class C extends",
  "import.meta", "import.meta.x", "import . meta", "import.meta=1", "import.meta()", "import.meta++", "import.metas", "import.target", "import.", "import", "function f(){import.meta}", "()=>import.meta", "new import.meta", "typeof import.meta", "delete import.meta", "import.m\\u0065ta", "im\\u0070ort.meta", "import\n.meta",
  "import('a')", "import('a',{})", "import('a',{},)", "import('a',)", "import('a',{},1)", "import(...a)", "import(a,...b)", "import()", "import(,)", "import(a,,b)", "import('a')=1", "import('a')++", "import('a').then", "new import('a')", "new (import('a'))", "import('a')`x`", "import?.('a')", "typeof import('a')", "delete import('a')", "[import('a')]=[]", "({a:import('a')}={})", "for(import('a') of []);", "for(import('a') in {});", "import(a=1)", "import(a,b)", "import(yield)", "import(await 1)", "import.defer('a')", "import.source('a')", "import.defer", "import.source", "import.defer()", "import.source()", "import.defer.x", "import.defer\n('a')",
  "import a from 'b'", "import 'a'", "import {} from 'a'", "import * as a from 'b'", "import a,{b} from 'c'", "import {a as b} from 'c'", "import {default as a} from 'c'", "import {a} from", "import a", "import {a}", "import a from", "import * from 'a'", "import * as from 'a'", "import {a,} from 'b'", "import {,} from 'b'", "import {a b} from 'c'", "import a, from 'b'", "import a,b from 'c'", "import a,* as b from 'c'", "import * as a,b from 'c'", "import {a} from 'b' with {type:'json'}", "import {a} from 'b' assert {type:'json'}", "import 'a' with {}", "import 'a' with {a:'b'}", "import 'a' with {a:'b',a:'c'}", "import 'a' with {a:1}", "import 'a' with {'a':'b'}", "import 'a' with {a:'b',}", "import 'a' with", "import 'a' with {", "import 'a' with a", "import {a as} from 'b'", "import {as as as} from 'b'", "import {if as a} from 'b'", "import {if} from 'b'", "import {a as if} from 'b'", "import {'a' as b} from 'c'", "import {'a'} from 'c'", "import {a as 'b'} from 'c'", "import {eval} from 'a'", "import {eval as e} from 'a'", "import {e as eval} from 'a'", "import eval from 'a'", "import arguments from 'a'", "import {arguments} from 'a'", "import * as eval from 'a'", "import await from 'a'", "import yield from 'a'", "import let from 'a'", "import static from 'a'", "import a from 'b';import a from 'c'", "import a from 'b';var a", "import a from 'b';let a", "import a from 'b';function a(){}", "import a from 'b';class a{}", "import a from 'b';a=1", "import {a} from 'b';a++", "import * as a from 'b';a.x=1", "import {a,a} from 'b'", "import {a as b,c as b} from 'd'", "import a,{a} from 'b'", "import a,* as a from 'b'", "{import a from 'b'}", "function f(){import a from 'b'}", "if(1)import a from 'b'", "label:import a from 'b'", "import a from 'b' c", "import a from b", "import a from 1", "import a from `b`", "import a from 'b'\nc", "import a from '\\u{110000}'", "import a from '\\x'", "import a from '\\08'", "import a from '\\01'", "import a from '\\8'", "import\ta\tfrom\t'b'",
  "export var a", "export let a", "export const a=1", "export function f(){}", "export class C{}", "export async function f(){}", "export function* g(){}", "export async function* g(){}", "export default 1", "export default function(){}", "export default function f(){}", "export default class{}", "export default class C{}", "export default async function(){}", "export default async function f(){}", "export default function*(){}", "export default (1,2)", "export default a=1", "export default a,b", "export default let", "export default var a", "export default const a=1", "export default let a", "export default async ()=>1", "export default async", "export default async\nfunction f(){}", "export default function f(){};export default 1", "export default 1;export default 2", "export default class C{};export {C as default}", "export {a}", "export {a as b}", "export {a as default}", "export {a,b}", "export {a,}", "export {,}", "export {}", "export {a} from 'b'", "export * from 'a'", "export * as a from 'b'", "export * as default from 'a'", "export * as 'a' from 'b'", "export {'a'} from 'b'", "export {'a'}", "export {a as 'b'}", "export {'a' as b} from 'c'", "export {'a' as 'b'} from 'c'", "export {if}", "export {if} from 'a'", "export {a as if}", "export {eval}", "export {arguments}", "export {a as eval}", "export {yield}", "export {await}", "export {let}", "export {static}", "export {a,a}", "export {a as b,c as b}", "export {a as b,b}", "export var a;export var a", "export let a;export let a", "export var a;export let a", "export function a(){}export function a(){}", "export function a(){}export var a", "export class a{}export var a", "export let a;var a", "export {a};export {a}", "export {a};var a;export {a as a}", "export * from 'a' with {type:'json'}", "export {a} from 'b' with {type:'json'}", "export * as a,b from 'c'", "export a from 'b'", "export a", "export", "export;", "export 1", "export a=1", "export {a} b", "export {a} from", "export * from", "export * as from 'a'", "export {a as} from 'b'", "export * 'a'", "export\n*\nfrom\n'a'", "export default", "export default;", "export default\n", "export default function", "export default class", "export default async function", "export default export var a", "export export var a", "export import a from 'b'", "export {a} export {b}", "{export var a}", "function f(){export var a}", "if(1)export var a", "label:export var a", "export var\na", "export let\na", "export const\na=1", "export\nvar a", "export\ndefault 1", "export default\n1", "export var a=1,b=2", "export let [a,b]=[]", "export const {a,b}={}", "export var [a,a]=[]", "export let {a,a}={}", "export var {a:b,c:b}={}", "export class a{}export class a{}", "export function a(){}export class a{}", "export function*a(){}export function a(){}", "export async function a(){}export function a(){}", "export function a(){}export async function a(){}",
  "await 1", "await", "await x", "await x;await y", "await\nx", "await(x)", "await(x)=1", "await=1", "await++", "await in x", "await.x", "await[0]", "await()", "await`x`", "await/x/g", "await => 1", "(await)=>1", "var await", "let await", "const await=1", "function await(){}", "class await{}", "(function await(){})", "await:1", "({await})", "({await:1})", "({await}=x)", "[await]=x", "x.await", "x?.await", "async function f(){}await 1", "(async function(){})\nawait 1", "for await(x of y);", "for await(var x of y);", "for await(;;);", "for await(x in y);", "for await(x of y,z);", "for\nawait(x of y);", "{await 1}", "if(1)await 1", "x=await 1", "x=await", "[await 1]", "({a:await 1})", "(await 1)", "typeof await 1", "!await 1", "await 1+1", "await 1,await 2", "await await 1", "await async()=>1", "await import('a')", "await (async()=>1)()", "await new Promise(r=>r())", "await\n(1)", "await\n/x/", "await /x/", "await -1", "await +1", "await ++x", "await --x", "await !x", "await ~x", "await typeof x", "await void x", "await delete x", "await new x", "await yield", "await this", "await null", "await 1n", "await 1_0", "await 'a'", "await `a`", "await [a]", "await {a}", "await function(){}", "await class{}", "await async function(){}", "await(async function(){})", "await ()=>1", "await async()=>1", "await a=>1", "await async a=>1", "await ()", "await (", "await )", "await ,", "await ;", "await\n;", "await\n}",
]) {
  add(s, "E");
  add(s, "F");
  add("'use strict';" + s, "F");
}

// ---- 6. Regex literais inválidos com flags.
const reBodies = [
  "", "a", "a|b", "(", ")", "(a", "a)", "[", "]", "[a", "a]", "{", "}", "{1}", "a{1", "a{1,", "a{1,2", "a{,2}", "a{2,1}", "a{1}{2}", "a**", "a+*", "a?*", "a*?", "a+?", "a??", "a{1}?", "a{1,2}?", "*", "+", "?", "a|*", "(*)", "(?", "(?:", "(?:a", "(?=a", "(?!a", "(?<=a", "(?<!a", "(?<n>a", "(?<n", "(?<>a)", "(?<1a>a)", "(?<a-b>a)", "(?<a>a)(?<a>b)", "(?<a>a)|(?<a>b)", "(?<a>a)\\k<b>", "\\k<a>", "\\k", "(?<a>a)\\k", "(?<a>a)\\k<", "\\k<a", "(?<a>.)\\k<a>", "(?<𝒜>.)", "(?<\\u{1d49c}>.)", "(?<\\ud835\\udc9c>.)", "(?<a\\u{1d49c}>.)", "\\1", "\\2(a)", "(a)\\2", "(a)\\1", "\\0", "\\00", "\\01", "\\8", "\\9", "[\\8]", "[\\1]", "[\\0]", "[\\00]",
  "\\c", "\\ca", "\\cA", "\\c1", "\\c_", "[\\c]", "[\\ca]", "[\\c1]", "[\\c_]", "[\\c*]", "\\c*", "\\x", "\\x4", "\\x41", "\\xg", "[\\x]", "\\u", "\\u1", "\\u12", "\\u123", "\\u1234", "\\u{", "\\u{}", "\\u{1}", "\\u{110000}", "\\u{10ffff}", "\\u{0000000041}", "\\u{g}", "\\ud800", "\\ud800\\udc00", "\\ud800\\u0041", "\\udc00", "\\ud83d\\ude00", "[\\ud83d\\ude00]", "😀", "[😀]", "😀+", "[😀-😁]", "[\\u{1f600}-\\u{1f601}]", "\\p", "\\p{", "\\p{}", "\\p{L}", "\\p{Lu}", "\\P{L}", "\\p{Letter}", "\\p{Script=Latin}", "\\p{sc=Latn}", "\\p{scx=Latn}", "\\p{Script_Extensions=Greek}", "\\p{Foo}", "\\p{L", "\\pL", "\\p{ASCII}", "\\p{Any}", "\\p{Assigned}", "\\p{General_Category=Lu}", "\\p{gc=Lu}", "\\p{Script=Foo}", "\\p{Script}", "\\p{=Latin}", "\\p{lu}", "\\p{ L }", "\\p{Emoji}", "\\p{RGI_Emoji}", "\\p{Basic_Emoji}", "\\p{Emoji_Keycap_Sequence}", "\\P{RGI_Emoji}", "[\\p{RGI_Emoji}]", "[^\\p{RGI_Emoji}]", "\\p{Lowercase}", "\\p{Uppercase}", "\\p{Alphabetic}", "\\p{White_Space}", "\\p{ID_Start}", "\\p{ID_Continue}", "\\p{Hex_Digit}", "\\p{Nd}", "\\p{Cased}", "\\p{Cased_Letter}", "\\p{LC}",
  "[a-z]", "[z-a]", "[a-a]", "[\\d-z]", "[a-\\d]", "[\\d-\\w]", "[a-\\w]", "[\\w-a]", "[--a]", "[a--]", "[a-]", "[-a]", "[-]", "[a-b-c]", "[^]", "[^a]", "[]", "[]]", "[^]]", "[\\]]", "[\\b]", "\\b", "\\B", "[\\B]", "\\d", "\\D", "\\s", "\\S", "\\w", "\\W", "\\q", "\\-", "[\\-]", "\\/", "\\_", "\\a", "\\e", "\\z", "\\y", "\\i", "\\Z", "\\A", "\\!", "\\@", "\\ ", "\\\n", "[\\ ]", "[\\a]", "[\\q]", "[\\-]", "\\^", "\\$", "\\.", "\\*", "\\+", "\\?", "\\(", "\\)", "\\[", "\\]", "\\{", "\\}", "\\|", "\\\\", "^*", "$*", "^+", "$+", "^?", "$?", "^{1}", "$?", "\\b*", "\\B+", "\\b?", "\\b{1}", "(?=a)*", "(?=a)+", "(?=a)?", "(?=a){1}", "(?!a)*", "(?!a)+", "(?!a)?", "(?<=a)*", "(?<=a)+", "(?<!a)?", "(?<=a){1}", "(?:a)*", "(?:a)+", "(?:a)?", "(?:a){1}", "(?:)", "()", "(?:)*", "()*", "(|)", "(?:|)", "||", "a||b", "|a", "a|", "(?i:a)", "(?i-s:a)", "(?-i:a)", "(?ims:a)", "(?i-i:a)", "(?ii:a)", "(?-:a)", "(?:a)", "(?i)", "(?i-:a)", "(?x:a)", "(?I:a)", "(?i:a", "(?i:)", "(?s-m:.)", "(?m-s:^)",
  "[a&&b]", "[a--b]", "[[a]&&[b]]", "[[a]--[b]]", "[\\w&&\\d]", "[\\w--\\d]", "[a&&&b]", "[a&&]", "[&&a]", "[a--]", "[--a]", "[a-b&&c]", "[a&&b-c]", "[[a-b]--c]", "[^a&&b]", "[^[a]&&[b]]", "[\\q{abc}]", "[\\q{a|bc}]", "[\\q{}]", "[\\q{a}]", "[\\q{a", "[\\q", "[\\q{abc|d}--\\q{abc}]", "[[a-z]--[aeiou]]", "[[a-z]&&[^aeiou]]", "[a(]", "[a)]", "[a{]", "[a}]", "[a|]", "[a/]", "[a[]", "[a]]", "[(]", "[)]", "[{]", "[}]", "[|]", "[/]", "[[]", "[\\[]", "[[]]", "[[a]]", "[[a][b]]", "[[a]b]", "[a[b]]", "[^[a]]", "[[^a]]", "[[a]-b]", "[a-[b]]", "[!!]", "[##]", "[$$]", "[%%]", "[**]", "[++]", "[,,]", "[..]", "[::]", "[;;]", "[<<]", "[==]", "[>>]", "[??]", "[@@]", "[``]", "[~~]", "[^^]", "[a^]", "[&]", "[&&]", "[a&b]", "[a&&&&b]",
  "a{1}{2}", "a{1}*", "a{1}+", "a{1}?", "a{99999999999999999999}", "a{1,99999999999999999999}", "a{99999999999999999999,1}", "a{0}", "a{0,0}", "a{4294967295}", "a{4294967296}", "a{2147483648}", "a{2147483647,}", "a{ 1}", "a{1 }", "a{1, 2}", "a{1 ,2}", "a{-1}", "a{1.5}", "a{x}", "a{1,x}", "a{x,1}", "a{", "a{}", "a{,}", "a{1,}", "a{,1}", "{a}", "{1,2}", "x{1,2}{3}", "(a){1}{2}", "(?:a){1}{2}", "(?<n>a){1}", "(?<n>a)\\k<n>{2}", "\\1{2}", "(a)\\1{2}", "\\k<n>{2}", "(?<n>.)\\k<n>*",
  "(?=a)b", "(?!a)b", "(?<=a)b", "(?<!a)b", "(?<=(a))b", "(?<=\\1(a))b", "(?<=(a)\\1)b", "(?<=a*)b", "(?<=a+)b", "(?<=a{2})b", "(?<=a|b)c", "(?<=a(?=b))c", "(?<=a(?<=b))c", "(?<=\\b)a", "(?<=^)a", "(?<=$)a", "(?<=)a", "(?<!)a", "(?=)a", "(?!)a", "(?<=a)*", "(?<=a)+", "(?<=a)?", "(?<=a){1}", "(?<!a)*", "(?<!a)+", "(?<!a){1}", "(?<!a)?", "(?<=a)(?<=b)", "(?<=a)\\1", "(?<=a)\\k<n>", "(?<n>a)(?<=\\k<n>)b", "(?<=(?<n>a)\\k<n>)b",
  "\\8(a)", "(a)\\8", "\\10", "\\10(a)", "(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\\10", "(a)\\10", "\\11", "\\1(a)", "(a)\\1(b)\\2", "\\00(a)", "\\08", "\\18", "\\78", "\\377", "\\400", "\\777", "[\\377]", "[\\400]", "\\0a", "\\0\\0", "\\0\\1", "\\012", "\\1a", "\\a1", "[\\1a]", "[\\a1]",
  "a/", "a\\", "\\", "[\\", "[a\\", "(?:\\", "a\n", "a\r", "a\u2028", "a\u2029", "[\n]", "[\u2028]", "\\\n", "\\\u2028", "\\\u2029", "(?<n>\n)", "a\u0085", "a\u00a0", "a\ufeff", "a/b", "a\\/b", "[/]", "[\\/]", "(/)", "(?:/)", "\\//", "/",
  "a\\u{61}", "\\u{61}", "[\\u{61}]", "\\u{61}-\\u{62}", "[\\u{61}-\\u{62}]", "[\\u{62}-\\u{61}]", "\\u0061", "\\ud83d\\ude00", "\\ud83d", "\\ude00", "[\\ud83d]", "[\\ude00]", "[\\ud83d\\ude00]", "[^\\ud83d\\ude00]", "[\\ud83d-\\ude00]", "\\ud83d\\ud83d", "\\ude00\\ud83d", "\\uD83D\\uDE00", "\\u{1F600}", "\\u{1f600}+", "\\u{d83d}\\u{de00}", "\\u{d83d}", "\\u{de00}", "\\u{FFFF}", "\\u{10000}", "\\u{FFFFF}", "\\u{10FFFF}", "\\u{110000}", "\\u{FFFFFFFF}", "\\u{ 1}", "\\u{1 }", "\\u{-1}", "\\u{+1}", "\\u{1.0}", "\\u{0x1}",
];
const reFlags = ["", "g", "i", "m", "s", "u", "v", "y", "d", "gi", "gimsuyd", "uv", "gg", "x", "G", "ii", "dd", "gu", "iu", "vi", "gv", "iv", "uu", "vv", "a", "1", "\\u0067", "g\\u0069", "gy", "sy", "ym"];
for (const b of reBodies) {
  for (const f of ["", "u", "v", "g", "i", "uv", "gg", "x", "m", "iu", "iv", "s", "y", "d", "gimsuyd"]) {
    const lit = `/${b}/${f}`;
    add(`(${lit})`, "F");
    add(`x=${lit}`, "F");
  }
  // Via construtor RegExp e fonte do literal em posições onde `/` pode ser divisão.
  add(`1 /${b}/ 2`, "F");
  add(`new RegExp(${JSON.stringify(b)})`, "F");
  add(`new RegExp(${JSON.stringify(b)},"u")`, "F");
  add(`new RegExp(${JSON.stringify(b)},"v")`, "F");
  add(`new RegExp(${JSON.stringify(b)},"gi")`, "F");
}
for (const f of reFlags) {
  add(`/a/${f}`, "F");
  add(`(/a/${f})`, "F");
  add(`new RegExp("a",${JSON.stringify(f)})`, "F");
  add(`/(?:)/${f}.x`, "F");
  add(`x=/a/${f}\n1`, "F");
  add(`/a/${f}1`, "F");
  add(`/a/${f}$`, "F");
  add(`/a/${f}_`, "F");
  add(`/a/${f}\\u0067`, "F");
}
for (const s of [
  "/a/\\u0067", "/a/\\u{67}", "/a/g\\u0069", "/a/\\x67", "/a/g\\", "/a/ g", "/a/\ng", "/a/\u2028g", "/a/g/", "/a/g/g", "/a/gg", "/a/gimsuyd", "/a/gimsuvyd", "/a/gimsuyd1", "/a/é", "/a/\u00e9", "/a/g\u00e9", "/a/_", "/a/$", "/a/\u200c", "/a/\u200d", "/a/g\u200d", "/*/", "/**/", "//", "/\n/", "/\r/", "/", "/a", "/a\n/", "/[/]/", "/[/", "/[\\]/]/", "/[]/", "/[^]/", "/\\//", "/\\/", "a=/=/", "a/=1", "a /= 1", "a/ =1", "/=/", "/=a/", "/==/", "/=\\/", "/a/.test('a')", "/a/ .test('a')", "/a/\n.test('a')", "/a/g.lastIndex", "/a/g?.lastIndex", "/a/ in x", "/a/instanceof RegExp", "x/a/g", "x /a/ g", "x /a/g", "x/ /a/ ", "(x)/a/g", "x++/a/g", "x--/a/g", "++/a/.x", "typeof/a/", "void/a/", "delete/a/", "new/a/", "in/a/", "return/a/", "{}/a/", "(function(){})/a/g", "[]/a/g", "`x`/a/g", "1/a/g", "1/a/", "'a'/a/g", "this/a/g", "null/a/g", "true/a/g", "a?/b/:/c/", "a?/b/g:/c/g", "a:/b/", "case/a/:", "a=>/b/", "a=>{}/b/", "a=>{}\n/b/", "()=>/a/", "()=>{}/a/", "yield/a/", "await/a/", "async/a/", "of/a/", "let/a/", "let /a/", "x=let/a/", "var let/a/", "if(1)/a/", "if(1)/a/g;else/b/", "if(x)/a/.test(1)", "while(1)/a/;", "do/a/;while(0)", "for(;;)/a/;", "with(a)/a/", "(1)/a/g", "if(1)(1)/a/g", "if(x)y\n/a/g", "x\n/a/g", "x\n/a/g\n/b/", "}/a/g", "{/a/g}", "{}/a/g", "({})/a/g", "({}/a/g)", "class C{}/a/g", "class C{}\n/a/g", "(class{})/a/g", "function f(){}/a/g", "function f(){}\n/a/g", "(function(){})/a/g", "x=function(){}/a/g", "x=class{}/a/g", "x=()=>{}/a/g", "x=a=>{}\n/b/g", "x=a=>{}/b/g", "x=a=>/b/g", "x=async a=>/b/g", "x=async()=>/b/g", "x=async()=>{}/b/g", "x=async()=>{}\n/b/g", "x=async function(){}/b/g", "x=function*(){}/b/g", "x=async function*(){}/b/g", "x=(function(){}())/b/g", "x=function(){}()/b/g", "x=new function(){}/b/g", "x=new class{}/b/g",
]) both(s);

// ---- 7. Números.
const nums = [
  "1__0", "1_", "1_0", "_1", "1_.5", "1._5", "1.5_", "1.5_5", "1e_5", "1e5_", "1e+_5", "1_e5", "1e1_0", "1e1__0", "0_1", "0_0", "00_1", "01_0", "0_", "0_8", "08_1", "09_0", "07_7", "0.0_1", "0._1", "0e_1", "0b_1", "0b1_", "0b1_0", "0b1__0", "0b_", "0_b1", "0x_1", "0x1_", "0x1_f", "0x1__f", "0x_", "0_x1", "0o_1", "0o1_", "0o1_7", "0o1__7", "0o_", "0_o1", "1_n", "1n_", "1_0n", "1__0n", "0b1_0n", "0x1_fn", "0o1_7n", "0_n", "0_0n", "1_000_000", "1_000__000", "1_000_", "1_000._5", "1_000.5_5", ".1_1", "._1", ".1_", "1_.", "1._", "1_.e1", "1.e_1",
  "08", "09", "08.5", "09.5", "08e1", "08n", "09n", "07", "010", "0777", "0778", "07n", "010n", "00", "000", "00.5", "00n", "01.5", "01e1", "01n", "0_1n", "08.", "08.e1", "08_5", "0.8", "0.08", "00.8", "07.5", "07.", "07e1", "0e1", "0E1", "0.e1", "0.E1", "0e", "0e+", "0e-", "0e+1", "0e-1", "0.5e", "0.5e+", "0.5e-", "1e", "1e+", "1e-", "1E", "1E+", "1E-", "1ee1", "1e1e1", "1e1.5", "1e.5", "1e+.5", "1e1_", "1.5.5", "1..5", "1...5", "1.5..5", "1.e5", "1.E5", ".e5", ".5e", "1.toString()", "1..toString()", "1.0.toString()", "1 .toString()", "1.e1.toString()", "0.toString()", "0..toString()", "08.toString()", "08..toString()", "07.toString()", "07..toString()", "0b1.toString()", "0b1..toString()", "0x1.toString()", "0x1..toString()", "0o1.toString()", "0o1..toString()", "1n.toString()", "1n..toString()", "1n.5", "1n.", "0b1n.toString()",
  "0b", "0B", "0b2", "0b12", "0b1 2", "0b102", "0b1a", "0b1_a", "0b1.5", "0b1e1", "0b1n", "0B1N", "0b1N", "0b1nn", "0b1n1", "0b1_1n", "0b.1", "0b-1", "0b+1", "0b 1", "0b\n1", "0b0", "0b00", "0b01", "0b11111111111111111111111111111111111111111111111111111111111111111", "0bg", "0b\u0661", "0b1\u0661", "0b1\u00e9", "0b1_\u00e9",
  "0o", "0O", "0o8", "0o18", "0o1 8", "0o1a", "0o1.5", "0o1e1", "0o1n", "0O1N", "0o1nn", "0o1n1", "0o.1", "0o-1", "0o 1", "0o0", "0o00", "0o01", "0o7", "0o77", "0o777", "0o7777777777777777777777", "0og", "0o9", "0o1_8", "0o1\u0661",
  "0x", "0X", "0xg", "0x1g", "0x1 g", "0x1.5", "0x1e1", "0x1e+1", "0x1n", "0X1N", "0x1nn", "0x1n1", "0x.1", "0x-1", "0x 1", "0x0", "0x00", "0x01", "0xf", "0xF", "0xff", "0xFFFFFFFFFFFFFFFFFFFF", "0xfn", "0xe+1", "0xe-1", "0x1_g", "0x1\u0661", "0xa_b_c", "0xa__b", "0xa_", "0x_a", "0x1.toString()", "0x1..toString()", "0x1 .toString()",
  "1a", "1_a", "1a_", "1$", "1_$", "1\\u0061", "1\u00e9", "1\u0661", "3in x", "3in[]", "3 in[]", "1in x", "1 in x", "0in x", "0b1in x", "0o1in x", "0x1in x", "1instanceof x", "1 instanceof x", "1n in x", "1nin x", "1if", "1else", "1var", "1in", "1.in", "1.5in", "1e1in", "1.e1in", "1_0in", "01in", "08in", "1n instanceof x", "1ninstanceof x", ".5in x", ".5instanceof x", "1.a", "1.5a", "1.5_a", "1e1a", "1n.a", "0b1.a", "0x1.a", "0o1.a", "01.a", "08.a", ".1a", ".1.a", ".1..a", ".a", "..1", "...1", "1...a", "1.\\u0061",
  "1n", "0n", "00n", "-1n", "1N", "1.5n", "1e5n", "1e-5n", "0.0n", ".1n", "1_0n", "0xFFn", "0b1n", "0o7n", "0_0n", "1n+1", "1n**1n", "-1n**1n", "(-1n)**1n", "+1n", "~1n", "1n>>>1n", "1n/0n", "1n%0n", "1nn", "1n1", "1n_1", "1n_", "1n.toString()", "1n\u0661", "1n\u00e9", "1n\\u0061", "1nin x", "1n in x",
  "9007199254740993", "1e400", "-1e400", "1e-400", "0.0000001", "123456789012345678901234567890", "1.7976931348623157e308", "1.7976931348623159e308", "5e-324", "2e-324", "1e1000000000", "1e-1000000000", "0e1000000000",
];
for (const n of nums) {
  both(n);
  add(`x=${n}`, "F");
  add(`(${n})`, "F");
  add(`[${n}]`, "F");
  add(`x=[${n},${n}]`, "F");
  add(`({a:${n}})`, "F");
  add(`({${n}:1})`, "F");
  add(`({${n}(){}})`, "F");
  add(`class C{${n}(){}}`, "F");
  add(`class C{${n}=1}`, "F");
  add(`x.${n}`, "F");
  add(`x[${n}]`, "F");
  add(`-${n}`, "F");
  add(`${n}\n`, "F");
}
add("'use strict';({010:1})");
add("'use strict';({08:1})");
add("'use strict';({'\\01':1})");
add("'use strict';x=010;");
add("'use strict';x=0_1");
add("'use strict';x=00");
add("'use strict';x=07");
add("function f(){'use strict';010}");
add("function f(){010;'use strict'}");
add("function f(){'use strict';08}");
add("function f(){'use strict';09.5}");
add("function f(){'use strict';0_1}");
add("function f(){'use strict';0o10}");
add("function f(){'use strict';0.10}");
add("function f(){'use strict';0e10}");
add("function f(){'use strict';0.5}");
add("function f(){'use strict';.5}");
add("function f(){'use strict';0n}");
add("function f(){'use strict';0}");
add("function f(){'use strict';00n}");
add("function f(a=010){'use strict'}");
add("function f(a=08){'use strict'}");
add("(a=010)=>{'use strict'}");
add("010;'use strict'");
add("'use strict';010");
add("function f(){010;function g(){'use strict';010}}");
add("function f(){'use strict';function g(){010}}");
add("function f(){'use strict';()=>010}");
add("function f(){'use strict';class C{x=010}}");
add("class C{x=010}");
add("class C{m(){010}}");
add("class C{[010](){}}");
add("class C extends (010){}");
add("({m(){010}})");
add("`${010}`");
add("`${08}`");
add("010`x`");

// ---- 8. Strings com escapes inválidos.
const strEsc = [
  "\\x", "\\x4", "\\x41", "\\xg1", "\\x4g", "\\xGG", "\\u", "\\u1", "\\u12", "\\u123", "\\u1234", "\\u12g4", "\\u{", "\\u{}", "\\u{1", "\\u{1}", "\\u{10FFFF}", "\\u{110000}", "\\u{FFFFFFFF}", "\\u{0000000000041}", "\\u{g}", "\\u{ 1}", "\\u{1 }", "\\u{-1}", "\\u{+1}", "\\u{1,2}", "\\u{1.0}",
  "\\0", "\\00", "\\01", "\\07", "\\08", "\\09", "\\1", "\\7", "\\8", "\\9", "\\10", "\\18", "\\19", "\\37", "\\38", "\\377", "\\378", "\\400", "\\777", "\\0a", "\\00a", "\\1a", "\\8a", "\\08a", "\\0\\0", "\\0 ", "\\0\\n", "\\00\\0", "\\400\\0",
  "\\a", "\\b", "\\c", "\\d", "\\e", "\\f", "\\q", "\\v", "\\z", "\\A", "\\_", "\\$", "\\'", "\\\"", "\\`", "\\\\", "\\/", "\\ ", "\\-", "\\\n", "\\\r", "\\\r\n", "\\\u2028", "\\\u2029", "\\\u0085", "\\\u00a0", "\\é", "\\😀", "\\\ud800", "\\\udc00",
  "\u2028", "\u2029", "\0", "\u0001", "\u007f", "\u0085", "\u00a0", "\ufeff", "\ud800", "\udc00", "\ud800\udc00", "😀", "\\ud800", "\\udc00", "\\ud800\\udc00", "\\u{d800}", "\\u{dc00}", "\\ud800\\u0041", "\\u0041\\udc00",
];
for (const e of strEsc) {
  for (const q of ["'", '"']) {
    const s = `${q}a${e}b${q}`;
    both(s);
    add(`x=${s}`, "F");
    add(`({${s}:1})`, "F");
    add(`class C{${s}(){}}`, "F");
    add(`x.y=${s}`, "F");
    add(`${s};'use strict'`, "F");
    add(`function f(){${s};'use strict'}`, "F");
    add(`function f(){'use strict';${s}}`, "F");
    add(`function f(a=${s}){'use strict'}`, "F");
    add(`${s}\n;`, "F");
  }
  add("`a" + e + "b`", "F");
  add("'use strict';`a" + e + "b`", "F");
  add("x=`a" + e + "b`", "F");
  add("x`a" + e + "b`", "F");
  add("x.y`a" + e + "b`", "F");
  add("x?.y`a" + e + "b`", "F");
  add("`${1}a" + e + "b`", "F");
  add("`a" + e + "b${1}`", "F");
  add("x`${1}a" + e + "b`", "F");
  add("x`a" + e + "b${1}`", "F");
  add("class C{x=`a" + e + "b`}", "F");
  add("(`a" + e + "b`)", "F");
  add("`\\u{110000}${1}`", "F");
  add(`import('a${e}')`, "E");
  add(`/${e}/`, "F");
  add(`/${e}/u`, "F");
}
for (const s of [
  "'", "\"", "'abc", "\"abc", "'abc\n'", "\"abc\n\"", "'abc\r'", "'abc\u2028'", "'abc\u2029'", "'abc\\", "\"abc\\", "'abc\\\n", "'a\\\nb'", "'a\\\r\nb'", "`abc", "`abc${", "`abc${1", "`abc${1}", "`abc${}`", "`abc${1,}`", "`abc${1 2}`", "`${`", "`${`${`", "`${`${1}`}`", "`${`${1}`", "`${'`'}`", "`\\``", "`\\${1}`", "`$`", "`${`", "`$${1}`", "`$\\{1}`", "`\r\n`", "`\n`", "`\u2028`", "`\\u{`", "`\\u{1`", "`\\x`", "`\\xg`", "`\\01`", "`\\1`", "`\\8`", "`\\08`", "`\\00`", "`\\0`", "`\\0a`", "`\\09`",
  "x`\\u{`", "x`\\x`", "x`\\01`", "x`\\1`", "x`\\8`", "x`\\08`", "x`\\00`", "x`\\0`", "x`\\0a`", "x`\\09`", "x`\\u`", "x`\\u1`", "x`\\u12`", "x`\\u123`", "x`\\u{110000}`", "x`\\u{g}`", "x`\\xg`", "x`\\x4`", "x`\\x4g`", "x`\\ug`", "x`\\u{}`", "x`\\u{1`", "x `\\u{`", "x\n`\\u{`", "x()`\\u{`", "x[0]`\\u{`", "(x)`\\u{`", "new x`\\u{`", "new x()`\\u{`", "x.y`\\u{`", "x?.y`\\u{`", "x?.`a`", "x?.`a${1}`", "x?.y`a`", "x?.y.z`a`", "x?.[0]`a`", "x?.()`a`", "x?.y()`a`", "x?.y?.`a`", "x?.y?.z`a`", "x`a``b`", "x`a`.y`b`", "x`a`?.y", "x`a`?.y`b`", "x`a`?.`b`", "(x?.y)`a`", "(x?.)`a`", "(x?.y.z)`a`", "a?.b`c`", "a?.b?.c`d`", "a?.b.c`d`", "a?.b[c]`d`", "a?.b()`c`", "a?.b?.()`c`", "a?.b`c`.d", "a?.b`c`?.d", "new a?.b", "new a?.b()", "new a?.()", "new a?.[0]", "new a?.b`c`", "new (a?.b)", "new (a?.b)()", "new (a?.())", "new a.b?.c", "new a().b?.c", "new new a?.b", "new a?.b?.c", "a?.b=1", "a?.b+=1", "a?.b++", "++a?.b", "a?.b--", "--a?.b", "a?.[0]=1", "a?.[0]++", "a?.()=1", "a?.().b=1", "a?.b.c=1", "a?.b[c]=1", "a?.b()=1", "(a?.b).c=1", "(a?.b)=1", "(a?.b)++", "[a?.b]=[]", "({x:a?.b}={})", "({x:a?.b=1}={})", "[a?.b=1]=[]", "[...a?.b]=[]", "({...a?.b}={})", "for(a?.b of []);", "for(a?.b in {});", "for(a?.b;;);", "for([a?.b] of []);", "for({x:a?.b} of []);", "(a?.b)=>1", "(a?.b=1)=>1", "async(a?.b)=>1", "a?.b??c", "a?.b?.c??d", "a??b?.c", "a?.b||c", "a?.b&&c", "a?.b?c:d", "a?.b ? .5 : 1", "a?.5:1", "a?.5", "a ?.5:1", "a?.\n5", "a?.5.b", "a?. 5", "a ? .5 : 1", "a?.b?.5:1", "a?.[0]?.5:1", "a?.()?.5:1", "a? .5:1", "a ?. 5 : 1", "a?.5", "a?.e5", "a?.0", "a?.0e1", "a?.b.5", "a?.#b", "a?.b.#c", "class C{#b;m(){a?.#b}}", "class C{#b;m(){a?.b.#b}}", "class C{#b;m(){this?.#b}}", "class C{#b;m(){this?.a?.#b}}", "class C{#b;m(){this.#b?.x}}", "class C{#b;m(){this?.#b?.x}}", "class C{#b;m(){this.#b?.()}}", "class C{#b;m(){this?.#b()}}", "class C{#b;m(){this?.#b=1}}", "class C{#b;m(){this?.#b++}}", "class C{#b;m(){[this?.#b]=[]}}", "class C{#b;m(){({a:this?.#b}={})}}", "class C{#b;m(){#b in this?.x}}", "class C{#b;m(){this?.x in #b}}",
]) both(s);

// ---- 9. Alvos de atribuição inválidos e destructuring.
const targets = [
  "1", "1.5", "'a'", "`a`", "`${a}`", "a`b`", "true", "false", "null", "this", "new.target", "super", "import.meta", "a()", "a(b)", "a?.b", "a?.()", "a?.[0]", "a.b?.c", "a?.b.c", "(a?.b)", "(a?.b).c", "(a,b)", "(a,b,c)", "a,b", "(a)", "((a))", "(a.b)", "((a.b))", "(a[b])", "(a=1)", "a=1", "(a+b)", "a+b", "a-b", "a*b", "a**b", "a/b", "a%b", "a<<b", "a>>b", "a>>>b", "a<b", "a>b", "a<=b", "a>=b", "a==b", "a!=b", "a===b", "a!==b", "a&b", "a|b", "a^b", "a&&b", "a||b", "a??b", "a?b:c", "a in b", "a instanceof b", "!a", "~a", "-a", "+a", "typeof a", "void a", "delete a", "delete a.b", "++a", "--a", "a++", "a--", "(++a)", "(a++)", "(a--)", "new a", "new a()", "new a.b", "new a().b", "new.target.x", "function(){}", "(function(){})", "function f(){}", "class{}", "(class{})", "()=>1", "(()=>1)", "async()=>1", "async a=>1", "a=>1", "{}", "{a}", "{a:1}", "({})", "({a})", "({a:1})", "({a:b})", "({a:b.c})", "({a:b()})", "({a:1}.x)", "[]", "[a]", "[1]", "[a,b]", "[a,,b]", "[...a]", "[...a,b]", "[...a,]", "[a,...b]", "[...[a]]", "[...{a}]", "[...[a],b]", "[a=1]", "[a=1,b]", "[(a)]", "[(a.b)]", "[(a=1)]", "[(a,b)]", "[(a)=1]", "[((a))]", "[(a?.b)]", "[...(a)]", "[...(a.b)]", "[...(a=1)]", "[...a=1]", "[...a,...b]", "[1,2]", "[a+1]", "[a()]", "[a?.b]", "[new.target]", "[this]", "[`a`]", "['a']", "[null]", "[true]", "[super.x]", "({a:[b]})", "({a:{b}})", "({a:[b]=1})", "({a:{b}=1})", "({a:1=1})", "({a=1})", "({a=1,b})", "({a,b=1})", "({a:b=1})", "({a:(b)=1})", "({a:(b=1)})", "({a:(b)})", "({a:((b))})", "({a:(b.c)})", "({a:(b,c)})", "({...a})", "({...a,b})", "({...a,})", "({a,...b})", "({...{a}})", "({...[a]})", "({...a.b})", "({...a=1})", "({...(a)})", "({...(a.b)})", "({...a?.b})", "({...a()})", "({...1})", "({...a,...b})", "({[a]:b})", "({[a]})", "({[a]=1})", "({'a':b})", "({'a'})", "({1:b})", "({1})", "({a(){}})", "({get a(){}})", "({set a(v){}})", "({async a(){}})", "({*a(){}})", "({a:function(){}})", "({a:()=>1})", "({a:class{}})", "({__proto__:a})", "({__proto__:a,__proto__:b})", "({__proto__:a,'__proto__':b})", "({__proto__:a,['__proto__']:b})", "({__proto__:a,__proto__})", "({__proto__,__proto__:b})", "({__proto__(){},__proto__:a})", "({__proto__:a,__proto__(){}})", "({get __proto__(){},__proto__:a})", "({__proto__:a,__proto__:b}={})", "({__proto__:a,__proto__:b})=>1", "({__proto__:a,__proto__:b}=x)", "[{__proto__:a,__proto__:b}]=[]", "({a=1})", "({a=1}.x)", "[{a=1}]", "[{a=1}]=[]", "({x:{a=1}})", "({x:{a=1}}=y)", "({x:{a=1}}).y", "f({a=1})", "f({a=1},{b=2})", "f({a=1})=>1", "(({a=1}))", "(({a=1})=>1)", "(({a=1})=1)", "({a=1})=1", "({a=1}=1)", "({a=1}+1)", "[({a=1})]=[]", "[({a:b})]=[]", "[({a})]=[]", "[([a])]=[]", "[([a])=1]=[]", "({a:({b})}={})", "({a:([b])}={})", "({a:({b})=1}={})", "({a:(b)=1}={})", "({a:(b=1)}={})", "({a:((b))}={})",
];
const assigns = ["=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=", "??="];
for (const t of targets) {
  both(`${t}=1`);
  both(`${t}+=1`);
  both(`${t}&&=1`);
  both(`${t}??=1`);
  both(`${t}++`);
  both(`--${t}`);
  both(`for(${t} of []);`);
  both(`for(${t} in {});`);
  both(`for(var ${t} of []);`);
  both(`for(let ${t} of []);`);
  both(`for(const ${t} in {});`);
  both(`var ${t}=1`);
  both(`let ${t}=1`);
  both(`const ${t}=1`);
  both(`function f(${t}){}`);
  both(`(${t})=>1`);
  both(`try{}catch(${t}){}`);
  both(`[${t}]=[]`);
  both(`({a:${t}}={})`);
  both(`[a=${t}]=[]`);
  both(`(${t})`);
  both(`x=${t}`);
  both(`async(${t})=>1`);
}
for (const op of assigns) {
  for (const l of ["1", "a?.b", "(a,b)", "f()", "this", "a.b", "a[b]", "(a)", "((a.b))", "[a]", "({a})", "new.target", "a++", "++a", "-a", "a+b", "(a=1)", "a=b", "`x`", "null", "super.x", "a?.()", "(a?.b)", "async()=>1", "()=>1", "a=>1", "function(){}", "class{}", "eval", "arguments", "yield", "await", "undefined", "NaN", "let", "async", "of", "get"]) {
    both(`${l}${op}1`);
    both(`(${l})${op}1`);
    both(`x=${l}${op}1`);
    both(`${l} ${op} 1`);
  }
}
for (const s of [
  "a?.b=1", "a?.b.c=1", "a?.[0]=1", "(a,b)=1", "f()=1", "f()+=1", "f()++", "--f()", "1=2", "1++", "--1", "'a'=1", "this=1", "this++", "null=1", "true=1", "eval=1", "arguments=1", "eval++", "arguments--", "++eval", "--arguments", "undefined=1", "NaN=1", "Infinity=1",
  "new.target=1", "import.meta=1", "a=>1=2", "()=>1=2", "a?.b++", "++a?.b", "a?.b--", "[a?.b]=[]", "({a:b?.c}={})", "for(a?.b of []);", "for(a?.b in {});", "for(f() of []);", "for(f() in {});", "for(1 of []);", "for(1 in {});", "for(this of []);", "for(this in {});", "for((a,b) of []);", "for((a,b) in {});",
  "for(a=1 of []);", "for(a=1 in {});", "for(var a=1 of []);", "for(var a=1 in {});", "for(let a=1 of []);", "for(let a=1 in {});", "for(const a=1 of []);", "for(const a=1 in {});", "for(var [a]=1 of []);", "for(var [a]=1 in {});", "for(var {a}=1 of []);", "for(var {a}=1 in {});", "for(var a,b of []);", "for(var a,b in {});", "for(let a,b of []);", "for(let a,b in {});", "for(const a of []);", "for(const a in {});", "for(a of b,c);", "for(a in b,c);", "for(a of b=c);", "for(a in b=c);", "for(a of (b,c));", "for(a in b,c);", "for(let of x);", "for(let of of x);", "for(let.x of y);", "for(let.x in y);", "for(let[a] of y);", "for(let[a] in y);", "for(let\n[a] of y);", "for(let in y);", "for(let=1 in y);", "for(let=1;;);", "for(let;;);", "for(let\n;;);", "for(var let of y);", "for(var let in y);", "for(async of y);", "for(async.x of y);", "for(async in y);", "for((async) of y);", "for(async\nof y);", "for(\\u0061sync of y);", "for(async of=>1;;);", "for(async of=>1 of y);", "for(async of y);", "for(a of []);", "for(a.b of []);", "for(a[b] of []);", "for([a] of []);", "for({a} of []);", "for([a]=1 of []);", "for({a}=1 of []);", "for([a].b of []);", "for({a}.b of []);", "for((a) of []);", "for(((a)) of []);", "for((a.b) of []);", "for(([a]) of []);", "for(({a}) of []);", "for([(a)] of []);", "for({a:(b)} of []);", "for([(a.b)] of []);", "for([...a] of []);", "for([...a,b] of []);", "for([a,...b] of []);", "for({...a} of []);", "for({...a,b} of []);", "for({a,...b} of []);", "for(x of y)let\nz", "for(x of y)let z", "for(x of y)const z=1", "for(x of y)class C{}", "for(x of y)function f(){}", "for(x of y)label:function f(){}", "for(x in y)let z", "for(x in y)class C{}", "for(;;)let z", "for(;;)let\nz", "for(;;)let\n[z]", "for(;;)let[z]", "while(0)let\nz", "while(0)let[z]", "if(0)let\nz", "if(0)let[z]", "if(0);else let z", "if(0);else let\nz", "if(0);else let[z]", "if(0);else class C{}", "if(0);else function f(){}", "if(0);else const z=1", "if(0)function f(){}else function g(){}", "if(0)function f(){}else;", "if(0);else function f(){}", "if(0)function* g(){}", "if(0)async function f(){}", "if(0)label:function f(){}", "if(0)l1:l2:function f(){}", "if(0){}else l:function f(){}", "do let\nz;while(0)", "do let[z];while(0)", "do function f(){}while(0)", "do class C{}while(0)", "do const z=1;while(0)", "do;while(0)x", "do;while(0);x", "do;while(0)\nx", "do;while(0)}", "{do;while(0)}", "do;while(0)do;while(0)", "do;while(0)1", "do;while(0)a:1", "do x\nwhile(0)", "do x;\nwhile(0)", "do x\n;while(0)", "do x;while(0)y", "do x;while(0);y",
]) both(s);

// ---- 10. Classes: campos duplicados, constructor especial, #privados.
const members = [
  "constructor(){}", "'constructor'(){}", "[`constructor`](){}", "['constructor'](){}", "get constructor(){return 1}", "set constructor(v){}", "*constructor(){}", "async constructor(){}", "async *constructor(){}", "static constructor(){}", "static get constructor(){return 1}", "static *constructor(){}", "static async constructor(){}", "static 'constructor'(){}", "static ['constructor'](){}", "constructor=1", "'constructor'=1", "static constructor=1", "static 'constructor'=1", "static ['constructor']=1", "['constructor']=1", "constructor", "'constructor'", "static constructor", "get 'constructor'(){return 1}", "set 'constructor'(v){}", "*'constructor'(){}", "async 'constructor'(){}", "#constructor", "#constructor=1", "#constructor(){}", "static #constructor", "get #constructor(){}", "static constructor(){}static constructor(){}", "constructor(){}constructor(){}", "constructor(){}'constructor'(){}", "'constructor'(){}\"constructor\"(){}", "constructor(){};constructor(){}", "constructor(){}static constructor(){}", "constructor(){}get constructor(){}",
  "prototype(){}", "static prototype(){}", "static 'prototype'(){}", "static ['prototype'](){}", "static get prototype(){}", "static set prototype(v){}", "static *prototype(){}", "static async prototype(){}", "static prototype=1", "static 'prototype'=1", "static ['prototype']=1", "static prototype", "prototype=1", "'prototype'=1", "get prototype(){}", "static #prototype", "static #prototype=1", "static #prototype(){}", "static async *prototype(){}", "static \\u0070rototype(){}", "static prot\\u006ftype(){}",
  "a(){}a(){}", "a;a;", "a=1;a=2", "a(){}a=1", "a=1;a(){}", "get a(){}get a(){}", "set a(v){}set a(v){}", "get a(){}set a(v){}", "set a(v){}get a(){}", "get a(){}a(){}", "a(){}get a(){}", "static a(){}static a(){}", "static a;static a;", "static a(){}a(){}", "a(){}static a(){}", "static get a(){}get a(){}", "static get a(){}static get a(){}", "static get a(){}static set a(v){}", "static a=1;a=2", "#a;#a", "#a;#a(){}", "#a(){}#a(){}", "get #a(){}get #a(){}", "set #a(v){}set #a(v){}", "get #a(){}set #a(v){}", "set #a(v){}get #a(){}", "static get #a(){}set #a(v){}", "get #a(){}static set #a(v){}", "static get #a(){}static set #a(v){}", "get #a(){}#a", "#a;get #a(){}", "#a=1;#a=2", "static #a;#a", "#a;static #a", "static #a;static #a", "static #a(){}#a(){}", "async #a(){}#a", "*#a(){}#a", "async *#a(){}get #a(){}", "get #a(){}get #a(){}set #a(v){}", "get #a(){}set #a(v){}set #a(v){}", "get #a(){}set #a(v){}#a",
  "'a'(){}a(){}", "1(){}1(){}", "1(){}'1'(){}", "1(){}1.0(){}", "0x1(){}1(){}", "1n(){}1(){}", "[a](){}[a](){}", "['a'](){}a(){}", "a(){}['a'](){}", "get a(){}get ['a'](){}", "a=1;['a']=2", "[1]=1;1=2", "'__proto__'(){}__proto__(){}", "__proto__:1", "__proto__(){}__proto__(){}", "__proto__=1;__proto__=2", "static __proto__(){}static __proto__(){}",
  "a", "a;b", "a b", "a\nb", "a\nb=1", "a=1\nb=2", "a=1 b=2", "a\n*b(){}", "a *b(){}", "a;*b(){}", "a=1\n*b(){}", "a\n[b]", "a\n[b]=1", "a=1\n[b]=2", "a\nstatic b", "a\nget b(){}", "a\nset b(v){}", "a\nasync b(){}", "get\nb(){}", "set\nb(v){}", "async\nb(){}", "static\nb(){}", "get;b", "set;b", "async;b", "static;b", "get\n;b", "get=1", "set=1", "async=1", "static=1", "get\n=1", "get(){}", "set(){}", "async(){}", "static(){}", "get\n(){}", "async\n(){}", "static\n(){}", "static static", "static static=1", "static static(){}", "static\nstatic", "static get", "static get=1", "static get(){}", "static set", "static async", "static async=1", "static async(){}", "static async\nb(){}", "static async\n*b(){}", "static *b(){}", "static async *b(){}", "static async\n*b(){}", "async\n*b(){}", "async *b(){}", "async* b(){}", "async *\nb(){}", "*\nb(){}", "* b(){}", "async\n(){}", "get *b(){}", "set *b(v){}", "get async b(){}", "static get *b(){}", "get\n*b(){}", "get b(){}()", "get b()", "get b", "get", "set", "static", "async", "get b(a){}", "set b(){}", "set b(a,b){}", "set b(...a){}", "set b(a=1){}", "set b([a]){}", "set b({a}){}", "set b(a,){}", "set b(){}", "get b(a=1){}", "get b(...a){}", "get b(,){}",
  "a=arguments", "a=()=>arguments", "a=function(){arguments}", "a=eval('arguments')", "a=eval", "static a=arguments", "static{arguments}", "static{()=>arguments}", "static{eval('arguments')}", "a=super.x", "a=super()", "a=new.target", "a=this", "a=await", "a=yield", "a=let", "a=static", "a=async", "a=of", "a=get", "a=#a in this", "[arguments]=1", "[eval]=1", "[this]=1", "[super.x]=1", "[new.target]=1", "[await]=1", "[yield]=1", "[a=arguments]=1", "[()=>arguments](){}", "[function(){arguments}](){}", "m(a=arguments){}", "m(){arguments}", "static m(){arguments}", "get m(){return arguments}", "m=function(){arguments}", "a=class{[arguments](){}}", "a=class{x=arguments}", "a=class{static{arguments}}", "a=class extends arguments{}", "a=class extends(arguments){}", "a=class{m(){arguments}}",
  "m(){this.#a}", "#a;m(){this.#a}", "#a;m(){this.#b}", "#a;m(){#a in this}", "#a;m(){#b in this}", "m(){#a in this}", "#a;m(){#a}", "#a;m(){#a in}", "#a;m(){in #a}", "#a;m(){this in #a}", "#a;m(){#a in #a}", "#a;m(){#a in #a in this}", "#a;m(){(#a) in this}", "#a;m(){(#a in this)}", "#a;m(){1+#a in this}", "#a;m(){#a in this+1}", "#a;m(){#a in this in this}", "#a;m(){#a<this}", "#a;m(){#a==this}", "#a;m(){#a+this}", "#a;m(){!#a in this}", "#a;m(){a=#a in this}", "#a;m(){a=#a}", "#a;m(){a?#a in this:1}", "#a;m(){for(#a in this;;);}", "#a;m(){for(var x=#a in this;;);}", "#a;m(){for(#a in this of []);}", "#a;m(){[#a in this]}", "#a;m(){({a:#a in this})}", "#a;m(){f(#a in this)}", "#a;m(){`${#a in this}`}", "#a;m(){#a in this&&1}", "#a;m(){1&&#a in this}", "#a;m(){1??#a in this}", "#a;m(){#a in this??1}", "#a;m(){1,#a in this}", "#a;m(){yield #a in this}", "#a;*m(){yield #a in this}", "#a;async m(){await #a in this}", "#a;m(){#a\nin this}", "#a;m(){#a in\nthis}", "#a;m(){# a in this}", "#a;m(){#\\u0061 in this}", "#a;m(){this.#\\u0061}", "#\\u0061;m(){this.#a}", "#\\u{61};m(){this.#a}", "#a;m(){this.# a}", "#a;m(){this.#\na}", "#a;m(){this . #a}", "#a;m(){this.\n#a}", "#a;m(){this#a}", "#a;m(){this.#}", "#a;m(){this.#1}", "#a;m(){this.#@}", "#;m(){}", "#1;m(){}", "# a;m(){}", "#a#b", "#a;#b", "#a #b", "#\\u0061;#a", "#\\u{61};#a", "#a;#\\u0061", "#\\u0061b;#ab", "#a\\u0062;#ab",
  "#a;m(){this?.#a}", "#a;m(){this?.b.#a}", "#a;m(){this.b?.#a}", "#a;m(){this.b?.c.#a}", "#a;m(){delete this.#a}", "#a;m(){delete this?.#a}", "#a;m(){delete (this.#a)}", "#a;m(){delete ((this.#a))}", "#a;m(){delete (this?.#a)}", "#a;m(){delete this.b.#a}", "#a;m(){delete this.#a.b}", "#a;m(){delete this.b?.#a}", "#a;m(){delete this.#a?.b}", "#a;m(){delete this?.b.#a}", "#a;m(){delete [this.#a]}", "#a;m(){delete{a:this.#a}}", "#a;m(){delete(0,this.#a)}", "#a;m(){delete(this.#a,0)}", "#a;m(){delete this.#a,0}", "#a;m(){delete(this.#a=1)}", "#a;m(){delete this.#a++}", "#a;m(){delete-this.#a}", "#a;m(){delete typeof this.#a}",
  "#a;m(){this.#a=1}", "#a;m(){this.#a++}", "#a;m(){[this.#a]=[]}", "#a;m(){({x:this.#a}={})}", "#a;m(){for(this.#a of []);}", "#a;m(){for(this.#a in {});}", "#a;m(){super.#a}", "#a;m(){super?.#a}", "#a;m(){new.target.#a}", "#a;m(){this.#a`x`}", "#a;m(){this.#a()}", "#a;m(){new this.#a}", "#a;m(){new this.#a()}", "#a;m(){this.#a.b}", "#a;m(){this.#a[b]}", "#a;m(){this.#a?.b}", "#a;m(){this.#a?.[b]}", "#a;m(){this.#a?.()}", "#a;static m(){this.#a}", "static #a;static m(){this.#a}", "static #a;m(){this.#a}", "#a;static m(){#a in this}", "get #a(){}m(){this.#a=1}", "set #a(v){}m(){this.#a}", "#a(){}m(){this.#a=1}", "#a(){}m(){this.#a++}", "#a(){}m(){[this.#a]=[]}", "#a(){}m(){delete this.#a}", "#a(){}m(){this.#a()}", "#a(){}m(){new this.#a}", "#a(){}m(){#a in this}",
  "m(){class D{#a;n(){this.#a}}}", "m(){class D{n(){this.#a}}}", "#a;m(){class D{n(){this.#a}}}", "#a;m(){class D{#a;n(){this.#a}}}", "#a;m(){class D{n(){this.#b}}}", "#a;m(){class D{#b;n(){this.#a}}}", "#a;m(){(class{n(){this.#a}})}", "#a;m(){(class{#a;n(){this.#a}})}", "#a;m(){(class{#b;n(){this.#a}})}", "#a;m(){(class{n(){this.#c}})}", "#a;m(){(class extends(this.#a){})}", "#a;m(){(class extends(this.#b){})}", "#a;m(){(class{[this.#a](){}})}", "#a;m(){(class{[this.#b](){}})}", "#a;m(){(class{x=this.#a})}", "#a;m(){(class{x=this.#b})}", "#a;m(){(class{static x=this.#a})}", "#a;m(){(class{static{this.#a}})}", "#a;m(){(class{static{this.#b}})}", "#a;m(){()=>this.#a}", "#a;m(){()=>this.#b}", "#a;m(){function f(){this.#a}}", "#a;m(){function f(){this.#b}}", "#a;m(){({n(){this.#a}})}", "#a;m(){({n(){this.#b}})}", "#a;m(){eval('this.#a')}", "#a;m(){eval('this.#b')}", "#a;m(){new Function('this.#a')}", "#a;m(){new Function('return this.#b')}",
  "static{}", "static{;}", "static{this.x=1}", "static{var a;var a}", "static{let a;let a}", "static{var a;let a}", "static{let a;var a}", "static{function a(){}function a(){}}", "static{function a(){}var a}", "static{function a(){}let a}", "static{class a{}var a}", "static{const a=1;var a}", "static{await}", "static{await 1}", "static{var await}", "static{yield}", "static{var yield}", "static{return}", "static{break}", "static{continue}", "static{a:break a}", "static{a:{break a}}", "static{while(1)break}", "static{arguments}", "static{()=>arguments}", "static{(function(){arguments})}", "static{super.x}", "static{super()}", "static{new.target}", "static{this}", "static{async function f(){await 1}}", "static{async()=>await 1}", "static{function* g(){yield}}", "static{(await)=>1}", "static{(a=await)=>1}", "static{class await{}}", "static{({await:1})}", "static{({await})}", "static{x.await}", "static{await:1}", "static{try{}catch(await){}}", "static{try{}catch{var await}}", "static{import('a')}", "static{import.meta}", "static{'use strict';010}", "static{010}", "static{with(a);}", "static{delete a}", "static{eval=1}", "static{arguments=1}", "static{var eval}", "static{var arguments}", "static{let let}", "static{var let}", "static{var static}", "static{var implements}", "static{var yield}",
  "static{}static{}", "static{}static{}static{}", "static{}a=1;static{}", "static async{}", "static\n{}", "static async\n{}", "static *{}", "static get{}", "static {} {}", "static x{}", "static {a:1}", "static {a}", "static {a;b}", "static {1}", "static {,}", "static {;;}", "static {}\n{}",
  "'a'", "'a'=1", "'a'(){}", "1", "1=1", "1(){}", "1n(){}", "1n=1", ".5(){}", ".5=1", "[1,2](){}", "[1,2]=1", "[a,b](){}", "[a=b](){}", "[a?b:c](){}", "[a,b]", "[]", "[]=1", "[](){}", "[...a](){}", "[yield](){}", "*[yield](){}", "[await](){}", "async[await](){}", "async*[await](){}", "async*[yield](){}", "*[await](){}", "*[yield 1](){}", "async[await 1](){}", "[super.x](){}", "[super()](){}", "[this](){}", "[new.target](){}", "[arguments](){}", "[#a in this](){}", "#a;[#a in this](){}", "#a;[this.#a](){}",
  "a(){}b(){}", "a(){};b(){}", "a(){}\n;b(){}", ";", ";;", "a(){};;", ";a(){}", "a();", "a()", "a(){} b", "a b(){}", "a(){}}", "a(){", "a(", "a", "{", "}", "(){}", "()", "(", ")", "a(){}(", "a,b", "a,b(){}", "a(){},b(){}", "a:1", "a:b", "a=>1", "a=1,b=2", "a,", "a;,", "a(){}.b", "a.b", "a.b(){}", "a.b=1", "a['b']", "a[b]=1", "a[b](){}", "'a'.b", "(a)(){}", "(a)=1", "{a}", "{a}=1", "{a}(){}", "...a", "...a=1", "a...", "a=...b", "?", "a?", "a?b", "a?b:c", "a??b", "!a", "~a", "-a", "+a", "typeof a", "void a", "delete a", "new a", "this", "super", "null", "true", "false", "function(){}", "function f(){}", "class{}", "class A{}", "async function f(){}", "function*g(){}", "var a", "let a", "const a=1", "if(1);", "for(;;);", "while(1);", "do;while(0)", "try{}catch{}", "throw 1", "return", "break", "continue", "debugger", "with(a);", "switch(1){}", "label:1", "import a from 'b'", "export var a", "var\na", "let\na", "const\na=1", "get\n a", "set\n a", "async\n a", "static\n a",
];
for (const m of members) {
  add(`class C{${m}}`, "F");
  add(`class C extends D{${m}}`, "F");
  add(`(class{${m}})`, "F");
  add(`'use strict';class C{${m}}`, "F");
  add(`({${m}})`, "F");
}
for (const s of [
  "class", "class{", "class{}", "class C", "class C{", "class C{}", "class C extends", "class C extends{", "class extends C{}", "class extends{}", "(class extends C{})", "(class{})", "(class C{})", "class let{}", "class static{}", "class yield{}", "class await{}", "class async{}", "class of{}", "class get{}", "class set{}", "class implements{}", "class interface{}", "class package{}", "class private{}", "class protected{}", "class public{}", "class enum{}", "class eval{}", "class arguments{}", "class null{}", "class true{}", "class this{}", "class new{}", "class if{}", "class class{}", "class function{}", "class var{}", "class const{}", "class undefined{}", "class NaN{}", "class Infinity{}", "class Object{}", "(class let{})", "(class static{})", "(class yield{})", "(class await{})", "(class eval{})", "(class arguments{})", "(class implements{})", "(class enum{})", "(class null{})", "(class if{})",
  "class C{}class C{}", "class C{};class C{}", "class C{}var C", "var C;class C{}", "class C{}let C", "let C;class C{}", "class C{}const C=1", "class C{}function C(){}", "function C(){}class C{}", "class C{}C=1", "class C{C=1}", "class C{static C=1}", "class C{static C(){}}", "class C{m(){C=1}}", "class C{m(){class C{}}}", "class C{m(){var C}}", "class C{m(){let C}}", "class C{m(){function C(){}}}", "class C{m(C){}}", "class C{m(){try{}catch(C){}}}", "class C extends C{}", "class C extends (C){}", "class C extends(()=>C){}", "(class C extends C{})", "var C=class C{}", "var C=class C{m(){C=1}}", "var C=class C{m(){var C}}", "var C=class C{m(C){}}", "(class C{static C(){}})", "(class C{C=1})",
  "class C{constructor(){}}class D extends C{constructor(){}}", "class C extends D{constructor(){}}", "class C extends D{}", "class C extends D{constructor(...a){super(...a)}}", "class C extends D{constructor(){super();}}", "class C extends D{constructor(){return}}", "class C extends D{constructor(){return 1}}", "class C extends D{constructor(){return undefined}}", "class C extends D{constructor(){return{}}}", "class C extends D{constructor(){return super()}}", "class C extends D{constructor(){return this}}", "class C extends D{constructor(){this.a=1;super()}}", "class C extends D{constructor(){super();this.a=1}}", "class C extends D{constructor(){()=>super()}}", "class C extends D{constructor(){()=>this}}", "class C extends D{constructor(){eval('super()')}}", "class C extends D{constructor(){eval('this')}}", "class C extends D{constructor(){new.target}}", "class C extends D{constructor(){arguments}}", "class C extends D{constructor(){var super}}", "class C extends D{constructor(){super=1}}", "class C extends D{constructor(){super\n()}}", "class C extends D{constructor(){super ()}}", "class C extends D{constructor(){super.()}}", "class C extends D{constructor(){super..x}}", "class C extends D{constructor(){super[]}}",
  "class C{'use strict'(){}}", "class C{m(){'use strict';010}}", "class C{m(){010}}", "class C{m(a,a){}}", "class C{m(eval){}}", "class C{m(arguments){}}", "class C{m(yield){}}", "class C{m(let){}}", "class C{m(static){}}", "class C{m(await){}}", "class C{m(implements){}}", "class C{m(){var eval}}", "class C{m(){eval=1}}", "class C{m(){with(a);}}", "class C{m(){delete a}}", "class C{m(){var let}}", "class C{m(){var static}}", "class C{m(){var yield}}", "class C{m(){var implements}}", "class C{m(){var interface}}", "class C{m(){var package}}", "class C{m(){var private}}", "class C{m(){var protected}}", "class C{m(){var public}}", "class C{m(){var enum}}", "class C{m(){var await}}", "class C{m(){let let}}", "class C{m(){let static}}", "class C{m(){function static(){}}}", "class C{m(){static:1}}", "class C{m(){let:1}}", "class C{m(){yield:1}}", "class C{m(){implements:1}}", "class C{m(){x=let}}", "class C{m(){x=static}}", "class C{m(){x=yield}}", "class C{m(){x=implements}}", "class C{m(){let\nx}}", "class C{m(){let\n[x]=[]}}", "class C{m(){let[x]=[]}}", "class C{m(){if(1)let\nx}}", "class C{[let](){}}", "class C{[static](){}}", "class C{[yield](){}}", "class C{[implements](){}}", "class C{let(){}}", "class C{static(){}}", "class C{yield(){}}", "class C{implements(){}}", "class C{eval(){}}", "class C{arguments(){}}", "class C{await(){}}", "class C{async(){}}", "class C{get(){}}", "class C{set(){}}", "class C{of(){}}", "class C{if(){}}", "class C{class(){}}", "class C{new(){}}", "class C{null(){}}", "class C{true(){}}", "class C{this(){}}", "class C{enum(){}}", "class C{let=1}", "class C{static=1}", "class C{yield=1}", "class C{implements=1}", "class C{eval=1}", "class C{arguments=1}", "class C{await=1}", "class C{async=1}", "class C{get=1}", "class C{set=1}", "class C{of=1}", "class C{if=1}", "class C{class=1}", "class C{new=1}", "class C{null=1}", "class C{true=1}", "class C{this=1}", "class C{enum=1}",
]) both(s);
// ---- 11. Destructuring inválido (declarações e parâmetros).
for (const d of [
  "[a", "[a,", "[a,]", "[,a]", "[,]", "[,,]", "[...a", "[...a,]", "[...a,b]", "[...a=1]", "[...]", "[...,]", "[a,...]", "[a...]", "[a b]", "[a;b]", "[a:b]", "[a=]", "[a=1", "[=1]", "[1]", "[1=2]", "['a']", "[a.b]", "[a[b]]", "[a()]", "[(a)]", "[(a)=1]", "[a=(b)]", "[[a]]", "[[a]=1]", "[{a}]", "[{a}=1]", "[[a],[b]]", "[[a,[b]]]", "[...[a]]", "[...{a}]", "[...[...a]]", "[...[a,...b]]", "[...[a],b]", "[...[a],]", "[...{a},b]", "[...[]]", "[...{}]", "[[]]", "[{}]", "[[],{}]", "[a,a]", "[a,[a]]", "[a,{a}]", "[a,...a]", "[a=a]", "[a=b,c=a]", "[a,b=a]", "[let]", "[yield]", "[await]", "[static]", "[eval]", "[arguments]", "[this]", "[new.target]", "[super.x]", "[null]", "[true]", "[class]", "[if]", "[enum]", "[let=1]", "[let,let]", "[eval=1]", "[arguments=1]", "[a=yield]", "[a=await]", "[a=super.x]", "[a=new.target]", "[a=this]", "[a=arguments]", "[a=eval]",
  "{a", "{a,", "{a,}", "{,a}", "{,}", "{...a", "{...a,}", "{...a,b}", "{...a=1}", "{...}", "{...,}", "{a...}", "{...[a]}", "{...{a}}", "{...[]}", "{...{}}", "{...a.b}", "{...a[b]}", "{...(a)}", "{...a()}", "{...1}", "{a b}", "{a;b}", "{a:}", "{a:b", "{a:b,", "{a:b,}", "{a:b c}", "{:b}", "{a=}", "{a=1", "{a=1,", "{=1}", "{1}", "{1:a}", "{1:a=1}", "{'a'}", "{'a':b}", "{'a':b=1}", "{a:1}", "{a:1=2}", "{a:b.c}", "{a:b[c]}", "{a:b()}", "{a:(b)}", "{a:(b)=1}", "{a:b=(c)}", "{a:[b]}", "{a:[b]=1}", "{a:{b}}", "{a:{b}=1}", "{a:{b:c}}", "{a:{b:{c}}}", "{[a]}", "{[a]:b}", "{[a]:b=1}", "{[a]=1}", "{['a']:b}", "{[a+b]:c}", "{[a,b]:c}", "{[]:c}", "{[a]:}", "{get a(){}}", "{set a(v){}}", "{a(){}}", "{async a(){}}", "{*a(){}}", "{a(){}=1}", "{a:function(){}}", "{a:()=>1}", "{a,a}", "{a,b:a}", "{a:a,a}", "{a:b,a:b}", "{a,...a}", "{a:b,...b}", "{a=a}", "{a=b,b=a}", "{let}", "{yield}", "{await}", "{static}", "{eval}", "{arguments}", "{this}", "{null}", "{true}", "{class}", "{if}", "{enum}", "{let=1}", "{yield=1}", "{await=1}", "{eval=1}", "{arguments=1}", "{this=1}", "{null=1}", "{class=1}", "{if=1}", "{enum=1}", "{a:let}", "{a:yield}", "{a:await}", "{a:static}", "{a:eval}", "{a:arguments}", "{a:this}", "{a:null}", "{a:true}", "{a:class}", "{a:if}", "{a:enum}", "{if:a}", "{class:a}", "{null:a}", "{true:a}", "{this:a}", "{new:a}", "{let:a}", "{yield:a}", "{await:a}", "{static:a}", "{enum:a}", "{eval:a}", "{arguments:a}", "{if:a=1}", "{class:a=1}", "{a=yield}", "{a=await}", "{a=super.x}", "{a=new.target}", "{a=this}", "{a=arguments}", "{a=eval}", "{__proto__:a}", "{__proto__:a,__proto__:b}", "{__proto__:a,__proto__}",
]) {
  for (const k of ["var", "let", "const"]) {
    both(`${k} ${d}=x`);
    both(`${k} ${d}`);
    both(`for(${k} ${d} of x);`);
    both(`for(${k} ${d} in x);`);
    both(`for(${k} ${d}=x;;);`);
  }
  both(`${d}=x`);
  both(`(${d}=x)`);
  both(`(${d})=x`);
  both(`(${d})=>1`);
  both(`async(${d})=>1`);
  both(`function f(${d}){}`);
  both(`function f(a,${d}){}`);
  both(`function f(${d}=1){}`);
  both(`function f(...${d}){}`);
  both(`(function(${d}){})`);
  both(`({m(${d}){}})`);
  both(`class C{m(${d}){}}`);
  both(`try{}catch(${d}){}`);
  both(`for(${d} of x);`);
  both(`for(${d} in x);`);
  both(`for(${d}=1 of x);`);
  both(`[${d}]=x`);
  both(`({a:${d}}=x)`);
  both(`[...${d}]=x`);
  both(`({...${d}}=x)`);
  both(`(${d},a)=>1`);
  both(`(a,${d})=>1`);
  both(`(${d},${d})=>1`);
  both(`function f(${d},${d}){}`);
  both(`(...${d})=>1`);
  both(`(a,...${d})=>1`);
  both(`(...${d},a)=>1`);
  both(`(a,...${d},)=>1`);
  both(`({${d}})`);
}

// ---- 12. Vários: ASI, expressões, operadores, comentários e recursos de lexer.
for (const s of [
  "a ?? b || c", "a || b ?? c", "a ?? b && c", "a && b ?? c", "(a ?? b) || c", "a ?? (b || c)", "a ?? b ?? c", "a || b || c", "a ?? b ? c : d", "a ?? b, c", "a?.b ?? c", "-a ** b", "+a ** b", "!a ** b", "~a ** b", "typeof a ** b", "void a ** b", "delete a ** b", "await a ** b", "(-a) ** b", "(+a) ** b", "-(a ** b)", "a ** -b", "a ** +b", "a ** !b", "a ** typeof b", "a ** ++b", "++a ** b", "a++ ** b", "a-- ** b", "--a ** b", "a ** b ** c", "(a ** b) ** c", "a ** b++", "a ** b--", "a **= b ** c", "a **= -b", "-a **= b", "(-a) **= b", "async function f(){-await a ** b}", "async function f(){await a ** b}", "async function f(){(await a) ** b}", "async function f(){a ** await b}",
  "a\n++\nb", "a\n++b", "a++\nb", "a\n++", "++\na", "a\n--\nb", "a ++ b", "a ++ ++ b", "a + + b", "a + ++b", "a++ + b", "a+++b", "a---b", "a+ ++b", "a- --b", "a++++b", "a----b", "+++a", "---a", "++a++", "--a--", "++a--", "--a++", "(a++)++", "(++a)++", "++(++a)", "++(a)", "++((a))", "(a)++", "((a))++", "++(a.b)", "(a.b)++", "++a.b", "a.b++", "++a[b]", "a[b]++", "++a()", "a()++", "++a?.b", "a?.b++",
  "throw", "throw;", "throw\n1", "throw 1", "throw(1)", "throw 1;", "throw\n;", "throw /x/", "try", "try{", "try{}", "try{}catch", "try{}catch(", "try{}catch()", "try{}catch(a", "try{}catch(a)", "try{}catch(a){", "try{}catch(a){}", "try{}catch{", "try{}catch{}", "try{}finally", "try{}finally{", "try{}finally{}", "try{}catch(a){}finally", "try{}catch(a){}finally{}", "try{}catch(){}", "try{}catch(a,b){}", "try{}catch(a=1){}", "try{}catch(...a){}", "try{}catch([a]){}", "try{}catch({a}){}", "try{}catch([a],b){}", "try{}catch(a.b){}", "try{}catch(1){}", "try{}catch('a'){}", "try{}catch(let){}", "try{}catch(yield){}", "try{}catch(await){}", "try{}catch(static){}", "try{}catch(eval){}", "try{}catch(arguments){}", "try{}catch(this){}", "try{}catch(null){}", "try{}catch(class){}", "try{}catch(if){}", "try{}catch(enum){}", "try{}catch(implements){}", "try{}catch(async){}", "try{}catch(of){}", "try{}catch(get){}", "try{}catch(set){}", "try{}catch(\\u0061){}", "try{}catch(a){}catch(b){}", "try{}catch{}catch{}", "try{}finally{}finally{}", "try{}finally{}catch{}", "try{}catch(e){}\nfinally{}", "try{}\ncatch(e){}", "try\n{}catch(e){}", "try{}catch\n(e){}", "try{}catch(e)\n{}", "try;catch", "try x", "try{}x", "try{}catch(e)x", "try{}catch(e){}x", "try{}finally x", "try{}catch(e){}finally x",
  "switch", "switch(", "switch()", "switch(a", "switch(a)", "switch(a){", "switch(a){}", "switch(a){case}", "switch(a){case:}", "switch(a){case 1}", "switch(a){case 1:}", "switch(a){case 1:case 2:}", "switch(a){default}", "switch(a){default:}", "switch(a){default:default:}", "switch(a){default:case 1:default:}", "switch(a){case 1:default:case 2:}", "switch(a){a}", "switch(a){a:1}", "switch(a){1}", "switch(a){;}", "switch(a){case 1:;}", "switch(a){case 1:{}}", "switch(a){case 1:{}case 2:{}}", "switch(a){case 1:var b;case 2:var b}", "switch(a){case 1:let b;case 2:let b}", "switch(a){case 1:let b;default:var b}", "switch(a){case 1:let b}let b", "switch(a){case 1:var b}let b", "switch(a){case 1:const b=1;case 2:b=2}", "switch(a){case 1:function b(){}case 2:function b(){}}", "switch(a){case 1:function b(){}case 2:let b}", "switch(a){case 1:function b(){}case 2:var b}", "switch(a){case 1:function b(){}case 2:class b{}}", "switch(a){case 1:class b{}case 2:class b{}}", "switch(a){case 1:class b{}case 2:var b}", "switch(a){case 1:break}", "switch(a){case 1:continue}", "switch(a){case 1:break a}", "x:switch(a){case 1:break x}", "x:switch(a){case 1:continue x}", "switch(a){case 1:return}", "switch(a){case 1:yield}", "switch(a){case 1,2:}", "switch(a){case 1;2:}", "switch(a){case 1::}", "switch(a){case 1 2:}", "switch(a){case(1):}", "switch(a){case 1?2:3:}", "switch(a){case a?b:c:}", "switch(a){case a=>1:}", "switch(a){case ()=>1:}", "switch(a){case(()=>1):}", "switch(a){case x:y:z:}", "switch(a){case x:y:z:1}", "switch(a){case x:y:y:1}", "switch(a){case x:y:{y:1}}", "switch(a){case 1:default:}", "switch(a){default:case 1:}", "switch(a){default;}", "switch(a){default:;default:;}", "switch(a,b){}", "switch(a;b){}", "switch a {}", "switch(a)b", "switch(a);", "switch(a){}x", "switch(a){}{}", "switch(a){}\n{}", "switch(a){case 1:}\nswitch(b){}", "switch(a){case 1:}}", "{switch(a){case 1:}", "switch(a){case 1:{}", "switch(a){case 1:}{",
  "with", "with(", "with()", "with(a", "with(a)", "with(a);", "with(a){}", "with(a)b", "with(a,b)c", "with(a;b)c", "with a b", "with(a)function f(){}", "with(a)class C{}", "with(a)let x", "with(a)let\nx", "with(a)const x=1", "with(a)var x", "with(a)label:1", "with(a)label:function f(){}", "with(a){var x}", "with(a){let x}", "with(a){function f(){}}", "with(a){class C{}}", "with(a)with(b);", "with(a){with(b);}", "function f(){with(a);}", "function f(){'use strict';with(a);}", "'use strict';function f(){with(a);}", "function f(){with(a){'use strict'}}", "with(a){'use strict'}", "with(a)'use strict'", "with(a){var yield}", "with(a){yield}", "with(a){eval=1}", "with(a){delete x}", "with(a){010}", "with(a){arguments}", "with(a){let}", "with(a){let=1}", "with(a){var let}", "with(a)(function(){'use strict';with(b);})", "with(a){(function(){'use strict';with(b);})}", "with(a){class C{m(){with(b);}}}", "class C{m(){with(a);}}", "class C{static{with(a);}}", "class C{x=function(){with(a);}}", "class C{x=()=>{with(a);}}", "(class{m(){with(a);}})", "({m(){with(a);}})", "({m(){'use strict';with(a);}})", "async function f(){with(a);}", "function* g(){with(a);}", "async function* g(){with(a);}", "()=>{with(a);}", "async()=>{with(a);}", "a=>{with(a);}", "(a)=>{with(a);}",
  "debugger", "debugger;", "debugger\n", "debugger x", "debugger 1", "debugger()", "debugger.x", "debugger=1", "debugger:1", "x:debugger", "{debugger}", "if(1)debugger", "if(1)debugger;else debugger", "while(0)debugger", "do debugger;while(0)", "for(;;)debugger", "debugger\ndebugger", "debugger;debugger", "var debugger", "let debugger", "const debugger=1", "function debugger(){}", "class debugger{}", "({debugger})", "({debugger:1})", "({debugger(){}})", "x.debugger", "x?.debugger", "debugger=>1", "(debugger)=>1", "(debugger)", "[debugger]", "typeof debugger", "new debugger", "\\u0064ebugger", "d\\u0065bugger", "debugger\\u0020", "x={debugger}", "x={debugger=1}={}", "x=debugger", "x(debugger)",
  "if", "if(", "if()", "if(a", "if(a)", "if(a);", "if(a){}", "if(a)else", "if(a);else", "if(a);else;", "if(a);else if(b);else;", "if(a);else if", "if(a);else if(", "if(a);else if(b", "if(a);else if(b)", "if(a)a;else", "if(a)a else b", "if(a)a\nelse b", "if(a)a;\nelse b", "if(a){}else", "if(a){}else{", "if(a){}else{}", "if(a){}else{}else{}", "if(a){}else if(b){}else if(c){}else{}", "if(a,b);", "if(a;b);", "if a b", "if(a)b c", "if(a)b;c", "if(a)b\nc", "if(a)\nb", "if\n(a)b", "if(a)function f(){}", "if(a)function f(){}else function g(){}", "if(a)function f(){}else;", "if(a);else function f(){}", "if(a)function* f(){}", "if(a)async function f(){}", "if(a)class C{}", "if(a)let x", "if(a)let\nx", "if(a)let[x]", "if(a)let\n[x]", "if(a)const x=1", "if(a)var x", "if(a)label:1", "if(a)label:function f(){}", "if(a)l:m:function f(){}", "if(a)'use strict'", "if(a)return", "if(a)break", "if(a)continue", "if(a)yield", "if(a)import a from 'b'", "if(a)export var a", "if(a)import('a')", "if(a)import.meta", "if(a)new.target", "if(a)super.x", "if(a)super()", "if(a)await 1", "if(a)for await(x of y);", "if(a)async function f(){}", "if(a)async\nfunction f(){}", "if(a)async\nfunction*f(){}", "if(a)async function*f(){}", "if(a)function f(){}function g(){}", "if(a)function f(){}else if(b)function g(){}", "if(a)function f(){}else if(b);else function g(){}",
]) both(s);

// ---- 13. Comentários, espaço, HTML-like, hashbang, unicode.
for (const s of [
  "/*", "/* ", "/**", "/*/", "/***/", "/* */", "/*\n*/", "/* /* */", "/* */ */", "*/", "/* */*/", "/*/ */", "/**/ /**/", "//", "// ", "//\n", "//\r", "//\u2028", "//\u2029", "// */", "/* // */", "/* \u2028 */", "/*\u2028*/ 1", "1 /*\n*/ 2", "1 /**/ 2", "a/*\n*/++b", "a/**/++b", "a/*\n*/\n++b", "return/*\n*/1", "x=1/*\n*/\n/2/g", "a\n/b/g", "a /*\n*/ /b/g",
  "<!--", "<!-- a", "<!-- a\n", "<!--\n1", "1<!--2", "1 <!-- 2", "1\n<!-- 2", "1;<!-- 2", "1 <!--", "x<!--y", "x < !--y", "x<!-- y\nz", "x = 1 <!-- 2\n3", "<!-- a\n<!-- b", "<!-- a\n--> b", "-->", "--> a", "\n-->", "\n--> a", " -->", " --> a", "/* */ -->", "/* */ --> a", "/*\n*/ -->", "/*\n*/ --> a", "/* */\n-->", "/* */\n--> a", "/**/-->", "/**/ --> a", "1 -->", "1 --> 2", "1\n-->", "1\n--> 2", "1\n--> 2\n3", "1 /* */ -->", "1 /*\n*/ -->", "1 /*\n*/ --> 2", "1/*\n*/-->2", "x-->0", "x --> 0", "x\n--> 0", "x\n -->0", "x/**/-->0", "x/*\n*/-->0", "x/*\n*/ -->0", "x\n/* */-->0", "x\n/* */ -->0", "x\n//\n-->0", "x\n// \n-->0", "x // \n-->0", "// \n-->", "// \n--> a", "// \r--> a", "// \u2028--> a", "// \u2029--> a", "//\n\n-->", "//\n \n-->", "//\n/**/-->", "//\n/**/ -->", "//\n/*\n*/-->", "//\n/*\n*/ -->", "//\n/* */ -->", "//\n1-->", "//\n1 -->", "//\n1\n-->",
  "#!", "#!/", "#!x", "#!\n", "#!x\n1", "#!x\r1", "#!x\u20281", "#!x\u20291", " #!x", "\n#!x", "\t#!x", "//\n#!x", "/**/#!x", "/*\n*/#!x", "1\n#!x", "1;#!x", "1 #!x", "#!x\n#!y", "#!x\n\n#!y", "#!x\n//\n#!y", "#!x\n /**/ #!y", "# !x", "#\n!x", "#!\\u0078", "x\\u0023!x", "\\u0023!x", "#\\u0021x", "#x", "#1", "# ", "#\n", "#!/usr/bin/env x\nreturn 1", "#!/usr/bin/env x\n'use strict';010", "#!/usr/bin/env x\n'use strict';with(a);", "#!/usr/bin/env x\n010", "#!/usr/bin/env x\n<!-- a", "#!/usr/bin/env x\n--> a", "#!/usr/bin/env x\n/*\n*/ --> a", "#!/usr/bin/env x\n/* */ --> a",
  "\u00a0", "\u00a01", "1\u00a0", "1\u00a02", "\u1680", "\u2000", "\u2001", "\u2002", "\u2003", "\u2004", "\u2005", "\u2006", "\u2007", "\u2008", "\u2009", "\u200a", "\u202f", "\u205f", "\u3000", "\ufeff", "\ufeff1", "1\ufeff2", "1\ufeff", "\u180e", "\u180e1", "1\u180e", "1\u180e2", "\u200b", "\u200b1", "1\u200b", "1\u200b2", "\u200c", "\u200d", "1\u200c", "1\u200d", "a\u200c", "a\u200d", "\u200ca", "\u200da", "a\u200cb", "a\u200db", "\u0085", "\u0085 1", "1\u0085", "1\u00852", "\u2028", "\u2029", "\u20281", "\u20291", "1\u2028", "1\u2029", "1\u20282", "1\u20292", "\u2028\u2029", "x\u2028++\u2029y", "x\u2028++y", "x++\u2029y", "return\u20281", "return\u20291", "break\u2028a", "continue\u2029a", "throw\u20281", "throw\u20291", "a:\u2028b", "var\u2028a", "var\u2029a", "a\u2028=1", "a=\u20281", "'a\u2028b'", "'a\u2029b'", "\"a\u2028b\"", "\"a\u2029b\"", "`a\u2028b`", "`a\u2029b`", "/a\u2028b/", "/a\u2029b/", "//a\u2028b", "//a\u2029b", "/*a\u2028b*/", "/*a\u2029b*/", "x=>\u20281", "x\u2028=>1", "async\u2028x=>1", "async x\u2028=>1", "x\u2029=>1", "async\u2029x=>1", "async x\u2029=>1", "async\u2028function f(){}", "async\u2029function f(){}", "(async\u2028function f(){})", "(async\u2029function f(){})", "async\u2028()=>1", "async()\u2028=>1", "async\u2028(x)=>1", "async(x)\u2028=>1", "async\u2028()", "async\u2028(x)", "x\u2028?.y", "x?.\u2028y", "x?\u2028.y", "x?.5:1", "yield\u2028x", "function*g(){yield\u2028x}", "function*g(){yield\u2029x}", "function*g(){yield\u2028*x}", "function*g(){yield\u2029*x}", "function*g(){yield*\u2028x}", "a\u2028?\u2028b\u2028:\u2028c", "a\u2028.\u2028b", "a\u2028(\u2028b\u2028)", "a\u2028[\u2028b\u2028]", "a\u2028`b`", "a`b`\u2028`c`", "if\u2028(a)\u2028b\u2028else\u2028c", "for\u2028(;;)\u2028break", "while\u2028(0)\u2028;", "do\u2028;\u2028while\u2028(0)", "{\u2028}", "[\u2028]", "(\u2028)", "({\u2028})", "function\u2028f\u2028(\u2028)\u2028{\u2028}", "class\u2028C\u2028{\u2028}", "class C{\u2028m\u2028(\u2028)\u2028{\u2028}\u2028}", "class C{static\u2028m(){}}", "class C{get\u2028m(){}}", "class C{set\u2028m(v){}}", "class C{async\u2028m(){}}", "class C{*\u2028m(){}}", "class C{static\u2028*m(){}}", "class C{static\u2028async m(){}}", "class C{static async\u2028m(){}}", "class C{a\u2028=1}", "class C{a\u2028}", "class C{a\u2028b}", "class C{a\u2028*b(){}}", "class C{a\u2028[b]}", "class C{a\u2028(){}}", "class C{get\u2028}", "class C{static\u2028}", "class C{async\u2028}", "class C{set\u2028}", "class C{static\u2028;}", "class C{static\u2028=1}", "class C{static\u2028(){}}", "class C{get\u2028(){}}", "class C{async\u2028(){}}", "class C{set\u2028(){}}",
  "var \u00e9", "var \u00e9\u0301", "var \u0301", "var a\u0301", "var \u00aa", "var \u00ba", "var \u02b0", "var \u2118", "var \u212e", "var \u309b", "var \u309c", "var \u00b7", "var a\u00b7", "var \u0387", "var a\u0387", "var \u1369", "var a\u1369", "var \u19da", "var a\u19da", "var \u2160", "var \u2188", "var \u1885", "var \u1886", "var a\u1885", "var \u3005", "var \u3006", "var \u3007", "var \u3021", "var \u3029", "var \u3038", "var \u303c", "var \u0e01", "var \u0e50", "var a\u0e50", "var \u0660", "var a\u0660", "var \u0966", "var a\u0966", "var \uff10", "var a\uff10", "var \uff21", "var \uff41", "var \ufe33", "var a\ufe33", "var \ufe4d", "var a\ufe4d", "var \u203f", "var a\u203f", "var \u2040", "var a\u2040", "var \u2054", "var a\u2054", "var _", "var $", "var a_", "var a$", "var _a", "var $a", "var \\u005f", "var \\u0024", "var \\u0061", "var \\u{61}", "var \\u{0061}", "var \\u{000061}", "var \\u{110000}", "var \\u{FFFFFFFF}", "var \\u{}", "var \\u{", "var \\u", "var \\u1", "var \\u12", "var \\u123", "var \\u00", "var \\ug", "var \\u{g}", "var a\\u0020", "var a\\u0020b", "var \\u0020", "var \\u0030", "var a\\u0030", "var \\u0041", "var \\u00e9", "var \\u0301", "var a\\u0301", "var \\u200c", "var a\\u200c", "var \\u200d", "var a\\u200d", "var \\u2028", "var \\u2029", "var \\ud800", "var \\udc00", "var \\ud800\\udc00", "var \\u{d800}", "var \\u{dc00}", "var \\u{10000}", "var \\u{1d49c}", "var \\u{1F600}", "var \\u{20bb7}", "var \ud800", "var \udc00", "var \ud800\udc00", "var \ud835\udc9c", "var a\ud835\udc9c", "var \ud83d\ude00", "var a\ud83d\ude00", "var \ud840\udc00", "var \u{20bb7}", "var \u{1d7ce}", "var a\u{1d7ce}", "var \u{e0100}", "var a\u{e0100}", "var \u{e0000}", "var \u{1f600}", "var \u{10ffff}", "var \u{10000}", "var \u{10400}", "var \u{104a0}", "var a\u{104a0}",
  "v\\u0061r a", "\\u0076ar a", "va\\u0072 a", "\\u{76}ar a", "v\\u{61}r a", "var\\u0020a", "var a\\u003d1", "var a\\u003D1", "var a=\\u0031", "\\u0069f(1);", "i\\u0066(1);", "\\u{69}f(1);", "if\\u0020(1);", "typ\\u0065of a", "\\u0074ypeof a", "\\u{74}ypeof a", "n\\u0065w a", "\\u006eew a", "ne\\u0077 a", "d\\u0065lete a.b", "v\\u006fid 0", "t\\u0068is", "n\\u0075ll", "tru\\u0065", "fals\\u0065", "f\\u0075nction f(){}", "cl\\u0061ss C{}", "\\u0063lass C{}", "c\\u006fnst a=1", "l\\u0065t a", "\\u006cet a", "let\\u0020a", "l\\u0065t\na", "l\\u0065t\n[a]=[]", "(l\\u0065t)", "l\\u0065t=1", "(l\\u0065t=1)", "'use strict';l\\u0065t=1", "'use strict';var l\\u0065t", "'use strict';(l\\u0065t)", "'use strict';l\\u0065t", "y\\u0069eld", "'use strict';y\\u0069eld", "'use strict';var y\\u0069eld", "function*g(){y\\u0069eld}", "function*g(){y\\u0069eld 1}", "function*g(){var y\\u0069eld}", "function*g(){(y\\u0069eld)}", "function*g(){y\\u0069eld\n1}", "function*g(){y\\u0069eld*1}", "function*g(){yi\\u0065ld*1}", "function*g(){\\u0079ield*1}", "function*g(){\\u{79}ield*1}", "function*g(){x={y\\u0069eld}}", "function*g(){x={y\\u0069eld:1}}", "function*g(){x.y\\u0069eld}", "function*g(){x={y\\u0069eld(){}}}", "a\\u0077ait", "async function f(){a\\u0077ait 1}", "async function f(){\\u0061wait 1}", "async function f(){\\u{61}wait 1}", "async function f(){var a\\u0077ait}", "async function f(){(a\\u0077ait)}", "async function f(){x={a\\u0077ait}}", "async function f(){x={a\\u0077ait:1}}", "async function f(){x.a\\u0077ait}", "async function f(){a\\u0077ait\n1}", "\\u0061sync function f(){}", "as\\u0079nc function f(){}", "\\u0061sync()=>1", "as\\u0079nc()=>1", "\\u0061sync x=>1", "as\\u0079nc x=>1", "x=\\u0061sync()", "x=as\\u0079nc(1)", "x=\\u0061sync", "x=as\\u0079nc", "(\\u0061sync)", "\\u0061sync\nfunction f(){}", "x={\\u0061sync m(){}}", "x={as\\u0079nc m(){}}", "x={\\u0061sync:1}", "x={\\u0061sync}", "x={\\u0061sync(){}}", "class C{\\u0061sync m(){}}", "class C{as\\u0079nc m(){}}", "class C{\\u0061sync(){}}", "class C{\\u0061sync=1}", "class C{static \\u0061sync m(){}}", "class C{static as\\u0079nc m(){}}", "class C{\\u0067et m(){}}", "class C{g\\u0065t m(){}}", "class C{\\u0073et m(){v}}", "class C{s\\u0065t m(v){}}", "class C{\\u0073tatic m(){}}", "class C{st\\u0061tic m(){}}", "class C{static\\u0020m(){}}", "x={\\u0067et m(){}}", "x={g\\u0065t m(){}}", "x={\\u0073et m(v){}}", "x={s\\u0065t m(v){}}", "x={\\u0067et:1}", "x={\\u0067et}", "x={\\u0067et(){}}", "for(x \\u006ff y);", "for(x o\\u0066 y);", "for(x \\u{6f}f y);", "for(x \\u0069n y);", "for(x i\\u006e y);", "for(x \\u{69}n y);", "x \\u0069n y", "x i\\u006e y", "x \\u0069nstanceof y", "x instanceo\\u0066 y", "x\\u0020in y", "for(\\u0061sync of y);", "for(as\\u0079nc of y);", "for(\\u0061sync x of y);", "for(async\\u0020x of y);", "for(\\u006cet x of y);", "for(l\\u0065t x of y);", "for(l\\u0065t of y);", "for(\\u006cet of y);", "for(l\\u0065t in y);", "for(l\\u0065t.x of y);", "for(l\\u0065t[x] of y);", "for(l\\u0065t\n[x] of y);", "for(\\u0061wait x of y);", "async function f(){for \\u0061wait(x of y);}", "async function f(){for aw\\u0061it(x of y);}", "async function f(){for await(x \\u006ff y);}", "async function f(){for await(x o\\u0066 y);}", "if(1);\\u0065lse;", "if(1);el\\u0073e;", "do;\\u0077hile(0)", "do;wh\\u0069le(0)", "try{}\\u0063atch(e){}", "try{}c\\u0061tch(e){}", "try{}\\u0066inally{}", "try{}f\\u0069nally{}", "switch(1){\\u0063ase 1:}", "switch(1){c\\u0061se 1:}", "switch(1){\\u0064efault:}", "switch(1){d\\u0065fault:}", "\\u0077hile(0);", "wh\\u0069le(0);", "\\u0066or(;;);", "f\\u006fr(;;);", "\\u0072eturn", "re\\u0074urn", "function f(){\\u0072eturn}", "function f(){re\\u0074urn}", "function f(){return\\u0020}", "\\u0062reak", "br\\u0065ak", "\\u0063ontinue", "co\\u006ftinue", "\\u0074hrow 1", "thr\\u006fw 1", "\\u0073witch(1){}", "sw\\u0069tch(1){}", "\\u0077ith(a);", "wi\\u0074h(a);", "\\u0064o;while(0)", "d\\u006f;while(0)", "\\u0069mport a from 'b'", "im\\u0070ort a from 'b'", "\\u0065xport var a", "ex\\u0070ort var a", "\\u0065xtends", "class C \\u0065xtends D{}", "class C ext\\u0065nds D{}", "class C extends D{\\u0073tatic m(){}}", "\\u0073uper.x", "su\\u0070er.x", "class C extends D{constructor(){\\u0073uper()}}", "class C extends D{constructor(){su\\u0070er()}}", "class C extends D{m(){\\u0073uper.x}}", "class C extends D{m(){su\\u0070er.x}}", "\\u006eew.target", "n\\u0065w.target", "function f(){\\u006eew.target}", "function f(){n\\u0065w.target}", "function f(){new.t\\u0061rget}", "function f(){new.\\u0074arget}", "function f(){new.\\u{74}arget}", "function f(){new.target\\u0020}", "function f(){new\\u0020.target}", "import.m\\u0065ta", "import.\\u006deta", "\\u0069mport.meta", "im\\u0070ort.meta", "import('a')\\u0020", "\\u0069mport('a')", "im\\u0070ort('a')", "\\u0065num", "en\\u0075m", "var \\u0065num", "var en\\u0075m", "\\u0069mplements", "'use strict';\\u0069mplements", "'use strict';impl\\u0065ments", "'use strict';var \\u0069mplements", "'use strict';var impl\\u0065ments", "var \\u0069mplements", "var impl\\u0065ments", "'use strict';\\u0069nterface", "'use strict';int\\u0065rface", "'use strict';var \\u0070ackage", "'use strict';var pack\\u0061ge", "'use strict';var \\u0070rivate", "'use strict';var priv\\u0061te", "'use strict';var \\u0070rotected", "'use strict';var prot\\u0065cted", "'use strict';var \\u0070ublic", "'use strict';var pub\\u006cic", "'use strict';var \\u0073tatic", "'use strict';var st\\u0061tic", "'use strict';var \\u006cet", "'use strict';var l\\u0065t", "'use strict';var \\u0079ield", "'use strict';var yi\\u0065ld", "'use strict';var \\u0065val", "'use strict';var ev\\u0061l", "'use strict';var \\u0061rguments", "'use strict';var arg\\u0075ments", "'use strict';\\u0065val=1", "'use strict';ev\\u0061l=1", "'use strict';\\u0061rguments=1", "'use strict';arg\\u0075ments=1", "'use strict';\\u0065val++", "'use strict';ev\\u0061l++", "'use strict';function \\u0065val(){}", "'use strict';function ev\\u0061l(){}", "'use strict';(\\u0065val)=>1", "'use strict';(ev\\u0061l)=>1", "'use strict';\\u0065val=>1", "'use strict';ev\\u0061l=>1", "'use strict';try{}catch(\\u0065val){}", "'use strict';try{}catch(ev\\u0061l){}", "'use strict';[\\u0065val]=[]", "'use strict';[ev\\u0061l]=[]", "'use strict';({\\u0065val}={})", "'use strict';({ev\\u0061l}={})", "'use strict';({a:\\u0065val}={})", "'use strict';({a:ev\\u0061l}={})", "'use strict';for(\\u0065val of []);", "'use strict';for(ev\\u0061l of []);", "'use strict';for(\\u0065val in {});", "'use strict';for(ev\\u0061l in {});",
]) both(s);

// ---- 14. Grade de operadores e tokens inesperados.
const toks = [
  "+", "-", "*", "/", "%", "**", "<<", ">>", ">>>", "<", ">", "<=", ">=", "==", "!=", "===", "!==", "&", "|", "^", "&&", "||", "??", "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=", "??=", "=>", "?", ":", "?.", ".", "..", "...", ",", ";", "(", ")", "[", "]", "{", "}", "!", "~", "++", "--", "typeof", "void", "delete", "new", "in", "instanceof", "of", "as", "async", "await", "yield", "let", "static", "get", "set", "if", "else", "for", "while", "do", "var", "const", "class", "function", "return", "break", "continue", "throw", "try", "catch", "finally", "switch", "case", "default", "with", "debugger", "import", "export", "extends", "super", "this", "null", "true", "false", "enum", "implements", "interface", "package", "private", "protected", "public", "eval", "arguments", "#a", "@", "\\", "`", "'", "\"", "#", "\u00a7", "\u2603", "\u0000", "\u0001", "\u001f", "\u007f", "\u0080",
];
for (const t of toks) {
  both(t);
  both(`a ${t}`);
  both(`${t} a`);
  both(`a ${t} b`);
  both(`(${t})`);
  both(`[${t}]`);
  both(`{${t}}`);
  both(`({${t}})`);
  both(`({a:${t}})`);
  both(`f(${t})`);
  both(`a ? ${t} : b`);
  both(`a ? b : ${t}`);
  both(`var ${t}`);
  both(`var a = ${t}`);
  both(`${t};`);
  both(`1 ${t}`);
  both(`${t} 1`);
  both(`1 ${t} 1`);
  both(`x = ${t}`);
  both(`x = y ${t}`);
  both(`x = y ${t} z`);
  both(`() => ${t}`);
  both(`(a) => { ${t} }`);
  both(`class C { ${t} }`);
  both(`class C { m() { ${t} } }`);
  both(`function f() { ${t} }`);
  both(`for (${t};;);`);
  both(`for (;${t};);`);
  both(`for (;;${t});`);
  both(`switch (a) { case ${t}: }`);
  both(`\`\${${t}}\``);
}

// ---- 15. Parâmetros e corpo de funções, setters e getters, arrow, async arrow, trailing comma.
const plist = [
  "", ",", "a,", "a,,", ",a", "a,b", "a,b,", "a,,b", "...a", "...a,", "...a,b", "...a=1", "...", "...,", "a...", "a=", "a=1,", "a=1,b", "a=1,b=2", "a=1,...b", "a,...b=1", "a,...b,", "...a,...b", "[a]", "[a],", "[a]=[]", "[a]=1", "[...a]", "[...a,]", "[...a,b]", "{a}", "{a},", "{a}={}", "{a}=1", "{...a}", "{...a,}", "{...a,b}", "{a:b}", "{a:b=1}", "{a=1}", "{a:1}", "{a:b.c}", "{[a]:b}", "{[a]}", "(a)", "(a),b", "((a))", "a.b", "a[b]", "a()", "1", "'a'", "this", "null", "true", "new.target", "super.x", "a+b", "a=b=c", "a=(b)", "a=(b=1)", "a,(b)", "(a,b)", "(a=1)", "a?b:c", "a=>1", "a=>b=>1", "async a=>1", "async()=>1", "function(){}", "class{}", "`a`", "a`b`", "typeof a", "!a", "-a", "++a", "a++", "await a", "yield a", "yield", "await", "let", "static", "async", "eval", "arguments", "a,eval", "eval,a", "a,arguments", "arguments,a", "eval=1", "arguments=1", "a=eval", "a=arguments", "[eval]", "{eval}", "{a:eval}", "[arguments]", "{arguments}", "...eval", "...arguments", "a,a", "a,a,a", "a,b,a", "a,[a]", "[a],a", "a,{a}", "{a},a", "a,...a", "...a,a", "[a,a]", "{a,a}", "{a:b,c:b}", "[a],[a]", "{a},{a}", "a=1,a", "a,a=1", "a=a", "a=b,b", "a,b=a", "[a=b],b", "{a=b},b", "a=eval", "a=()=>a", "a=function a(){}", "a=class a{}", "a=a=>a", "a=(a)=>a", "(a)=>a", "a=>a", "a=1)=>1", "a,b)=>1",
];
for (const p of plist) {
  both(`function f(${p}){}`);
  both(`(function(${p}){})`);
  both(`(${p})=>1`);
  both(`async(${p})=>1`);
  both(`async function f(${p}){}`);
  both(`function* g(${p}){}`);
  both(`async function* g(${p}){}`);
  both(`({m(${p}){}})`);
  both(`({set m(${p}){}})`);
  both(`({get m(${p}){}})`);
  both(`class C{m(${p}){}}`);
  both(`class C{constructor(${p}){}}`);
  both(`class C{static m(${p}){}}`);
  both(`class C{set m(${p}){}}`);
  both(`class C{get m(${p}){}}`);
  both(`f(${p})`);
  both(`new f(${p})`);
  both(`f?.(${p})`);
  both(`super(${p})`);
  both(`(${p})`);
  both(`[${p}]`);
  both(`new Function(${JSON.stringify(p)},"")`.replace(/^new Function/, "new (function(){}).constructor"));
}

// ---- 16. Expressões incompletas e operadores combinados (grade pequena).
const lhs = ["a", "1", "'a'", "this", "(a)", "a.b", "a()", "[a]", "{}", "({})", "function(){}", "()=>1", "a=>1", "class{}", "`a`", "typeof a", "!a", "a++", "++a", "new a", "new a()", "async()=>1", "async function(){}", "a?.b", "/a/", "null", "true", "yield", "await", "let", "async", "of", "get", "static"];
const ops = ["+", "-", "*", "/", "%", "**", "<<", ">>", ">>>", "<", ">", "<=", ">=", "==", "!=", "===", "!==", "&", "|", "^", "&&", "||", "??", "in", "instanceof", ",", "?:", "=", "=>", ".", "?.", "(", "["];
for (const l of lhs) for (const o of ops) {
  both(`${l} ${o}`);
  both(`${l} ${o} ${l}`);
  both(`(${l} ${o})`);
  both(`${o} ${l}`);
}

// ---- 17. Fechando: parênteses, arrow e async arrow.
for (const s of [
  "()", "()=>", "()=>{", "()=>{}", "()=>{}()", "()=>{}.x", "()=>{}[0]", "()=>{}`x`", "()=>{}+1", "()=>{}\n+1", "()=>{}\n(1)", "()=>{}\n[1]", "()=>{}\n`x`", "()=>{},1", "()=>{}?1:2", "()=>{}||1", "()=>{}??1", "()=>{}in x", "()=>{}instanceof x", "()=>({})", "()=>({}).x", "()=>({})()", "()=>1()", "()=>1+1", "()=>1,2", "()=>1?2:3", "()=>1=2", "()=>a=2", "()=>a+=2", "()=>a++", "()=>++a", "()=>!a", "()=>-a", "()=>a=>1", "()=>()=>1", "()=>async()=>1", "()=>async a=>1", "()=>function(){}", "()=>class{}", "()=>`a`", "()=>this", "()=>null", "()=>yield", "()=>await", "()=>let", "()=>arguments", "()=>eval", "()=>new.target", "()=>super.x", "()=>super()", "()=>import.meta", "()=>import('a')", "()=>a?.b", "()=>a?.[0]", "()=>a?.()", "()=>typeof a", "()=>void 0", "()=>delete a.b", "()=>delete a", "()=>new a", "()=>new a()", "()=>[]", "()=>[...a]", "()=>({...a})", "()=>({a})", "()=>({a:1})", "()=>({a(){}})", "()=>({get a(){}})", "()=>({[a]:1})", "()=>{a:1}", "()=>{a:b:1}", "()=>{a}", "()=>{a,b}", "()=>{a;b}", "()=>{var a;var a}", "()=>{let a;let a}", "()=>{let a;var a}", "()=>{return}", "()=>{return 1}", "()=>{yield}", "()=>{await}", "()=>{'use strict';010}", "()=>{010;'use strict'}", "()=>{'use strict';with(a);}", "()=>{with(a);}", "()=>{break}", "()=>{continue}", "()=>{new.target}", "()=>{super.x}", "()=>{super()}", "()=>{arguments}", "()=>{eval}", "()=>{this}", "()=>{debugger}", "()=>{import.meta}", "()=>{import('a')}", "()=>{export var a}", "()=>{import a from 'b'}", "(a)=>", "(a)=>{", "(a)=>{}", "(a,)=>1", "(a,,)=>1", "(,)=>1", "(,a)=>1", "(a,b,)=>1", "(...a,)=>1", "(...a)=>1", "(...a,b)=>1", "(a,...b)=>1", "(a,...b,c)=>1", "(...)=>1", "(a=1)=>1", "(a=1,b)=>1", "(a=1,...b)=>1", "([a])=>1", "({a})=>1", "([a]=[])=>1", "({a}={})=>1", "(a.b)=>1", "(a[b])=>1", "(a())=>1", "(1)=>1", "('a')=>1", "(this)=>1", "(null)=>1", "(true)=>1", "(a+b)=>1", "(a,b+c)=>1", "(a=>1)=>1", "((a))=>1", "((a),b)=>1", "(a,(b))=>1", "(a,b)\n=>1", "(a,b)=>\n1", "(a,b)=\n>1", "(a,b)= >1", "(a,b)=> 1", "(a,b) => 1", "(a,b)=>1;", "(a,b)=>1\n;", "a=>", "a=>{", "a=>{}", "a\n=>1", "a=>\n1", "a =>1", "a=> 1", "a,b=>1", "a,b=>{}", "(a),b=>1", "a=>1,b=>2", "a=>b=>1", "a=>b=>c=>1", "a=>(b=>1)", "(a=>b)=>1", "a=>{}=>1", "a=>{}()", "a=>{}\n()", "a=>1()", "a=>({})", "a=>{}.x", "a=>{}`x`", "a=>{} `x`", "a=>{}\n`x`", "a=>a`x`", "async a=>", "async a=>{}", "async a\n=>1", "async\na=>1", "async a=>\n1", "async (a)=>1", "async (a)\n=>1", "async\n(a)=>1", "async (a,)=>1", "async (a,,)=>1", "async (,)=>1", "async (...a)=>1", "async (...a,)=>1", "async (a,...b)=>1", "async (a=1)=>1", "async ([a])=>1", "async ({a})=>1", "async (a.b)=>1", "async (1)=>1", "async (a,a)=>1", "async (a,[a])=>1", "async (await)=>1", "async (a=await 1)=>1", "async (a=await)=>1", "async await=>1", "async (yield)=>1", "async (a=yield)=>1", "async yield=>1", "async (let)=>1", "async let=>1", "async (static)=>1", "async static=>1", "async (eval)=>1", "async eval=>1", "async (arguments)=>1", "async arguments=>1", "async (async)=>1", "async async=>1", "async (of)=>1", "async of=>1", "async (a)=>{'use strict'}", "async (a=1)=>{'use strict'}", "async (a)=>{var a}", "async (a)=>{let a}", "async (a)=>{function a(){}}", "async (a,a)=>{}", "async (a)\n=>{}", "async()\n=>{}", "async\n()=>{}", "async ()=>await 1", "async ()=>{await 1}", "async ()=>{await}", "async ()=>await", "async function(){}", "async function f(){}", "async function*f(){}", "async *f(){}", "async function", "async function f", "async function f(", "async function f()", "async function f(){", "async function f(){}", "async\nfunction f(){}", "async\nfunction*f(){}", "async function\nf(){}", "async function*\nf(){}", "async function f\n(){}", "async function f()\n{}", "async function\n*f(){}", "async\n function f(){}", "async f(){}", "async(){}", "async()", "async(a)", "async(a,)", "async(a,,)", "async(,)", "async(...a)", "async(...a,)", "async(a=1)", "async(a,b)", "async(a)(b)", "async(a)\n(b)", "async\n(a)", "async\n(a)\n(b)", "async\n(a)=>1", "async(a)=>1", "async(a)\n=>1", "async a", "async a b", "async a=>1 b", "async a=>1\nb", "async a=>1;b", "async a\n", "async a;", "async 1", "async 'a'", "async this", "async null", "async true", "async new", "async new a", "async typeof a", "async void a", "async delete a", "async !a", "async -a", "async ++a", "async a++", "async yield", "async await", "async let", "async static", "async of", "async in", "async in x", "async instanceof x", "async instanceof", "async ? 1 : 2", "async ?? 1", "async || 1", "async && 1", "async , 1", "async = 1", "async += 1", "async++", "++async", "async.x", "async?.x", "async[0]", "async`x`", "async\n`x`", "async()`x`", "async()\n`x`", "async/1/g", "async /1/g", "async\n/1/g", "async/ 1/g", "async/=1", "async /= 1", "async\n/=1",
]) both(s);

fs.writeFileSync("/dev/stderr", `programas: ${progs.length}\n`);

// ---- Descarta o que o golden syntax-errors.tsv já cobre (mesma fonte, qualquer variante de wrapper não conta).
const existing = new Set();
try {
  const ex = fs.readFileSync(path.join(__dirname, "../tests/golden/syntax-errors.tsv"), "utf8");
  for (const line of ex.split("\n")) {
    const tab = line.indexOf("\t");
    if (tab > 0) {
      try { existing.add(JSON.parse(line.slice(0, tab))); } catch {}
    }
  }
} catch {}
const fresh = progs.filter((p) => !(p.kind === "F" && existing.has(p.src))).map((p) => p.prog);
// A grade completa passa de cem mil programas; fica uma amostra determinística (hash FNV da fonte) de cerca de 1 em 20.
const SAMPLE_MOD = 20;
const dedup = fresh.filter((p) => {
  let h = 2166136261;
  for (let i = 0; i < p.length; i++) h = Math.imul(h ^ p.charCodeAt(i), 16777619) >>> 0;
  return h % SAMPLE_MOD === 0;
});
fs.writeFileSync("/dev/stderr", `após dedup contra syntax-errors: ${dedup.length}\n`);

function runOne(prog) {
  return new Promise((resolve) => {
    const c = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    c.stdout.on("data", (d) => (out += d));
    c.on("close", () => resolve(decodeResult(out) ?? ""));
    c.stdin.end(prog);
  });
}
(async () => {
  const results = new Array(dedup.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (next < dedup.length) {
      const i = next++;
      results[i] = await runOne(dedup[i]);
    }
  });
  await Promise.all(workers);
  const lines = [];
  for (let i = 0; i < dedup.length; i++) lines.push(`${JSON.stringify(dedup[i])}\t${JSON.stringify(results[i])}`);
  process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
})();
