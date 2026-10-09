// Gera tests/golden/text_encoder_bun.tsv: `TextEncoder` do global medido no bun 1.4.2 (descritor, `length`, `name`,
// `toString()`, chaves e atributos do construtor e do protótipo, getter `encoding`, chamada sem `new`, `this`
// inválido, `encode` com unidade substituta solta, `encodeInto` com destino pequeno ou inválido, `{ read, written }`).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda no bun por
// indirect eval no mesmo processo (sem estado). Fora do golden de propósito: a posição da chave no global e o
// valor de `originalLine`/`originalColumn`/`sourceURL` dos erros com `code` (ver src/runtime/node_error.rs).
// Uso: bun scripts/gen-text-encoder-golden.js > tests/golden/text_encoder_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.constructor.name + '|' + e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n" +
  "var D = function (d) { return d === undefined ? 'undefined' : [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get, typeof d.set] };\n" +
  "var e = new TextEncoder(), p = TextEncoder.prototype;\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const bytes = (code) => expr(`Array.from(${code})`);

// Descritor e forma do construtor.
expr("D(Object.getOwnPropertyDescriptor(globalThis, 'TextEncoder'))");
expr("TextEncoder.length");
expr("TextEncoder.name");
expr("TextEncoder.toString()");
expr("Object.getOwnPropertyNames(TextEncoder)");
expr("Reflect.ownKeys(TextEncoder).length");
expr("Object.getPrototypeOf(TextEncoder) === Function.prototype");
expr("D(Object.getOwnPropertyDescriptor(TextEncoder, 'prototype'))");
expr("TextEncoder.prototype === p");
expr("typeof TextEncoder");

// Protótipo.
expr("Reflect.ownKeys(p).map(String)");
expr("Object.getPrototypeOf(p) === Object.prototype");
expr("Object.prototype.toString.call(p)");
expr("Object.prototype.toString.call(e)");
expr("String(e)");
expr("p.constructor === TextEncoder");
for (const k of ["constructor", "encoding", "encode", "encodeInto", "Symbol.toStringTag"]) {
  const key = k.startsWith("Symbol.") ? k : `'${k}'`;
  expr(`D(Object.getOwnPropertyDescriptor(p, ${key}))`);
}
expr("p[Symbol.toStringTag]");
expr("Object.keys(p)");
for (const m of ["encode", "encodeInto"]) {
  expr(`p.${m}.length`);
  expr(`p.${m}.name`);
  expr(`p.${m}.toString()`);
  expr(`Object.getOwnPropertyNames(p.${m})`);
  expr(`'prototype' in p.${m}`);
}
expr("(function(g){ return [g.name, g.length, g.toString()] })(Object.getOwnPropertyDescriptor(p, 'encoding').get)");

// Construção.
expr("Reflect.ownKeys(e)");
expr("e instanceof TextEncoder");
expr("Object.getPrototypeOf(e) === p");
expr("JSON.stringify(e)");
expr("e.encoding");
expr("new TextEncoder('x', 1).encoding");
expr("new TextEncoder({ toString() { throw new RangeError('x') } }).encoding");
expr("new TextEncoder(5).encoding");
expr("TextEncoder()");
expr("TextEncoder.call({})");
expr("TextEncoder.call(e)");
expr("TextEncoder.apply(null, [])");
expr("(function(){ class X extends TextEncoder {} var x = new X(); return [x.encoding, Object.getPrototypeOf(x) === X.prototype, x instanceof TextEncoder, Array.from(x.encode('a'))] })()");
expr("(function(){ var x = Reflect.construct(TextEncoder, [], Object); return [Object.getPrototypeOf(x) === Object.prototype, x.encoding, typeof x.encode] })()");
expr("Reflect.construct(TextEncoder, [], Object).constructor === Object");
expr("(function(){ function F() {} F.prototype = p; var x = new F(); return x.encode('a') })()");
expr("TextEncoder.prototype.encode('a')");

// encoding.
expr("e.encoding");
expr("(function(){ e.encoding = 'x'; return e.encoding })()");
expr("(function(){ 'use strict'; e.encoding = 'x'; return e.encoding })()");
expr("Object.getOwnPropertyDescriptor(p, 'encoding').get.call({})");
expr("Object.getOwnPropertyDescriptor(p, 'encoding').get.call(p)");
expr("Object.getOwnPropertyDescriptor(p, 'encoding').get.call(null)");
expr("Object.getOwnPropertyDescriptor(p, 'encoding').get.call(e)");
expr("p.encoding");

