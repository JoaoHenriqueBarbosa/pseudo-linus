// Gera tests/golden/completion_value_bun.tsv: valor de completude (completion value) de statements, medido no bun
// 1.4.2 com `eval(fonte)` dentro de um programa que grava o texto em `globalThis.R`. Cada programa roda num bun filho
// novo (sem vazamento de `var` entre linhas), com timeout de 5 s e no máximo 8 filhos ao mesmo tempo.
// Cobre if/else, laços com break/continue (UpdateEmpty), switch, try/catch/finally, blocos rotulados, with, `var`,
// declarações de função e de classe (vazias), blocos vazios e combinações, em três modos de eval: direto sloppy (0),
// indireto (1) e direto com a diretiva "use strict" na fonte do eval (2).
// Colunas: o sufixo do programa (JSON) e o texto do valor (JSON), no formato de `S` abaixo; erro vira `throw Nome`.
// O prelúdio comum sai em tests/golden/completion_value.preludes.json (scripts/golden-prelude.js).
// Programas cujo texto já aparece em outro golden são descartados.
// Uso: bun scripts/gen-completion-value-golden.js > tests/golden/completion_value_bun.tsv
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

// `S` formata o valor; `C(fonte, modo)` avalia a fonte por eval e devolve o texto (erro vira `throw Nome`).
const PRELUDE =
  'function S(v){if(v===undefined)return "undefined";if(v===null)return "null";if(Object.is(v,-0))return "-0";var t=typeof v;' +
  'if(t==="string")return "string:"+v;if(t==="number"||t==="boolean")return t+":"+String(v);return t}\n' +
  'function C(s,m){try{if(m===1)return S((0,eval)(s));if(m===2)s="\\"use strict\\";"+s;return S((function(){return eval(s)})())}' +
  'catch(e){return "throw "+(e&&e.name)}}\n';

const progs = new Set();
const add = (...list) => list.forEach(p => progs.add(p));

// 1. Base: statements isolados.
add("", "1", "1;", ";", "{}", "{;}", "1;;", "1; {}", "{} 1", "1; var x = 2", "1; var x", "1; let y = 2", "1; const z = 3", "var x = 1", "let y = 1",
  "1; function f() {}", "function f() {} 1", "1; class C {}", "1; (class C {})", "1; (function () {})", "1; debugger", "1; ;", "'a'", "1; 'a'; 2",
  "1; { 2 }", "1; { 2; }", "1; { 2; {} }", "1; { {} }", "1; {;}", "1; { var q = 1 }", "1; { function g() {} }", "1; { let a = 1 }",
  "1; { class D {} }", "1; var a = 1, b = 2", "1; (0, 1)", "1, 2", "null", "undefined", "1; undefined", "1; void 0", "x = 5", "var x; x = 5", "typeof 1",
  "1; throw 2", "`t`", "[1]", "({})", "1n", "-0", "+0");

// 2. if / else.
const conds = ["1", "0", "true", "false", "null"];
const bodies = ["2", "{}", "{ 2 }", "{ 2; 3 }", ";", "var v = 4", "{ var v = 4 }", "{ 5; var v }", "function f() {}", "{ }"];
for (const c of conds) {
  for (const b of bodies.slice(0, 7)) {
    add(`if (${c}) ${b}`, `1; if (${c}) ${b}`, `1; if (${c}) ${b} else 9`, `1; if (${c}) 8; else ${b}`);
  }
}
add("1; if (0) 2; else {}", "1; if (1) {} else 2", "if (1) if (0) 2; else 3", "1; if (1) if (0) 2", "1; if (0) if (1) 2; else 3",
  "1; if (1) { if (0) 2 }", "1; if (1) { 3; if (0) 2 }", "1; if (0) 2; 3", "if (1) 2; 3; if (0) 4", "1; if (1) ; else 2", "1; if (0) ; else ;");

