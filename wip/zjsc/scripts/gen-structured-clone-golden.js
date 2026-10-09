// Gera tests/golden/structured_clone_bun.tsv: `reportError` e `structuredClone` do global medidos no bun 1.4.2
// (descritor, `length`, `name`, `toString()`, ordem de chaves, identidade em ciclos, protótipo perdido, getters, não
// enumeráveis, função e Symbol, argumentos, opções, e as chaves próprias do `DataCloneError` lançado).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda no bun por indirect
// eval no mesmo processo. Entram `name`, `message`, `code` e `instanceof Error`; o `DataCloneError` lançado por nativo
// também entra com `Object.keys`, `line`, `column`, `typeof stack` e a primeira linha da `stack` (`sourceURL` e a cauda
// da `stack` dependem do hospedeiro e saem, como em scripts/gen-dom-exception-golden.js).
// Uso: bun scripts/gen-structured-clone-golden.js > tests/golden/structured_clone_bun.tsv
const { emitRow } = require("./golden-prelude.js");

// `reportError` entrega ao `uncaughtException` do processo; sem handler o bun imprime o erro e sai com código 1 no
// fim. O handler mudo mantém a saída do gerador limpa (o porte não tem `process`; o relatório vai ao vazio).
process.on("uncaughtException", () => {});

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'bigint') return v + 'n'; if (Object.is(v, -0)) return '-0'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const body = (code) => programs.push(HELPER + code);
const expr = (code) => body(`try { R = S(${code}) } catch (e) { R = E(e) }`);

