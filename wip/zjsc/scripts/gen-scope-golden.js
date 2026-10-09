// Gera tests/golden/scope_bun.tsv: escopo e closures (TDZ, bindings por iteração, parâmetros default, named function
// expression, class binding, catch, hoisting em blocos, redeclarações, eval, delete, with, generators, async, this
// lexical, private names, closures e listas de parâmetros grandes, identificadores unicode) medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança); SyntaxError sai por eval indireto.
// Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Uso: bun scripts/gen-scope-golden.js > tests/golden/scope_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
// Programa com captura de exceção; o corpo atribui R.
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Fonte compilada por eval indireto (escopo global), para os SyntaxError.
const S = src => add(`try { (0, eval)(${JSON.stringify(src)}); R = 'ok' } catch (e) { R = e.name + ': ' + e.message }`);
// Fonte compilada por eval indireto que devolve um valor.
const V = src => add(`try { R = String((0, eval)(${JSON.stringify(src)})) } catch (e) { R = e.name + ': ' + e.message }`);
const J = "JSON.stringify";

// ---- TDZ: forma de declaração x forma de acesso.
const decls = [
  ["let x = 1;", "let"],
  ["const x = 1;", "const"],
  ["class x {}", "class"],
];
const accesses = [
  "R = x",
  "R = typeof x",
  "x = 2; R = 'assigned'",
  "x++; R = 'inc'",
  "R = (() => x)()",
  "R = (function () { return typeof x })()",
  "R = [x]",
  "R = { x }",
  "R = `${x}`",
  "R = x?.y",
  "R = delete x",
  "R = x in {}",
  "R = void x",
  "R = (0, x)",
  "R = (x, 1)",
  "R = !x",
  "var { a = x } = {}; R = a",
  "var [a = x] = []; R = a",
  "for (var i of [x]) R = i",
  "switch (x) { default: R = 'sw' }",
  "R = x ? 1 : 2",
  "R = eval('x')",
  "R = eval('typeof x')",
  "R = (() => { try { return x } catch (e) { return e.name } })()",
];
for (const [decl] of decls) for (const access of accesses) T(`{ ${access}; ${decl} }`);
for (const [decl] of decls) for (const access of accesses.slice(0, 6)) T(`(function () { ${access}; ${decl} })()`);
for (const [decl] of decls) T(`{ ${decl} R = typeof x }`);
T("{ R = typeof x; let x }");
T("{ R = typeof undeclaredVar }");
T("{ let x = x; R = x }");
T("{ const x = x + 1; R = x }");
T("{ let x = (() => x)(); R = x }");
T("{ let f = () => x; let x = 1; R = f() }");
T("{ let f = () => x; try { f() } catch (e) { R = e.message } let x = 1 }");
T("{ class C extends C {} }");
T("{ class C { static s = C.name } R = C.s }");
T("{ class C { static s = D; } class D {} }");
T("{ let c = class C { static s = C.name }; R = c.s }");
T("{ let c = class C extends (R = typeof C, Object) {}; }");
T("{ class C { [C] = 1 } }");
T("{ class C { static [C.name] = 1 } }");
T("{ class C { [(C, 'k')]() {} } }");
// parâmetros default com referência cruzada
T("function f(a = b, b) { return a } R = f()");
T("function f(a = b, b) { return a } R = f(1)");
T("function f(a = b, b) { return a } R = f(undefined, 2)");
T("function f(a, b = a) { return b } R = f(3)");
T("function f(a = a) { return a } R = f()");
T("function f(a = a) { return a } R = f(5)");
T("function f(a = () => b, b = 2) { return a() } R = f()");
T("function f(a = typeof b, b) { return a } R = f()");
T("function f(a = b, b = a) { return a } R = f()");
T("function f(a = (() => b)(), b) { return a } R = f()");
T("function f({ a = b }, b) { return a } R = f({})");
T("function f([a = b], b) { return a } R = f([])");
T("function f(a = eval('b'), b) { return a } R = f()");
T("var f = (a = b, b) => a; R = f()");
T("var f = (a = b, b) => a; R = f(1)");
T("var f = (a, b = a) => b; R = f(9)");
T("class K { m(a = b, b) { return a } } R = new K().m()");
T("class K { constructor(a = b, b) { this.a = a } } R = new K().a");
T("var o = { m(a = b, b) { return a } }; R = o.m()");
T("function* g(a = b, b) { yield a } R = g().next().value");
T("async function g(a = b, b) { return a } g().catch(e => { R = e.name + ': ' + e.message })");
T("function f(a = this) { return a } R = typeof f()");
T("function f(a = arguments.length) { return a } R = f(undefined, 1, 2)");
T("function f(a = arguments) { return a[1] } R = f(undefined, 7)");
T("function f(a, b = arguments[0]) { return b } R = f(4)");
// TDZ em loops e switch
T("for (let i = 0; i < 2; i++) { try { y } catch (e) { R = e.name } let y = 1 }");
T("for (let i = i; false;) {}");
T("for (let i = 0, j = i; i < 1; i++) R = j");
T("for (let i = j, j = 0; false;) {}");
T("for (let x of [x]) {}");
T("for (let x in { a: x }) {}");
T("for (const x of [1].map(() => x)) {}");
T("for (let x of (() => x)()) {}");
T("for (let x of ((R = typeof x), [])) {}");
T("for (let x in ((R = 'a'), {})) {}; R = R");
T("switch (1) { case 0: let s = 1; case 1: R = typeof s; }");
T("switch (1) { case 0: let s = 1; case 1: R = s; }");
T("switch (1) { case 1: let s = 1; R = s; break; case 2: s = 2 }");
T("switch (1) { case 0: let s; break; default: s = 1 }");
T("switch (1) { default: R = (() => s)(); case 3: let s = 3 }");
T("switch (0) { case 0: let s = 1; break; case 1: R = 'n' } R = R || 'u'");
T("switch (2) { case 1: const c = 1; case 2: R = c }");
T("switch (2) { case 1: class C {} case 2: R = typeof C }");
T("switch (1) { case 1: function sf() { return 1 } } R = typeof sf");
T("var { a = b, b } = { }; R = a");
T("let { a = b, b = 1 } = {}; R = a");
T("let [a = b, b = 1] = []; R = a");
T("let { a = 1, b = a } = {}; R = b");
T("let [a, b = a] = [3]; R = b");
T("let { a, ...rest } = { a, b: 1 }; R = a");
T("const { x = y, y = 2 } = {}; R = x");
T("let a = [a] = [1]; R = a");
T("let { [k]: v } = { }; let k = 'a'");
T("{ let k = 'a'; let { [k]: v } = { a: 5 }; R = v }");
T("{ let { [k]: v } = { a: 5 }; let k = 'a'; R = v }");
T("(function () { for (const [a = b, b] of [[]]) R = a })()");
T("try { throw 1 } catch ({ a = b, b }) { R = a }");
T("try { throw {} } catch ({ a = b, b }) { R = a }");
T("try { throw {} } catch ({ a = 1, b = a }) { R = b }");
T("let tdz1 = 1; { R = typeof tdz1; let tdz1 = 2 }");
T("let outer = 'o'; { try { R = outer } catch (e) { R = e.name } let outer = 'i' }");
T("function g1() { return v1 } try { g1() } catch (e) { R = e.name } let v1 = 1; R = R + g1()");
T("var f = () => w; let w = 1; R = f()");
T("typeof gl; let gl = 1; R = gl");

// ---- Closures capturando var/let em loops.
const loops = [
  ["for (let i = 0; i < 3; i++)", "i"],
  ["for (var i = 0; i < 3; i++)", "i"],
  ["for (const i of [0, 1, 2])", "i"],
  ["for (let i of [0, 1, 2])", "i"],
  ["for (var i of [0, 1, 2])", "i"],
  ["for (let i in { a: 0, b: 0, c: 0 })", "i"],
  ["for (const i in { a: 0, b: 0, c: 0 })", "i"],
  ["for (var i in { a: 0, b: 0, c: 0 })", "i"],
  ["for (let [i] of [[0], [1], [2]])", "i"],
  ["for (let { i } of [{ i: 0 }, { i: 1 }, { i: 2 }])", "i"],
  ["for (let i = 0, j = 10; i < 3; i++, j--)", "i + ':' + 0"],
  ["for (let i = 0; i < 3; i++)", "i"],
];
const captures = [
  "fs.push(() => V)",
  "fs.push(function () { return V })",
  "fs.push(() => () => V)",
  "fs.push({ get g() { return V } }.g === undefined ? null : () => V)",
  "fs.push(((v) => () => v + V)(1))",
  "{ fs.push(() => V) }",
  "if (true) fs.push(() => V)",
  "try { fs.push(() => V) } finally { }",
  "fs.push(class { static m() { return V } }.m)",
  "fs.push(() => eval('V'))",
  "fs.push(async () => V)",
];
for (const [head, v] of loops) {
  for (const cap of captures) {
    const body = cap.split("V").join(v);
    T(`var fs = []; ${head} ${body}; R = ${J}(fs.map(f => { var r = f(); return typeof r === 'function' ? r() : (r instanceof Promise ? 'p' : r) }))`);
  }
}
// mutação no corpo e closures na condição/incremento
T("var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i); i++ } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i++) } R = fs.map(f => f()).join() + '|' + fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; fs.push(() => i), i++) {} R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; fs.push(() => i), i < 3; i++) {} R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; (fs.push(() => i), i < 3); i++) {} R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i); i += 0 } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0, f = () => i; i < 3; i++) { fs.push(f) } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0, f = () => i; i < 3; i++) { fs.push(f); i = 5 } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { let j = i * 2; fs.push(() => j) } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i); continue } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 5; i++) { fs.push(() => i); if (i == 2) break } R = fs.map(f => f()).join()");
T("var fs = []; o: for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { fs.push(() => i * 10 + j); if (j == 1) continue o } } R = fs.map(f => f()).join()");
T("var fs = []; for (let i of [1, 2, 3]) { fs.push(() => i); i *= 2 } R = fs.map(f => f()).join()");
T("var fs = []; for (const k in { a: 1, b: 2 }) { fs.push(() => k) } R = fs.map(f => f()).join()");
T("var fs = []; var i; for (i = 0; i < 3; i++) fs.push(() => i); R = fs.map(f => f()).join()");
T("var fs = []; for (var i = 0; i < 3; i++) { let j = i; fs.push(() => j) } R = fs.map(f => f()).join()");
T("var fs = []; for (var i = 0; i < 3; i++) (function (j) { fs.push(() => j) })(i); R = fs.map(f => f()).join()");
T("var fs = []; let i = 0; while (i < 3) { let j = i; fs.push(() => j); i++ } R = fs.map(f => f()).join()");
T("var fs = []; let i = 0; do { let j = i; fs.push(() => j + i) } while (++i < 3); R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 2; i++) for (let j = 0; j < 2; j++) fs.push(() => [i, j]); R = " + J + "(fs.map(f => f()))");
T("var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i); var i2 = i } R = fs.map(f => f()).join() + i2");
T("var fs = []; for (let [a, b] = [0, 1]; a < 3; [a, b] = [b, a + b]) fs.push(() => a + ':' + b); R = fs.map(f => f()).join()");
T("var fs = []; for (let x of [1, 2]) { fs.push(() => x); { let x = 9; fs.push(() => x) } } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { setTimeout(() => fs.push(i)); } setTimeout(() => { R = fs.join() }, 1)");
T("var gen = function* () { for (let i = 0; i < 3; i++) yield () => i }; R = [...gen()].map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { switch (i) { case 1: let z = i; fs.push(() => z); break; default: fs.push(() => -i) } } R = fs.map(f => f()).join()");
T("var fs = []; for (var v of [1, 2]) { fs.push(() => v) } R = fs.map(f => f()).join()");
T("var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i) } i = 'g'; R = fs.map(f => f()).join() + typeof i");
T("let a = []; for (let i = 0; i < 2; i++) { a[i] = () => i } R = a[0]() + a[1]()");
T("var r = []; for (let i = 0; i < 3; i++) { r.push(i); let i2 = i } R = r.join()");
T("for (let i = 0; i < 1; i++) { let i = 'inner'; R = i }");
T("for (let i = 0; i < 1; i++) { var i = 1 }");
T("for (let i of [1]) { var i = 1 }");
T("for (var i of [1]) { let i = 'in'; R = i }");
T("var fs = []; for (let i = 0; i < 2; i++) { fs.push(() => this === globalThis) } R = fs[0]()");