// 3. Laços com break/continue (UpdateEmpty).
const loops = [
  b => `do { ${b} } while (0)`,
  b => `while (true) { ${b}; break }`,
  b => `for (var i = 0; i < 1; i++) { ${b} }`,
  b => `for (let i = 0; i < 1; i++) { ${b} }`,
  b => `for (var k in {a: 1}) { ${b} }`,
  b => `for (var o of [1]) { ${b} }`,
];
const loopBodies = ["", "1", "1; break", "1; continue", "break", "continue", "1; if (0) 2", "1; if (1) break", "2; if (1) continue", "var z = 1",
  "if (1) 3", "if (1) break; else 4", "if (0) 3; else continue", "{ 5; break }", "{ 5; continue }", "1; {}", "7; { break }", "7; { continue }"];
for (const l of loops) for (const b of loopBodies) add(l(b), `0; ${l(b)}`);
add("do { 1; break; } while(0)", "1; do { 2; break; } while(0)", "do { 1; continue; } while(0)", "1; do { continue; } while(0)",
  "for (var i=0;i<3;i++) { if (i==1) continue; i }", "for (var i=0;i<3;i++) { i; if (i==1) break; }", "for (var i=0;i<3;i++) { if (i==1) break; i }",
  "for (var i=0;i<3;i++) { if (i==2) continue; i }", "1; for (var i=0;i<3;i++) { if (i==0) continue; i }", "for (var i=0;i<3;i++) { if (i==0) continue; i; if (i==2) break }",
  "for (;false;) 1", "1; for (;false;) 2", "1; while (false) 2", "1; do ; while (false)", "do 5; while (false)", "var n = 0; while (n < 3) n++",
  "var n = 0; do n++; while (n < 3)", "var n = 0; while (n < 3) { n++; }", "var n = 0; while (n < 3) { n++; if (n == 2) break; }",
  "var n = 0; while (n < 3) { n++; if (n == 2) continue; n * 10 }", "for (var a = 1; a < 3; a++);", "for (var a = 1; a < 3; a++) ;", "1; for (var a = 1; a < 3; a++);",
  "for (var a of []) 1", "1; for (var a of []) 2", "1; for (var a in {}) 2", "for (var a of [1, 2, 3]) a", "for (var a of [1, 2, 3]) { a; if (a == 2) continue }",
  "for (var a of [1, 2, 3]) { if (a == 2) break; a }", "for (var a in {x: 1, y: 2}) a", "for (let [p, q] of [[1, 2]]) p + q", "for (const e of 'ab') e",
  "do { 1; if (1) break; 2 } while (0)", "do { 1; { break } } while (0)", "1; do { 2; try { break } finally { 3 } } while (0)",
  "1; do { 2; try { 4 } finally { break } } while (0)", "do { 1; try { break } finally { } } while (0)", "do { try { 1; break } finally { 2 } } while (0)");

// 4. switch.
add("1; switch(0){}", "switch(0){}", "switch(1){case 1:}", "1; switch(1){case 1:}", "switch(1){case 1: 2}", "switch(1){case 1: 2; break}", "switch(1){case 1: break}",
  "1; switch(1){case 1: break}", "switch(1){case 1: 2; break; case 2: 3}", "switch(2){case 1: 2; break; case 2: 3}", "switch(3){case 1: 2; case 2: 3}",
  "switch(1){case 1: 2; case 2: 3}", "switch(1){case 1: 2; case 2: }", "switch(1){case 1: 2; case 2: var s}", "switch(1){case 1: 2; default: 3}",
  "switch(9){case 1: 2; default: 3}", "1; switch(9){case 1: 2}", "switch(9){default:}", "1; switch(9){default:}", "switch(9){default: 4}", "switch(1){default: 4; case 1: 5}",
  "switch(2){default: 4; case 1: 5}", "switch(1){case 1: { 6 }}", "switch(1){case 1: 6; {}}", "switch(1){case 1: if (0) 7}", "1; switch(1){case 1: if (0) 7}",
  "switch(1){case 1: 2; if (1) break; 3}", "switch(1){case 1: 2; { break } }", "switch(1){case 1: 2; case 2: 3; break; default: 4}", "switch(0){case 0: for (;;) { 3; break }}",
  "1; switch(1){case 1: for (;;) { break }}", "switch(1){case 1: try { 2 } finally { 3 }}", "switch(1){case 1: 2; try { break } finally { 3 }}",
  "switch(1){case 1: 2; try { 4 } finally { break }}", "switch(1){case 1: var q = 1}", "switch(1){case 1: function f() {}}", "switch(1){case 1: 2; function f() {}}",
  "switch(1){case 1: let r = 1}", "switch(1){case 1: 2; let r = 1}", "L: switch(1){case 1: 2; break L}", "L: switch(1){case 1: break L}", "1; L: switch(1){case 1: break L}");
