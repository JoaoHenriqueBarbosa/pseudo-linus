// Gera tests/golden/eval_scope_bun.tsv: complemento de gen-eval-golden.js, medido no bun 1.4.2. Cobre eval direto e
// indireto (vazamento de var, strict, let/const, arguments, this, new.target), `new Function`/GeneratorFunction/
// AsyncFunction (corpo, parâmetros com comentários, toString), `with` + Symbol.unscopables, hoisting de function em
// bloco (Annex B), `arguments` mapeado e não mapeado, closures em laços, getters e setters em literais e classes,
// label + break em blocos, vírgula e `void`/`typeof`/`delete` de borda. Programas já presentes em eval_bun.tsv ou
// scope_bun.tsv são descartados. Cada programa roda como script (vm.runInThisContext), num processo novo, e grava o
// texto em `globalThis.R`. Colunas: a fonte (JSON) e o valor de `R` (JSON).
// Uso: bun scripts/gen-eval-scope-golden.js > tests/golden/eval_scope_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const q = JSON.stringify;
const show = expr => `try { globalThis.R = String(${expr}) } catch (e) { globalThis.R = e.name + ': ' + e.message }`;
const run = stmts => `try { ${stmts} } catch (e) { globalThis.R = e.name + ': ' + e.message }`;
// Programa com corpo livre que termina atribuindo R; exceções viram "Nome: mensagem".
const body = stmts => run(stmts);
const ev = code => `eval(${q(code)})`;
const ie = code => `(0, eval)(${q(code)})`;

// ---- 1. eval direto vs indireto: vazamento de var, strict, let/const, function.
const evalSnippets = [
  "var zz = 1", "let zz = 1", "const zz = 1", "function zz() {}", "class zz {}", "'use strict'; var zz = 1",
  "var zz = 1; let ww = 2", "function zz() { return 1 } var yy = zz()", "var zz; var zz = 3", "{ var zz = 4 }", "{ function zz() {} }",
  "if (true) { function zz() {} }", "for (var zz = 0; zz < 2; zz++);", "try { throw 1 } catch (zz) { var zz = 5 }",
  "label: { var zz = 6; break label }", "var zz = typeof zz", "var zz = this === globalThis",
];
for (const code of evalSnippets) {
  for (const mode of ["direct-sloppy", "indirect-sloppy", "direct-strict", "indirect-strict", "function-direct", "function-strict"]) {
    let src;
    if (mode === "direct-sloppy") src = `${ev(code)}; globalThis.R = typeof zz + ',' + typeof ww`;
    else if (mode === "indirect-sloppy") src = `${ie(code)}; globalThis.R = typeof zz + ',' + typeof ww`;
    else if (mode === "direct-strict") src = `'use strict'; ${ev(code)}; globalThis.R = typeof zz + ',' + typeof ww`;
    else if (mode === "indirect-strict") src = `'use strict'; ${ie(code)}; globalThis.R = typeof zz + ',' + typeof ww`;
    else if (mode === "function-direct") src = `(function () { ${ev(code)}; globalThis.R = typeof zz + ',' + typeof ww })()`;
    else src = `(function () { 'use strict'; ${ev(code)}; globalThis.R = typeof zz + ',' + typeof ww })()`;
    add(body(src));
  }
}
const evalValues = [
  "1;", "1; var a;", "var a = 1", "var a = 1; 2", "if (true) 3; else 4", "do { 5; break } while (0)", "L: { 6; break L; 7 }",
  "for (var i = 0; i < 3; i++) i", "switch (1) { case 1: 8; break; default: 9 }", "try { 10 } finally { 11 }", "try { throw 1 } catch (e) { 12 }",
  "while (false);", "7; if (false) 8", "7; do { 8; continue } while (false)", "9; var x = 1", "'a'; function f() {}", "10; {}", "11; ;", "12; L: ;",
  "with ({}) 13", "14; with ({}) ;", "15; for (var k in {a: 1}) k", "16; for (var k of []) k", "({}).x", "[1, 2, 3]", "(1, 2, 3)", "void 0", "typeof 1",
];
for (const code of evalValues) {
  add(show(ev(code)));
  add(show(ie(code)));
  add(show(`(function () { 'use strict'; return ${ev(code)} })()`));
}
// this, arguments, new.target e super dentro de eval em vários contextos.
const evalNames = ["this", "arguments.length", "typeof arguments", "new.target", "typeof new.target", "arguments[0]", "typeof this", "super.x"];
const evalContexts = [
  c => ev(c),
  c => ie(c),
  c => `(function () { return ${ev(c)} })(7, 8)`,
  c => `(function () { return ${ie(c)} })(7, 8)`,
  c => `(function () { 'use strict'; return ${ev(c)} })(7, 8)`,
  c => `(() => ${ev(c)})()`,
  c => `(function () { return (() => ${ev(c)})() }).call(5, 7)`,
  c => `new (function () { this.v = ${ev(c)} })().v`,
  c => `({ m() { return ${ev(c)} } }).m(3)`,
  c => `({ __proto__: { x: 'px' }, m() { return ${ev(c)} } }).m()`,
  c => `new (class { constructor() { this.v = ${ev(c)} } })().v`,
  c => `(function* () { yield ${ev(c)} })().next().value`,
];
for (const name of evalNames) for (const ctx of evalContexts) add(show(ctx(name)));
// eval como valor e como referência.
for (const code of [
  "var e = eval; e('typeof zz')", "var e = eval; e('var zz = 1'); typeof zz", "var o = { eval }; o.eval('var zz = 1'); typeof zz",
  "(eval)('var zz = 1'); typeof zz", "(0 || eval)('var zz = 1'); typeof zz", "eval?.('var zz = 1'); typeof zz", "(eval?.('var zz = 1')); typeof zz",
  "eval('var zz = 1'); typeof zz", "eval.call(null, 'var zz = 1'); typeof zz", "eval.apply(null, ['var zz = 1']); typeof zz",
  "Reflect.apply(eval, null, ['var zz = 1']); typeof zz", "new.target", "eval(); typeof eval()", "eval(1, 2)", "eval({})", "eval(['1+1'])",
  "eval(new String('1+1'))", "eval(Symbol())", "eval(1n)", "eval(undefined)", "eval(null)", "eval(true)", "eval('')", "eval(' ')",
  "eval('/*c*/')", "eval('//c')", "eval('\\n')", "eval('a b')", "eval('{')", "eval('1 +')", "eval('var')", "eval('let let = 1')",
  "eval('yield')", "eval('await')", "eval('await 1')", "eval('return 1')", "eval('break')", "eval('continue')", "eval('super()')",
  "eval('super.x')", "eval('import.meta')", "eval('import(\"x\")')", "eval('x => x')(1)", "eval('(x) => x * 2')(4)", "eval('({a:1}).a')",
]) add(show(`(function () { ${code} })()`));

