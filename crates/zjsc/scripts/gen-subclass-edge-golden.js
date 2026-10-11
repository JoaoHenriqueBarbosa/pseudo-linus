// Gera tests/golden/subclass_edge_bun.tsv: herança de builtins (class extends Array/Map/Set/Promise/RegExp/Error/Date/
// Function/ArrayBuffer/Uint8Array/Boolean/Number/String/...; Symbol e Proxy dão erro), `new.target` em
// Reflect.construct com newTarget diferente (protótipo vem do newTarget, com fallback para o realm do newTarget),
// cross-realm via ShadowRealm (JSC puro, sem host), Symbol.species em map/filter/slice/then, constructor sobrescrito,
// toString/Symbol.toStringTag, instanceof, length/name dos construtores, `super` em métodos de objeto literal,
// Object.setPrototypeOf em instâncias de builtins, Error.captureStackTrace em subclasses e `cause`. Medido no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// O programa roda por `vm.runInThisContext` (ProgramExecutable do JSC puro) só dentro do gerador; os programas
// não referenciam `vm` nem nenhuma API do host. Caminho da máquina no resultado descarta o programa.
// Uso: bun scripts/gen-subclass-edge-golden.js > tests/golden/subclass_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);

// Builtin: [nome, argumentos de construção, expressão que descreve o estado interno da instância `o`].
const builtins = [
  ["Array", "3", "o.length"],
  ["Array", "1,2,3", "o.join('-')"],
  ["Map", "[[1,2]]", "o.size + ':' + o.get(1)"],
  ["Set", "[1,2,2]", "o.size + ':' + o.has(2)"],
  ["WeakMap", "", "o.has({})"],
  ["WeakSet", "", "o.has({})"],
  ["Promise", "r => r(7)", "typeof o.then"],
  ["RegExp", "'a+','g'", "o.source + '/' + o.flags + ':' + o.test('caat')"],
  ["Error", "'boom', { cause: 'c' }", "o.message + '|' + o.cause"],
  ["TypeError", "'tt'", "o.message"],
  ["RangeError", "'rr'", "o.message"],
  ["AggregateError", "[1,2], 'agg'", "o.errors.length + o.message"],
  ["Date", "0", "o.getTime()"],
  ["Function", "'a', 'return a + 1'", "o(2)"],
  ["ArrayBuffer", "8", "o.byteLength"],
  ["DataView", "new ArrayBuffer(4)", "o.byteLength"],
  ["Uint8Array", "[1,2,300]", "o.join() + ':' + o.length"],
  ["Float64Array", "2", "o.length"],
  ["Boolean", "1", "o.valueOf()"],
  ["Number", "'5'", "o.valueOf() + 1"],
  ["String", "'abc'", "o.length + o.charAt(1)"],
  ["Object", "", "typeof o"],
  ["Symbol", "'s'", "typeof o"],
  ["Proxy", "{}, {}", "typeof o"],
];
// Builtins que o usuário pede no enunciado mas que não são construtíveis com `new` direto (Symbol): ficam como erro.
const uniq = [];
for (const b of builtins) uniq.push(b);

