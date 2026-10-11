// Gera tests/golden/console_object_more_bun.tsv: `console.log` (stdout) de funções, classes, Map, Set, WeakMap, WeakSet, Date,
// RegExp, Error (com a pilha normalizada), boxed, Symbol.toStringTag, protótipo nulo, Promise, typed arrays e ArrayBuffer,
// medidos no bun 1.4.2. Colunas: a fonte do programa (JSON), os bytes do stdout em hex, os bytes do stderr em hex e o valor
// da variável global `R` (a exceção, como `Nome|mensagem`, ou `<undefined>`).
// Uso: bun scripts/gen-console-object-more-golden.js > tests/golden/console_object_more_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");
const { runMain } = require("./uncaught-run.js");

const E = "var E = function (e) { return e.name + '|' + e.message };\n";
// Pilha fixa para todo erro: o conteúdo real traz caminho de arquivo e número de linha da máquina de quem gera.
const S = "var S = function (e) { e.stack = e.name + ': ' + e.message + '\\n    at <anonymous>'; return e };\n";
const programs = [];
const mainSources = [];
const run = (code) => programs.push(E + S + `try { ${code} } catch (e) { R = E(e) }`);
const log = (expr) => run(`console.log(${expr})`);

// Funções e classes.
for (const e of [
  "function f() {}", "function () {}", "(function () {})", "(() => {})", "(function f() {})", "(a => a)", "({ m() {} }).m", "({ m: function () {} }).m",
  "({ m: () => {} }).m", "({ get g() { return 1 } }).g", "Object.getOwnPropertyDescriptor({ get g() { return 1 } }, 'g').get",
  "async function f() {}", "(async function () {})", "(async () => {})", "async function* ag() {}", "(async function* () {})",
  "function* g() {}", "(function* () {})", "({ *m() {} }).m", "({ async m() {} }).m", "({ async *m() {} }).m",
  "class A {}", "(class {})", "class B extends (class A {}) {}", "(class extends Object {})", "class A { static x = 1 }",
  "class A { static m() {} }", "class A { constructor() {} }", "(class A { static name = 'z' })", "(class { static name = 5 })",
  "Math.max", "Object", "Array.prototype.push", "Symbol", "console.log", "Function.prototype", "(function f() {}).bind(null)",
  "(function () {}).bind(null)", "(class A {}).bind(null)", "Object.defineProperty(function () {}, 'name', { value: '' })",
  "Object.defineProperty(function () {}, 'name', { value: 'x y' })", "Object.defineProperty(function () {}, 'name', { value: Symbol('s') })",
  "Object.defineProperty(function () {}, 'name', { value: 5 })", "Object.defineProperty(function f() {}, 'name', { value: undefined })",
  "(() => { const x = function () {}; return x })()", "(() => { const x = () => {}; return x })()", "[function f() {}]", "{ f: function f() {} }",
  "{ f() {} }", "{ a: class A {}, b: class {} }", "[class A {}, () => {}]", "new Function('a', 'return a')", "Function('return 1')",
  "Object.setPrototypeOf(function f() {}, null)", "Object.setPrototypeOf(class A {}, null)", "Object.setPrototypeOf(class A extends Object {}, null)",
  "Object.setPrototypeOf(function f() {}, Array.prototype)", "Object.setPrototypeOf(async function f() {}, null)",
  "(function () { return arguments })()", "(class A extends Array {})", "(class A extends null {})", "(class A extends Map {})",
]) {
  log(e);
}
for (const e of [
  "Object.assign(function f() {}, { a: 1 })", "Object.assign(function () {}, { a: 1, b: 'x' })", "Object.assign(() => {}, { a: 1 })",
  "Object.assign(class A {}, { a: 1 })", "Object.assign(class B extends (class A {}) {}, { a: 1 })", "Object.assign(async function f() {}, { a: 1 })",
  "Object.assign(function* g() {}, { a: 1 })", "Object.assign(function f() {}, { a: { b: 1 } })", "Object.assign(function f() {}, { a() {} })",
  "Object.assign(function f() {}, { [Symbol('k')]: 1 })", "Object.defineProperty(function f() {}, 'h', { value: 1, enumerable: false })",
  "Object.defineProperty(function f() {}, 'h', { value: 1, enumerable: true })", "(() => { function f() {}; f.prototype.x = 1; return f })()",
  "(() => { class A { static s = 1; static t() {} }; return A })()", "(() => { class A { static get g() { return 1 } }; return A })()",
  "(() => { function f() {}; f.self = f; return f })()", "(() => { const f = function () {}; f[Symbol.toStringTag] = 'T'; return f })()",
  "Object.assign(function f() {}, { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10, k: 11, l: 12, m: 13, n: 14, o: 15, p: 16, q: 17, r: 18, s: 19, t: 20, u: 21, v: 22, w: 23, x: 24, y: 25, z: 26 })",
]) {
  log(e);
}
run("var f = function () {}; f.a = 1; console.log([f]); console.log({ f })");
run("class A { static s = 1 } console.log({ A }, [A])");
run("console.log('%s', function f() {}); console.log('%o', function f() {}); console.log('%O', function f() {})");
run("console.log('%o', class A {}); console.log('%O', class A { static x = 1 })");
run("console.log('%j', function f() {})");

// Map, Set, WeakMap, WeakSet.
for (const e of [
  "new Map()", "new Map([[1, 2]])", "new Map([[1, 2], ['a', 'b']])", "new Map([['a', { b: 1 }]])", "new Map([[{ a: 1 }, [1, 2]]])",
  "new Map([[1, new Map([[2, 3]])]])", "new Map([[1, new Map([[2, new Map([[3, new Map([[4, 5]])]])]])]])", "new Map([[NaN, -0]])",
  "new Map([[undefined, null]])", "new Map([[Symbol('s'), 1n]])", "new Map([['k', new Set([1])]])", "new Map([[1, 'x']]).entries()",
  "new Map([[1, 'x']]).keys()", "new Map([[1, 'x']]).values()", "new Map([[1, 'x']])[Symbol.iterator]()", "new Map().entries()",
  "new Map([[1, 2], [3, 4]]).keys()", "new Map([[1, 2], [3, 4]]).values()", "(() => { const i = new Map([[1, 2], [3, 4]]).keys(); i.next(); return i })()",
  "(() => { const i = new Map([[1, 2]]).entries(); i.next(); i.next(); return i })()",
  "Object.assign(new Map([[1, 2]]), { extra: 1 })", "Object.assign(new Map(), { extra: 1 })", "(() => { const m = new Map(); m.set(m, m); return m })()",
  "(() => { const m = new Map(); m.set('self', m); return m })()", "new (class M extends Map {})()", "new (class M extends Map {})([[1, 2]])",
  "(() => { class M extends Map {}; return new M([[1, 2]]) })()", "Object.setPrototypeOf(new Map([[1, 2]]), null)",
  "(() => { const m = new Map([[1, 2]]); m[Symbol.toStringTag] = 'X'; return m })()",
  "new Map(Array.from({ length: 101 }, (_, i) => [i, i]))", "new Map(Array.from({ length: 100 }, (_, i) => [i, i]))",
  "new Map(Array.from({ length: 30 }, (_, i) => [i, i]))", "new Map([['a'.repeat(100), 'b'.repeat(100)]])",
  "new Set()", "new Set([1])", "new Set([1, 2, 3])", "new Set(['a', 'b'])", "new Set([{ a: 1 }, [1]])", "new Set([new Set([1])])",
  "new Set([new Set([new Set([new Set([1])])])])", "new Set([NaN, -0, 0])", "new Set([undefined, null])", "new Set([1]).values()", "new Set([1]).entries()",
  "new Set([1])[Symbol.iterator]()", "new Set().values()", "(() => { const i = new Set([1, 2, 3]).values(); i.next(); return i })()",
  "Object.assign(new Set([1]), { extra: 1 })", "(() => { const s = new Set(); s.add(s); return s })()", "new (class S extends Set {})([1, 2])",
  "Object.setPrototypeOf(new Set([1]), null)", "(() => { const s = new Set([1]); s[Symbol.toStringTag] = 'X'; return s })()",
  "new Set(Array.from({ length: 101 }, (_, i) => i))", "new Set(Array.from({ length: 30 }, (_, i) => i))",
  "new Set(Array.from({ length: 5 }, (_, i) => 'item number ' + i + ' of the set with a long text'))",
  "new WeakMap()", "new WeakSet()", "(() => { const k = {}; const w = new WeakMap(); w.set(k, 1); return w })()",
  "(() => { const k = {}; const w = new WeakSet(); w.add(k); return w })()", "Object.assign(new WeakMap(), { a: 1 })", "Object.assign(new WeakSet(), { a: 1 })",
  "new WeakRef({})", "new WeakRef({ a: 1 })", "new FinalizationRegistry(() => {})",
  "[new Map([[1, 2]]), new Set([1])]", "{ m: new Map([[1, 2]]), s: new Set([1]) }", "{ w: new WeakMap(), v: new WeakSet() }",
]) {
  log(e);
}

