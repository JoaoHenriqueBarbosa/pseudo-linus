// Gera tests/golden/dom_exception_bun.tsv: o global `DOMException` medido no bun 1.4.2 (descritor, `length`, `name`,
// `toString()`, chaves próprias do construtor e do protótipo, as 25 constantes legadas, acessores `code`/`name`/
// `message`, `@@toStringTag`, protótipo, instâncias, argumentos padrão e coerções, `cause`, tabela de códigos por
// nome, chamada sem `new`, `this` inválido nos getters, subclasse e `Reflect.construct`).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda no bun por
// indirect eval no mesmo processo (funções puras, sem estado; classes só dentro de função).
// Também cobre a atribuição aos acessores sem setter (ignorada em sloppy, `TypeError` em strict) e as propriedades
// próprias (`line`, `column`, `stack`) de um `DOMException` lançado por função nativa; `sourceURL` e a cauda da
// `stack` dependem do hospedeiro (eval do bun contra script nomeado do porte) e saem da comparação.
// Uso: bun scripts/gen-dom-exception-golden.js > tests/golden/dom_exception_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

const LEGACY_NAMES = [
  "IndexSizeError", "DOMStringSizeError", "HierarchyRequestError", "WrongDocumentError", "InvalidCharacterError",
  "NoDataAllowedError", "NoModificationAllowedError", "NotFoundError", "NotSupportedError", "InUseAttributeError",
  "InvalidStateError", "SyntaxError", "InvalidModificationError", "NamespaceError", "InvalidAccessError",
  "ValidationError", "TypeMismatchError", "SecurityError", "NetworkError", "AbortError", "URLMismatchError",
  "QuotaExceededError", "TimeoutError", "InvalidNodeTypeError", "DataCloneError", "EncodingError",
  "NotReadableError", "UnknownError", "ConstraintError", "DataError", "TransactionInactiveError", "ReadOnlyError",
  "VersionError", "OperationError", "NotAllowedError", "OptOutError", "abortError", "ABORTERROR", "Error", "", " AbortError",
];

// Global e construtor.
expr("(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, 'DOMException'))");
expr("typeof DOMException");
expr("DOMException.length");
expr("DOMException.name");
expr("DOMException.toString()");
expr("Function.prototype.toString.call(DOMException)");
expr("Object.getOwnPropertyNames(DOMException)");
expr("Reflect.ownKeys(DOMException).length");
expr("Object.getPrototypeOf(DOMException) === Function.prototype");
expr("Object.prototype.hasOwnProperty.call(globalThis, 'DOMException')");
expr("(function(){ var k = Object.getOwnPropertyNames(globalThis); return [k.indexOf('DOMException') > k.indexOf('btoa'), k.indexOf('DOMException') > k.indexOf('atob'), k.indexOf('DOMException') > k.indexOf('ResolveMessage'), k.indexOf('DOMException') > k.indexOf('BuildMessage')] })()");
for (const n of ["length", "name", "prototype", "INDEX_SIZE_ERR", "DATA_CLONE_ERR", "DOMSTRING_SIZE_ERR", "NOPE"]) {
  expr(`(function(d){ return d ? [typeof d.value, d.value === DOMException.prototype ? 'proto' : d.value, d.writable, d.enumerable, d.configurable, 'get' in d] : null })(Object.getOwnPropertyDescriptor(DOMException, '${n}'))`);
}
expr("Object.getOwnPropertyNames(DOMException).slice(3).map(function (n) { return n + '=' + DOMException[n] })");
expr("Object.keys(DOMException)");
expr("DOMException.INDEX_SIZE_ERR + DOMException.DATA_CLONE_ERR");
expr("Object.isExtensible(DOMException)");
expr("Object.isFrozen(DOMException.prototype)");
expr("'prototype' in DOMException");