// ---- 2. Conflitos de declaração em eval.
for (const [outer, inner] of [
  ["let a = 1", "var a = 2"], ["const a = 1", "var a = 2"], ["var a = 1", "let a = 2"], ["var a = 1", "const a = 2"], ["let a = 1", "function a() {}"],
  ["function a() {}", "let a = 2"], ["class a {}", "var a"], ["let a", "{ var a }"], ["let a", "if (1) { var a }"], ["let a", "for (var a of []);"],
  ["let a", "try {} catch (e) { var a }"], ["var a", "var a"], ["", "let a = 1; var a"], ["", "var a; let a"], ["", "function f() {} let f"],
  ["", "let a; { var a }"], ["", "const a = 1; a = 2"],
]) {
  add(body(`(function () { ${outer}; ${ev(inner)}; globalThis.R = 'ok' })()`));
  add(body(`${outer}; ${ev(inner)}; globalThis.R = 'ok'`));
  add(body(`{ ${outer}; ${ev(inner)}; globalThis.R = 'ok' }`));
}
for (const catchVar of ["e", "[e]", "{ e }"]) {
  add(body(`try { throw [1] } catch (${catchVar}) { eval('var e = 2'); globalThis.R = String(typeof e) }`));
  add(body(`try { throw [1] } catch (${catchVar}) { eval('let e = 2'); globalThis.R = String(e) }`));
}
add(body("function f(a) { eval('var a = 2'); return a } globalThis.R = f(1)"));
add(body("function f(a = 1) { eval('var a = 2'); return a } globalThis.R = f()"));
add(body("function f(a, b = eval('var a = 3')) { return a } globalThis.R = f(1)"));
add(body("function f(a = eval('var z = 3'), b = z) { return b } globalThis.R = f()"));
add(body("function f(a = eval('var z = 3')) { var z; return z } globalThis.R = f()"));
add(body("function f(a = () => z) { var z = 1; return a() } globalThis.R = f()"));
add(body("var z = 'outer'; function f(a = () => z) { var z = 1; return a() } globalThis.R = f()"));
add(body("var z = 'outer'; function f(a = () => z) { eval('var z = 1'); return a() } globalThis.R = f()"));
add(body("function f() { eval('var arguments = 1'); return arguments } globalThis.R = f(9)"));
add(body("function f() { eval('var arguments = 1') } f(); globalThis.R = 'ok'"));
add(body("function f() { 'use strict'; eval('var arguments = 1') } f(); globalThis.R = 'ok'"));
add(body("function f() { 'use strict'; eval('arguments = 1') } f(); globalThis.R = 'ok'"));
add(body("function f() { eval('arguments = 1'); return arguments } globalThis.R = f(2)"));
add(body("function f(a) { eval('arguments[0] = 9'); return a } globalThis.R = f(1)"));
add(body("function f(a) { 'use strict'; eval('arguments[0] = 9'); return a } globalThis.R = f(1)"));
add(body("function f(a) { eval('a = 9'); return arguments[0] } globalThis.R = f(1)"));
add(body("var eval = 1; globalThis.R = typeof eval"));
add(body("'use strict'; var eval = 1"));
add(body("'use strict'; eval = 1"));
add(body("'use strict'; function eval() {}"));
add(body("'use strict'; function f(eval) {}"));
add(body("'use strict'; function f(arguments) {}"));
add(body("'use strict'; (eval) => 1"));
add(body("'use strict'; ({ eval } = {})"));
add(body("'use strict'; try {} catch (eval) {}"));
add(body("function f() { var eval; return typeof eval } globalThis.R = f()"));
add(body("function f() { var eval = 'x'; return eval('1') } globalThis.R = f()"));
add(body("function f(eval) { return eval('1+1') } globalThis.R = f(function (s) { return 'shadow:' + s })"));
add(body("var o = { eval: function (s) { return 'o:' + s } }; with (o) { globalThis.R = eval('1+1') }"));
add(body("var o = { eval: function (s) { return 'o:' + s } }; with (o) { var zz; eval('var zz = 7') } globalThis.R = typeof zz"));
add(body("with ({ eval: undefined }) { globalThis.R = eval('1') }"));

// ---- 3. new Function / GeneratorFunction / AsyncFunction / AsyncGeneratorFunction.
const GF = "Object.getPrototypeOf(function* () {}).constructor";
const AF = "Object.getPrototypeOf(async function () {}).constructor";
const AGF = "Object.getPrototypeOf(async function* () {}).constructor";
const ctors = [["Function", "Function"], ["GeneratorFunction", GF], ["AsyncFunction", AF], ["AsyncGeneratorFunction", AGF]];
const fnArgs = [
  [], ["a"], ["a", "b"], ["a,b"], ["a, b"], ["a,b", "c"], ["a", "b,c"], ["...a"], ["a", "...b"], ["a = 1"], ["a", "b = a"], ["[a]"], ["{a}"], ["{a}", "[b]"],
  ["a /*c*/"], ["/*c*/ a"], ["a //c\n"], ["a", "//c\nb"], ["a /* , */ , b"], ["a,", "b"], ["a,"], ["", "a"], [""], [" "], ["\n"], ["a", ""],
  ["a=1", "b=2"], ["a-"], ["1"], ["a b"], ["a;"], ["a)"], ["a) {"], ["}"], ["a", "}"], ["/*"], ["a = /*"], ["eval"], ["arguments"], ["yield"], ["await"],
  ["let"], ["static"], ["a", "a"], ["a, a"], ["'use strict'"], ["a", "a, a"], ["this"], ["new.target"], ["super"], ["a = new.target"], ["a = super.x"], ["a = this"],
];
const fnBodies = [
  "", "return 1", "return a", "return a + b", ";", "}", "} {", "{", "/*", "//c", "//c\nreturn 3", "return this", "return typeof this", "'use strict'; return this",
  "return arguments.length", "return new.target", "return typeof new.target", "yield 1", "await 1", "return await 1", "yield", "return yield", "var a = 1", "let a = 1",
  "return typeof a", "return eval('typeof a')", "'use strict'; var a", "super.x", "super()", "import.meta", "return (a, b)", "return 1 }", "} return 1", "-->", "<!--",
  "-->\nreturn 1", "return 1\n-->", "'use strict'; with ({}) ;", "return a =>", "return `${1}`", "return function () { return this }()",
];
for (const [name, ctor] of ctors) {
  const call = (args, b) => `new (${ctor})(${[...args.map(q), q(b)].join(", ")})`;
  for (const args of fnArgs.slice(0, 20)) add(show(`${call(args, "return 1")}.toString()`));
  for (const args of fnArgs.slice(20)) add(show(`${call(args, "return 1")}.toString()`));
  for (const b of fnBodies) add(show(`${call(["a", "b"], b)}.toString()`));
  for (const b of fnBodies.slice(0, 18)) add(show(`${call([], b)}.name + '|' + ${call([], b)}.length`));
  add(show(`(${ctor})('a', 'return a').constructor === ${ctor}`));
  add(show(`Object.getPrototypeOf(${call(["a"], "return a")}) === ${ctor}.prototype`));
  add(show(`${ctor}.name + ':' + ${ctor}.length`));
  add(show(`${ctor}().toString()`));
  add(show(`typeof ${ctor}()`));
  add(show(`Object.prototype.toString.call(${ctor}())`));
  add(show(`${call(["a"], "return a")}.hasOwnProperty('prototype')`));
  add(show(`typeof ${call([], "")}.prototype`));
  add(show(`Object.getOwnPropertyNames(${call(["a", "b"], "")}).join()`));
}
for (const [name, ctor] of ctors.slice(0, 2)) {
  add(show(`${ctor}('a', 'b', 'return a + b')(1, 2)`));
  add(show(`${ctor}('a, b', 'return a * b')(3, 4)`));
  add(show(`${ctor}('...r', 'return r.length')(1, 2, 3)`));
  add(show(`${ctor}('a = 5', 'return a')()`));
  add(show(`${ctor}('{x, y}', 'return x - y')({ x: 9, y: 4 })`));
}
for (const code of [
  "new Function('return this')() === globalThis", "new Function('\"use strict\"; return this')()", "typeof new Function('return typeof arguments')()",
  "new Function('return new.target')()", "new (new Function('this.a = new.target === undefined'))().a", "new Function('a', 'return arguments.length')(1, 2, 3)",
  "new Function('a', 'a = 5; return arguments[0]')(1)", "new Function('a', '\"use strict\"; a = 5; return arguments[0]')(1)",
  "var zz = 'g'; new Function('return typeof zz')()", "(function () { var zz = 'l'; return new Function('return typeof zz')() })()",
  "new Function('var zz = 1; return typeof zz')() + typeof zz", "new Function('return eval(\"var zz = 1\"), typeof zz')() + typeof zz",
  "Function.prototype.constructor === Function", "Function('return 1').call.length", "(new Function).toString()", "new Function().length",
  "new Function('a', 'b', 'return a').length", "new Function('a = 1', 'b', 'return a').length", "new Function('...a', '').length", "new Function('a', '').name",
  "new Function('a', '').bind().name", "(function () {}).constructor === Function", "(class {}).constructor === Function", "new Function('return 1').caller === null",
  "Object.getOwnPropertyDescriptor(new Function(), 'name').value", "Object.getOwnPropertyDescriptor(new Function(), 'name').configurable",
  "Reflect.ownKeys(new Function()).join()", "Function('a', 'b', 'c', 'return a+b+c')(1, 2, 3)", "Function.apply(null, ['a', 'return a * 2'])(4)",
  "Function.call(null, 'return 8')()", "Reflect.construct(Function, ['return 1'])()", "Reflect.construct(Function, ['return 1'], Array)() instanceof Array",
  "Object.getPrototypeOf(Reflect.construct(Function, ['return 1'], Array)) === Array.prototype", "new Function(`/*${'x'}*/`).toString()",
  "new Function('a', 'return a', 'extra')(1)", "new Function(1, 2, 3)", "new Function(null)", "new Function(undefined)", "new Function({toString() { return 'return 4' }})()",
  "new Function({ toString() { return 'a' } }, 'return a')(6)", "new Function('a', { toString() { throw new RangeError('boom') } })",
  "new Function(Symbol())", "new Function('a', Symbol())", "new Function(1n)()", "new Function('return 1n')()",
]) add(show(code));