// Date.
for (const e of [
  "new Date(0)", "new Date(1700000000000)", "new Date(NaN)", "new Date('invalid')", "new Date(8.64e15)", "new Date(-8.64e15)", "new Date(-1)",
  "new Date(-62198755200000)", "new Date(253402300800000)", "new Date(Date.UTC(2020, 1, 29, 12, 30, 45, 678))", "new Date(2020, 0, 1)",
  "new Date('2020-01-01')", "new Date('2020-01-01T00:00:00.000+05:30')", "[new Date(0)]", "{ d: new Date(0) }", "[new Date(NaN)]",
  "Object.assign(new Date(0), { a: 1 })", "Object.assign(new Date(NaN), { a: 1 })", "Object.setPrototypeOf(new Date(0), null)",
  "new (class D extends Date {})(0)", "(() => { class D extends Date {}; return new D(0) })()", "(() => { const d = new Date(0); d[Symbol.toStringTag] = 'X'; return d })()",
  "Date.prototype", "Date", "new Date(0).getTime", "(() => { const d = new Date(0); d.toISOString = () => 'custom'; return d })()",
  "(() => { const d = new Date(0); d.toISOString = undefined; return d })()", "(() => { const d = new Date(0); d.toISOString = () => { throw new Error('boom') }; return d })()",
  "new Date(0).toString()", "[new Date(0), new Date(1)]", "new Map([[new Date(0), new Date(1)]])",
]) {
  log(e);
}

// RegExp.
for (const e of [
  "/a/", "/a/g", "/a/gimsuy", "/a/dgimsuvy".replace("uvy", "uy"), "/a/v", "/(?<n>x)/", "/[/]/", "/\\//", "new RegExp('/')", "new RegExp('')", "new RegExp('a', 'g')",
  "new RegExp('\\n')", "new RegExp('\\\\n')", "RegExp.prototype", "RegExp", "/a/.exec", "[/a/]", "{ r: /a/ }", "[/a/, /b/g]",
  "Object.assign(/a/, { x: 1 })", "(() => { const r = /a/g; r.lastIndex = 3; return r })()", "(() => { const r = /a/g; r.exec('a'); return r })()",
  "Object.setPrototypeOf(/a/, null)", "new (class R extends RegExp {})('a')", "(() => { class R extends RegExp {}; return new R('a', 'g') })()",
  "(() => { const r = /a/; r[Symbol.toStringTag] = 'X'; return r })()", "(() => { const r = /a/; r.source; return Object.defineProperty(r, 'source', { value: 'z' }) })()",
  "(() => { const r = /a/; r.toString = () => 'custom'; return r })()", "/\\u{1F600}/u", "/./s", "new Map([[/a/, /b/]])", "'abc'.matchAll(/b/g)",
  "'abc'.match(/b/)", "'abc'.match(/(?<n>b)/)", "'abc'.match(/b/g)", "/b/.exec('abc')", "'abc'.matchAll(/b/g).next()",
]) {
  log(e);
}

// Error (pilha normalizada).
for (const e of [
  "S(new Error('m'))", "S(new Error())", "S(new Error(''))", "S(new TypeError('t'))", "S(new RangeError('r'))", "S(new SyntaxError('s'))",
  "S(new ReferenceError('r'))", "S(new EvalError('e'))", "S(new URIError('u'))", "S(new AggregateError([new Error('x')], 'agg'))",
  "S(new Error('m', { cause: 'c' }))", "S(new Error('m', { cause: S(new Error('inner')) }))", "S(new Error('m', { cause: { a: 1 } }))",
  "S(new Error('m', { cause: undefined }))", "S(new Error('m', { cause: null }))", "S(Object.assign(new Error('m'), { code: 'E1' }))",
  "S(Object.assign(new Error('m'), { code: 'E1', errno: 2, nested: { a: 1 } }))", "S(Object.assign(new Error('m'), { a: 1, b: 'x', c: [1, 2] }))",
  "S(Object.assign(new TypeError('m'), { name: 'Custom' }))", "S(Object.assign(new Error('m'), { name: 'Custom' }))", "S(Object.assign(new Error('m'), { name: '' }))",
  "S(Object.assign(new Error('m'), { name: undefined }))", "S(Object.assign(new Error('m'), { message: 'changed' }))",
  "S(Object.assign(new Error('m'), { message: '' }))", "S(Object.assign(new Error('m'), { message: 5 }))", "S(Object.assign(new Error('m'), { message: 'a\\nb' }))",
  "S(new Error('line1\\nline2'))", "S(new Error('  padded  '))", "S(new Error('\\u00e9'))",
  "(() => { const e = new Error('m'); e.stack = undefined; return e })()", "(() => { const e = new Error('m'); e.stack = ''; return e })()",
  "(() => { const e = new Error('m'); e.stack = 'custom stack'; return e })()", "(() => { const e = new Error('m'); e.stack = 'Error: m\\n    at x (a.js:1:1)'; return e })()",
  "(() => { const e = new Error('m'); delete e.stack; return e })()", "(() => { const e = new Error('m'); e.stack = 5; return e })()",
  "(() => { const e = new Error('m'); e.stack = { a: 1 }; return e })()", "(() => { const e = new Error('m'); e.stack = null; return e })()",
  "(() => { class MyErr extends Error {}; return S(new MyErr('m')) })()", "(() => { class MyErr extends Error { constructor(m) { super(m); this.name = 'MyErr' } }; return S(new MyErr('m')) })()",
  "(() => { class MyErr extends Error { get name() { return 'Getter' } }; return S(new MyErr('m')) })()",
  "(() => { class MyErr extends TypeError { extra = 1 }; return S(new MyErr('m')) })()", "(() => { class MyErr extends Error {}; MyErr.prototype.name = 'Proto'; return S(new MyErr('m')) })()",
  "S(Object.setPrototypeOf(new Error('m'), null))", "S(Object.setPrototypeOf(new Error('m'), Object.create(null)))", "S(Object.setPrototypeOf(new Error('m'), TypeError.prototype))",
  "S(Object.create(Error.prototype))", "S(Object.create(TypeError.prototype))", "Error.prototype", "TypeError.prototype", "Error", "Error.captureStackTrace",
  "[S(new Error('m'))]", "{ e: S(new Error('m')) }", "{ a: { e: S(new Error('m')) } }", "[S(new Error('a')), S(new Error('b'))]", "new Map([[1, S(new Error('m'))]])",
  "new Set([S(new Error('m'))])", "[[S(new Error('m'))]]", "[[[S(new Error('m'))]]]", "{ e: S(new Error('m')), x: 1 }",
  "(() => { const e = S(new Error('m')); e.self = e; return e })()", "(() => { const e = S(new Error('m')); e.cause = e; return e })()",
  "(() => { const e = S(new Error('m')); e.list = [e]; return e })()", "(() => { const e = S(new Error('m')); e.m = new Map([[1, e]]); return e })()",
  "S(new AggregateError([], 'empty'))", "S(new AggregateError([1, 'x'], 'vals'))", "S(new AggregateError([S(new Error('a')), S(new TypeError('b'))], 'two'))",
  "(() => { try { null.x } catch (e) { return S(e) } })()", "(() => { try { undefinedVar } catch (e) { return S(e) } })()", "(() => { try { JSON.parse('{') } catch (e) { return S(e) } })()",
  "(() => { try { new Array(-1) } catch (e) { return S(e) } })()", "(() => { try { decodeURI('%') } catch (e) { return S(e) } })()",
  "(() => { try { BigInt(1.5) } catch (e) { return S(e) } })()", "(() => { try { Symbol() + '' } catch (e) { return S(e) } })()",
  "(() => { try { x = y } catch (e) { return S(e) } })()", "(() => { try { (void 0)() } catch (e) { return S(e) } })()", "(() => { try { new (class A { #p; static t(o) { return o.#p } }).t({}) } catch (e) { return S(e) } })()",
  "(() => { try { 1n + 1 } catch (e) { return S(e) } })()", "(() => { try { 'a'.repeat(-1) } catch (e) { return S(e) } })()", "(() => { try { Object.defineProperty(1, 'a', {}) } catch (e) { return S(e) } })()",
  "(() => { try { structuredClone(() => {}) } catch (e) { return S(e) } })()", "(() => { try { new Intl.NumberFormat('x-invalid-') } catch (e) { return S(e) } })()",
  "(() => { try { atob('*') } catch (e) { return S(e) } })()", "(() => { try { new URL('nope') } catch (e) { return S(e) } })()",
  "new DOMException('m', 'AbortError')", "(() => { try { new Intl.DateTimeFormat('en', { timeZone: 'Nope/Zone' }) } catch (e) { return S(e) } })()",
]) {
  log(e);
}
run("console.log('%s', S(new Error('m'))); console.log('%o', S(new Error('m'))); console.log('%O', S(new Error('m'))); console.log('%j', S(new Error('m')))");
run("console.log('x', S(new Error('m')), 'y')");
run("console.log('%s', Object.assign(S(new TypeError('m')), { code: 1 }))");
run("console.log(String(S(new Error('m'))), S(new Error('m')).toString())");
run("console.log(Object.prototype.toString.call(S(new Error('m'))))");
run("console.log(S(new Error('m', { cause: S(new Error('c1', { cause: S(new Error('c2')) })) })))");
run("console.log(S(new Error('a'))); console.log(S(new Error('b')))");

