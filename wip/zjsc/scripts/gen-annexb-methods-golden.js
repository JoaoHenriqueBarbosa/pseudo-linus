// Gera tests/golden/annexb_methods_bun.tsv (e annexb_methods.preludes.json): métodos legados e Annex B de objeto e string
// medidos no bun 1.4.2 (TZ=UTC): __defineGetter__, __defineSetter__, __lookupGetter__ e __lookupSetter__ em receptores
// variados (primitivos, protótipo nulo, Proxy, congelados, revogados); o getter e o setter de Object.prototype.__proto__ em
// receptores exóticos; os métodos HTML de String (anchor, big, ...) com argumentos com aspas; escape e unescape;
// Date.prototype.getYear, setYear e toGMTString; as estáticas legadas de RegExp (só as que o próprio programa
// estabelece antes de ler) e RegExp.prototype.compile; Function.prototype.caller e arguments; hasOwnProperty,
// isPrototypeOf, propertyIsEnumerable, toLocaleString, valueOf e toString com receptores null, undefined e primitivos.
// Cada programa é o prelúdio comum (S, N, C, T) mais uma única linha `T(function () { ... });`, que grava o texto do
// resultado ou `Nome: mensagem` do erro em `globalThis.R`. Sem APIs de host. Um bun filho novo por programa, no máximo 8
// em paralelo, com timeout de 5 s; deduplica por igualdade do programa contra tests/golden/*.tsv.
// Colunas do tsv: formato fatorado de scripts/golden-prelude.js.
// Uso: bun scripts/gen-annexb-methods-golden.js
const fs = require("fs");
const os = require("os");
const path = require("path");
const crypto = require("crypto");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, GOLDEN_DIR } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

const NAME = "annexb_methods";
const PRELUDE = [
  "var S = function S(v) {",
  "  switch (typeof v) {",
  '    case "string": return JSON.stringify(v);',
  '    case "number": return Object.is(v, -0) ? "-0" : String(v);',
  '    case "bigint": return v + "n";',
  '    case "symbol": return v.toString();',
  '    case "function": return "function";',
  '    case "object": return v === null ? "null" : Array.isArray(v) ? "[" + v.map(function (x) { return S(x); }).join(",") + "]" : "object";',
  "    default: return String(v);",
  "  }",
  "};",
  "var NAMES = [[Object.prototype, 'Object.prototype'], [Array.prototype, 'Array.prototype'], [Function.prototype, 'Function.prototype'], [String.prototype, 'String.prototype'], [Number.prototype, 'Number.prototype'], [Boolean.prototype, 'Boolean.prototype'], [Symbol.prototype, 'Symbol.prototype'], [BigInt.prototype, 'BigInt.prototype'], [Date.prototype, 'Date.prototype'], [RegExp.prototype, 'RegExp.prototype']];",
  "var N = function (p) { if (p === null) return 'null'; for (var i = 0; i < NAMES.length; i++) if (NAMES[i][0] === p) return NAMES[i][1]; return typeof p === 'function' ? 'other-function' : 'other-object'; };",
  "var C = function (o) { return (typeof o === 'object' || typeof o === 'function') ? Object.create(o) : {}; };",
  "var T = function (f) {",
  "  var r;",
  "  try { r = S(f()); } catch (e) {",
  "    try { r = (typeof e === 'object' && e !== null && typeof e.name === 'string') ? e.name + ': ' + e.message : 'throw ' + S(e); } catch (e2) { r = 'unprintable'; }",
  "  }",
  "  globalThis.R = r;",
  "};",
  "",
].join("\n");

const programs = [];
const add = body => programs.push(PRELUDE + "T(function () { " + body + " });\n");
const q = JSON.stringify;
// Literal de string sem travessão nem separadores de linha crus.
const BAD = new RegExp("[" + [0x2013, 0x2014, 0x2028, 0x2029].map(c => "\\u" + c.toString(16)).join("") + "]", "g");
const lit = s => q(s).replace(BAD, c => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));

const REVOKED = "(function () { var p = Proxy.revocable({}, {}); p.revoke(); return p.proxy; })()";
const REVOKED_FN = "(function () { var p = Proxy.revocable(function () {}, {}); p.revoke(); return p.proxy; })()";
const ALL = [
  "undefined", "null", "0", "-0", "NaN", "''", "'abc'", "true", "false", "Symbol('s')", "10n",
  "({})", "[]", "[1, 2]", "(function () {})", "(() => 1)", "(class {})", "Object.create(null)", "Object.create({ a: 1 })",
  "new Proxy({}, {})", "new Proxy([], {})", "new Proxy(function () {}, {})",
  "Object.freeze({ a: 1 })", "Object.freeze([1])", "Object.seal({ a: 1 })", "Object.preventExtensions({})",
  "new String('ab')", "new Number(3)", "new Boolean(false)", "(function () { return arguments; })(1, 2)",
  "new Date(0)", "/x/g", "new Map()", "new Error('e')", "Math", "JSON", "globalThis", "Object.prototype", "Array.prototype",
  "Function.prototype", "new Uint8Array(2)", REVOKED, REVOKED_FN,
  "new Proxy({}, { defineProperty() { return false; }, getOwnPropertyDescriptor() { return undefined; }, setPrototypeOf() { return false; }, getPrototypeOf() { return null; } })",
  "new Proxy({}, { defineProperty() { throw new RangeError('dp'); }, getOwnPropertyDescriptor() { throw new RangeError('gopd'); }, getPrototypeOf() { throw new RangeError('gpo'); }, setPrototypeOf() { throw new RangeError('spo'); }, has() { throw new RangeError('has'); }, get() { throw new RangeError('get'); } })",
];
const MAIN = [
  "undefined", "null", "0", "'abc'", "true", "Symbol('s')", "10n",
  "({})", "[]", "(function () {})", "Object.create(null)", "Object.create({ a: 1 })",
  "new Proxy({}, {})", "new Proxy({}, { defineProperty() { return false; }, getOwnPropertyDescriptor() { return undefined; } })",
  "Object.freeze({ a: 1 })", "Object.freeze([1])", "Object.seal({ a: 1 })", "Object.preventExtensions({})",
  "new String('ab')", "(function () { return arguments; })(1, 2)", "new Date(0)", "new Uint8Array(2)", REVOKED,
  "new Proxy({}, { defineProperty() { throw new RangeError('dp'); }, getOwnPropertyDescriptor() { throw new RangeError('gopd'); } })",
];
const P = "Object.prototype";

