// Gera tests/golden/global_semantics_bun.tsv: semântica de globais medida no bun 1.4.2 (var/function/let/const/class no
// topo de script, propriedades do global contra o registro léxico, descritores, redeclaração entre scripts, `delete`,
// atribuição a não declarada, TDZ, getters/setters e propriedades não graváveis no global, `defineProperty` sobre o
// global, `this` no topo, `with(globalThis)`, Annex B de função em bloco, ordem de inicialização entre scripts e
// eval indireto). Um programa é uma lista de scripts separados por SEP; cada script roda em sequência no mesmo
// contexto (vm.runInThisContext, sem o invólucro de módulo do bun), num processo novo. O resultado é
// `<erros dos scripts separados por |>#<texto de globalThis.R>`; cada erro é `índice:Nome: mensagem`.
// Colunas: a fonte (JSON) e o resultado (JSON). Todos os programas são conferidos.
// O que o golden mede é o JavaScriptCore, não o host do bun. As propriedades que só o bun define (process, Bun,
// fetch, setTimeout...) nunca entram nos programas, e os programas que olham o próprio objeto global (protótipo,
// Symbol.toStringTag, extensibilidade, nomes próprios) rodam num contexto novo de `vm.createContext({})`, cujo
// global é o JSGlobalObject puro: só nomes de ECMAScript mais `console` (medido: a lista de nomes próprios do
// contexto novo menos a do global do bun é vazia, e nenhum programa lê `console`). Os resultados desses programas
// foram conferidos iguais nos dois ambientes (o global do bun tem Object.prototype como protótipo e nenhum símbolo).
// O sandbox do contexto puro é um Proxy cuja armadilha defineProperty engole a escrita: o NodeVMGlobalObject do bun
// espelha cada função declarada no sandbox (e `getOwnPropertyNames` lista o sandbox antes), e sem o espelho a ordem
// é a da SymbolTable do JSC puro (medido: `function b(){};function c(){};function z(){};var q,r` dá
// `r,Infinity,NaN,b,q,undefined,c,z`, igual ao simulador de bucket do WTF; com `{}` simples dá `b,c,z,r,...`).
// Uso: bun scripts/gen-global-semantics-golden.js > tests/golden/global_semantics_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const SEP = "\n/*--script--*/\n";
const programs = [];
const pureBodies = new Set();
const add = (...scripts) => programs.push(scripts.join(SEP));
const addPure = (...scripts) => {
  const body = scripts.join(SEP);
  pureBodies.add(body);
  programs.push(body);
};
const STRICT = "'use strict';\n";
const CATCH = "catch (e) { globalThis.R = e.name + ': ' + e.message }";
// Script que grava em R o texto de uma expressão, ou "Nome: mensagem" se lançar.
const show = (expr, strict) => `${strict ? STRICT : ""}try { globalThis.R = String(${expr}) } ${CATCH}`;
// Script que roda instruções e depois grava em R o texto de uma expressão.
const runThen = (stmts, expr, strict) =>
  `${strict ? STRICT : ""}try { ${stmts}; globalThis.R = 'ok:' + String(${expr}) } ${CATCH}`;
const DESC = d =>
  `(function (o) { if (!o) return 'none'; return ('value' in o ? 'v:' + (typeof o.value === 'function' ? 'fn' : String(o.value)) : 'a:' + typeof o.get + '/' + typeof o.set) + ' w' + o.writable + ' e' + o.enumerable + ' c' + o.configurable })(${d})`;
const STATE = n => `(function () { try { return typeof ${n} + '/' + String(${n}) } catch (e) { return e.name } })()`;

// ---- A. Cada tipo de declaração contra cada sonda (propriedade do global ou registro léxico).
const decls = [
  "var n = 1;", "var n;", "function n() {}", "let n = 1;", "let n;", "const n = 1;", "class n {}",
  "var n = 1; var n = 2;", "function n() {} var n;", "var n; function n() {}",
];
const probes = [
  "typeof globalThis.n", "'n' in globalThis", "globalThis.hasOwnProperty('n')",
  "Object.getOwnPropertyNames(globalThis).includes('n')", "Object.keys(globalThis).includes('n')",
  "Reflect.ownKeys(globalThis).includes('n')", DESC("Object.getOwnPropertyDescriptor(globalThis, 'n')"),
  "delete globalThis.n", "delete n", "typeof n", "n", "this.n", "globalThis.n", "(0, eval)('typeof n')", "(0, eval)('n')",
  "new Function('return typeof n')()", "(function () { return typeof n })()", "(globalThis.n = 5, n)",
  "(n = 5, globalThis.n)", "(delete globalThis.n, typeof n)", "(Object.defineProperty(globalThis, 'n', { value: 9 }), n)",
  "globalThis.propertyIsEnumerable('n')", "JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'n'))",
  "Object.getOwnPropertyDescriptor(globalThis, 'n') === undefined", "n === globalThis.n", "(n++, n)", "(n = n, n)",
  "typeof n === typeof globalThis.n",
];
for (const d of decls) {
  const dn = d.replace(/\bn\b/g, "gx");
  for (const p of probes) {
    const pn = p.replace(/'n'/g, "'gx'").replace(/\bn\b/g, "gx");
    add(dn, show(pn));
    add(`${STRICT}${dn}\n${show(pn, true)}`);
  }
}