// Boxed.
for (const e of [
  "new Number(1)", "new Number(-0)", "new Number(NaN)", "new Number(Infinity)", "new Number(1.5)", "new Number(1e21)", "Object(1)", "Object(-0)",
  "new String('a')", "new String('')", "new String('a b')", "new String('a\\'b')", "new String('a\"b')", "new String('a\\nb')", "Object('x')", "new String('\\u00e9')",
  "new String('abc').length", "new Boolean(true)", "new Boolean(false)", "Object(true)", "Object(Symbol('s'))", "Object(Symbol())", "Object(Symbol.iterator)",
  "Object(1n)", "Object(-1n)", "Object(2n**70n)", "[new Number(1)]", "{ n: new Number(1), s: new String('a'), b: new Boolean(true) }",
  "Object.assign(new Number(1), { a: 1 })", "Object.assign(new String('a'), { a: 1 })", "Object.assign(new Boolean(true), { a: 1 })",
  "Object.assign(Object(Symbol('s')), { a: 1 })", "Object.assign(Object(1n), { a: 1 })", "Object.setPrototypeOf(new Number(1), null)",
  "Object.setPrototypeOf(new String('a'), null)", "new (class N extends Number {})(1)", "new (class S extends String {})('a')", "(() => { class N extends Number {}; return new N(5) })()",
  "(() => { class B extends Boolean {}; return new B(true) })()", "(() => { const n = new Number(1); n[Symbol.toStringTag] = 'X'; return n })()",
  "Number.prototype", "String.prototype", "Boolean.prototype", "Symbol.prototype", "BigInt.prototype", "Number", "String", "Boolean",
  "new String('abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz')", "[new String('a'), new String('b')]", "new Map([[new Number(1), new String('a')]])",
  "Object.assign(new String('ab'), { 5: 'x' })", "(() => { const s = new String('ab'); s.extra = 1; return s })()", "new Set([new Number(1), new Boolean(false)])",
]) {
  log(e);
}
run("console.log('%s', new Number(1), new String('a')); console.log('%d', new Number(1)); console.log('%o', new Number(1)); console.log('%O', new String('a')); console.log('%j', new Number(1))");

// Symbol.toStringTag e protótipo nulo.
for (const e of [
  "{ [Symbol.toStringTag]: 'T' }", "{ [Symbol.toStringTag]: 'T', a: 1 }", "{ get [Symbol.toStringTag]() { return 'G' } }", "{ [Symbol.toStringTag]: '' }",
  "{ [Symbol.toStringTag]: 5 }", "{ [Symbol.toStringTag]: undefined }", "{ [Symbol.toStringTag]: null }", "{ [Symbol.toStringTag]: 'Object' }",
  "{ [Symbol.toStringTag]: 'a b' }", "{ [Symbol.toStringTag]: 'Map' }", "{ [Symbol.toStringTag]: 'Array' }",
  "(() => { class A { get [Symbol.toStringTag]() { return 'Tagged' } }; return new A() })()", "(() => { class A { get [Symbol.toStringTag]() { return 'A' } }; return new A() })()",
  "(() => { class A {}; A.prototype[Symbol.toStringTag] = 'Proto'; return new A() })()", "(() => { class A { constructor() { this.x = 1 } get [Symbol.toStringTag]() { return 'T' } }; return new A() })()",
  "(() => { class A { static [Symbol.toStringTag] = 'S' }; return A })()", "(() => { class A { get [Symbol.toStringTag]() { return 'T' } }; class B extends A {}; return new B() })()",
  "(() => { class A {}; return Object.defineProperty(new A(), Symbol.toStringTag, { value: 'Own' }) })()",
  "(() => { class A {}; return Object.defineProperty(new A(), Symbol.toStringTag, { value: 'Own', enumerable: true }) })()",
  "Object.create(null)", "Object.assign(Object.create(null), { a: 1 })", "Object.assign(Object.create(null), { a: 1, b: { c: 2 } })", "Object.setPrototypeOf({ a: 1 }, null)",
  "Object.create(null, { x: { value: 1, enumerable: true } })", "Object.create(null, { x: { value: 1 } })", "{ __proto__: null }", "{ __proto__: null, a: 1 }",
  "[Object.create(null)]", "{ a: Object.create(null) }", "{ a: Object.assign(Object.create(null), { b: 1 }) }", "[Object.assign(Object.create(null), { b: 1 })]",
  "Object.assign(Object.create(null), { [Symbol.toStringTag]: 'T' })", "Object.assign(Object.create(null), { [Symbol.toStringTag]: 'T', a: 1 })",
  "Object.setPrototypeOf([1, 2], null)", "Object.setPrototypeOf([], null)", "Object.setPrototypeOf(Object.assign([1], { a: 1 }), null)",
  "Object.setPrototypeOf(new Uint8Array(2), null)", "Object.setPrototypeOf(Promise.resolve(1), null)", "Object.setPrototypeOf(new ArrayBuffer(2), null)",
  "Object.setPrototypeOf(new Error('m'), null)", "Object.setPrototypeOf(Object.create(null), null)", "Object.create(Object.create(null))",
  "Object.create(Object.create(null), { a: { value: 1, enumerable: true } })", "(() => { const p = Object.create(null); p.x = 1; return Object.create(p) })()",
  "(() => { function F() {}; F.prototype = Object.create(null); return new F() })()", "(() => { function F() { this.a = 1 }; return new F() })()",
  "(() => { function F() {}; F.prototype.constructor = undefined; return new F() })()", "(() => { function F() {}; return Object.setPrototypeOf(new F(), null) })()",
  "(() => { class A { constructor() { this.a = 1 } }; return Object.setPrototypeOf(new A(), null) })()", "(() => { class A {}; return new A() })()",
  "(() => { class A { x = 1 }; return new A() })()", "(() => { class A { #p = 1; x = 2 }; return new A() })()", "(() => { class A extends Object {}; return new A() })()",
  "(() => { const o = {}; Object.defineProperty(o, 'constructor', { value: 5 }); return o })()", "(() => { const o = { constructor: 5 }; return o })()",
  "(() => { const o = { constructor: { name: 'Fake' } }; return o })()", "(() => { class A {}; const a = new A(); a.constructor = 5; return a })()",
  "(() => { class A {}; const a = new A(); Object.defineProperty(a, 'constructor', { value: { name: 'Fake' } }); return a })()",
  "(() => { class A {}; A.prototype.constructor = { name: 'Fake' }; return new A() })()", "(() => { class A {}; Object.defineProperty(A, 'name', { value: 'Renamed' }); return new A() })()",
  "(() => { class A {}; Object.defineProperty(A, 'name', { value: '' }); return new A() })()", "(() => { const A = class {}; return new A() })()", "new (class {})()",
  "new (class extends Object {})()", "(() => { class A { static name = 'S' }; return new A() })()",
]) {
  log(e);
}