// ---- __defineGetter__ e __defineSetter__
const FN = "function () { return 1; }";
const defineArgs = [
  ['"a"', FN], ['"a"', "1"], ['"a"', "undefined"], ['"a"', "null"], ['"a"', "{}"], ['"a"', "class {}"],
  ["Symbol.iterator", FN], ["1", FN], ["{ toString() { throw new RangeError('key'); } }", FN], ["undefined", FN],
  ['"a"', "new Proxy(function () {}, {})"], ['"__proto__"', FN],
];
for (const method of ["__defineGetter__", "__defineSetter__"]) {
  for (const recv of MAIN) {
    for (const [key, fn] of defineArgs) {
      const tail = "var d = Object.getOwnPropertyDescriptor(Object(o), k); return [r, d ? [typeof d.get, typeof d.set, d.enumerable, d.configurable] : 'none'];";
      add(`var o = ${recv}; var k = ${key}; var f = ${fn}; var r = ${P}.${method}.call(o, k, f); ${tail}`);
      add(`var o = ${recv}; var k = ${key}; var f = ${fn}; var r = o.${method}(k, f); ${tail}`);
    }
    add(`var o = ${recv}; return ${P}.${method}.call(o);`);
    add(`var o = ${recv}; return ${P}.${method}.call(o, 'a');`);
  }
}
for (const c of [
  "var o = {}; o.__defineGetter__('x', function () { return this === o; }); return [o.x, Object.keys(o).join()];",
  "var o = {}; o.__defineSetter__('x', function (v) { this.y = v; }); o.x = 5; return [o.y, o.x, Object.keys(o).join()];",
  "var o = { x: 1 }; o.__defineGetter__('x', function () { return 2; }); return [o.x, Object.getOwnPropertyDescriptor(o, 'x').enumerable];",
  "var o = {}; Object.defineProperty(o, 'x', { value: 1, configurable: false }); o.__defineGetter__('x', function () {});",
  "var o = {}; Object.defineProperty(o, 'x', { value: 1, configurable: true }); o.__defineGetter__('x', function () { return 3; }); return [o.x, Object.getOwnPropertyDescriptor(o, 'x').configurable];",
  "var o = {}; o.__defineGetter__('x', function () { return 1; }); o.__defineSetter__('x', function () {}); var d = Object.getOwnPropertyDescriptor(o, 'x'); return [typeof d.get, typeof d.set];",
  "var o = {}; o.__defineGetter__('x', function () { return 1; }); o.__defineGetter__('x', function () { return 2; }); return o.x;",
  "var o = []; o.__defineGetter__('length', function () { return 1; });",
  "var o = []; o.__defineGetter__('0', function () { return 9; }); return [o[0], o.length];",
  "var o = new Uint8Array(2); o.__defineGetter__('0', function () { return 9; });",
  "var o = new String('ab'); o.__defineGetter__('0', function () { return 9; });",
  "var o = {}; var f = function () { return 1; }; o.__defineGetter__('x', f); return Object.getOwnPropertyDescriptor(o, 'x').get === f;",
  "var o = {}; o.__defineGetter__('x', function () {}); return Object.getOwnPropertyDescriptor(o, 'x').get.name;",
  "var o = {}; return o.__defineGetter__('x', function () {});",
  "var o = {}; return o.__defineSetter__('x', function () {});",
  "var o = Object.create(null); o.__defineGetter__ = 1; return typeof o.__defineGetter__;",
  "'use strict'; var o = Object.freeze({}); o.__defineSetter__('a', function () {});",
  "var log = []; var o = new Proxy({}, { defineProperty(t, k, d) { log.push(k + ':' + Object.keys(d).join('/')); return Reflect.defineProperty(t, k, d); } }); o.__defineGetter__('a', function () {}); o.__defineSetter__('b', function () {}); return log.join();",
  "var log = []; var o = new Proxy({}, { defineProperty(t, k, d) { log.push(d.enumerable + ',' + d.configurable); return true; } }); o.__defineGetter__('a', function () {}); return log.join();",
  "var log = []; try { P_.__defineGetter__.call({}, { toString() { log.push('key'); return 'k'; } }, 1); } catch (e) { log.push(e.name); } return log.join();".replace("P_", P),
  "var log = []; try { " + P + ".__defineGetter__.call(null, { toString() { log.push('key'); return 'k'; } }, function () {}); } catch (e) { log.push(e.name); } return log.join();",
  "var log = []; try { " + P + ".__defineGetter__.call({}, { toString() { log.push('key'); return 'k'; } }, 2); } catch (e) { log.push(e.name); } return log.join();",
]) add(c);

