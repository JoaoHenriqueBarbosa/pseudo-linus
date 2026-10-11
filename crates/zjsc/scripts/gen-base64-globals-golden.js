// Gera tests/golden/base64_globals_bun.tsv: `atob` e `btoa` do global medidos no bun 1.4.2 (descritor, `length`,
// `name`, `toString()`, ordem de chaves, entradas válidas e inválidas, espaço, preenchimento, coerções, erro de
// caractere inválido, argumento ausente).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda no bun por
// indirect eval no mesmo processo (funções puras, sem estado). O erro de caractere inválido é um `DOMException`
// (src/runtime/js_dom_exception.rs): entram `name`, `message`, `code`, `constructor.name`, `instanceof`,
// `Object.prototype.toString` e o protótipo; as propriedades próprias da exceção lançada por função nativa
// (`line`, `column`, `stack`) entram sem `sourceURL` nem a cauda da `stack`, que dependem do hospedeiro).
// Uso: bun scripts/gen-base64-globals-golden.js > tests/golden/base64_globals_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

// Descritores e forma.
for (const n of ["atob", "btoa"]) {
  expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${n}'))`);
  expr(`${n}.length`);
  expr(`${n}.name`);
  expr(`${n}.toString()`);
  expr(`Object.getOwnPropertyNames(${n})`);
  expr(`'prototype' in ${n}`);
  expr(`Object.getPrototypeOf(${n}) === Function.prototype`);
  expr(`Object.prototype.hasOwnProperty.call(globalThis, '${n}')`);
  expr(`new ${n}('aGk=')`);
}
// Ordem entre os globais do host.
expr("Object.getOwnPropertyNames(globalThis).indexOf('btoa') - Object.getOwnPropertyNames(globalThis).indexOf('atob')");

// atob: entradas.
const atobInputs = [
  "aGVsbG8=", "aGVsbG8", " aGVs bG8= ", "aGVsbG8==", "aGVsb", "a", "aGVsbG8!", "", "\\n\\t\\f\\r aGk=", "\\v aGk=", "aG=k",
  "aGk==", "aGk=x", "YQ", "YQ=", "YQ==", "YQ===", "-_", "+/+/", "\\u0100", "Zm9v\\u00a0", "====", "YR==", "//8=",
  "Zm9v", "Zm9vYg==", "Zm9vYmE=", "Zm9vYmFy", "Zg", "Zm8", "Zm8=", "Zm8==", "=", "==", "A===", "AAAA====", "AA==AA==",
  "Zm9v YmFy", "Zm9v\\nYmFy", "Zm\\u00009v", "\\ufeffZm9v", "Z m 9 v", "Zm9v=", "Zm9=", "Z===", "AAA=", "AAA", "AA=", "AA",
  "/+8=", "/-8=", "_-8=", "!!!!", "Zm9vYg=", "Zm9vYg==\\n", "  ", "\\u00ff\\u00ff", "ÿ", "Zm9v\\u2028", "Zm9v\\x0b",
];
for (const s of atobInputs) expr(`atob('${s}')`);
// atob: coerções e argumentos.
for (const c of ["", "null", "undefined", "{ toString() { return 'aGk=' } }", "123", "Symbol()", "1n", "['aGk=']", "true", "false",
  "NaN", "{ toString() { throw new RangeError('x') } }", "{ valueOf() { return 'aGk=' } }", "{ [Symbol.toPrimitive]() { return 'aGk=' } }",
  "'aGk=', 'ignorado'", "new String('aGk=')", "[]", "{}", "new Uint8Array([97])"]) expr(`atob(${c})`);
expr("atob.call(null, 'aGk=')");
expr("atob.call(undefined)");
expr("atob.apply(null, [])");
expr("atob.apply(null, [undefined])");