// ---- B. Redeclaração: cada par de declarações, no mesmo script, em scripts distintos, e por eval.
const redecl = [
  "var n = 1;", "var n;", "function n() { return 1 }", "let n = 1;", "const n = 1;", "class n {}",
];
for (const a of redecl) for (const b of redecl) {
  const an = a.replace(/\bn\b/g, "rd"), bn = b.replace(/\bn\b/g, "rd").replace("return 1", "return 2");
  const probe = show("typeof rd + '/' + String(rd)");
  add(an, bn, probe);
  add(`${an}\n${bn}`, probe);
  add(an, `try { (0, eval)(${JSON.stringify(bn)}) } catch (e) { globalThis.E = e.name + ': ' + e.message }`, show("typeof rd + '/' + globalThis.E"));
  add(an, `try { eval(${JSON.stringify(bn)}) } catch (e) { globalThis.E = e.name + ': ' + e.message }`, show("typeof rd + '/' + globalThis.E"));
  add(an, `(function () { try { eval(${JSON.stringify(bn)}) } catch (e) { globalThis.E = e.name + ': ' + e.message } })()`, show("typeof rd + '/' + globalThis.E"));
  add(`(0, eval)(${JSON.stringify(an)});`, bn, probe);
  add(STRICT + an, STRICT + bn, probe);
}
// let/const/class falhos não deixam o nome declarado; script com erro de sintaxe não declara nada.
for (const d of ["let sx = 1;", "const sx = 1;", "class sx {}", "var sx = 1;", "function sx() {}"]) {
  add(`${d} let sx = 2;`, show("typeof sx"));
  add(`${d} syntax error here`, show("typeof sx"));
  add(`${d} throw new Error('boom')`, show("typeof sx"));
  add(`${d} throw new Error('boom')`, "let sx = 3;", show("typeof sx + '/' + sx"));
  add(`${d} throw new Error('boom')`, "var sx = 3;", show("typeof sx + '/' + sx"));
  add("let sx = (function () { throw new Error('b') })();", show("typeof sx"));
  add("let sx = (function () { throw new Error('b') })();", "let sx = 1;", show("typeof sx"));
  add("let sx = (function () { throw new Error('b') })();", "var sx = 1;", show("typeof sx"));
  add("const sx = (function () { throw new Error('b') })();", show("sx"));
  add("class sx { static [(function () { throw new Error('b') })()]() {} }", show("typeof sx"));
}

// ---- C. let/const/class/var/function sobre propriedades que já existem no global.
const existing = ["Array", "Object", "undefined", "NaN", "Infinity", "globalThis", "parseInt", "eval", "Math", "JSON", "Symbol", "Reflect", "escape", "Number", "Boolean"];
const over = [
  "let n = 1;", "const n = 1;", "class n {}", "var n;", "var n = 2;", "function n() {}", "let n;",
  "{ function n() {} }", "if (true) { function n() {} }",
];
for (const nm of existing) for (const o of over) {
  const body = o.replace(/\bn\b/g, nm);
  add(body, show(`typeof ${nm}`));
  add(body, show(`typeof globalThis.${nm}`));
  add(body, show(DESC(`Object.getOwnPropertyDescriptor(globalThis, '${nm}')`)));
}
// Propriedade definida pelo programa: configurável ou não, dado ou acessor.
const attrs = [
  "{ value: 1 }", "{ value: 1, configurable: true }", "{ value: 1, writable: true }", "{ value: 1, writable: true, configurable: true }",
  "{ value: 1, writable: true, enumerable: true }", "{ value: 1, writable: true, enumerable: true, configurable: true }",
  "{ get() { return 7 } }", "{ get() { return 7 }, configurable: true }", "{ set(v) { globalThis.W = v } }",
  "{ get() { return 7 }, set(v) { globalThis.W = v }, configurable: true }", "{ get() { return 7 }, set(v) { globalThis.W = v } }",
];
const redefs = ["let dp = 1;", "const dp = 1;", "class dp {}", "var dp;", "var dp = 2;", "function dp() {}", "dp = 3;", "dp++;", "delete globalThis.dp;"];
for (const at of attrs) for (const r of redefs) {
  const setup = `Object.defineProperty(globalThis, 'dp', ${at});`;
  const after = show(`${STATE("dp")} + '|' + String(globalThis.W) + '|' + ${DESC("Object.getOwnPropertyDescriptor(globalThis, 'dp')")}`);
  add(setup, r, after);
  add(setup, STRICT + r, after);
  add(`${setup}\n${r}`, after);
}

// ---- D. Atribuição, leitura e delete de nomes em cada estado inicial, sloppy e strict.
const states = {
  none: "", plain: "globalThis.u = 1;", var: "var u = 1;", let: "let u = 1;", const: "const u = 1;",
  fn: "function u() {}", class: "class u {}", ro: "Object.defineProperty(globalThis, 'u', { value: 1 });",
  rw: "Object.defineProperty(globalThis, 'u', { value: 1, writable: true, configurable: true });",
  getter: "Object.defineProperty(globalThis, 'u', { get() { return 7 }, configurable: true });",
  setter: "Object.defineProperty(globalThis, 'u', { set(v) { globalThis.W = v }, configurable: true });",
  both: "Object.defineProperty(globalThis, 'u', { get() { return 7 }, set(v) { globalThis.W = v } });",
  tdz: "try { let u = (function () { throw new Error('b') })() } catch (e) {} ",
  proto: "Object.prototype.u = 1;", protoRo: "Object.defineProperty(Object.prototype, 'u', { value: 1 });",
};
const ops = [
  "u = 2", "u", "typeof u", "delete u", "delete globalThis.u", "u++", "u += 1", "u ||= 3", "u &&= 3", "u ??= 4", "[u] = [5]",
  "({ u } = { u: 6 })", "for (u of [7, 8]);", "for (u in { a: 1 });", "u = (delete globalThis.u, 3)", "void (u = 2, u = 3)",
  "(function () { u = 9 })()", "(function () { return u })()", "eval('u = 10')", "(0, eval)('u = 11')", "new Function('u = 12')()",
];
for (const [sn, setup] of Object.entries(states)) for (const op of ops) {
  const probeExpr = `${STATE("u")} + '|' + String(globalThis.W) + '|' + ${DESC("Object.getOwnPropertyDescriptor(globalThis, 'u')")}`;
  for (const strict of [false, true]) {
    add(setup, runThen(op, probeExpr, strict));
    if (sn === "none" || sn === "let") add(`${setup}\n${runThen(op, probeExpr, strict)}`);
  }
}