// ---- __lookupGetter__ e __lookupSetter__
const setup = "try { Object.defineProperty(o, 'a', { get: function () { return 1; }, set: function (v) {}, configurable: true }); } catch (e) {}";
const view = "return [typeof r, typeof r === 'function' ? r.name + '/' + r.length : r];";
const lookupScenarios = [
  ["var r = CALL('a');", "plain"],
  [`${setup} var r = CALL('a');`, "own"],
  [`${setup} var c = C(o); var r = ${P}.METHOD.call(c, 'a');`, "inherited"],
  [`${setup} var c = C(C(o)); var r = ${P}.METHOD.call(c, 'a');`, "grand"],
  [`${setup} var c = C(o); try { Object.defineProperty(c, 'a', { value: 1 }); } catch (e) {} var r = ${P}.METHOD.call(c, 'a');`, "shadow"],
  ["try { Object.defineProperty(o, Symbol.iterator, { get: function () {}, configurable: true }); } catch (e) {} var r = CALL(Symbol.iterator);", "symbol"],
  ["try { Object.defineProperty(o, '1', { get: function () {}, configurable: true }); } catch (e) {} var r = CALL(1);", "index"],
  ["var r = CALL({ toString() { throw new RangeError('k'); } });", "keythrow"],
  ["var r = CALL();", "noargs"],
  ["var r = CALL('__proto__');", "proto"],
  ["var r = CALL('toString');", "builtin"],
  [`${setup} var r = o.METHOD('a');`, "direct"],
  ["var r = o.METHOD('a');", "directplain"],
];
for (const method of ["__lookupGetter__", "__lookupSetter__"]) {
  for (const recv of MAIN) {
    for (const [body] of lookupScenarios) {
      const code = body.replace(/METHOD/g, method).replace(/CALL\(/g, `${P}.${method}.call(o, `).replace(/call\(o, \)/, "call(o)");
      add(`var o = ${recv}; ${code} ${view}`);
    }
  }
}
add("var o = {}; o.__defineGetter__('a', function () { return 5; }); return [o.__lookupGetter__('a')(), o.__lookupSetter__('a')];");
add("var o = {}; o.__defineSetter__('a', function () {}); return [o.__lookupGetter__('a'), typeof o.__lookupSetter__('a')];");
add("var o = { get a() { return 1; } }; return [typeof o.__lookupGetter__('a'), o.__lookupGetter__('a').name];");
add("var o = { set a(v) {} }; return [typeof o.__lookupSetter__('a'), o.__lookupSetter__('a').name];");
add("var o = new Proxy({}, { getOwnPropertyDescriptor(t, k) { return { get: function () {}, configurable: true }; } }); return typeof o.__lookupGetter__('zz');");
add("var log = []; var o = new Proxy({}, { getOwnPropertyDescriptor(t, k) { log.push('gopd:' + String(k)); return undefined; }, getPrototypeOf(t) { log.push('gpo'); return null; } }); o.__lookupGetter__('q'); return log.join();");
add("var log = []; var o = new Proxy({}, { getOwnPropertyDescriptor(t, k) { log.push('gopd'); return Reflect.getOwnPropertyDescriptor(t, k); }, getPrototypeOf(t) { log.push('gpo'); return Object.prototype; } }); o.__lookupSetter__('q'); return log.join();");

// ---- __proto__ (getter e setter) em receptores exóticos
const GP = "var gp = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__');";
const protoValues = ["null", "{}", "1", "undefined", "Array.prototype", "function () {}", "'s'", "Object.prototype", "o", "C(o)", "new Proxy({}, {})", "Symbol()", "Function.prototype"];
for (const recv of ALL) {
  add(`${GP} var o = ${recv}; return N(gp.get.call(o));`);
  add(`var o = ${recv}; return N(o.__proto__);`);
  add(`var o = ${recv}; return N(Reflect.getPrototypeOf(Object(o)));`);
  for (const v of protoValues) {
    add(`${GP} var o = ${recv}; var v = ${v}; var r = gp.set.call(o, v); return [r, N(Object.getPrototypeOf(Object(o)))];`);
  }
}
for (const recv of MAIN) {
  for (const v of protoValues) {
    add(`var o = ${recv}; var v = ${v}; o.__proto__ = v; return N(Object.getPrototypeOf(Object(o)));`);
    add(`var o = ${recv}; var v = ${v}; (function () { 'use strict'; o.__proto__ = v; })(); return N(Object.getPrototypeOf(Object(o)));`);
  }
}
for (const c of [
  "var log = []; var o = new Proxy({}, { getPrototypeOf(t) { log.push('gpo'); return Array.prototype; } }); return [N(o.__proto__), log.join()];",
  "var log = []; var o = new Proxy({}, { setPrototypeOf(t, p) { log.push('spo'); return true; } }); o.__proto__ = null; return log.join();",
  "var log = []; var o = new Proxy({}, { set(t, k, v, r) { log.push('set:' + k); return true; } }); o.__proto__ = null; return log.join();",
  "var log = []; var o = new Proxy({}, { setPrototypeOf(t, p) { log.push('spo'); return false; } }); try { o.__proto__ = null; } catch (e) { log.push(e.name + ': ' + e.message); } return log.join();",
  "var o = Object.create(new Proxy({}, { set(t, k, v, r) { return Reflect.set(t, k, v, r); } })); o.__proto__ = null; return N(Object.getPrototypeOf(o));",
  "var o = {}; Object.defineProperty(o, '__proto__', { value: 1, writable: true, enumerable: true, configurable: true }); return [o.__proto__, N(Object.getPrototypeOf(o))];",
  "var o = {}; o['__proto__'] = null; return N(Object.getPrototypeOf(o));",
  "var o = { __proto__: null }; o.__proto__ = {}; return [Object.keys(o).join(), N(Object.getPrototypeOf(o))];",
  "var o = Object.create(null); Object.defineProperty(o, '__proto__', { value: 7 }); return o.__proto__;",
  "var a = {}; var b = Object.create(a); try { a.__proto__ = b; } catch (e) { return e.name + ': ' + e.message; }",
  "var a = {}; try { a.__proto__ = a; } catch (e) { return e.name + ': ' + e.message; }",
  "var a = Object.create(Object.create(Object.create(null))); var d = Object.getPrototypeOf(Object.getPrototypeOf(a)); try { d.__proto__ = a; } catch (e) { return e.name + ': ' + e.message; }",
  "try { Object.prototype.__proto__ = {}; } catch (e) { return e.name + ': ' + e.message; }",
  "try { Object.prototype.__proto__ = null; return 'ok'; } catch (e) { return e.name + ': ' + e.message; }",
  "try { Object.setPrototypeOf(Object.prototype, {}); } catch (e) { return e.name + ': ' + e.message; }",
  "try { globalThis.__proto__ = {}; return 'ok'; } catch (e) { return e.name + ': ' + e.message; }",
  "var f = function () {}; f.__proto__ = Array.prototype; return [Array.isArray(f), f instanceof Array];",
  "var a = []; a.__proto__ = Object.prototype; return [Array.isArray(a), a.length, typeof a.push];",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); return [d.get.name, d.set.name, d.get.length, d.set.length, d.enumerable, d.configurable, typeof d.get.prototype];",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { new d.get(); } catch (e) { return e.name + ': ' + e.message; }",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); return d.set.call({}) === undefined;",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); try { d.set.call({}); return Object.getPrototypeOf({}) === Object.prototype; } catch (e) { return e.name + ': ' + e.message; }",
  "var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); var o = {}; var r = d.set.call(o, null); return [r, N(Object.getPrototypeOf(o))];",
]) add(c);