// ---- Shadowing de parâmetros e arguments.
T("function f(a) { var a; return a } R = f(1)");
T("function f(a) { var a = 2; return a } R = f(1)");
T("function f(a) { function a() {} return typeof a } R = f(1)");
T("function f(a) { { function a() {} } return typeof a } R = f(1)");
T("function f(a) { let b = a; { let a = 2; b += a } return b } R = f(1)");
T("function f(a) { arguments[0] = 9; return a } R = f(1)");
T("function f(a) { 'use strict'; arguments[0] = 9; return a } R = f(1)");
T("function f(a = 0) { arguments[0] = 9; return a } R = f(1)");
T("function f(a) { a = 9; return arguments[0] } R = f(1)");
T("function f(a) { a = 9; return arguments[0] } R = f()");
T("function f(a) { 'use strict'; a = 9; return arguments[0] } R = f(1)");
T("function f(a, a) { return a } R = f(1, 2)");
T("function f(a, a) { 'use strict' }");
S("function f(a, a) { 'use strict' }");
S("'use strict'; function f(a, a) {}");
S("function f(a, a = 1) {}");
S("function f(a, [a]) {}");
S("(a, a) => 1");
S("function f(a) { let a }");
S("function f(a = 1) { let a }");
S("function f([a]) { let a }");
S("function f(...a) { let a }");
S("function f(a) { const a = 1 }");
S("function f(a) { class a {} }");
T("function f(a) { var arguments; return typeof arguments } R = f(1)");
T("function f(a) { var arguments = 3; return arguments } R = f(1)");
T("function f(arguments) { return arguments } R = f(5)");
T("function f() { function arguments() {} return typeof arguments } R = f()");
T("function f() { let arguments = 1; return arguments } R = f(5)");
T("function f(a = arguments) { var arguments; return typeof arguments } R = f()");
T("function f(a = arguments) { var arguments = 1; return [typeof a, arguments] } R = " + J + "(f())");
T("function f() { return typeof arguments } R = f()");
T("var f = () => typeof arguments; R = f()");
T("function f() { return (() => arguments[0])() } R = f(7)");
T("function f() { return (() => (() => arguments.length)())() } R = f(7, 8)");
T("function f() { var g = () => { var arguments = 2; return arguments }; return [g(), arguments[0]] } R = " + J + "(f(1))");
T("function f() { var g = function () { return arguments[0] }; return g(2) } R = f(1)");
T("function f(a, b) { return arguments.length } R = f(1)");
T("function f(a, b) { return f.length } R = f(1)");
T("function f(...r) { return arguments.length + ':' + r.length } R = f(1, 2)");
T("function f(a, ...r) { arguments[0] = 5; return a } R = f(1, 2)");
T("function f(a, b) { arguments.length = 1; return [arguments[1], b] } R = " + J + "(f(1, 2))");
T("function f(a) { return Object.prototype.toString.call(arguments) } R = f()");
T("function f(a) { 'use strict'; return arguments.callee } R = f()");
T("function f(a) { return arguments.callee === f } R = f()");
T("function f(a) { 'use strict'; return Object.getOwnPropertyNames(arguments).join() } R = f(1)");
T("function f(a) { return Object.getOwnPropertyNames(arguments).join() } R = f(1)");
T("function f(a) { delete arguments[0]; arguments[0] = 3; return a } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { value: 5 }); return a } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { writable: false }); a = 2; return arguments[0] } R = f(1)");
T("function f(a) { Object.defineProperty(arguments, '0', { writable: false, value: 3 }); return a } R = f(1)");
T("function f(a) { return eval('arguments[0]') } R = f(8)");
T("function f(a) { return eval('var arguments = 5; arguments') } R = f(8)");
T("function f(a) { eval('var a = 5'); return a } R = f(8)");
T("function f(a) { eval('var b = 5'); return b } R = f(8)");
T("function f(a) { 'use strict'; eval('var b = 5'); return typeof b } R = f(8)");
T("function f(a) { 'use strict'; eval('var a = 5'); return a } R = f(8)");

// ---- Parâmetros default com escopo separado do corpo.
T("var x = 'outer'; function f(a = () => x) { var x = 'inner'; return a() } R = f()");
T("var x = 'outer'; function f(a = () => x) { let x = 'inner'; return a() } R = f()");
T("var x = 'outer'; function f(a = () => x) { x = 'set'; var x; return a() } R = f()");
T("var x = 'outer'; function f(a = () => x, b = 1) { var x; return [a(), typeof x] } R = " + J + "(f())");
T("function f(a, b = () => a) { var a = 'body'; return b() } R = f('param')");
T("function f(a, b = () => a) { var a; return [a, b()] } R = " + J + "(f('param'))");
T("function f(a, b = () => a) { a = 'set'; return b() } R = f('param')");
T("function f(a, b = () => a) { var a = 'set'; return a + b() } R = f('param')");
T("function f(a = 1, b = () => a) { var a = 2; return [a, b()] } R = " + J + "(f())");
T("function f(a = 1) { var a; return a } R = f()");
T("function f(a = 1) { var a = a + 1; return a } R = f()");
T("function f(a = 1) { function a() {} return typeof a } R = f()");
T("function f(a, g = () => a) { function a() {} return [typeof a, typeof g()] } R = " + J + "(f(1))");
T("function f(g = () => v) { var v = 1; return g() } R = f()");
T("var v = 'out'; function f(g = () => v) { var v = 1; return g() } R = f()");
T("function f(g = () => typeof h) { function h() {} return g() } R = f()");
T("function f(a = eval('var z = 1; z'), b = typeof z) { return b } R = f()");
T("function f(a = eval('var z = 1'), b = z) { return b } R = f()");
T("function f(a = eval('var z = 1')) { return typeof z } R = f()");
T("function f(a = eval('var a2 = 1')) { var a2 = 2; return a2 } R = f()");
T("function f(a = eval('var a = 1')) { return a } R = f()");
T("function f(a, b = eval('var a = 1')) { return a } R = f(0)");
T("var f = (a = 1, g = () => a) => { var a = 2; return [a, g()] }; R = " + J + "(f())");
T("function f(a = 1) { { var a = 2 } return a } R = f()");
T("function f(x = 1) { return (() => { var x = 2; return x })() + x } R = f()");
T("var o = { f(a = () => z) { var z = 1; return a } }; try { o.f()() } catch (e) { R = e.name }");
T("class K { m(a = () => z) { let z = 1; return a } } try { new K().m()() } catch (e) { R = e.name }");
T("var z = 1; function f(a = () => z) { let z = 2; return a() } R = f()");
T("function f(a, b = a) { a = 2; return [a, b] } R = " + J + "(f(1))");
T("function f(a = 1, b = a++) { return [a, b] } R = " + J + "(f())");
T("function f(a = (b = 3, 1), b) { return [a, b] } R = " + J + "(f())");
T("function f(a = b = 3, b) {} try { f() } catch (e) { R = e.name + ': ' + e.message }");
T("function f(a = () => b, b = 2) { var b; return [a(), b] } R = " + J + "(f())");
T("function f(a = () => b, b = 2) { var b = 3; return [a(), b] } R = " + J + "(f())");
T("function f(a = 1) { return typeof f } R = f()");
T("function f(f = 1) { return f } R = f()");
T("function f(a = () => f) { var f = 1; return a() === f } R = f()");

// ---- Named function expression.
T("var f = function g() { g = 1; return typeof g }; R = f()");
T("var f = function g() { 'use strict'; g = 1 }; f()");
T("var f = function g() { 'use strict'; g++ }; f()");
T("var f = function g() { 'use strict'; g += 1 }; f()");
T("var f = function g() { 'use strict'; ({ g } = { g: 1 }) }; f()");
T("var f = function g() { 'use strict'; [g] = [1] }; f()");
T("var f = function g() { 'use strict'; for (g of [1]) {} }; f()");
T("var f = function g() { 'use strict'; for (g in { a: 1 }) {} }; f()");
T("var f = function g() { 'use strict'; delete g; return 'ok' }; R = f()");
T("var f = function g() { var g = 1; return g }; R = f()");
T("var f = function g() { var g; return typeof g }; R = f()");
T("var f = function g() { let g = 1; return g }; R = f()");
T("var f = function g() { function g() {} return typeof g }; R = f()");
T("var f = function g(g) { return g }; R = f(2)");
T("var f = function g(a = g) { return typeof a }; R = f()");
T("var f = function g() { return g }; R = f() === f");
T("var f = function g() { return typeof g }; var h = f; f = null; R = h()");
T("var f = function g() { g = 1; return g === f }; R = f()");
T("var f = function g() { return eval('g = 1; typeof g') }; R = f()");
T("var f = function g() { 'use strict'; return eval('g = 1') }; f()");
T("var f = function g() { return (() => { g = 1; return typeof g })() }; R = f()");
T("var f = function g() { return (() => { 'use strict'; g = 1; return typeof g })() }; f()");
T("var f = function g() { with ({ g: 5 }) { return g } }; R = f()");
T("var f = function g() { with ({}) { g = 1; return typeof g } }; R = f()");
T("var f = function g() { try { throw 1 } catch (g) { return g } }; R = f()");
T("var f = function g() { 'use strict'; try { throw 1 } catch (g) { g = 2; return g } }; R = f()");
T("var f = function* g() { g = 1; return typeof g }; R = f().next().value + ''");
T("var f = function* g() { 'use strict'; g = 1 }; f().next()");
T("var f = async function g() { 'use strict'; g = 1 }; f().catch(e => { R = e.name + ': ' + e.message })");
T("var f = async function g() { g = 1; return typeof g }; f().then(v => { R = v })");
T("var f = class g { static m() { g = 1 } }; f.m()");
T("var f = class g { static m() { return typeof g } }; R = f.m()");
T("var f = class g { static m() { var g = 2; return g } }; R = f.m()");
T("var f = class g { static m() { let g = 2; return g } }; R = f.m()");
T("var f = class g { m() { g = 1 } }; new f().m()");
T("var f = class g { static x = (() => { g = 1 })() }");
T("class g2 { static m() { g2 = 1 } } g2.m()");
T("class g2 { static m() { g2 = 1 } } try { g2.m() } catch (e) { R = e.message } R = typeof g2");
T("class g3 { static m() { return g3 } } var h3 = g3; g3 = 1; R = h3.m() === h3");
T("class g4 { static m() { return g4 } } let h4 = g4; try { g4 = 1 } catch (e) { R = e.message }");
T("const c5 = class { static m() { return c5 } }; R = c5.m() === c5");
T("var c6 = class { static m() { return c6 } }; var d6 = c6; c6 = 1; R = typeof d6.m()");
T("var c7 = class C7 { static m() { return C7 } }; R = typeof C7");
T("var c8 = class C8 { static m() { return () => C8 } }; R = c8.m()() === c8");
T("class C9 { static m() { eval('C9 = 1') } } C9.m()");
T("class C10 { static m() { return (() => { C10 = 1 })() } } C10.m()");
T("class C11 { static { C11 = 1 } }");
T("class C12 { static x = C12 }; R = typeof C12.x");
T("class C13 { x = C13; } R = typeof new C13().x");
T("class C14 { static x = () => C14 } var o14 = C14; C14 = 1; R = typeof o14.x()");
T("class C15 { m() { return typeof C15 } } var o15 = new C15(); var C15b = C15; R = o15.m()");
T("class C16 { [(() => typeof C16)()]() {} }");
T("class C17 { static [(R = 'k', 'a')] = 1 } R = R + C17.a");
T("class C18 extends (class { static p() { return 1 } }) { static q() { return super.p() } } R = C18.q()");
T("class C19 { constructor() { C19 = 1 } } new C19()");
T("var f = function g() { return g.name }; R = f()");
T("var f = function () { return typeof f }; var h = f; f = 1; R = h()");