for (const [name, args, state] of uniq) {
  const n = JSON.stringify(name);
  const tag = `Object.prototype.toString.call(o)`;
  // class extends simples
  T(`class C extends ${name} {}; var o = new C(${args}); R = [o instanceof C, o instanceof ${name}, Object.getPrototypeOf(o) === C.prototype, ${tag}, C.name, C.length, Object.getPrototypeOf(C) === ${name}, ${state}].join('|')`);
  // constructor sobrescrito com campo
  T(`class C extends ${name} { f = 1; constructor(...a) { super(...a); this.g = 2 } }; var o = new C(${args}); R = [o.f, o.g, Object.keys(o).join(), ${tag}, C.length, ${state}].join('|')`);
  // constructor sem chamar super
  T(`class C extends ${name} { constructor() { } }; new C(); R = 'ok'`);
  T(`class C extends ${name} { constructor() { this.x = 1; super(${args}) } }; new C(); R = 'ok'`);
  T(`class C extends ${name} { constructor() { super(${args}); super(${args}) } }; new C(); R = 'ok'`);
  // chamada sem new
  // Date() sem new devolve a hora corrente: o golden grava só se o formato bate, não o instante.
  if (name === "Date") T(String.raw`R = /^\w{3} \w{3} \d\d \d{4} \d\d:\d\d:\d\d GMT[+-]\d{4} \(.+\)$/.test(Date(${args}))`);
  else T(`R = String(${name}(${args}))`);
  T(`class C extends ${name} {}; C(); R = 'ok'`);
  // Reflect.construct com newTarget diferente
  T(`function F() {}; var o = Reflect.construct(${name}, [${args}], F); R = [Object.getPrototypeOf(o) === F.prototype, ${tag}, o instanceof F, o instanceof ${name}].join('|')`);
  T(`function F() {}; F.prototype = 1; var o = Reflect.construct(${name}, [${args}], F); R = [Object.getPrototypeOf(o) === ${name}.prototype, ${tag}].join('|')`);
  T(`var F = function () {}.bind(); var o = Reflect.construct(${name}, [${args}], F); R = [Object.getPrototypeOf(o) === ${name}.prototype, ${tag}].join('|')`);
  T(`var P = {}; function F() {}; F.prototype = P; var o = Reflect.construct(${name}, [${args}], F); R = [Object.getPrototypeOf(o) === P, ${tag}, (${state})].join('|')`);
  T(`var o = Reflect.construct(${name}, [${args}], Object); R = [Object.getPrototypeOf(o) === Object.prototype, ${tag}].join('|')`);
  T(`var o = Reflect.construct(${name}, [${args}], ${name}); R = [Object.getPrototypeOf(o) === ${name}.prototype, ${tag}].join('|')`);
  T(`Reflect.construct(${name}, [], 5)`);
  T(`Reflect.construct(${name}, [], () => {})`);
  T(`var nt = new Proxy(function () {}, { get(t, k) { return k === 'prototype' ? Array.prototype : t[k] } }); var o = Reflect.construct(${name}, [${args}], nt); R = Object.getPrototypeOf(o) === Array.prototype`);
  T(`var log = []; var nt = new Proxy(function () {}, { get(t, k) { log.push(String(k)); return t[k] } }); Reflect.construct(${name}, [${args}], nt); R = log.join()`);
  // new.target dentro de função que delega
  T(`function F() { return Reflect.construct(${name}, [${args}], new.target) }; F.prototype = Object.create(${name}.prototype); var o = new F(); R = [Object.getPrototypeOf(o) === F.prototype, o instanceof F].join('|')`);
  // cross-realm
  // ShadowRealm: só primitivos atravessam; função vira wrapper (callable, não construtor)
  T(`var r = new ShadowRealm(); R = r.evaluate('var o = new ' + ${n} + '(${args.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}); [Object.getPrototypeOf(o) === ' + ${n} + '.prototype, Object.prototype.toString.call(o)].join("|")')`);
  T(`var r = new ShadowRealm(); R = r.evaluate('new ' + ${n} + '(${args.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}) instanceof ' + ${n}) + '|' + (r.evaluate(${n}) !== ${name})`);
  T(`var r = new ShadowRealm(); var C = r.evaluate(${n}); R = [typeof C, Object.getPrototypeOf(C) === Function.prototype, C.name, C.length].join('|')`);
  T(`var r = new ShadowRealm(); var C = r.evaluate(${n}); var o = Reflect.construct(${name}, [${args}], C); R = Object.getPrototypeOf(o) === ${name}.prototype`);
  T(`var r = new ShadowRealm(); var C = r.evaluate(${n}); class D extends C {}; R = typeof D`);
  // setPrototypeOf em instância de builtin
  T(`var o = new ${name}(${args}); Object.setPrototypeOf(o, Object.prototype); R = [${tag}, o instanceof ${name}, Object.getPrototypeOf(o) === Object.prototype].join('|')`);
  T(`var o = new ${name}(${args}); Object.setPrototypeOf(o, null); R = [${tag}, o instanceof ${name}, Object.getPrototypeOf(o)].join('|')`);
  T(`class C {}; var o = new ${name}(${args}); Object.setPrototypeOf(o, C.prototype); R = [${tag}, o instanceof C, o instanceof ${name}].join('|')`);
  T(`var o = new ${name}(${args}); Object.setPrototypeOf(o, Object.create(${name}.prototype)); R = [${tag}, o instanceof ${name}].join('|')`);
  // toStringTag
  T(`class C extends ${name} { get [Symbol.toStringTag]() { return 'Tg' } }; var o = new C(${args}); R = ${tag} + '|' + String(o instanceof ${name})`);
  T(`class C extends ${name} {}; C.prototype[Symbol.toStringTag] = 'Own'; var o = new C(${args}); R = ${tag}`);
  T(`var d = Object.getOwnPropertyDescriptor(${name}.prototype, Symbol.toStringTag); R = d ? [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get].join() : 'none'`);
  // length e name dos construtores
  T(`R = [${name}.length, ${name}.name, Object.getOwnPropertyNames(${name}).join()].join('|')`);
  T(`var d = Object.getOwnPropertyDescriptor(${name}, 'prototype'); R = d ? [d.writable, d.enumerable, d.configurable].join() : 'none'`);
  T(`class C extends ${name} {}; R = [Object.getOwnPropertyNames(C).join(), Object.getOwnPropertyNames(C.prototype).join(), C.prototype.constructor === C].join('|')`);
  T(`class C extends ${name} { static s() { return super.name } }; R = [C.s(), C.toString()].join('|')`);
  // instanceof e Symbol.hasInstance
  T(`class C extends ${name} {}; class D extends C {}; var o = new D(${args}); R = [o instanceof D, o instanceof C, o instanceof ${name}, C.prototype.isPrototypeOf(o), ${name}.prototype.isPrototypeOf(o)].join('|')`);
  T(`class C extends ${name} { static [Symbol.hasInstance](v) { return true } }; R = [1 instanceof C, ({}) instanceof C, null instanceof C].join('|')`);
  T(`R = typeof ${name}[Symbol.hasInstance] + ':' + Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance).writable`);
  // extends com valor inválido
  T(`class C extends ${name} {}; class D extends C {}; D.prototype.__proto__ = null; R = String(new D(${args}) instanceof C)`);
  T(`function F() {}; F.prototype = ${name}.prototype; var o = new F(); R = [${tag}, o instanceof ${name}].join('|')`);
  // super em métodos herdados
  T(`class C extends ${name} { toString() { return 'C:' + super.toString() } }; var o = new C(${args}); R = String(o)`);
  T(`class C extends ${name} { get constructor2() { return super.constructor === ${name} } }; R = new C(${args}).constructor2`);
}