// ---- E. undefined, NaN, Infinity: propriedades não graváveis, não configuráveis.
const frozen = ["undefined", "NaN", "Infinity"];
const frozenOps = [
  "n = 1", "n++", "++n", "n += 1", "n ??= 1", "delete n", "delete globalThis.n", "globalThis.n = 1", "typeof n", "n",
  "Object.defineProperty(globalThis, 'n', { value: 1 })", "Object.defineProperty(globalThis, 'n', { value: n })",
  "Object.defineProperty(globalThis, 'n', { get() {} })", "Object.defineProperty(globalThis, 'n', { enumerable: true })",
  "Reflect.set(globalThis, 'n', 1)", "Reflect.deleteProperty(globalThis, 'n')", "Reflect.defineProperty(globalThis, 'n', { value: 1 })",
  "Object.getOwnPropertyDescriptor(globalThis, 'n').writable", "JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'n'))",
  "(function () { n = 1 })()", "(function (n) { n = 2; return n })(0)", "(function () { var n = 3; return n })()",
  "(function () { let n = 3; return n })()", "(function n() { n = 4; return typeof n })()", "(() => { n = 1 })()",
];
for (const nm of frozen) for (const op of frozenOps) {
  const o = op.replace(/\bn\b(?!')/g, nm).replace(/'n'/g, `'${nm}'`);
  for (const strict of [false, true]) {
    add(runThen(o, `String(${nm})`, strict));
    add(`${strict ? STRICT : ""}try { globalThis.R = String((0, eval)(${JSON.stringify(o)})) } ${CATCH}`);
    add(`${strict ? STRICT : ""}try { globalThis.R = String(new Function(${JSON.stringify(strict ? STRICT + o : o)})()) } ${CATCH}`);
  }
}
for (const nm of frozen) for (const d of [`var ${nm};`, `var ${nm} = 5;`, `function ${nm}() {}`, `let ${nm} = 1;`, `const ${nm} = 1;`, `class ${nm} {}`, `{ function ${nm}() {} }`]) {
  add(d, show(`typeof ${nm} + '/' + String(${nm})`));
  add(STRICT + d, show(`typeof ${nm} + '/' + String(${nm})`));
  add(`(0, eval)(${JSON.stringify(d)})`, show(`typeof ${nm} + '/' + String(${nm})`));
  add(`(function () { ${d} return typeof ${nm} + '/' + String(${nm}) })()`.replace(/^/, "globalThis.R = "), "");
}

// ---- F. TDZ.
const tdz = [
  "typeof x; let x;", "x; let x;", "x = 1; let x;", "let x = x;", "const x = x + 1;", "class x extends x {}", "class x { static a = x }",
  "class x { static [x] = 1 }", "let x = (() => x)();", "let x = typeof x;", "let x = (() => typeof x)();", "{ typeof x; let x }",
  "{ x = 1; let x }", "function f() { return x } f(); let x = 1;", "function f() { return typeof x } f(); let x = 1;",
  "var f = () => x; try { f() } catch (e) { globalThis.E1 = e.message } let x = 1; globalThis.E2 = f()",
  "try { x } catch (e) { globalThis.E1 = e.message } let x = 1", "try { x = 2 } catch (e) { globalThis.E1 = e.message } let x = 1",
  "try { typeof x } catch (e) { globalThis.E1 = e.message } let x = 1", "try { delete x } catch (e) { globalThis.E1 = String(e) } let x = 1",
  "try { x++ } catch (e) { globalThis.E1 = e.message } let x = 1", "const x = 1; try { x = 2 } catch (e) { globalThis.E1 = e.message }",
  "const x = 1; try { x++ } catch (e) { globalThis.E1 = e.message }", "const x = 1; try { x += 1 } catch (e) { globalThis.E1 = e.message }",
  "const x = 1; try { x ||= 3 } catch (e) { globalThis.E1 = e.message } globalThis.E2 = x", "const x = 1; try { [x] = [2] } catch (e) { globalThis.E1 = e.message }",
  "const x = 1; try { for (x of [1]); } catch (e) { globalThis.E1 = e.message }", "const x = 1; try { for (x in {a: 1}); } catch (e) { globalThis.E1 = e.message }",
  "const x = 1; try { ({ x } = { x: 2 }) } catch (e) { globalThis.E1 = e.message }", "class x {}; try { x = 1 } catch (e) { globalThis.E1 = e.message } globalThis.E2 = typeof x",
  "const x = 1; try { delete x } catch (e) { globalThis.E1 = e.message } globalThis.E2 = typeof x",
];
for (const t of tdz) {
  const fin = show("String(globalThis.E1) + '|' + String(globalThis.E2) + '|' + (function () { try { return typeof x + String(x) } catch (e) { return e.name + ': ' + e.message } })()");
  add(t, fin);
  add(STRICT + t, fin.replace(STRICT, ""));
  add(`(0, eval)(${JSON.stringify(t)})`, fin);
  add(t, "x = 7;", fin);
  add(t, STRICT + "x = 7;", fin);
  add(t, "var x = 7;", fin);
  add(t, "function x() {}", fin);
}

// ---- G. this, globalThis e o descritor do global.
const thisProbes = [
  "this === globalThis", "typeof this", "(function () { return this === globalThis })()", "(function () { 'use strict'; return this })()",
  "(() => this === globalThis)()", "({ m() { return this === globalThis } }).m()", "({ get g() { return this === globalThis } }).g",
  "(function () { return this }).call(undefined) === globalThis", "(function () { return this }).call(null) === globalThis",
  "(function () { return typeof this }).call(1)", "(function () { 'use strict'; return typeof this }).call(1)", "eval('this === globalThis')",
  "(0, eval)('this === globalThis')", "new Function('return this === globalThis')()", "new Function('\"use strict\"; return this')()",
  "(function () { return eval('this === globalThis') })()", "(function () { 'use strict'; return (0, eval)('this === globalThis') })()",
  "globalThis.globalThis === globalThis", "this.globalThis === globalThis", JSON_DESC("globalThis"), "typeof globalThis", "Object.is(this, globalThis)",
  "delete globalThis.globalThis", "(delete globalThis.globalThis, typeof globalThis)", "globalThis = 1", "(globalThis = 1, typeof globalThis)",
  "Object.getOwnPropertyDescriptor(globalThis, 'this')", "'this' in globalThis", "globalThis.hasOwnProperty('globalThis')",
  "Object.keys(globalThis).includes('globalThis')", "Object.getOwnPropertyNames(globalThis).includes('globalThis')",
  "this.x = 1, typeof x", "(this.y = 2, y)", "(y = 3, this.y)", "Reflect.has(this, 'Array')", "this.Array === Array", "Array === globalThis.Array",
  "Object.getOwnPropertyNames(globalThis).includes('Array')", JSON_DESC("Array"), JSON_DESC("Math"), JSON_DESC("JSON"), JSON_DESC("Reflect"),
  JSON_DESC("parseInt"), JSON_DESC("eval"), JSON_DESC("undefined"), JSON_DESC("NaN"), JSON_DESC("Infinity"), JSON_DESC("escape"),
  JSON_DESC("Symbol"), JSON_DESC("Proxy"), JSON_DESC("Promise"), JSON_DESC("Atomics"), JSON_DESC("WebAssembly"), JSON_DESC("Intl"),
  "typeof Intl", "typeof WebAssembly", "typeof SharedArrayBuffer", "typeof FinalizationRegistry", "typeof WeakRef", "typeof AggregateError",
  "typeof Iterator", "typeof Temporal", "typeof ShadowRealm", "typeof DisposableStack",
  "Math.hasOwnProperty('abs') && globalThis.hasOwnProperty('Math')", "Array.prototype.constructor === Array", "globalThis.constructor === Object",
];
function JSON_DESC(nm) {
  return `JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, '${nm}') && { w: Object.getOwnPropertyDescriptor(globalThis, '${nm}').writable, e: Object.getOwnPropertyDescriptor(globalThis, '${nm}').enumerable, c: Object.getOwnPropertyDescriptor(globalThis, '${nm}').configurable, t: typeof Object.getOwnPropertyDescriptor(globalThis, '${nm}').value })`;
}
for (const p of thisProbes) {
  add(show(p));
  add(show(p, true));
  add("var x;", show(p));
  add("let x;", show(p, true));
}
// O próprio objeto global (protótipo, toStringTag...): medido no global puro do JSC, sem o host do bun.
for (const p of [
  "Object.prototype.toString.call(globalThis)", "String(globalThis)", "globalThis[Symbol.toStringTag]",
  "JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, Symbol.toStringTag))", "Object.getPrototypeOf(globalThis) === Object.prototype",
  "Object.getPrototypeOf(Object.getPrototypeOf(globalThis)) === Object.prototype", "Object.getPrototypeOf(globalThis) === null",
  "Object.prototype.toString.call(this)", "Symbol.toStringTag in globalThis", "Object.isExtensible(globalThis)", "Object.isFrozen(globalThis)",
  "globalThis.constructor.name", "Object.getOwnPropertySymbols(globalThis).length",
]) addPure(show(p));

// ---- H. with(globalThis) e with sobre o global.
const withs = [
  "with (globalThis) { var wx = 1 } typeof wx", "with (globalThis) { wx = 1 } typeof wx", "with (globalThis) { typeof Array }",
  "with (globalThis) { var Array = 1 } typeof Array", "with (globalThis) { let wx = 1 } typeof wx", "with (globalThis) { function wf() {} } typeof wf",
  "var wx = 1; with (globalThis) { wx = 2 } globalThis.wx", "let wx = 1; with (globalThis) { wx = 2 } wx + '/' + globalThis.wx",
  "let wx = 1; with (globalThis) { var wx2 = wx } wx2", "globalThis.wx = 1; let wx = 2; with (globalThis) { wx = 3 } wx + '/' + globalThis.wx",
  "globalThis.wx = 1; let wx = 2; with (globalThis) { typeof wx }", "with (globalThis) { delete globalThis.Array } typeof Array",
  "with ({ wx: 1 }) { var wx } typeof wx", "with ({ wx: 1 }) { var wx = 2 } wx", "with ({ wx: 1 }) { var wx = 2 } globalThis.wx",
  "with ({ wx: 1 }) { wx = 5 } typeof wx", "with ({}) { wx = 5 } typeof wx", "with (globalThis) { this === globalThis }",
  "with (globalThis) { (function () { return this === globalThis })() }", "with ({ f() { return this } }) { typeof f() }",
  "with ({ f() { return this.k }, k: 3 }) { f() }", "with (globalThis) { undefined = 1 } typeof undefined",
  "with (globalThis) { NaN = 1 } String(NaN)", "with (globalThis) { eval('var wv = 1') } typeof wv",
  "with (globalThis) { (0, eval)('var wv = 1') } typeof wv", "with (globalThis) { new Function('return typeof wx')() }",
  "with ({ [Symbol.unscopables]: { wx: true }, wx: 1 }) { typeof wx }", "var wx = 'g'; with ({ [Symbol.unscopables]: { wx: true }, wx: 1 }) { wx }",
  "with (globalThis) { x = 5 } typeof x", "with (globalThis) { typeof wzz }", "with (globalThis) { wzz }",
  "Object.defineProperty(globalThis, Symbol.unscopables, { value: { Array: true }, configurable: true }); with (globalThis) { typeof Array }",
  "globalThis[Symbol.unscopables] = { wx: true }; globalThis.wx = 1; var wx2 = 'o'; with (globalThis) { typeof wx }",
];
for (const w of withs) {
  const [pre, last] = [w.replace(/;?\s*[^;}]*$/, ""), w];
  add(show(`eval(${JSON.stringify(w.replace(/^(.*[;}]\s*|)([^;}]+)$/s,"$1globalThis.R = $2"))})`));
  add(`globalThis.R = (0, eval)(${JSON.stringify(w)})`.replace("globalThis.R = ", "try { globalThis.R = ") + ` } ${CATCH}`);
  add(`try { globalThis.R = eval(${JSON.stringify(w)}) } ${CATCH}`);
  add(`try { globalThis.R = (function () { return eval(${JSON.stringify(w)}) })() } ${CATCH}`);
  void pre; void last;
}
// with em script de verdade (sloppy), lendo o resultado com uma sonda.
for (const [body, expr] of [
  ["with (globalThis) { var wa = 1 }", "typeof wa"], ["with (globalThis) { wa = 1 }", "typeof wa"], ["with (globalThis) { var wa = 1; wa = 2 }", "globalThis.wa"],
  ["var wa = 0; with (globalThis) { var wa = 1 }", "globalThis.wa"], ["let wa = 0; with (globalThis) { wa = 1 }", "wa + '/' + globalThis.wa"],
  ["let wa = 0; globalThis.wa = 9; with (globalThis) { wa = 1 }", "wa + '/' + globalThis.wa"], ["with (globalThis) { delete globalThis.Array }", "typeof Array"],
  ["with (globalThis) { function wf() {} }", "typeof wf + '/' + typeof globalThis.wf"], ["with (globalThis) { let wl = 1 }", "typeof wl"],
  ["with (globalThis) { Object.defineProperty(globalThis, 'wd', { get() { return 8 }, configurable: true }); wd }", "wd"],
  ["globalThis.wc = 1; with (globalThis) { delete globalThis.wc; wc = 2 }", "globalThis.wc"],
  ["globalThis.wc = 1; with (globalThis) { (function () { delete globalThis.wc; wc = 2 })() }", "String(globalThis.wc) + typeof wc"],
  ["globalThis.wc = 1; with (globalThis) { wc = (delete globalThis.wc, 2) }", "String(globalThis.wc) + typeof wc"],
  ["with (globalThis) { var undefined = 3 }", "typeof undefined"],
]) {
  add(body, show(expr));
  add(`${body}\n${show(expr)}`);
}

// ---- I. Annex B: função em bloco no topo de script.
const annex = [
  "typeof f; { function f() {} }; typeof f", "{ function f() {} } typeof f", "{ function f() {} } typeof globalThis.f", "if (true) { function f() {} } typeof f",
  "if (false) { function f() {} } typeof f", "if (true) function f() {} typeof f", "if (false) function f() {} typeof f", "switch (1) { case 1: function f() {} } typeof f",
  "switch (0) { case 1: function f() {} } typeof f", "typeof f; switch (1) { case 1: function f() {} } typeof f", "label: function f() {} typeof f", "{ function f() {} function f() { return 2 } } f()",
  "let f = 1; { function f() {} } f", "{ let f = 1; { function f() {} } } typeof f", "const f = 1; { function f() {} } f", "class f {} { function f() {} } typeof f",
  "var f = 1; { function f() {} } typeof f", "var f = 1; { f = 2; function f() {} } f", "{ f = 2; function f() {} } f", "{ function f() { return 1 } f = 2; } typeof f",
  "{ function f() { return 1 } } f()", "{ function* f() {} } typeof f", "{ async function f() {} } typeof f", "{ class f {} } typeof f", "try { function f() {} } catch (e) {} typeof f",
  "try { throw 1 } catch (f) { { function f() {} } } typeof f", "try { throw 1 } catch (f) { function f() {} }", "try { throw 1 } catch ([f]) { { function f() {} } } typeof f",
  "for (let i = 0; i < 1; i++) { function f() {} } typeof f", "for (var i = 0; i < 1; i++) { function f() {} } typeof f", "for (let f of [1]) { function f() {} }",
  "(function () { { function f() {} } return typeof f })()", "(function () { typeof f; { function f() {} } return typeof f })()", "(function (f) { { function f() {} } return typeof f })(1)",
  "(function () { let f; { function f() {} } return typeof f })()", "(function () { 'use strict'; { function f() {} } return typeof f })()", "'use strict'; { function f() {} } typeof f",
  "'use strict'; if (true) function f() {}", "'use strict'; label: function f() {}", "{ function arguments() {} }", "{ function eval() {} } typeof eval",
  "{ function undefined() {} } typeof undefined", "{ function NaN() {} } typeof NaN", "{ function Array() {} } typeof Array + '/' + (Array === globalThis.Array)",
  "{ function parseInt() { return 1 } } parseInt('2')", "{ function f() {} } delete globalThis.f", "{ function f() {} } JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'f') && Object.getOwnPropertyDescriptor(globalThis, 'f').configurable)",
  "{ function f() {} } Object.keys(globalThis).includes('f')", "globalThis.f = 1; { function f() {} } typeof f", "Object.defineProperty(globalThis, 'f', { value: 1 }); { function f() {} } typeof f",
  "Object.defineProperty(globalThis, 'f', { value: 1, writable: true }); { function f() {} } typeof f", "Object.defineProperty(globalThis, 'f', { get() { return 1 } }); { function f() {} } typeof f",
  "Object.defineProperty(globalThis, 'f', { get() { return 1 }, configurable: true }); { function f() {} } typeof f",
  "{ function f() { return 'a' } } { function f() { return 'b' } } f()", "{ function f() { return 'a' } } function f() { return 'c' } f()", "function f() { return 'c' } { function f() { return 'a' } } f()",
  "f(); { function f() {} }", "typeof f === 'undefined' && (function () { { function f() {} } return typeof f })()", "{ function f() {} } { function g() {} } typeof f + typeof g",
  "{ function f() {} } eval('typeof f')", "{ function f() {} } (0, eval)('typeof f')", "eval('{ function f() {} } typeof f')", "(0, eval)('{ function f() {} } typeof f')",
  "(0, eval)('{ function f() {} }'); typeof f", "(0, eval)('{ function f() {} }'); typeof globalThis.f", "eval('{ function f() {} }'); typeof f",
];
for (const a of annex) {
  add(show(`eval(${JSON.stringify(a.replace(/^(.*[;}]\s*|)([^;}]+)$/s,"$1globalThis.R2 = $2"))})`).replace("String(eval(", "(0, String)(eval("));
  add(`try { ${a.replace(/^(.*[;}]\s*|)([^;}]+)$/s,"$1globalThis.R = $2")} } ${CATCH}`);
  add(a.replace(/^(.*[;}]\s*|)([^;}]+)$/s,"$1globalThis.R = String($2)"));
  add(a.replace(/^(.*[;}]\s*|)([^;}]+)$/s,"$1globalThis.R = String($2)").replace(/\bf\b/g, "ff"));
  add(a.replace(/\btypeof [^;}]+$/s, "").replace(/^(.*[;}\s])$/s, "$1"), show("typeof f + '/' + typeof globalThis.f + '/' + (function () { try { return String(f) } catch (e) { return e.name } })()"));
}