// Protótipo.
expr("Reflect.ownKeys(DOMException.prototype).map(String)");
expr("Object.keys(DOMException.prototype)");
expr("Object.getPrototypeOf(DOMException.prototype) === Error.prototype");
expr("DOMException.prototype instanceof Error");
expr("DOMException.prototype.constructor === DOMException");
expr("(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable] })(Object.getOwnPropertyDescriptor(DOMException.prototype, 'constructor'))");
for (const n of ["code", "name", "message"]) {
  expr(`(function(d){ return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length, d.get.toString(), 'value' in d] })(Object.getOwnPropertyDescriptor(DOMException.prototype, '${n}'))`);
  expr(`Object.getOwnPropertyNames(Object.getOwnPropertyDescriptor(DOMException.prototype, '${n}').get)`);
  expr(`'prototype' in Object.getOwnPropertyDescriptor(DOMException.prototype, '${n}').get`);
}
expr("(function(d){ return [d.value, d.writable, d.enumerable, d.configurable] })(Object.getOwnPropertyDescriptor(DOMException.prototype, Symbol.toStringTag))");
expr("Object.prototype.toString.call(DOMException.prototype)");
expr("Object.prototype.toString.call(DOMException)");
expr("DOMException.prototype.hasOwnProperty('stack')");
expr("DOMException.prototype.hasOwnProperty('cause')");
expr("Object.getOwnPropertyNames(DOMException.prototype).slice(4).map(function (n) { return n + '=' + DOMException.prototype[n] })");
expr("DOMException.prototype.toString === Error.prototype.toString");
expr("Object.isExtensible(DOMException.prototype)");

// Instância.
const inst = "new DOMException('m', 'InvalidCharacterError')";
expr(`(function(e){ return [e.code, e.name, e.message, String(e), Object.prototype.toString.call(e), Object.getOwnPropertyNames(e), e instanceof Error, e instanceof DOMException, Object.keys(e), JSON.stringify(e), typeof e.stack, e.constructor.name, e.constructor === DOMException, Object.getPrototypeOf(e) === DOMException.prototype, Object.isExtensible(e), Reflect.ownKeys(e).length] })(${inst})`);
expr(`(function(e){ return [Object.prototype.hasOwnProperty.call(e, 'name'), Object.prototype.hasOwnProperty.call(e, 'message'), Object.prototype.hasOwnProperty.call(e, 'code'), 'name' in e, 'cause' in e] })(${inst})`);
expr(`(function(e){ var s = []; for (var k in e) s.push(k); return s.length })(${inst})`);
// Descritores completos do construtor e do protótipo (todas as chaves próprias, na ordem do bun), em JSON.
programs.push(HELPER +
  "var F = function (f) { return typeof f === 'function' ? [f.name, f.length, f.toString()] : f === undefined ? 'undefined' : typeof f };" +
  " var V = function (v) { return typeof v === 'function' ? F(v) : typeof v === 'object' && v !== null ? 'object' : typeof v === 'symbol' ? String(v) : v };" +
  " var X = function (o) { return Reflect.ownKeys(o).map(function (k) { var d = Object.getOwnPropertyDescriptor(o, k); return [String(k), 'value' in d ? V(d.value) : 'accessor', d.writable, d.enumerable, d.configurable, F(d.get), F(d.set)] }) };\n" +
  "try { R = JSON.stringify([X(DOMException), X(DOMException.prototype)]) } catch (e) { R = E(e) }");
expr(`(function(e){ e.extra = 1; return [e.extra, Object.keys(e)] })(${inst})`);
expr(`Error.prototype.toString.call(${inst})`);
expr(`(function(e){ return [e.name, e.message, e.code, String(e)] })(new DOMException('', 'AbortError'))`);
expr(`(function(e){ return [e.name, e.message, e.code, String(e)] })(new DOMException('m', ''))`);
expr("(function(){ try { throw new DOMException('m', 'AbortError') } catch (e) { return [e.name, e.code, e instanceof DOMException, typeof e.stack] } })()");