// ---- catch (e) e Annex B.
T("try { throw 1 } catch (e) { var e = 2; R = e } R = R + ':' + e");
T("try { throw 1 } catch (e) { var e = 2 } R = typeof e");
T("try { throw 1 } catch (e) { var e; R = e } R = R + typeof e");
T("try { throw 1 } catch (e) { { var e = 3 } R = e }");
T("try { throw 1 } catch (e) { for (var e = 5; false;); R = e }");
T("try { throw 1 } catch (e) { for (var e of [7]); R = e }");
S("try { throw 1 } catch (e) { for (var e of [7]); }");
S("try { throw 1 } catch (e) { let e }");
S("try { throw 1 } catch (e) { const e = 1 }");
S("try { throw 1 } catch (e) { class e {} }");
S("try { throw 1 } catch (e) { function e() {} }");
S("try { throw 1 } catch ([e]) { var e }");
S("try { throw 1 } catch ({ e }) { var e }");
S("try { throw 1 } catch ([e, e]) { }");
S("try { throw 1 } catch (e) { { let e } }");
S("try { throw 1 } catch (e) { { var e } }");
S("try { throw 1 } catch (e) { for (var e in {}); }");
S("try { throw 1 } catch (e) { for (var e;;) break }");
S("try { } catch (e) { var e; }");
S("try { } catch { let e }");
S("try { } catch ({}) { }");
T("try { throw 1 } catch (e) { function e2() {} R = typeof e2 }");
T("var e = 'outer'; try { throw 1 } catch (e) { e = 2 } R = e");
T("var e = 'outer'; try { throw 1 } catch (e) { var e = 2 } R = e");
T("try { throw 1 } catch (e) { eval('var e = 4'); R = e } R = R + typeof e");
T("try { throw 1 } catch (e) { eval('var q = e'); } R = q");
T("try { throw 1 } catch (e) { (() => { e = 9 })(); R = e }");
T("var fs = []; for (var i = 0; i < 2; i++) { try { throw i } catch (e) { fs.push(() => e) } } R = fs.map(f => f()).join()");
T("try { throw 1 } catch (e) { try { throw 2 } catch (e) { R = e } R = R + ':' + e }");
T("try { throw 1 } catch ({ message = 'd' }) { R = message }");
T("try { throw new Error('m') } catch ({ message }) { R = message }");
T("try { throw null } catch ({ message }) { R = message }");
T("try { throw undefined } catch ([a]) { R = a }");
T("try { throw [1, 2] } catch ([a, b = a]) { R = a + b }");
T("try { throw 1 } catch { R = typeof e }");
T("try { throw 1 } catch (e) { R = typeof e } R = R + typeof e");
T("try { throw 1 } catch (e) { var e2 = () => e; } R = e2()");
T("try { throw 1 } catch (arguments) { R = arguments }");
T("'use strict'; try { throw 1 } catch (eval) { R = eval }");
S("'use strict'; try { throw 1 } catch (eval) { }");
S("'use strict'; try { throw 1 } catch (arguments) { }");
S("try { throw 1 } catch (e) { var e = 2 } let e");
T("try { throw 1 } catch (e) { with ({}) { var e = 6 } R = e }");
T("function f() { try { throw 1 } catch (e) { return () => e } } R = f()()");
T("function f() { try { throw 1 } catch (e) { var e = 3; return e } } R = f()");
T("function f() { try { throw 1 } catch (e) { var e = 3 } return e } R = f()");
T("function f() { try { throw 1 } catch (e) { var e = 3 } return typeof e } R = f()");
T("function f() { try { throw 1 } catch (e) { var g = () => e; var e = 3; return [g(), e] } } R = " + J + "(f())");

// ---- Hoisting de function em blocos (Annex B).
const hoistBodies = [
  "R = typeof f; { function f() {} }",
  "{ function f() {} } R = typeof f",
  "R = typeof f; if (true) function f() {}",
  "R = typeof f; if (false) function f() {}",
  "if (true) function f() {} R = typeof f",
  "{ R = typeof f; function f() {} }",
  "{ function f() { return 1 } } { function f() { return 2 } } R = f()",
  "{ function f() { return 1 } function f() { return 2 } } R = f()",
  "var f = 1; { function f() {} } R = typeof f",
  "let f = 1; { function f() {} } R = f",
  "{ function f() {} let g = 1 } R = typeof f",
  "{ let f = 1; { function f() {} } } R = typeof f",
  "{ let f = 1; { function f() {} } R = typeof f }",
  "function f() { return 'outer' } { function f() { return 'inner' } } R = f()",
  "function f() { return 'outer' } R = f(); { function f() { return 'inner' } } R += f()",
  "{ f = 1; function f() {} R = typeof f } R += typeof f",
  "{ function f() {} f = 1; R = typeof f } R += typeof f",
  "{ function f() {} f = 1 } R = typeof f",
  "try { { function f() {} } R = 1 } catch (e) { R = e.name }",
  "switch (1) { case 1: function f() {} } R = typeof f",
  "switch (1) { case 0: function f() {} } R = typeof f",
  "for (var i = 0; i < 1; i++) { function f() {} } R = typeof f",
  "for (let i = 0; i < 1; i++) { function f() { return i } } R = typeof f",
  "L: function f() {} R = typeof f",
  "{ L: function f() {} } R = typeof f",
  "{ function* f() {} } R = typeof f",
  "{ async function f() {} } R = typeof f",
  "{ class f {} } R = typeof f",
  "(function () { { function f() {} } R = typeof f })()",
  "(function () { 'use strict'; { function f() {} } R = typeof f })()",
  "(function (f) { { function f() {} } R = typeof f })(1)",
  "(function (f = 1) { { function f() {} } R = typeof f })()",
  "(function () { var f = 1; { function f() {} } R = typeof f })()",
  "(function () { let f = 1; { function f() {} } R = typeof f })()",
  "(function () { const f = 1; { function f() {} } R = typeof f })()",
  "(function () { { function f() {} } let f2; R = typeof f })()",
  "(function () { { function arguments() {} } R = typeof arguments })()",
  "(function () { { function f() {} } var g = () => f; R = typeof g() })()",
  "(function () { if (1) { function f() { return 'a' } } else { function f() { return 'b' } } R = f() })()",
  "(function () { if (0) { function f() { return 'a' } } else { function f() { return 'b' } } R = f() })()",
  "(function () { R = typeof f; if (0) { function f() {} } })()",
  "(function () { { function f() { return 1 } f = 2 } R = typeof f })()",
  "(function () { { f = 2; function f() { return 1 } } R = typeof f })()",
  "(function () { { function f() { return 1 } } { function f() { return 2 } } R = f() })()",
  "(function () { { { function f() {} } } R = typeof f })()",
  "(function () { eval('{ function f() {} }'); R = typeof f })()",
  "(function () { eval('R = typeof f; { function f() {} }'); })()",
  "(function () { 'use strict'; eval('{ function f() {} }'); R = typeof f })()",
  "(function () { var o = { m() { { function f() {} } return typeof f } }; R = o.m() })()",
  "(function () { var a = () => { { function f() {} } return typeof f }; R = a() })()",
  "(function () { { function f() {} } return typeof f })(); R = typeof f",
  "(function () { { function f() {} } R = f.name })()",
  "(function () { { function f() {} } R = f.length })()",
  "{ function f(a, b) {} } R = f.length",
  "{ function f() { return typeof f } } R = f()",
  "{ function f() { f = 1; return typeof f } } R = f() + typeof f",
];
for (const b of hoistBodies) {
  T(b);
}
S("{ function f() {} function f() {} }");
S("'use strict'; { function f() {} function f() {} }");
S("{ function f() {} let f }");
S("{ let f; function f() {} }");
S("{ function f() {} var f }");
S("{ var f; function f() {} }");
S("{ function* f() {} function f() {} }");
S("{ async function f() {} function f() {} }");
S("{ function f() {} class f {} }");
S("switch (1) { case 1: function f() {} case 2: function f() {} }");
S("switch (1) { case 1: let f; case 2: function f() {} }");
S("switch (1) { case 1: var f; case 2: function f() {} }");
S("function f() {} function f() {}");
S("function f() {} var f");
S("var f; function f() {}");
S("function f() {} let f");
S("let f; function f() {}");
S("function f() { } { var f }");
S("if (1) function f() {} else function f() {}");
S("'use strict'; if (1) function f() {}");
S("while (0) function f() {}");
S("for (;;) function f() {}");
S("if (1) async function f() {}");
S("if (1) function* f() {}");
S("if (1) class C {}");
S("if (1) let x");
S("if (1) const x = 1");
S("while (0) let x");
S("while (0) let\nx");
S("do let x; while (0)");
S("L: let x");
S("L: const x = 1");
S("L: class C {}");
S("L: function* g() {}");
S("L: async function g() {}");
S("'use strict'; L: function f() {}");
S("L: L: ;");
S("L: { L: ; }");
S("L: { M: ; L: ; }");
S("L: function f() {} L: ;");
V("L: { 5 }");
V("var x = 1; L: { x = 2; break L; x = 3 } x");
V("if (1) function f() {}; typeof f");
V("for (let in {}) ; 'ok'");
V("var let = 1; let");
V("let\nx = 1; x");
V("var let; for (let in {}) ; typeof let");
V("var let; for (let of => 0; false;) ; 1");
V("var let = [1]; let[0]");
S("for (let of []) ;");
S("for (let.x of []) ;");
S("for (let in {}) ; let x");
S("for (let let of []) ;");
S("for (let let in {}) ;");
S("let let = 1");
S("const let = 1");
S("class let {}");
S("'use strict'; var let");
S("'use strict'; let\nx");
S("for (let x, y of []) ;");
S("for (let x = 1 of []) ;");
S("for (let x = 1 in {}) ;");
S("for (var x = 1 in {}) ;");
S("'use strict'; for (var x = 1 in {}) ;");
S("for (var [x] = 1 in {}) ;");
S("for (const x of []) { x = 1 }");
V("var r = 0; for (const x of [1]) { try { x = 2 } catch (e) { r = e.message } } r");
V("var r = ''; for (const x in { a: 1 }) { try { x = 2 } catch (e) { r = e.message } } r");
V("var r = ''; for (const i = 0; i < 1; ) { try { i++ } catch (e) { r = e.message } break } r");
V("var r = ''; label: for (let x of [1]) { try { x; continue label } finally { r = 'f' } } r");
V("var r = []; a: for (let i = 0; i < 2; i++) { b: for (let j = 0; j < 2; j++) { r.push(() => i + j); continue a } } r.map(f => f()).join()");
V("var r = []; for (let x in { p: 1 }) { r.push(x) } for (let x in { q: 1 }) { r.push(x) } r.join()");
V("var r = []; for (let x of [1]) { let y = x; r.push(() => y) } r[0]()");
V("typeof (function () { for (let x in { a: 1 }) { return () => x } })()()");
V("(function () { for (let x in { a: 1 }) { return () => x } })()()");
V("(function () { for (let x of [5]) { return () => x } })()()");
V("(function () { for (const x of [5]) { return () => x } })()()");