// Promise.
const ph = "var p; ";
for (const e of [
  "new Promise(() => {})", "Promise.resolve(1)", "Promise.resolve('a')", "Promise.resolve({ a: 1 })", "Promise.resolve([1, 2])", "Promise.resolve()", "Promise.resolve(null)",
  "Promise.resolve(Promise.resolve(1))", "Promise.resolve(new Map([[1, 2]]))", "Promise.resolve(new Promise(() => {}))", "Promise.resolve(function f() {})",
  "Promise.resolve(Symbol('s'))", "Promise.resolve(1n)", "Promise.resolve(-0)", "Promise.resolve(S(new Error('m')))", "Promise.resolve(new Set([1]))",
  "Promise.resolve({ a: { b: { c: { d: 1 } } } })", "Promise.resolve('x'.repeat(200))", "Promise.resolve([[1, [2, [3, [4]]]]])",
  "(() => { const p = Promise.reject(1); p.catch(() => {}); return p })()", "(() => { const p = Promise.reject('a'); p.catch(() => {}); return p })()",
  "(() => { const p = Promise.reject({ a: 1 }); p.catch(() => {}); return p })()", "(() => { const p = Promise.reject(undefined); p.catch(() => {}); return p })()",
  "(() => { const p = Promise.reject(S(new Error('m'))); p.catch(() => {}); return p })()", "(() => { const p = Promise.reject(null); p.catch(() => {}); return p })()",
  "(() => { const p = Promise.reject([1]); p.catch(() => {}); return p })()", "(() => { const p = Promise.reject(new Map()); p.catch(() => {}); return p })()",
  "(() => { const p = new Promise(() => {}); p.x = 1; return p })()", "(() => { const p = Promise.resolve(1); p.x = 1; return p })()",
  "(() => { const p = Promise.reject(1); p.catch(() => {}); p.x = 1; return p })()", "Object.assign(Promise.resolve(1), { a: 1, b: 'x' })",
  "(() => { const p = Promise.resolve(1); return p.then(x => x) })()", "(() => { const p = Promise.resolve(1); p.then(() => {}); return p })()",
  "(() => { const p = new Promise(() => {}); return p.then(() => {}) })()", "(() => { const p = new Promise(() => {}); return p.finally(() => {}) })()",
  "(() => { const p = Promise.reject(1); return p.catch(() => 5) })()", "(() => { const p = Promise.reject(1); const q = p.then(() => {}); q.catch(() => {}); return q })()",
  "[Promise.resolve(1)]", "{ p: Promise.resolve(1) }", "[new Promise(() => {})]", "{ a: { p: Promise.resolve({ b: 1 }) } }", "new Map([[1, Promise.resolve(2)]])", "new Set([Promise.resolve(2)])",
  "(() => { const p = Promise.resolve(); p.self = p; return p })()", "(() => { const r = {}; const p = Promise.resolve(r); r.p = p; return p })()",
  "new (class P extends Promise {})(() => {})", "(() => { class P extends Promise {}; return P.resolve(1) })()", "(() => { class P extends Promise {}; return new P(r => r(2)) })()",
  "(() => { const p = Promise.resolve(1); p[Symbol.toStringTag] = 'X'; return p })()", "Promise.prototype", "Promise", "Promise.resolve", "Promise.withResolvers()",
  "Promise.all([])", "Promise.all([1, 2])", "Promise.allSettled([1])", "Promise.race([])", "Promise.any([1])", "(async () => {})()", "(async () => 5)()",
  "(async () => { await new Promise(() => {}) })()", "(async function* () {})()", "(function* () {})()", "(function* g() { yield 1 })()", "(async function* () { yield 1 })()",
  "(() => { const g = (function* () { yield 1 })(); g.next(); return g })()", "[(function* () {})()]", "{ g: (function* () {})() }", "[1, 2][Symbol.iterator]()",
  "[1, 2].entries()", "[1, 2].keys()", "[1, 2].values()", "'ab'[Symbol.iterator]()", "'ab'.matchAll(/a/g)", "Object.getPrototypeOf(function* () {})",
  "Object.getPrototypeOf(async function () {})", "Object.getPrototypeOf(async function* () {})", "Object.getPrototypeOf((function* () {})())",
  "globalThis.Iterator ? Iterator.prototype : 0", "globalThis.Iterator ? Iterator.from([1, 2]) : 0", "globalThis.Iterator ? [1, 2].values().map(x => x) : 0",
]) {
  log(e);
}
run("var p = Promise.resolve(1); console.log(p); console.log('%s', p); console.log('%o', p); console.log('%O', p); console.log('%j', p)");
run("var p = new Promise(() => {}); console.log('%s', p); console.log('%o', p); console.log('%O', p); console.log('%j', p)");
run("var p = Promise.reject(1); p.catch(() => {}); console.log('%s', p); console.log('%O', p)");
// Promise que muda de estado depois do log: o log vê o estado do momento.
run("var res; var p = new Promise(r => { res = r }); console.log(p); res(5); console.log(p)");
run("var res; var p = new Promise(r => { res = r }); res(5); console.log(p); p.then(() => console.log(p))");
run("var p = Promise.reject(new Error('x')); p.catch(() => {}); console.log(p)");
run("var p = Promise.resolve(1); setTimeout(() => console.log(p), 0); console.log('first')");

// Typed arrays, ArrayBuffer e afins.
const ta = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"];
for (const T of ta) {
  const big = T.startsWith("Big");
  const v = (n) => (big ? n + "n" : String(n));
  log(`new ${T}(0)`);
  log(`new ${T}(1)`);
  log(`new ${T}(3)`);
  log(`new ${T}([${v(1)}, ${v(2)}, ${v(3)}])`);
  log(`new ${T}(101)`);
  log(`new ${T}(100)`);
  log(`new ${T}(30)`);
  log(`Object.assign(new ${T}(2), { extra: 1 })`);
  log(`[new ${T}(2)]`);
  log(`{ t: new ${T}(2) }`);
  log(`new ${T}(new ArrayBuffer(16), 8)`);
  log(`new ${T}(2).subarray(1)`);
  log(`${T}.prototype`);
  log(T);
  log(`Object.getPrototypeOf(${T})`);
  log(`new (class X extends ${T} {})(2)`);
  log(`(() => { const t = new ${T}(2); t[Symbol.toStringTag]; return Object.defineProperty(t, Symbol.toStringTag, { value: 'Z' }) })()`);
  log(`new ${T}(2).buffer`);
  log(`new ${T}(2).entries()`);
  log(`new ${T}(2)[Symbol.iterator]()`);
  log(`new ${T}(new SharedArrayBuffer(8))`);
}
for (const e of [
  "new Float32Array([1.5, -0, NaN, Infinity, -Infinity])", "new Float64Array([0.1, 1e21, 1e-7, -0])", "new Float32Array([0.1])", "new Uint8Array([255, 256, -1])",
  "new Uint8ClampedArray([255, 256, -1, 1.5, 2.5])", "new Int8Array([127, 128, -129])", "new BigInt64Array([2n**63n, -1n])", "new BigUint64Array([2n**64n - 1n, 0n])",
  "new Float64Array([1, 2, 3]).map(x => x * 1.5)", "Uint8Array.from('abc', c => c.charCodeAt(0))", "new TextEncoder().encode('hello')", "new TextEncoder().encode('\\u00e9\\u20ac')",
  "new Uint8Array(0).buffer", "new ArrayBuffer(0)", "new ArrayBuffer(1)", "new ArrayBuffer(4)", "new ArrayBuffer(16)", "new ArrayBuffer(50)", "new ArrayBuffer(51)", "new ArrayBuffer(100)",
  "new ArrayBuffer(8, { maxByteLength: 16 })", "new ArrayBuffer(0, { maxByteLength: 16 })", "new ArrayBuffer(4).slice(1)", "Object.assign(new ArrayBuffer(2), { a: 1 })",
  "new SharedArrayBuffer(0)", "new SharedArrayBuffer(4)", "new SharedArrayBuffer(60)", "Object.assign(new SharedArrayBuffer(2), { a: 1 })",
  "(() => { const b = new ArrayBuffer(4); new Uint8Array(b).set([1, 2, 3, 4]); return b })()", "(() => { const b = new ArrayBuffer(60); new Uint8Array(b).fill(255); return b })()",
  "(() => { const b = new ArrayBuffer(8); b.transfer(); return b })()", "(() => { const b = new ArrayBuffer(8); b.transfer(); return new Uint8Array(b.slice ? 1 : 1) })()",
  "(() => { const b = new ArrayBuffer(8); const t = new Uint8Array(b); b.transfer(); return t })()", "(() => { const b = new ArrayBuffer(8, { maxByteLength: 16 }); const t = new Uint8Array(b); b.resize(12); return t })()",
  "(() => { const b = new ArrayBuffer(8, { maxByteLength: 16 }); const t = new Uint8Array(b, 4); b.resize(2); return t })()", "[new ArrayBuffer(2)]", "{ b: new ArrayBuffer(2) }",
  "Object.setPrototypeOf(new ArrayBuffer(2), null)", "new (class B extends ArrayBuffer {})(2)", "ArrayBuffer.prototype", "ArrayBuffer", "SharedArrayBuffer.prototype",
  "new DataView(new ArrayBuffer(4))", "new DataView(new ArrayBuffer(8), 2, 4)", "new DataView(new ArrayBuffer(0))", "Object.assign(new DataView(new ArrayBuffer(2)), { a: 1 })",
  "DataView.prototype", "DataView", "new Uint8Array(2).constructor", "Object.getPrototypeOf(Uint8Array.prototype)", "Object.getPrototypeOf(Uint8Array)",
  "new Map([[new Uint8Array(1), new ArrayBuffer(1)]])", "new Set([new Uint8Array(1)])", "Buffer.from('abc')", "Buffer.alloc(3)", "Buffer.alloc(0)", "Buffer.from([1, 2, 3])",
  "Buffer.alloc(60)", "Buffer.from('\\u00e9')", "[Buffer.from('a')]", "{ b: Buffer.from('a') }", "Object.assign(Buffer.from('a'), { x: 1 })", "Buffer.from('abc').buffer.byteLength > 0",
  "Buffer.prototype.constructor === Buffer", "Buffer", "Buffer.prototype", "Buffer.from('abc').subarray(1)",
  "new Blob(['abc'])", "new File(['abc'], 'a.txt', { lastModified: 5 })", "new URL('http://a.test/b?c=1#d')", "new URLSearchParams('a=1&b=2')", "new Headers({ a: '1' })", "new Request('http://a.test/')",
  "new Response('x')", "new AbortController()", "new AbortController().signal", "new Event('x')", "new EventTarget()", "new TextDecoder()", "new TextEncoder()",
  "structuredClone(new Map([[1, new Uint8Array(2)]]))", "(() => { const a = new Uint8Array(3); a.x = 1; return a })()", "(() => { const a = new Uint8Array(3); a[5] = 1; return a })()",
  "(() => { const a = new Uint8Array(3); a.foo = 'bar'; a.baz = { q: 1 }; return a })()", "(() => { const a = new Uint8Array(3); a[Symbol('s')] = 1; return a })()",
  "(() => { const a = new Uint8Array(2); a.self = a; return a })()", "(() => { const a = new Uint8Array(2); a.b = a.buffer; return a })()",
]) {
  log(e);
}
run("var t = new Uint8Array(3); console.log('%s', t); console.log('%o', t); console.log('%O', t); console.log('%j', t); console.log('%d', t)");
run("var b = new ArrayBuffer(4); console.log('%s', b); console.log('%o', b); console.log('%O', b); console.log('%j', b)");
run("console.log(new Uint8Array(2), new Uint8Array(2))");
run("console.log('a', new Uint8Array(2), 'b', new ArrayBuffer(2))");
run("console.log([new Uint8Array(2), [new Uint8Array(2), [new Uint8Array(2), [new Uint8Array(2)]]]])");