// Argumentos padrão e coerções.
const argCases = [
  "", "undefined", "undefined, undefined", "null, null", "123, 456", "'a', {}", "'a', { name: 'Q' }", "'a', []", "'a', function () {}",
  "'a', true", "'a', 7", "'a', new String('S')", "'a', { name: undefined }", "'a', { name: 5 }", "'a', { name: null }",
  "'a', { name: 'X', extra: 1 }", "'a', 'AbortError', 'ignorado'", "{ toString() { return 'objmsg' } }, { name: 'TimeoutError' }",
  "'a', { toString() { return 'objname' } }", "'a', { name: { toString() { return 'nested' } } }", "'a', null", "'a', undefined",
  "'a', NaN", "'a', 0", "'a', -0", "'a', 1n", "'a', ['AbortError']", "'a', Object.create({ name: 'Inherited' })",
  "'a', new Proxy({ name: 'Prox' }, {})", "['x', 'y']", "false", "-0", "1n", "{}", "[]",
];
for (const a of argCases) expr(`(function(e){ return [e.name, e.message, e.code, Object.getOwnPropertyNames(e)] })(new DOMException(${a}))`);
// Erros das coerções.
for (const a of ["Symbol()", "'a', Symbol()", "{ toString() { throw new RangeError('boom') } }", "'a', { name: { toString() { throw new RangeError('nome') } } }",
  "'a', { get name() { throw new RangeError('getter') } }", "'a', { get cause() { throw new RangeError('cause') } }",
  "'a', new Proxy({}, { has() { throw new RangeError('has') } })", "'a', new Proxy({}, { get() { throw new RangeError('get') } })"]) {
  expr(`new DOMException(${a})`);
}
expr("(function(){ var order = []; new DOMException({ toString() { order.push('message'); return 'm' } }, { get name() { order.push('name'); return 'x' }, get cause() { order.push('cause'); return 1 } }); return order })()");
expr("(function(){ var order = []; new DOMException('a', new Proxy({}, { has(t, k) { order.push('has:' + String(k)); return false }, get(t, k) { order.push('get:' + String(k)); return undefined } })); return order })()");

// Códigos por nome.
expr(`[${LEGACY_NAMES.map((n) => JSON.stringify(n)).join(", ")}].map(function (n) { return n + '=' + new DOMException('', n).code })`);
expr(`[${LEGACY_NAMES.map((n) => JSON.stringify(n)).join(", ")}].map(function (n) { return new DOMException('', n).name })`);

// cause.
expr("(function(d){ return [d.value, d.writable, d.enumerable, d.configurable] })(Object.getOwnPropertyDescriptor(new DOMException('a', { name: 'Zed', cause: 1 }), 'cause'))");
expr("Object.getOwnPropertyNames(new DOMException('a', { cause: undefined }))");
expr("Object.getOwnPropertyNames(new DOMException('a', { name: 'N', cause: 0 }))");
expr("(function(){ var c = {}; return new DOMException('a', { cause: c }).cause === c })()");
expr("Object.getOwnPropertyNames(new DOMException('a', Object.create({ cause: 1 })))");
expr("new DOMException('a', Object.create({ cause: 1 })).cause");
expr("Object.getOwnPropertyNames(new DOMException('a', 'AbortError'))");
expr("(function(){ var order = []; new DOMException('a', { get cause() { order.push('cause'); return 1 }, get name() { order.push('name'); return 'x' } }); return order })()");

// Chamada sem new e `this` inválido.
for (const c of ["DOMException()", "DOMException('m', 'AbortError')", "DOMException.call({})", "DOMException.call(undefined, 'a')", "DOMException.apply(null, ['a'])",
  "Reflect.apply(DOMException, undefined, [])"]) expr(c);