// ---- Métodos HTML de String
const HTML = ["anchor", "big", "blink", "bold", "fixed", "fontcolor", "fontsize", "italics", "link", "small", "strike", "sub", "sup"];
const HTML_ATTR = ["anchor", "fontcolor", "fontsize", "link"];
const htmlRecv = [
  '"ab"', '""', "'a\"b\"'", '"<b>&amp;"', '"\\u00e9\\ud800"', "123", "-0", "true", "null", "undefined", "Symbol('s')", "10n",
  "{ toString() { return 'obj'; } }", "[1, 2]", "new String('bx')", "{ toString() { throw new RangeError('rec'); } }", "(function () {})",
  "new Proxy({}, {})", "Object.create(null)", "{ valueOf() { return 'v'; }, toString: undefined }",
];
const htmlArgs = [
  '"x"', '""', "'x\"y'", "'\"'", "'\"\"\"'", "'a&quot;b'", "undefined", "null", "1", "-0", "{ toString() { return 't\"t'; } }", "Symbol('s')",
  "{ toString() { throw new RangeError('arg'); } }", "10n", "'<>&\\''", "'a\\nb'",
];
for (const m of HTML) {
  for (const recv of htmlRecv) {
    add(`return String.prototype.${m}.call(${recv});`);
    if (HTML_ATTR.includes(m)) add(`return String.prototype.${m}.call(${recv}, 'q"q');`);
    else add(`return String.prototype.${m}.call(${recv}, 'ignored');`);
  }
  add(`var d = Object.getOwnPropertyDescriptor(String.prototype, '${m}'); return [d.writable, d.enumerable, d.configurable, d.value.length, d.value.name, typeof d.value.prototype];`);
  add(`try { new String.prototype.${m}(); } catch (e) { return e.name + ': ' + e.message; }`);
  add(`return ['abc'.${m}(), 'abc'.${m}().length, ''.${m}()];`);
  add(`return 'x'.${m}.call === Function.prototype.call;`);
  add(`return [String.prototype.${m}.call(new String('s')), typeof String.prototype.${m}.call(new String('s'))];`);
  add(`var log = []; var r = String.prototype.${m}.call({ toString() { log.push('recv'); return 'R'; } }, { toString() { log.push('arg'); return 'A'; } }); return [r, log.join()];`);
  add(`var log = []; try { String.prototype.${m}.call(null, { toString() { log.push('arg'); return 'A'; } }); } catch (e) { log.push(e.name + ': ' + e.message); } return log.join();`);
  add(`return 'a'.${m}().${m}();`);
  add(`return [String.prototype.${m}.call(1, 2), String.prototype.${m}.call(true, false)];`);
  if (HTML_ATTR.includes(m)) {
    for (const recv of ['"ab"', '""', "null", "undefined", "123", "{ toString() { return 'o'; } }"]) {
      for (const arg of htmlArgs) add(`return String.prototype.${m}.call(${recv}, ${arg});`);
    }
    add(`return ['ab'.${m}(), 'ab'.${m}(undefined), 'ab'.${m}(null), 'ab'.${m}('')];`);
    add(`return 'ab'.${m}('x', 'y', 'z');`);
    add(`return 'ab'.${m}('"a" "b"').length;`);
  }
}

// ---- escape e unescape
const hex = (n, w) => n.toString(16).toUpperCase().padStart(w, "0");
const escapeInputs = [];
for (let cp = 0; cp < 256; cp++) escapeInputs.push(String.fromCharCode(cp));
for (const cp of [0x100, 0x1ff, 0x7ff, 0x800, 0xd7ff, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xe000, 0xfeff, 0xfffd, 0xffff, 0x2028, 0x2029, 0x2014, 0x2013, 0x20ac]) escapeInputs.push(String.fromCharCode(cp));
escapeInputs.push("😀", "\ud83d", "\ude00\ud83d", "a\ud800b", "éé", "café", "A-Z a-z 0-9", "@*_+-./", "hello world", "50% off", "a=b&c=d", "?q=über#x", "~!'()", "\t\r\n", "\u0000\u0001", "x".repeat(40), "ÿĀ");
for (const s of escapeInputs) add(`return escape(${lit(s)});`);
const unescapeInputs = [];
for (let cp = 0; cp < 256; cp++) unescapeInputs.push("%" + hex(cp, 2));
for (let cp = 10; cp < 256; cp += 7) unescapeInputs.push("%" + hex(cp, 2).toLowerCase());
for (const cp of [0, 0x41, 0xff, 0x100, 0x20ac, 0xd7ff, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xfffe, 0xffff, 0x1234, 0xabcd]) {
  unescapeInputs.push("%u" + hex(cp, 4), "%u" + hex(cp, 4).toLowerCase());
}
unescapeInputs.push(
  "", "%", "%%", "%4", "%41%", "%zz", "%4z", "%z4", "%u", "%u0", "%u00", "%u004", "%u00G1", "%uG000", "%U0041", "%%41", "%u%41",
  "%uD83D%uDE00", "%uDE00%uD83D", "%u0041%u0042", "%41%u0042", "a%20b", "%2", "%20%20", "%E9", "%e9", "%C3%A9", "+", "%2b", "%25", "%2525", "%u0025", "%u002541",
  "abc", "%0", "%00", "%1G", "%G1", "%-1", "%+1", "% 1", "%u 041", "%u+041", "%u-041", "%0x41", "%u0x41", "100%", "%uffff", "%UFFFF", "%u12345",
);
for (const s of unescapeInputs) add(`return unescape(${lit(s)});`);
for (const v of ["null", "undefined", "1", "-0", "NaN", "true", "{}", "[1, 2]", "[]", "{ toString() { return '%41'; } }", "{ toString() { throw new RangeError('s'); } }", "Symbol('s')", "10n", "(function () {})", "new String('a b')", "Object.create(null)"]) {
  add(`return escape(${v});`);
  add(`return unescape(${v});`);
}
add("return escape();");
add("return unescape();");
add("return [escape.length, escape.name, unescape.length, unescape.name];");
add("var d = Object.getOwnPropertyDescriptor(globalThis, 'escape'); return [d.writable, d.enumerable, d.configurable];");
add("var d = Object.getOwnPropertyDescriptor(globalThis, 'unescape'); return [d.writable, d.enumerable, d.configurable];");
add("try { new escape(); } catch (e) { return e.name + ': ' + e.message; }");
add("try { new unescape(); } catch (e) { return e.name + ': ' + e.message; }");
add("return unescape(escape('\\u00e9\\u20ac\\ud83d\\ude00 x'));");
add("var s = ''; for (var i = 0; i < 300; i++) s += String.fromCharCode(i * 217 % 65536); return unescape(escape(s)) === s;");
add("return escape(escape('\\u00e9'));");
add("return unescape('%2541');");
add("return [typeof escape(''), typeof unescape('')];");
add("return escape('\\u00e9').length;");