// Combinações de aninhamento, profundidade e quebra de linha.
for (const e of [
  "{ a: new Map([[1, { b: 2 }]]), c: new Set([[1, 2]]), d: new Date(0), e: /x/g, f: function g() {}, h: class I {}, j: Promise.resolve(1), k: new Uint8Array(2), l: new ArrayBuffer(2) }",
  "[new Map([[1, 2]]), new Set([1]), new Date(0), /x/, function f() {}, class A {}, Promise.resolve(1), new Uint8Array(1), new Number(1), new String('a')]",
  "{ a: { b: { c: { d: new Map([[1, 2]]) } } } }", "{ a: { b: { c: { d: new Set([1]) } } } }", "{ a: { b: { c: { d: function f() {} } } } }",
  "{ a: { b: { c: { d: new Date(0) } } } }", "{ a: { b: { c: { d: /x/ } } } }", "{ a: { b: { c: { d: Promise.resolve(1) } } } }", "{ a: { b: { c: { d: new Uint8Array(2) } } } }",
  "{ a: { b: { c: { d: S(new Error('m')) } } } }", "{ a: { b: { c: { d: new Number(1) } } } }", "{ a: { b: { c: { d: Object.create(null) } } } }", "{ a: { b: { c: { d: new ArrayBuffer(2) } } } }",
  "{ a: { b: { c: { d: class A {} } } } }", "{ a: { b: { c: { d: async () => {} } } } }", "{ a: { b: { c: new Map([[1, new Map([[2, new Map()]])]]) } } }",
  "[[[[new Map()]]]]", "[[[[new Set()]]]]", "[[[[function f() {}]]]]", "[[[[new Date(0)]]]]", "[[[[/x/]]]]", "[[[[Promise.resolve(1)]]]]", "[[[[new Uint8Array(1)]]]]",
  "[[[[S(new Error('m'))]]]]", "[[[[new Number(1)]]]]", "[[[[new ArrayBuffer(1)]]]]", "[[[[class A {}]]]]",
  "new Map([['aaaaaaaaaaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbb'], ['cccccccccccccccccccc', 'dddddddddddddddddddd'], ['eeeeeeeeeeeeeeeeeeee', 'ffffffffffffffffffff']])",
  "new Set(['aaaaaaaaaaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbb', 'cccccccccccccccccccc', 'dddddddddddddddddddd', 'eeeeeeeeeeeeeeeeeeee'])",
  "[function aaaaaaaaaaaaaaaaaaaa() {}, function bbbbbbbbbbbbbbbbbbbb() {}, function cccccccccccccccccccc() {}, function dddddddddddddddddddd() {}]",
  "[new Date(0), new Date(1), new Date(2), new Date(3), new Date(4), new Date(5)]", "[/aaaaaaaaaa/, /bbbbbbbbbb/, /cccccccccc/, /dddddddddd/, /eeeeeeeeee/, /ffffffffff/]",
  "Array.from({ length: 10 }, (_, i) => new Number(i))", "Array.from({ length: 10 }, (_, i) => new String('s' + i))", "Array.from({ length: 5 }, (_, i) => new Uint8Array(i))",
  "Array.from({ length: 5 }, (_, i) => new Map([[i, i]]))", "Array.from({ length: 5 }, (_, i) => new Set([i]))", "Array.from({ length: 5 }, (_, i) => function () {})",
  "Array.from({ length: 5 }, (_, i) => Promise.resolve(i))", "Array.from({ length: 3 }, (_, i) => S(new Error('e' + i)))",
  "{ f: function () {}, g: () => {}, h: class {}, i: async function () {}, j: function* () {}, k: Math.abs, l: class extends Object {} }",
  "{ m: new Map([['k', function () {}]]), s: new Set([class A {}]) }", "{ [Symbol('s')]: new Map([[1, 2]]) }", "{ 'key with space': new Set([1]), 'a-b': new Map() }",
  "{ date: new Date(0), nested: { date: new Date(1) } }", "{ re: /a/g, nested: { re: /b/ } }", "[1, [2, new Map([[3, [4, new Set([5])]]])]]",
  "new Map([[1, 2]]).set(3, new Map([[4, 5]])).set(6, new Set([7]))", "new Set([1]).add(new Set([2]).add(new Set([3]).add(new Set([4]))))",
  "Object.assign([new Map([[1, 2]])], { extra: new Set([1]) })", "Object.assign([function f() {}], { extra: class A {} })", "Object.assign([1, 2], { extra: new Date(0) })",
]) {
  log(e);
}
run("console.log(new Map([[1, 2]]), new Set([1]), new Date(0), /x/, function f() {}, class A {})");
run("console.log('%s|%s|%s|%s|%s|%s', new Map([[1, 2]]), new Set([1]), new Date(0), /x/, function f() {}, class A {})");
run("console.log('%o|%o|%o|%o', new Map([[1, 2]]), new Set([1]), new Date(0), /x/)");
run("console.log('%O|%O|%O|%O', new Map([[1, 2]]), new Set([1]), new Date(0), /x/)");
run("console.log('%j|%j|%j|%j', new Map([[1, 2]]), new Set([1]), new Date(0), /x/)");
run("console.log('%d|%d|%d|%d', new Map([[1, 2]]), new Set([1]), new Date(1), /x/)");
run("console.log('%i|%f', new Date(1), new Date(2))");
run("console.log('%s|%s|%s|%s', new WeakMap(), new WeakSet(), Object.create(null), Object.assign(Object.create(null), { a: 1 }))");
run("console.log('%s', { [Symbol.toStringTag]: 'T' }); console.log('%s', Promise.resolve(1)); console.log('%s', new Uint8Array(2)); console.log('%s', new ArrayBuffer(2))");
run("console.log('%s', new (class A {})(), new (class B extends Map {})([[1, 2]]))");
run("console.log(String(new Map()), String(new Set()), String(new Date(0)), String(/x/), String(function f() {}), String(class A {}), String(Promise.resolve(1)))");
run("console.log(`${new Map()}|${new Set()}|${/x/}|${Promise.resolve(1)}|${new Uint8Array(2)}|${new ArrayBuffer(2)}|${Object.create(null) instanceof Object}`)");
run("console.log(Object.prototype.toString.call(new Map()), Object.prototype.toString.call(new Set()), Object.prototype.toString.call(new Date(0)), Object.prototype.toString.call(/x/), Object.prototype.toString.call(Promise.resolve(1)), Object.prototype.toString.call(new Uint8Array(1)), Object.prototype.toString.call(new ArrayBuffer(1)), Object.prototype.toString.call(function () {}), Object.prototype.toString.call(async function () {}), Object.prototype.toString.call(function* () {}), Object.prototype.toString.call({ [Symbol.toStringTag]: 'T' }))");

// Membros herdados (`forEachPropertyImpl`): métodos não enumeráveis de classe, getter e setter no protótipo, dois níveis,
// símbolo, o limite de cinco níveis e o dono mais próximo da chave. Ficam no fim para não deslocar as linhas anteriores.
for (const e of [
  "(() => { class A { m() {} } return new A() })()", "(() => { class A { get g() { return 1 } } return new A() })()",
  "(() => { class A { constructor() { this.x = 1 } m() {} get g() { return 1 } } return new A() })()",
  "(() => { const p = { get g() { return 1 } }; return Object.create(p) })()", "(() => { const p = { set s(v) {} }; return Object.create(p) })()",
  "(() => { const p = { a: 1, get g() { return 1 } }; return Object.create(p) })()",
  "(() => { const p = {}; Object.defineProperty(p, 'h', { value: 1, enumerable: false }); p.v = 2; return Object.create(p) })()",
  "(() => { const p = {}; Object.defineProperty(p, 'h', { value: 1, enumerable: false }); return Object.create(p) })()",
  "(() => { const p1 = { a: 1 }; const p2 = Object.create(p1); p2.b = 2; return Object.create(p2) })()",
  "(() => { const p1 = { a: 1 }; const p2 = Object.create(p1); return Object.create(p2) })()",
  "(() => { const p = { [Symbol('s')]: 1, a: 2 }; return Object.create(p) })()",
  "(() => { const p = { a: 1 }; const o = Object.create(p); o.b = 2; return o })()",
  "(() => { let p = { k0: 0 }; for (let i = 1; i < 8; i++) { p = Object.create(p); p['k' + i] = i } return Object.create(p) })()",
  "(() => { class A { m() {} } A.prototype.v = 1; class B extends A {} B.prototype.w = 2; return new B() })()",
  "(() => { const p = { a: 1 }; const o = Object.create(p); Object.defineProperty(o, 'a', { value: 5, enumerable: false }); return o })()",
  "(() => { class A {} A.prototype.m = function () {}; return new A() })()",
  "(() => { const p = { [Symbol.toStringTag]: 'T', a: 1 }; return Object.create(p) })()",
  "[1, (() => { const p = { a: 1 }; return Object.create(p) })()]",
]) {
  log(e);
}