// this inválido.
for (const m of ["encode", "encodeInto"]) {
  for (const t of ["{}", "null", "undefined", "p", "1", "'s'", "TextEncoder", "[]", "Object.create(e)"]) {
    expr(`p.${m}.call(${t}, 'a', new Uint8Array(4))`);
  }
}
expr("p.encodeInto.call({})");
expr("p.encode.call({}, Symbol())");

// encode.
const encodeInputs = [
  "", "hello", "h\\u00e9\\u20ac\\ud83d\\ude00", "a\\ud800b", "\\udc00", "\\ud83d", "\\ud83dx", "x\\ud83d", "\\ude00\\ud83d",
  "\\ud83d\\ud83d\\ude00", "\\u0000", "\\u007f", "\\u0080", "\\u07ff", "\\u0800", "\\uffff", "\\ufeff", "\\ud7ff", "\\ue000",
  "\\udbff\\udfff", "\\ud800\\udc00", "a\\u0000b", "\\ud800\\ud800", "\\udc00\\udc00", "\\udc00\\ud800", "\\n\\r\\t",
];
for (const s of encodeInputs) bytes(`e.encode('${s}')`);
for (const c of ["", "undefined", "null", "12", "-0", "1n", "true", "{}", "[]", "[1,2]", "new String('ab')", "{ toString() { return 'zz' } }",
  "{ valueOf() { return 'vv' } }", "{ [Symbol.toPrimitive]() { return 'pp' } }", "'a', 'b'", "NaN", "1e21", "new Uint8Array([97])", "function(){}"]) {
  bytes(`e.encode(${c})`);
}
expr("e.encode(Symbol())");
expr("e.encode({ toString() { throw new RangeError('x') } })");
expr("e.encode({ toString() { return {} }, valueOf() { return {} } })");
expr("e.encode.length");
expr("(function(r){ return [r.constructor.name, r.length, r.byteOffset, r.byteLength, r.buffer.byteLength, r.buffer.constructor.name, Object.prototype.toString.call(r), Object.isFrozen(r)] })(e.encode('hi'))");
expr("(function(r){ return [r.length, r.byteOffset, r.buffer.byteLength] })(e.encode(''))");
expr("(function(r){ return [r.length, r.byteOffset, r.buffer.byteLength] })(e.encode('h\\u00e9llo'))");
expr("e.encode('a') === e.encode('a')");
expr("e.encode('a').buffer === e.encode('a').buffer");
expr("(function(){ var a = e.encode('abc'); a[0] = 120; return [Array.from(a), Array.from(e.encode('abc'))] })()");
expr("e.encode('abc'.repeat(100000)).length");
expr("e.encode('\\u20ac'.repeat(1000)).length");
expr("e.encode('\\ud83d\\ude00'.repeat(1000)).length");
expr("e.encode('\\ud800'.repeat(10)).length");
expr("(function(){ var s = ''; for (var i = 0; i < 0x800; i++) s += String.fromCharCode(i); return e.encode(s).length })()");
expr("(function(){ var s = ''; for (var i = 0xd7f0; i < 0xe010; i++) s += String.fromCharCode(i); var r = e.encode(s); var n = 0; for (var i = 0; i < r.length; i++) n = (n * 31 + r[i]) >>> 0; return [r.length, n] })()");