// ---- Symbol.species.
const arrOps = ["map(x => x)", "filter(x => true)", "slice(0)", "splice(0, 1)", "concat([4])", "flat()", "flatMap(x => [x])", "toSorted()", "toReversed()", "with(0, 9)", "toSpliced(0, 1)"];
for (const op of arrOps) {
  T(`class C extends Array {}; var o = C.from([1,2,3]).${op}; R = [o instanceof C, Object.getPrototypeOf(o) === C.prototype, Array.isArray(o)].join('|')`);
  T(`class C extends Array { static get [Symbol.species]() { return Array } }; var o = C.from([1,2,3]).${op}; R = [o instanceof C, Object.getPrototypeOf(o) === Array.prototype].join('|')`);
  T(`class C extends Array { static get [Symbol.species]() { return undefined } }; var o = C.from([1,2,3]).${op}; R = [o instanceof C, Object.getPrototypeOf(o) === Array.prototype].join('|')`);
  T(`class C extends Array { static get [Symbol.species]() { return null } }; var o = C.from([1,2,3]).${op}; R = [o instanceof C, Object.getPrototypeOf(o) === Array.prototype].join('|')`);
  T(`class C extends Array { static get [Symbol.species]() { return 5 } }; var o = C.from([1,2,3]).${op}; R = typeof o`);
  T(`var a = [1,2,3]; a.constructor = { [Symbol.species]: function (n) { this.made = n; return this } }; var o = a.${op}; R = [o.made, typeof o].join('|')`);
  T(`var a = [1,2,3]; a.constructor = 1; var o = a.${op}; R = Array.isArray(o)`);
  T(`var a = [1,2,3]; a.constructor = undefined; var o = a.${op}; R = Array.isArray(o)`);
}
T(`class C extends Array {}; var log = []; var o = C.from([1,2]); o.constructor = new Proxy(C, { get(t, k) { log.push(String(k)); return t[k] } }); o.map(x => x); R = log.join()`);
T(`var r = new ShadowRealm(); var A = r.evaluate('Array'); var a = [1,2]; a.constructor = A; var o = a.map(x => x); R = [Object.getPrototypeOf(o) === Array.prototype, typeof o].join('|')`);
T(`var r = new ShadowRealm(); R = r.evaluate('var a = [1,2,3]; var o = a.map(x => x); [Array.isArray(o), o instanceof Array, o.length].join("|")')`);
T(`var r = new ShadowRealm(); var A = r.evaluate('Array'); R = [A === Array, A[Symbol.species] === A, typeof A.from].join('|')`);
T(`R = [Array[Symbol.species] === Array, Map[Symbol.species] === Map, Set[Symbol.species] === Set, Promise[Symbol.species] === Promise, RegExp[Symbol.species] === RegExp, ArrayBuffer[Symbol.species] === ArrayBuffer, Object.getPrototypeOf(Uint8Array)[Symbol.species] === Object.getPrototypeOf(Uint8Array), Uint8Array[Symbol.species] === Uint8Array].join()`);
T(`class C extends Map {}; R = C[Symbol.species] === C`);
T(`class C extends Set {}; R = C[Symbol.species] === C`);
T(`class C extends Promise {}; R = C[Symbol.species] === C`);
T(`class C extends Uint8Array {}; R = C[Symbol.species] === C`);
T(`var d = Object.getOwnPropertyDescriptor(Array, Symbol.species); R = [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name].join('|')`);
// Promise species
for (const m of ["then(x => x)", "catch(x => x)", "finally(() => {})"]) {
  T(`class C extends Promise {}; var p = C.resolve(1).${m}; R = [p instanceof C, Object.getPrototypeOf(p) === C.prototype].join('|')`);
  T(`class C extends Promise { static get [Symbol.species]() { return Promise } }; var p = C.resolve(1).${m}; R = [p instanceof C, Object.getPrototypeOf(p) === Promise.prototype].join('|')`);
  T(`class C extends Promise { static get [Symbol.species]() { return 1 } }; C.resolve(1).${m}; R = 'ok'`);
  T(`class C extends Promise { static get [Symbol.species]() { return function () {} } }; C.resolve(1).${m}; R = 'ok'`);
  T(`var log = []; class C extends Promise { constructor(f) { log.push('ctor'); super(f) } }; C.resolve(1).${m}; R = log.join()`);
}
T(`class C extends Promise {}; R = [C.resolve(1) instanceof C, C.reject(1).catch(() => {}) instanceof C, C.all([]) instanceof C, C.race([]) instanceof C, C.allSettled([]) instanceof C, C.any([]) instanceof C, Promise.resolve(C.resolve(1)) instanceof C].join()`);
T(`class C extends Promise {}; var p = C.resolve(1); R = [C.resolve(p) === p, Promise.resolve(p) === p]`);
T(`Promise.resolve.call(1, 1)`);
T(`Promise.resolve.call(function () {}, 1)`);
T(`Promise.resolve.call(function (ex) { ex(() => {}, () => {}) }, 1); R = 'ok'`);
T(`var r; var C = function (ex) { ex(v => { r = v }, () => {}) }; Promise.resolve.call(C, 5); R = r`);
T(`class C extends Promise { constructor(ex) { super(ex); this.k = 'k' } }; R = C.resolve(1).then(x => x).k`);
T(`class C extends Promise { constructor(ex) { super((a, b) => ex(a, b)); } }; var out = ''; C.resolve(1).then(v => { globalThis.R = 'then ' + v }); R = 'sync'`);
T(`class C extends Promise {}; var o = new C(r => r(1)); R = [Object.prototype.toString.call(o), String(o), o.constructor === C].join('|')`);
T(`new Promise.prototype.constructor()`);
// ArrayBuffer / TypedArray species
T(`class C extends ArrayBuffer {}; var b = new C(8); var s = b.slice(2); R = [s instanceof C, s.byteLength, Object.getPrototypeOf(s) === C.prototype].join('|')`);
T(`class C extends ArrayBuffer { static get [Symbol.species]() { return ArrayBuffer } }; var s = new C(8).slice(2); R = [s instanceof C, s.byteLength].join('|')`);
T(`class C extends ArrayBuffer { static get [Symbol.species]() { return function () { return new ArrayBuffer(1) } } }; R = new C(8).slice(2).byteLength`);
T(`class C extends ArrayBuffer { static get [Symbol.species]() { return function () { return {} } } }; new C(8).slice(2)`);
T(`class C extends ArrayBuffer { static get [Symbol.species]() { return function (n) { return new ArrayBuffer(n + 5) } } }; new C(8).slice(2)`);
T(`var b = new ArrayBuffer(8); b.constructor = { [Symbol.species]: function (n) { return new ArrayBuffer(n) } }; R = b.slice(1, 4).byteLength`);
for (const op of ["map(x => x)", "filter(x => true)", "slice(1)", "subarray(1)"]) {
  T(`class C extends Uint8Array {}; var o = new C([1,2,3]).${op}; R = [o instanceof C, Object.getPrototypeOf(o) === C.prototype, o.length].join('|')`);
  T(`class C extends Uint8Array { static get [Symbol.species]() { return Uint8Array } }; var o = new C([1,2,3]).${op}; R = [o instanceof C, Object.getPrototypeOf(o) === Uint8Array.prototype].join('|')`);
  T(`class C extends Uint8Array { static get [Symbol.species]() { return Uint16Array } }; var o = new C([1,2,3]).${op}; R = [Object.getPrototypeOf(o) === Uint16Array.prototype, o.length].join('|')`);
  T(`class C extends Uint8Array { static get [Symbol.species]() { return Array } }; var o = new C([1,2,3]).${op}; R = Array.isArray(o)`);
  T(`class C extends Uint8Array { static get [Symbol.species]() { return undefined } }; var o = new C([1,2,3]).${op}; R = Object.getPrototypeOf(o) === Uint8Array.prototype`);
}
T(`class C extends Uint8Array { constructor(...a) { super(...a); this.tag = 't' } }; var o = new C([1,2,3]).map(x => x); R = o.tag`);
T(`R = Object.getPrototypeOf(Uint8Array).name + '|' + Object.getPrototypeOf(Uint8Array).length`);
T(`new (Object.getPrototypeOf(Uint8Array))()`);
T(`Object.getPrototypeOf(Uint8Array)()`);
T(`class C extends Object.getPrototypeOf(Uint8Array) {}; new C()`);
// RegExp species e Symbol.*
T(`class C extends RegExp {}; var r = new C('a', 'g'); R = [r.exec('aa').index, r.lastIndex, 'aXa'.replace(r, 'b'), 'a,a'.split(new C(',')).join('|'), String(r), r instanceof C].join('|')`);
T(`class C extends RegExp { exec(s) { R2 = 'exec'; return super.exec(s) } }; var R2; var r = new C('a'); r.test('a'); R = R2`);
T(`class C extends RegExp { static get [Symbol.species]() { return RegExp } }; var parts = 'a,b'.split(new C(',')); R = parts.join('|')`);
T(`class C extends RegExp { constructor(p, f) { super(p, f); globalThis.made = (globalThis.made || 0) + 1 } }; var r = new C(',', 'g'); 'a,b'.split(r); R = globalThis.made`);
T(`class C extends RegExp { constructor(p, f) { super(p, f); globalThis.made2 = (globalThis.made2 || 0) + 1 } }; var r = new C('a', 'g'); Array.from('aa'.matchAll(r)); R = globalThis.made2`);
T(`class C extends RegExp { get flags() { return 'g' } }; R = String(new C('x'))`);
T(`class C extends RegExp { get source() { return 'S' } }; R = String(new C('x'))`);
T(`R = [RegExp(new (class extends RegExp {})('a')).constructor === RegExp, new RegExp(new (class extends RegExp {})('a')).constructor === RegExp].join()`);
T(`class C extends RegExp {}; var r = new C('a'); R = [RegExp(r) === r, new RegExp(r) === r, RegExp.prototype.constructor === RegExp].join()`);
T(`var r = /a/; r.constructor = function () {}; r.constructor[Symbol.species] = RegExp; R = RegExp(r) === r`);
// Error: cause, captureStackTrace
T(`class E extends Error { constructor(m, o) { super(m, o); this.name = 'E' } }; var e = new E('m', { cause: 7 }); R = [e.cause, Object.getOwnPropertyDescriptor(e, 'cause').enumerable, String(e), e.stack.split('\\n')[0]].join('|')`);
T(`class E extends Error {}; var e = new E('m'); R = [e.name, String(e), Object.getOwnPropertyNames(e).sort().join(), e.stack.split('\\n')[0]].join('|')`);
T(`class E extends Error {}; E.prototype.name = 'EE'; var e = new E('m'); R = [String(e), e.stack.split('\\n')[0]].join('|')`);
T(`class E extends Error { get name() { return 'G' } }; var e = new E('m'); R = [String(e), e.stack.split('\\n')[0]].join('|')`);
T(`class E extends Error { constructor(m) { super(m); this.name = 'Late' } }; var e = new E('m'); R = [String(e), e.stack.split('\\n')[0]].join('|')`);
T(`class E extends Error { constructor(m) { super(m); Error.captureStackTrace(this, E) } }; var e = new E('m'); R = [e.stack.split('\\n')[0], typeof e.stack, Object.getOwnPropertyDescriptor(e, 'stack') ? 'own' : 'none'].join('|')`);
T(`class E extends Error { constructor(m) { super(m); this.name = 'N'; Error.captureStackTrace(this, E) } }; R = new E('m').stack.split('\\n')[0]`);
T(`class E extends Error { constructor(m) { super(m); this.name = 'N'; Error.captureStackTrace(this) } }; R = new E('m').stack.split('\\n')[0]`);
T(`var o = { name: 'X', message: 'mm' }; Error.captureStackTrace(o); R = [o.stack.split('\\n')[0], Object.getOwnPropertyDescriptor(o, 'stack') ? 'own' : 'none'].join('|')`);
T(`var o = {}; Error.captureStackTrace(o); R = o.stack.split('\\n')[0]`);
T(`var o = Object.freeze({}); Error.captureStackTrace(o)`);
T(`Error.captureStackTrace(1)`);
T(`Error.captureStackTrace()`);
T(`Error.captureStackTrace(null)`);
T(`var o = new Proxy({}, {}); Error.captureStackTrace(o); R = typeof o.stack`);
T(`var o = {}; Error.captureStackTrace(o, 5); R = typeof o.stack`);
T(`function f() { var o = {}; Error.captureStackTrace(o, f); return o.stack } R = String(f()).split('\\n')[0]`);
T(`R = [typeof Error.captureStackTrace, Error.captureStackTrace.length, Error.captureStackTrace.name, typeof Error.stackTraceLimit, 'captureStackTrace' in TypeError].join()`);
T(`var d = Object.getOwnPropertyDescriptor(new Error('x'), 'stack'); R = d ? [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get].join() : 'none'`);
T(`class E extends Error {}; var e = new E('a', { cause: undefined }); R = ['cause' in e, Object.getOwnPropertyNames(e).join()].join('|')`);
T(`class E extends Error {}; var e = new E('a', {}); R = ['cause' in e, Object.getOwnPropertyNames(e).join()].join('|')`);
T(`class E extends Error {}; var e = new E('a', 5); R = 'cause' in e`);
T(`class E extends Error {}; var e = new E('a', { get cause() { return 'g' } }); R = e.cause`);
T(`class E extends Error {}; var e = new E(undefined, { cause: 1 }); R = [Object.getOwnPropertyNames(e).join(), e.message === ''].join('|')`);
T(`var p = new Proxy({}, { has(t, k) { log.push('has:' + String(k)); return true }, get(t, k) { log.push('get:' + String(k)); return 1 } }); var log = []; new Error('m', p); R = log.join()`);
T(`var e = new Error('m', { cause: new Error('inner') }); R = [e.cause.message, e.cause instanceof Error].join()`);
T(`class E extends Error { constructor() { super('m', { cause: 'x' }); this.cause = 'y' } }; R = new E().cause`);
T(`var e = new AggregateError([1], 'm', { cause: 'c' }); R = [e.cause, e.errors.join(), Object.getOwnPropertyNames(e).sort().join()].join('|')`);
T(`class E extends AggregateError {}; var e = new E([new Error('a')], 'm'); R = [e.errors[0].message, e.name, e instanceof Error, Object.getPrototypeOf(E) === AggregateError].join('|')`);
T(`R = [Object.getPrototypeOf(TypeError) === Error, Object.getPrototypeOf(TypeError.prototype) === Error.prototype, TypeError.prototype.name, TypeError.prototype.hasOwnProperty('message'), Object.getPrototypeOf(AggregateError) === Error].join()`);
T(`R = [Error.length, TypeError.length, AggregateError.length, Error('x') instanceof Error, TypeError('x') instanceof TypeError, Object.getOwnPropertyNames(Error.prototype).sort().join()].join('|')`);
T(`class E extends Error {}; R = Error.prototype.toString.call(new E('m')) + '|' + Error.prototype.toString.call({ name: 'N', message: 'M' }) + '|' + Error.prototype.toString.call({ name: '', message: 'M' }) + '|' + Error.prototype.toString.call({})`);
T(`Error.prototype.toString.call(1)`);
T(`class E extends Error { toString() { return 'custom' } }; R = String(new E('m')) + '|' + new E('m').stack.split('\\n')[0]`);
T(`var e = Reflect.construct(Error, ['m'], function F() {}); R = [Object.getPrototypeOf(e).constructor.name, String(e), e.stack.split('\\n')[0]].join('|')`);
T(`var e = Reflect.construct(TypeError, ['m'], Error); R = [Object.getPrototypeOf(e) === Error.prototype, String(e), Object.prototype.toString.call(e)].join('|')`);
T(`var F = function () {}; F.prototype = { name: 'Fake' }; var e = Reflect.construct(Error, ['m'], F); R = [String(e), e.stack.split('\\n')[0]].join('|')`);