{
  // Descritores e forma.
  for (const n of ["reportError", "structuredClone"]) {
    expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${n}'))`);
    expr(`${n}.length`);
    expr(`${n}.name`);
    expr(`${n}.toString()`);
    expr(`Object.getOwnPropertyNames(${n})`);
    expr(`'prototype' in ${n}`);
    expr(`Object.getPrototypeOf(${n}) === Function.prototype`);
    expr(`new ${n}(1)`);
  }
  // Ordem entre os globais do host.
  expr("(function(k){ return [k.indexOf('queueMicrotask') < k.indexOf('reportError'), k.indexOf('reportError') < k.indexOf('setImmediate'), k.indexOf('setTimeout') < k.indexOf('structuredClone'), k.indexOf('reportError') - k.indexOf('queueMicrotask'), k.indexOf('structuredClone') - k.indexOf('setTimeout')] })(Object.getOwnPropertyNames(globalThis))");

  // reportError: devolve undefined, não lança, o código seguinte roda.
  expr("reportError(new Error('x'))");
  expr("reportError()");
  expr("reportError(1)");
  expr("reportError('texto')");
  expr("(function(){ reportError(undefined); return 'seguiu' })()");

  // structuredClone: primitivos.
  for (const c of ["1", "-0", "NaN", "Infinity", "'s'", "''", "true", "false", "null", "undefined", "1n", "-5n", "0.1", "'\\u00e9\\ud83d\\ude00'"]) expr(`structuredClone(${c})`);
  expr("Object.is(structuredClone(-0), -0)");
  // Objeto e Array.
  expr("(function(){ var o = {a: 1, b: 'x', c: [1, 2, {d: null}], e: undefined}; var c = structuredClone(o); return [c !== o, c.c !== o.c, JSON.stringify(c), 'e' in c] })()");
  expr("(function(){ var o = {a: 1}; o.self = o; var c = structuredClone(o); return [c !== o, c.self === c, c.self !== o] })()");
  expr("(function(){ var a = [1]; a.push(a); var c = structuredClone(a); return [c !== a, c[1] === c, c.length] })()");
  expr("(function(){ var s = {}; var c = structuredClone({a: s, b: s, c: [s]}); return [c.a === c.b, c.a !== s, c.c[0] === c.a] })()");
  expr("(function(){ var a = {}, b = {a: a}; a.b = b; var c = structuredClone(a); return [c.b.a === c, c !== a] })()");
  // Protótipo perdido, instâncias, herdadas.
  expr("(function(){ class A { constructor() { this.x = 1 } get y() { return 2 } m() {} } var c = structuredClone(new A); return [Object.getPrototypeOf(c) === Object.prototype, JSON.stringify(c), c instanceof A, 'y' in c] })()");
  expr("(function(){ var c = structuredClone(Object.create({inh: 1})); return [JSON.stringify(c), 'inh' in c, Object.getPrototypeOf(c) === Object.prototype] })()");
  expr("(function(){ var c = structuredClone(Object.create(null)); return [Object.getPrototypeOf(c) === Object.prototype, Object.getOwnPropertyNames(c).length] })()");
  expr("(function(){ var c = structuredClone([1, 2]); return [Array.isArray(c), Object.getPrototypeOf(c) === Array.prototype] })()");
  // Getters executados, não enumeráveis e símbolos perdidos, ordem de chaves.
  expr("(function(){ var log = []; var c = structuredClone({get z() { log.push('g'); return 5 }, w: 1}); var d = Object.getOwnPropertyDescriptor(c, 'z'); return [log.length, JSON.stringify(c), 'value' in d, d.writable, d.enumerable, d.configurable] })()");
  expr("(function(){ var o = {}; Object.defineProperty(o, 'h', {value: 1, enumerable: false}); o.v = 2; return Object.getOwnPropertyNames(structuredClone(o)) })()");
  expr("(function(){ var o = {k: 1}; o[Symbol('s')] = 2; return Reflect.ownKeys(structuredClone(o)).map(String) })()");
  expr("Object.keys(structuredClone({b: 1, 2: 'x', a: 2, 1: 'y'}))");
  expr("(function(){ var l = []; structuredClone({get a() { l.push('a'); return 1 }, get b() { l.push('b'); return 2 }}); return l })()");
  expr("structuredClone({get a() { throw new RangeError('boom') }})");
  expr("(function(){ var o = {}; Object.defineProperty(o, 'x', {get() { return 7 }, enumerable: true, configurable: true}); return structuredClone(o).x })()");
  // Arrays: buracos, propriedades extras, length.
  expr("(function(){ var a = [1, , 3]; a.p = 9; var c = structuredClone(a); return [c.length, 1 in c, c.p, Array.isArray(c)] })()");
  expr("(function(){ var a = []; a.length = 3; var c = structuredClone(a); return [c.length, 0 in c] })()");
  expr("structuredClone([undefined, null, , 'x'])");
  expr("structuredClone({0: 'a', 1: 'b', length: 2})");
  // Função e Symbol lançam.
  for (const c of ["() => 1", "function () {}", "class {}", "Symbol('q')", "Symbol.iterator", "{f() {}}", "{a: {b: () => 1}}", "[Symbol()]", "{s: Symbol()}", "Math.max", "async function () {}", "function* () {}"]) expr(`structuredClone(${c})`);
  // Argumentos e opções.
  expr("structuredClone()");
  expr("structuredClone(1, 2)");
  expr("structuredClone(1, 'x')");
  expr("structuredClone(1, true)");
  expr("[structuredClone(1, null), structuredClone(1, undefined), structuredClone(1, {}), structuredClone(1, {transfer: undefined}), structuredClone(1, [])]");
  expr("structuredClone(1, 2, 3)");
  expr("structuredClone.call(null, 5)");
  expr("structuredClone.call(undefined, {a: 1}).a");
  expr("structuredClone.apply(null, [])");
  expr("structuredClone(1, {get transfer() { throw new RangeError('t') }})");
  expr("structuredClone(1, function () {})");
  // Exceção do getter não envolta, e propriedades de Proxy.
  expr("structuredClone(new Proxy({}, {}))");
  expr("structuredClone(new WeakMap)");
  expr("structuredClone(new WeakSet)");
  expr("structuredClone(new Promise(() => {}))");
  expr("structuredClone(Promise.resolve())");
  expr("structuredClone((function* () {})())");
  expr("structuredClone(Math)");
  expr("structuredClone(JSON)");
  expr("structuredClone(globalThis)");
  expr("structuredClone(Object(Symbol()))");
  expr("structuredClone(new WeakRef({}))");
  expr("structuredClone([].values())");
  // Exóticos: protótipos intrínsecos, arguments, Proxy, global, módulos, instâncias de classe, Intl, Temporal.
  expr("(function(c){ return [Array.isArray(c), Object.getPrototypeOf(c) === Object.prototype, Object.getPrototypeOf(c) === Array.prototype, Object.getOwnPropertyNames(c)] })(structuredClone(Array.prototype))");
  expr("(function(c){ return [c !== Object.prototype, Object.getOwnPropertyNames(c).length, Object.getPrototypeOf(c) === Object.prototype] })(structuredClone(Object.prototype))");
  expr("(function(){ return structuredClone(arguments) })(1, 2)");
  expr("(function(){ 'use strict'; return structuredClone(arguments) })(1, 2)");
  expr("structuredClone(new Proxy({a: 1}, {}))");
  expr("structuredClone(new Proxy([1], {}))");
  expr("structuredClone(new Proxy(function () {}, {}))");
  expr("structuredClone(Reflect)");
  expr("structuredClone(Atomics)");
  expr("structuredClone(Intl)");
  expr("structuredClone(Symbol.prototype)");
  expr("structuredClone(Error.prototype)");
  expr("structuredClone(Function.prototype)");
  expr("structuredClone(Date.prototype)");
  expr("structuredClone(RegExp.prototype)");
  expr("structuredClone(Map.prototype)");
  expr("structuredClone(Set.prototype)");
  expr("structuredClone(ArrayBuffer.prototype)");
  expr("structuredClone(Uint8Array.prototype)");
  expr("structuredClone(BigInt.prototype)");
  expr("structuredClone(Promise.prototype)");
  expr("structuredClone(Number.prototype)");
  expr("structuredClone(String.prototype)");
  expr("structuredClone(Boolean.prototype)");
  expr("structuredClone(function () {}.bind(null))");
  expr("structuredClone([][Symbol.iterator]())");
  expr("structuredClone(new Map().entries())");
  expr("structuredClone(new Intl.Locale('en'))");
  expr("structuredClone(new Intl.DateTimeFormat())");
  expr("structuredClone(Temporal.PlainDate.from('2020-01-01'))");
  expr("structuredClone(new EventTarget)");
  expr("structuredClone([Object(Symbol())])");
  expr("(function(){ class A { #p = 1; x = 2; static s = 3; get q() { return 1 } } var c = structuredClone(new A); return [JSON.stringify(c), Object.getPrototypeOf(c) === Object.prototype] })()");
  expr("(function(){ var n = new Number(1); n.x = 5; var c = structuredClone(n); return [c instanceof Number, c.x, c.valueOf()] })()");
  expr("(function(){ var n = new String('ab'); n.x = 5; var c = structuredClone(n); return [c.x, c.valueOf(), Object.getOwnPropertyNames(c).join()] })()");
  expr("(function(){ var o = Object.create(null); o[2] = 1; o[1] = 2; o.b = 3; o[0] = 4; var c = structuredClone(o); return [Object.getPrototypeOf(c) === Object.prototype, Object.keys(c).join(), JSON.stringify(c)] })()");
  expr("(function(){ var o = Object.create(Array.prototype); o.a = 1; return structuredClone(o) })()");
  expr("(function(){ var o = JSON.parse('{\"__proto__\":{\"a\":1}}'); var c = structuredClone(o); return [Object.getPrototypeOf(c) === Object.prototype, Object.getOwnPropertyNames(c).join()] })()");
  expr("(function(){ var o = {}; Object.defineProperty(o, '__proto__', {value: 1, enumerable: true}); var c = structuredClone(o); return [Object.getPrototypeOf(c) === Object.prototype, Object.getOwnPropertyNames(c).join()] })()");
  expr("(function(){ var o = {get a() { o.b = 2; return 1 }}; return structuredClone(o) })()");
  expr("(function(){ var o = {get a() { delete o.b; return 1 }, b: 2}; return structuredClone(o) })()");
  // Arrays exóticos: buracos e extras, length não gravável, esparso, não enumerável, getter que muda o array.
  expr("(function(){ var a = [1, , 3]; a.x = 1; var c = structuredClone(a); return [c.length, 1 in c, c.x, Object.keys(c).join()] })()");
  expr("(function(){ var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); var c = structuredClone(a); return [c.length, Object.getOwnPropertyDescriptor(c, 'length').writable] })()");
  expr("(function(){ var a = [1, 2, 3]; a.length = 5; Object.defineProperty(a, 'length', {writable: false}); var c = structuredClone(a); return [c.length, Object.getOwnPropertyDescriptor(c, 'length').writable, 3 in c] })()");
  expr("(function(){ var a = []; a[1e6] = 1; var c = structuredClone(a); return [c.length, c[1e6], Object.keys(c).join()] })()");
  expr("(function(){ var a = []; a[4294967294] = 1; var c = structuredClone(a); return [c.length, c[4294967294]] })()");
  expr("(function(){ class A extends Array {} var c = structuredClone(A.from([1, 2])); return [Array.isArray(c), Object.getPrototypeOf(c) === Array.prototype, c.length] })()");
  expr("(function(){ var a = [1]; a.length = 3; var c = structuredClone(a); return [c.length, Object.keys(c).join()] })()");
  expr("(function(){ var a = []; Object.defineProperty(a, 0, {get() { return 'g' }, enumerable: true}); var c = structuredClone(a); return [c[0], Object.getOwnPropertyDescriptor(c, 0).writable] })()");
  expr("(function(){ var a = [1, 2]; Object.defineProperty(a, 0, {enumerable: false}); var c = structuredClone(a); return [c.length, 0 in c, 1 in c, Object.getOwnPropertyDescriptor(c, 0).enumerable] })()");
  expr("(function(){ var a = [1, 2]; Object.defineProperty(a, 1, {enumerable: false}); var c = structuredClone(a); return [c.length, 1 in c] })()");
  expr("(function(){ var a = [1, 2]; Object.defineProperty(a, 'n', {value: 1, enumerable: false}); a.v = 2; return Object.getOwnPropertyNames(structuredClone(a)).join() })()");
  expr("(function(){ var a = Object.freeze([1, 2]); var c = structuredClone(a); return [Object.isFrozen(c), c.length] })()");
  expr("(function(){ var a = Object.freeze({a: 1}); return [Object.isFrozen(structuredClone(a))] })()");
  expr("(function(){ var a = [1, 2, 3]; Object.defineProperty(a, 0, {get() { a.length = 1; return 'x' }, enumerable: true, configurable: true}); var c = structuredClone(a); return [c.length, JSON.stringify(c)] })()");
  expr("(function(){ var a = [1, 2]; Object.defineProperty(a, 0, {get() { a.push(9); return 'x' }, enumerable: true, configurable: true}); var c = structuredClone(a); return [c.length, JSON.stringify(c)] })()");
  expr("(function(){ var a = [1, , 3]; Object.defineProperty(a, 0, {get() { a[1] = 'novo'; return 'x' }, enumerable: true, configurable: true}); return structuredClone(a) })()");
  expr("(function(){ var a = [1]; Object.setPrototypeOf(a, null); var c = structuredClone(a); return [Array.isArray(c), Object.getPrototypeOf(c) === Array.prototype] })()");
  expr("structuredClone({e: new Error('a')}).e.message");
  expr("(function(){ var c = structuredClone(new DOMException('m', 'AbortError')); return [c.name, c.message, c.code, c instanceof DOMException] })()");
  // Blob e File clonam (bytes, type, nome, lastModified); propriedade extra se perde; objetos do bun sem tag lançam.
  expr("(function(){ var b = new Blob(['abc'], {type: 'text/x'}); b.x = 1; var c = structuredClone(b); return [c instanceof Blob, c !== b, c.size, c.type, c.x, c instanceof File] })()");
  expr("(function(){ var f = new File(['abc'], 'n.txt', {type: 'text/x', lastModified: 5}); var c = structuredClone(f); return [c instanceof File, c !== f, c.name, c.size, c.lastModified, c.type] })()");
  expr("(function(){ var b = new Blob([]); var c = structuredClone({a: b, b: b}); return [c.a === c.b, c.a !== b, c.a.lastModified] })()");
  for (const c of ["new Headers({a: '1'})", "new URL('http://a/b')", "new URLSearchParams('a=1')", "new FormData()", "new AbortController()", "new AbortController().signal", "new Response('a')", "new Request('http://a/')", "new Event('x')", "new EventTarget()", "new TextEncoder()", "new WeakRef({})", "new WeakMap()", "Promise.resolve(1)", "new Proxy({}, {})"]) expr(`structuredClone(${c})`);
  expr("(function(){ var e = new Error('m', {cause: 1}); var c = structuredClone(e); return ['cause' in c, Object.getOwnPropertyNames(c).join()] })()");
  expr("(function(){ class E2 extends TypeError {} var e = new E2('m'); e.name = 'Zz'; var c = structuredClone(e); return [c.name, Object.getPrototypeOf(c) === Error.prototype] })()");
  expr("(function(){ var c = structuredClone(new AggregateError([1], 'm')); return [c.name, c.errors, c.message] })()");
  expr("(function(){ var b = new ArrayBuffer(1); structuredClone(b, {transfer: [b]}); return structuredClone(b) })()");
  expr("(function(){ var a = [1, , 3]; a.x = 5; a[10] = 1; var c = structuredClone(a); return [c.length, 1 in c, c.x, Object.keys(c).join()] })()");
  expr("(function(){ var c = structuredClone(Buffer.from('ab')); return [c instanceof Buffer, c instanceof Uint8Array, c.length] })()");
}
{
  const moved = expr;
  moved("(function(){ var d = new Date(5); var c = structuredClone(d); return [c !== d, c.getTime(), structuredClone(new Date(NaN)).getTime(), c instanceof Date] })()");
  moved("(function(){ var d = new Date(0); d.x = 1; return structuredClone(d).x })()");
  moved("(function(){ var r = /a+/gi; r.lastIndex = 3; var c = structuredClone(r); return [c.source, c.flags, c.lastIndex, c !== r] })()");
  moved("(function(){ var r = /x/; r.x = 1; return structuredClone(r).x })()");
  moved("(function(){ var m = new Map([[1, {a: 1}], ['k', 2]]); m.set(m, m); var c = structuredClone(m); return [c.size, c.get(c) === c, c !== m, c instanceof Map] })()");
  moved("(function(){ var m = new Map; m.x = 1; return structuredClone(m).x })()");
  moved("(function(){ var s = new Set([1, 2]); s.add(s); var c = structuredClone(s); return [c.size, c.has(c), c instanceof Set] })()");
  moved("(function(){ var ab = new ArrayBuffer(4); new Uint8Array(ab).set([1, 2, 3, 4]); var c = structuredClone(ab); return [c !== ab, Array.from(new Uint8Array(c)), c.byteLength, ab.byteLength] })()");
  moved("(function(){ var ab = new ArrayBuffer(4); var u = new Uint16Array(ab, 2, 1); var c = structuredClone(u); return [c.constructor.name, c.byteOffset, c.length, c.buffer.byteLength, c.buffer !== ab] })()");
  moved("(function(){ var ab = new ArrayBuffer(4); var c = structuredClone([new Uint8Array(ab, 0, 2), new Uint8Array(ab, 2, 2)]); return c[0].buffer === c[1].buffer })()");
  moved("(function(){ var ab = new ArrayBuffer(4); return [structuredClone(new DataView(ab, 1, 2)).byteLength, Array.from(structuredClone(new Float64Array([1.5, NaN])))] })()");
  moved("(function(){ var c = structuredClone(new Float16Array([1])); return c.constructor.name })()");
  moved("(function(){ var r = new ArrayBuffer(4, {maxByteLength: 8}); var c = structuredClone(r); return [c.resizable, c.maxByteLength] })()");
  moved("(function(){ var s = new SharedArrayBuffer(4); var c = structuredClone(s); return [c === s, c instanceof SharedArrayBuffer] })()");
  moved("(function(){ var e = new RangeError('boom', {cause: 'c'}); e.extra = 1; var c = structuredClone(e); return [c.constructor.name, c.name, c.message, c.cause, c.extra, Object.getOwnPropertyNames(c).join(), c.stack === e.stack, Object.getPrototypeOf(c) === RangeError.prototype] })()");
  moved("(function(){ class MyE extends Error {} var c = structuredClone(new MyE('m')); return [c.constructor.name, c.name, c.message] })()");
  moved("(function(){ var e = new Error('x'); e.name = 'Custom'; return structuredClone(e).name })()");
  moved("(function(){ var e = new Error('x'); e.name = 'TypeError'; return structuredClone(e).constructor.name })()");
  moved("(function(){ var e = new TypeError('m'); e.message = 'changed'; return structuredClone(e).message })()");
  moved("Object.getOwnPropertyNames(structuredClone(new AggregateError([1], 'q'))).join()");
  moved("(function(){ var a = structuredClone(new AggregateError([1], 'q')); return [a.constructor.name, a.name, a.errors] })()");
  moved("(function(){ var e = new Error('m'); Object.defineProperty(e, 'message', {get() { return 'g' }}); return structuredClone(e).message })()");
  moved("(function(){ var e = new Error('m'); e.message = 5; return structuredClone(e).message })()");
  moved("(function(){ var e = new Error('m'); e.stack = 5; var c = structuredClone(e); return [c.stack, Object.getOwnPropertyNames(c).join()] })()");
  moved("(function(){ var e = new Error('m'); e.stack = 'custom'; return structuredClone(e).stack })()");
  moved("(function(){ var e = new Error('q'); e.name = 'EvalError'; return structuredClone(e).constructor.name })()");
  moved("(function(){ var e = new Error('q'); Object.setPrototypeOf(e, null); return structuredClone(e).name })()");
  moved("(function(){ var m = new Map([[{a: 1}, {b: 2}]]); var c = structuredClone(m); var k = [...m.keys()][0], ck = [...c.keys()][0]; return [ck !== k, ck.a, c.get(ck).b] })()");
  moved("(function(){ var x = {}, m = new Map([[1, x]]); var c = structuredClone([m, m, x, new Set([x])]); return [c[0] === c[1], c[0].get(1) === c[2], [...c[3]][0] === c[2]] })()");
  moved("(function(){ var r = /a/g; var c = structuredClone(r); return [Object.getOwnPropertyNames(c).join(), Object.getPrototypeOf(c) === RegExp.prototype] })()");
  moved("(function(){ var b = structuredClone(Object(1n)); return [typeof b, b instanceof BigInt, b.valueOf() + ''] })()");
  moved("[structuredClone(new Number(3)) instanceof Number, structuredClone(new String('x')) instanceof String, structuredClone(new Boolean(false)).valueOf(), structuredClone(Object(1n)).valueOf() + '']");
  moved("(function(){ var b = new ArrayBuffer(8); var c = structuredClone(b, {transfer: [b]}); return [b.byteLength, b.detached, c.byteLength] })()");
  moved("(function(){ var b = new ArrayBuffer(8); var c = structuredClone({b: b}, {transfer: [b]}); return [b.detached, c.b.byteLength] })()");
  moved("(function(){ var b = new ArrayBuffer(8); var u = new Uint8Array(b); var c = structuredClone(u, {transfer: [b]}); return [u.length, c.length, b.detached] })()");
  moved("(function(){ var b = new ArrayBuffer(8); structuredClone(b, {transfer: [b]}); return structuredClone(b, {transfer: [b]}) })()");
  moved("structuredClone(1, {transfer: [1]})");
  moved("structuredClone(1, {transfer: 5})");
  moved("structuredClone(1, {transfer: [{}]})");
  moved("structuredClone({a: 1}, {transfer: []})");
  moved("(function(){ var b = new ArrayBuffer(2); return structuredClone(b, {transfer: [b, b]}) })()");

  // O `DataCloneError` lançado por nativo ganha `line`, `column`, `sourceURL` (fora da comparação) e `stack` próprios.
  const THROWN =
    "var T = function (e) { var f = function (n) { return n !== 'sourceURL' }; var d = function (n) { var x = Object.getOwnPropertyDescriptor(e, n); return x && [typeof x.value, x.writable, x.enumerable, x.configurable] }; " +
    "return [e.name, e.message, e.code, Object.getOwnPropertyNames(e).filter(f), Object.keys(e).filter(f), e.line, e.column, typeof e.stack, String(e.stack).split('\\n')[0], " +
    "d('line'), d('column'), d('stack'), e instanceof DOMException, e instanceof Error] };\n";
  const thrown = (call) => body(`${THROWN}try { ${call}; R = 'sem erro' } catch (e) { R = S(T(e)) }`);
  thrown("structuredClone(function () {})");
  thrown("structuredClone(Symbol())");
  thrown("structuredClone({ f() {} })");
  thrown("structuredClone({ a: [1, { b: () => 1 }] })");
  thrown("structuredClone(new WeakMap)");
  thrown("structuredClone(new Promise(() => {}))");
  thrown("\n\n   structuredClone(Symbol())");
  thrown("(function f() {\n    structuredClone(Symbol())\n  })()");
  thrown("[Symbol()].map(structuredClone)");
  thrown("Reflect.apply(structuredClone, undefined, [Symbol()])");
  thrown("structuredClone.call(null, Symbol())");
  thrown("(0, structuredClone)(Symbol())");
  thrown("new Function('structuredClone(Symbol())')()");
  thrown("eval('structuredClone(Symbol())')");
  thrown("(function () { 'use strict'; structuredClone(Symbol()) })()");
  thrown("var b = new ArrayBuffer(2); structuredClone(b, {transfer: [b, b]})");
  thrown("var b = new ArrayBuffer(8); structuredClone(b, {transfer: [b]}); structuredClone(b, {transfer: [b]})");
  thrown("structuredClone(1, {transfer: [{}]})");
  // `TypeError` com `code` (protótipo por código, `code` herdado): ver src/runtime/node_error.rs.
  thrown("structuredClone(1, {transfer: 5})");
  thrown("structuredClone(1, {transfer: Symbol()})");
  body("var p; try { structuredClone(1, {transfer: 5}) } catch (e) { p = Object.getPrototypeOf(e) } R = S([p === TypeError.prototype, Object.getPrototypeOf(p) === TypeError.prototype, Object.getOwnPropertyNames(p), p.code, p.name, Object.keys(p)])");
  thrown("structuredClone(new Error('m', {cause: Symbol()}), {transfer: [new ArrayBuffer(1), {}]})");
  body(THROWN + "var a, b; try { structuredClone(Symbol()) } catch (e) { a = e } try { structuredClone(Symbol()) } catch (e) { b = e } R = S([a !== b, a.stack === b.stack, a.line === b.line, a.column === b.column])");
  body(THROWN + "try { structuredClone(Symbol()) } catch (e) { e.stack = 'novo'; e.line = 99; R = S([e.stack, e.line, Object.keys(e).filter(function (n) { return n !== 'sourceURL' })]) }");

  // `MessagePort` na lista `transfer`: no bun a porta não se move, o resultado é a própria porta (mesma identidade, ainda
  // ligada ao par); fora da lista, fechada ou repetida é `DataCloneError`. Só casos síncronos (a entrega é de
  // scripts/gen-message-channel-golden.js).
  expr("(function(){ var c = new MessageChannel(); var r = structuredClone(c.port1, {transfer: [c.port1]}); return [r === c.port1, r instanceof MessagePort, c.port1.hasRef(), r.hasRef()] })()");
  expr("(function(){ var c = new MessageChannel(); var r = structuredClone({a: [c.port1], b: c.port1}, {transfer: [c.port1]}); return [r.a[0] === c.port1, r.a[0] === r.b, r.a[0] instanceof MessagePort, r.a !== undefined] })()");
  expr("(function(){ var c = new MessageChannel(); var r = structuredClone({x: 1}, {transfer: [c.port1]}); return [JSON.stringify(r), c.port1.hasRef()] })()");
  expr("(function(){ var c = new MessageChannel(); var r = structuredClone(new Map([[1, c.port1]]), {transfer: [c.port1]}); return [r.get(1) === c.port1] })()");
  expr("(function(){ var c = new MessageChannel(); var ab = new ArrayBuffer(8); var r = structuredClone({p: c.port1, ab: ab}, {transfer: [c.port1, ab]}); return [r.p === c.port1, r.ab === ab, r.ab.byteLength, ab.byteLength] })()");
  expr("(function(){ var c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port1]}); return structuredClone(c.port1, {transfer: [c.port1]}) === c.port1 })()");
  expr("(function(){ var c = new MessageChannel(); var r = structuredClone(c.port1, {transfer: [c.port1]}); var r2 = structuredClone(r, {transfer: [r]}); return [r2 === r, r2 === c.port1] })()");
  expr("(function(){ var c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port1]}); c.port1.ref(); var h = c.port1.hasRef(); c.port1.close(); return [h] })()");
  thrown("var c = new MessageChannel(); structuredClone({p: c.port1})");
  thrown("var c = new MessageChannel(); structuredClone(c.port1)");
  thrown("var c = new MessageChannel(); structuredClone({p: c.port1}, {transfer: [new ArrayBuffer(1)]})");
  thrown("var c = new MessageChannel(); structuredClone({p: c.port2}, {transfer: [c.port1]})");
  thrown("var c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port2]})");
  thrown("var c = new MessageChannel(); c.port1.close(); structuredClone(c.port1, {transfer: [c.port1]})");
  thrown("var c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port1, c.port1]})");
  thrown("var c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port1, 1]})");
  thrown("var c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port1, {}]})");
  thrown("var c = new MessageChannel(), d = new MessageChannel(); d.port1.postMessage(1, [c.port1]); structuredClone(c.port1, {transfer: [c.port1]})");
}

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