// btoa: entradas.
const btoaInputs = [
  "hello", "", "\\u00ff\\u0080\\u0000", "\\u0100", "a\\u20acb", "\\ud800", "\\udc00", "\\ud83d\\ude00", "f", "fo", "foo", "foob", "fooba", "foobar",
  "\\u00ff", "\\u0000", "\\u00ff\\u00ff\\u00ff", "\\u00fe\\u00ff", "~~~", "???", ">>>", "a b", "\\n", "\\u0080", "\\u00a0\\u00a0\\u00a0",
];
for (const s of btoaInputs) expr(`btoa('${s}')`);
for (const c of ["", "null", "undefined", "{ toString() { return 'hi' } }", "123", "Symbol()", "['a','b']", "1n", "true", "-0", "1e21", "NaN",
  "{ toString() { throw new RangeError('x') } }", "'a', 'b'", "new String('hi')", "[]", "{}", "{ toString() { return '\\u0100' } }", "0.5"]) expr(`btoa(${c})`);
expr("btoa.call(null, 'hi')");
expr("btoa.apply(null, [])");

// O erro de caractere inválido é um DOMException de verdade.
for (const call of ["atob('!')", "atob('a')", "atob('\\u0100')", "atob('Zm9v=')", "btoa('\\u0100')", "btoa('a\\u20acb')", "btoa('\\ud800')"]) {
  expr(`(function(){ try { ${call} } catch (e) { return [e.constructor.name, e instanceof DOMException, e instanceof Error, Object.prototype.toString.call(e), String(e), e.code, e.name, e.message, e.INVALID_CHARACTER_ERR, Object.getPrototypeOf(e) === DOMException.prototype, e.constructor === DOMException, typeof e.toString, Error.prototype.isPrototypeOf(e)] } return 'sem erro' })()`);
}
expr("(function(){ try { atob('!') } catch (e) { return [Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call(e), Object.getOwnPropertyDescriptor(DOMException.prototype, 'name').get.call(e), Object.getOwnPropertyDescriptor(DOMException.prototype, 'message').get.call(e)] } })()");
expr("(function(){ try { atob('!') } catch (e) { return e.code === DOMException.INVALID_CHARACTER_ERR } })()");

// As propriedades próprias da exceção lançada por `atob`/`btoa` (`line`, `column`, `stack`; `sourceURL` e a cauda da
// `stack` dependem do hospedeiro, eval do bun contra script nomeado do porte, e saem da comparação).
const THROWN =
  "var T = function (e) { var f = function (n) { return n !== 'sourceURL' }; var d = function (n) { var x = Object.getOwnPropertyDescriptor(e, n); return x && [typeof x.value, x.writable, x.enumerable, x.configurable] }; " +
  "return [Object.getOwnPropertyNames(e).filter(f), Object.keys(e).filter(f), e.line, e.column, typeof e.stack, String(e.stack).split('\\n')[0], " +
  "JSON.stringify(e, function (k, v) { return k === 'sourceURL' ? undefined : v }), d('line'), d('column'), d('stack')] };\n";
const thrown = (call) => programs.push(HELPER + THROWN + `try { ${call}; R = 'sem erro' } catch (e) { R = S(T(e)) }`);
for (const call of ["atob('!')", "atob('a')", "atob('Zm9v=')", "atob(' !')", "btoa('\\u0100')", "btoa('a\\u20acb')", "btoa('\\ud800')",
  "\n\n  atob('!')", "(function g() {\n    btoa('\\u0100')\n  })()", "[1].map(btoa.bind(null, '\\u0100'))", "[1].map(atob)", "Reflect.apply(atob, null, ['!'])",
  "atob.call(null, '!')", "(0, btoa)('\\u0100')", "new Function('atob(\"!\")')()", "(function () { 'use strict'; atob('!') })()"]) thrown(call);
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = 0; try { atob('!') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = 1; try { btoa('\\u0100') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = undefined; try { atob('!') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = -3; try { atob('!') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = 2.5; try { atob('!') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "try { atob('!') } catch (e) { e.line = 7; e.stack = 's'; R = S([e.line, e.stack, Object.keys(e).filter(function (n) { return n !== 'sourceURL' })]) }");

// Ida e volta.
expr("(function(){ var s = ''; for (var i = 0; i < 256; i++) s += String.fromCharCode(i); return [btoa(s), atob(btoa(s)) === s] })()");
expr("(function(){ var s = ''; for (var i = 0; i < 300; i++) s += String.fromCharCode(i % 256); return atob(btoa(s)) === s })()");
expr("atob(btoa('x'.repeat(1000))).length");
expr("btoa('x'.repeat(1000)).length");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