// ---- Date, Function, primitivos embrulhados e coleções.
T(`class D extends Date { constructor() { super(0) } }; var d = new D(); R = [d.getTime(), d.toISOString(), typeof Date.prototype.getTime.call(d), Object.prototype.toString.call(d), JSON.stringify(d)].join('|')`);
T(`class D extends Date { [Symbol.toPrimitive](h) { return h } }; R = [String(new D(0)), new D(0) + '', +new D(0)].join('|')`);
T(`class D extends Date {}; R = [typeof D(), D.UTC(1970, 0), D.now() > 0, Date.prototype.toString.call(new D(NaN))].join('|')`);
T(`Date.prototype.getTime.call(Object.create(Date.prototype))`);
T(`Date.prototype.getTime.call({})`);
T(`class F extends Function {}; var f = new F('a', 'return a * 2'); R = [f(4), f instanceof F, f.name, f.length, typeof f, Object.getPrototypeOf(f) === F.prototype, f.toString().replace(/\\s+/g, ' ')].join('|')`);
T(`class F extends Function { constructor() { super('return this.v'); this.v = 3 } }; var f = new F(); R = [f.v, f.call({ v: 9 }), f()].join('|')`);
T(`class F extends Function { constructor() { super('return 1'); return new Proxy(this, {}) } }; R = new F()()`);
T(`class F extends Function {}; var f = F('return 5'); R = [f(), f instanceof F, Object.getPrototypeOf(f) === F.prototype].join('|')`);
T(`class F extends Function {}; var f = new F(); R = [f(), typeof f, f.name, f.toString().replace(/\\s+/g, ' ')].join('|')`);
T(`class F extends Function {}; var f = new F('return new.target'); R = [f(), typeof new f()].join('|')`);
T(`var f = Reflect.construct(Function, ['return 1'], Array); R = [Object.getPrototypeOf(f) === Array.prototype, typeof f, Object.prototype.toString.call(f)].join('|')`);
T(`var f = Reflect.construct(Function, ['return 1'], Object); R = typeof f`);
T(`class AF extends Object.getPrototypeOf(async function () {}).constructor {}; var f = new AF('return 7'); R = [Object.prototype.toString.call(f), f() instanceof Promise, f instanceof AF].join('|')`);
T(`var GF = Object.getPrototypeOf(function* () {}).constructor; class G extends GF {}; var g = new G('yield 1'); R = [Object.prototype.toString.call(g), [...g()].join(), g instanceof G].join('|')`);
T(`class B extends Boolean {}; var b = new B(0); R = [b.valueOf(), typeof b, !!b, b ? 'truthy' : 'falsy', Boolean.prototype.toString.call(b), Object.prototype.toString.call(b), b + ''].join('|')`);
T(`class N extends Number { constructor(v) { super(v); this.extra = 1 } }; var n = new N(5); R = [n + 1, n.toFixed(1), JSON.stringify(n), n.extra, typeof n, Number.isInteger(n), Object.prototype.toString.call(n), `+"`${n}`"+`].join('|')`);
T(`class S extends String { get length2() { return this.length } }; var s = new S('héllo'); R = [s.length2, s.toUpperCase(), s[1], Object.keys(s).join(), s + '!', JSON.stringify(s), typeof s, s instanceof String, Object.getOwnPropertyNames(s).join()].join('|')`);
T(`class S extends String {}; var s = new S('ab'); s[5] = 'x'; s.length = 10; R = [s.length, s[5], Object.keys(s).join()].join('|')`);
T(`class S extends String {}; R = [S.fromCharCode(65), S.raw({ raw: ['a', 'b'] }, 1), S.name].join('|')`);
T(`class N extends Number {}; R = [N.parseFloat('1.5'), N.isNaN(NaN), N.MAX_SAFE_INTEGER, N.name].join('|')`);
T(`class Sy extends Symbol {}; new Sy('x')`);
T(`class Sy extends Symbol {}; Sy('x')`);
T(`R = typeof Symbol.prototype.constructor + '|' + Symbol.length + '|' + Object.getPrototypeOf(Object(Symbol('s'))) === Symbol.prototype`);
T(`new Symbol()`);
T(`class Sy extends Symbol { constructor() { } }; new Sy()`);
T(`var s = Reflect.construct(Object, [Symbol('q')], Array); R = [typeof s, Object.getPrototypeOf(s) === Symbol.prototype, Object.getPrototypeOf(s) === Array.prototype].join('|')`);
T(`class P extends Proxy {}`);
T(`class P extends Proxy.prototype.constructor {}`);
T(`R = [typeof Proxy, Proxy.length, 'prototype' in Proxy, typeof Proxy.revocable, Proxy.name].join('|')`);
T(`Proxy()`);
T(`class P extends (new Proxy(function () {}, {})) {}; R = typeof new P()`);
T(`var PC = new Proxy(class { constructor() { this.x = 1 } }, {}); class P extends PC {}; var p = new P(); R = [p.x, p instanceof PC, p instanceof P].join('|')`);
T(`var PC = new Proxy(Array, { construct(t, a, nt) { return Reflect.construct(t, a, nt) } }); class P extends PC {}; var p = new P(1, 2); R = [p.length, Array.isArray(p), p instanceof P, p instanceof Array].join('|')`);
T(`var PC = new Proxy(Array, { construct(t, a, nt) { return new t(9, 9, 9) } }); class P extends PC {}; var p = new P(1); R = [p.length, p instanceof P, Object.getPrototypeOf(p) === Array.prototype].join('|')`);
T(`var PC = new Proxy(Array, { construct(t, a, nt) { return 1 } }); class P extends PC {}; new P()`);
T(`var PC = new Proxy(Array, { construct(t, a, nt) { R = String(nt === P); return Reflect.construct(t, a, nt) } }); class P extends PC {}; new P()`);
T(`var r = Proxy.revocable(function () {}, {}); r.revoke(); class P extends r.proxy {}`);
T(`class M extends Map { set(k, v) { return super.set(k, v * 2) } }; var m = new M([[1, 1], [2, 2]]); R = [m.get(1), m.get(2), m.size].join('|')`);
T(`class M extends Map { constructor(it) { super(); this.n = 0 } set(k, v) { this.n++; return super.set(k, v) } }; var m = new M([[1, 1]]); R = [m.n, m.size].join('|')`);
T(`class M extends Map { constructor(it) { super(it); this.n = 0 } set(k, v) { this.n = (this.n || 0) + 1; return super.set(k, v) } }; var m = new M([[1, 1], [2, 2]]); R = [m.n, m.size].join('|')`);
T(`var orig = Map.prototype.set; var n = 0; Map.prototype.set = function (k, v) { n++; return orig.call(this, k, v) }; try { new Map([[1, 2]]); R = n } finally { Map.prototype.set = orig }`);
T(`var orig = Set.prototype.add; var n = 0; Set.prototype.add = function (v) { n++; return orig.call(this, v) }; try { class S extends Set {}; new S([1, 2, 3]); R = n } finally { Set.prototype.add = orig }`);
T(`Map.prototype.set = 5; try { new Map([[1, 2]]) } catch (e) { R = e.name + ': ' + e.message } finally { }`);
T(`class M extends Map { get size() { return 42 } }; var m = new M([[1, 2]]); R = [m.size, Map.prototype.entries.call(m).next().value.join()].join('|')`);
T(`class M extends Map {}; var m = new M([[1, 2]]); R = [Map.prototype.get.call(m, 1), Object.prototype.toString.call(m), [...m].join(), m[Symbol.iterator] === Map.prototype[Symbol.iterator], M.groupBy ? 'g' : 'ng'].join('|')`);
T(`class M extends Map {}; var g = M.groupBy([1, 2, 3], x => x % 2); R = [g instanceof M, g instanceof Map, g.size].join('|')`);
T(`class A extends Array {}; var g = A.from('ab'); var h = A.of(1, 2); R = [g instanceof A, h instanceof A, g.length, h.length, A.from.call(Object, [1]) instanceof Object].join('|')`);
T(`class A extends Array {}; var a = new A(); a[3] = 1; R = [a.length, Object.keys(a).join(), JSON.stringify(a), A.isArray(a), Array.isArray(a)].join('|')`);
T(`class A extends Array { constructor() { super(); this.push(1, 2) } }; var a = new A(); R = [a.length, a.map(x => x * 2).length].join('|')`);
T(`class A extends Array { constructor(n) { super(n) } }; var a = new A(3); R = [a.length, a.map(x => x).length, a.filter(x => true).length].join('|')`);
T(`class A extends Array { constructor(n) { super(); this.length = n } }; var a = new A(2); R = a.concat([1]).length`);
T(`class A extends Array {}; var a = new A(1, 2, 3); a.length = 1; R = [a.length, a.join(), Object.getOwnPropertyDescriptor(a, 'length').writable].join('|')`);
T(`class A extends Array { get [Symbol.isConcatSpreadable]() { return false } }; R = [].concat(new A(1, 2)).length`);
T(`class A extends Array {}; R = [].concat(new A(1, 2)).length + '|' + (new A(1, 2).concat([3]) instanceof A)`);
T(`class A extends Array { static get [Symbol.species]() { return Object } }; var r = new A(1, 2).map(x => x); R = [Array.isArray(r), Object.getPrototypeOf(r) === Object.prototype, Object.keys(r).join()].join('|')`);
T(`var a = new Proxy([], {}); class C extends Array {}; R = [Array.isArray(a), Object.prototype.toString.call(a), a instanceof Array].join('|')`);
T(`class A extends Array {}; var a = new A(); a.length = 4294967295; R = a.length; a.length = -1`);
T(`class S extends Set { add(v) { return super.add(v + 1) } }; var s = new S([1, 2]); R = [...s].join()`);
T(`class S extends Set {}; var s = new S([1, 2]); var u = s.union(new Set([3])); R = [u instanceof S, u instanceof Set, u.size].join('|')`);
T(`class W extends WeakMap {}; var k = {}; var w = new W([[k, 1]]); R = [w.get(k), w instanceof WeakMap, Object.prototype.toString.call(w)].join('|')`);
T(`class AB extends ArrayBuffer {}; var b = new AB(4, { maxByteLength: 8 }); R = [b.resizable, b.maxByteLength, b.byteLength, b instanceof AB, Object.prototype.toString.call(b)].join('|')`);
T(`class AB extends ArrayBuffer {}; R = [AB.isView(new Uint8Array(1)), AB.name, AB.length].join('|')`);
T(`class U extends Uint8Array {}; var u = new U(new ArrayBuffer(4), 1, 2); R = [u.length, u.byteOffset, u.buffer.byteLength, U.BYTES_PER_ELEMENT, u.BYTES_PER_ELEMENT].join('|')`);
T(`class U extends Uint8Array {}; R = [U.from([1, 2]) instanceof U, U.of(1, 2) instanceof U, U.from([1, 2]).join()].join('|')`);
T(`class U extends Uint8Array {}; var u = new U(2); R = [Object.prototype.toString.call(u), u[Symbol.toStringTag], Object.getPrototypeOf(u)[Symbol.toStringTag], Object.getPrototypeOf(Uint8Array.prototype)[Symbol.toStringTag]].join('|')`);
T(`class DV extends DataView {}; var d = new DV(new ArrayBuffer(4)); d.setUint8(0, 5); R = [d.getUint8(0), d instanceof DV, Object.prototype.toString.call(d)].join('|')`);
T(`class O extends Object { constructor() { super(1) } }; var o = new O(); R = [typeof o, o instanceof O, Object.getPrototypeOf(o) === O.prototype].join('|')`);
T(`class O extends Object { constructor() { super(5) } }; R = typeof new O().valueOf()`);
T(`R = [typeof Object(1), Object(1) instanceof Number, typeof new Object('s'), new Object(null) instanceof Object, Reflect.construct(Object, [1], Array) instanceof Array].join()`);
T(`class BI extends BigInt {}`);
T(`new BigInt(1)`);
T(`class BI extends BigInt {}; new BI(1)`);