for (const d of [0, 1, 2]) for (const b of ["1", "1; break", "break", "1; if (0) 2", "if (1) 3", "{}", "var u = 1"]) add(`switch(${d}){case 0: ${b}; case 1: ${b}; default: ${b}}`);

// 5. try / catch / finally.
const tb = ["1", "", "1; {}", "if (0) 1", "var t = 1", "1; var t", "{}"];
for (const t of tb) {
  add(`try { ${t} } catch (e) { 9 }`, `1; try { ${t} } finally { 3 }`, `try { ${t} } finally { 3 }`, `try { ${t} } finally { }`, `try { ${t} } catch (e) { 9 } finally { 3 }`,
    `0; try { ${t} } finally { }`, `try { ${t} } finally { 4; }`, `try { ${t} } finally { var w = 3 }`, `try { ${t} } finally { if (0) 3 }`);
  add(`try { throw 1; ${t} } catch (e) { 9 }`, `1; try { throw 1 } catch (e) { ${t} }`, `try { throw 1 } catch (e) { ${t} } finally { 3 }`,
    `0; try { throw 1 } catch (e) { ${t} }`, `0; try { throw 1 } catch { ${t} } finally { }`, `try { 2; throw 1 } catch (e) { }`, `try { 2; throw 1 } catch (e) { } finally { 3 }`);
}
add("1; try { 2 } finally { 3 }", "try { 2 } catch (e) { 4 }", "try { throw 2 } catch (e) { e }", "try { throw 2 } catch (e) { e } finally { 5 }", "try { throw 2 } catch (e) { }",
  "try { throw 2 } catch (e) { } finally { 5 }", "1; try { throw 2 } catch (e) { }", "try { 2; throw 1 } catch (e) { }", "try { throw 1 } finally { 2 }",
  "try { try { throw 1 } finally { 2 } } catch (e) { 3 }", "try { try { throw 1 } finally { 2 } } catch (e) { }", "try { throw 1 } catch (e) { throw 2 } finally { 3 }",
  "try { 1 } catch (e) { } finally { throw 3 }", "do { try { 1; break } finally { 2 } } while (0)", "do { 0; try { 1; break } finally { 2; } } while (0)",
  "do { try { 1 } finally { 2; break } } while (0)", "do { 0; try { 1 } finally { 2; break } } while (0)", "do { try { 1 } finally { break } } while (0)",
  "do { 0; try { 1 } finally { break } } while (0)", "do { try { 1; continue } finally { 2 } } while (0)", "do { 0; try { 1 } finally { continue } } while (0)",
  "do { try { 1 } finally { continue } } while (0)", "L: try { 1; break L } finally { 2 }", "L: try { 1 } finally { 2; break L }", "L: try { 1 } finally { break L }",
  "0; L: try { 1 } finally { break L }", "L: try { throw 1 } catch (e) { 2; break L } finally { 3 }", "L: try { throw 1 } catch (e) { 2 } finally { 3; break L }",
  "for (var i = 0; i < 2; i++) { try { i } finally { 9 } }", "for (var i = 0; i < 2; i++) { try { i; continue } finally { 9 } }", "for (var i = 0; i < 2; i++) { try { i } finally { continue } }",
  "for (var i = 0; i < 2; i++) { 5; try { i } finally { continue } }", "for (var i = 0; i < 2; i++) { try { i } finally { 9; continue } }", "for (var i = 0; i < 2; i++) { try { i; break } finally { 9 } }",
  "for (var i = 0; i < 2; i++) { 7; try { break } finally { 9 } }", "for (var i = 0; i < 2; i++) { try { throw i } catch (e) { e } }", "for (var i = 0; i < 2; i++) { try { throw i } catch (e) { continue } }",
  "for (var i = 0; i < 2; i++) { try { throw i } catch (e) { e; continue } }", "for (var i = 0; i < 2; i++) { try { throw i } catch (e) { e; break } finally { 8 } }",
  "1; try { 2 } catch ({ message }) { 3 }", "try { null.x } catch ({ name }) { name }", "try { null.x } catch (e) { e instanceof TypeError }", "try { undefinedVar } catch (e) { e.name }",
  "try { 1 } finally { try { 2 } finally { 3 } }", "try { 1 } finally { try { throw 2 } catch (e) { 4 } }", "try { 1 } catch (e) { 2 } finally { 3 } 4", "try { throw 1 } catch (e) { 2 } 3");