// ---- const sem inicializador e redeclarações.
S("const x");
S("const x, y = 1");
S("const x = 1, y");
S("let x, const y");
S("const [x]");
S("const { x }");
S("let [x]");
S("let { x }");
S("var [x]");
S("var { x }");
S("for (const x;;) ;");
S("for (const x, y of []) ;");
S("for (const x = 1;;) break");
S("for (const [a];;) break");
S("let x; let x");
S("let x; var x");
S("var x; let x");
S("const x = 1; var x");
S("var x; const x = 1");
S("let x; const x = 1");
S("const x = 1; const x = 2");
S("class x {}; let x");
S("let x; class x {}");
S("class x {}; var x");
S("var x; class x {}");
S("class x {}; class x {}");
S("class x {}; function x() {}");
S("function x() {}; class x {}");
S("let x; function x() {}");
S("let x, x");
S("let [x, x] = []");
S("let { x, y: x } = {}");
S("const [x, ...x] = []");
S("var x; { let x; var x }");
S("{ let x; var x }");
S("{ var x; let x }");
S("{ let x; { var x } }");
S("{ var x; { let x } }");
S("{ const x = 1; var x }");
S("{ let x; function x() {} }");
S("{ function x() {} let x }");
S("{ class x {} function x() {} }");
S("{ let x; { let x } }");
S("let x; { let x }");
S("function f() { let x; var x }");
S("function f() { var x; let x }");
S("function f() { let x; { var x } }");
S("function f() { let x; function x() {} }");
S("function f() { function x() {} let x }");
S("function f() { function x() {} var x }");
S("function f() { function x() {} function x() {} }");
S("function f(x) { function x() {} }");
S("function f() { class x {} class x {} }");
S("(function () { let a, a })");
S("(() => { let a; var a })");
S("(() => { let a; let a })");
S("class A { m() { let a; var a } }");
S("class A { static { let a; var a } }");
S("class A { static { var a; let a } }");
S("class A { static { var a; var a } }");
S("class A { static { function a() {} function a() {} } }");
S("class A { static { let a; function a() {} } }");
S("class A { static { await } }");
S("class A { static { return } }");
S("class A { static { arguments } }");
S("class A { static { var arguments } }");
S("class A { static { super() } }");
S("class A { static { yield } }");
S("class A { static { var await } }");
S("class A { static { () => await } }");
S("function f() { class A { static { var x } } var x }");
S("switch (1) { case 1: let x; case 2: let x }");
S("switch (1) { case 1: let x; default: var x }");
S("switch (1) { case 1: var x; case 2: let x }");
S("switch (1) { case 1: const x = 1; case 2: class x {} }");
S("try { } catch (e) { } let e; let e");
S("let x; try { } catch (x) { var x }");
S("for (let x;;) { var x; break }");
S("for (let x of []) { var x }");
S("for (let x in {}) { var x }");
S("for (const x = 0;;) { let x; break }");
S("for (let x;;) { let x; break }");
S("for (let x, x;;) break");
S("for (let [x, x] of []) ;");
S("for (var x of []) { let x }");
S("for (let x of []) { function x() {} }");
S("for (let x of []) { { var x } }");
S("function f(a = 1, a) {}");
S("function f({ a }, a) {}");
S("function f(a, ...a) {}");
S("(a, ...a) => 1");
S("({ m(a, a) {} })");
S("class A { m(a, a) {} }");
S("class A { constructor(a, a) {} }");
S("'use strict'; function f(eval) {}");
S("'use strict'; function f(arguments) {}");
S("'use strict'; function eval() {}");
S("'use strict'; var eval");
S("'use strict'; var arguments");
S("'use strict'; eval = 1");
S("'use strict'; arguments++");
S("'use strict'; ({ eval } = {})");
S("'use strict'; [arguments] = []");
S("'use strict'; var yield");
S("'use strict'; var let");
S("'use strict'; var static");
S("'use strict'; var implements");
S("'use strict'; var interface");
S("'use strict'; var package");
S("'use strict'; var private");
S("'use strict'; var protected");
S("'use strict'; var public");
S("var yield; var await; var async; var of; var get; var set; var static");
S("function* g() { var yield }");
S("function* g(yield) {}");
S("function* g() { function yield() {} }");
S("function* g() { function* yield() {} }");
S("(function* yield() {})");
S("(function yield() {})");
S("async function f() { var await }");
S("async function f(await) {}");
S("(async function await() {})");
S("(async await => 1)");
S("async (await) => 1");
S("function await() {}");
S("var await = 1; function f() { return await }");
S("class await {}");
S("class yield {}");
S("class async {}");
S("'use strict'; class let {}");
S("class A extends B { constructor() { super.x; super() } }");
S("class A { constructor() { super() } }");
S("function f() { super.x }");
S("function f() { new.target }");
S("new.target");
S("() => new.target");
S("class A { x = new.target }");
S("class A { x = arguments }");
S("class A { x = () => arguments }");
S("class A { x = function () { return arguments } }");
S("class A { static x = arguments }");
S("class A { #a; #a }");
S("class A { #a; static #a }");
S("class A { get #a() {} set #a(v) {} }");
S("class A { get #a() {} get #a() {} }");
S("class A { static get #a() {} set #a(v) {} }");
S("class A { m() { this.#b } }");
S("class A { m() { delete this.#a } #a }");
S("class A { #constructor }");
S("class A { constructor() {} constructor() {} }");
S("class A { 'constructor'() {} constructor() {} }");
S("class A { get constructor() {} }");
S("class A { static prototype() {} }");
S("class A { static prototype = 1 }");
S("class A { constructor = 1 }");
S("class A { static constructor = 1 }");
S("class A { 'constructor' = 1 }");
S("({ __proto__: 1, __proto__: 2 })");
S("({ __proto__: 1, '__proto__': 2 })");
S("({ __proto__: 1, ['__proto__']: 2 })");
S("({ __proto__: 1, __proto__ })");
S("({ __proto__: 1, __proto__() {} })");
S("({ __proto__: a, __proto__: b } = {})");

// ---- eval var injection.
T("function f() { eval('var x = 1'); return typeof x } R = f()");
T("function f() { 'use strict'; eval('var x = 1'); return typeof x } R = f()");
T("function f() { eval('\"use strict\"; var x = 1'); return typeof x } R = f()");
T("function f() { eval('var x = 1'); return x } R = f()");
T("function f() { return eval('var x = 1; x') + typeof x } R = f()");
T("function f() { eval('let x = 1'); return typeof x } R = f()");
T("function f() { eval('const x = 1'); return typeof x } R = f()");
T("function f() { eval('class x {}'); return typeof x } R = f()");
T("function f() { eval('function x() {}'); return typeof x } R = f()");
T("function f() { eval('{ function x() {} }'); return typeof x } R = f()");
T("function f() { var x = 'a'; eval('var x = \"b\"'); return x } R = f()");
T("function f() { let x = 'a'; eval('var x = \"b\"'); return x } R = f()");
T("function f() { let x = 'a'; { eval('var x = \"b\"') } return x } R = f()");
T("function f() { { let x = 'a'; eval('var x = \"b\"') } } f()");
T("function f() { { let x = 'a'; { eval('var x = \"b\"') } } } f()");
T("function f() { const x = 'a'; eval('var x = 1') } f()");
T("function f() { class x {} eval('var x = 1') } f()");
T("function f(x) { eval('var x = 1'); return x } R = f(0)");
T("function f(x = 0) { eval('var x = 1'); return x } R = f()");
T("function f(x = 0) { eval('let x = 1'); return x } R = f()");
T("function f() { eval('var x = 1'); eval('var x = 2'); return x } R = f()");
T("function f() { eval('var x = 1'); return (() => x)() } R = f()");
T("function f() { eval('var x = 1'); return delete x } R = f()");
T("function f() { var x = 1; return delete x } R = f()");
T("function f() { eval('var x = 1'); delete x; return typeof x } R = f()");
T("function f() { eval('var x = 1'); delete x; delete x; return typeof x } R = f()");
T("function f() { eval('var x = 1'); var g = () => delete x; return g() + typeof x } R = f()");
T("function f() { return eval('var x = 5; function g() { return x } g()') } R = f()");
T("function f() { eval('function g() { return 1 }'); return g() } R = f()");
T("function f() { eval('function g() { return 1 }'); eval('function g() { return 2 }'); return g() } R = f()");
T("function f() { function g() { return 0 } eval('function g() { return 2 }'); return g() } R = f()");
T("function f() { eval('var arguments = 2'); return arguments } R = f()");
T("function f() { eval('var f = 2'); return typeof f } R = f()");
T("function f() { eval('var f2 = this'); return typeof f2 } R = f()");
T("function f() { var g = () => eval('var x = 3; x'); return [g(), typeof x] } R = " + J + "(f())");
T("function f() { var g = () => { eval('var x = 3'); return x }; return g() + typeof x } R = f()");
T("function f() { var e = eval; e('var gv1 = 1'); return typeof gv1 } R = f() + typeof gv1");
T("function f() { (0, eval)('var gv2 = 1'); return typeof gv2 } R = f() + typeof gv2");
T("function f() { eval?.('var gv3 = 1'); return typeof gv3 } R = f() + typeof gv3");
T("function f() { return eval('this') === this } R = f.call({})");
T("function f() { return eval('typeof new.target') } R = f()");
T("function f() { return eval('arguments.length') } R = f(1, 2)");
T("function f() { return (() => eval('arguments.length'))() } R = f(1, 2)");
T("var f = () => eval('typeof arguments'); R = f()");
T("function f() { return eval('(() => this)()') } R = typeof f.call(5)");
T("var o = { m() { return eval('super.toString === Object.prototype.toString') } }; R = o.m()");
T("function f() { return eval('super.x') } f()");
T("class A { m() { return eval('new.target') } } R = new A().m()");
T("class A { constructor() { eval('super()') } } class B extends A {} R = typeof new B()");
T("class B extends Object { constructor() { eval('super()'); R = typeof this } } new B()");
T("class B { constructor() { eval('super()') } } new B()");
T("function f() { eval('var x = 1; let y = 2'); return typeof y } R = f() + typeof x");
T("function f() { eval('let y = 2; var y2 = y'); return typeof y2 } R = f()");
T("function f() { eval('var y; let y') } f()");
T("function f() { eval('let y; var y') } f()");
T("function f() { eval('let y = 1; { var y2 }'); } R = f()");
T("function f() { eval('{ let y; var y }') } f()");
T("function f() { eval('var x = 1'); { let x = 2; return x } } R = f()");
T("function f() { { let x = 2; eval('var x = 1') } } f()");
T("function f() { let x = 1; return eval('var y = x + 1; y') } R = f()");
T("function f() { var x = 1; return eval('let x = 2; x') + x } R = f()");
T("function f() { var x = 1; eval('let x = 2'); return x } R = f()");
T("function f() { var x = 1; eval('x = 2'); return x } R = f()");
T("function f() { return eval('x'); let x = 1 } f()");
T("function f() { return eval('typeof x'); let x = 1 } f()");
T("let top1 = 1; R = eval('top1')");
T("let top2 = 1; eval('var top2b = top2'); R = top2b");
T("eval('var topv = 1'); R = typeof topv + delete globalThis.topv");
T("eval('var topv2 = 1'); R = delete topv2 + typeof topv2");
T("var topv3 = 1; R = delete topv3 + typeof topv3");
T("globalThis.gp = 1; R = delete gp + typeof gp");
T("function topf() {} R = delete topf");
T("let topl = 1; R = delete topl");
T("R = delete undefinedThing");
T("R = delete globalThis.undefinedThing");
T("var o = { p: 1 }; R = delete o.p");
T("var o = { p: 1 }; R = delete o");
T("R = delete 1");
T("R = delete (0, undefinedThing2)");
T("'use strict'; var s = 1; R = delete s");
S("'use strict'; var s = 1; delete s");
S("'use strict'; delete s");
S("'use strict'; delete (s)");
S("'use strict'; delete ((s))");
S("'use strict'; delete s.x");
S("'use strict'; delete (s, s)");
S("'use strict'; function f() { delete s }");
S("'use strict'; delete this.#a");
S("class A { #a; m() { delete this.#a } }");
S("class A { #a; m() { delete (this.#a) } }");
S("class A { #a; m() { delete this?.#a } }");
S("class A { #a; m() { delete this.x.#a } }");
S("'use strict'; delete (1, s)");
S("'use strict'; delete s?.x");
S("class A { m() { delete s } }");
T("class A { m() { return delete s } } R = new A().m()");
T("class A { m() { return delete A } } R = new A().m()");
T("function f() { 'use strict'; return eval('var a = 1; delete a') } R = f()");
T("function f() { return eval('var a = 1; delete a') } R = f()");
T("(0, eval)('\"use strict\"; delete globalThis.nothing')");
T("R = eval('var ev1 = 1; delete ev1')");
T("R = (0, eval)('var ev2 = 1; delete ev2')");
T("(function () { var x = 1; R = (function () { return delete x })() })()");
T("(function () { var x = 1; (function () { eval('var x = 2') })(); R = x })()");
T("(function () { var x = 1; (function () { eval('x = 2') })(); R = x })()");
T("(function () { eval('var x = 1'); (function () { R = typeof x })() })()");
T("(function () { eval('var x = 1'); (function () { R = x })() })()");
T("var xx = 'g'; (function () { eval('var xx = 1'); delete xx; R = xx })()");
T("var xx2 = 'g'; (function () { var xx2 = 1; delete xx2; R = xx2 })()");
T("(function () { eval('var xx3 = 1'); (function () { eval('var xx3 = 2') })(); R = xx3 })()");

