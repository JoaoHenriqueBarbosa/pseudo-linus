// Gera tests/golden/text_decoder_bun.tsv: `TextDecoder` do global medido no bun 1.4.2 (descritor, forma do construtor e do
// protótipo, rótulos normalizados, opções, erros com `code`, `decode` com `stream` em sequência partida, BOM com e sem
// `ignoreBOM`, `fatal`, UTF-8, UTF-16LE/BE e windows-1252, `this` inválido, entradas de tipos variados).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda no bun por
// indirect eval no mesmo processo (sem estado). Fora do golden de propósito: a posição da chave no global, se `code`
// é própria nos erros `ERR_ILLEGAL_CONSTRUCTOR`/`ERR_INVALID_THIS`, e as codificações que o porte não tem (ver
// src/runtime/text_decoder.rs).
// Uso: bun scripts/gen-text-decoder-golden.js > tests/golden/text_decoder_bun.tsv
const { emitRow } = require("./golden-prelude.js");
const { LABELS } = require("./text-decoder-labels.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.constructor.name + '|' + e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n" +
  "var D = function (d) { return d === undefined ? 'undefined' : [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get, typeof d.set] };\n" +
  "var u = function () { return new Uint8Array(Array.prototype.slice.call(arguments)) };\n" +
  "var cp = function (s) { return Array.from(s, function (c) { return c.codePointAt(0) }) };\n" +
  "var d = new TextDecoder(), p = TextDecoder.prototype;\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
// Corpo de função que devolve o resultado, rodado numa IIFE.
const fn = (body) => expr(`(function(){ ${body} })()`);

// Descritor e forma do construtor.
expr("D(Object.getOwnPropertyDescriptor(globalThis, 'TextDecoder'))");
expr("TextDecoder.length");
expr("TextDecoder.name");
expr("TextDecoder.toString()");
expr("Object.getOwnPropertyNames(TextDecoder)");
expr("Reflect.ownKeys(TextDecoder).length");
expr("Object.getPrototypeOf(TextDecoder) === Function.prototype");
expr("D(Object.getOwnPropertyDescriptor(TextDecoder, 'prototype'))");
expr("typeof TextDecoder");

// Protótipo.
expr("Reflect.ownKeys(p).map(String)");
expr("Object.getPrototypeOf(p) === Object.prototype");
expr("Object.prototype.toString.call(p)");
expr("Object.prototype.toString.call(d)");
expr("String(d)");
expr("Object.keys(p)");
expr("p.constructor === TextDecoder");
for (const k of ["constructor", "encoding", "fatal", "ignoreBOM", "decode", "Symbol.toStringTag"]) {
  const key = k.startsWith("Symbol.") ? k : `'${k}'`;
  expr(`D(Object.getOwnPropertyDescriptor(p, ${key}))`);
}
expr("p[Symbol.toStringTag]");
expr("[p.decode.length, p.decode.name, p.decode.toString(), 'prototype' in p.decode, Object.getOwnPropertyNames(p.decode)]");
for (const k of ["encoding", "fatal", "ignoreBOM"]) {
  expr(`(function(g){ return [g.name, g.length, g.toString()] })(Object.getOwnPropertyDescriptor(p, '${k}').get)`);
}

// Construção.
expr("Reflect.ownKeys(d)");
expr("d instanceof TextDecoder");
expr("Object.getPrototypeOf(d) === p");
expr("JSON.stringify(d)");
expr("[d.encoding, d.fatal, d.ignoreBOM]");
expr("TextDecoder()");
expr("TextDecoder.call({})");
expr("TextDecoder.call(d)");
expr("(function(){ class X extends TextDecoder {} var x = new X('latin1'); return [x.encoding, Object.getPrototypeOf(x) === X.prototype, x instanceof TextDecoder, x.decode(u(0xe9))] })()");
expr("(function(){ var x = Reflect.construct(TextDecoder, [], Object); return [Object.getPrototypeOf(x) === Object.prototype, typeof x.decode] })()");
expr("(function(){ function F() {} F.prototype = p; var x = new F(); return x.decode() })()");
expr("TextDecoder.prototype.decode()");

// Rótulos.
for (const label of ["utf-8", "UTF8", " utf8 ", "unicode-1-1-utf-8", "unicode11utf8", "unicode20utf8", "x-unicode20utf8", "\\tutf-8\\n", "\\rutf8\\f", "utf-16le", "utf-16",
  "ucs-2", "unicode", "csunicode", "iso-10646-ucs-2", "unicodefeff", "UTF-16LE", "utf-16be", "unicodefffe", "latin1", "Latin1", "ascii", "us-ascii", "iso-8859-1",
  "ISO-8859-1", "windows-1252", "l1", "cp1252", "cp819", "ibm819", "csisolatin1", "iso-ir-100", "iso8859-1", "iso88591", "iso_8859-1", "iso_8859-1:1987",
  "x-cp1252", "ansi_x3.4-1968", "replacement", "hz-gb-2312", "", "x", "\\u000butf-8", "\\u00a0utf-8", "utf-8\\u0000", "\\uff35\\uff34\\uff26-8", "utf-8\\u212a",
  "utf-32", "utf-7", "null", "5"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
// Todos os rótulos da tabela do WHATWG (o `replacement` e os rótulos dele o bun recusa), crus e em maiúsculas com
// espaço ASCII nas pontas.
for (const [label] of LABELS) {
  expr(`new TextDecoder("${label}").encoding`);
  expr(`new TextDecoder("\\t${label.toUpperCase()}\\n").encoding`);
}
expr("new TextDecoder().encoding");
expr("new TextDecoder(undefined).encoding");
expr("new TextDecoder(null).encoding");
expr("new TextDecoder(5).encoding");
expr("new TextDecoder({ toString() { return 'latin1' } }).encoding");
expr("new TextDecoder(Symbol())");
expr("new TextDecoder({ toString() { throw new RangeError('boom') } }, 5)");
expr("new TextDecoder('x', 5)");

// Opções.
expr("(function(x){ return [x.fatal, x.ignoreBOM] })(new TextDecoder('utf-8', { fatal: 1, ignoreBOM: 'x' }))");
expr("(function(x){ return [x.fatal, x.ignoreBOM] })(new TextDecoder('utf-8', { fatal: 0, ignoreBOM: '' }))");
expr("(function(x){ return [x.fatal, x.ignoreBOM] })(new TextDecoder('utf-8', { fatal: {} }))");
expr("new TextDecoder('utf-8', null).fatal");
expr("new TextDecoder('utf-8', function () {}).fatal");
expr("new TextDecoder('utf-8', []).fatal");
for (const options of ["5", "'x'", "true", "Symbol()", "1n"]) expr(`new TextDecoder('utf-8', ${options})`);
expr("(function(){ var l = []; new TextDecoder({ toString() { l.push('label'); return 'utf-8' } }, { get fatal() { l.push('fatal'); return 1 }, get ignoreBOM() { l.push('bom'); return 1 } }); return l })()");
expr("new TextDecoder('x', { get fatal() { throw new Error('f') } })");
expr("new TextDecoder('utf-8', { get fatal() { throw new RangeError('g') } })");

// Getters e `this`.
for (const k of ["encoding", "fatal", "ignoreBOM"]) {
  for (const thisValue of ["{}", "null", "5", "p", "[]"]) expr(`Object.getOwnPropertyDescriptor(p, '${k}').get.call(${thisValue})`);
}
expr("(function(){ d.encoding = 'x'; d.fatal = true; return [d.encoding, d.fatal] })()");
expr("(function(){ 'use strict'; d.encoding = 'x' })()");
for (const thisValue of ["{}", "null", "undefined", "5", "'s'", "Symbol()", "1n", "[]", "function foo() {}", "(function(){})", "p", "new TextEncoder()", "Object.create(d)"]) {
  expr(`p.decode.call(${thisValue})`);
}
expr("p.decode.call({}, 5)");
expr("p.decode.call({}, u(97), 5)");

// Entradas de `decode`.
expr("d.decode()");
expr("d.decode(undefined)");
for (const input of ["null", "5", "'abc'", "{}", "[97]", "true", "Symbol()", "{ length: 1 }", "function(){}"]) expr(`d.decode(${input})`);
expr("d.decode(u(97, 98).buffer)");
expr("d.decode(new SharedArrayBuffer(2))");
expr("d.decode(new DataView(u(97, 98, 99).buffer, 1))");
expr("d.decode(new DataView(u(97, 98, 99).buffer, 1, 1))");
expr("d.decode(new Uint16Array([0x6261, 0x63]))");
expr("d.decode(u(97, 98, 99, 100).subarray(1, 3))");
expr("d.decode(new Float32Array(1))");
expr("d.decode(new Uint8ClampedArray([104, 105]))");
expr("d.decode(Buffer.from('h\\u00e9llo'))");
expr("d.decode(new Uint8Array(70000).fill(97)).length");
expr("d.decode(new Uint8Array(0)).length");
expr("(function(){ var b = new ArrayBuffer(4); new Uint8Array(b).set([97, 98, 99, 100]); return [d.decode(b), d.decode(new Uint8Array(b, 1, 2))] })()");
expr("(function(){ var b = new ArrayBuffer(2, { maxByteLength: 8 }); new Uint8Array(b).set([97, 98]); var v = new Uint8Array(b); b.resize(4); return d.decode(v) })()");
expr("(function(){ var b = new ArrayBuffer(4, { maxByteLength: 8 }); var v = new Uint8Array(b, 2, 2); b.resize(3); return d.decode(v) })()");
expr("(function(){ var b = new ArrayBuffer(4); structuredClone(b, { transfer: [b] }); return d.decode(b) })()");
expr("(function(){ var v = new Uint8Array(4); structuredClone(v.buffer, { transfer: [v.buffer] }); return d.decode(v) })()");
expr("(function(){ var b = new ArrayBuffer(4); var v = new DataView(b); structuredClone(b, { transfer: [b] }); return d.decode(v) })()");

// Opções de `decode`: ordem das conferências.
expr("d.decode(u(97), null)");
expr("d.decode(u(97), function(){})");
expr("d.decode(u(97), [])");
for (const options of ["5", "'x'", "true", "Symbol()"]) expr(`d.decode(u(97), ${options})`);
expr("d.decode(5, 5)");
expr("(function(){ var l = []; try { d.decode(5, { get stream() { l.push('s'); return 1 } }) } catch (e) { l.push(e.message) } return l })()");
expr("d.decode({}, { get stream() { throw new RangeError('g') } })");
expr("(function(){ var l = []; d.decode(u(97), { get stream() { l.push('s'); return 1 } }); return l })()");

// UTF-8: BOM e dados inválidos.
expr("cp(d.decode(u(0xef, 0xbb, 0xbf, 97)))");
expr("d.decode(u(0xef, 0xbb, 0xbf)).length");
expr("cp(new TextDecoder('utf-8', { ignoreBOM: true }).decode(u(0xef, 0xbb, 0xbf, 97)))");
expr("cp(d.decode(u(97, 0xef, 0xbb, 0xbf, 98)))");
expr("cp(d.decode(u(0xef, 0xbb, 0xbf, 0xef, 0xbb, 0xbf, 97)))");
expr("cp(d.decode(u(0xff, 97, 0xc3, 0x28, 0xe2, 0x82, 0xed, 0xa0, 0x80, 0xf0, 0x9f, 0x98)))");
expr("cp(d.decode(u(0xf0, 0x9f, 0x98, 97)))");
expr("cp(d.decode(u(0xc0, 0x80, 0xe0, 0x80, 0x80, 0xf4, 0x90, 0x80, 0x80)))");
expr("cp(d.decode(u(0xed, 0xa0, 0x80)))");
expr("cp(d.decode(u(0xf0, 0x9f, 0x98, 0x80)))");
expr("d.decode(u(0xe2, 0x82, 0xac, 0x68, 0x69))");
const f = "var f = new TextDecoder('utf-8', { fatal: true });\n";
const fatal = (body) => fn(f + body);
fatal("return f.decode(u(0xe2, 0x82, 0xac))");
fatal("return f.decode(u(0xff))");
fatal("return f.decode(u(0xe2, 0x82))");
fatal("return f.decode(u(0x80))");
fatal("try { f.decode(u(0xff)) } catch (e) {} return f.decode(u(97))");
fatal("return f.decode(u(0x80), { stream: true })");
fatal("var x = f; x.decode(u(0xe2), { stream: true }); return x.decode()");
fatal("f.decode(u(0xe2), { stream: true }); return f.decode(u(97), { stream: true })");
fatal("f.decode(u(0xe2), { stream: true }); try { f.decode(u(97), { stream: true }) } catch (e) {} return f.decode(u(98))");
fatal("try { f.decode(u(97), { stream: true }) } catch (e) {} try { f.decode(u(0xff), { stream: true }) } catch (e) {} return cp(f.decode(u(0xef, 0xbb, 0xbf, 98)))");

// Stream.
const stream = (setup, body) => fn(`var s = new TextDecoder(${setup});\n${body}`);
stream("", "return [s.decode(u(0xe2), { stream: true }), s.decode(u(0x82), { stream: true }), s.decode(u(0xac), { stream: true }), s.decode()]");
stream("", "return [s.decode(u(0xe2, 0x82), { stream: true }), s.decode()]");
stream("", "return [s.decode(u(0xe2, 0x82), { stream: true }), s.decode(u(97))]");
stream("", "return [s.decode(u(0xe2, 0x82), { stream: true }), s.decode(u(0x41), { stream: true })]");
stream("", "return [s.decode(u(0xf0, 0x9f), { stream: true }), s.decode(u(0x98), { stream: true }), s.decode(u(0x80))]");
stream("", "return [s.decode(u(0xf0, 0x9f, 0x98), { stream: true }), s.decode()]");
stream("", "return [s.decode(u(97, 0xff), { stream: true }), s.decode()]");
stream("", "return [s.decode(u(0xe0), { stream: true }), s.decode(u(0x80), { stream: true }), s.decode()]");
stream("", "return [s.decode(u(0xe2), { stream: true }), s.decode(new Uint8Array(0), { stream: true }), s.decode(u(0x82, 0xac))]");
stream("", "return [s.decode(u(0xe2), { stream: 'yes' }), s.decode(u(0x82, 0xac))]");
stream("", "return [s.decode(u(0xe2), { stream: 0 }), s.decode(u(0x82, 0xac))]");
stream("", "s.decode(u(0xe2), { stream: true }); return s.decode()");
stream("", "s.decode(u(0xe2), { stream: true }); return s.decode(undefined, { stream: true })");
stream("", "s.decode(u(0xe2), { stream: true }); s.decode(undefined, { stream: true }); return s.decode(u(0x82, 0xac))");
stream("", "return [cp(s.decode(u(0xef), { stream: true })), cp(s.decode(u(0xbb, 0xbf, 97), { stream: true })), cp(s.decode(u(0xef, 0xbb, 0xbf, 98)))]");
stream("", "return [cp(s.decode(u(97), { stream: true })), cp(s.decode(u(0xef, 0xbb, 0xbf, 98)))]");
stream("", "return [cp(s.decode(u(0xef, 0xbb, 0xbf, 97))), cp(s.decode(u(0xef, 0xbb, 0xbf, 98)))]");
stream("", "s.decode(u(97), { stream: true }); s.decode(); return cp(s.decode(u(0xef, 0xbb, 0xbf, 98)))");
stream("", "s.decode(u(), { stream: true }); return cp(s.decode(u(0xef, 0xbb, 0xbf, 98), { stream: true }))");
stream("", "return [cp(s.decode(u(0xef, 0xbb, 0xbf), { stream: true })), cp(s.decode(u(0xef, 0xbb, 0xbf, 98)))]");
stream("", "return [cp(s.decode(u(0xef, 0xbb), { stream: true })), cp(s.decode(u(0x61)))]");
stream("", "return [cp(s.decode(u(0xef), { stream: true })), cp(s.decode(u(0x61)))]");

// UTF-16LE e UTF-16BE.
const w = (body) => stream("'utf-16le'", body);
w("return s.decode(u(0x61, 0, 0x62, 0))");
w("return cp(s.decode(u(0x61, 0, 0x62)))");
w("return cp(s.decode(u(0xff, 0xfe, 0x61, 0)))");
w("return cp(s.decode(u(0xff, 0xfe)))");
w("return cp(new TextDecoder('utf-16le', { ignoreBOM: true }).decode(u(0xff, 0xfe, 0x61, 0)))");
w("return cp(s.decode(u(0xfe, 0xff, 0x61, 0)))");
w("return cp(s.decode(u(0x00, 0xd8, 0x61, 0)))");
w("return cp(s.decode(u(0x00, 0xdc, 0x61, 0)))");
w("return cp(s.decode(u(0x3d, 0xd8, 0x00, 0xde)))");
w("return cp(s.decode(u(0x3d, 0xd8)))");
w("return [cp(s.decode(u(0x61), { stream: true })), cp(s.decode(u(0, 0x3d), { stream: true })), cp(s.decode(u(0xd8, 0, 0xde)))]");
w("return [cp(s.decode(u(0x3d, 0xd8), { stream: true })), cp(s.decode(u(0x00, 0xde), { stream: true })), cp(s.decode())]");
w("return [cp(s.decode(u(0x3d, 0xd8), { stream: true })), cp(s.decode())]");
w("return [cp(s.decode(u(0x61), { stream: true })), cp(s.decode())]");
w("return [cp(s.decode(u(0x3d, 0xd8), { stream: true })), cp(s.decode(u(0x61, 0), { stream: true }))]");
w("return [cp(s.decode(u(0xff), { stream: true })), cp(s.decode(u(0xfe, 0x61, 0), { stream: true }))]");
const wf = (body) => fn(`var s = new TextDecoder('utf-16le', { fatal: true });\n${body}`);
wf("return s.decode(u(0x61))");
wf("return s.decode(u(0x00, 0xd8))");
wf("return s.decode(u(0x00, 0xd8, 0x61, 0))");
wf("return s.decode(u(0x00, 0xdc))");
wf("return s.decode(u(0x3d, 0xd8, 0x00, 0xde))");
wf("return [s.decode(u(0x3d), { stream: true }), s.decode(u(0xd8), { stream: true }), s.decode(u(0x00, 0xde))]");
wf("s.decode(u(0x3d, 0xd8), { stream: true }); return s.decode()");
wf("s.decode(u(0x3d, 0xd8), { stream: true }); return s.decode(u(0x61, 0), { stream: true })");
const b = (body) => stream("'utf-16be'", body);
b("return [cp(s.decode(u(0, 0x61, 0, 0x62))), cp(s.decode(u(0xfe, 0xff, 0, 0x61))), cp(s.decode(u(0xff, 0xfe, 0, 0x61))), cp(s.decode(u(0xd8, 0x3d, 0xde, 0))), cp(s.decode(u(0, 0x61, 0)))]");
b("return [cp(s.decode(u(0xd8, 0x3d), { stream: true })), cp(s.decode(u(0xde, 0x00)))]");
b("return cp(new TextDecoder('utf-16be', { ignoreBOM: true }).decode(u(0xfe, 0xff, 0, 0x61)))");

// windows-1252.
const l = (body) => stream("'latin1'", body);
l("var a = new Uint8Array(256); for (var i = 0; i < 256; i++) a[i] = i; return Array.from(s.decode(a), function (c) { return c.charCodeAt(0).toString(16) }).join(',')");
l("return cp(s.decode(u(0xef, 0xbb, 0xbf, 97)))");
l("return [s.decode(u(0xe9), { stream: true }), s.decode()]");
expr("new TextDecoder('windows-1252', { fatal: true }).decode(u(0x81, 0x80, 0xff))");
expr("cp(new TextDecoder('latin1', { ignoreBOM: true }).decode(u(0xef, 0xbb, 0xbf)))");
expr("new TextDecoder('ascii').decode(u(0x80, 0x9f, 0xa0))");

// Demais codificações de byte único do WHATWG: o nome canônico de cada rótulo e a tabela inteira de cada uma
// (tabelas em src/runtime/text_decoder_single_byte_data.rs, geradas por scripts/gen-text-decoder-tables.js).
const SINGLE_BYTE = ["ibm866", "iso-8859-2", "iso-8859-3", "iso-8859-4", "iso-8859-5", "iso-8859-6", "iso-8859-7", "iso-8859-8", "iso-8859-8-i", "iso-8859-10",
  "iso-8859-13", "iso-8859-14", "iso-8859-15", "iso-8859-16", "koi8-r", "koi8-u", "macintosh", "windows-874", "windows-1250", "windows-1251", "windows-1252",
  "windows-1253", "windows-1254", "windows-1255", "windows-1256", "windows-1257", "windows-1258", "x-mac-cyrillic", "x-user-defined"];
for (const name of SINGLE_BYTE) {
  // Todos os 256 bytes sem `fatal` (os indefinidos viram U+FFFD) e depois um a um com `fatal`.
  expr(`(function(){ var a = new Uint8Array(256); for (var i = 0; i < 256; i++) a[i] = i; var s = new TextDecoder('${name}'); return [s.encoding, cp(s.decode(a)).join(',')] })()`);
  expr(`(function(){ var s = new TextDecoder('${name}', { fatal: true }), bad = []; for (var i = 0; i < 256; i++) { try { s.decode(u(i)) } catch (e) { bad.push(i) } } return bad })()`);
}
for (const label of ["866", "cp866", "latin2", "ISO_8859-2:1987", "l3", "latin4", "cyrillic", "arabic", "asmo-708", "iso-8859-6-e", "greek", "greek8", "sun_eu_greek",
  "hebrew", "visual", "iso-8859-8-e", "logical", "csiso88598i", "latin6", "iso885913", "iso-8859-14", "l9", "iso-8859-16", "koi", "koi8_r", "koi8-ru", "mac",
  "x-mac-roman", "iso-8859-11", "tis-620", "dos-874", "x-cp1250", "cp1251", "cp1253", "latin5", "iso-8859-9", "l5", "windows-1255", "cp1256", "x-cp1257",
  "cp1258", "x-mac-ukrainian", "x-user-defined", "iso-8859-12", "iso-8859-8-i-e", "koi8-ru ", "\\tKOI8-R\\n", "cp1252", "windows-1251\\u0000", "latin-2", "mac-roman"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
// Sem BOM, sem estado de fluxo, `fatal` num byte indefinido no meio, `stream` sem presos.
expr("cp(new TextDecoder('iso-8859-2').decode(u(0xef, 0xbb, 0xbf, 97)))");
expr("cp(new TextDecoder('koi8-r', { ignoreBOM: true }).decode(u(0xef, 0xbb, 0xbf)))");
expr("(function(){ var s = new TextDecoder('windows-1251'); return [s.decode(u(0xcf, 0xf0), { stream: true }), s.decode(u(0xe8), { stream: true }), s.decode()] })()");
expr("new TextDecoder('windows-1253', { fatal: true }).decode(u(0x61, 0xaa, 0x62))");
expr("(function(){ var s = new TextDecoder('windows-1255', { fatal: true }); try { s.decode(u(0xd9)) } catch (e) {} return s.decode(u(0xe0)) })()");
expr("new TextDecoder('x-user-defined').decode(u(0x41, 0x80, 0xff)).length");
expr("cp(new TextDecoder('x-user-defined', { fatal: true }).decode(u(0x41, 0x80, 0xff)))");

// euc-kr (tabela em src/runtime/text_decoder_euc_kr_data.rs, gerada por scripts/gen-text-decoder-multibyte-tables.js):
// rótulos, o intervalo de ponteiros inteiro (lead 0x81..0xFE, trail 0x41..0xFE, um hash por lead), bytes fora do
// intervalo, trail ASCII que volta ao fluxo, trail não ASCII consumido, lead preso no fim, `stream` e `fatal`.
for (const label of ["euc-kr", "korean", "KS_C_5601-1987", "windows-949", "cseuckr", "csksc56011987", "iso-ir-149", "ks_c_5601-1989", "ksc5601", "ksc_5601",
  " euc-kr\\n", "x-euc-kr", "euc_kr", "euckr"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
expr(
  "(function(){ var s = new TextDecoder('euc-kr'), out = []; for (var l = 0x81; l <= 0xfe; l++) { var a = []; for (var t = 0x41; t <= 0xfe; t++) a.push(l, t); " +
    "out.push(cp(s.decode(new Uint8Array(a))).join(',')) } return out })()",
);
expr("(function(){ var s = new TextDecoder('euc-kr', { fatal: true }), bad = []; for (var l = 0; l < 256; l++) { for (var t = 0; t < 256; t += 5) { try { s.decode(u(l, t)) } catch (e) { bad.push(l * 256 + t) } } } return bad.length + ':' + bad.reduce(function (h, x) { return (h * 31 + x) % 1000003 }, 7) })()");
const k = (body) => stream("'euc-kr'", body);
k("var a = new Uint8Array(256); for (var i = 0; i < 256; i++) a[i] = i; return cp(s.decode(a)).join(',')");
k("return cp(s.decode(u(0xc7, 0xd1, 0xb1, 0xdb, 0x41, 0xa1, 0xa1)))");
k("return cp(s.decode(u(0xc7)))");
k("return cp(s.decode(u(0xc7, 0x41)))");
k("return cp(s.decode(u(0xc7, 0x20, 0x41)))");
k("return cp(s.decode(u(0xc7, 0x80, 0x41)))");
k("return cp(s.decode(u(0xc7, 0xff, 0x41)))");
k("return cp(s.decode(u(0xc9, 0xa1, 0x41)))");
k("return cp(s.decode(u(0x80, 0xff, 0xfe, 0xfe, 0x81, 0x40)))");
k("return cp(s.decode(u(0x81, 0x41, 0x41)))");
k("return cp(s.decode(u(0xa1, 0x0a)))");
k("return cp(s.decode(u(0xef, 0xbb, 0xbf, 0x41)))");
k("return cp(new TextDecoder('euc-kr', { ignoreBOM: true }).decode(u(0xef, 0xbb, 0xbf, 0x41)))");
k("return [cp(s.decode(u(0xc7), { stream: true })), cp(s.decode(u(0xd1), { stream: true })), cp(s.decode(u(0xc7), { stream: true })), cp(s.decode())]");
k("return [cp(s.decode(u(0xc7, 0x41), { stream: true })), cp(s.decode(u(0xc7), { stream: true })), cp(s.decode(u(0x41), { stream: true })), cp(s.decode())]");
k("return [cp(s.decode(u(0xc7, 0xd1, 0xc7), { stream: true })), cp(s.decode(u(0xd1, 0xc7), { stream: true })), cp(s.decode(u(0x20)))]");
k("return [cp(s.decode(u(0xc7), { stream: true })), cp(s.decode(u(0x80), { stream: true })), cp(s.decode(u(0x41)))]");
k("return [cp(s.decode(u(0xc9), { stream: true })), cp(s.decode(u(0xa1), { stream: true })), cp(s.decode())]");
k("return [cp(s.decode(u(0x41, 0xc7), { stream: true })), cp(s.decode(undefined, { stream: true })), cp(s.decode())]");
k("s.decode(u(0xc7), { stream: true }); s.decode(); return cp(s.decode(u(0xd1)))");
const kf = (body) => fn(`var s = new TextDecoder('euc-kr', { fatal: true });\n${body}`);
kf("return cp(s.decode(u(0xc7, 0xd1, 0x41)))");
kf("return s.decode(u(0xc7))");
kf("return s.decode(u(0xc7, 0x41))");
kf("return s.decode(u(0xc7, 0xff))");
kf("return s.decode(u(0x80))");
kf("return s.decode(u(0xc9, 0xa1))");
kf("return [s.decode(u(0xc7), { stream: true }), s.decode(u(0xd1), { stream: true }), s.decode()]");
kf("s.decode(u(0xc7), { stream: true }); return s.decode()");
kf("s.decode(u(0xc7), { stream: true }); try { s.decode(u(0x41)) } catch (e) {} return s.decode(u(0xd1))");
kf("s.decode(u(0xc7), { stream: true }); return s.decode(u(0x41), { stream: true })");

// big5 (tabela em src/runtime/text_decoder_big5_data.rs, gerada por scripts/gen-text-decoder-multibyte-tables.js): rótulos,
// o intervalo de ponteiros inteiro (lead 0x81..0xFE, trail 0x40..0x7E e 0xA1..0xFE, um hash por lead; inclui os planos 2 e 3
// e os quatro pares de dois pontos de código), bytes fora do intervalo, trail ASCII que volta ao fluxo, `stream` e `fatal`.
for (const label of ["big5", "big5-hkscs", "cn-big5", "csbig5", "x-x-big5", "BIG5", " Big5\\n", "big-5", "big5hkscs", "x-big5"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
expr(
  "(function(){ var s = new TextDecoder('big5'), out = []; for (var l = 0x81; l <= 0xfe; l++) { var a = []; for (var t = 0x40; t <= 0xfe; t++) { if (t > 0x7e && t < 0xa1) continue; a.push(l, t) } " +
    "out.push(cp(s.decode(new Uint8Array(a))).join(',')) } return out })()",
);
expr("(function(){ var s = new TextDecoder('big5', { fatal: true }), bad = []; for (var l = 0; l < 256; l++) { for (var t = 0; t < 256; t += 5) { try { s.decode(u(l, t)) } catch (e) { bad.push(l * 256 + t) } } } return bad.length + ':' + bad.reduce(function (h, x) { return (h * 31 + x) % 1000003 }, 7) })()");
const b5 = (body) => stream("'big5'", body);
b5("var a = new Uint8Array(256); for (var i = 0; i < 256; i++) a[i] = i; return cp(s.decode(a)).join(',')");
b5("return cp(s.decode(u(0xa4, 0x40, 0xa4, 0x41, 0x41, 0xf9, 0xfe)))");
b5("return cp(s.decode(u(0x88, 0x62, 0x88, 0x64, 0x88, 0xa3, 0x88, 0xa5)))");
b5("return cp(s.decode(u(0x87, 0x40, 0x87, 0x41, 0x8e, 0x69)))");
b5("return cp(s.decode(u(0xa4)))");
b5("return cp(s.decode(u(0xa4, 0x41)))");
b5("return cp(s.decode(u(0xa4, 0x20, 0x41)))");
b5("return cp(s.decode(u(0xa4, 0x80, 0x41)))");
b5("return cp(s.decode(u(0xa4, 0xff, 0x41)))");
b5("return cp(s.decode(u(0x81, 0x40, 0x41)))");
b5("return cp(s.decode(u(0x80, 0xff, 0xfe, 0xfe, 0x81, 0x7f)))");
b5("return cp(s.decode(u(0x81, 0x41, 0x41)))");
b5("return cp(s.decode(u(0xef, 0xbb, 0xbf, 0x41)))");
b5("return cp(new TextDecoder('big5', { ignoreBOM: true }).decode(u(0xef, 0xbb, 0xbf, 0x41)))");
b5("return [cp(s.decode(u(0xa4), { stream: true })), cp(s.decode(u(0x40), { stream: true })), cp(s.decode(u(0xa4), { stream: true })), cp(s.decode())]");
b5("return [cp(s.decode(u(0xa4, 0x41), { stream: true })), cp(s.decode(u(0xa4), { stream: true })), cp(s.decode(u(0x41), { stream: true })), cp(s.decode())]");
b5("return [cp(s.decode(u(0x88), { stream: true })), cp(s.decode(u(0x62), { stream: true })), cp(s.decode(u(0x88, 0x64, 0x88), { stream: true })), cp(s.decode(u(0xa5)))]");
b5("return [cp(s.decode(u(0xa4), { stream: true })), cp(s.decode(u(0x80), { stream: true })), cp(s.decode(u(0x41)))]");
b5("return [cp(s.decode(u(0x41, 0xa4), { stream: true })), cp(s.decode(undefined, { stream: true })), cp(s.decode())]");
b5("s.decode(u(0xa4), { stream: true }); s.decode(); return cp(s.decode(u(0x40)))");
const b5f = (body) => fn(`var s = new TextDecoder('big5', { fatal: true });\n${body}`);
b5f("return cp(s.decode(u(0xa4, 0x40, 0x88, 0x62, 0x41)))");
b5f("return s.decode(u(0xa4))");
b5f("return s.decode(u(0xa4, 0x41))");
b5f("return s.decode(u(0xa4, 0xff))");
b5f("return s.decode(u(0x80))");
b5f("return s.decode(u(0x81, 0x40))");
b5f("return [s.decode(u(0xa4), { stream: true }), s.decode(u(0x40), { stream: true }), s.decode()]");
b5f("s.decode(u(0xa4), { stream: true }); return s.decode()");
b5f("s.decode(u(0xa4), { stream: true }); try { s.decode(u(0x41)) } catch (e) {} return s.decode(u(0x40))");

// shift_jis e euc-jp (jis0208 compartilhada e jis0212 em src/runtime/text_decoder_jis_data.rs, geradas por
// scripts/gen-text-decoder-multibyte-tables.js jis): rótulos, o intervalo de ponteiros inteiro de cada um (um hash por
// lead), bytes fora do intervalo, katakana de meia largura, trail ASCII que volta ao fluxo, trail não ASCII consumido,
// prefixo preso no fim (inclusive o de três bytes do euc-jp), `stream` e `fatal`.
for (const label of ["shift_jis", "sjis", "Shift-JIS", "csshiftjis", "ms932", "ms_kanji", "windows-31j", "x-sjis", "shift_jisx", "euc-jp", "x-euc-jp",
  "cseucpkdfmtjapanese", " EUC-JP\\n", "eucjp", "euc_jp", "iso-2022-jp"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
expr(
  "(function(){ var s = new TextDecoder('shift_jis'), out = []; for (var l = 0; l < 256; l++) { if (l > 0x80 && l < 0xa1 || l > 0xdf && l < 0xe0 || l > 0xfc) continue; var a = []; " +
    "for (var t = 0x40; t <= 0xfc; t++) a.push(l, t); out.push(cp(s.decode(new Uint8Array(a))).join(',')) } return out })()",
);
expr(
  "(function(){ var s = new TextDecoder('euc-jp'), out = []; for (var l = 0x8e; l <= 0xfe; l++) { var a = [], b = []; for (var t = 0xa1; t <= 0xfe; t++) { a.push(l, t); b.push(0x8f, l, t) } " +
    "out.push(cp(s.decode(new Uint8Array(a))).join(','), cp(s.decode(new Uint8Array(b))).join(',')) } return out })()",
);
expr("(function(){ var s = new TextDecoder('euc-jp'), out = []; for (var t = 0xa1; t <= 0xdf; t++) out.push(cp(s.decode(u(0x8e, t)))[0]); return out })()");
for (const label of ["shift_jis", "euc-jp"]) {
  expr(`(function(){ var s = new TextDecoder('${label}', { fatal: true }), bad = []; for (var l = 0; l < 256; l++) { for (var t = 0; t < 256; t += 3) { try { s.decode(u(l, t)) } catch (e) { bad.push(l * 256 + t) } } } return bad.length + ':' + bad.reduce(function (h, x) { return (h * 31 + x) % 1000003 }, 7) })()`);
  const j = (body) => stream(`'${label}'`, body);
  j("var a = new Uint8Array(256); for (var i = 0; i < 256; i++) a[i] = i; return cp(s.decode(a)).join(',')");
  j("return cp(s.decode(u(0x41, 0x5c, 0x7e, 0x7f, 0x80, 0x81, 0x5c, 0xa1, 0xc0, 0xb1, 0xb1)))");
  j("return cp(s.decode(u(0x8e, 0xa1, 0x8e, 0xdf, 0x8e, 0xe0, 0x8e, 0x41, 0x8e)))");
  j("return cp(s.decode(u(0x8f, 0xa2, 0xaf, 0x8f, 0xa2, 0x41, 0x8f, 0x41, 0x8f, 0xa2, 0xff, 0x8f, 0x8f, 0xa2)))");
  j("return cp(s.decode(u(0x81)))");
  j("return cp(s.decode(u(0x8f, 0xa2)))");
  j("return cp(s.decode(u(0x81, 0x41, 0x41)))");
  j("return cp(s.decode(u(0x81, 0x7f, 0x81, 0x80, 0x81, 0xfd, 0xf0, 0x40, 0xf9, 0xfc, 0xfa, 0x40, 0xfc, 0xfc)))");
  j("return cp(s.decode(u(0xef, 0xbb, 0xbf, 0x41)))");
  j("return [cp(s.decode(u(0x8f), { stream: true })), cp(s.decode(u(0xa2), { stream: true })), cp(s.decode(u(0xaf), { stream: true })), cp(s.decode())]");
  j("return [cp(s.decode(u(0x8f, 0xa2), { stream: true })), cp(s.decode(u(0x41), { stream: true })), cp(s.decode())]");
  j("return [cp(s.decode(u(0x41, 0x8e), { stream: true })), cp(s.decode(u(0xa1), { stream: true })), cp(s.decode(undefined, { stream: true })), cp(s.decode())]");
  j("return [cp(s.decode(u(0xb0), { stream: true })), cp(s.decode(u(0xa1), { stream: true })), cp(s.decode(u(0xb0), { stream: true })), cp(s.decode())]");
  j("s.decode(u(0x8f, 0xa2), { stream: true }); s.decode(); return cp(s.decode(u(0xaf)))");
  const jf = (body) => fn(`var s = new TextDecoder('${label}', { fatal: true });\n${body}`);
  jf("return cp(s.decode(u(0xb0, 0xa1, 0x41)))");
  jf("return s.decode(u(0x8f, 0xa2))");
  jf("return s.decode(u(0x81))");
  jf("return s.decode(u(0xa0))");
  jf("return s.decode(u(0x8e, 0xe0))");
  jf("return s.decode(u(0x8f, 0xa2, 0xff))");
  jf("return [s.decode(u(0x8f), { stream: true }), s.decode(u(0xa2, 0xaf), { stream: true }), s.decode()]");
  jf("s.decode(u(0x8f, 0xa2), { stream: true }); return s.decode()");
  jf("s.decode(u(0x8f, 0xa2), { stream: true }); try { s.decode(u(0x41)) } catch (e) {} return s.decode(u(0xaf))");
}

// iso-2022-jp (decodificador de estados do WHATWG sobre a jis0208; modelo conferido em sequências aleatórias por
// scripts/check-text-decoder-iso-2022-jp.js): rótulos, o plano 94 x 94 inteiro, katakana, Roman, escapes inválidos, ESC
// no fim, flag de saída (escape repetido), controles, bytes altos, estado persistente com `stream` e `fatal`.
for (const label of ["iso-2022-jp", "csiso2022jp", "ISO-2022-JP", " iso-2022-jp\\n", "CsISO2022JP", "iso-2022-jp-2", "iso-2022-kr", "iso2022jp"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
expr(
  "(function(){ var s = new TextDecoder('iso-2022-jp'), out = []; for (var l = 0x21; l <= 0x7e; l++) { var a = [0x1b, 0x24, 0x42]; for (var t = 0x21; t <= 0x7e; t++) a.push(l, t); " +
    "out.push(cp(s.decode(new Uint8Array(a))).join(',')) } return out })()",
);
expr("(function(){ var s = new TextDecoder('iso-2022-jp'), out = []; for (var t = 0; t < 256; t++) out.push(cp(s.decode(u(0x1b, 0x28, 0x49, t))).join(',')); return out })()");
expr("(function(){ var s = new TextDecoder('iso-2022-jp'), out = []; for (var t = 0; t < 256; t++) out.push(cp(s.decode(u(0x1b, 0x28, 0x4a, t, t))).join(',')); return out })()");
expr("(function(){ var s = new TextDecoder('iso-2022-jp'), out = []; for (var t = 0; t < 256; t++) out.push(cp(s.decode(u(t, 0x41))).join(',')); return out })()");
expr("(function(){ var s = new TextDecoder('iso-2022-jp'), out = []; for (var a = 0; a < 256; a++) for (var b = 0; b < 256; b += 7) out.push(cp(s.decode(u(0x1b, a, b, 0x41))).join(',')); return out.join(';') })()");
expr("(function(){ var s = new TextDecoder('iso-2022-jp'), bad = []; for (var a = 0; a < 256; a += 3) for (var b = 0; b < 256; b += 5) { var r = cp(s.decode(u(0x1b, 0x24, a, b, 0x1b, 0x28, b, 0x30, 0x21))).join(','); bad.push(r) } return bad.length + ':' + bad.join(';').length + ':' + bad.reduce(function (h, x) { return (h * 31 + x.length) % 1000003 }, 7) })()");
const isoBytes = [
  "0x41, 0x1b", "0x41, 0x1b, 0x24", "0x1b, 0x24, 0x42", "0x1b, 0x24, 0x42, 0x30, 0x21, 0x1b, 0x28, 0x42, 0x41", "0x1b, 0x24, 0x42, 0x30",
  "0x1b, 0x24, 0x42, 0x30, 0x1b, 0x28, 0x42, 0x41", "0x1b, 0x24, 0x43, 0x41", "0x1b, 0x41, 0x42", "0x1b, 0x28, 0x42, 0x1b, 0x28, 0x42, 0x41",
  "0x1b, 0x24, 0x42, 0x1b, 0x24, 0x40, 0x30, 0x21", "0x1b, 0x28, 0x4a, 0x5c, 0x7e, 0x41", "0x1b, 0x28, 0x49, 0x21, 0x5f, 0x60, 0x20, 0x41",
  "0x0e, 0x0f, 0x41", "0x80, 0xa1, 0x41", "0, 0x0a, 0x7f", "0x1b, 0x24, 0x42, 0x0a, 0x30, 0x0a, 0x21", "0x1b, 0x24, 0x42, 0x21, 0x21, 0x7e, 0x7e",
  "0x1b, 0x24, 0x42, 0x30, 0x80, 0x30, 0x21", "0x1b, 0x24, 0x40, 0x30, 0x21", "0x1b, 0x28", "0x1b, 0x28, 0x4a", "0x1b, 0x24, 0x42, 0x30, 0x21, 0x1b",
  "0xef, 0xbb, 0xbf, 0x41", "0x1b, 0x1b, 0x1b, 0x28, 0x42", "0x1b, 0x24, 0x28, 0x42, 0x41", "0x1b, 0x28, 0x49, 0x1b, 0x28, 0x4a, 0x5c",
];
for (const bytes of isoBytes) {
  expr(`(function(){ var s = new TextDecoder('iso-2022-jp'); return cp(s.decode(u(${bytes}))) })()`);
  expr(`(function(){ var s = new TextDecoder('iso-2022-jp', { fatal: true }); return cp(s.decode(u(${bytes}))) })()`);
}
const iso = (body) => stream("'iso-2022-jp'", body);
iso("return [cp(s.decode(u(0x1b, 0x24), { stream: true })), cp(s.decode(u(0x42, 0x30), { stream: true })), cp(s.decode(u(0x21), { stream: true })), cp(s.decode(u(0x1b), { stream: true })), cp(s.decode()), cp(s.decode(u(0x41)))]");
iso("return [cp(s.decode(u(0x1b, 0x24, 0x42, 0x30), { stream: true })), cp(s.decode()), cp(s.decode(u(0x30, 0x21, 0x41)))]");
iso("return [cp(s.decode(u(0x1b, 0x24, 0x42, 0x30, 0x21), { stream: true })), cp(s.decode(u(0x30, 0x21))), cp(s.decode(u(0x30, 0x21)))]");
iso("return [cp(s.decode(u(0x1b, 0x28), { stream: true })), cp(s.decode(u(0x42), { stream: true })), cp(s.decode(u(0x41)))]");
iso("return [cp(s.decode(u(0x1b, 0x28, 0x4a), { stream: true })), cp(s.decode(u(0x5c))), cp(s.decode(u(0x5c)))]");
iso("return [cp(s.decode(u(0x1b, 0x28, 0x42), { stream: true })), cp(s.decode(u(0x1b, 0x28, 0x42)))]");
iso("return [cp(s.decode(u(0x1b, 0x28, 0x42, 0x41), { stream: true })), cp(s.decode(u(0x1b, 0x28, 0x42)))]");
iso("return [cp(s.decode(u(0x1b), { stream: true })), cp(s.decode(u(0x24), { stream: true })), cp(s.decode(u(0x43), { stream: true })), cp(s.decode())]");
iso("return [cp(s.decode(u(0x1b, 0x24, 0x42), { stream: true })), cp(s.decode(undefined, { stream: true })), cp(s.decode(u(0x1b, 0x28, 0x49, 0x31), { stream: true })), cp(s.decode())]");
iso("return [cp(s.decode(u(0x1b, 0x24, 0x42, 0x30), { stream: true })), cp(s.decode(u(0x1b), { stream: true })), cp(s.decode(u(0x28, 0x42, 0x41)))]");
const isof = (body) => fn(`var s = new TextDecoder('iso-2022-jp', { fatal: true });\n${body}`);
isof("return [s.decode(u(0x1b, 0x24, 0x42, 0x30), { stream: true }), s.decode(u(0x21), { stream: true }), s.decode()]");
isof("s.decode(u(0x1b, 0x24, 0x42, 0x30), { stream: true }); try { s.decode(u(0x41)) } catch (e) {} return cp(s.decode(u(0x30, 0x21)))");
isof("s.decode(u(0x1b, 0x24, 0x42), { stream: true }); try { s.decode(u(0x0a)) } catch (e) {} return cp(s.decode(u(0x30, 0x21)))");
isof("s.decode(u(0x1b), { stream: true }); return s.decode()");
isof("s.decode(u(0x1b, 0x28, 0x4a), { stream: true }); try { s.decode(u(0x80), { stream: true }) } catch (e) {} return cp(s.decode(u(0x5c)))");
isof("return cp(new TextDecoder('iso-2022-jp', { fatal: true, ignoreBOM: true }).decode(u(0x41)))");

// gb18030 e gbk (tabelas em src/runtime/text_decoder_gb18030_data.rs, geradas por scripts/gen-text-decoder-multibyte-tables.js;
// modelo conferido por scripts/check-text-decoder-gb18030.js): rótulos (o `encoding` do gbk é "gbk"), os pares de dois bytes
// inteiros (um hash por lead), o espaço de quatro bytes inteiro (um hash por primeiro byte, com os planos suplementares),
// `0x80`, bytes fora do intervalo, trail ASCII que volta ao fluxo, o terceiro e o quarto byte fora do intervalo, prefixo
// de até três bytes preso com `stream`, e `fatal`.
for (const label of ["gb18030", "GB18030", " gb18030\\n", "gbk", "GBK", "gb2312", "chinese", "csgb2312", "csiso58gb231280", "gb_2312", "gb_2312-80",
  "iso-ir-58", "x-gbk", "x-gb18030", "gb-18030", "gb18030-2005", "hz-gb-2312", "gb2312-80"]) {
  expr(`(function(x){ return [x.encoding, x.fatal, x.ignoreBOM] })(new TextDecoder("${label}"))`);
}
const hashCps = "function (h, x) { return (h * 31 + x) % 1000003 }";
for (const label of ["gb18030", "gbk"]) {
  expr(
    `(function(){ var s = new TextDecoder('${label}'), out = []; for (var l = 0x81; l <= 0xfe; l++) { var a = []; for (var t = 0x40; t <= 0xfe; t++) { if (t === 0x7f) continue; a.push(l, t) } ` +
      "out.push(cp(s.decode(new Uint8Array(a))).join(',')) } return out })()",
  );
}
// Os quatro bytes: para cada b1 e b2, todos os b3 e b4 (126 * 10 sequências), com o hash dos pontos de código.
expr(
  "(function(){ var s = new TextDecoder('gb18030'), out = []; for (var b1 = 0x81; b1 <= 0xfe; b1++) { var h = 7, n = 0; " +
    "for (var b2 = 0x30; b2 <= 0x39; b2++) { var a = []; for (var b3 = 0x81; b3 <= 0xfe; b3++) for (var b4 = 0x30; b4 <= 0x39; b4++) a.push(b1, b2, b3, b4); " +
    `var c = cp(s.decode(new Uint8Array(a))); n += c.length; h = c.reduce(${hashCps}, h) } out.push(n + ':' + h) } return out })()`,
);
expr("(function(){ var s = new TextDecoder('gb18030', { fatal: true }), bad = []; for (var l = 0; l < 256; l++) { for (var t = 0; t < 256; t += 5) { try { s.decode(u(l, t)) } catch (e) { bad.push(l * 256 + t) } } } return bad.length + ':' + bad.reduce(function (h, x) { return (h * 31 + x) % 1000003 }, 7) })()");
const gb = (body) => stream("'gb18030'", body);
gb("var a = new Uint8Array(256); for (var i = 0; i < 256; i++) a[i] = i; return cp(s.decode(a)).join(',')");
gb("return cp(s.decode(u(0x80, 0x41, 0xa2, 0xe3, 0xa1, 0xa4, 0xa1, 0xaa, 0xa8, 0xbf, 0xa6, 0xd9, 0xfe, 0xfe, 0x81, 0x40)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x81, 0x30, 0x84, 0x31, 0xa4, 0x39, 0x90, 0x30, 0x81, 0x30, 0xe3, 0x32, 0x9a, 0x35)))");
gb("return cp(s.decode(u(0x84, 0x31, 0xa5, 0x30, 0x41, 0xe3, 0x32, 0x9a, 0x36, 0x41, 0xfe, 0x39, 0xfe, 0x39, 0x41)))");
gb("return cp(s.decode(u(0x81, 0x35, 0xf4, 0x37, 0x81, 0x35, 0xf4, 0x38, 0x81, 0x36, 0x81, 0x30, 0x81, 0x30, 0xfe, 0x30)))");
gb("return cp(s.decode(u(0x81)))");
gb("return cp(s.decode(u(0x81, 0x30)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x81)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x81, 0x30, 0x81)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x41)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x81, 0x41)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x81, 0x20, 0x41)))");
gb("return cp(s.decode(u(0x81, 0x30, 0x81, 0xff)))");
gb("return cp(s.decode(u(0x81, 0x30, 0xff)))");
gb("return cp(s.decode(u(0x81, 0x41, 0x41)))");
gb("return cp(s.decode(u(0x81, 0x7f, 0x81, 0x20, 0x81, 0x00)))");
gb("return cp(s.decode(u(0x81, 0x80, 0x81, 0xff, 0x81, 0xfe, 0xff, 0xa0)))");
gb("return cp(s.decode(u(0x81, 0x3a, 0x81, 0x2f, 0x81, 0x3f)))");
gb("return cp(s.decode(u(0xef, 0xbb, 0xbf, 0x41)))");
gb("return cp(new TextDecoder('gb18030', { ignoreBOM: true }).decode(u(0xef, 0xbb, 0xbf, 0x41)))");
gb("return [cp(s.decode(u(0x81), { stream: true })), cp(s.decode(u(0x30), { stream: true })), cp(s.decode(u(0x81), { stream: true })), cp(s.decode(u(0x30), { stream: true })), cp(s.decode())]");
gb("return [cp(s.decode(u(0x81, 0x30, 0x81), { stream: true })), cp(s.decode(u(0x41), { stream: true })), cp(s.decode())]");
gb("return [cp(s.decode(u(0x81, 0x30), { stream: true })), cp(s.decode(u(0x41), { stream: true })), cp(s.decode())]");
gb("return [cp(s.decode(u(0x81, 0x30, 0x81), { stream: true })), cp(s.decode(u(0x30), { stream: true })), cp(s.decode(u(0x41), { stream: true })), cp(s.decode())]");
gb("return [cp(s.decode(u(0x81, 0x30, 0x81), { stream: true })), cp(s.decode())]");
gb("return [cp(s.decode(u(0x41, 0xa2), { stream: true })), cp(s.decode(u(0xe3), { stream: true })), cp(s.decode(undefined, { stream: true })), cp(s.decode())]");
gb("return [cp(s.decode(u(0xa2), { stream: true })), cp(s.decode(u(0x80), { stream: true })), cp(s.decode(u(0x41)))]");
gb("return [cp(s.decode(u(0x84, 0x31, 0xa4), { stream: true })), cp(s.decode(u(0x39, 0x90), { stream: true })), cp(s.decode(u(0x30, 0x81, 0x30)))]");
gb("s.decode(u(0x81, 0x30, 0x81), { stream: true }); s.decode(); return cp(s.decode(u(0x30)))");
const gbf = (body) => fn(`var s = new TextDecoder('gb18030', { fatal: true });\n${body}`);
gbf("return cp(s.decode(u(0xa1, 0xa4, 0x81, 0x30, 0x81, 0x30, 0x80, 0x41)))");
gbf("return s.decode(u(0x81))");
gbf("return s.decode(u(0x81, 0x30, 0x81))");
gbf("return s.decode(u(0x81, 0x30, 0x41))");
gbf("return s.decode(u(0x81, 0x41))");
gbf("return s.decode(u(0x81, 0xff))");
gbf("return s.decode(u(0xff))");
gbf("return s.decode(u(0x84, 0x31, 0xa5, 0x30))");
gbf("return s.decode(u(0xa1, 0x7f))");
gbf("return [s.decode(u(0x81, 0x30), { stream: true }), s.decode(u(0x81, 0x30), { stream: true }), s.decode()]");
gbf("s.decode(u(0x81, 0x30, 0x81), { stream: true }); return s.decode()");
gbf("s.decode(u(0x81, 0x30, 0x81), { stream: true }); try { s.decode(u(0x41)) } catch (e) {} return cp(s.decode(u(0x30, 0x41)))");
expr("(function(){ var s = new TextDecoder('gbk'), a = u(0xa2, 0xe3, 0x80, 0x81, 0x30, 0x81, 0x30); return [s.encoding, cp(s.decode(a))] })()");

// Auditoria de UTF-8/UTF-16 em `stream`: prefixos inválidos que não esperam o resto, BOM de UTF-16 partido entre
// pedaços, `fatal` com `ignoreBOM`, `fatal` com UTF-16BE e com SharedArrayBuffer, sequência presa com `fatal`.
const au = (encoding, options, body) => fn(`var s = new TextDecoder('${encoding}', ${options});\n${body}`);
au("utf-16le", "{}", "return [cp(s.decode(u(0xff), { stream: true })), cp(s.decode(u(0xfe), { stream: true })), cp(s.decode(u(0x61, 0)))]");
au("utf-16le", "{}", "return [cp(s.decode(u(0xff, 0xfe, 0x61, 0), { stream: true })), cp(s.decode(u(0xff, 0xfe, 0x62, 0)))]");
au("utf-16le", "{}", "return cp(s.decode(u(0xff, 0xfe), { stream: true }))");
au("utf-16le", "{}", "return [cp(s.decode(u(0x3d, 0xd8, 0x61), { stream: true })), cp(s.decode(u(0)))]");
au("utf-16le", "{}", "return [cp(s.decode(u(0x3d), { stream: true })), cp(s.decode(u(0xd8, 0x00), { stream: true })), cp(s.decode(u(0xde)))]");
au("utf-16be", "{ fatal: true }", "return s.decode(u(0xd8, 0x3d))");
au("utf-16be", "{ fatal: true }", "return s.decode(u(0, 0x61, 0))");
au("utf-16le", "{ fatal: true }", "return s.decode(new SharedArrayBuffer(3))");
au("utf-16le", "{ fatal: true }", "return s.decode(u(0x61, 0, 0x62))");
au("utf-8", "{ fatal: true, ignoreBOM: true }", "return cp(s.decode(u(0xef, 0xbb, 0xbf, 0x61)))");
au("utf-8", "{ fatal: true }", "return s.decode(u(0xf4, 0x90, 0x80, 0x80))");
au("utf-8", "{ fatal: true }", "return s.decode(u(0xef, 0xbb))");
au("utf-8", "{ fatal: true }", "return s.decode(u(0xef, 0xbb), { stream: true })");
au("utf-8", "{}", "return [cp(s.decode(u(0xf0, 0x80), { stream: true })), cp(s.decode(u(0x80, 0x80)))]");
au("utf-8", "{}", "return [cp(s.decode(u(0xf4), { stream: true })), cp(s.decode(u(0x90), { stream: true })), cp(s.decode(u(0x80, 0x80)))]");
au("utf-8", "{}", "return [cp(s.decode(u(0xe2, 0x82), { stream: true })), cp(s.decode(u(0xe2, 0x82, 0xac)))]");
au("utf-8", "{}", "return [cp(s.decode(u(0xed), { stream: true })), cp(s.decode(u(0xa0), { stream: true })), cp(s.decode(u(0x80)))]");
au("utf-8", "{}", "return [cp(s.decode(u(0xe0), { stream: true })), cp(s.decode(u(0x9f), { stream: true })), cp(s.decode(u(0x80)))]");
expr("new TextDecoder().decode(new Uint8Array(new SharedArrayBuffer(3), 1, 2))");
expr("new TextDecoder('\\u0000')");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