// 6. Blocos rotulados.
add("L: { 1; break L; }", "L: { 1; break L; 2 }", "1; L: { break L }", "1; L: { 2; break L }", "L: { 1; { break L } }", "L: { break L }", "L: 5", "L: ;", "L: {}", "1; L: {}",
  "1; L: ;", "L: M: 6", "L: { M: { 1; break L } 2 }", "L: { M: { 1; break M } 2 }", "L: { M: { break M } 2 }", "L: { 3; M: { break M } }", "L: { 3; M: { 4; break M } }",
  "L: if (1) { 2; break L }", "L: if (1) { break L }", "1; L: if (1) { break L }", "L: for (;;) { 2; break L }", "L: for (var i = 0; i < 3; i++) { for (;;) { i; continue L } }",
  "L: for (var i = 0; i < 3; i++) { for (;;) { i; break L } }", "L: for (var i = 0; i < 3; i++) { 0; for (;;) { break L } }", "L: do { 1; continue L } while (0)",
  "L: do { 1; break L } while (0)", "L: while (1) { 2; break L }", "L: { 1 }", "L: { 1; 2 }", "L: { var lv = 1 }", "L: function f() {}", "1; L: function f() {}",
  "L: { 1; try { break L } finally { 2 } }", "L: { try { 1 } finally { 2 } }", "L: { 1; switch(1){case 1: break L} }", "L: { 1; switch(1){case 1: 2; break} }",
  "A: B: C: { 1; break B }", "A: { B: { 1; break A } 2 }");

// 7. with.
add("with ({}) 1", "with ({}) {}", "with ({}) ;", "1; with ({}) {}", "1; with ({}) ;", "with ({a: 1}) a", "with ({a: 1}) { a }", "with ({a: 1}) { 2; a }", "with ({a: 1}) { var wv = a }",
  "1; with ({a: 1}) { var wv = a }", "with ({}) if (0) 1", "1; with ({}) if (0) 2", "with ({}) { 1; if (0) 2 }", "with ({}) { 1; break_ = 2 }", "L: with ({}) { 1; break L }",
  "1; L: with ({}) { break L }", "do { with ({}) { 1; break } } while (0)", "do { 0; with ({}) { break } } while (0)", "do { with ({}) { 1; continue } } while (0)",
  "do { 0; with ({}) { continue } } while (0)", "with ({}) with ({}) 3", "with ({}) { try { 1 } finally { 2 } }", "with (null) 1", "with ({a: 1}) { function f() {} }",
  "1; with ({a: 1}) { function f() {} }", "with ({a: 1}) { let l = 1 }", "with ({a: 1}) { 4; let l = 1 }", "with ({a: 1}) for (var wi = 0; wi < 2; wi++) a + wi");