// ---- J. Ordem de inicialização entre scripts, eval indireto e configurabilidade.
const multi = [
  ["var a = 1;", "var a; globalThis.R = String(a)"], ["var a = 1;", "var a = 2; globalThis.R = String(a)"], ["var a = 1;", "var a; var a; globalThis.R = String(a)"],
  ["function g() { return lx }", "let lx = 5; globalThis.R = String(g())"], ["function g() { return lx }; try { g() } catch (e) { globalThis.E = e.name + ': ' + e.message }", "let lx = 5; globalThis.R = globalThis.E + '|' + g()"],
  ["let lx = 1;", "function g() { return lx } globalThis.R = String(g())"], ["let lx = 1;", "globalThis.lx = 2; globalThis.R = lx + '/' + globalThis.lx"],
  ["globalThis.s = 1;", "let s = 2; globalThis.R = s + '/' + globalThis.s"], ["globalThis.s = 1;", "var s = 2; globalThis.R = s + '/' + globalThis.s"],
  ["globalThis.s = 1;", "function s() {} globalThis.R = typeof s + '/' + typeof globalThis.s"], ["globalThis.s = 1;", "class s {} globalThis.R = typeof s + '/' + typeof globalThis.s"],
  ["(0, eval)('var ev = 1');", "globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'ev'))"],
  ["(0, eval)('function ev() {}');", "globalThis.R = JSON.stringify([Object.getOwnPropertyDescriptor(globalThis, 'ev').configurable, Object.getOwnPropertyDescriptor(globalThis, 'ev').enumerable, Object.getOwnPropertyDescriptor(globalThis, 'ev').writable])"],
  ["(0, eval)('var ev = 1');", "globalThis.R = String(delete globalThis.ev) + typeof ev"], ["(0, eval)('var ev = 1');", "globalThis.R = String(delete ev) + typeof ev"],
  ["(0, eval)('var ev = 1'); delete globalThis.ev;", "let ev = 2; globalThis.R = String(ev)"], ["(0, eval)('var ev = 1');", "let ev = 2; globalThis.R = String(ev)"],
  ["(0, eval)('var ev = 1');", "const ev = 2; globalThis.R = String(ev)"], ["var ev = 1;", "(0, eval)('var ev = 2'); globalThis.R = String(ev) + String(delete globalThis.ev)"],
  ["(0, eval)('let ev = 1');", "globalThis.R = typeof ev"], ["(0, eval)('let ev = 1');", "let ev = 2; globalThis.R = String(ev)"], ["(0, eval)('const ev = 1');", "var ev = 2; globalThis.R = String(ev)"],
  ["(0, eval)('class ev {}');", "globalThis.R = typeof ev"], ["eval('var ev = 1');", "globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'ev'))"],
  ["eval('function ev() {}');", "globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'ev').configurable)"],
  ["let ev = 1;", "(0, eval)('var ev = 2')"], ["let ev = 1;", "eval('var ev = 2')"], ["let ev = 1;", "(0, eval)('function ev() {}')"], ["const ev = 1;", "(0, eval)('var ev')"], ["class ev {}", "(0, eval)('var ev')"],
  ["var ev;", "(0, eval)('let ev = 2; globalThis.R = String(ev)')"], ["var ev;", "(0, eval)('let ev = 2'); globalThis.R = typeof ev + String(globalThis.hasOwnProperty('ev'))"],
  ["var f1 = function () { return 1 };", "function f1() { return 2 } globalThis.R = String(f1())"], ["function f1() { return 1 }", "function f1() { return 2 } globalThis.R = String(f1())"],
  ["function f1() { return 1 }", "var f1; globalThis.R = String(typeof f1)"], ["function f1() { return 1 }", "var f1 = 3; globalThis.R = String(f1)"],
  ["function f1() { return 1 }", "globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'f1').configurable)"],
  ["var v1 = 1;", "globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'v1'))"],
  ["Object.defineProperty(globalThis, 'v1', { value: 1, configurable: true, writable: true, enumerable: false });", "var v1 = 5; globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'v1'))"],
  ["Object.defineProperty(globalThis, 'v1', { value: 1, configurable: true, writable: true, enumerable: false });", "function v1() {} globalThis.R = JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'v1').enumerable) + typeof v1"],
  ["Object.defineProperty(globalThis, 'v1', { value: 1, configurable: false, writable: true, enumerable: true });", "function v1() {} globalThis.R = typeof v1"],
  ["Object.defineProperty(globalThis, 'v1', { value: 1, configurable: false, writable: true, enumerable: false });", "function v1() {} globalThis.R = typeof v1"],
  ["Object.defineProperty(globalThis, 'v1', { value: 1, configurable: false, writable: false, enumerable: true });", "function v1() {} globalThis.R = typeof v1"],
  ["Object.defineProperty(globalThis, 'v1', { get() { return 1 }, configurable: false });", "function v1() {} globalThis.R = typeof v1"],
  ["Object.defineProperty(globalThis, 'v1', { get() { return 1 }, configurable: true });", "function v1() {} globalThis.R = typeof v1 + JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'v1').writable)"],
  ["Object.defineProperty(globalThis, 'v1', { get() { return 1 }, configurable: false });", "var v1; globalThis.R = String(v1)"],
  ["Object.defineProperty(globalThis, 'v1', { get() { return 1 }, set(v) { globalThis.W = v }, configurable: false });", "var v1 = 4; globalThis.R = String(v1) + String(globalThis.W)"],
  ["Object.preventExtensions(globalThis);", "var nv = 1; globalThis.R = typeof nv"], ["Object.preventExtensions(globalThis);", "function nf() {} globalThis.R = typeof nf"],
  ["Object.preventExtensions(globalThis);", "let nl = 1; globalThis.R = String(nl)"], ["Object.preventExtensions(globalThis);", "nu = 1; globalThis.R = typeof nu"],
  ["Object.preventExtensions(globalThis);", "'use strict'; nu = 1; globalThis.R = typeof nu"], ["Object.preventExtensions(globalThis);", "globalThis.R = String((0, eval)('var nv = 1; typeof nv'))"],
  ["Object.preventExtensions(globalThis);", "var Array; globalThis.R = typeof Array"], ["Object.preventExtensions(globalThis);", "Array = 1; globalThis.R = String(Array)"],
  ["var pre1 = 1; Object.preventExtensions(globalThis);", "var pre1 = 2; globalThis.R = String(pre1)"], ["Object.seal(globalThis);", "var sv = 1; globalThis.R = typeof sv"],
  ["Object.seal(globalThis);", "delete globalThis.Array; globalThis.R = typeof Array"], ["Object.seal(globalThis);", "Array = 1; globalThis.R = String(Array)"],
  ["Object.freeze(globalThis);", "Array = 1; globalThis.R = typeof Array"], ["Object.freeze(globalThis);", "'use strict'; Array = 1; globalThis.R = typeof Array"],
  ["Object.freeze(globalThis);", "let fz = 1; globalThis.R = String(fz)"], ["Object.freeze(globalThis);", "var fz = 1; globalThis.R = typeof fz"],
  ["Object.freeze(globalThis);", "fz = 1; globalThis.R = typeof fz"], ["Object.freeze(globalThis);", "globalThis.R = String(Object.isFrozen(globalThis)) + String(Object.isSealed(globalThis))"],
  ["Object.prototype.q = 1;", "globalThis.R = String(q) + String(typeof globalThis.q) + String(globalThis.hasOwnProperty('q'))"],
  ["Object.prototype.q = 1;", "q = 2; globalThis.R = String(globalThis.hasOwnProperty('q')) + String(Object.prototype.q)"],
  ["Object.prototype.q = 1;", "'use strict'; q = 2; globalThis.R = String(globalThis.hasOwnProperty('q')) + String(Object.prototype.q)"],
  ["Object.prototype.q = 1;", "let q = 3; globalThis.R = String(q) + String(globalThis.q)"], ["Object.prototype.q = 1;", "var q; globalThis.R = String(q) + String(globalThis.hasOwnProperty('q'))"],
  ["Object.prototype.q = 1;", "var q = 4; globalThis.R = String(q) + String(Object.prototype.q)"], ["Object.prototype.q = 1;", "function q() {} globalThis.R = typeof q + String(Object.prototype.q)"],
  ["Object.prototype.q = 1;", "delete globalThis.q; globalThis.R = String(q)"], ["Object.prototype.q = 1;", "globalThis.R = typeof q + String(delete q)"],
  ["Object.defineProperty(Object.prototype, 'q', { get() { return 8 }, set(v) { globalThis.W = v }, configurable: true });", "q = 5; globalThis.R = String(globalThis.W) + String(globalThis.hasOwnProperty('q'))"],
  ["Object.defineProperty(Object.prototype, 'q', { get() { return 8 }, set(v) { globalThis.W = v }, configurable: true });", "var q = 5; globalThis.R = String(globalThis.W) + String(globalThis.hasOwnProperty('q'))"],
  ["Object.defineProperty(Object.prototype, 'q', { value: 1 });", "q = 5; globalThis.R = String(q) + String(globalThis.hasOwnProperty('q'))"],
  ["Object.defineProperty(Object.prototype, 'q', { value: 1 });", "'use strict'; q = 5; globalThis.R = String(q)"],
  ["Object.defineProperty(Object.prototype, 'q', { value: 1 });", "var q = 5; globalThis.R = String(q) + String(globalThis.hasOwnProperty('q'))"],
  ["Object.defineProperty(Object.prototype, 'q', { get() { return 8 }, configurable: true });", "q = 5; globalThis.R = String(q)"],
  ["Object.defineProperty(Object.prototype, 'q', { get() { return 8 }, configurable: true });", "'use strict'; q = 5; globalThis.R = String(q)"],
];
for (const parts of multi) {
  add(...parts.map((s, i) => (i === parts.length - 1 ? s : s)));
  add(parts.join("\n"));
  // Cada script fica sob try, para o erro de um não esconder o resultado do seguinte.
  add(...parts.map(s => `try { ${s.replace(/^'use strict';\s*/, "")} } catch (e) { globalThis.R = e.name + ': ' + e.message }`));
}
// Getter e setter no global lidos por identificador, em cada forma de acesso.
const accessor = [
  ["Object.defineProperty(globalThis, 'ga', { get() { return this === globalThis }, configurable: true });", "ga"],
  ["Object.defineProperty(globalThis, 'ga', { get() { 'use strict'; return typeof this }, configurable: true });", "ga"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return typeof this }, configurable: true });", "ga"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return this === globalThis }, configurable: true });", "(function () { return ga })()"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return this === globalThis }, configurable: true });", "(0, eval)('ga')"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return this === globalThis }, configurable: true });", "globalThis.ga"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return this === globalThis }, configurable: true });", "this.ga"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) { globalThis.W = this === globalThis }, configurable: true });", "(ga = 1, globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) { globalThis.W = typeof this }, configurable: true });", "(ga = 1, globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) { 'use strict'; globalThis.W = typeof this }, configurable: true });", "(ga = 1, globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { globalThis.C = (globalThis.C || 0) + 1; return 1 }, configurable: true });", "(ga, ga, typeof ga, globalThis.C)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { globalThis.C = (globalThis.C || 0) + 1; return 1 }, configurable: true });", "(ga += 1, globalThis.C)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { globalThis.C = (globalThis.C || 0) + 1; return 1 }, set(v) { globalThis.W = v }, configurable: true });", "(ga += 1, globalThis.C + '/' + globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { globalThis.C = (globalThis.C || 0) + 1; return 1 }, set(v) { globalThis.W = v }, configurable: true });", "(ga++, globalThis.C + '/' + globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { globalThis.C = (globalThis.C || 0) + 1; return 1 }, set(v) { globalThis.W = v }, configurable: true });", "(ga ||= 5, globalThis.C + '/' + globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { globalThis.C = (globalThis.C || 0) + 1; return 0 }, set(v) { globalThis.W = v }, configurable: true });", "(ga ||= 5, globalThis.C + '/' + globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { delete globalThis.ga; return 1 }, configurable: true });", "(ga, typeof ga)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { delete globalThis.ga; return 1 }, configurable: true });", "typeof ga + typeof ga"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) { delete globalThis.ga; globalThis.W = v }, configurable: true });", "(ga = 4, typeof ga + globalThis.W)"],
  ["Object.defineProperty(globalThis, 'ga', { get() { throw new TypeError('gx') }, configurable: true });", "typeof ga"],
  ["Object.defineProperty(globalThis, 'ga', { get() { throw new TypeError('gx') }, configurable: true });", "ga"],
  ["Object.defineProperty(globalThis, 'ga', { get() { throw new TypeError('gx') }, configurable: true });", "(function () { try { ga } catch (e) { return e.message } })()"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) { throw new TypeError('sx') }, configurable: true });", "ga = 1"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return 1 } });", "ga = 2"], ["Object.defineProperty(globalThis, 'ga', { get() { return 1 } });", "ga += 2"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return 1 } });", "ga++"], ["Object.defineProperty(globalThis, 'ga', { get() { return 1 } });", "(function () { 'use strict'; ga = 2 })()"],
  ["Object.defineProperty(globalThis, 'ga', { get() { return 1 } });", "(function () { ga = 2 })()"], ["Object.defineProperty(globalThis, 'ga', { get() { return 1 } });", "delete ga"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) {} });", "typeof ga"], ["Object.defineProperty(globalThis, 'ga', { set(v) {} });", "String(ga)"],
  ["Object.defineProperty(globalThis, 'ga', { set(v) {} });", "(ga = 3, typeof ga)"],
  ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: false, configurable: true });", "(ga = 2, ga)"], ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: false, configurable: true });", "(function () { 'use strict'; ga = 2 })()"],
  ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: false, configurable: true });", "(delete globalThis.ga, ga = 2, ga)"], ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: false, configurable: true });", "(Object.defineProperty(globalThis, 'ga', { writable: true }), ga = 2, ga)"],
  ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: true, configurable: true });", "(Object.defineProperty(globalThis, 'ga', { writable: false }), ga = 2, ga)"],
  ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: true, configurable: true });", "(Object.defineProperty(globalThis, 'ga', { get() { return 4 } }), ga)"],
  ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: true, configurable: true });", "(Object.defineProperty(globalThis, 'ga', { get() { return 4 } }), ga = 5, ga)"],
  ["Object.defineProperty(globalThis, 'ga', { value: 1, writable: true, configurable: true });", "(function () { return ga })() + (Object.defineProperty(globalThis, 'ga', { value: 3 }), (function () { return ga })())"],
  ["function rd() { return ga }", "(Object.defineProperty(globalThis, 'ga', { value: 1, configurable: true }), rd())"],
  ["function rd() { return typeof ga }", "rd() + (globalThis.ga = 1, rd()) + (delete globalThis.ga, rd())"],
  ["function rd() { ga = 1 }", "(rd(), typeof ga + globalThis.hasOwnProperty('ga'))"],
  ["function rd() { 'use strict'; ga = 1 }", "(function () { try { rd() } catch (e) { return e.name + ': ' + e.message } })()"],
  ["function rd() { return ga }", "(globalThis.ga = 1, rd() + (globalThis.ga = 2, rd()) + (delete globalThis.ga, (function () { try { return rd() } catch (e) { return e.message } })()))"],
  ["function rd() { return ga } rd2 = rd;", "(function () { try { return rd2() } catch (e) { return e.message } })()"],
  ["var rd = function () { return ga };", "(globalThis.ga = 1, rd()) + (globalThis.ga = 2, rd()) + (Object.defineProperty(globalThis, 'ga', { get() { return 'g' }, configurable: true }), rd())"],
  ["var rd = function () { return ga };", "(Object.defineProperty(globalThis, 'ga', { get() { return 'g' }, configurable: true }), rd()) + (Object.defineProperty(globalThis, 'ga', { value: 'v', configurable: true }), rd())"],
  ["var rd = function () { ga = 5 };", "(Object.defineProperty(globalThis, 'ga', { value: 'v', configurable: true, writable: true }), rd(), ga) + (Object.defineProperty(globalThis, 'ga', { value: 'v', writable: false, configurable: true }), rd(), ga)"],
  ["var rd = function () { return ga };", "(Object.defineProperty(globalThis, 'ga', { value: 'v', configurable: true }), rd()) + (globalThis.ga = 'w', rd()) + (let_ga = 1, rd())"],
  ["var rd = function () { return ga };", "(globalThis.ga = 'p', rd()) + (0, eval)('let ga = 2; rd()')"], ["var rd = function () { return ga };", "(globalThis.ga = 'p', rd()) + (0, eval)('var ga = 2; rd()')"],
];
for (const [setup, expr] of accessor) {
  add(setup, show(expr));
  add(`${setup}\n${show(expr)}`);
  add(setup, show(expr, true));
  add(setup, `let ga = 'L';`, show(expr));
  add(`let ga = 'L';`, setup, show(expr));
}