// ---- with + closures.
T("var o = { x: 1 }; var f; with (o) { f = () => x } o.x = 2; R = f()");
T("var o = { x: 1 }; var f; with (o) { f = () => x } delete o.x; R = typeof f()");
T("var o = { x: 1 }; var f; with (o) { f = () => x } delete o.x; var x = 'g'; R = f()");
T("var o = { x: 1 }; with (o) { var x = 2 } R = o.x + ':' + x");
T("var o = { }; with (o) { var x = 2 } R = o.x + ':' + x");
T("var o = { x: 1 }; with (o) { x = 5 } R = o.x");
T("var o = { x: 1 }; with (o) { x++ } R = o.x");
T("var o = { x: 1 }; with (o) { (() => { x = 5 })() } R = o.x");
T("var o = { x: 1 }; with (o) { eval('x = 5') } R = o.x");
T("var o = { x: 1 }; with (o) { eval('var x = 5') } R = o.x + ':' + x");
T("var o = { x: 1 }; with (o) { function f() { return x } } R = f()");
T("var o = { x: 1 }; with (o) { function f() { return x } } o.x = 3; R = f()");
T("var o = { x: 1 }; with (o) { let x = 2; R = x } R += o.x");
T("var o = { f() { return this === o } }; with (o) { R = f() }");
T("var o = { f() { return this === o } }; with (o) { R = (f)() }");
T("var o = { f() { return this === o } }; with (o) { R = (0, f)() }");
T("var o = { f() { return typeof this } }; with (o) { R = (() => f())() }");
T("var o = { x: 1, [Symbol.unscopables]: { x: true } }; var x = 'g'; with (o) { R = x }");
T("var o = { x: 1, [Symbol.unscopables]: { x: false } }; var x = 'g'; with (o) { R = x }");
T("var o = { x: 1, get [Symbol.unscopables]() { R = 'u'; return {} } }; with (o) { x }");
T("var o = { toString: 1 }; with (o) { R = typeof toString }");
T("with (Array.prototype) { R = typeof map + typeof values + typeof keys }");
T("with ([]) { R = typeof keys + typeof flat + typeof at }");
T("var o = { x: 1 }; with (o) { var f = function () { return typeof x } } o.x = 2; R = f()");
T("var o = { x: 1 }; var f; with (o) { f = function g() { return typeof g + x } } R = f()");
T("var o = { g: 1 }; var f; with (o) { f = function g() { return typeof g } } R = f()");
T("var o = { x: 1 }; var f; with (o) { f = function* () { yield x } } o.x = 4; R = f().next().value");
T("var o = { x: 1 }; var f; with (o) { f = async () => x } o.x = 4; f().then(v => { R = v })");
T("var o = { x: 1 }; with (o) { class C { m() { return x } } var c = new C() } o.x = 9; R = c.m()");
T("var o = { x: 1 }; with (o) { R = typeof x; { let x = 2 } }");
T("var o = { x: 1 }; with (o) { for (let x = 0; x < 1; x++) { R = x } }");
T("var o = { x: 1 }; with (o) { for (var x = 0; x < 1; x++) { } } R = o.x");
T("var o = { x: 1 }; var fs = []; with (o) { for (let i = 0; i < 2; i++) fs.push(() => x + i) } o.x = 10; R = fs.map(f => f()).join()");
T("with ({ a: 1 }) { with ({ b: 2 }) { R = a + b } }");
T("with ({ a: 1 }) { with ({ a: 2 }) { R = a } }");
T("with ({ a: 1 }) { var g = () => { with ({ a: 3 }) { return a } }; R = g() + a }");
T("with ({ a: 1 }) { try { throw 2 } catch (a) { R = a } }");
T("with ({ a: 1 }) { try { throw 2 } catch (e) { var a = e } R = a }");
T("with ({ a: 1 }) { R = (function () { return typeof a })() }");
T("with ({ a: 1 }) { R = (function (a) { return a })(5) }");
T("with ({ a: 1 }) { R = (function () { var a = 5; return a })() }");
T("with ({ a: 1 }) { R = (function () { return typeof a2; var a2 })() }");
T("with (null) {}");
T("with (undefined) {}");
T("with (1) { R = typeof toFixed }");
T("with ('str') { R = length }");
T("with (Symbol()) { R = typeof description }");
T("with ({}) { function f() {} } R = typeof f");
T("var o = new Proxy({}, { has(t, k) { (R = R || []).push(typeof k == 'symbol' ? 'sym' : k); return false } }); with (o) { zz1 } ");
T("R = []; var o = new Proxy({ x: 1 }, { has(t, k) { R.push(typeof k == 'symbol' ? 'sym' : k); return k in t }, get(t, k) { R.push('get:' + (typeof k == 'symbol' ? 'sym' : k)); return t[k] } }); with (o) { x } R = R.join()");
T("R = []; var o = new Proxy({ x: 1 }, { has(t, k) { R.push('has:' + (typeof k == 'symbol' ? 'sym' : k)); return k in t }, get(t, k) { R.push('get:' + (typeof k == 'symbol' ? 'sym' : k)); return t[k] }, set(t, k, v) { R.push('set:' + k); t[k] = v; return true } }); with (o) { x = 2 } R = R.join()");
T("R = []; var o = new Proxy({ x: 1 }, { has(t, k) { R.push('has:' + (typeof k == 'symbol' ? 'sym' : k)); return k in t }, get(t, k) { R.push('get:' + (typeof k == 'symbol' ? 'sym' : k)); return t[k] }, set(t, k, v) { R.push('set:' + k); t[k] = v; return true } }); with (o) { x++ } R = R.join()");
T("R = []; var o = new Proxy({ x: 1 }, { has(t, k) { R.push('has:' + (typeof k == 'symbol' ? 'sym' : k)); return k in t }, get(t, k) { R.push('get:' + (typeof k == 'symbol' ? 'sym' : k)); return t[k] } }); with (o) { typeof x } R = R.join()");
T("R = []; var o = new Proxy({ x: 1 }, { has(t, k) { R.push('has:' + (typeof k == 'symbol' ? 'sym' : k)); return k in t }, get(t, k) { R.push('get:' + (typeof k == 'symbol' ? 'sym' : k)); return t[k] }, deleteProperty(t, k) { R.push('del:' + k); return delete t[k] } }); with (o) { delete x } R = R.join()");
T("R = []; var o = new Proxy({ f() { return 1 } }, { has(t, k) { R.push('has:' + (typeof k == 'symbol' ? 'sym' : k)); return k in t }, get(t, k) { R.push('get:' + (typeof k == 'symbol' ? 'sym' : k)); return t[k] } }); with (o) { f() } R = R.join()");
S("'use strict'; with ({}) {}");
S("function f() { 'use strict'; with ({}) {} }");
S("with ({}) function f() {}");
S("with ({}) let\nx");
S("with ({}) class C {}");
S("with ({}) const x = 1");
S("class A { m() { with ({}) {} } }");
S("with (1) with (2) ;");
S("with ({}) { let x; var x }");
S("with ({}) { function f() {} function f() {} }");

// ---- Generators e async capturando escopo.
T("function* g() { var a = 1; yield () => a; a = 2; yield () => a } var it = g(); var f1 = it.next().value; var f2 = it.next().value; R = f1() + ':' + f2()");
T("function* g() { let a = 1; yield () => a++; yield () => a } var it = g(); var f1 = it.next().value; var f2 = it.next().value; f1(); R = f2()");
T("function* g() { for (let i = 0; i < 3; i++) yield () => i } R = [...g()].map(f => f()).join()");
T("function* g() { for (var i = 0; i < 3; i++) yield () => i } R = [...g()].map(f => f()).join()");
T("function* g(a, b = () => a) { a = 2; yield b() } R = g(1).next().value");
T("function* g(a) { yield arguments.length; yield arguments[0] } var it = g(5, 6); R = it.next().value + ':' + it.next().value");
T("function* g() { yield this } R = typeof g.call(5).next().value");
T("function* g() { yield (() => this)() } R = typeof g.call(5).next().value");
T("function* g() { yield (() => arguments[0])() } R = g(9).next().value");
T("function* g() { yield new.target } R = g().next().value");
T("function* g() { yield eval('typeof x'); var x } R = g().next().value");
T("function* g() { yield eval('var x = 1; x'); yield typeof x } var it = g(); R = it.next().value + ':' + it.next().value");
T("function* g() { try { yield 1 } finally { R = 'fin' } } var it = g(); it.next(); it.return()");
T("function* g() { let x = yield 1; yield x + 1 } var it = g(); it.next(); R = it.next(5).value");
T("function* g() { const x = yield; yield () => x } var it = g(); it.next(); R = it.next(7).value()");
T("function* g() { yield* (function* () { yield 1; yield 2 })() } R = [...g()].join()");
T("function* g() { var fs = []; for (let i of [1, 2]) fs.push(() => i); yield fs } R = g().next().value.map(f => f()).join()");
T("function* g() { yield typeof x; let x } try { g().next() } catch (e) { R = e.name + ': ' + e.message }");
T("function* g() { yield x; let x } try { g().next() } catch (e) { R = e.name + ': ' + e.message }");
T("function* g() { x; let x } try { g().next() } catch (e) { R = e.name + ': ' + e.message }");
T("var o = { *g() { yield super.toString === Object.prototype.toString } }; R = o.g().next().value");
T("class K { *g() { yield this instanceof K } } R = new K().g().next().value");
T("class K { static *g() { yield this === K } } R = K.g().next().value");
T("class K { #p = 1; *g() { yield this.#p } } R = new K().g().next().value");
T("function* g() { yield 1 } var it = g(); R = typeof it[Symbol.iterator]() + (it[Symbol.iterator]() === it)");
T("function* g() { var a = 1; { let a = 2; yield a } yield a } R = [...g()].join()");
T("function* g() { yield function () { return typeof g } } R = g().next().value()");
T("var g = function* h() { yield typeof h } R = g().next().value");
T("var g = function* h() { h = 1; yield typeof h } R = g().next().value");
T("async function f() { var a = 1; await null; return () => a } f().then(g => { R = g() })");
T("async function f() { for (let i = 0; i < 2; i++) { await null; R = (R || '') + i } } f()");
T("async function f() { var fs = []; for (let i = 0; i < 3; i++) { await null; fs.push(() => i) } return fs } f().then(fs => { R = fs.map(g => g()).join() })");
T("async function f() { var fs = []; for (var i = 0; i < 3; i++) { await null; fs.push(() => i) } return fs } f().then(fs => { R = fs.map(g => g()).join() })");
T("async function f(a, b = () => a) { a = 2; await null; return b() } f(1).then(v => { R = v })");
T("async function f() { return arguments.length } f(1, 2).then(v => { R = v })");
T("async function f() { return (() => arguments[0])() } f(3).then(v => { R = v })");
T("async function f() { await null; return arguments[0] } f(3).then(v => { R = v })");
T("async function f() { return this } f.call(7).then(v => { R = typeof v })");
T("async function f() { return (() => this)() } f.call(7).then(v => { R = typeof v })");
T("async function f() { await null; return this === globalThis } f.call(undefined).then(v => { R = v })");
T("async function f() { return typeof x; let x } f().catch(e => { R = e.name + ': ' + e.message })");
T("async function f() { await null; x; let x } f().catch(e => { R = e.name + ': ' + e.message })");
T("async function f() { const c = 1; await null; c = 2 } f().catch(e => { R = e.name + ': ' + e.message })");
T("async function f() { try { await Promise.reject(1) } catch (e) { var e = 2; return e } } f().then(v => { R = v })");
T("async function f() { try { await Promise.reject(1) } catch (e) { return () => e } } f().then(g => { R = g() })");
T("async function f() { return eval('typeof x'); var x } f().then(v => { R = v })");
T("async function f() { return eval('var x = 1; x') } f().then(v => { R = v })");
T("async function f() { await null; eval('var z = 3'); return z } f().then(v => { R = v })");
T("async () => { await 1 }; R = typeof arguments");
T("var f = async () => { await null; return typeof arguments }; f().then(v => { R = v })");
T("function o() { return async () => { await null; return arguments[0] } } o(4)().then(v => { R = v })");
T("function o() { return async () => { await null; return this } } o.call(8)().then(v => { R = typeof v })");
T("function o() { return async () => { await null; return new.target } } new o()().then(v => { R = typeof v })");
T("class K { async m() { await null; return this instanceof K } } new K().m().then(v => { R = v })");
T("class K { async m() { await null; return super.constructor === Object } } new K().m().then(v => { R = v })");
T("var o = { async m() { await null; return super.toString === Object.prototype.toString } }; o.m().then(v => { R = v })");
T("class K { #p = 5; async m() { await null; return this.#p } } new K().m().then(v => { R = v })");
T("async function* g() { var a = 1; yield () => a; a = 2 } g().next().then(r => { R = r.value() })");
T("async function* g() { for (let i = 0; i < 2; i++) { await null; yield () => i } } (async () => { var r = []; for await (var f of g()) r.push(f); R = r.map(h => h()).join() })()");
T("async function* g() { yield arguments.length } g(1, 2).next().then(r => { R = r.value })");
T("(async () => { for await (let x of [Promise.resolve(1)]) { R = x } })()");
T("(async () => { var fs = []; for await (let x of [1, 2]) fs.push(() => x); R = fs.map(f => f()).join() })()");
T("(async () => { var fs = []; for await (var x of [1, 2]) fs.push(() => x); R = fs.map(f => f()).join() })()");
T("(async () => { var fs = []; for await (const [x] of [[1], [2]]) fs.push(() => x); R = fs.map(f => f()).join() })()");
T("(async function f() { f = 1; R = typeof f })()");
T("(async function f() { await null; R = typeof f })()");
T("(async function f() { var f = 1; await null; R = f })()");
T("var o = { async *g() { yield this === o } }; o.g().next().then(r => { R = r.value })");
S("async function f() { function g() { await 1 } }");
S("async function f() { (a = await 1) => 1 }");
S("async function f(a = await 1) {}");
S("async (a = await 1) => 1");
S("function* g(a = yield) {}");
S("function* g() { (a = yield) => 1 }");
S("function* g() { function h(a = yield) {} }");
S("async function* g() { yield await 1 }");
S("async function* g(a = yield) {}");
S("function f() { await 1 }");
S("function f() { yield 1 }");
S("(async function () { for await (x of []); })");
S("function f() { for await (x of []); }");
S("(async function () { for await (x in {}); })");
S("(async function () { for await (let x = 1;;); })");
S("async function f() { let await }");
S("async function f() { var await }");
S("function* g() { let yield }");
S("class A { async constructor() {} }");
S("class A { *constructor() {} }");
S("class A { async *constructor() {} }");
S("class A { get constructor() {} }");
S("class A { set constructor(v) {} }");
S("class A { static async *prototype() {} }");