// ---- 4. with + Symbol.unscopables e escopo de objeto.
const scopeObjs = [
  "{ a: 1 }", "{ a: 1, [Symbol.unscopables]: { a: true } }", "{ a: 1, [Symbol.unscopables]: { a: false } }", "{ a: 1, [Symbol.unscopables]: { a: 1 } }",
  "{ a: 1, [Symbol.unscopables]: { a: 0 } }", "{ a: 1, [Symbol.unscopables]: { a: '' } }", "{ a: 1, [Symbol.unscopables]: { a: 'x' } }",
  "{ a: 1, [Symbol.unscopables]: null }", "{ a: 1, [Symbol.unscopables]: 1 }", "{ a: 1, [Symbol.unscopables]: undefined }", "{ a: 1, [Symbol.unscopables]: 'a' }",
  "{ a: 1, [Symbol.unscopables]: { b: true } }", "{ a: 1, [Symbol.unscopables]: { __proto__: { a: true } } }",
  "{ a: 1, get [Symbol.unscopables]() { return { a: true } } }", "{ a: 1, get [Symbol.unscopables]() { throw new Error('u') } }",
  "{ get a() { return 'g' } }", "{ set a(v) {} }", "{ a: undefined }", "{ a: null }", "Object.create({ a: 'inherited' })", "Object.create({ a: 1 }, { [Symbol.unscopables]: { value: { a: true } } })",
  "Object.create(null, { a: { value: 1 } })", "new Proxy({}, { has: (t, k) => k === 'a', get: (t, k) => k === Symbol.unscopables ? undefined : 'px' })",
  "new Proxy({ a: 1 }, { has: () => false })", "new Proxy({}, { has: (t, k) => { (globalThis.L = globalThis.L || []).push(String(k)); return false } })",
  "[10, 20]", "'str'", "new String('ab')", "Array.prototype", "function () {}", "Math", "new Number(5)", "new Boolean(false)",
];
const withBodies = [
  "typeof a", "a", "a = 2; typeof a + ',' + String(this.a)", "(function () { return a })()", "(() => a)()", "var a = 9; typeof a", "var a; String(a)",
  "function a() {}; typeof a", "delete a", "a++; typeof a", "typeof globalThis.a", "String(a) + typeof b", "eval('a')", "eval('var a = 3'); typeof a",
  "let b = a; b", "a?.x", "a ??= 7; String(a)", "a += 1; String(a)",
];
for (const obj of scopeObjs.slice(0, 14)) for (const b of withBodies) add(show(`(function () { var o = ${obj}; with (o) { return (${q(b)}, eval(${q(b)})) } })()`));
for (const obj of scopeObjs.slice(14)) for (const b of withBodies.slice(0, 6)) add(show(`(function () { var o = ${obj}; with (o) { return eval(${q(b)}) } })()`));
add(body("with ({ a: 1 }) { var f = function () { return a } } globalThis.R = f() + ',' + typeof a"));
add(body("var o = { a: 1 }; with (o) { var f = function () { return a } } delete o.a; globalThis.R = typeof f()"));
add(body("var o = { a: 1 }; with (o) { var f = function () { return a } } o.a = 5; globalThis.R = f()"));
add(body("var o = { a: 1 }; with (o) { a = 2; var a = 3 } globalThis.R = o.a + ',' + a"));
add(body("var o = { a: 1 }; with (o) { function g() { return a } } globalThis.R = g()"));
add(body("with ({ x: 1 }) with ({ y: 2 }) globalThis.R = x + y"));
add(body("with ({ x: 1 }) with ({ x: 2 }) globalThis.R = x"));
add(body("with (null) {}"));
add(body("with (undefined) {}"));
add(body("with (1) { globalThis.R = typeof toFixed }"));
add(body("with ('abc') { globalThis.R = length }"));
add(body("with (Symbol()) { globalThis.R = typeof description }"));
add(body("with ({}) { globalThis.R = typeof toString }"));
add(body("with ({ f() { return this } }) { globalThis.R = String(f() === undefined) }"));
add(body("var o = { f() { return this === o } }; with (o) { globalThis.R = f() }"));
add(body("var o = { f() { return this === o } }; with (o) { globalThis.R = (f)() }"));
add(body("var o = { f() { return this === o } }; with (o) { globalThis.R = (0, f)() }"));
add(body("var o = { f() { return this === o } }; with (o) { globalThis.R = f.call(o) }"));
add(body("var o = { f() { return this === o } }; with (o) { globalThis.R = `${f()}` }"));
add(body("var o = { f() { return typeof this } }; with (o) { globalThis.R = f?.() }"));
add(body("'use strict'; with ({}) {}"));
add(body("function f() { 'use strict'; with ({}) {} }"));
add(body("with ({ a: 1 }) { let a = 2; globalThis.R = a }"));
add(body("with ({ a: 1 }) { const a = 2; globalThis.R = a }"));
add(body("var log = []; var p = new Proxy({ a: 1 }, { has(t, k) { log.push('has:' + String(k)); return k in t }, get(t, k) { log.push('get:' + String(k)); return t[k] } }); with (p) { a } globalThis.R = log.join()"));
add(body("var log = []; var p = new Proxy({ a: 1 }, { has(t, k) { log.push('has:' + String(k)); return k in t }, get(t, k) { log.push('get:' + String(k)); return t[k] }, set(t, k, v) { log.push('set:' + String(k)); t[k] = v; return true } }); with (p) { a = 2 } globalThis.R = log.join()"));
add(body("var log = []; var p = new Proxy({ a: 1 }, { has(t, k) { log.push('has:' + String(k)); return k in t }, get(t, k) { log.push('get:' + String(k)); return t[k] }, set(t, k, v) { log.push('set:' + String(k)); t[k] = v; return true } }); with (p) { a += 1 } globalThis.R = log.join()"));
add(body("var log = []; var p = new Proxy({ a: 1 }, { has(t, k) { log.push('has:' + String(k)); return k in t }, get(t, k) { log.push('get:' + String(k)); return t[k] }, deleteProperty(t, k) { log.push('del:' + String(k)); return delete t[k] } }); with (p) { delete a } globalThis.R = log.join()"));
add(body("var log = []; var p = new Proxy({ a: 1 }, { has(t, k) { log.push('has:' + String(k)); return k in t }, get(t, k) { log.push('get:' + String(k)); return t[k] } }); with (p) { typeof a; typeof zzz } globalThis.R = log.join()"));
add(body("var log = []; var p = new Proxy({ a: 1 }, { has(t, k) { log.push('has:' + String(k)); return k in t }, get(t, k) { log.push('get:' + String(k)); return t[k] } }); with (p) { a(); } globalThis.R = log.join()"));
add(body("var o = { a: 1 }; with (o) { a = (delete o.a, 2) } globalThis.R = JSON.stringify(o) + typeof a"));
add(body("var o = { a: 1 }; with (o) { a = (Object.freeze(o), 2) } globalThis.R = JSON.stringify(o)"));
add(body("'use strict'; var o = { a: 1 }; eval('with (o) {}')"));
add(body("var o = { a: 1 }; (function () { 'use strict'; eval('var q = 1') })(); globalThis.R = typeof q"));
add(body("var o = { a: 1 }; (0, eval)('with (o) { var vv = a }'); globalThis.R = typeof vv"));
add(show("Object.keys(Array.prototype[Symbol.unscopables]).sort().join() + Object.getPrototypeOf(Array.prototype[Symbol.unscopables])"));
add(body("with ([]) { globalThis.R = typeof values + typeof keys + typeof push + typeof flat + typeof at }"));
add(body("with ([]) { globalThis.R = typeof includes + typeof findLast + typeof toSorted + typeof copyWithin + typeof fill }"));
add(body("var values = 'outer'; with ([1]) { globalThis.R = values }"));
add(body("var length = 'outer'; with ([1, 2]) { globalThis.R = length }"));