// ---- Date legado
const times = [0, -1, 1, 1e12, -62198755200000, 946684800000, 915148800000, -2208988800000, 4102444800000, 8.64e15, -8.64e15, NaN, 1e11, -1e11, 253402300800000, -62167219200000, -62135596800000, 2e12, 86399999, -86400001, 1700000000000, 951782400000, 1078012800000, -2177452800000];
for (const t of times) {
  add(`var d = new Date(${t}); return [d.getYear(), d.getFullYear(), d.getFullYear() - 1900];`);
  add(`var d = new Date(${t}); return [d.toGMTString(), d.toUTCString(), d.toGMTString() === d.toUTCString()];`);
}
add("return Date.prototype.toGMTString === Date.prototype.toUTCString;");
add("return [Date.prototype.toGMTString.name, Date.prototype.toGMTString.length, Date.prototype.getYear.name, Date.prototype.getYear.length, Date.prototype.setYear.name, Date.prototype.setYear.length];");
add("var d = Object.getOwnPropertyDescriptor(Date.prototype, 'getYear'); return [d.writable, d.enumerable, d.configurable];");
add("var d = Object.getOwnPropertyDescriptor(Date.prototype, 'setYear'); return [d.writable, d.enumerable, d.configurable];");
add("var d = Object.getOwnPropertyDescriptor(Date.prototype, 'toGMTString'); return [d.writable, d.enumerable, d.configurable];");
const years = ["-1", "0", "1", "50", "69", "70", "99", "99.9", "100", "1899", "1900", "1999", "2000", "275760", "NaN", "undefined", "'70'", "'abc'", "null", "Infinity", "-Infinity", "{ valueOf() { return 85; } }", "10n", "Symbol()", "true", "-0.5", "0.5"];
for (const base of ["0", "1e12", "NaN", "-62135596800000", "946684800000", "8.64e15"]) {
  for (const y of years) add(`var d = new Date(${base}); var r = d.setYear(${y}); return [r, d.getTime(), d.getYear()];`);
  add(`var d = new Date(${base}); return d.setYear();`);
}
for (const recv of ALL) {
  add(`var o = ${recv}; return Date.prototype.getYear.call(o);`);
  add(`var o = ${recv}; return Date.prototype.setYear.call(o, 99);`);
  add(`var o = ${recv}; return Date.prototype.toGMTString.call(o);`);
}
add("var log = []; var d = new Date(0); d.setYear({ valueOf() { log.push('v'); return 80; } }); return [log.join(), d.getTime()];");
add("var d = new Date(NaN); d.setYear(2001); return [d.getTime(), d.getMonth(), d.getDate()];");
add("var d = new Date(2020, 1, 29); d.setYear(2021); return [d.getMonth(), d.getDate()];");
add("class D extends Date {}; var d = new D(0); return [d.getYear(), d.toGMTString()];");
add("return new Date(2000, 0, 1).getYear() + ':' + new Date(1999, 0, 1).getYear() + ':' + new Date(2100, 0, 1).getYear();");