// ---- `super` em métodos de objeto literal.
T(`var base = { greet() { return 'base' } }; var o = { __proto__: base, greet() { return 'o>' + super.greet() } }; R = o.greet()`);
T(`var base = { get x() { return this.v } }; var o = { __proto__: base, v: 4, get x() { return super.x * 2 } }; R = o.x`);
T(`var base = { set x(v) { this._x = v } }; var o = { __proto__: base, set x(v) { super.x = v + 1 } }; o.x = 1; R = o._x`);
T(`var o = { m() { return super.toString === Object.prototype.toString } }; R = o.m()`);
T(`var o = { m() { return super.x } }; Object.setPrototypeOf(o, { x: 'late' }); R = o.m()`);
T(`var o = { m() { return super.x } }; var p = { m: o.m, x: 'p-x' }; R = p.m()`);
T(`var o = { m() { return super.x }, x: 'self' }; var q = Object.create({ x: 'protoQ' }); q.m = o.m; R = q.m()`);
T(`var o = { m() { super.y = 5; return Object.keys(this).join() + ':' + Object.getPrototypeOf(this) === Object.prototype } }; R = String(o.m())`);
T(`var o = { m() { super.y = 5 } }; var t = { __proto__: null }; o.m.call(t); R = [Object.keys(t).join(), t.y]`);
T(`var o = { m() { return () => super.toString === Object.prototype.toString } }; R = o.m()()`);
T(`var o = { m() { return delete super.x } }; o.m()`);
T(`var o = { m() { return super[Symbol.toStringTag] } }; R = String(o.m())`);
T(`var o = { async m() { return super.toString === Object.prototype.toString } }; o.m().then(v => { globalThis.R = 'async ' + v }); R = 'sync'`);
T(`var o = { *m() { yield super.toString === Object.prototype.toString } }; R = o.m().next().value`);
T(`var o = { m: function () { return 1 } }; R = (0, eval)('({ m() { return super.x } })').m()`);
T(`var base = { f() { return 'B' + this.tag } }; var o = { __proto__: base, tag: 'o', f() { return super.f.call({ tag: 'z' }) } }; R = o.f()`);
T(`var base = [1, 2]; var o = { __proto__: base, f() { return super.length + ':' + super.map === Array.prototype.map } }; R = String(o.f())`);
T(`class A { static s() { return 'A.s' } m() { return 'A.m' } }; var o = { __proto__: A.prototype, m() { return 'o>' + super.m() } }; R = o.m()`);
T(`var o = { __proto__: new Map([[1, 2]]), size2() { return super.size } }; o.size2()`);
T(`var o = { __proto__: new Date(0), m() { return super.getTime() } }; o.m()`);
T(`var o = { __proto__: [], m() { return super.length } }; R = o.m()`);
T(`var m = new Map([[1, 2]]); var o = { m() { return super.get } }; Object.setPrototypeOf(o, m); R = typeof o.m()`);
T(`var o = { __proto__: Array.prototype, length: 2, 0: 'a', 1: 'b' }; R = [o.join('-'), Array.isArray(o), o instanceof Array, JSON.stringify(o), Object.prototype.toString.call(o)].join('|')`);
T(`var o = { __proto__: Error.prototype, message: 'm' }; R = [String(o), o instanceof Error, Object.prototype.toString.call(o)].join('|')`);
T(`var o = { __proto__: Date.prototype }; o.getTime()`);
T(`var o = { __proto__: Map.prototype }; o.get(1)`);
T(`var o = { __proto__: Promise.prototype }; o.then(() => {})`);
T(`var o = { __proto__: RegExp.prototype }; o.test('a')`);
T(`var o = { __proto__: RegExp.prototype }; R = [String(o), o.flags, o.source].join('|')`);
T(`var o = { __proto__: Number.prototype }; o.valueOf()`);
T(`var o = { __proto__: String.prototype }; o.toString()`);
T(`var o = { __proto__: Boolean.prototype }; o.valueOf()`);
T(`var o = { __proto__: Function.prototype }; R = [typeof o, Object.prototype.toString.call(o), o instanceof Function].join('|'); o()`);
T(`var o = { __proto__: ArrayBuffer.prototype }; o.byteLength`);
T(`var o = { __proto__: Uint8Array.prototype }; o.length`);
T(`var o = { __proto__: Symbol.prototype }; o.toString()`);
T(`var o = Object.create(Symbol.prototype); R = o.description`);
T(`var o = { __proto__: Set.prototype }; o.size`);
T(`var o = Object.create(WeakMap.prototype); o.has({})`);
T(`R = Object.getOwnPropertyNames(Object.getOwnPropertyDescriptor(Map.prototype, 'size')).join()`);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "subclass-golden-"));
const source_file = path.join(dir, "subclass_source.js");
const file = path.join(dir, "subclass_case.js");
fs.writeFileSync(
  file,
  `globalThis.vm = require("node:vm");\ntry { vm.runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
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
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: "America/Sao_Paulo" } });
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