// ---- 5. Hoisting de function em bloco (Annex B).
const annexB = [
  "{ function f() { return 1 } } typeof f", "typeof f; { function f() {} }", "{ function f() {} } { function f() { return 2 } } f()", "if (true) function f() {} typeof f",
  "if (false) function f() {} typeof f", "if (false) function f() {}; f", "if (1) function f() { return 1 } else function f() { return 2 } f()",
  "if (0) function f() { return 1 } else function f() { return 2 } f()", "let f = 1; { function f() {} } typeof f", "{ let f = 1; { function f() {} } } typeof f",
  "var f = 1; { function f() {} } typeof f", "var f = 1; { function f() {} f = 2 } f", "{ function f() {} f = 2 } typeof f", "{ function f() {} f = 2; } f()",
  "{ f = 3; function f() {} } typeof f", "{ function f() { return 1 } function f() { return 2 } } f()", "switch (1) { case 1: function f() { return 'a' } } f()",
  "switch (2) { case 1: function f() {} } typeof f", "switch (1) { case 0: function f() {} case 1: f() }", "try { function f() { return 1 } } finally {} f()",
  "try { throw 0 } catch (e) { function f() {} } typeof f", "try { throw 0 } catch (f) { { function f() {} } } typeof f", "for (var i = 0; i < 1; i++) { function f() { return i } } f()",
  "for (let i = 0; i < 2; i++) { function f() { return i } } f()", "for (const k of [1]) { function f() { return k } } f()", "L: { function f() {} } typeof f",
  "L: function f() {} typeof f", "{ L: function f() {} } typeof f", "while (false) { function f() {} } typeof f", "do { function f() {} } while (false); typeof f",
  "{ function f() {} } delete f", "{ function f() {} } Object.getOwnPropertyDescriptor(globalThis, 'f') === undefined", "{ function* f() {} } typeof f", "{ async function f() {} } typeof f",
  "{ async function* f() {} } typeof f", "{ class f {} } typeof f", "{ function arguments() {} } typeof arguments", "{ function eval() {} } typeof eval", "{ function undefined() {} } typeof undefined",
  "{ function NaN() {} } typeof NaN", "{ { function f() {} } } typeof f", "{ { function f() {} } function f() { return 1 } } f()", "{ function f() {} { let f; } } typeof f",
  "{ function f() {} { var f; } } typeof f", "{ var f; { function f() {} } } typeof f", "{ function f(a) {} } f.length", "{ function f() {} } f.name",
  "typeof f; { function f() {} } typeof f", "var r = typeof f; { function f() {} } r + typeof f", "var r = f; { function f() {} } String(r)",
  "{ function f() { return typeof g } function g() {} } f()", "{ function f() { return typeof f } } f()", "{ function f() { f = 1; return typeof f } } f()",
  "{ function f() {} } { f = 1 } typeof f", "if (true) { function f() { return 'x' } } f()", "if (true) { function f() { return 'x' } } else { function f() { return 'y' } } f()",
  "function g() { { function f() { return 1 } } return typeof f } g()", "function g() { var r = typeof f; { function f() {} } return r + typeof f } g()",
  "function g(f) { { function f() {} } return typeof f } g(1)", "function g(f) { { function f() {} } return f } g(1)", "function g() { let f = 1; { function f() {} } return typeof f } g()",
  "function g() { 'use strict'; { function f() {} } return typeof f } g()", "'use strict'; { function f() {} } typeof f", "'use strict'; if (true) function f() {}",
  "'use strict'; L: function f() {}", "function g() { { function f() { return 1 } } { function f() { return 2 } } return f() } g()",
  "function g() { { function arguments() {} } return typeof arguments } g()", "function g() { { function g() {} } return typeof g } g()",
  "(function f() { { function f() {} } return typeof f })()", "(function f() { var r = typeof f; { function f() {} } return r })()", "(function () { { function f() {} } return eval('typeof f') })()",
  "(function () { eval('{ function f() {} }'); return typeof f })()", "eval('{ function f() {} }'); typeof f", "(0, eval)('{ function f() {} }'); typeof f",
  "(function () { 'use strict'; eval('{ function f() {} }'); return typeof f })()", "(() => { { function f() {} } return typeof f })()", "({ m() { { function f() {} } return typeof f } }).m()",
  "(class { static m() { { function f() {} } return typeof f } }).m()", "(function () { { function f() {} } return Object.getOwnPropertyNames(this).includes('f') }).call({})",
];
for (const code of annexB) {
  add(show(`(function () { return eval(${q(code)}) })()`));
  add(show(`eval(${q(code)})`));
}