// ---- RegExp legado
const STATICS = ["$1", "$2", "$3", "$4", "$5", "$6", "$7", "$8", "$9", "input", "$_", "lastMatch", "$&", "lastParen", "$+", "leftContext", "$`", "rightContext", "$'", "multiline", "$*"];
const RESET = "/(?:)/.exec('');";
for (const n of STATICS) {
  add(`var d = Object.getOwnPropertyDescriptor(RegExp, ${q(n)}); return d ? [typeof d.get, typeof d.set, d.enumerable, d.configurable, 'value' in d] : 'none';`);
  add(`${RESET} return RegExp[${q(n)}];`);
  add(`${RESET} RegExp[${q(n)}] = 'zz'; return RegExp[${q(n)}];`);
  add(`${RESET} (function () { 'use strict'; RegExp[${q(n)}] = 'zz'; })(); return RegExp[${q(n)}];`);
  add(`var d = Object.getOwnPropertyDescriptor(RegExp, ${q(n)}); if (!d) return 'none'; return [d.get && d.get.name, d.get && d.get.length, d.set && d.set.name, d.set && d.set.length];`);
}
const matches = [
  ["/(a)(b)/", "'xaby'"], ["/(a)|(b)/", "'xby'"], ["/(\\d+)-(\\d+)/", "'tel 12-345 ok'"], ["/b/", "'abc'"], ["/(?:x)(y)?/", "'x'"],
  ["/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)/", "'abcdefghij!'"], ["/\\w+/g", "'one two three'"], ["/(é)/", "'caf\\u00e9!'"], ["/$/", "'end'"], ["/^/m", "'a\\nb'"],
  ["/(?<n>q)(r)/", "'pqrs'"], ["/(a*)/", "'baaac'"], ["/x/", "'abc'"], ["/(\\ud83d\\ude00)/u", "'a\\ud83d\\ude00b'"],
];
for (const [re, input] of matches) {
  const all = STATICS.map(n => `RegExp[${q(n)}]`).join(", ");
  add(`${re}.exec(${input}); return [${all}];`);
  add(`${re}.test(${input}); return [${all}];`);
  add(`${input}.replace(${re}, 'z'); return [${all}];`);
  add(`${input}.match(${re}); return [${all}];`);
  add(`${input}.search(${re}); return [${all}];`);
  add(`${input}.split(${re}); return [${all}];`);
  add(`${re}.exec(${input}); ${re}.exec('no-match-here-zzz\\u0000'); return [${all}];`);
  add(`var re = ${re}; re.exec(${input}); RegExp.input = 'forced'; return [RegExp.input, RegExp.$_, RegExp.lastMatch, RegExp.$1];`);
}
for (const v of ["true", "false", "1", "0", "''", "'x'", "null", "undefined", "{}", "Symbol.iterator"]) {
  add(`${RESET} try { RegExp.multiline = ${v}; return [RegExp.multiline, RegExp.$*]; } catch (e) { return e.name + ': ' + e.message; }`);
  add(`${RESET} try { RegExp.input = ${v}; return [RegExp.input, RegExp.$_]; } catch (e) { return e.name + ': ' + e.message; }`);
  add(`${RESET} try { RegExp.$_ = ${v}; return [RegExp.input, RegExp.$_]; } catch (e) { return e.name + ': ' + e.message; }`);
  add(`${RESET} try { RegExp.$* = ${v}; return [RegExp.multiline]; } catch (e) { return e.name + ': ' + e.message; }`);
}
add("/(a)/.exec('a'); RegExp.input = 'q'; /(b)/.exec('b'); return [RegExp.input, RegExp.lastMatch];");
add("var A = RegExp; /(a)/.exec('a'); return [A.$1, RegExp.$1, Object.keys(RegExp).join()];");
add("/(a)/.exec('a'); return Object.getOwnPropertyNames(RegExp).filter(function (n) { return n.charAt(0) === '$' || /^(input|lastMatch|lastParen|leftContext|rightContext|multiline)$/.test(n); }).sort().join();");
// compile
const compileRecv = ["/a/g", "/a/y", "new RegExp('a', 'gimsuy')", "(function () { class R2 extends RegExp {} return new R2('a'); })()", "{}", "null", "undefined", "1", "Object.create(RegExp.prototype)", "Object.freeze(/a/)", "new Proxy(/a/, {})", "(function () { var r = /a/g; r.lastIndex = 3; return r; })()"];
const compileArgs = ["", "'b'", "'b', 'i'", "'b', undefined", "/c/g", "/c/g, 'i'", "undefined", "null", "'('", "'a', 'zz'", "{ toString() { return 'ts'; } }", "1", "'b', 'gg'", "/c/g, undefined", "'[', 'u'", "'\\\\u{1F600}', 'u'", "'a', 'd'", "'a', 'v'"];
for (const recv of compileRecv) {
  for (const a of compileArgs) add(`var r = ${recv}; var s = RegExp.prototype.compile.call(r${a ? ", " + a : ""}); return [s === r, r.source, r.flags, r.lastIndex];`);
}
add("return [RegExp.prototype.compile.length, RegExp.prototype.compile.name];");
add("var d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'compile'); return [d.writable, d.enumerable, d.configurable];");
add("var r = /a/g; r.lastIndex = 5; r.compile('a', 'g'); return r.lastIndex;");
add("var r = /a/g; Object.defineProperty(r, 'lastIndex', { writable: false, value: 1 }); try { r.compile('b'); } catch (e) { return e.name + ': ' + e.message; }");
add("var r = /a/; return [r.compile('b') === r, r.test('b'), r.test('a')];");
add("var r = /a/g; r.compile(/b/); return r.global;");
add("try { /a/.compile(/b/, 'g'); } catch (e) { return e.name + ': ' + e.message; }");
add("class R3 extends RegExp {}; var r = new R3('a'); try { r.compile('b'); return r.source; } catch (e) { return e.name + ': ' + e.message; }");

