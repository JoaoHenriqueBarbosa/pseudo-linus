// Gera tests/golden/bytecode_eval.txt: o bytecode que o JavaScriptCore do bun 1.4.2 gera para cada
// programa, compilado como código de eval indireto (`(0, eval)(src)`). O arquivo do bun passa por
// transformação e vira módulo estrito; o eval indireto compila a string como ela é, num
// `EvalCodeBlock`, que é o que o gerador de bytecode do porte produz para o mesmo texto.
//
// Uso: bun scripts/gen-bytecode-golden.js > tests/golden/bytecode_eval.txt
//
// Normalização: endereços (`0x...`), `StructureID` e o par `[ptr/id]` das células mudam a cada
// execução e viram `?`. O resto (opcodes, operandos, offsets, metadados, identificadores,
// constantes, cabeçalho com o `CodeBlockHash`) fica como o bun imprime.
const programs = [
  "1 + 1",
  "1 + 2 * 3",
  "var x = 1; x + 1",
  "var x = 5; var y = 7; x * y",
  "var x = 3; x = x + 4; x",
  "let a = 2; const b = 3; a ** b",
  "1 < 2 ? 10 : 20",
  "-1 >>> 0",
  "~5",
  "typeof x",
  "void 0",
  "!true",
  "'a' + 'b'",
  "null ?? 3",
  "var o = {a: 1}; o.a",
  "var a = [1, 2, 3]; a[1]",
  "var i = 0; while (i < 3) i++; i",
  "var s = 0; for (var i = 0; i < 4; i++) s += i; s",
  "if (1) 2; else 3",
  "x = 1",
  "function f(a) { return a + 1 } f(2)",
  "function g(a, b) { var c = a * b; return c - 1 } g(3, 4)",
  "var h = function (n) { return n > 1 ? n * h(n - 1) : 1 }; h(5)",
  "(function () { let t = 0; for (let i = 0; i < 3; i++) t += i; return t })()",
  "var k = (a) => a * 2; k(21)",
  "function m() { return arguments.length } m(1, 2, 3)",
  "function p(o) { return o.x + o.y } p({x: 1, y: 2})",
  "try { throw 1 } catch (e) { e + 1 }",
  "switch (2) { case 1: 10; break; case 2: 20; break; default: 30 }",
  "var q = 0; do { q++ } while (q < 5); q",
  // Spread em chamada, new e array literal (call_varargs, construct_varargs).
  "function f(a, b, c) { return a + b + c } var xs = [1, 2, 3]; f(...xs)",
  "function f(a, b, c) { return a + b + c } var xs = [2, 3]; f(1, ...xs)",
  "function f() { return arguments.length } var xs = [1]; var ys = [2, 3]; f(...xs, 0, ...ys)",
  "function C(a, b) { this.s = a + b } var xs = [1, 2]; new C(...xs)",
  "function C() { this.n = arguments.length } var xs = [1, 2]; new C(0, ...xs, 3)",
  "var xs = [1, 2]; [...xs, 3]",
  "var xs = [1, 2]; var ys = [3]; [0, ...xs, ...ys, 4]",
  "var s = 'abc'; [...s]",
  "var o = {a: 1}; var p = {b: 2}; ({...o, ...p, c: 3})",
  "var m = Math; m.max(...[1, 5, 3])",
  "var o = {f(a, b) { return a - b }}; var xs = [5, 2]; o.f(...xs)",
  "var xs = [1]; (function () { return arguments.length }).apply(null, xs)",
  "var xs = [1, 2]; Math.max(...xs, ...xs)",
  "function g() { return new.target } new g(...[])",
  // Destructuring.
  "var [a, b] = [1, 2]; a + b",
  "var [a, , c] = [1, 2, 3]; c",
  "var [a, ...r] = [1, 2, 3]; r",
  "var [a = 5, b = 6] = [1]; a + b",
  "var {x, y} = {x: 1, y: 2}; x + y",
  "var {x: p, y: q = 4} = {x: 1}; p + q",
  "var {a, ...rest} = {a: 1, b: 2, c: 3}; rest",
  "var {a: {b}} = {a: {b: 7}}; b",
  "var [[a], [b]] = [[1], [2]]; a + b",
  "var a, b; [a, b] = [1, 2]; [a, b] = [b, a]; a",
  "var a, b; ({a, b} = {a: 1, b: 2}); a + b",
  "var o = {}; [o.x, o['y']] = [1, 2]; o.x + o.y",
  "var k = 'z'; var {[k]: v} = {z: 9}; v",
  "function f({a, b = 2}, [c, d] = [3, 4]) { return a + b + c + d } f({a: 1})",
  "let [a, b] = [1, 2]; const {c} = {c: 3}; a + b + c",
  "var [a, b] = 'xy'; a + b",
  "(function () { var {length} = 'abc'; return length })()",
  "for (var [k, v] of [[1, 2], [3, 4]]) k + v",
  "var f = ([a, b]) => a * b; f([3, 4])",
  // for-in, for-of, for await.
  "var o = {a: 1, b: 2}; var s = ''; for (var k in o) s += k; s",
  "var s = 0; for (var i in [5, 6, 7]) s += +i; s",
  "var o = {a: 1}; for (let k in o) { (function () { return k })() }",
  "var s = 0; for (var x of [1, 2, 3]) s += x; s",
  "var s = 0; for (let x of [1, 2, 3]) { s += x; if (s > 2) break } s",
  "var s = ''; for (const c of 'abc') { if (c == 'b') continue; s += c } s",
  "var m = new Map([[1, 2]]); var t = 0; for (var [k, v] of m) t += k + v; t",
  "var o = {}; for (o.k in {a: 1}); o.k",
  "async function f() { var s = 0; for await (var x of [1, 2]) s += x; return s } f()",
  "async function f(it) { for await (const x of it) { if (x) break } } f([1, 2])",
  "async function f() { for await (var x of (async function* () { yield 1 })()) x } f()",
  // Generators, yield e yield*.
  "function* g() { yield 1; yield 2 } var it = g(); it.next()",
  "function* g() { var x = yield 1; return x } var it = g(); it.next(); it.next(5)",
  "function* g() { yield* [1, 2] } g().next()",
  "function* g() { yield* h() } function* h() { yield 1 } g().next()",
  "function* g() { try { yield 1 } finally { yield 2 } } var it = g(); it.next(); it.return(0)",
  "function* g(n) { for (var i = 0; i < n; i++) yield i } [...g(3)]",
  "var o = {*g() { yield 1 }}; o.g().next()",
  "var g = function* () { yield this }; g().next()",
  "function* g() { const x = yield; return x } g().next()",
  "var gen = async function* () { yield 1; yield* [2] }; gen().next()",
  // Classes.
  "class A {} new A",
  "class A { constructor(x) { this.x = x } get() { return this.x } } new A(1).get()",
  "class A { x = 1; y = this.x + 1 } new A().y",
  "class A { static s = 5; static m() { return A.s } } A.m()",
  "class A { #p = 1; get() { return this.#p } } new A().get()",
  "class A { #m() { return 2 } call() { return this.#m() } } new A().call()",
  "class A { static #c = 0; static inc() { return ++A.#c } } A.inc()",
  "class A { static { A.z = 3 } } A.z",
  "class A { constructor() { this.a = 1 } } class B extends A { constructor() { super(); this.b = 2 } } new B().b",
  "class A { f() { return 1 } } class B extends A { f() { return super.f() + 1 } } new B().f()",
  "class A { static f() { return 1 } } class B extends A { static f() { return super.f() + 1 } } B.f()",
  "class A { get v() { return 1 } set v(x) { this._v = x } } var a = new A(); a.v = 2; a.v",
  "class A { #x = 1; static has(o) { return #x in o } } A.has(new A)",
  "class A { ['a' + 'b']() { return 1 } } new A().ab()",
  "var C = class N { static n() { return N.name } }; C.n()",
  "class A { x; y = 2 } new A().y",
  "class A { static async f() { return 1 } async g() { return 2 } } A.f()",
  "class A { *g() { yield 1 } } new A().g().next()",
  "class B extends Array {} new B().length",
  "class A { constructor() { return {r: 1} } } new A().r",
  "class A { static get s() { return 1 } } class B extends A {} B.s",
  // Optional chaining, nullish, logical assignment.
  "var o = {a: {b: 1}}; o?.a?.b",
  "var o = null; o?.a",
  "var o = {f() { return 1 }}; o.f?.()",
  "var o = {}; o.f?.()",
  "var o = null; o?.[0]",
  "var o = {a: null}; o.a?.b.c.d",
  "var o = {a: 1}; (o?.a)",
  "var a = null; a ?? 1",
  "var a = 0; a ?? 1",
  "var a = null, b = undefined; a ?? b ?? 3",
  "var a = null; a ??= 5; a",
  "var a = 1; a ||= 5; a",
  "var a = 1; a &&= 5; a",
  "var o = {}; o.x ??= 1; o.x ||= 2; o.x &&= 3; o.x",
  "var o = {a: null}; o['a'] ??= 7",
  "var a = null; (a ?? 1) + (a?.b ?? 2)",
  // Template literal, tagged.
  "var x = 1; `a${x}b`",
  "var x = 1, y = 2; `${x}+${y}=${x + y}`",
  "function t(s, ...v) { return s.raw.length + v.length } t`a${1}b${2}c`",
  "function t(s) { return s[0] } t`plain`",
  "var o = {t(s) { return this === o }}; o.t`x`",
  "String.raw`a\\nb${1}`",
  "function t(s) { return s } function f() { return t`x` } f() === f()",
  // Labeled break/continue.
  "var n = 0; a: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { if (j == 1) continue a; n++ } } n",
  "var n = 0; a: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { if (i == 1) break a; n++ } } n",
  "b: { 1; break b; 2 }",
  "var n = 0; a: while (true) { do { n++; if (n > 3) break a } while (true) } n",
  "a: for (var x of [1, 2]) { for (var y of [3]) { continue a } } 1",
  // try/finally com return.
  "function f() { try { return 1 } finally { 2 } } f()",
  "function f() { try { return 1 } finally { return 2 } } f()",
  "function f() { try { throw 1 } catch (e) { return e } finally { 3 } } f()",
  "function f() { for (var i = 0; i < 3; i++) { try { continue } finally { i++ } } return i } f()",
  "function f() { l: try { return 1 } finally { break l } return 2 } f()",
  "function f() { try { try { throw 1 } finally { 2 } } catch (e) { return e } } f()",
  "try { 1 } catch { 2 } finally { 3 }",
  "function f() { try { return g() } finally { h() } } function g() { return 1 } function h() {} f()",
  "try { null.x } catch ({message}) { message }",
  // switch.
  "switch ('b') { case 'a': 1; break; case 'b': 2; break; case 'c': 3; break; default: 4 }",
  "var s = 'z'; switch (s) { case 'a': 1; case 'b': 2; break; default: 9 }",
  "switch (3) { case 0: 'a'; break; case 1: 'b'; break; case 2: 'c'; break; case 3: 'd'; break; case 4: 'e' }",
  "var i = 5; switch (i) { case 1: 1; break; case 2: 2; break; case 3: 3; break; case 5: 5 }",
  "switch (1) { default: 0; case 1: 1 }",
  "var x = 7; switch (true) { case x < 5: 'lt'; break; case x < 10: 'mid'; break; default: 'hi' }",
  "switch ('a') { case 'a': let q = 1; q }",
  "function f(x) { switch (x) { case 1: return 'one'; case 2: return 'two'; case 3: return 'three'; default: return 'many' } } f(2)",
  // with, delete, typeof, in, instanceof, void.
  "var o = {a: 1}; with (o) { a + 1 }",
  "var o = {a: 1}; var b = 2; with (o) { a = b; b = a } o.a",
  "var o = {f() { return 1 }}; with (o) { f() }",
  "var o = {a: 1}; delete o.a",
  "var o = {a: 1}; delete o['a']",
  "var a = [1, 2]; delete a[0]",
  "delete 1",
  "var x = 1; delete x",
  "y = 1; delete y",
  "typeof undefinedName",
  "var f = function () {}; typeof f",
  "typeof typeof 1",
  "typeof null === 'object'",
  "var o = {a: 1}; 'a' in o",
  "1 in [1, 2]",
  "[] instanceof Array",
  "function F() {} new F() instanceof F",
  "var x = 1; typeof x == 'number' && x > 0",
  // Getters, setters, computed keys, literais.
  "var o = {get a() { return 1 }}; o.a",
  "var o = {set a(v) { this._a = v }}; o.a = 2; o._a",
  "var o = {get a() { return 1 }, set a(v) {}}; o.a",
  "var k = 'x'; var o = {[k]: 1, [k + 'y']: 2}; o.xy",
  "var o = {['a']: 1, b: 2, 3: 4}; o[3]",
  "var a = 1, b = 2; ({a, b})",
  "var o = {f() { return 1 }, g: function () { return 2 }, h: () => 3}; o.f() + o.g() + o.h()",
  "var o = {__proto__: null}; Object.getPrototypeOf(o)",
  "var o = {get [1 + 1]() { return 3 }}; o[2]",
  "var o = {async f() { return 1 }, *g() { yield 1 }}; o.f()",
  "var o = {a: 1, a: 2}; o.a",
  "[1, , 3]",
  "[,]",
  "var o = {'a-b': 1, 2: 3, [Symbol.iterator]: 4}; o['a-b']",
  // Closures, arguments, rest, default params, arrow this.
  "function mk() { var c = 0; return function () { return ++c } } var f = mk(); f(); f()",
  "function mk() { var fs = []; for (let i = 0; i < 3; i++) fs.push(() => i); return fs[1]() } mk()",
  "function mk() { var fs = []; for (var i = 0; i < 3; i++) fs.push(() => i); return fs[1]() } mk()",
  "function a() { var x = 1; function b() { function c() { return x } return c() } return b() } a()",
  "function f() { return arguments[0] } f(7)",
  "function f(a) { arguments[0] = 2; return a } f(1)",
  "function f(a) { 'use strict'; arguments[0] = 2; return a } f(1)",
  "function f(a, b) { return arguments.length + a } f(1, 2, 3)",
  "function f() { return () => arguments[0] } f(4)()",
  "function f(a, ...r) { return r.length } f(1, 2, 3)",
  "function f(...r) { return r } f()",
  "function f(a = 1, b = a + 1) { return a + b } f()",
  "function f(a, b = () => a) { var a = 2; return b() } f(1)",
  "function f(a = arguments.length) { return a } f()",
  "var f = (a, b = 2, ...c) => a + b + c.length; f(1)",
  "function O() { this.v = 1; this.f = () => this.v } new O().f()",
  "var o = {v: 1, f() { return (() => this.v)() }}; o.f()",
  "var o = {v: 1, f() { return [1].map(() => this.v) }}; o.f()",
  "var f = () => () => () => 1; f()()()",
  "var x = 1; (function () { var x = 2; return (() => x)() })()",
  "(function () { 'use strict'; return this })()",
  "(function () { return this })()",
  "function f() { var self = this; return function () { return self } } f.call(1)()",
  "function f(a, b) { 'use strict'; return f.length } f()",
  // eval direto.
  "function f() { return eval('1 + 1') } f()",
  "function f() { var x = 1; return eval('x + 1') } f()",
  "function f() { eval('var y = 2'); return y } f()",
  "function f() { 'use strict'; eval('var y = 2'); return typeof y } f()",
  "function f(a) { return eval('a') } f(3)",
  "function f() { return () => eval('this') } f()()",
  "var x = 1; eval('x')",
  "eval('var z = 1'); z",
  // async/await.
  "async function f() { return 1 } f()",
  "async function f() { var x = await 1; return x + 1 } f()",
  "async function f(p) { try { await p } catch (e) { return e } } f(Promise.reject(1))",
  "async function f() { var a = await 1, b = await 2; return a + b } f()",
  "var f = async () => { await null; return 1 }; f()",
  "async function f() { for (var i = 0; i < 2; i++) await i } f()",
  "async function f() { return await g() } async function g() { return 1 } f()",
  "var o = {async f() { return await this }}; o.f()",
  "async function f() { return [await 1, await 2] } f()",
  "async function f() { try { return await 1 } finally { await 2 } } f()",
  // BigInt e exponent.
  "1n + 2n",
  "10n ** 3n",
  "var a = 5n; a * a",
  "var a = 5n; -a",
  "var a = 7n; a % 3n",
  "var a = 1n; a++; a",
  "typeof 1n",
  "0x10n",
  "1n < 2",
  "2 ** 10",
  "var a = 2; a **= 3; a",
  "2 ** 3 ** 2",
  "(-2) ** 2",
  "var a = 2, b = 3; a ** b ** 2",
  // Variados.
  "var a = 1, b = 2; a, b",
  "var a = 1; a++ + ++a",
  "var o = {a: 1}; o.a += 2; o.a",
  "var a = [1, 2]; a[0] += a[1]; a",
  "var o = {n: 1}; o.n++; --o.n",
  "var i = 0; var r = []; while (i < 3) { r[i] = i * i; i++ } r",
  "var x = 3; x > 2 && x < 5 || x == 0",
  "var o = {}; o.a = o.b = 1; o.a",
  "var s = 'abc'; s.length + s[1]",
  "new Date(0).getTime()",
  "new Array(3).length",
  "var o = {toString() { return 'x' }}; o + ''",
  "if (typeof require === 'undefined') 1; else 2",
  "var f = function g() { return typeof g }; f()",
  "var n = 0; for (;;) { if (++n > 2) break } n",
  "var a = 1; { let a = 2; { let a = 3 } } a",
  "let x = 1; { x = 2 } x",
  "const c = 1; (function () { return c })()",
  "var f = function () { return new.target }; f()",
  "debugger; 1",
  "function f() {} f.prototype.m = function () { return 1 }; new f().m()",
  "var o = Object.create(null); o.a = 1; o.a",
  "(1, 2, 3)",
  "var a = [3, 1, 2]; a.sort((x, y) => x - y)",
  "var re = /a(b)/g; re.test('ab')",
  "var x; x = x || 1; x",
  "function f() { return; } f()",
  "var x = 1; x = x ? x : 0",
  // Tamanho de frame (wip/notes/stack-limits.md): callee registers e argv de cada forma de chamada.
  "function f(n) { return n ? 1 + f(n - 1) : 0 } f(1)",
  "function f(n, a, b, c) { return n ? 1 + f(n - 1, a, b, c) : 0 } f(1, 1, 2, 3)",
  "function f(n) { let a = 1, b = 2, c = 3, d = 4, e = 5, g = 6, h = 7, i = 8, j = 9, k = 10; return n ? 1 + f(n - 1) : a + b + c + d + e + g + h + i + j + k } f(1)",
  "function F(n) { this.x = n ? new F(n - 1) : 0 } new F(1)",
  "function f(n) { return n ? 1 + f.call(null, n - 1) : 0 } f(1)",
];