// ---- 6. arguments mapeado vs não mapeado.
const argBodies = [
  "a = 9; return arguments[0]", "arguments[0] = 9; return a", "arguments[1] = 9; return b", "b = 9; return arguments[1]", "arguments.length = 0; return a",
  "delete arguments[0]; arguments[0] = 5; return a", "delete arguments[0]; a = 5; return arguments[0]", "Object.defineProperty(arguments, '0', { value: 7 }); return a",
  "Object.defineProperty(arguments, '0', { value: 7, writable: false }); a = 8; return arguments[0]", "Object.defineProperty(arguments, '0', { writable: false }); a = 8; return arguments[0]",
  "Object.defineProperty(arguments, '0', { writable: false }); arguments[0] = 8; return a", "Object.defineProperty(arguments, '0', { get() { return 'g' } }); a = 2; return arguments[0]",
  "Object.defineProperty(arguments, '0', { enumerable: false }); a = 2; return arguments[0]", "Object.freeze(arguments); a = 2; return arguments[0]", "Object.seal(arguments); a = 2; return arguments[0]",
  "Object.preventExtensions(arguments); a = 2; return arguments[0]", "return Object.getOwnPropertyDescriptor(arguments, '0').writable", "return Object.getOwnPropertyNames(arguments).join()",
  "return Object.keys(arguments).join()", "return JSON.stringify(arguments)", "return typeof arguments.callee", "return arguments.callee === f", "return arguments.length",
  "return Object.prototype.toString.call(arguments)", "return arguments[Symbol.iterator] === Array.prototype[Symbol.iterator]", "return [...arguments].join()",
  "return Array.prototype.slice.call(arguments).join()", "return Array.from(arguments).length", "return Object.getOwnPropertyDescriptor(arguments, 'callee').configurable",
  "return Object.getOwnPropertyDescriptor(arguments, 'length').enumerable", "return Object.getOwnPropertyNames(arguments).length", "a = 1; arguments[0] = 2; return a + ',' + arguments[0]",
  "var arguments = 5; return arguments", "var arguments; return typeof arguments", "function arguments() {} return typeof arguments", "arguments = 3; return arguments", "return (() => arguments[0])()",
  "return (() => { a = 9; return arguments[0] })()", "return eval('arguments[0]')", "eval('a = 9'); return arguments[0]", "return (function () { return arguments.length })()",
  "return Reflect.ownKeys(arguments).map(String).join()", "return arguments.hasOwnProperty('0') + ',' + arguments.hasOwnProperty('2')",
  "arguments[2] = 'new'; return arguments.length + ',' + arguments[2]", "arguments.length = 5; return Array.prototype.join.call(arguments)", "return Array.prototype.map.call(arguments, x => x * 2).join()",
  "Object.defineProperty(arguments, '0', { value: 1, writable: true }); a = 3; return arguments[0]", "Object.defineProperty(arguments, '0', { configurable: false }); delete arguments[0]; return arguments[0]",
  "'use strict'; a = 9; return arguments[0]", "'use strict'; arguments[0] = 9; return a", "'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get === Object.getOwnPropertyDescriptor(arguments, 'callee').set",
  "'use strict'; return arguments.callee", "'use strict'; arguments.callee = 1", "'use strict'; return typeof Object.getOwnPropertyDescriptor(arguments, 'callee').get",
];
const sigs = ["a, b", "a, b, c", "a, b = 2", "a, ...b", "{ a }, b", "a = 1, b", "a, a", "a, b, a"];
for (const b of argBodies) {
  for (const sig of ["a, b", "a = 1, b", "a, ...b"]) add(show(`(function f(${sig}) { ${b} })(1, 2)`));
  add(show(`(function f(a, b) { ${b} })(1)`));
  add(show(`(function f(a, b) { ${b} })()`));
  add(show(`(function f(a, b) { ${b} })(1, 2, 3)`));
}
for (const sig of sigs) add(show(`(function f(${sig}) { arguments[0] = 'x'; return JSON.stringify([...arguments]) + ',' + String(typeof a) })(1, 2)`));
add(body("function f(a) { arguments = 1; a = 2; return typeof arguments } globalThis.R = f(1)"));
add(body("function f(a, a) { return a + ',' + arguments[0] + ',' + arguments[1] } globalThis.R = f(1, 2)"));
add(body("function f(a, a) { a = 9; return arguments[0] + ',' + arguments[1] } globalThis.R = f(1, 2)"));
add(body("function f(a, a) { arguments[1] = 9; return a } globalThis.R = f(1, 2)"));
add(body("function f(a, a) { arguments[0] = 9; return a } globalThis.R = f(1, 2)"));

// ---- 7. Closures em laços.
const loopTypes = [
  "for (var i = 0; i < 3; i++) fs.push(() => i)", "for (let i = 0; i < 3; i++) fs.push(() => i)", "for (const i of [0, 1, 2]) fs.push(() => i)",
  "for (let i = 0; i < 3; i++) { fs.push(() => i); i++ }", "for (let i = 0; i < 3; i++) { fs.push(() => i++) }", "for (let i = 0, j = 10; i < 3; i++, j--) fs.push(() => i + ':' + j)",
  "for (let i = 0; fs.push(() => i), i < 2; i++);", "for (let i = 0; i < 3; fs.push(() => i), i++);", "for (let i = 0, f = () => i; i < 3; i++) fs.push(f)",
  "for (let i = 0, f = () => i++; i < 3; i++) fs.push(f)", "for (let [i] = [0]; i < 3; i++) fs.push(() => i)", "for (let { i } = { i: 0 }; i < 3; i++) fs.push(() => i)",
  "for (var k in { a: 1, b: 2 }) fs.push(() => k)", "for (let k in { a: 1, b: 2 }) fs.push(() => k)", "for (const k in { a: 1, b: 2 }) fs.push(() => k)",
  "for (let [x, y] of [[1, 2], [3, 4]]) fs.push(() => x + y)", "for (let x of [1, 2, 3]) { fs.push(() => x); x *= 2 }", "var i = 0; while (i < 3) { let j = i; fs.push(() => j); i++ }",
  "var i = 0; while (i < 3) { var j = i; fs.push(() => j); i++ }", "var i = 0; do { let j = i; fs.push(() => j) } while (++i < 3)", "for (let i = 0; i < 3; i++) { let i = 'in'; fs.push(() => i) }",
  "for (let i = 0; i < 3; i++) { var v = i; fs.push(() => v) }", "for (let i = 0; i < 3; i++) { function g() { return i } fs.push(g) }", "for (let i = 0; i < 3; i++) { class C { static v = i } fs.push(() => C.v) }",
  "for (let i = 0; i < 3; i++) { fs.push({ get v() { return i } }.constructor === Object ? () => i : null) }", "for (let i = 0; i < 3; i++) { try { throw i } catch (e) { fs.push(() => e) } }",
  "for (let i = 0; i < 3; i++) { switch (i) { case 0: let s = 'z'; fs.push(() => s + i); break; default: fs.push(() => i) } }", "L: for (let i = 0; i < 5; i++) { if (i == 3) break L; fs.push(() => i) }",
  "L: for (let i = 0; i < 5; i++) { if (i % 2) continue L; fs.push(() => i) }", "for (let i = 0; i < 3; i++) { fs.push(function () { return i }) }", "for (let i = 0; i < 3; i++) fs.push(eval('() => i'))",
  "for (let i = 0; i < 3; i++) fs.push(new Function('return 0'))", "for (let i = 0; i < 3; i++) { setI = () => i = 'set'; fs.push(() => i) } setI()", "for (let i = 0; i < 2; i++) { for (let j = 0; j < 2; j++) fs.push(() => i + '' + j) }",
  "for (var i = 0; i < 2; i++) { for (let j = 0; j < 2; j++) fs.push(() => i + '' + j) }", "for (let i = 0; i < 3; i++) { fs.push(() => i); continue }", "for (let i of [1, 2]) { fs.push(async () => i) }",
  "for (let i = 0; i < 3; i++) { fs.push(function* () { yield i }) }", "for (let i = 0; i < 3; i++) { fs.push({ m() { return i } }.m) }", "for (let i = 0; i < 3; i++) { fs.push(class { static m() { return i } }.m) }",
];
for (const loop of loopTypes) {
  add(show(`(function () { var fs = [], setI; ${loop}; return fs.map(f => { try { const r = f(); return r && r.next ? r.next().value : r } catch (e) { return e.name } }).join() })()`));
  add(show(`(function () { var fs = [], setI; ${loop}; return fs.map(f => typeof f).join() })()`));
}