// ---- Function.prototype.caller e arguments
const funcs = [
  ["sloppy", "function () {}"], ["strict", "function () { 'use strict'; }"], ["arrow", "() => 1"], ["class", "class {}"],
  ["bound", "(function () {}).bind()"], ["async", "async function () {}"], ["generator", "function* () {}"], ["method", "({ m() {} }).m"],
  ["builtin", "Math.max"], ["protofn", "Function.prototype"], ["newfn", "new Function('return 1')"], ["strictbound", "(function () { 'use strict'; }).bind()"],
  ["asyncgen", "async function* () {}"], ["getter", "Object.getOwnPropertyDescriptor({ get g() { return 1; } }, 'g').get"], ["proxyfn", "new Proxy(function () {}, {})"],
];
for (const [, f] of funcs) {
  add(`var f = ${f}; return f.caller;`);
  add(`var f = ${f}; return f.arguments;`);
  add(`var f = ${f}; return [f.hasOwnProperty('caller'), f.hasOwnProperty('arguments'), 'caller' in f, 'arguments' in f];`);
  add(`var f = ${f}; var d = Object.getOwnPropertyDescriptor(f, 'caller'); return d ? [typeof d.get, typeof d.set, 'value' in d, d.value, d.writable, d.enumerable, d.configurable] : 'none';`);
  add(`var f = ${f}; var d = Object.getOwnPropertyDescriptor(f, 'arguments'); return d ? [typeof d.get, typeof d.set, 'value' in d, d.value, d.writable, d.enumerable, d.configurable] : 'none';`);
  add(`var f = ${f}; f.caller = 1; return f.hasOwnProperty('caller');`);
  add(`var f = ${f}; (function () { 'use strict'; f.caller = 1; })(); return f.hasOwnProperty('caller');`);
  add(`var f = ${f}; (function () { 'use strict'; f.arguments = 1; })();`);
  add(`var f = ${f}; return delete f.caller;`);
  add(`var f = ${f}; (function () { 'use strict'; return delete f.arguments; })();`);
  add(`var f = ${f}; return Object.getOwnPropertyNames(f).join();`);
  add(`var f = ${f}; return Reflect.ownKeys(f).map(String).join();`);
  add(`var f = ${f}; try { return Object.defineProperty(f, 'caller', { value: 7 }).caller; } catch (e) { return e.name + ': ' + e.message; }`);
  add(`var f = ${f}; try { return Reflect.set(f, 'caller', 5); } catch (e) { return e.name + ': ' + e.message; }`);
}
const FPD = "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller');";
for (const c of [
  `${FPD} return d ? [typeof d.get, typeof d.set, d.get === d.set, d.get.name, d.get.length, d.enumerable, d.configurable] : 'none';`,
  "var d = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); return d ? [typeof d.get, typeof d.set, d.get === d.set, d.get.name, d.get.length, d.enumerable, d.configurable] : 'none';",
  "var a = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'), b = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); return a.get === b.get;",
  `${FPD} return d.get.call(function () { 'use strict'; });`,
  `${FPD} return d.get.call(function () {});`,
  `${FPD} return d.get.call({});`,
  `${FPD} return d.get.call(undefined);`,
  `${FPD} return d.get.call(() => 1);`,
  `${FPD} return d.set.call(function () {}, 1);`,
  `${FPD} return d.set.call(function () { 'use strict'; }, 1);`,
  `${FPD} try { new d.get(); } catch (e) { return e.name + ': ' + e.message; }`,
  "function g() { return g.caller; } function h() { return g(); } return h === h && h() === h;",
  "function g() { return g.caller === null; } return g();",
  "function g() { return g.arguments.length; } return g(1, 2, 3);",
  "function g() { return typeof g.arguments; } return g();",
  "function g(a) { a = 9; return g.arguments[0]; } return g(1);",
  "function g() { return g.arguments === arguments; } return g();",
  "function g() { 'use strict'; return g.caller; } return g();",
  "function g() { 'use strict'; return g.arguments; } return g();",
  "return (function f() { 'use strict'; return f.caller; })();",
  "return (function f() { return f.caller; })();",
  "function outer() { 'use strict'; return inner(); } function inner() { return inner.caller; } return outer();",
  "function sl() { return st(); } function st() { 'use strict'; return st.caller; } return sl();",
  "var g = function () { return g.caller; }; return (function () { return g(); })() !== null;",
  "return (function () { return arguments.callee === undefined; })();",
  "return (function () { 'use strict'; return arguments.callee; })();",
  "return (function () { 'use strict'; var d = Object.getOwnPropertyDescriptor(arguments, 'callee'); return [typeof d.get, d.get === d.set, d.enumerable, d.configurable]; })();",
  "return (function () { var d = Object.getOwnPropertyDescriptor(arguments, 'callee'); return [typeof d.value, d.writable, d.enumerable, d.configurable]; })();",
  "return (function () { 'use strict'; return arguments.caller; })();",
  "return (function () { 'use strict'; return Object.getOwnPropertyNames(arguments).join(); })();",
  "return (function (a) { return Object.getOwnPropertyNames(arguments).join(); })(1);",
  "return (function () { 'use strict'; arguments.callee = 1; })();",
  "return (function () { 'use strict'; return Object.getOwnPropertyDescriptor(arguments, 'callee').get.call(1); })();",
  "return (() => { try { return arguments.callee; } catch (e) { return e.name; } })();",
  "return [Function.prototype.hasOwnProperty('caller'), Function.prototype.hasOwnProperty('arguments'), Object.prototype.hasOwnProperty.call(Function.prototype, 'caller')];",
  "return [typeof Function.prototype.caller === 'undefined'];",
  "try { return Function.prototype.caller; } catch (e) { return e.name + ': ' + e.message; }",
  "try { return Function.prototype.arguments; } catch (e) { return e.name + ': ' + e.message; }",
  "try { Function.prototype.caller = 1; return 'ok'; } catch (e) { return e.name + ': ' + e.message; }",
  "'use strict'; try { Function.prototype.arguments = 1; return 'ok'; } catch (e) { return e.name + ': ' + e.message; }",
  "return (class { static m() { return typeof this.caller; } }).m();",
]) add(c);