// Orçamento de níveis do `forEachPropertyImpl` (caminho rápido confere depois de cada nível, o lento antes; objeto vazio
// começa no primeiro protótipo) e `@@toStringTag` não enumerável ou como única chave. Ficam no fim, depois das anteriores.
const chain = (own, getterAt) =>
  `(() => { let prev = Object.prototype; for (let i = 7; i >= 1; i--) { const p = Object.create(prev); if (i === ${getterAt}) Object.defineProperty(p, 'p' + i, { get() { return i }, enumerable: true }); else p['p' + i] = i; prev = p } const o = Object.create(prev); ${own ? "o.own = 0;" : ""} return o })()`;
for (const e of [
  chain(true, 0), chain(false, 0), chain(true, 1), chain(false, 1), chain(false, 3), chain(true, 3), chain(false, 6), chain(false, 7),
  "(() => { class A { m() {} } class B extends A { n() {} } class C extends B { o() {} } class D extends C { p() {} } class E extends D { q() {} } class F extends E { r() {} } class G extends F { s() {} } return new G() })()",
  "(() => { const p = {}; Object.defineProperty(p, 'h', { value: 1 }); return (() => { let q = p; for (let i = 0; i < 6; i++) { q = Object.create(q); q['k' + i] = i } return Object.create(q) })() })()",
  "Object.defineProperty({}, Symbol.toStringTag, { value: 'T' })", "Object.defineProperty({ a: 1 }, Symbol.toStringTag, { value: 'T' })",
  "Object.defineProperty(Object.create(null), Symbol.toStringTag, { value: 'T' })",
  "Object.defineProperty({}, Symbol.toStringTag, { value: 'T', enumerable: true })",
  "Object.create(Object.defineProperty({}, Symbol.toStringTag, { value: 'T' }))",
  "Object.create(Object.defineProperty({}, Symbol.toStringTag, { value: 'T', enumerable: true }))",
  "Object.create(Object.defineProperty({ a: 1 }, Symbol.toStringTag, { value: 'T' }))",
  "Object.create({ get [Symbol.toStringTag]() { return 'T' } })", "new (class A { get [Symbol.toStringTag]() { return 'T' } })()",
  "Object.defineProperty({}, Symbol.toStringTag, { get() { return 'T' }, enumerable: true })",
]) {
  log(e);
}

// Error aninhado em array e objeto, subclasse e protótipo nulo (o relato completo do erro no stdout). No fim, para as
// linhas antigas não mudarem de índice. O prelúdio troca `S` por uma linha em branco: o bun descarta `var S` do fonte do
// `eval` (nenhuma atribuição sobrevive, nem chamada), o que mudaria o trecho de contexto, e o erro não passa por `S`
// porque ele reescreve `stack` e o bun então não mostra o trecho nem os frames reais.
for (const e of [
  "[new Error('m'), 1]", "({ e: new RangeError('r'), n: 1 })", "{ a: [new TypeError('t')] }",
  "(() => { class MyErr extends Error {}; return [new MyErr('m')] })()",
  "(() => { class MyErr extends TypeError { constructor(m) { super(m); this.name = 'MyErr' } }; return { e: new MyErr('m') } })()",
  "Object.setPrototypeOf(new Error('m'), null)", "[Object.setPrototypeOf(new Error('m'), null)]", "new Error('m')", "new TypeError('t')",
]) {
  programs.push(E + "\n" + `try { console.log(${e}) } catch (e) { R = E(e) }`);
  // O mesmo caso como programa principal (`/app/main.js`), sem o invólucro de `eval`: os frames e o trecho de fonte do
  // relato são os do programa. Vai para `console_error_main_bun.tsv` (fonte, stderr, código de saída e stdout).
  mainSources.push(E + "\n" + `try { console.log(${e}) } catch (e) { R = E(e) }\n`);
}

// `Symbol.for('nodejs.util.inspect.custom')`: a string de retorno sai crua, o resto é formatado de novo; `(depth, options,
// inspect)`; a exceção do método escapa do `console.log`. Fica no fim para não deslocar os índices acima (linhas 956 em
// diante). O terceiro argumento (`util.inspect`) não é medido: o porte ainda não tem `node:util`.
const CUSTOM = "var C = Symbol.for('nodejs.util.inspect.custom');\n";
for (const e of [
  "{ [C]() { return 'hello' } }",
  "{ [C]() { return { a: 1, b: [1, 2] } } }",
  "{ [C]() { return 42 } }",
  "{ x: 1, [C]() { return this } }",
  "{ k: { [C]() { return 'raw' } } }",
  "[{ [C]() { return 'raw' } }]",
  "{ k: { [C]() { return 'a\\nb' } } }",
  "{ k: { [C]() { return { p: 1, q: { r: 2 } } } } }",
  "new Map([['a', { [C]() { return 'mv' } }]])",
  "new Set([{ [C]() { return 'sv' } }])",
  "{ [C](d, o) { return `d=${d} t=${typeof o} ${o.depth} ${o.colors}` } }",
  "{ k: { [C](d, o) { return `d=${d} ${o.depth}` } } }",
  "{ a: { b: { c: { [C](d) { return 'deep' + d } } } } }",
  "[[[{ [C](d) { return 'd' + d } }]]]",
  "{ get [C]() { return () => 'getter' } }",
  "new (class A { [C]() { return 'proto' } })()",
  "[new (class B extends (class A { [C]() { return 'proto' } }) {})()]",
  "Object.create({ [C]() { return 'inh' } })",
  "{ [C]: () => 'arrow' }",
  "{ [C]: 1 }",
  "{ [C]: 'notfn' }",
  "{ [C]() { return undefined } }",
  "{ [C]() { return null } }",
  "{ [C]() { return '' } }",
  "{ [C]() { return 1n } }",
  "{ [C]() { return Symbol('s') } }",
  "{ [C]() { return [1, 2, { a: 3 }] } }",
  "{ [C]() { return new Map([[1, 2]]) } }",
  "{ [C]() { return { [C]() { return 'inner' } } } }",
  "{ [C]() { return [{ [C]() { return 'inner2' } }] } }",
  "{ [C]() { return 'x\\ny' }, z: 1 }",
  "[{ [C]() { return 'x\\ny' } }, 2]",
  "{ [C]() { throw new Error('boom') } }",
  "{ k: [{ [C]() { throw 5 } }] }",
  "new TextEncoderStream()",
  "{ a: new TextEncoderStream() }",
  "new Headers({ a: 'b' })",
  "{ h: new Headers({ a: 'b' }) }",
  "new ReadableStream()",
  "{ r: new ReadableStream() }",
]) {
  programs.push(E + CUSTOM + `try { console.log(${e}) } catch (e) { R = typeof e === 'object' ? E(e) : String(e) }`);
}
for (const [format, e] of [
  ["%o", "{ [C]() { return 'pct-o' } }"],
  ["%O", "{ [C]() { return 'pct-O' } }"],
  ["%s", "{ [C]() { return 'pct-s' } }"],
  ["%j", "{ [C]() { return 'pct-j' } }"],
  ["%s|%d", "{ [C]() { return 'q' } }, 1"],
]) {
  programs.push(E + CUSTOM + `console.log(${JSON.stringify(format)}, ${e})`);
}
programs.push(E + CUSTOM + "console.log({ [C]() { return 'a' } }, 'b', { [C]() { return 'c' } })");
programs.push(E + CUSTOM + "console.dir({ k: { [C](d, o) { return `d=${d} ${o.depth} ${o.colors}` } } }, { depth: 5 })");
programs.push(E + CUSTOM + "console.dir({ k: { [C](d, o) { return `d=${d} ${o.depth} ${o.colors} ${o.stylize('x', 'string')}` } } }, { depth: 5, colors: true })");
programs.push(E + CUSTOM + "console.dir({ [C]() { return { a: 1 } } }, { colors: true })");
programs.push(E + "console.dir(Performance.prototype, { colors: true })");
programs.push(E + "console.dir({ p: Performance.prototype }, { colors: true, depth: 0 })");
programs.push(E + "console.dir(Object.keys(performance), { colors: true })");
programs.push(E + CUSTOM + "console.dir({ [C](d, o) { return `d=${d} ${o.depth} ${o.colors}` } }, { depth: null })");