// encodeInto.
const ei = (source, size) => expr(`(function(){ var u = new Uint8Array(${size}); var r = e.encodeInto('${source}', u); return [r.read, r.written, Array.from(u), Object.keys(r), Object.getPrototypeOf(r) === Object.prototype] })()`);
const intoCases = [
  ["hello", 10], ["hello", 3], ["hello", 0], ["hello", 5], ["hello", 4], ["h\\u00e9llo", 2], ["h\\u00e9llo", 3], ["h\\u00e9llo", 7],
  ["\\u20ac", 2], ["\\u20ac", 3], ["\\u20ac", 4], ["\\ud83d\\ude00", 3], ["\\ud83d\\ude00", 4], ["\\ud83d\\ude00", 0], ["a\\ud800b", 5], ["a\\ud800b", 3],
  ["a\\ud800b", 4], ["\\ud83d", 5], ["\\ud83d", 3], ["\\ud83d", 2], ["\\udc00", 3], ["", 4], ["", 0], ["a\\ud83d\\ude00b", 5],
  ["a\\ud83d\\ude00b", 6], ["a\\ud83d\\ude00b", 4], ["a\\ud83d\\ude00b", 1], ["\\ud83d\\ud83d\\ude00", 7], ["\\ud83d\\ud83d\\ude00", 6],
  ["\\u00e9\\u00e9\\u00e9", 5], ["\\u00e9\\u00e9\\u00e9", 6], ["\\u07ff\\u0800", 4], ["\\u07ff\\u0800", 5], ["x\\u0000y", 3],
];
for (const [s, n] of intoCases) ei(s, n);
expr("(function(){ var u = new Uint8Array(8); var r = e.encodeInto('ab', u.subarray(2, 4)); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var u = new Uint8Array(8); var r = e.encodeInto('\\u00e9', u.subarray(1, 3)); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var u = new Uint8Array(4); var r = e.encodeInto('abc', u); return Object.getOwnPropertyDescriptors(r) })()");
expr("(function(){ var u = new Uint16Array(4); var r = e.encodeInto('abc', u); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var u = new Uint16Array(2); var r = e.encodeInto('abcdef', u); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var u = new Uint8ClampedArray(4); var r = e.encodeInto('abc', u); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var u = new Int8Array(3); var r = e.encodeInto('\\u00e9a', u); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var u = new Float64Array(1); var r = e.encodeInto('abcdefghi', u); return [r.read, r.written, Array.from(new Uint8Array(u.buffer))] })()");
expr("(function(){ var d = new DataView(new ArrayBuffer(4), 1, 2); var r = e.encodeInto('abc', d); return [r.read, r.written, Array.from(new Uint8Array(d.buffer))] })()");
expr("(function(){ var u = new Uint8Array(new SharedArrayBuffer(4)); var r = e.encodeInto('abc', u); return [r.read, r.written, Array.from(u)] })()");
expr("(function(){ var ab = new ArrayBuffer(8); var u = new Uint8Array(ab); ab.transfer(); var r = e.encodeInto('abc', u); return [r.read, r.written, u.length] })()");
expr("e.encodeInto.length");
for (const args of ["", "'a'", "'a', null", "'a', undefined", "'a', {}", "'a', []", "'a', 5", "'a', 'b'", "'a', new ArrayBuffer(4)", "'a', new SharedArrayBuffer(4)",
  "'a', [1, 2]", "'a', { length: 4 }", "'a', function(){}", "undefined", "undefined, new Uint8Array(2)", "null, new Uint8Array(2)", "5, new Uint8Array(2)",
  "{}, new Uint8Array(2)", "[1,2], new Uint8Array(4)", "Symbol(), new Uint8Array(2)", "Symbol(), 5", "Symbol()", "1n, new Uint8Array(4)",
  "{ toString() { throw new RangeError('s') } }, 5", "{ toString() { throw new RangeError('s') } }, new Uint8Array(2)",
  "'a', new Uint8Array(2), 'extra'", "new String('ab'), new Uint8Array(4)"]) {
  expr(`e.encodeInto(${args})`);
}
// ordem: a fonte é convertida antes de conferir o destino.
expr("(function(){ var log = []; try { e.encodeInto({ toString() { log.push('s'); return 'a' } }, 5) } catch (x) { log.push(x.message) } return log })()");
// Erros com `code` (src/runtime/node_error.rs): protótipo próprio por código com `name`, `code` e `toString`;
// o `code` não é própria (salvo `ERR_MISSING_ARGS`); `originalLine`...`stack` próprios e não enumeráveis.
// `originalLine` e `originalColumn` ficam só como tipo (o bun os mede no fonte transpilado); `sourceURL` fica fora.
const NODE_ERROR =
  "var N = function (e) { var p = Object.getPrototypeOf(e), f = function (n) { return n !== 'sourceURL' }, d = function (o, n) { var x = Object.getOwnPropertyDescriptor(o, n); return x && [typeof x.value, x.writable, x.enumerable, x.configurable] }; " +
  "return S([e.constructor.name, String(e), e.name, e.code, e.message, Object.getOwnPropertyNames(e).filter(f), Object.keys(e), Object.prototype.hasOwnProperty.call(e, 'code'), 'code' in e, " +
  "p === TypeError.prototype, Object.getOwnPropertyNames(p), d(p, 'name'), d(p, 'code'), d(p, 'toString'), p.toString.length, p.toString.name, Object.getPrototypeOf(p) === TypeError.prototype, e instanceof TypeError, e instanceof Error, " +
  "Object.prototype.toString.call(e), typeof e.originalLine, typeof e.originalColumn, e.line, e.column, d(e, 'line'), d(e, 'stack'), d(e, 'message'), String(e.stack).split('\\n')[0]]) };\n";