// ---- 8. Getters e setters em literais e classes.
const accessors = [
  "({ get a() { return 1 } }).a", "({ set a(v) {} }).a", "({ get a() { return 1 }, set a(v) { this._a = v } })", "Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').set",
  "typeof Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').get", "Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').enumerable",
  "Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').configurable", "Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').get.name",
  "Object.getOwnPropertyDescriptor({ set a(v) {} }, 'a').set.name", "Object.getOwnPropertyDescriptor({ set a(v) {} }, 'a').set.length", "Object.getOwnPropertyDescriptor({ get a() { return 1 } }, 'a').get.length",
  "Object.getOwnPropertyDescriptor({ get [Symbol.iterator]() { return 1 } }, Symbol.iterator).get.name", "Object.getOwnPropertyDescriptor({ get ['x' + 1]() { return 1 } }, 'x1').get.name",
  "Object.getOwnPropertyDescriptor({ get 1() { return 1 } }, '1').get.name", "Object.getOwnPropertyDescriptor({ get 'a b'() { return 1 } }, 'a b').get.name", "Object.getOwnPropertyDescriptor({ get 1n() { return 1 } }, '1').get.name",
  "({ get a() { return 1 }, a: 2 }).a", "({ a: 2, get a() { return 1 } }).a", "({ get a() { return 1 }, get a() { return 2 } }).a", "({ get a() { return 1 }, set a(v) {}, get a() { return 3 } }).a",
  "var o = { get a() { return 1 }, set a(v) {} }; Object.getOwnPropertyDescriptor(o, 'a').set !== undefined", "var o = { get a() { return 1 }, a: 5 }; Object.getOwnPropertyDescriptor(o, 'a').get",
  "var o = { set a(v) { this.v = v } }; o.a = 3; o.v", "var o = { set a(v) { this.v = v } }; o.a += 3; o.v", "var o = { get a() { return 2 } }; o.a = 3; o.a", "'use strict'; var o = { get a() { return 2 } }; o.a = 3",
  "var o = { get a() { return this } }; o.a === o", "var o = { get a() { return this } }; Object.create(o).a === o", "var o = { get a() { return typeof this } }; Reflect.get(o, 'a', 5)",
  "var o = { get a() { 'use strict'; return typeof this } }; Reflect.get(o, 'a', 5)", "var o = { get a() { return typeof this } }; Reflect.get(o, 'a', undefined)", "var o = { get a() { return super.x }, __proto__: { x: 'sx' } }; o.a",
  "var o = { set a(v) { super.x = v }, __proto__: {} }; o.a = 1; o.x", "var o = { get a() { return arguments.length } }; o.a", "var o = { set a(v) { this.n = arguments.length } }; o.a = 1; o.n",
  "var o = { get a() { return new.target } }; String(o.a)", "var o = { set a([x, y]) { this.s = x + y } }; o.a = [1, 2]; o.s", "var o = { set a({ x }) { this.s = x } }; o.a = { x: 5 }; o.s",
  "var o = { set a(v = 3) { this.s = v } }; o.a = undefined; o.s", "var o = { set a(...v) { } }", "var o = { set a() { } }", "var o = { set a(x, y) { } }", "var o = { get a(x) { } }", "var o = { get a() { } , get() { return 1 } }.get()",
  "var o = { get: 1, set: 2 }; o.get + o.set", "var o = { get() { return 1 }, set() { return 2 } }; o.get() + o.set()", "var o = { get a() { return 1 } }; delete o.a; typeof o.a", "var o = { get a() { return 1 } }; Object.keys(o).join()",
  "var o = { get a() { return 1 } }; JSON.stringify(o)", "var o = { get a() { return 1 } }; ({ ...o }).a", "var o = { get a() { return 1 } }; Object.getOwnPropertyDescriptor({ ...o }, 'a').value",
  "var o = { get a() { return 1 } }; Object.assign({}, o).a", "var o = { get a() { return 1 } }; var { a } = o; a", "var c = 0; var o = { get a() { c++; return 1 } }; o.a; o.a; c", "var c = 0; var o = { get a() { c++; return 1 } }; ({ ...o }); c",
  "class C { get a() { return 1 } } new C().a", "class C { set a(v) { this.v = v } } var c = new C(); c.a = 4; c.v", "class C { static get a() { return 'sa' } } C.a", "class C { static set a(v) { C.v = v } } C.a = 2; C.v",
  "class C { get a() { return 1 } } Object.getOwnPropertyDescriptor(C.prototype, 'a').enumerable", "class C { get a() { return 1 } } Object.getOwnPropertyNames(C.prototype).join()",
  "class C { get a() { return 1 } } typeof Object.getOwnPropertyDescriptor(C.prototype, 'a').get", "class C { get a() { return 1 } } Object.getOwnPropertyDescriptor(C.prototype, 'a').get.name",
  "class C { static get a() { return 1 } } Object.getOwnPropertyDescriptor(C, 'a').get.name", "class C { get a() { return 1 } set a(v) {} } Object.getOwnPropertyDescriptor(C.prototype, 'a').set.name",
  "class C { get a() { return 1 } } class D extends C { } new D().a", "class C { get a() { return 1 } } class D extends C { get a() { return super.a + 1 } } new D().a",
  "class C { set a(v) { this._a = v } } class D extends C { set a(v) { super.a = v * 2 } } var d = new D(); d.a = 2; d._a", "class C { get a() { return 1 } } class D extends C { set a(v) { } } new D().a",
  "class C { get a() { return 1 } } class D extends C { set a(v) { } } Object.getOwnPropertyDescriptor(D.prototype, 'a').get", "class C { get a() { return 1 } } new C().hasOwnProperty('a')",
  "class C { get a() { return 1 } } 'a' in new C()", "class C { get ['x' + 'y']() { return 1 } } new C().xy", "class C { get [Symbol.toStringTag]() { return 'Tag' } } String(new C())",
  "class C { static get [Symbol.species]() { return Array } } C[Symbol.species] === Array", "class C { get constructor() { return 1 } }", "class C { static get prototype() { return 1 } }", "class C { static get name() { return 'custom' } } C.name",
  "class C { static get length() { return 42 } } C.length", "class C { get #p() { return 'priv' } read() { return this.#p } } new C().read()", "class C { set #p(v) { this.v = v } w() { this.#p = 1; return this.v } } new C().w()",
  "class C { get #p() { return 1 } w() { this.#p = 1 } } new C().w()", "class C { set #p(v) { } r() { return this.#p } } new C().r()", "class C { static get #p() { return 's' } static r() { return C.#p } } C.r()",
  "class C { get #p() { return 1 } static t(o) { return #p in o } } C.t(new C()) + ',' + C.t({})", "class C { get a() { return 1 } get a() { return 2 } } new C().a",
  "class C { get a() { return 1 } a() { return 2 } } typeof new C().a", "class C { a() { return 2 } get a() { return 1 } } typeof new C().a", "class C { static get a() { return 1 } static a() { return 2 } } typeof C.a",
  "class C { get a() { return this } } var c = new C(); c.a === c", "class C { get a() { return () => this } } var c = new C(); c.a() === c", "class C { accessor x = 1 }", "class C { static { C.v = 'blk' } } C.v",
];
for (const code of accessors) add(show(`(function () { return eval(${q(code)}) })()`));