const { spawnSync } = require("child_process");
const { mkdtempSync, writeFileSync, rmSync } = require("fs");
const { tmpdir } = require("os");
const { join } = require("path");

const dir = mkdtempSync(join(tmpdir(), "zjsc-bc-"));
const header = /^\S.*#[A-Za-z0-9]{6}:\[0x/;

function normalize(line) {
  return line
    .replace(/0x[0-9a-f]+/g, "0x?")
    .replace(/StructureID: \d+/g, "StructureID: ?")
    .replace(/\[0x\?\/\d+/g, "[0x?/?");
}

function evalBlock(source) {
  const file = join(dir, "p.js");
  writeFileSync(file, `(0, eval)(${JSON.stringify(source)});\n`);
  const run = spawnSync(process.execPath, [file], {
    env: { ...process.env, BUN_JSC_dumpGeneratedBytecodes: "1" },
    encoding: "utf8",
  });
  const lines = (run.stdout + run.stderr).split("\n");
  const start = lines.findIndex((line) => line.startsWith("<eval>#"));
  if (start < 0) throw new Error(`sem bloco <eval> para ${source}`);
  // O bloco do eval e os das funções que ele chama (compiladas na primeira chamada), até o
  // primeiro bloco do próprio bun (`bun:main`, que roda depois do arquivo).
  let end = start + 1;
  while (end < lines.length && !(header.test(lines[end]) && lines[end].startsWith("bun:"))) end++;
  while (end > start && lines[end - 1].trim() === "") end--;
  return lines.slice(start, end).map(normalize).join("\n");
}

try {
  for (const source of programs) {
    console.log(`=== ${source}`);
    console.log(evalBlock(source));
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}