// ---- Arrow this/arguments/new.target/super lexicais.
T("function f() { return (() => this)() } R = f.call('s') === 's' || typeof f.call('s')");
T("function f() { 'use strict'; return (() => this)() } R = f.call('s')");
T("function f() { 'use strict'; return (() => this)() } R = f.call(undefined)");
T("function f() { return (() => this)() } R = f.call(undefined) === globalThis");
T("function f() { return (() => (() => this)())() } R = typeof f.call(1)");
T("function f() { return () => this } var g = f.call({ a: 1 }); R = g.call({ a: 2 }).a");
T("function f() { return () => this } var g = f.call({ a: 1 }); R = new (class { m() { return g() } })().m().a");
T("function f() { return () => this } var g = f.call({ a: 1 }); R = g.bind({ a: 3 })().a");
T("function f() { return () => this } var g = f.call({ a: 1 }); try { new g() } catch (e) { R = e.name + ': ' + e.message }");
T("var g = () => this; R = typeof g() + (g() === globalThis)");
T("var g = () => this; R = g.call(1) === globalThis");
T("var o = { f() { return () => this } }; R = o.f()() === o");
T("var o = { f: () => this }; R = o.f() === globalThis");
T("var o = { f() { return { g: () => this } } }; R = o.f().g() === o");
T("var o = { f() { return [1].map(() => this)[0] } }; R = o.f() === o");
T("var o = { f() { return [1].map(function () { return this })[0] } }; R = o.f() === globalThis");
T("var o = { f() { return [1].map(function () { return this }, 5)[0] } }; R = typeof o.f()");
T("var o = { f() { 'use strict'; return [1].map(function () { return this }, 5)[0] } }; R = typeof o.f()");
T("function f() { return () => arguments[0] } R = f(1)(2)");
T("function f() { return () => () => arguments.length } R = f(1, 2, 3)()()");
T("function f() { var a = () => { arguments = 5 }; a(); return arguments } R = f(1)");
T("function f() { var a = () => { arguments[0] = 5 }; a(); return arguments[0] } R = f(1)");
T("function f(a) { var g = () => { arguments[0] = 5 }; g(); return a } R = f(1)");
T("function f(a) { var g = () => { a = 5 }; g(); return arguments[0] } R = f(1)");
T("function f() { return () => new.target } R = f()() + '' + typeof new f()()");
T("function f() { return () => new.target === f } R = new f()()");
T("function f() { return eval('() => new.target') } R = typeof new f()()");
T("function f() { var g = () => () => new.target; return g()() } R = f() + '|' + (new f() instanceof f)");
T("function F() { this.t = (() => new.target === F)() } R = new F().t");
T("function F() { return new.target } R = F() + '|' + (new F() === F)");
T("function F() { return typeof new.target } R = F() + (new F() instanceof Object)");
T("class A { constructor() { this.nt = new.target.name } } class B extends A {} R = new B().nt");
T("class A { constructor() { this.nt = (() => new.target.name)() } } class B extends A {} R = new B().nt");
T("class A { m() { return () => new.target } } R = new A().m()()");
T("class A { static m() { return () => new.target } } R = A.m()()");
T("class A { static x = new.target } R = A.x");
T("class A { x = new.target } R = new A().x");
T("class A { x = () => new.target } R = new A().x()");
T("class A { x = function () { return new.target } } R = new A().x()");
T("class A { static { R = new.target } }");
T("class A { static { R = (() => new.target)() } }");
T("class A { static { R = this === A } }");
T("class A { static { R = (() => this)() === A } }");
T("class A { x = this } R = (new A().x instanceof A)");
T("class A { x = () => this } var a = new A(); R = a.x() === a");
T("class A { static x = this } R = A.x === A");
T("class A { static x = () => this } R = A.x() === A");
T("class A { static x = function () { return this } } R = A.x() === A");
T("class A { m() { return super.toString } } R = new A().m() === Object.prototype.toString");
T("class A { m() { return () => super.toString } } R = new A().m()() === Object.prototype.toString");
T("class A { static m() { return () => super.name } } R = A.m()()");
T("class A { m() { return { f: () => super.toString } } } R = typeof new A().m().f()");
T("class A { m() { return { f() { return typeof super.toString } } } } R = new A().m().f()");
T("class A { m() { return function () { return typeof super.toString } } } R = typeof A");
S("class A { m() { return function () { return super.toString } } }");
S("class A { m() { return function () { return super() } } }");
S("class A extends Object { constructor() { function f() { super() } } }");
S("class A extends Object { constructor() { (() => super())() } }");
S("class A extends Object { constructor() { () => super() } }");
S("class A extends Object { m() { () => super() } }");
S("class A extends Object { x = super() }");
S("class A extends Object { x = super.x }");
S("class A extends Object { static x = super.x }");
S("class A extends Object { static { super.x } }");
S("class A extends Object { static { super() } }");
S("({ m() { super() } })");
S("({ m() { super.x } })");
S("({ m: function () { super.x } })");
S("({ m: () => super.x })");
S("({ get g() { return super.x } })");
S("({ set s(v) { super.x = v } })");
S("({ *g() { super.x } })");
S("({ async m() { super.x } })");
S("({ [super.x]: 1 })");
S("class A { [super.x]() {} }");
S("class A extends Object { [super.x]() {} }");
T("class A { constructor() { this.x = 1 } } class B extends A { constructor() { var f = () => super(); f(); this.y = this.x } } R = new B().y");
T("class A { constructor() { this.x = 1 } } class B extends A { constructor() { var f = () => this; try { f() } catch (e) { R = e.name + ': ' + e.message } super() } } new B()");
T("class A { constructor() { this.x = 1 } } class B extends A { constructor() { var f = () => this; super(); R = f() === this } } new B()");
T("class A {} class B extends A { constructor() { var f = () => super(); f(); try { f() } catch (e) { R = e.name + ': ' + e.message } } } new B()");
T("class A {} class B extends A { constructor() { super(); try { super() } catch (e) { R = e.name + ': ' + e.message } } } new B()");
T("class A {} class B extends A { constructor() { } } try { new B() } catch (e) { R = e.name + ': ' + e.message }");
T("class A {} class B extends A { constructor() { return {} } } R = typeof new B()");
T("class A {} class B extends A { constructor() { return 1 } } try { new B() } catch (e) { R = e.name + ': ' + e.message }");
T("class A {} class B extends A { constructor() { super(); return 1 } } try { new B() } catch (e) { R = e.name + ': ' + e.message }");
T("class A {} class B extends A { constructor() { return undefined } } try { new B() } catch (e) { R = e.name + ': ' + e.message }");
T("class A {} class B extends A { constructor() { eval('super()') } } R = typeof new B()");
T("class A {} class B extends A { constructor() { (0, eval)('super()') } } try { new B() } catch (e) { R = e.name }");
T("class A {} class B extends A { constructor() { var g = () => eval('super()'); g() } } R = typeof new B()");
T("class A {} class B extends A { constructor() { super(); this.f = () => this } } var b = new B(); R = b.f() === b");
T("class A {} class B extends A { x = this; constructor() { super() } } var b = new B(); R = b.x === b");
T("class A {} class B extends A { x = () => this; constructor() { super() } } var b = new B(); R = b.x() === b");
T("class A { constructor() { this.m() } m() { R = 'A' } } class B extends A { x = 1; m() { R = 'B' + this.x } } new B()");