// ---- K. Script real: expressões de topo que mexem em tudo junto.
const topo = [
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = [a, b, c, typeof d, typeof e, Object.getOwnPropertyNames(globalThis).filter(function (k) { return ['a','b','c','d','e'].includes(k) }).join()].join('/')",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = Object.keys(globalThis).filter(function (k) { return ['a','b','c','d','e'].includes(k) }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return k in globalThis }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return delete globalThis[k] }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return globalThis.hasOwnProperty(k) }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return Object.getOwnPropertyDescriptor(globalThis, k) && Object.getOwnPropertyDescriptor(globalThis, k).configurable }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return Object.getOwnPropertyDescriptor(globalThis, k) && Object.getOwnPropertyDescriptor(globalThis, k).enumerable }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return (0, eval)('typeof ' + k) }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return new Function('return typeof ' + k)() }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = ['a','b','c','d','e'].map(function (k) { return typeof globalThis[k] }).join()",
  "var a = 1; let b = 2; const c = 3; function d() {} class e {}; globalThis.R = JSON.stringify(Object.getOwnPropertyNames(globalThis).slice(-0).indexOf('b') < 0)",
  "globalThis.R = [typeof a, typeof b, typeof c, typeof d, typeof e].join(); var a = 1; let b = 2; const c = 3; function d() {} class e {}",
  "try { globalThis.R = typeof b } catch (e) { globalThis.R = e.name } let b;",
  "globalThis.R = typeof d + typeof a + (function () { try { return typeof b } catch (e) { return e.name } })(); var a; let b; function d() {}",
];
// Ordem dos nomes próprios do global: o global do bun tem histórico próprio (host), então a ordem sai do contexto puro.
for (const t of topo.slice(0, 2)) addPure(t);
for (const t of topo) {
  add(t);
  add(STRICT + t);
  add("var pre;", t);
  add(`(0, eval)(${JSON.stringify(t)})`);
  add(`(function () { ${t} })()`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "global-semantics-golden-"));