for (const n of ["code", "name", "message"]) {
  const g = `Object.getOwnPropertyDescriptor(DOMException.prototype, '${n}').get`;
  for (const t of ["{}", "new Error('x')", "null", "undefined", "DOMException.prototype", "1", "'str'", "Symbol()", "Object.create(DOMException.prototype)",
    "{ name: 'AbortError', message: 'x', code: 20 }", "DOMException", "function () {}", "[]"]) expr(`${g}.call(${t})`);
  expr(`${g}.call(new DOMException('mm', 'AbortError'))`);
  expr(`${g}()`);
  expr(`(function(){ try { DOMException.prototype.${n} } catch (e) { return E(e) } })()`);
  expr(`(function(){ var o = Object.create(DOMException.prototype); try { return o.${n} } catch (e) { return E(e) } })()`);
}
expr("Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call(new DOMException('', 'AbortError')) + 1");
expr("new DOMException('x', 'AbortError').code === DOMException.ABORT_ERR");
expr("new DOMException('x', 'AbortError').ABORT_ERR");
expr("new DOMException('x', 'AbortError').INDEX_SIZE_ERR");

// Subclasse e Reflect.construct.
expr("(function(){ class X extends DOMException {} var e = new X('a', 'AbortError'); return [e.name, e.code, e.message, e instanceof X, e instanceof DOMException, e instanceof Error, e.constructor.name, Object.prototype.toString.call(e), String(e), Object.getOwnPropertyNames(e)] })()");
expr("(function(){ class X extends DOMException { constructor() { super('sub', 'SyntaxError') } } var e = new X(); return [e.name, e.code, e.message, e instanceof X] })()");
expr("(function(){ class X extends DOMException { get name() { return 'overridden' } } var e = new X('a', 'AbortError'); return [e.name, e.code, String(e)] })()");
expr("(function(){ var e = Reflect.construct(DOMException, ['a', 'AbortError'], Object); return [Object.getPrototypeOf(e) === Object.prototype, e instanceof DOMException, Object.prototype.toString.call(e), Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call(e), Object.getOwnPropertyDescriptor(DOMException.prototype, 'name').get.call(e)] })()");
expr("(function(){ function F() {} F.prototype = Array.prototype; var e = Reflect.construct(DOMException, ['a'], F); return [Object.getPrototypeOf(e) === Array.prototype, Array.isArray(e), e.message] })()");
expr("(function(){ function F() {} F.prototype = null; var e = Reflect.construct(DOMException, ['a', 'AbortError'], F); return [Object.getPrototypeOf(e) === Object.prototype, Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call(e)] })()");
expr("(function(){ var e = new DOMException('a', 'AbortError'); Object.setPrototypeOf(e, Object.prototype); return [Object.getOwnPropertyDescriptor(DOMException.prototype, 'name').get.call(e), String(e)] })()");
expr("(function(){ var e = Object.create(new DOMException('a', 'AbortError')); try { return e.name } catch (x) { return E(x) } })()");
expr("new DOMException('a', 'AbortError') instanceof Object");
expr("Object.getPrototypeOf(new DOMException('a')) === DOMException.prototype");
expr("[typeof DOMException.captureStackTrace, typeof DOMException.isError, typeof DOMException.stackTraceLimit]");

// Atribuição aos acessores sem setter: em sloppy é ignorada, em strict lança (medido no bun por indirect eval;
// um arquivo do bun é módulo e portanto strict, o que enganava a medida anterior).
expr("(function(){ var e = new DOMException('a', 'AbortError'); e.name = 'x'; e.code = 3; e.message = 'y'; return [e.name, e.code, e.message, Object.getOwnPropertyNames(e)] })()");
for (const n of ["name", "code", "message"]) {
  expr(`(function(){ 'use strict'; var e = new DOMException('a', 'AbortError'); try { e.${n} = 'x' } catch (x) { return [x instanceof TypeError, E(x), e.${n}] } return 'sem erro' })()`);
  expr(`(function(){ var e = new DOMException('a', 'AbortError'); return [Reflect.set(e, '${n}', 'x'), e.${n}] })()`);
  expr(`(function(){ var e = new DOMException('a', 'AbortError'); Object.defineProperty(e, '${n}', { value: 'z' }); e.${n} = 'w'; return [e.${n}, Object.getOwnPropertyDescriptor(e, '${n}').writable, Object.getOwnPropertyNames(e)] })()`);
  expr(`(function(){ var e = new DOMException('a', 'AbortError'); return [delete e.${n}, e.${n}] })()`);
}
expr("(function(){ 'use strict'; try { DOMException.INDEX_SIZE_ERR = 5 } catch (x) { return E(x) } return 'sem erro' })()");
expr("(function(){ DOMException.INDEX_SIZE_ERR = 5; return DOMException.INDEX_SIZE_ERR })()");