// ---- 9. label + break em blocos.
const labels = [
  "L: { break L; }", "var r = 1; L: { r = 2; break L; r = 3 } r", "var r = ''; A: { B: { r += 'b'; break A; r += 'x' } r += 'a' } r", "var r = ''; A: { B: { r += 'b'; break B; r += 'x' } r += 'a' } r",
  "var r = 0; L: if (true) { r = 1; break L; r = 2 } r", "var r = 0; L: try { r = 1; break L } finally { r += 10 } r", "var r = 0; L: try { r = 1 } finally { r += 10; break L } r",
  "var r = 0; L: try { throw 1 } catch (e) { r = 5; break L } finally { r += 10 } r", "var r = 0; L: { try { r = 1; break L } finally { r = 9 } } r", "var r = ''; L: for (var i = 0; i < 3; i++) { r += i; if (i == 1) break L } r",
  "var r = ''; L: for (var i = 0; i < 3; i++) { r += i; if (i == 1) continue L; r += '.' } r", "var r = ''; L: for (var i = 0; i < 2; i++) { for (var j = 0; j < 2; j++) { r += i + '' + j; if (j == 0) continue L } } r",
  "var r = ''; L: for (var i = 0; i < 2; i++) { for (var j = 0; j < 2; j++) { r += i + '' + j; if (j == 0) break L } } r", "var r = ''; L: { for (var i = 0; i < 3; i++) { r += i; if (i == 1) break L } r += 'x' } r",
  "var r = ''; L: switch (1) { case 1: r += 'a'; break L; case 2: r += 'b' } r", "var r = ''; L: switch (1) { case 1: for (;;) { r += 'a'; break L } } r", "var r = ''; switch (1) { case 1: for (;;) { r += 'a'; break } r += 'b' } r",
  "var r = ''; L: while (true) { r += 'a'; break L } r", "var r = ''; L: do { r += 'a'; break L } while (true); r", "var r = ''; L: do { r += 'a'; continue L } while (false); r",
  "var r = ''; L1: L2: { r += 'a'; break L1 } r", "var r = ''; L1: L2: { r += 'a'; break L2 } r", "var r = ''; L: { L2: { break L } r += 'x' } r", "var r = ''; L: L: { }", "L: { L: { } }",
  "var r = ''; L: { (function () { try { break L } catch (e) { r = e.name } })() } r", "L: { (function () { break L })() }", "L: ; break L", "break", "continue", "L: { continue L }", "L: for (;;) { continue M }",
  "var r = ''; L: { r += 1; { r += 2; break L } } r", "var r = ''; L: { r += 1; { r += 2; break L; r += 3 } r += 4 } r", "var r = 0; L: { with ({}) { r = 1; break L } r = 2 } r", "var r = 0; L: with ({}) { r = 1; break L; r = 2 } r",
  "var r = 0; L: { eval('r = 1') ; break L } r", "L: { eval('break L') }", "var r = 0; L: { try { eval('r = 1; break L') } catch (e) { r = e.name } } r", "var r = 0; L: function f() {} typeof f", "L: let x", "L: const x = 1", "L: class C {}",
  "L: async function f() {}", "L: function* f() {}", "'use strict'; L: function f() {}", "var r = 0; L: { r = 1; { break L } } r", "var r = ''; L: for (let i = 0; i < 2; i++) { r += i; for (let j = 0; j < 2; j++) { if (j) continue L; r += 'j' } } r",
  "var r = ''; L: for (var i of [1, 2, 3]) { if (i == 2) continue L; r += i } r", "var r = ''; L: for (var k in { a: 1, b: 2 }) { if (k == 'a') continue L; r += k } r", "var r = 0; yield: { r = 1; break yield } r", "var r = 0; await: { r = 1; break await } r",
  "var r = 0; async: { r = 1; break async } r", "var r = 0; let: { r = 1; break let } r", "var r = 0; of: { r = 1; break of } r", "var r = 0; static: { r = 1; break static } r", "'use strict'; yield: ;", "'use strict'; let: ;",
  "var r = 0; L: { r = 1; if (r) break L; r = 2 } r", "var r = 0; L: { do { r++; if (r > 2) break L } while (true) } r", "var r = 0; L: for (;;) { switch (r++) { case 0: continue L; case 1: break L } } r",
  "var r = ''; (function* () { L: { yield 1; break L } })().next().value", "(function () { L: { return 1 } })()", "(function () { L: { try { return 1 } finally { break L } } return 2 })()",
  "(function () { L: try { return 1 } finally { break L } return 2 })()", "(function () { for (;;) { try { return 1 } finally { break } } return 2 })()", "(function () { for (var i = 0; i < 2; i++) { try { continue } finally { return 'f' + i } } })()",
];
for (const code of labels) add(show(`eval(${q(code)})`));