// 8. Declarações e expressões especiais.
add("function f() {}", "1; function f() {}", "function f() {} 1; function g() {}", "class C {}", "1; class C {}", "class C { static x = 5 }", "1; class C { static x = 5 }",
  "class C { static { 1 } }", "1; class C { static { 2 } }", "let a = 1; a", "let a = 1; a; let b = 2", "const c = 1; c; const d = 2", "var v1 = 1; v1; var v2 = 2", "1; var [p] = [2]",
  "1; var {p} = {p: 2}", "1; let [p] = [2]", "(function () { 1 })()", "(function () { return 1 })()", "(() => 2)()", "1; (() => { 3 })()", "eval('1;')", "eval('1; var ev = 2')", "eval('var ev = 2; 3')",
  "eval('if (1) { 4 }')", "eval('L: { 5; break L }')", "1; eval('')", "1; eval(';')", "1; eval('{}')", "eval('do { 6; break } while (0)')", "(0, eval)('7; var e1')", "eval('1; try { 2 } finally { 3 }')",
  "new Function('return 1')()", "1; new Function('1')()", "x = 1; x", "x = 1; x++", "x = 1; ++x", "var x = 1; x++; x", "var x = 1; x += 2", "1; delete globalThis.nope", "1; void 0; ;",
  "async function af() {}", "1; async function af() {}", "function* gf() {}", "1; function* gf() {}", "1; (async () => {})", "var g = function* () {}; g().next().done",
  "1; { function h() {} 2 }", "{ function h() {} } h", "if (1) function h() {}", "1; if (1) function h() {}", "if (0) function h() {}", "1; if (0) function h() {}");

// 9. Combinações aninhadas geradas.
const wrappers = [
  b => b,
  b => `{ ${b} }`,
  b => `if (1) { ${b} }`,
  b => `if (0) 0; else { ${b} }`,
  b => `try { ${b} } finally { }`,
  b => `try { ${b} } catch (e) { 0 }`,
  b => `L: { ${b} }`,
  b => `do { ${b}; break } while (0)`,
  b => `with ({}) { ${b} }`,
];
const leaves = ["1", "1; if (0) 2", "1; if (1) {} else 3", "1; for (;false;);", "1; var a2 = 2", "1; function f2() {}", "1; ;", "1; {}", "4; switch(0){}", "5; try { } finally { 6 }"];
for (const w of wrappers) for (const lf of leaves) add(w(lf), `0; ${w(lf)}`, `${w(lf)}; 7`, `${w(lf)}; if (0) 8`);

// 10. Valores não primitivos e erros.
add("1; throw new Error('x')", "({a: 1})", "[1, 2]", "1; (function () {})", "1; (class {})", "new Error('e')", "Symbol('s')", "1; null", "try { null.x } catch (e) { e }", "1; (() => {})",
  "x = {}; x.y = 1", "if (1) ({})", "if (1) [1]", "do { ({}) } while (0)", "1; let;", "var let = 1; let", "1; if (1) ;", "1; do ; while (0)", "1; while (0);", "1 +", "1; var 5", "break", "continue",
  "L: { break M }", "return 1", "1; await 2", "function f() { 1 }", "yield");

const basePrograms = new Set(progs);