const runner = path.join(dir, "runner.js");
fs.writeFileSync(
  runner,
  [
    "const vm = require('vm');",
    "const fs = require('fs');",
    "const pure = process.argv[3] === 'pure';",
    "const context = pure ? vm.createContext(new Proxy({}, { defineProperty() { return true } })) : null;",
    "const run = src => pure ? vm.runInContext(src, context, { filename: 'eval_case.js' }) : vm.runInThisContext(src, { filename: 'eval_case.js' });",
    "const scripts = fs.readFileSync(process.argv[2], 'utf8').split(" + JSON.stringify(SEP) + ");",
    "const errors = [];",
    "scripts.forEach((src, i) => {",
    "  try { run(src); }",
    "  catch (e) { errors.push(i + ':' + (e && e.name) + ': ' + (e && e.message)); if (!(e instanceof Error)) errors.push('NONERROR'); }",
    "});",
    "setTimeout(() => {",
    "  let r; try { const R = pure ? vm.runInContext('globalThis.R', context) : globalThis.R; r = R === undefined ? '<undefined>' : String(R) } catch (e) { r = 'R: ' + e.name }",
    "  process.stdout.write('\\u0001' + JSON.stringify(errors.join('|') + '#' + r) + '\\n');",
    "}, 0);",
  ].join("\n"),
);
const file = path.join(dir, "case.js");
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  fs.writeFileSync(file, body);
  const result = spawnSync(process.execPath, [runner, file, pureBodies.has(body) ? "pure" : "host"], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (result.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    continue;
  }
  const value = JSON.parse(marked.slice(1));
  if (value.includes("NONERROR") || value.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(value)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(body) + "\t" + JSON.stringify(value));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