// Classes web com `inspect.custom` nativo ou impressão própria do bun (medidas com `Object.getOwnPropertySymbols`).
// `Headers`, `URLSearchParams` e `FormData` não passam pelo símbolo: o bun imprime o `toJSON` com chaves entre aspas.
for (const e of [
  "new TextDecoderStream()",
  "new CompressionStream('gzip')",
  "new DecompressionStream('gzip')",
  "new WritableStream()",
  "new TransformStream()",
  "new CountQueuingStrategy({ highWaterMark: 3 })",
  "new ByteLengthQueuingStrategy({ highWaterMark: 3 })",
  "new ReadableStream({ type: 'bytes' })",
  "new ReadableStream().getReader()",
  "new WritableStream().getWriter()",
  "new URL('http://a.com/x?y=1#z')",
  "{ u: new URL('http://a.com/') }",
  "new URLSearchParams('a=1&b=2')",
  "new URLSearchParams('a=1&a=2')",
  "new URLSearchParams()",
  "{ p: new URLSearchParams('a=1') }",
  "new Headers()",
  "[new Headers({ a: 'b', c: 'd' })]",
  "new Headers([['set-cookie', 'a=1'], ['set-cookie', 'b=2']])",
  // Iteradores nativos (medidos no bun 1.4.2): saem como `<Classe> Iterator { next, toArray, ... }`, com os membros do
  // protótipo do iterador e do `%IteratorPrototype%` (helpers de iterador), nunca com os itens.
  "new Headers({ a: '1' }).entries()",
  "new Headers({ a: '1' }).keys()",
  "new Headers({ a: '1' }).values()",
  "new URLSearchParams('a=1').entries()",
  "new URLSearchParams('a=1').keys()",
  "new URLSearchParams('a=1').values()",
  "new FormData().entries()",
  "{ i: new Headers({ a: '1' }).entries() }",
]) {
  programs.push(E + `try { console.log(${e}) } catch (e) { R = typeof e === 'object' ? E(e) : String(e) }`);
}
// Com cores (`Bun.inspect(x, { colors: true })` no bun), `Headers`, `URLSearchParams` e o iterador de `Headers`.
for (const e of [
  "new Headers({ a: '1' })",
  "new URLSearchParams('a=1')",
  "new Headers({ a: '1' }).entries()",
]) {
  programs.push(E + `console.dir(${e}, { colors: true })`);
}
// O canal aberto segura o laço de eventos (medido): fecha depois de imprimir para o programa terminar.
programs.push(E + "var c = new BroadcastChannel('x'); try { console.log(c) } catch (e) { R = typeof e === 'object' ? E(e) : String(e) } c.close()");
for (const d of [0, 1, -1]) {
  programs.push(E + `var c = new BroadcastChannel('x'); console.log(c[Symbol.for('nodejs.util.inspect.custom')](${d}, {})); c.close(); console.log(c[Symbol.for('nodejs.util.inspect.custom')](1, {}))`);
}
programs.push(E + "console.log(require('util').inspect(new TransformStream(), { depth: 0 })); console.log(require('util').inspect(new TransformStream()))");
programs.push(E + "var f = new FormData(); f.append('a', '1'); f.append('a', '2'); f.append('b', 'x'); console.log(f); console.log(new FormData())");

// Classes web nativas (Request, Response, Blob, File, AbortSignal, AbortController, Event e subclasses, EventTarget,
// DOMException): o bun imprime por classe (`printAs`), com o tamanho no cabeçalho e o corpo como linha `Blob (...)` sem vírgula.
for (const e of [
  "new Request('http://a.test/')",
  "new Request('http://a.test/x', { method: 'POST', body: 'hi', headers: { a: '1' } })",
  "new Request('http://a.test/', { method: 'POST', body: 'a'.repeat(3000) })",
  "{ a: new Request('http://a.test/') }",
  "[new Request('http://a.test/')]",
  "new Response()",
  "new Response('hi', { status: 201, headers: { a: '1' } })",
  "new Response(new Blob(['abc'], { type: 'text/plain' }))",
  "Response.error()",
  "Response.redirect('http://a.test/', 301)",
  "Response.json({ a: 1 })",
  "{ a: new Response('hi') }",
  "(() => { const r = new Response('x'); r.text(); return r })()",
  "new Blob([])",
  "new Blob(['abc'], { type: 'text/plain' })",
  "new Blob(['a'.repeat(999)])",
  "new Blob(['a'.repeat(1000)])",
  "new Blob(['a'.repeat(1024)])",
  "new Blob(['a'.repeat(1536)])",
  "new Blob(['a'.repeat(10240)])",
  "new Blob(['a'.repeat(1500000)], { type: 'x/y' })",
  "[new Blob(['abc'], { type: 'x/y' }), new Blob([])]",
  "{ a: { b: new Blob(['abc'], { type: 'x/y' }) } }",
  "new File(['abc'], 'a.txt', { lastModified: 5 })",
  "new File(['abc'], 'a.txt', { type: 'text/plain', lastModified: 5 })",
  "new File([], 'e', { lastModified: 7 })",
  "new AbortSignal()",
  "new AbortController().signal",
  "new AbortController()",
  "{ a: new AbortController() }",
  "(() => { const c = new AbortController(); c.abort(); return c.signal })()",
  "AbortSignal.abort('r')",
  "AbortSignal.timeout(1000)",
  "{ a: AbortSignal.timeout(1000), b: new AbortController(), c: new EventTarget() }",
  "new CustomEvent('x', { detail: 1 })",
  "{ a: new CustomEvent('x', { detail: { k: 1 } }) }",
  "new Event('x')",
  "{ a: new Event('x', { bubbles: true }) }",
  "new EventTarget()",
  "new MessageEvent('message', { data: 1 })",
  "new ErrorEvent('error', { message: 'm' })",
  "new CloseEvent('close')",
  "new DOMException('m', 'AbortError')",
  "new DOMException()",
  "[new DOMException('m')]",
]) {
  programs.push(E + `try { console.log(${e}) } catch (e) { R = typeof e === 'object' ? E(e) : String(e) }`);
}
for (const e of [
  "new Request('http://a.test/')",
  "new Response('hi', { headers: { a: '1' } })",
  "new Blob(['abc'], { type: 'text/plain' })",
  "new File(['abc'], 'a.txt', { lastModified: 5 })",
  "new AbortController()",
  "new AbortController().signal",
  "AbortSignal.timeout(1000)",
  "new EventTarget()",
  "new Event('x')",
  "new CustomEvent('x', { detail: 1 })",
  "{ a: new CustomEvent('x', { detail: 1 }), b: new AbortController() }",
  "new DOMException('m', 'AbortError')",
  "{ a: { b: new Blob(['abc'], { type: 'x/y' }) } }",
]) {
  programs.push(E + `console.dir(${e}, { colors: true }); console.dir(${e}, { depth: 0 }); console.dir({ k: ${e} }, { depth: 0 })`);
}
// Medidos no bun 1.4.2: `Response` com `statusText` não vazio e `Response.redirect` (a `url` fica vazia), `Blob` com `type`
// aninhado no corpo de `Request` e `Response`, e o `CloseEvent` com os campos preenchidos, todos com e sem cores.
for (const e of [
  "Response.redirect('http://a.com/x', 302)",
  "new Response('x', { status: 404, statusText: 'Nope' })",
  "new Request('http://a.com/', { method: 'POST', body: new Blob(['ab'], { type: 'text/plain' }) })",
  "new Response(new Blob(['ab'], { type: 'text/plain' }))",
  "new CloseEvent('close', { code: 1000, reason: 'bye', wasClean: true })",
]) {
  programs.push(E + `console.dir(${e}, { colors: true }); console.dir(${e}, { colors: false })`);
}

// Objetos web aninhados fundo: `Headers` dentro de `Request` dentro de array dentro de objeto, `Map` com `Request` de valor,
// e os marcadores de profundidade excedida (`Headers [Object ...]`, `[URLSearchParams ...]`, `[FormData ...]`,
// `[MessageEvent ...]`), com `depth` 0, 1, 2 e null, com e sem cores. O `DOMException` com cores vai no mesmo laço.
const REQ = "new Request('http://a.test/', { headers: { a: '1' } })";
for (const e of [
  "new DOMException('m', 'AbortError')",
  `{ r: [ ${REQ} ] }`,
  `new Map([['k', ${REQ}]])`,
  `{ o: { a: [ ${REQ} ] } }`,
  "{ h: new Headers({ a: '1' }) }",
  "{ o: { h: new Headers({ a: '1' }) } }",
  "{ o: { h: new Headers() } }",
  "{ o: { h: new URLSearchParams('a=1') } }",
  "{ o: { h: new URLSearchParams() } }",
  "{ o: { h: (() => { const f = new FormData(); f.append('a', 'b'); return f })() } }",
  "{ o: { h: new FormData() } }",
  "{ o: { e: new MessageEvent('m', { data: 1 }) } }",
  "{ o: { h: new Response('a') } }",
  "{ o: { e: new Event('x') } }",
  "{ o: { e: new CustomEvent('x') } }",
  "{ o: { e: new CloseEvent('close') } }",
  "{ o: { e: new ErrorEvent('error') } }",
  "{ o: { e: new EventTarget() } }",
  "{ o: { e: new AbortController() } }",
  "{ o: { e: new AbortController().signal } }",
  "{ o: { e: new DOMException('m') } }",
  "{ o: { e: new TextEncoder() } }",
  "{ o: { e: new TextDecoder() } }",
  "{ o: { e: new PerformanceObserver(() => {}) } }",
  "new PerformanceObserver(() => {})",
  "PerformanceObserver.prototype",
  "{ o: { e: PerformanceObserver.prototype } }",
  "TextEncoder.prototype",
  "{ o: { e: TextEncoder.prototype } }",
  "TextDecoder.prototype",
  "{ o: { e: TextDecoder.prototype } }",
  "{ o: { e: new MessageChannel().port1 } }",
  "{ o: { e: new MessageChannel() } }",
  "{ o: { e: new (class Sub extends Event {})('x') } }",
  "{ o: { e: Object.create(Event.prototype) } }",
]) {
  for (const depth of ["0", "1", "2", "null"]) {
    for (const colors of ["true", "false"]) {
      programs.push(E + `console.dir(${e}, { depth: ${depth}, colors: ${colors} })`);
    }
  }
}