// ---- Getters em classes capturando private names.
T("class K { #p = 1; get p() { return this.#p } } R = new K().p");
T("class K { #p = 1; get p() { return () => this.#p } } R = new K().p()");
T("class K { #p = 1; static s(o) { return o.#p } } R = K.s(new K())");
T("class K { #p = 1; static s(o) { return o.#p } } try { K.s({}) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { #p = 1; static s(o) { return #p in o } } R = K.s(new K()) + ':' + K.s({})");
T("class K { #p = 1; static s(o) { return #p in o } } try { K.s(1) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { #m() { return 1 } static s(o) { return o.#m() } } R = K.s(new K())");
T("class K { #m() { return 1 } static s(o) { o.#m = 1 } } try { K.s(new K()) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { get #g() { return 1 } static s(o) { o.#g = 1 } } try { K.s(new K()) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { set #g(v) { } static s(o) { return o.#g } } try { K.s(new K()) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { get #g() { return 1 } set #g(v) { R = v } static s(o) { o.#g += 1 } } K.s(new K())");
T("class K { static #sp = 1; static s() { return K.#sp } } R = K.s()");
T("class K { static #sp = 1; static s(o) { return o.#sp } } class L extends K {} try { K.s(L) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { static #sm() { return 1 } static s() { return this.#sm() } } R = K.s()");
T("class K { #p = 1; m() { return class { n(o) { return o.#p } } } } var k = new K(); R = new (k.m())().n(k)");
T("class K { #p = 1; m() { return { n: o => o.#p } } } var k = new K(); R = k.m().n(k)");
T("class K { #p = 1; m() { return function (o) { return o.#p } } } var k = new K(); R = k.m()(k)");
T("class K { #p = 1; m() { return eval('this.#p') } } R = new K().m()");
T("class K { #p = 1; m() { return (0, eval)('this.#p') } } try { new K().m() } catch (e) { R = e.name }");
T("class K { #p = 1; m() { return new Function('o', 'return o.#p') } } try { new K().m() } catch (e) { R = e.name }");
T("class K { #p = 1; m() { return eval('(o) => o.#p') } } var k = new K(); R = k.m()(k)");
T("class K { #p = 1; m() { return eval('#p in this') } } R = new K().m()");
T("class K { #p = 1; m() { return eval('this.#q') } } try { new K().m() } catch (e) { R = e.name }");
T("class K { #p = 1; constructor() { this.#p = 2 } get() { return this.#p } } R = new K().get()");
T("class K { #p; constructor() { this.#p = 2 } get() { return this.#p } } R = new K().get()");
T("class K { #p = this.#q; #q = 1 } try { new K() } catch (e) { R = e.name + ': ' + e.message }");
T("class K { #q = 1; #p = this.#q + 1; get() { return this.#p } } R = new K().get()");
T("class K { #p = 1; #p2 = () => this.#p; get() { return this.#p2() } } R = new K().get()");
T("class A { constructor(o) { return o } } class K extends A { #p = 1; static has(o) { return #p in o } } var o = {}; new K(o); R = K.has(o)");
T("class A { constructor(o) { return o } } class K extends A { #p = 1 } var o = {}; new K(o); try { new K(o) } catch (e) { R = e.name + ': ' + e.message }");
T("class K { #a = 1; #b = this.#a + 1; static c = new K().#b } R = K.c");
T("class K { static #p = 1; static c = K.#p + 1 } R = K.c");
T("class K { static #p = 1; static { R = K.#p } }");
T("class K { #p = 1; static { R = typeof K } }");
T("var fs = []; class K { #p = 1; constructor() { fs.push(() => this.#p) } } new K(); new K(); R = fs.map(f => f()).join()");
T("function mk() { return class { #p = 1; static g(o) { return o.#p } } } var A = mk(), B = mk(); try { B.g(new A()) } catch (e) { R = e.name + ': ' + e.message }");
T("function mk() { return class { #p = 1; static g(o) { return o.#p } } } var A = mk(); R = A.g(new A())");
T("function mk() { return class { #p = 1; static h(o) { return #p in o } } } var A = mk(), B = mk(); R = A.h(new A()) + ':' + A.h(new B())");
T("class K { #x = 1; getX() { return this.#x } setX(v) { this.#x = v } } var k = new K(); k.setX(5); R = k.getX()");
T("class K { #x = 1; static cmp(a, b) { return a.#x === b.#x } } R = K.cmp(new K(), new K())");
T("class K { #x; constructor(v) { this.#x = v } static sum(a, b) { return a.#x + b.#x } } R = K.sum(new K(1), new K(2))");
T("class K { get g() { return () => typeof K } } R = new K().g()");
T("class K { get g() { return K } } var o = new K(); var Kb = K; K = null; R = o.g === Kb");
T("class K { static get g() { return this } } R = K.g === K");
T("class K { static get g() { return () => this } } R = K.g() === K");
T("class K { get ['a' + 'b']() { return 1 } } R = new K().ab");
T("var k = 'a'; class K { get [k]() { return k } } k = 'b'; R = new K().a");
T("var k = 'a'; class K { [k] = k } k = 'b'; R = new K().a");
T("var n = 0; class K { [n++] = n; [n++] = n } R = " + J + "(new K())");
T("var n = 0; class K { static [n++] = n } R = " + J + "(K) + n");
T("var order = []; class K { [(order.push('k1'), 'a')] = order.push('v1'); static [(order.push('k2'), 'b')] = order.push('v2') } new K(); R = order.join()");
T("var order = []; class K { static a = order.push('s1'); b = order.push('i1'); static { order.push('blk') } static c = order.push('s2') } new K(); R = order.join()");
T("class K { static a = 1; static b = this.a + 1 } R = K.b");
T("class K { static a = 1; static b = K.a + 1 } R = K.b");
T("class K { a = 1; b = this.a + 1 } R = new K().b");
T("class K { a = this.b; b = 1 } R = new K().a");
T("class K { 'a' = 1; 'b-c' = 2; 3 = 3 } R = Object.keys(new K()).join()");
T("class K { static name = 'x' } R = K.name");
T("class K { static name() {} } R = typeof K.name");
T("class K { a = (() => { try { return typeof b } catch (e) { return e.name } })(); b = 1 } R = new K().a");
T("let b = 'outer'; class K { a = b } R = new K().a");
T("class K { a = b; } try { new K() } catch (e) { R = e.name + ': ' + e.message } let b = 1");
T("class K { m() { return typeof b } } let b = 1; R = new K().m()");

// ---- IIFE patterns.
T("R = (function () { return 1 })()");
T("R = (function () { return 1 }())");
T("R = !function () { return 1 }()");
T("R = +function () { return 1 }()");
T("R = void function () { return 1 }()");
T("R = (() => 1)()");
T("R = (async () => 1)() instanceof Promise");
T("R = new function () { this.a = 1 }().a");
T("R = (function (a, b) { return a + b })(1, 2)");
T("R = (function () { return typeof arguments })()");
T("R = (function () { var x = 1; return (function () { return x + 1 })() })()");
T("var x = 0; (function () { x = 1 })(); R = x");
T("var x = 0; (function () { var x = 1 })(); R = x");
T("(function () { var a = b = 1 })(); R = typeof a + typeof b");
T("(function () { 'use strict'; var a = b = 1 })()");
T("(function () { a = 1 })(); R = typeof a + delete globalThis.a");
T("(function () { var a = 1; function g() { return a } a = 2; R = g() })()");
T("var counter = (function () { var c = 0; return { inc() { return ++c }, get() { return c } } })(); counter.inc(); counter.inc(); R = counter.get()");
T("var mk = (function () { var priv = 0; return function () { return priv++ } })(); mk(); R = mk()");
T("var a = (function () { return this })(); R = a === globalThis");
T("var a = (function () { 'use strict'; return this })(); R = a");
T("var a = (function () { return this }).call(1); R = typeof a");
T("var a = (function () { 'use strict'; return this }).call(1); R = typeof a");
T("var a = (() => this)(); R = a === globalThis");
T("R = (function f(n) { return n <= 1 ? 1 : n * f(n - 1) })(5)");
T("R = (function f() { return typeof f })()");
T("R = (function f() { var f = 1; return typeof f })()");
T("R = (function () { return (function () { return (function () { return 'deep' })() })() })()");
T("R = (function (undefined) { return typeof undefined })(1)");
T("R = (function (undefined) { return undefined })()");
T("R = (function () { var undefined = 1; return undefined })()");
T("var undefined; R = typeof undefined");
T("var NaN = 1; R = NaN");
T("var Infinity = 1; R = Infinity");
T("let undefined = 1");
T("var globalThis2 = globalThis; (function (globalThis) { R = typeof globalThis })(1)");
T("(function () { var Object = 1; R = typeof Object })()");
T("(function () { var Array = function () { return 'x' }; R = typeof [].map })()");
T("(function () { var r = []; for (var i = 0; i < 3; i++) (function (i) { r.push(function () { return i }) })(i); R = r.map(f => f()).join() })()");
T("(function () { var r = []; for (var i = 0; i < 3; i++) r.push((function (i) { return function () { return i } })(i)); R = r.map(f => f()).join() })()");
T("(function () { var r = []; for (var i = 0; i < 3; i++) r.push(function () { return i }); R = r.map(f => f()).join() })()");
T("(function () { var r = []; for (var i = 0; i < 3; i++) { var j = i; r.push(function () { return j }) } R = r.map(f => f()).join() })()");
T("(function () { var r = []; for (var i = 0; i < 3; i++) { r.push(((k) => () => k)(i)) } R = r.map(f => f()).join() })()");
T("R = (function () { if (true) { var v = 1 } return v })()");
T("R = (function () { if (false) { var v = 1 } return v })()");
T("R = (function () { return typeof v; var v = 1 })()");
T("R = (function () { return v; var v = 1 })()");
T("R = (function () { var v = 1; var v = 2; return v })()");
T("R = (function () { return typeof f; function f() {} })()");
T("R = (function () { return typeof f; var f = function () {} })()");
T("R = (function () { var f = 1; function f() {} return typeof f })()");
T("R = (function () { function f() { return 1 } function f() { return 2 } return f() })()");
T("R = (function () { f(); function f() { R = 'called' } })()");
T("R = (function () { return (function () { return f(); function f() { return 'in' } })() })()");
T("(function () { f(); function f() { R = 'before' } })()");
T("(function () { g(); var g = function () {} })()");
T("var f = 1; (function () { f = 2; function f() {} })(); R = f");
T("var f = 1; (function () { f = 2; var f })(); R = f");
T("var f = 1; (function (f) { f = 2 })(f); R = f");
T("(function () { var a = 1; { let a = 2; { let a = 3; R = a } } })()");
T("(function () { var a = 1; { let a = 2 } R = a })()");
T("(function () { let a = 1; { var b = 2 } R = a + b })()");
T("(function () { { let a = 1 } R = typeof a })()");
T("(function () { { const a = 1 } R = typeof a })()");
T("(function () { { class a {} } R = typeof a })()");
T("(function () { { function a() {} } R = typeof a })()");
T("(function () { if (true) { let a = 1 } R = typeof a })()");
T("(function () { for (let a = 0; a < 1; a++); R = typeof a })()");
T("(function () { for (var a = 0; a < 1; a++); R = typeof a })()");
T("(function () { try { var a = 1 } catch (e) {} R = a })()");
T("(function () { try { let a = 1 } finally { R = typeof a } })()");
T("(function () { switch (1) { case 1: var a = 1 } R = a })()");
T("(function () { switch (1) { case 1: let a = 1 } R = typeof a })()");
T("(function () { L: { var a = 1; break L } R = a })()");
T("(function () { do { var a = 1 } while (false); R = a })()");
T("(function () { while (!a) { var a = 1 } R = a })()");

// ---- Recursão mútua.
T("function isEven(n) { return n === 0 ? true : isOdd(n - 1) } function isOdd(n) { return n === 0 ? false : isEven(n - 1) } R = isEven(10) + ':' + isOdd(7)");
T("var isEven = n => n === 0 ? true : isOdd(n - 1), isOdd = n => n === 0 ? false : isEven(n - 1); R = isEven(11)");
T("let isEven = n => n === 0 ? true : isOdd(n - 1); let isOdd = n => n === 0 ? false : isEven(n - 1); R = isEven(12)");
T("let a = () => b(); let b = () => 1; R = a()");
T("let a = () => b(); try { a() } catch (e) { R = e.name + ': ' + e.message } let b = () => 1");
T("function a() { return b() } function b() { return c() } function c() { return 'c' } R = a()");
T("(function () { function a(n) { return n ? b(n - 1) : 'a' } function b(n) { return n ? a(n - 1) : 'b' } R = a(3) + b(3) })()");
T("(function () { var a = function (n) { return n ? b(n - 1) : 'a' }; var b = function (n) { return n ? a(n - 1) : 'b' }; R = a(4) })()");
T("(function () { const a = n => n ? b(n - 1) : 'a', b = n => n ? a(n - 1) : 'b'; R = a(5) })()");
T("class A { m(n) { return n ? B.m(n - 1) : 'A' } } class B { static m(n) { return n ? new A().m(n - 1) : 'B' } } R = new A().m(3)");
T("var o = { a(n) { return n ? this.b(n - 1) : 'a' }, b(n) { return n ? this.a(n - 1) : 'b' } }; R = o.a(5)");
T("function f(n) { return n ? g(n - 1) : 0 } var g = function (n) { return n ? f(n - 1) : 1 }; R = f(7)");
T("var f = function fact(n) { return n <= 1 ? 1 : n * fact(n - 1) }; R = f(6)");
T("var f = function fact(n) { return n <= 1 ? 1 : n * fact(n - 1) }, g = f; f = null; R = g(5)");
T("function depth(n) { return n ? 1 + depth(n - 1) : 0 } R = depth(1000)");
T("function depth(n) { return n ? 1 + depth(n - 1) : 0 } try { depth(1e7) } catch (e) { R = e.name }");
T("var a = n => n ? b(n - 1) : 'a'; var b = n => n ? a(n - 1) : 'b'; R = a(1000)");
T("function* g(n) { if (n) { yield n; yield* g(n - 1) } } R = [...g(5)].join()");
T("var ev = n => n === 0 || od(n - 1); var od = n => n !== 0 && ev(n - 1); R = ev(100)");
T("function a() { return typeof b } function b() { return typeof a } R = a() + b()");
T("function a() { return b } function b() { return a } R = a()() === a");
T("var memo = {}; function fib(n) { return n < 2 ? n : memo[n] || (memo[n] = fib(n - 1) + fib(n - 2)) } R = fib(40)");
T("var Y = f => (x => f(v => x(x)(v)))(x => f(v => x(x)(v))); R = Y(fn => n => n ? n * fn(n - 1) : 1)(5)");