const nodeError = (call) => programs.push(HELPER + NODE_ERROR + `try { ${call}; R = 'sem erro' } catch (e) { R = N(e) }`);
nodeError("TextEncoder()");
nodeError("\n\n   TextEncoder()");
nodeError("(function f() {\n    TextEncoder()\n  })()");
nodeError("p.encode.call({})");
nodeError("p.encodeInto.call(null, 'a', new Uint8Array(1))");
nodeError("e.encodeInto()");
nodeError("e.encodeInto('a')");
nodeError("structuredClone(1, { transfer: 5 })");
nodeError("atob()");
// TypeError nativo SEM `code` (`throw_native_type_error`): protótipo do próprio TypeError, mas com a pilha e a
// posição do frame nativo, como os coded.
nodeError("Object.getOwnPropertyDescriptor(p, 'encoding').get.call({})");
nodeError("\n  Object.getOwnPropertyDescriptor(p, 'encoding').get.call(1)");
nodeError("e.encodeInto('a', 1)");
nodeError("(function g() {\n   e.encodeInto('a')  \n })()");
nodeError("btoa()");
nodeError("structuredClone()");
nodeError("structuredClone(1, 2)");
nodeError("structuredClone(1, { transfer: [1] })");
nodeError("DOMException()");
nodeError("Object.getOwnPropertyDescriptor(DOMException.prototype, 'name').get.call({})");
nodeError("Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call(null)");
nodeError("Object.getOwnPropertyDescriptor(DOMException.prototype, 'message').get.call(undefined)");
nodeError("(function () { 'use strict'; TextEncoder() })()");
nodeError("new Function('TextEncoder()')()");
nodeError("eval('TextEncoder()')");
nodeError("Reflect.apply(TextEncoder, undefined, [])");
nodeError("[0].map(TextEncoder)");
// o protótipo é o mesmo entre erros do mesmo código, e diferente entre códigos
expr("(function(){ var a, b, c; try { TextEncoder() } catch (x) { a = x } try { TextEncoder() } catch (x) { b = x } try { p.encode.call({}) } catch (x) { c = x } return [a !== b, Object.getPrototypeOf(a) === Object.getPrototypeOf(b), Object.getPrototypeOf(a) === Object.getPrototypeOf(c), a.stack === b.stack, a.line === b.line] })()");
// o `toString` do protótipo não trata mensagem vazia nem `this` alheio
expr("(function(){ try { TextEncoder() } catch (x) { var t = Object.getPrototypeOf(x).toString; x.message = ''; return [String(x), t.call({ name: 'N', code: 'C', message: 'M' }), t.call({}), t.call(1)] } })()");
expr("(function(){ try { TextEncoder() } catch (x) { x.name = 'Foo'; delete x.message; return String(x) } })()");
// Descritores completos do construtor e do protótipo (todas as chaves próprias, na ordem do bun), em JSON.
programs.push(HELPER +
  "var F = function (f) { return typeof f === 'function' ? [f.name, f.length, f.toString()] : f === undefined ? 'undefined' : typeof f };" +
  " var V = function (v) { return typeof v === 'function' ? F(v) : typeof v === 'object' && v !== null ? 'object' : typeof v === 'symbol' ? String(v) : v };" +
  " var X = function (o) { return Reflect.ownKeys(o).map(function (k) { var d = Object.getOwnPropertyDescriptor(o, k); return [String(k), 'value' in d ? V(d.value) : 'accessor', d.writable, d.enumerable, d.configurable, F(d.get), F(d.set)] }) };\n" +
  "try { R = JSON.stringify([X(TextEncoder), X(TextEncoder.prototype)]) } catch (e) { R = E(e) }");
// round trip
expr("(function(){ var s = 'h\\u00e9\\u20ac\\ud83d\\ude00z'; var u = new Uint8Array(16); var r = e.encodeInto(s, u); return [r.read === s.length, Array.from(u.subarray(0, r.written)).join() === Array.from(e.encode(s)).join()] })()");

// Auditoria de `encodeInto` com unidade substituta solta depois de um par: o que não cabe (U+FFFD de três bytes) fica
// de fora e `read` só conta o que foi escrito.
for (const n of [4, 5, 7, 8]) {
  expr(`(function(){ var x = new Uint8Array(${n}); var r = e.encodeInto('\\ud83d\\ude00\\ud83d', x); return [r.read, r.written, Array.from(x)] })()`);
  expr(`(function(){ var x = new Uint8Array(${n}); var r = e.encodeInto('a\\ud83d\\ude00\\ud800', x); return [r.read, r.written, Array.from(x)] })()`);
}

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