// Exceção lançada por função nativa do host (`atob`, `structuredClone`): ganha `line`, `column`, `stack` próprios
// (e `sourceURL` quando o fonte tem nome; o indirect eval do bun não tem, o porte roda um script nomeado, então
// `sourceURL` sai da comparação) e fora do construtor segue sem a pilha de `Error`. O resto da `stack` depende do
// hospedeiro (frames de eval, módulo), por isso só a primeira linha entra.
const THROWN =
  "var T = function (e) { var f = function (n) { return n !== 'sourceURL' }; var d = function (n) { var x = Object.getOwnPropertyDescriptor(e, n); return x && [typeof x.value, x.writable, x.enumerable, x.configurable] }; " +
  "return [e.name, e.code, Object.getOwnPropertyNames(e).filter(f), Object.keys(e).filter(f), e.line, e.column, typeof e.stack, String(e.stack).split('\\n')[0], " +
  "JSON.stringify(e, function (k, v) { return k === 'sourceURL' ? undefined : v }), d('line'), d('column'), d('stack'), Object.getPrototypeOf(e) === DOMException.prototype, e instanceof DOMException] };\n";
const thrown = (call) => programs.push(HELPER + THROWN + `try { ${call}; R = 'sem erro' } catch (e) { R = S(T(e)) }`);
thrown("atob('*')");
thrown("atob('a')");
thrown("btoa('\\u0100')");
thrown("structuredClone(function () {})");
thrown("structuredClone(Symbol())");
thrown("structuredClone({ f() {} })");
thrown("\n\n   atob('*')");
thrown("(function f() {\n    atob('*')\n  })()");
thrown("[1].map(atob)");
thrown("Reflect.apply(atob, undefined, ['*'])");
thrown("atob.call(null, '*')");
thrown("(0, atob)('*')");
thrown("new Function('atob(\"*\")')()");
thrown("eval('atob(\"*\")')");
thrown("(function () { 'use strict'; atob('*') })()");
// Duas exceções do mesmo ponto não compartilham as próprias; `Error.stackTraceLimit` não as afeta como a `Error`.
programs.push(HELPER + THROWN + "var a, b; try { atob('*') } catch (e) { a = e } try { atob('*') } catch (e) { b = e } R = S([a !== b, a.stack === b.stack, a.line === b.line, a.column === b.column])");
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = 0; try { atob('*') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "var old = Error.stackTraceLimit; Error.stackTraceLimit = 1; try { atob('*') } catch (e) { R = S(T(e)) } Error.stackTraceLimit = old");
programs.push(HELPER + THROWN + "try { atob('*') } catch (e) { e.stack = 'novo'; e.line = 99; R = S([e.stack, e.line, Object.keys(e).filter(function (n) { return n !== 'sourceURL' })]) }");
programs.push(HELPER + THROWN + "try { atob('*') } catch (e) { R = S([delete e.stack, 'stack' in e, delete e.line, 'line' in e]) }");
// O `DOMException` criado por `new` não ganha nada, mesmo lançado do JS.
programs.push(HELPER + THROWN + "try { throw new DOMException('m', 'AbortError') } catch (e) { R = S(T(e)) }");
programs.push(HELPER + THROWN + "var e = new DOMException('m', 'AbortError'); try { atob('*') } catch (x) { R = S([x !== e, T(e)[2]]) }");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