// ---- Closures grandes (200 variáveis) e funções com 300 parâmetros.
{
  const n = 200;
  const decl = (kind, count) => Array.from({ length: count }, (_, i) => `${kind} v${i} = ${i};`).join(" ");
  const sumExpr = Array.from({ length: n }, (_, i) => `v${i}`).join(" + ");
  const sumOf = count => Array.from({ length: count }, (_, i) => `v${i}`).join(" + ");
  T(`(function () { ${decl("var", n)} return (function () { return ${sumExpr} })() })()`.replace(/^/, "R = "));
  T(`R = (function () { ${decl("let", n)} return () => ${sumExpr} })()()`);
  T(`R = (function () { ${decl("const", n)} return () => ${sumExpr} })()()`);
  T(`R = (function () { ${decl("var", n)} return () => { ${Array.from({ length: n }, (_, i) => `v${i}++;`).join(" ")} return ${sumExpr} } })()()`);
  T(`R = (function () { ${decl("let", n)} var fs = []; ${Array.from({ length: 5 }, (_, i) => `fs.push(() => v${i * 40})`).join("; ")}; return fs.map(f => f()).join() })()`);
  T(`R = (function () { ${decl("let", n)} return eval(${JSON.stringify(sumExpr)}) })()`);
  T(`R = (function () { ${decl("var", n)} return (() => eval(${JSON.stringify(sumExpr)}))() })()`);
  T(`R = (function () { ${decl("let", n)} { ${decl("let", 50).replace(/v(\d+)/g, "w$1")} return () => ${Array.from({ length: 50 }, (_, i) => `w${i} + v${i + 100}`).join(" + ")} } })()()`);
  T(`R = (function () { ${decl("let", n)} return (() => (() => (() => ${sumOf(n)})())())() })()`);
  T(`R = (function () { ${decl("var", n)} function h() { return ${sumOf(n)} } var v0 = 1000; return h() })()`);
  T(`R = (function () { ${decl("let", n)} let f = () => v199; ${"{ "}let v199 = 5; R = f() ${"}"} return R })()`);
  T(`R = (function () { return () => ${sumOf(n)}; ${decl("let", n)} })()()`);
  T(`var fs = []; for (let i = 0; i < 3; i++) { ${decl("let", n)} fs.push(() => i + ${sumOf(n)}) } R = fs.map(f => f()).join()`);
  T(`R = (function () { ${decl("var", n)} with ({ v5: 'w' }) { return (() => v5 + v6)() } })()`);
  T(`function* g() { ${decl("let", n)} yield () => ${sumOf(n)} } R = g().next().value()`);
  T(`async function g() { ${decl("let", n)} await null; return ${sumOf(n)} } g().then(v => { R = v })`);
  T(`class K { m() { ${decl("let", n)} return () => ${sumOf(n)} } } R = new K().m()()`);
  T(`R = (function () { ${decl("let", 300)} return () => ${sumOf(300)} })()()`);
  T(`R = (function () { ${decl("let", 1000)} return () => v0 + v999 })()()`);
  T(`R = (function () { ${decl("var", 1000)} return v0 + v999 })()`);
  T(`${decl("let", n)} R = (() => ${sumOf(n)})()`);
  T(`${decl("var", n)} R = ${sumOf(n)}`);
  T(`${decl("const", n)} R = ${J}([v0, v199])`);
  T(`R = (function () { ${decl("let", n)} return ${J}([typeof v0, typeof v199, typeof nope]) })()`);
  T(`R = (function () { var o = {}; ${decl("var", n)} return (function () { return arguments.length })(${sumOf(n).split(" + ").join(", ")}) })()`);
  for (const count of [255, 256, 257, 300, 500, 1000]) {
    const params = Array.from({ length: count }, (_, i) => `p${i}`).join(", ");
    const sum = Array.from({ length: count }, (_, i) => `p${i}`).join(" + ");
    const args = Array.from({ length: count }, (_, i) => i).join(", ");
    T(`function f(${params}) { return ${sum} } R = f(${args})`);
    T(`function f(${params}) { return arguments.length + ':' + f.length } R = f(${args})`);
    T(`function f(${params}) { return () => p0 + p${count - 1} } R = f(${args})()`);
    T(`function f(${params}) { return p${count - 1} } R = f(${args.split(", ").slice(0, count - 1).join(", ")})`);
    T(`var f = (${params}) => p${count - 1}; R = f(${args})`);
    T(`function f(${params}) { arguments[${count - 1}] = 'x'; return p${count - 1} } R = f(${args})`);
    T(`function f(${params}, ...rest) { return rest.length } R = f(${args}, 1, 2)`);
    T(`function f(${params}) { 'use strict'; return p${count - 1} } R = f(${args})`);
    T(`class K { m(${params}) { return p${count - 1} } } R = new K().m(${args})`);
    T(`function f(${params}) { return eval('p${count - 1}') } R = f(${args})`);
    T(`R = (new Function(${JSON.stringify(params)}, 'return p${count - 1}'))(${args})`);
    T(`function f(${params}) { var p0 = 'v'; return p0 } R = f(${args})`);
    T(`function f(p0 = 1, ${params.split(", ").slice(1).join(", ")}) { return p0 } R = f()`);
    T(`function f(${params}) { return Array.prototype.slice.call(arguments, ${count - 2}).join() } R = f(${args})`);
    T(`R = Math.max(${args})`);
    T(`R = [${args}].length`);
    T(`R = f.apply(null, [${args}]); function f(${params}) { return p${count - 1} }`);
  }
  // default em muitos parâmetros e destructuring grande
  const dparams = Array.from({ length: 300 }, (_, i) => `p${i} = ${i}`).join(", ");
  T(`function f(${dparams}) { return p0 + p299 } R = f()`);
  T(`function f(${dparams}) { return () => p150 } R = f(undefined, 5)()`);
  T(`function f(${dparams}) { var p5 = 'b'; return [p5, arguments.length] } R = ${J}(f())`);
  const dest = Array.from({ length: 300 }, (_, i) => `a${i}`).join(", ");
  T(`var [${dest}] = Array.from({ length: 300 }, (_, i) => i); R = a0 + a299`);
  T(`let [${dest}] = Array.from({ length: 300 }, (_, i) => i); R = (() => a0 + a299)()`);
  T(`var { ${dest} } = { ${Array.from({ length: 300 }, (_, i) => `a${i}: ${i}`).join(", ")} }; R = a0 + a299`);
}

// ---- Identificadores unicode e escapes.
const idents = [
  "\\u0061", "\\u{61}", "a\\u0062", "\\u0061b", "ñ", "日本語", "π", "Ω", "\\u03c0", "\\u{3c0}", "ünï", "a\\u200d", "a\\u200c", "𠮷", "\\u{20bb7}",
  "ℵ", "ⅷ", "a·b", "a\\u00b7", "_$", "$", "_", "\\u{1d7d8}a", "a\\u{1d7d8}", "ª", "µ", "ǅ", "ʰ", "𝒜", "ꙮ", "\\u0131", "İ", "ß", "Σ", "ς", "σ",
];
for (const id of idents) {
  S(`var ${id} = 1`);
  T(`var ${id} = 1; R = typeof ${id}`);
  T(`let ${id} = 'v'; R = (() => ${id})()`);
  T(`function ${id}() { return 'f' } R = ${id}()`);
  T(`var o = { ${id}: 1 }; R = ${J}(Object.keys(o))`);
  T(`class ${id} { static m() { return ${id}.name } } R = ${id}.m()`);
  T(`var ${id} = 5; R = eval(${JSON.stringify(id)})`);
  T(`function f(${id}) { return ${id} } R = f(3)`);
  T(`var ${id} = 1; R = globalThis.hasOwnProperty(${JSON.stringify(id.replace(/\\u\{([0-9a-f]+)\}/g, (_, h) => String.fromCodePoint(parseInt(h, 16))).replace(/\\u([0-9a-f]{4})/g, (_, h) => String.fromCharCode(parseInt(h, 16))))})`);
  T(`var ${id} = 1; R = (function ${id}() { return ${id} })() === ${id}`);
}
S("var \\u{61");
S("var \\u0061\\u");
S("var \\u0020");
S("var \\u{20}");
S("var a\\u0020b");
S("var \\u{110000}");
S("var \\u{0}");
S("var \\u00");
S("var \\u{}");
S("var \\ud800");
S("var \\ud800\\udc00");
S("var \\u{d800}");
S("var \\u0030");
S("var a\\u0030");
S("var \\u{30}a");
S("var \\u0061wait");
S("var aw\\u0061it");
S("var l\\u0065t");
S("var v\\u0061r");
S("v\\u0061r x = 1");
S("var \\u0076ar");
S("'use strict'; var l\\u0065t");
S("'use strict'; var yi\\u0065ld");
S("var yi\\u0065ld");
S("function* g() { var yi\\u0065ld }");
S("function* g() { yi\\u0065ld 1 }");
S("async function f() { aw\\u0061it 1 }");
S("async function f() { var aw\\u0061it }");
S("var a\\u{200d}b");
S("var \\u{200d}b");
S("var a\\u{2e2f}");
S("var \\u2e2f");
S("var 1a");
S("var a\\u{1}");
S("var \\u{10ffff}");
S("var \\u{1D7D8}");
S("var ℮");
S("var ゛");
S("var \\u180e");
S("var \\u00a0");
S("var a\\u00a0b");
S("var \\ufeff");
S("var \\u2028");
S("class \\u0041 {}");
S("({ \\u0061: 1 })");
S("({ g\\u0065t x() {} })");
S("({ \\u0067et x() {} })");
S("({ s\\u0065t x(v) {} })");
S("({ \\u0061sync m() {} })");
S("({ as\\u0079nc m() {} })");
S("class A { st\\u0061tic m() {} }");
S("class A { static \\u0067et x() {} }");
S("class A { \\u0073tatic m() {} }");
S("var o = { v\\u0061r: 1 }; o.v\\u0061r");
S("var o = {}; o.v\\u0061r; o.cl\\u0061ss; o.n\\u0065w");
S("new.t\\u0061rget");
S("function f() { new.t\\u0061rget }");
S("function f() { n\\u0065w.target }");
S("({ if: 1 }).\\u0069f");
S("\\u0069f (1) ;");
S("\\u0074rue");
S("var \\u0074rue");
S("x = \\u006eull");
S("typ\\u0065of 1");
S("1 \\u0069n {}");
S("1 \\u0069nstanceof Object");
S("for (var x \\u006ff []) ;");
S("for (var x o\\u0066 []) ;");
S("var a = 1; a \\u0069s 2");
S("let \\u0061 = 1; a");
S("l\\u0065t a = 1");
S("l\\u0065t\n a");
S("var l\\u0065t = 1; l\\u0065t");
S("'use strict'; l\\u0065t");
S("for (l\\u0065t x of []);");
S("for (l\\u0065t in {});");
S("async \\u0066unction f() {}");
S("\\u0061sync function f() {}");
S("(\\u0061sync () => 1)");
S("(\\u0061sync x => 1)");
S("\\u0061wait: 1");
S("var \\u0061sync; \\u0061sync\n function f() {}");
S("var \\u006ff = 1");
S("var o\\u0066 = 1; for (o\\u0066 of []);");
S("var o\\u0066 = 1; for (var x o\\u0066 []);");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "scope-golden-"));
// O bun passa arquivos pelo transpilador próprio (muda a semântica de script do JSC: função em bloco, var/let em eval,
// early errors). `vm.runInThisContext` roda como ProgramExecutable do JSC puro, então o programa vai por ele; o
// SyntaxError de compilação é engolido e `R` fica indefinido ("<undefined>").
const source_file = path.join(dir, "scope_source.js");
const file = path.join(dir, "scope_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