// 11. Pares de átomos, embrulhados: `A; B` sob cada wrapper, e `A` sob um wrapper seguido de `B`.
const atoms = ["1", "2", "", ";", "{}", "{ 3 }", "var a", "var b = 4", "let l = 5", "function f() {}", "class K {}", "if (0) 6", "if (1) 7", "if (1) {} else 8",
  "L: { 9; break L }", "L: break L", "for (;false;);", "do { 10; break } while (0)", "while (0) 11", "switch (1) { case 1: 12 }", "switch (0) { case 1: 12 }",
  "try { 13 } finally { 14 }", "try { throw 0 } catch (e) { 15 }", "with ({}) 16", "debugger", "(void 0)", "'s'", "null", "var c = 17, d", "{ var e = 18 }"];
const pairWrappers = [
  (a, b) => `${a}; ${b}`,
  (a, b) => `{ ${a}; ${b} }`,
  (a, b) => `if (1) { ${a}; ${b} }`,
  (a, b) => `do { ${a}; ${b} } while (0)`,
  (a, b) => `L: { ${a}; ${b} }`,
  (a, b) => `try { ${a}; ${b} } finally { }`,
  (a, b) => `try { ${a}; ${b} } catch (e) { 0 }`,
  (a, b) => `{ ${a} } ${b}`,
  (a, b) => `if (0) 0; else { ${a} } ${b}`,
  (a, b) => `${a}; { ${b} }`,
  (a, b) => `for (var q = 0; q < 1; q++) { ${a}; ${b} }`,
  (a, b) => `switch (1) { case 1: ${a}; ${b} }`,
];
for (const w of pairWrappers) for (const a of atoms) for (const b of atoms) add(w(a, b));
// Interrupções (break/continue) no meio do corpo de cada laço, depois de um valor e depois de um vazio.
const jumps = ["break", "continue", "{ break }", "{ continue }", "if (1) break", "if (1) continue", "if (0) break", "try { break } finally { 20 }", "try { continue } finally { 21 }",
  "try { 22 } finally { break }", "try { 23 } finally { continue }", "with ({}) break", "with ({}) continue", "switch (1) { case 1: break }"];
const loopForms = [
  j => `do { ${j} } while (0)`,
  j => `while (true) { ${j}; break }`,
  j => `for (var i = 0; i < 2; i++) { ${j} }`,
  j => `for (var k in {a: 1, b: 2}) { ${j} }`,
  j => `for (var o of [1, 2]) { ${j} }`,
  j => `L: for (;;) { ${j}; break L }`,
];
const priors = ["", "30; ", "var p; ", "30; var p; ", "if (0) 31; ", "{} ", "30; {} "];
for (const lf of loopForms) for (const j of jumps) for (const pr of priors) add(lf(pr + j), `40; ${lf(pr + j)}`, `${lf(pr + j)}; 41`);

// ---- Programas (fonte do eval x modo), dedup contra os outros goldens e execução com filhos em paralelo.
const dir = path.join(__dirname, "..", "tests", "golden");
const otherSources = new Set();
for (const file of fs.readdirSync(dir)) {
  if (!file.endsWith(".tsv") || file === "completion_value_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(dir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { otherSources.add(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const programs = [];
let dup = 0;
for (const src of progs) {
  for (const mode of basePrograms.has(src) ? [0, 1, 2] : [0]) {
    const text = PRELUDE + `globalThis.R=C(${JSON.stringify(src)},${mode});`;
    if (otherSources.has(text)) { dup++; continue; }
    programs.push(text);
  }
}

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let done = false;
    const finish = (value) => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => { try { child.kill("SIGKILL"); } catch (e) {} finish(null); }, 5000);
    child.stdout.on("data", (d) => (out += d));
    child.on("error", () => finish(null));
    child.on("close", (code) => finish(code === 0 ? decodeResult(out) : null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  await Promise.all(Array.from({ length: 8 }, async () => {
    while (next < programs.length) {
      const i = next++;
      results[i] = await runChild(programs[i]);
    }
  }));
  const rows = [];
  let dropped = 0;
  programs.forEach((source, i) => {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(source).slice(0, 200) + "\n");
      return;
    }
    rows.push({ source, result });
  });
  process.stdout.write(emitFactored("completion_value", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