// ---- 10. Vírgula, void, typeof, delete de borda.
const misc = [
  "(1, 2)", "(1, 2, 3)", "typeof (1, 2)", "(0, eval)('typeof zz')", "var o = { f() { return this } }; (0, o.f)() === globalThis", "var o = { f() { return this } }; (o.f)() === o", "var o = { f() { return this } }; (o.f, o.f)() === o",
  "var o = { f() { return this } }; (1 && o.f)() === o", "var o = { f() { return this } }; (o?.f)() === o", "var o = { f() { return this } }; o?.f() === o", "var o = { f() { return this } }; (o.f = o.f)() === o",
  "var a = (1, 2); a", "var a = [(1, 2)]; a.length", "var a = [1, 2, 3][(0, 1)]; a", "for (var i = 0, j = 5; i < 2; i++, j--); i + ',' + j", "(function () { return 1, 2 })()", "(() => (1, 2))()", "x => (x, 1)",
  "void 0", "void 'a'", "void (1, 2)", "typeof void 0", "void typeof 0", "void void 0", "typeof typeof 0", "typeof typeof typeof 0", "!void 0", "-void 0", "+void 0", "void 0 + 1", "(void 0)()", "void (function () {})()",
  "typeof undeclared", "typeof undeclared.x", "typeof (undeclared)", "typeof (0, undeclared)", "typeof undeclared + 1", "typeof (() => undeclared)", "typeof (function () { return undeclared })()", "typeof [undeclared]",
  "typeof null", "typeof undefined", "typeof NaN", "typeof Symbol()", "typeof 1n", "typeof class {}", "typeof function* () {}", "typeof async () => {}", "typeof new Function", "typeof Proxy", "typeof new Proxy(function () {}, {})",
  "typeof new Proxy(class {}, {})", "typeof new Proxy({}, {})", "typeof document", "typeof globalThis", "typeof this", "typeof typeof", "typeof /x/", "typeof new String('')", "typeof String('')", "typeof Object(1n)", "typeof Object(Symbol())",
  "var tdz = typeof x; let x", "typeof x; let x", "typeof x; var x", "typeof x; class x {}", "{ typeof x; let x }", "typeof (x), x; let x", "var o = {}; typeof o.a.b", "typeof o?.a", "var o = null; typeof o?.a", "typeof (a, b)",
  "delete 1", "delete 'a'", "delete null", "delete undefined", "delete NaN", "delete globalThis.undefined", "delete this", "delete void 0", "delete (1, 2)", "delete (0, [].length)", "delete [].length", "delete [][0]", "delete 'abc'[0]", "delete 'abc'.length",
  "delete 'abc'[5]", "delete Object.prototype", "delete Math.PI", "delete Math.abs", "delete Array.prototype.length", "delete function () {}.length", "delete (function () {}).prototype", "delete (class {}).prototype", "delete (() => {}).prototype",
  "var o = { a: 1 }; delete o.a", "var o = { a: 1 }; delete o['a']", "var o = { a: 1 }; delete (o.a)", "var o = { a: 1 }; delete ((o.a))", "var o = { a: 1 }; delete (0, o.a)", "var o = Object.freeze({ a: 1 }); delete o.a", "'use strict'; var o = Object.freeze({ a: 1 }); delete o.a",
  "'use strict'; delete Object.prototype", "'use strict'; delete [].length", "'use strict'; delete 'abc'[0]", "'use strict'; delete 1", "'use strict'; delete undefined", "'use strict'; delete NaN", "'use strict'; delete globalThis.undefined",
  "'use strict'; var x; delete x", "'use strict'; delete x", "'use strict'; delete (x)", "'use strict'; delete ((x))", "'use strict'; delete (0, x)", "'use strict'; function f() {} delete f", "var x = 1; delete x", "y = 1; delete y", "y = 1; delete y; typeof y",
  "eval('var z = 1'); delete z", "eval('var z = 1'); delete z; typeof z", "(function () { eval('var z = 1'); return delete z })()", "(function () { var z; return delete z })()", "(function (a) { return delete a })(1)", "(function () { return delete arguments })()",
  "(function () { return delete arguments[0] })(1)", "(function () { return delete arguments.length })(1)", "(function () { return delete arguments.callee })(1)", "(function () { 'use strict'; return delete arguments.callee })(1)",
  "var o = { get a() { return 1 } }; delete o.a", "var o = {}; Object.defineProperty(o, 'a', { value: 1 }); delete o.a", "var o = new Proxy({}, { deleteProperty: () => false }); delete o.a", "'use strict'; var o = new Proxy({}, { deleteProperty: () => false }); delete o.a",
  "var o = new Proxy({}, { deleteProperty: () => true }); delete o.a", "var o = null; delete o.a", "var o; delete o?.a", "var o = null; delete o?.a", "var o = null; delete o?.a.b.c", "var o = { a: null }; delete o.a?.b", "delete undefined.a", "delete null[0]",
  "delete super.x", "({ m() { delete super.x } }).m()", "({ m() { delete super[0] } }).m()", "({ m() { try { delete super.x } catch (e) { return e.name } } }).m()", "var a = [1, 2]; delete a[0]; a.length + ',' + (0 in a)", "var a = [1, 2]; delete a.length",
  "'use strict'; var a = [1, 2]; delete a.length", "var s = Symbol(); var o = { [s]: 1 }; delete o[s]; Object.getOwnPropertySymbols(o).length", "var o = { 1: 'a' }; delete o[1]; Object.keys(o).length", "delete globalThis.Array", "delete globalThis.eval; typeof eval",
  "var r = delete globalThis.nonexistent; r", "var o = { a: { b: 1 } }; delete o.a.b; JSON.stringify(o)", "var o = { a: 1 }; delete o.a, o.b = 2; JSON.stringify(o)", "var i = 0; var o = { a: 1 }; delete o[i++ ? 'a' : 'b']; i", "var o = { a: 1 }; delete o[(o.z = 1, 'a')]; JSON.stringify(o)",
  "var o = { a: 1 }; delete o[{ toString() { return 'a' } }]; JSON.stringify(o)", "var o = { a: 1 }; delete o[{ toString() { throw 1 } }]", "var o = null; delete o[(function () { throw 'order' })()]", "delete (void 0)?.a", "var o = {}; delete o?.['a']",
  "!!delete 1", "typeof delete 1", "delete delete 1", "void delete 1", "- -1", "+ +1", "- - -1", "typeof -0", "1 + + '1'", "1 - - '1'", "1 + - + '1'", "'a' + void 0", "[void 0] + ''", "[,].length", "[, ,].length", "[1, , 2].length", "[...[1, 2], ...[3]].length",
  "(function () { return typeof arguments })()", "(function () { return typeof arguments.callee })()", "(() => { try { return typeof arguments } catch (e) { return e.name } })()", "typeof new.target", "(function () { return typeof new.target })()",
  "new (function () { this.t = typeof new.target })().t", "typeof super", "typeof import.meta", "typeof await", "typeof yield", "typeof async", "typeof let", "typeof of", "typeof get", "typeof static",
];
for (const code of misc) {
  add(show(`eval(${q(code)})`));
  add(show(`(function () { 'use strict'; return eval(${q(code)}) })()`));
}

// ---- Execução.
const baseSources = new Set();
for (const program of knownPrograms("eval_scope_bun.tsv", ["eval_bun.tsv", "scope_bun.tsv"])) baseSources.add(JSON.stringify(program));
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "eval-scope-golden-"));
const runner = path.join(dir, "runner.js");
fs.writeFileSync(
  runner,
  [
    "const vm = require('vm');",
    "const fs = require('fs');",
    "const src = fs.readFileSync(process.argv[2], 'utf8');",
    "try { vm.runInThisContext(src, { filename: 'eval_scope_case.js' }); } catch (e) { process.stdout.write('\\u0002' + String(e && e.name) + ': ' + String(e && e.message) + '\\n'); process.exit(0); }",
    "setTimeout(() => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n'); }, 0);",
  ].join("\n"),
);
const file = path.join(dir, "case.js");
const seen = new Set();
let kept = 0;
let dropped = 0;
let repeated = 0;
// A matriz completa passa de 2300 programas; amostra por hash (sampleByHash; `TARGET`, padrão 520; `TARGET=100000` gera tudo).
const target = Number(process.env.TARGET || 520);
const uniquePrograms = programs.filter(source => !seen.has(source) && seen.add(source));
seen.clear();
const sampled = sampleByHash(uniquePrograms, target);
for (const source of sampled) {
  if (seen.has(source)) continue;
  seen.add(source);
  if (baseSources.has(JSON.stringify(source))) {
    repeated++;
    continue;
  }
  fs.writeFileSync(file, source);
  const result = spawnSync(process.execPath, [runner, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (result.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    continue;
  }
  const value = JSON.parse(marked.slice(1));
  if (value.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(value)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(value));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos de eval_bun/scope_bun ${repeated}\n`);
fs.rmSync(dir, { recursive: true, force: true });