// ---- hasOwnProperty, isPrototypeOf, propertyIsEnumerable, toLocaleString, valueOf, toString
const keys = ["'a'", "'length'", "'0'", "0", "Symbol.iterator", "'toString'", "'__proto__'", "undefined"];
for (const m of ["hasOwnProperty", "propertyIsEnumerable"]) {
  for (const recv of ALL) {
    for (const k of keys) add(`var o = ${recv}; return ${P}.${m}.call(o, ${k});`);
    add(`var o = ${recv}; return ${P}.${m}.call(o);`);
    add(`var log = []; var o = ${recv}; var r = ${P}.${m}.call(o, { toString() { log.push('key'); return 'a'; } }); return [r, log.join()];`);
    add(`var o = ${recv}; return ${P}.${m}.call(o, { toString() { throw new RangeError('k'); } });`);
  }
}
for (const m of ["hasOwnProperty", "propertyIsEnumerable"]) {
  for (const recv of ["undefined", "null", "'abc'", "10n", "Symbol('s')", "true", "5", "Object.create(null)", "new Proxy({}, {})"]) {
    add(`var o = ${recv}; return o.${m}('a');`);
    add(`var o = ${recv}; return o.${m}('length');`);
  }
}
const protoArgs = ["undefined", "null", "1", "'s'", "{}", "[]", "C(o)", "C(C(o))", "o", "function () {}", "Symbol()", "new Proxy(C(o), {})", "Object.create(null)", "new Proxy({}, { getPrototypeOf() { return o; } })", "10n", "new String('x')"];
for (const recv of ALL) {
  for (const a of protoArgs) add(`var o = ${recv}; return ${P}.isPrototypeOf.call(o, ${a});`);
  add(`var o = ${recv}; return ${P}.isPrototypeOf.call(o);`);
  add(`var o = ${recv}; return ${P}.isPrototypeOf.call(o, { __proto__: Object.prototype });`);
}
add("return [Object.prototype.isPrototypeOf.call(Object.prototype, {}), Object.prototype.isPrototypeOf.call(Object.prototype, Object.create(null)), Object.prototype.isPrototypeOf.call(Object.prototype, Object.prototype)];");
add("return [Array.prototype.isPrototypeOf([]), Function.prototype.isPrototypeOf(function () {}), Function.prototype.isPrototypeOf(Object), Object.prototype.isPrototypeOf(Object.prototype)];");
add("try { return Object.prototype.isPrototypeOf.call(null, 1); } catch (e) { return e.name + ': ' + e.message; }");
add("try { return Object.prototype.isPrototypeOf.call(undefined, {}); } catch (e) { return e.name + ': ' + e.message; }");
add("var log = []; var v = new Proxy({}, { getPrototypeOf() { log.push('gpo'); return null; } }); try { Object.prototype.isPrototypeOf.call(null, v); } catch (e) { log.push(e.name); } return log.join();");
add("var log = []; var v = new Proxy({}, { getPrototypeOf() { log.push('gpo'); return null; } }); Object.prototype.isPrototypeOf.call({}, v); return log.join();");
for (const m of ["toLocaleString", "valueOf", "toString"]) {
  for (const recv of ALL) {
    add(`var o = ${recv}; var r = ${P}.${m}.call(o); return typeof r === 'object' ? 'object' : r;`);
    add(`var o = ${recv}; var r = o.${m}(); return typeof r === 'object' ? 'object' : r;`);
    add(`var o = ${recv}; var r = ${P}.${m}.call(o, 1, 2); return [typeof r, typeof r === 'object' ? 'object' : r];`);
  }
  add(`var r = ${P}.${m}.call({ toString() { return 'custom'; }, valueOf() { return 7; } }); return typeof r === 'object' ? 'object' : r;`);
  add(`var d = Object.getOwnPropertyDescriptor(Object.prototype, '${m}'); return [d.writable, d.enumerable, d.configurable, d.value.length, d.value.name];`);
  add(`try { new ${P}.${m}(); } catch (e) { return e.name + ': ' + e.message; }`);
}
add("var log = []; var o = { toString() { log.push('ts'); return 'x'; }, valueOf() { log.push('vo'); return 1; } }; var r = Object.prototype.toLocaleString.call(o); return [r, log.join()];");
add("var o = { toString() { return 'tls'; } }; return Object.prototype.toLocaleString.call(o);");
add("var o = { toString: 5 }; try { return Object.prototype.toLocaleString.call(o); } catch (e) { return e.name + ': ' + e.message; }");
add("var o = { toString: undefined }; try { return Object.prototype.toLocaleString.call(o); } catch (e) { return e.name + ': ' + e.message; }");
add("try { return Object.prototype.toLocaleString.call(Object.create(null)); } catch (e) { return e.name + ': ' + e.message; }");
add("var o = { toString() { return this === o ? 'same' : 'other'; } }; return Object.prototype.toLocaleString.call(o);");
add("return Object.prototype.toLocaleString.call(1);");
add("Number.prototype.toString = function () { return 'patched'; }; return Object.prototype.toLocaleString.call(1);");
add("return Object.prototype.toString.call({ [Symbol.toStringTag]: 'X' });");
add("return Object.prototype.toString.call({ [Symbol.toStringTag]: 1 });");
add("return Object.prototype.toString.call(new Proxy([], {}));");
add("return Object.prototype.toString.call(new Proxy(function () {}, {}));");
add("return Object.prototype.toString.call(new Proxy({}, { get(t, k) { return k === Symbol.toStringTag ? 'Prox' : undefined; } }));");
add("try { return Object.prototype.toString.call(" + REVOKED + "); } catch (e) { return e.name + ': ' + e.message; }");
add("return Object.prototype.toString.call(Object.assign(function () {}, { [Symbol.toStringTag]: 'F' }));");
for (const n of ["hasOwnProperty", "isPrototypeOf", "propertyIsEnumerable", "toLocaleString", "valueOf", "toString", "__defineGetter__", "__defineSetter__", "__lookupGetter__", "__lookupSetter__"]) {
  add(`var d = Object.getOwnPropertyDescriptor(Object.prototype, '${n}'); return d ? [d.writable, d.enumerable, d.configurable, d.value.length, d.value.name, typeof d.value.prototype] : 'none';`);
  add(`try { new Object.prototype.${n}(); } catch (e) { return e.name + ': ' + e.message; }`);
}
add("return Object.getOwnPropertyNames(Object.prototype).sort().join();");

// ---- Execução
const known = new Set();
const sha = s => crypto.createHash("sha1").update(s).digest("hex");
for (const source of knownPrograms("annexb_methods_bun.tsv", f => f !== `${NAME}_bun.tsv`)) known.add(sha(source));

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "annexb-methods-golden-"));
const runner = path.join(dir, "runner.js");
// `vm.runInThisContext` roda o programa como script sloppy no JSC puro, sem o transpilador do bun.
fs.writeFileSync(
  runner,
  "const vm = require('node:vm'); const fs = require('node:fs');\n" +
    "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') });\n" +
    "try { vm.runInThisContext(fs.readFileSync(process.argv[2], 'utf8'), { filename: 'annexb_methods_case.js' }) } catch (e) {}\n",
);
const prefix = dir + "/";

function runOne(index, source) {
  const file = path.join(dir, `case_${index}.js`);
  fs.writeFileSync(file, source);
  return new Promise(resolve => {
    const child = spawn(process.execPath, [runner, file], { cwd: dir, env: { ...process.env, TZ: "UTC" }, stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    child.stdout.on("data", chunk => (out += chunk));
    const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
    child.on("close", () => {
      clearTimeout(timer);
      fs.rmSync(file, { force: true });
      resolve(out);
    });
  });
}

(async () => {
  const unique = [];
  const seen = new Set();
  let known_dropped = 0;
  for (const source of programs) {
    if (seen.has(source)) continue;
    seen.add(source);
    if (usesHostApi(source) || source.search(BAD) >= 0) { process.stderr.write("filtrado: " + q(source.slice(PRELUDE.length, PRELUDE.length + 120)) + "\n"); continue; }
    if (known.has(sha(source))) { known_dropped++; continue; }
    unique.push(source);
  }
  const results = new Array(unique.length);
  let next = 0;
  async function worker() {
    while (next < unique.length) {
      const i = next++;
      results[i] = await runOne(i, unique[i]);
    }
  }
  await Promise.all(Array.from({ length: 8 }, worker));
  const rows = [];
  let dropped = 0;
  unique.forEach((source, i) => {
    const marked = (results[i] || "").split("\n").find(line => line.startsWith("\u0001"));
    const tag = q(source.slice(PRELUDE.length, PRELUDE.length + 160));
    if (!marked) { dropped++; process.stderr.write("sem resultado para: " + tag + "\n"); return; }
    const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
    if (result === "<undefined>") { dropped++; process.stderr.write("R indefinido: " + tag + "\n"); return; }
    if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result) || result.search(BAD) >= 0) { dropped++; process.stderr.write("caminho ou travessão no resultado: " + tag + "\n"); return; }
    rows.push({ source, result });
  });
  fs.writeFileSync(path.join(GOLDEN_DIR, `${NAME}_bun.tsv`), emitFactored(NAME, rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, já em outros goldens ${known_dropped}\n`);
  fs.rmSync(dir, { recursive: true, force: true });
})();