// Classes com inspect custom nativo: com `depth` esgotado saem como `Nome [Object]`.
for (const e of [
  "new ReadableStream()",
  "new WritableStream()",
  "new ByteLengthQueuingStrategy({ highWaterMark: 1 })",
  "new CountQueuingStrategy({ highWaterMark: 1 })",
  "new TextEncoderStream()",
  "new TextDecoderStream()",
  "new CompressionStream('gzip')",
  "new DecompressionStream('gzip')",
]) {
  for (const depth of ["0", "1", "2", "null"]) {
    for (const colors of ["true", "false"]) {
      programs.push(`console.dir({ a: ${e} }, { depth: ${depth}, colors: ${colors} })`);
      programs.push(`console.dir([[[${e}]]], { depth: ${depth}, colors: ${colors} })`);
    }
  }
}

// `PerformanceMark`, `PerformanceMeasure`, `CryptoKey` e `URL`: o inspect custom monta `Nome { campos }` no formato do
// `util.inspect` (depth menos um; zero dá `Nome [Object]`, `detail` e `algorithm` aninhados viram `[Object]`).
for (const e of [
  "new PerformanceMark('a', { startTime: 5 })",
  "new PerformanceMark('a', { startTime: 5, detail: { a: { b: { c: { d: 1 } } } } })",
  "new PerformanceMark('a', { startTime: 5, detail: [1, 'x', null, true] })",
  "new PerformanceMark('a', { startTime: 5, detail: 'texto' })",
  "new PerformanceMark('a', { startTime: 5, detail: Array.from({ length: 30 }, (_, i) => i * 7) })",
  "new PerformanceMark('a', { startTime: 5, detail: ['apple', 'b', 'cherry', 'dd', 'eeeee', 'f', 'gg', 'hhhh'] })",
  "new PerformanceMark('a', { startTime: 5, detail: [1, 'x', 2, 3, 4, 5, 6, 7] })",
  "new PerformanceMark('a', { startTime: 5, detail: { k: Array.from({ length: 7 }, (_, i) => i) } })",
  "performance.measure('m', { start: 1, end: 3 })",
  "performance.measure('m', { start: 1, end: 3, detail: { k: 1 } })",
  "Performance.prototype",
  "{ p: Performance.prototype }",
  "EventTarget.prototype",
  "Object.keys(performance).concat(Object.getOwnPropertyNames(Object.getPrototypeOf(Object.getPrototypeOf(performance))))",
  "Object.getOwnPropertyNames(Object.getPrototypeOf(Performance.prototype))",
  "new URL('http://a.com/p?x=1')",
  "new URL('http://a.com/p?x=1&y=2#h')",
  "new URL('http://a.com/')",
]) {
  for (const depth of ["0", "1", "2", "3", "null"]) {
    for (const colors of ["true", "false"]) {
      programs.push(`console.dir(${e}, { depth: ${depth}, colors: ${colors} })`);
      programs.push(`console.dir({ a: [${e}] }, { depth: ${depth}, colors: ${colors} })`);
    }
  }
}
for (const algorithm of [
  "{ name: 'AES-GCM', length: 256 }",
  "{ name: 'HMAC', hash: 'SHA-256' }",
  "{ name: 'ECDSA', namedCurve: 'P-256' }",
]) {
  for (const depth of ["0", "1", "2", "null"]) {
    for (const colors of ["true", "false"]) {
      const usage = algorithm.includes("AES") ? "['encrypt', 'decrypt']" : "['sign', 'verify']";
      const generate = `crypto.subtle.generateKey(${algorithm}, true, ${usage})`;
      programs.push(`${generate}.then((k) => console.dir(k.privateKey ?? k, { depth: ${depth}, colors: ${colors} }))`);
      programs.push(`${generate}.then((k) => console.dir({ a: k.privateKey ?? k }, { depth: ${depth}, colors: ${colors} }))`);
    }
  }
}
// `util.inspect` dos tipos além de objeto simples: mais de cem itens, circular, símbolo, função, classe, Map, Set,
// typed array e instância de classe, no `detail` de PerformanceMark.
for (const detail of [
  "Array.from({ length: 101 }, (_, i) => i)",
  "Array.from({ length: 150 }, (_, i) => 'v' + i)",
  "(() => { const c = { a: 1 }; c.self = c; return c; })()",
  "(() => { const c = { a: { b: 1 } }; c.a.up = c; c.a.me = c.a; return c; })()",
  "Symbol('s')",
  "{ s: Symbol('t'), f: function f() {}, g: () => 1, h: class H {}, i: class I extends Array {} }",
  "{ f: async function af() {}, g: function* gen() {}, a: (() => {}) }",
  "new Map([[1, 2], ['a', { b: 1 }]])",
  "new Map()",
  "new Set([1, 2, 'x'])",
  "new Set()",
  "new Set(Array.from({ length: 102 }, (_, i) => i))",
  "new Uint8Array([1, 0, 1])",
  "new Float64Array([1.5, 2.5])",
  "new Uint8Array(20)",
  "(() => { class A { constructor() { this.x = 1; } } return new A(); })()",
  "(() => { class A {} return { a: new A() }; })()",
  "(() => { class A { constructor() { this.m = new Map([[1, new Set([2])]]); } } return new A(); })()",
  "{ [Symbol('k')]: 1, a: 2 }",
  "{ f: Object.assign(function g() {}, { x: 1 }) }",
  "{ f: Object.assign(() => 1, { x: { y: 2 } }) }",
  "(() => { const o = Object.create(null); o.a = 1; return o; })()",
  "{ n: Object.create(null) }",
  "(() => { class M extends Map {} return new M([[1, 2]]); })()",
  "(() => { class S extends Set {} return new S([1]); })()",
  "(() => { class A {} class B extends A {} return { b: new B() }; })()",
  "{ o: Object.create({}) }",
  "(() => { class B {} const o = Object.create(new B()); o.q = 1; return o; })()",
  "new Date(0)",
  "new Date(NaN)",
  "new Date(1e12)",
  "{ d: new Date(0), e: [new Date(NaN)] }",
  "(() => { class D extends Date {} return new D(0); })()",
  "Object(1)",
  "Object(-0)",
  "Object('a')",
  "Object(true)",
  "{ n: Object(2), s: Object('x'), b: Object(false) }",
  "(() => { class N extends Number {} return new N(1); })()",
]) {
  for (const depth of ["0", "1", "2", "null"]) {
    for (const colors of ["true", "false"]) {
      programs.push(`console.dir(performance.mark('x', { detail: ${detail} }), { depth: ${depth}, colors: ${colors} })`);
    }
  }
}

for (const code of programs) {
  const source = code.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cp-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `(0, eval)("var R");\n(0, eval)(${JSON.stringify(source)});\nprocess.stderr.write("\\u0000R" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
  );
  // Sem `encoding`: os dois fluxos ficam em bytes; o stderr do programa vem antes do marcador `\0R`.
  const result = spawnSync(process.execPath, [file], { timeout: 15000, input: "", env: { ...process.env, TZ: "UTC" } });
  fs.rmSync(dir, { recursive: true, force: true });
  if (result.status !== 0) throw new Error("bun falhou em: " + source + "\n" + result.stderr);
  const at = result.stderr.lastIndexOf(Buffer.from("\u0000R"));
  // O bun imprime `Error` com os quadros reais (`at file:///<tmp>/case.js:L:C`) e ignora `e.stack` sobrescrito; o diretório
  // temporário é trocado por um nome fixo, e linha e coluna são determinísticas porque o invólucro é fixo.
  const norm = (buf) => {
    const text = buf.toString("latin1").split("file://" + file).join("file:///case.js").split(file).join("/case.js").split(dir).join("/dir");
    if (/\/tmp\/|\/home\//.test(text)) throw new Error("caminho da máquina na saída de: " + source + "\n" + text);
    return Buffer.from(text, "latin1");
  };
  emitRow(
    [
      JSON.stringify(source),
      JSON.stringify(norm(result.stdout).toString("hex")),
      JSON.stringify(norm(result.stderr.subarray(0, at)).toString("hex")),
      result.stderr.subarray(at + 2).toString("utf8"),
    ].join("\t"),
  );
}

fs.writeFileSync(path.join(__dirname, "..", "tests", "golden", "console_error_main_bun.tsv"), mainSources.map((source) => runMain(source, true)).join("\n") + "\n");
