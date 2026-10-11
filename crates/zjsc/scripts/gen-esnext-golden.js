// Gera tests/golden/esnext_bun.tsv: APIs ESNext medidas no bun 1.4.2 com `vm.runInThisContext`, em matrizes que não
// repetem gen-recent-apis-golden.js nem gen-iterator-protocol-golden.js: helpers de Iterator por tipo de fonte,
// fechamento do iterador (contagem de next/return com iterador instrumentado), validação de argumentos, métodos novos
// de Set com set-likes hostis (size, has, keys), Object/Map.groupBy, Array.fromAsync, Promise.withResolvers/try,
// Error.isError, base64/hex de Uint8Array e RegExp.escape. APIs ausentes no bun são puladas.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Sem APIs de host (setTimeout, process, console, require, Bun, URL, Buffer) dentro dos programas.
// Uso: bun scripts/gen-esnext-golden.js > tests/golden/esnext_bun.tsv
const { emitFactored, stepSampler } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const has = expr => {
  try {
    return new Function("return typeof (" + expr + ") !== 'undefined'")();
  } catch {
    return false;
  }
};

// Texto que antecede cada programa: `show` serializa o valor, `tr` captura exceção, `spy` cria iterador instrumentado
// (registra next e return em `log`), `tick` e `done` ajudam nos programas assíncronos.
const PRELUDE = [
  "var log = [];",
  "var show = v => v === undefined ? 'undefined' : typeof v === 'symbol' ? String(v) : typeof v === 'bigint' ? v + 'n' : Object.is(v, -0) ? '-0' : v instanceof Set ? 'Set' + show([...v]) : v instanceof Map ? 'Map' + show([...v]) : v instanceof Error ? v.name + ':' + v.message : Array.isArray(v) ? '[' + v.map(show).join(',') + ']' : typeof v === 'function' ? 'fn' : v && typeof v === 'object' ? '{' + Reflect.ownKeys(v).map(k => String(k) + ':' + show(v[k])).join(',') + '}' : typeof v === 'string' ? JSON.stringify(v) : String(v);",
  "var tr = f => { try { return show(f()) } catch (e) { return 'throw ' + e.name + ': ' + e.message } };",
  "var spy = (arr, tag) => { var i = 0; return { next() { log.push((tag || '') + 'next'); return i < arr.length ? { value: arr[i++], done: false } : { value: undefined, done: true } }, return(v) { log.push((tag || '') + 'return'); return {} }, __proto__: Iterator.prototype } };",
  "var pa = f => Promise.resolve().then(f).then(v => { globalThis.R = show(v) + '|' + log.join() }, e => { globalThis.R = 'reject ' + (e && e.name) + ': ' + (e && e.message) + '|' + log.join() });",
].join(" ");

// Os candidatos entram em `pool`; os que só entram em parte levam a densidade (`thinE(passo, expr)`, 1 em `passo`) e a
// escolha é por hash do texto (`sampleByHash` dentro do `stepSampler`), nunca pela posição na lista nem pelo índice do método.
const pool = stepSampler();
const add = body => pool.push(1, body);
// Expressão síncrona: R recebe o `show` do valor, ou `throw Nome: mensagem`.
const exprProgram = expr => `R = tr(() => (${expr}))`;
const E = expr => add(exprProgram(expr));
const thinE = (step, expr) => pool.push(step, exprProgram(expr));
// Corpo síncrono que devolve valor via `return`; o log aparece depois do valor.
const B = body => add(`R = tr(() => { ${body} }) + '|' + log.join()`);
// Corpo assíncrono: `body` devolve (ou devolve uma promise de) o valor.
const A = body => add(`pa(() => { ${body} })`);
// Compilação por eval indireto, para SyntaxError (nenhum caso hoje, mas fica para completar lacunas).

// ---- Fontes de iterador, cada uma como expressão nova.
const sources = {
  array: "[1, 2, 3, 4, 5].values()",
  generator: "(function* () { yield 1; yield 2; yield 3; yield 4; yield 5 })()",
  set: "new Set([1, 2, 3, 4, 5]).values()",
  spy: "spy([1, 2, 3, 4, 5])",
  empty: "[].values()",
};
if (has("Iterator.prototype.map")) {
  const ops = [
    "map(x => x * 2).toArray()",
    "map((x, i) => [x, i]).toArray()",
    "map(function (x) { return this === undefined || this === globalThis }).toArray()",
    "filter(x => x % 2).toArray()",
    "filter((x, i) => i % 2 === 0).toArray()",
    "filter(() => false).toArray()",
    "take(0).toArray()",
    "take(1).toArray()",
    "take(3).toArray()",
    "take(10).toArray()",
    "take(2.9).toArray()",
    "take(Infinity).toArray()",
    "drop(0).toArray()",
    "drop(1).toArray()",
    "drop(3).toArray()",
    "drop(10).toArray()",
    "drop(Infinity).toArray()",
    "flatMap(x => [x, x]).toArray()",
    "flatMap(x => 'ab').toArray()",
    "flatMap(x => new Set([x])).toArray()",
    "flatMap((x, i) => [i]).toArray()",
    "flatMap(x => []).toArray()",
    "flatMap(x => [x].values()).toArray()",
    "reduce((a, b) => a + b)",
    "reduce((a, b) => a + b, 100)",
    "reduce((a, b, i) => a + i, 0)",
    "reduce((a, b) => a + '' + b, '')",
    "toArray()",
    "some(x => x > 2)",
    "some(x => x > 100)",
    "every(x => x > 0)",
    "every(x => x < 2)",
    "find(x => x > 2)",
    "find(x => x > 100)",
    "forEach(x => log.push('v' + x))",
    "map(x => x + 1).filter(x => x % 2).take(2).toArray()",
    "drop(1).take(2).map(x => x * 10).toArray()",
    "take(3).drop(1).flatMap(x => [x, -x]).toArray()",
    "filter(x => x > 1).drop(1).reduce((a, b) => a + b)",
    "[...take(2)]",
    "Array.from(drop(3))",
    "next()",
    "take(2).next()",
    "map(x => x).return()",
    "map(x => x).next()",
  ];
  for (const [name, src] of Object.entries(sources)) {
    for (const op of ops) B(`return ${src}.${op}`);
  }
}

// ---- Fechamento do iterador e validação de argumentos (o log mostra quantos next e return aconteceram).
if (has("Iterator.prototype.map")) {
  const closers = [
    ["take(2)", "toArray"],
    ["take(0)", "toArray"],
    ["take(5)", "toArray"],
    ["take(6)", "toArray"],
    ["drop(2)", "toArray"],
    ["map(x => x)", "toArray"],
  ];
  for (const [step, end] of closers) {
    B(`var h = spy([1, 2, 3, 4, 5]).${step}; var r = h.${end}(); return [r, log.join()]`);
    B(`var h = spy([1, 2, 3, 4, 5]).${step}; h.next(); h.return(); return [log.join(), h.next()]`);
    B(`var h = spy([1, 2, 3, 4, 5]).${step}; h.return(); h.return(); return log.join()`);
    B(`var h = spy([1, 2, 3, 4, 5]).${step}; h.return(); return h.next()`);
    B(`var h = spy([1, 2, 3, 4, 5]).${step}; var a = h.next(), b = h.next(), c = h.next(); h.return(); return [a, b, c, log.join()]`);
  }
  const cbs = ["map", "filter", "forEach", "some", "every", "find", "flatMap"];
  for (const op of cbs) {
    B(`spy([1, 2, 3]).${op}(x => { throw new RangeError('boom' + x) })${op === "forEach" || op === "some" || op === "every" || op === "find" ? "" : ".next()"}`);
    B(`spy([1, 2, 3]).${op}(x => { throw 7 })${op === "forEach" || op === "some" || op === "every" || op === "find" ? "" : ".next()"}`);
    B(`spy([1, 2, 3]).${op}(1)`);
    B(`spy([1, 2, 3]).${op}()`);
    B(`spy([1, 2, 3]).${op}(null)`);
    B(`spy([1, 2, 3]).${op}({})`);
    B(`spy([1, 2, 3]).${op}(class {})`);
    B(`spy([1, 2, 3]).${op}(function* () {})`);
    B(`spy([1, 2, 3]).${op}(async x => x)`);
    B(`Iterator.prototype.${op}.call(null, x => x)`);
    B(`Iterator.prototype.${op}.call(1, x => x)`);
    B(`Iterator.prototype.${op}.call('abc', x => x)`);
    B(`Iterator.prototype.${op}.call({}, x => x)`);
    B(`Iterator.prototype.${op}.call({ next: 1 }, x => x)`);
  }
  const args = ["NaN", "-1", "-0.5", "'x'", "'2'", "undefined", "null", "{}", "-Infinity", "1n", "Symbol()", "({ valueOf() { return 2 } })", "true", "[]", "2**53"];
  for (const arg of args) {
    B(`return spy([1, 2, 3, 4, 5]).take(${arg}).toArray()`);
    B(`return spy([1, 2, 3, 4, 5]).drop(${arg}).toArray()`);
  }
  B("spy([1, 2, 3]).reduce((a, b) => { throw new Error('r') }, 0)");
  B("spy([]).reduce((a, b) => a)");
  B("return spy([]).reduce((a, b) => a, 'init')");
  B("return spy([9]).reduce((a, b) => 'never')");
  B("return spy([1, 2]).reduce((a, b) => a + b, undefined)");
  B("return spy([1, 2, 3]).some(x => x === 2)");
  B("return spy([1, 2, 3]).every(x => x === 2)");
  B("return spy([1, 2, 3]).find(x => x === 3)");
  B("return spy([1, 2, 3]).find(x => x === 9)");
  B("return spy([1, 2, 3]).some(() => false)");
  B("return spy([1, 2, 3]).every(() => true)");
  B("return spy([1, 2, 3]).forEach(() => {})");
  B("var inner = spy(['a', 'b'], 'in:'); return spy([1, 2]).flatMap(x => inner).toArray()");
  B("var inner = spy(['a', 'b'], 'in:'); var h = spy([1, 2]).flatMap(x => inner); h.next(); h.return(); return 0");
  B("var h = spy([1, 2]).flatMap(x => spy(['a', 'b'], 'in' + x + ':')); h.next(); h.next(); h.next(); return h.return()");
  B("var h = spy([1, 2]).flatMap(x => spy(['a', 'b'], 'in:')); h.next(); h.return(); return h.next()");
  B("return spy([1, 2]).flatMap(x => 5).toArray()");
  B("return spy([1, 2]).flatMap(x => 'ab').toArray()");
  B("return spy([1, 2]).flatMap(x => new String('ab')).toArray()");
  B("return spy([1, 2]).flatMap(x => ({})).toArray()");
  B("return spy([1, 2]).flatMap(x => ({ next() { return { done: true } } })).toArray()");
  B("return spy([1, 2]).flatMap(x => ({ [Symbol.iterator]() { return { next() { return { done: true } } } } })).toArray()");
  B("return spy([1, 2]).flatMap(x => ({ [Symbol.iterator]: null })).toArray()");
  B("return spy([1, 2]).flatMap(x => null).toArray()");
  B("return spy([1, 2]).flatMap(x => undefined).toArray()");
  B("return spy([1, 2]).flatMap(x => Symbol()).toArray()");
  B("return spy([1, 2]).flatMap(x => 1n).toArray()");
  B("return spy([1, 2]).flatMap(x => [[x]]).toArray()");
  B("var it = { next() { log.push('n'); return { done: false, value: 1 } }, return() { log.push('r'); throw new Error('close fail') }, __proto__: Iterator.prototype }; return it.take(1).toArray()");
  B("var it = { next() { log.push('n'); throw new Error('next fail') }, return() { log.push('r'); return {} }, __proto__: Iterator.prototype }; return it.map(x => x).next()");
  B("var it = { next() { log.push('n'); return 5 }, return() { log.push('r'); return {} }, __proto__: Iterator.prototype }; return it.map(x => x).next()");
  B("var it = { next() { log.push('n'); return { get done() { log.push('d'); return false }, get value() { log.push('v'); return 1 } } }, __proto__: Iterator.prototype }; return it.map(x => x).next()");
  B("var it = { next() { log.push('n'); return { get done() { log.push('d'); return true }, get value() { log.push('v'); return 1 } } }, __proto__: Iterator.prototype }; return it.map(x => x).next()");
  B("var it = { get next() { log.push('getnext'); return () => ({ done: true }) }, __proto__: Iterator.prototype }; var h = it.map(x => x); log.push('made'); return h.next()");
  B("var it = { get next() { log.push('getnext'); return () => ({ done: true }) }, __proto__: Iterator.prototype }; var h = it.take(1); h.next(); h.next(); return 0");
  B("var it = { next() { log.push('n'); return { done: false, value: 1 } }, return() { log.push('r'); return 5 }, __proto__: Iterator.prototype }; var h = it.map(x => x); h.next(); return h.return()");
  B("var it = { next() { return { done: false, value: 1 } }, return: null, __proto__: Iterator.prototype }; var h = it.map(x => x); h.next(); return h.return()");
  B("var it = { next() { return { done: false, value: 1 } }, return: 1, __proto__: Iterator.prototype }; var h = it.map(x => x); h.next(); return h.return()");
  B("var it = { next() { return { done: false, value: 1 } }, __proto__: Iterator.prototype }; var h = it.map(x => x); return h.return()");
  B("var g = (function* () { try { yield 1; yield 2 } finally { log.push('fin') } })(); var h = g.take(1); h.next(); return [h.next(), log.join()]");
  B("var g = (function* () { try { yield 1; yield 2 } finally { log.push('fin') } })(); var r = g.take(1).toArray(); return [r, log.join()]");
  B("var g = (function* () { try { yield 1; yield 2 } finally { log.push('fin') } })(); g.some(x => x === 1); return log.join()");
  B("var g = (function* () { try { yield 1; yield 2 } finally { log.push('fin') } })(); try { g.map(x => { throw 1 }).next() } catch (e) {} return log.join()");
  B("var g = (function* () { try { yield 1; yield 2 } finally { log.push('fin') } })(); for (var x of g.map(x => x)) { break } return log.join()");
  B("var g = (function* () { try { yield 1; yield 2 } finally { log.push('fin') } })(); var [a] = g.filter(x => true); return [a, log.join()]");
  B("var g = (function* () { yield h.next() })(); var h = g.map(x => x); return h.next()");
  B("var h = (function* () { yield h2.next() })(); var h2 = h.map(x => x); return h2.next()");
  B("var self; var it = { next() { return self.next() }, __proto__: Iterator.prototype }; self = it.map(x => x); return self.next()");
  B("var self; var it = { next() { return { done: false, value: self.return() } }, __proto__: Iterator.prototype }; self = it.map(x => x); return self.next()");
}

// ---- Iterator.from, a classe Iterator e o objeto helper.
if (has("Iterator.from")) {
  const fromArgs = [
    "[1, 2]", "'ab'", "new Set([1])", "new Map([[1, 2]])", "(function* () { yield 1 })()", "{ next() { return { done: true } } }",
    "{ [Symbol.iterator]() { return { next() { return { done: true } } } } }", "{ [Symbol.iterator]: null, next() { return { done: true } } }",
    "{ [Symbol.iterator]: undefined }", "{ [Symbol.iterator]: 1 }", "{ [Symbol.iterator]() { return 1 } }", "{ [Symbol.iterator]() { return null } }",
    "{}", "1", "null", "undefined", "true", "Symbol()", "new String('xy')", "Object('x')", "10n", "{ next: 1 }", "{ next() {} }",
    "new Proxy([], {})", "Iterator.prototype", "[].values()", "Object.create(Iterator.prototype)", "Object.create(null)",
    "Object.assign(Object.create(Iterator.prototype), { next() { return { done: true } } })",
    "function () {}", "class {}", "Math", "{ length: 2, 0: 'a', 1: 'b' }",
  ];
  for (const arg of fromArgs) {
    B(`var r = Iterator.from(${arg}); return [typeof r, Object.getPrototypeOf(r) === Iterator.prototype, r instanceof Iterator, Object.prototype.toString.call(r)]`);
    B(`return Iterator.from(${arg}).toArray()`);
    B(`var o = ${arg}; var r = Iterator.from(o); return r === o`);
  }
  B("var o = { next() { return { done: false, value: 1 } }, return() { log.push('r'); return {} } }; var w = Iterator.from(o); w.return(); return Object.getPrototypeOf(w) === Iterator.prototype");
  B("var o = { next() { log.push('n'); return { done: false, value: 1 } } }; var w = Iterator.from(o); return [w.next(), w.return(), log.join()]");
  B("var o = { next() { log.push('n'); return { done: true } }, return() { log.push('r'); return { done: true, value: 5 } } }; var w = Iterator.from(o); return [w.return(), log.join()]");
  B("var o = { next(a) { log.push('n' + a); return { done: false, value: 1 } } }; var w = Iterator.from(o); w.next(5); return log.join()");
  B("var w = Iterator.from({ next() { return { done: false } } }); return Object.getOwnPropertyNames(w).join()");
  B("var w = Iterator.from({ next() { return { done: false } } }); return Object.getOwnPropertyNames(Object.getPrototypeOf(w)).join()");
  B("var w = Iterator.from({ next() { return { done: false } } }); var p = Object.getPrototypeOf(w); return [p === Iterator.prototype]");
  B("var w1 = Iterator.from({ next() { return { done: false } } }), w2 = Iterator.from({ next() { return { done: false } } }); return Object.getPrototypeOf(w1) === Object.getPrototypeOf(w2)");
}
if (has("Iterator")) {
  E("[typeof Iterator, Iterator.length, Iterator.name, Object.getPrototypeOf(Iterator) === Function.prototype]");
  E("Object.getOwnPropertyNames(Iterator).sort()");
  E("Object.getOwnPropertyNames(Iterator.prototype).sort()");
  E("Object.getOwnPropertyDescriptor(Iterator, 'prototype')");
  E("Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag)");
  E("Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor')");
  E("(() => { var d = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable] })()");
  E("(() => { var d = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag); return [typeof d.get, typeof d.set, d.enumerable, d.configurable] })()");
  E("Iterator.prototype[Symbol.toStringTag]");
  E("Iterator.prototype.constructor === Iterator");
  E("new Iterator()");
  E("Iterator()");
  E("class It extends Iterator {} ; new It() instanceof Iterator");
  E("class It extends Iterator { next() { return { done: true } } } ; new It().toArray()");
  E("class It extends Iterator { next() { return { done: true } } } ; [...new It()]");
  E("class It extends Iterator {} ; Object.prototype.toString.call(new It())");
  E("Reflect.construct(Iterator, [], Object)");
  E("Reflect.construct(Iterator, [], Iterator) instanceof Iterator");
  E("Iterator.prototype[Symbol.iterator].call(5)");
  E("Iterator.prototype[Symbol.iterator].call(undefined)");
  E("Iterator.prototype[Symbol.iterator].name");
  E("(() => { var o = {}; return Iterator.prototype[Symbol.iterator].call(o) === o })()");
  E("(() => { var o = {}; o.constructor = 1; return Object.keys(Iterator.prototype) })()");
  E("(() => { var o = Object.create(Iterator.prototype); o.constructor = 5; return [o.constructor, Iterator.prototype.constructor === Iterator] })()");
  E("(() => { var o = Object.create(Iterator.prototype); o[Symbol.toStringTag] = 'X'; return [Object.prototype.toString.call(o), Iterator.prototype[Symbol.toStringTag]] })()");
  E("(() => { Iterator.prototype.constructor = 5; return Iterator.prototype.constructor === Iterator })()");
  E("(() => { Iterator.prototype[Symbol.toStringTag] = 'Z'; return Iterator.prototype[Symbol.toStringTag] })()");
  E("(() => { Object.freeze(Iterator.prototype); var o = Object.create(Iterator.prototype); o.constructor = 1; return Object.keys(o) })()");
  E("(() => { var o = Object.create(Iterator.prototype); Object.defineProperty(o, 'constructor', { value: 1 }); return o.constructor })()");
  E("Object.getPrototypeOf(Object.getPrototypeOf([].values())) === Iterator.prototype");
  E("Object.getPrototypeOf(Object.getPrototypeOf((function* () {})())) === Object.getPrototypeOf(function* () {}).prototype");
  E("Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf((function* () {})()))) === Iterator.prototype");
  E("Iterator.prototype.isPrototypeOf(new Set().values())");
  E("Iterator.prototype.isPrototypeOf(new Map().entries())");
  E("Iterator.prototype.isPrototypeOf(''[Symbol.iterator]())");
  E("Iterator.prototype.isPrototypeOf('a'.matchAll(/a/g))");
  E("Iterator.prototype.isPrototypeOf(new RegExp('a').exec)");
  for (const name of ["map", "filter", "take", "drop", "flatMap", "reduce", "toArray", "forEach", "some", "every", "find"]) {
    if (!has(`Iterator.prototype.${name}`)) continue;
    E(`[Iterator.prototype.${name}.name, Iterator.prototype.${name}.length]`);
    E(`Object.getOwnPropertyDescriptor(Iterator.prototype, '${name}')`);
    E(`Object.getOwnPropertyNames(Iterator.prototype.${name}).sort()`);
    E(`(() => { try { new Iterator.prototype.${name}() } catch (e) { return e.name } })()`);
    E(`Iterator.prototype.${name}.hasOwnProperty('prototype')`);
  }
  E("(() => { var h = [1].values().map(x => x); return [Object.prototype.toString.call(h), Object.getOwnPropertyNames(h).length, h[Symbol.iterator]() === h] })()");
  E("(() => { var h = [1].values().map(x => x); var p = Object.getPrototypeOf(h); return [Object.getOwnPropertyNames(p).sort(), Object.getPrototypeOf(p) === Iterator.prototype, p[Symbol.toStringTag]] })()");
  E("(() => { var p1 = Object.getPrototypeOf([1].values().map(x => x)); var p2 = Object.getPrototypeOf([1].values().filter(x => x)); return p1 === p2 })()");
  E("(() => { var p = Object.getPrototypeOf([1].values().map(x => x)); return Object.getOwnPropertyDescriptor(p, Symbol.toStringTag) })()");
  E("(() => { var p = Object.getPrototypeOf([1].values().map(x => x)); return [p.next.name, p.next.length, p.return.name, p.return.length] })()");
  E("(() => { var p = Object.getPrototypeOf([1].values().map(x => x)); try { p.next.call({}) } catch (e) { return e.name + ': ' + e.message } })()");
  E("(() => { var p = Object.getPrototypeOf([1].values().map(x => x)); try { p.next.call([1].values()) } catch (e) { return e.name } })()");
  E("(() => { var p = Object.getPrototypeOf([1].values().map(x => x)); try { p.return.call({}) } catch (e) { return e.name } })()");
  E("(() => { var p = Object.getPrototypeOf([1].values().map(x => x)); try { p.next.call(undefined) } catch (e) { return e.name } })()");
  E("(() => { var a = [1].values().map(x => x), b = [2].values().filter(x => x); var p = Object.getPrototypeOf(a); try { return p.next.call(b) } catch (e) { return e.name } })()");
  E("(() => { var a = [1, 2].values().map(x => x), b = [1, 2].values().map(x => x * 2); var p = Object.getPrototypeOf(a); return [p.next.call(b), a.next()] })()");
}

// ---- Métodos novos de Set com set-likes.
if (has("Set.prototype.union")) {
  const methods = ["union", "intersection", "difference", "symmetricDifference", "isSubsetOf", "isSupersetOf", "isDisjointFrom"];
  const likes = [
    "new Set([2, 3, 4])", "new Set()", "new Set([1, 2, 3])", "new Set([1, 2, 3, 4, 5, 6])", "new Set([9])", "new Set([3, 2, 1])",
    "new Map([[2, 'a'], [3, 'b'], [4, 'c']])", "new Map()",
    "{ size: 2, has: x => x === 2 || x === 3, keys: () => [2, 3].values() }",
    "{ size: 3, has: x => x === 1, keys: () => [1, 5, 1].values() }",
    "{ size: 0, has: () => false, keys: () => [].values() }",
    "{ size: Infinity, has: () => true, keys: () => { throw new Error('keys') } }",
    "{ size: 100, has: x => x === 1, keys: () => [1, 2, 3].values() }",
    "{ size: 2.5, has: x => x === 2, keys: () => [2].values() }",
    "{ size: -0, has: () => false, keys: () => [].values() }",
    "{ size: '2', has: x => x === 2, keys: () => [2].values() }",
    "{ size: true, has: x => x === 2, keys: () => [2].values() }",
    "{ size: null, has: () => false, keys: () => [].values() }",
    "{ size: NaN, has: () => false, keys: () => [].values() }",
    "{ size: -1, has: () => false, keys: () => [].values() }",
    "{ size: undefined, has: () => false, keys: () => [].values() }",
    "{ has: () => false, keys: () => [].values() }",
    "{ size: 1n, has: () => false, keys: () => [].values() }",
    "{ size: Symbol(), has: () => false, keys: () => [].values() }",
    "{ size: { valueOf() { return 2 } }, has: x => x === 2, keys: () => [2].values() }",
    "{ size: 2, keys: () => [2].values() }",
    "{ size: 2, has: 1, keys: () => [2].values() }",
    "{ size: 2, has: null, keys: () => [2].values() }",
    "{ size: 2, has: () => true }",
    "{ size: 2, has: () => true, keys: 1 }",
    "{ size: 2, has: () => true, keys: () => 1 }",
    "{ size: 2, has: () => true, keys: () => ({}) }",
    "{ size: 2, has: () => true, keys: () => ({ next: 1 }) }",
    "{ size: 2, has: () => true, keys: () => null }",
    "{ size: 2, has: () => true, keys: () => [2, 3] }",
    "{ size: 2, has: () => true, keys: () => ({ next() { return { done: true } } }) }",
    "{ size: 2, has: () => true, keys: () => ({ next() { return 5 } }) }",
    "{ size: 2, has: () => 1, keys: () => [2].values() }",
    "{ size: 2, has: () => 0, keys: () => [2].values() }",
    "{ size: 2, has: () => 'x', keys: () => [2].values() }",
    "{ size: 2, has: () => undefined, keys: () => [2].values() }",
    "{ size: 2, has: x => { throw new Error('has') }, keys: () => [2].values() }",
    "{ size: { valueOf() { throw new Error('size') } }, has: () => true, keys: () => [].values() }",
    "{ size: 2, has: () => true, keys() { throw new Error('keys') } }",
    "{ size: 2, has: () => true, keys: () => ({ next() { throw new Error('next') } }) }",
    "{ size: 3, has: x => true, keys: () => [1.0, 2, -0, NaN].values() }",
    "{ size: 3, has: x => true, keys: () => [0, NaN].values() }",
    "[1, 2, 3]", "'abc'", "5", "null", "undefined", "true", "Symbol()", "[]", "{}", "new WeakSet()", "function () {}", "new Proxy(new Set([1]), {})",
    "Object.create(new Set([1, 2]))", "Object.assign(new Set([1, 2]), { size: 0 })", "new (class extends Set { get size() { return 1 } has() { return true } keys() { return [1].values() } })()",
    "new (class extends Set { has() { return false } keys() { return [7].values() } })([1, 2])",
  ];
  const bases = ["new Set([1, 2, 3])", "new Set()"];
  for (const method of methods) {
    // Cada método vê um terço dos set-likes, para manter o golden com tamanho razoável.
    for (const like of likes) thinE(3, `new Set([1, 2, 3]).${method}(${like})`);
    E(`new Set([1, 2, 3]).${method}()`);
    E(`Set.prototype.${method}.call({}, new Set())`);
    E(`Set.prototype.${method}.call(new Map(), new Set())`);
    E(`Set.prototype.${method}.call(null, new Set())`);
    E(`Set.prototype.${method}.call([1], new Set())`);
    E(`[Set.prototype.${method}.name, Set.prototype.${method}.length]`);
    E(`Object.getOwnPropertyDescriptor(Set.prototype, '${method}')`);
    for (const base of bases) E(`${base}.${method}(new Set([1, 3, 5, 7]))`);
    B(`var calls = []; var like = { get size() { calls.push('size'); return 2 }, get has() { calls.push('get has'); return x => { calls.push('has'); return true } }, get keys() { calls.push('get keys'); return () => { calls.push('keys'); return [1, 9].values() } } }; var r = new Set([1, 2, 3]).${method}(like); return [show(r), calls.join()]`);
    B(`var calls = []; var like = { get size() { calls.push('size'); return 10 }, get has() { calls.push('get has'); return x => { calls.push('has' + x); return x === 2 } }, get keys() { calls.push('get keys'); return () => { calls.push('keys'); return [2].values() } } }; var r = new Set([1, 2, 3]).${method}(like); return [show(r), calls.join()]`);
    B(`var calls = []; var like = { get size() { calls.push('size'); return 1 }, get has() { calls.push('get has'); return x => { calls.push('has' + x); return x === 2 } }, get keys() { calls.push('get keys'); return () => { calls.push('keys'); return { next() { calls.push('next'); return { done: true } }, return() { calls.push('return'); return {} } } } } }; var r = new Set([1, 2, 3]).${method}(like); return [show(r), calls.join()]`);
    B(`var s = new Set([1, 2, 3]); var like = { size: 5, has: x => { s.delete(2); return true }, keys: () => { s.delete(2); return [1, 2, 3].values() } }; return [show(s.${method}(like)), show(s)]`);
    B(`var s = new Set([1, 2, 3]); var like = { size: 1, has: x => { s.add(4); return true }, keys: () => { s.add(4); return [1].values() } }; return [show(s.${method}(like)), show(s)]`);
    B(`var r = new Set([1]).${method}(new Set([1])); return [r instanceof Set, r === true || r === false || Object.getPrototypeOf(r) === Set.prototype]`);
    B(`class S2 extends Set {} var r = new S2([1, 2]).${method}(new Set([2])); return [r instanceof S2, typeof r]`);
    B(`var s = new Set([3, 1, 2]); var r = s.${method}(new Set([2, 5, 3])); return show(r)`);
    B(`var s = new Set([1, 2, 3]); var r = s.${method}(new Map([[3, 'x'], [4, 'y'], [1, 'z']])); return show(r)`);
    B(`var s = new Set([-0, 1]); var r = s.${method}(new Set([0, NaN])); return show(r)`);
    B(`var s = new Set([NaN, 1]); var r = s.${method}(new Set([NaN])); return show(r)`);
  }
  // Ordem de iteração do resultado.
  E("[...new Set([1, 2, 3]).union(new Set([5, 4, 3]))]");
  E("[...new Set([3, 2, 1]).intersection(new Set([1, 2, 3]))]");
  E("[...new Set([1, 2, 3]).intersection({ size: 10, has: x => true, keys: () => [3, 2, 1].values() })]");
  E("[...new Set([1, 2, 3]).intersection({ size: 1, has: x => true, keys: () => [3, 2, 1, 3].values() })]");
  E("[...new Set([1, 2, 3, 4]).symmetricDifference(new Set([4, 0, 1, 9]))]");
  E("[...new Set([1, 2, 3, 4]).difference({ size: 1, has: x => x === 2, keys: () => [2].values() })]");
  E("[...new Set([1, 2, 3, 4]).difference({ size: 10, has: x => x === 2, keys: () => [4, 3].values() })]");
  E("[...new Set().union(new Set([1]))]");
  E("new Set([1, 2]).isSubsetOf(new Set([1, 2]))");
  E("new Set([1, 2]).isSupersetOf(new Set([1, 2]))");
  E("new Set([1, 2]).isDisjointFrom(new Set([1, 2]))");
  E("new Set().isSubsetOf(new Set())");
  E("new Set().isSupersetOf(new Set())");
  E("new Set().isDisjointFrom(new Set())");
  E("new Set([1]).isSubsetOf({ size: 0, has: () => true, keys: () => [].values() })");
  E("new Set([1]).isSupersetOf({ size: 5, has: () => true, keys: () => [1].values() })");
  E("new Set([1, 2, 3]).isSupersetOf({ size: 2, has: () => true, keys: () => [1, 9].values() })");
  E("new Set([1, 2, 3]).isDisjointFrom({ size: 2, has: () => true, keys: () => [9, 3].values() })");
  E("new Set([1, 2, 3]).isDisjointFrom({ size: 20, has: x => x === 3, keys: () => [9].values() })");
  E("new Set([1]).union(new Set([1])) === undefined");
  E("(() => { var s = new Set([1, 2]); var u = s.union(new Set([3])); return [s.size, u.size, s === u] })()");
}

// ---- Object.groupBy e Map.groupBy.
if (has("Object.groupBy")) {
  const inputs = [
    "[1, 2, 3, 4, 5]", "[]", "'abcabc'", "new Set([1, 2, 3])", "new Map([[1, 2], [3, 4]])", "(function* () { yield 1; yield 2; yield 3 })()",
    "[1, , 3]", "{ length: 3, 0: 'a', 1: 'b', 2: 'c' }", "[1, 2, 3].values()", "'😀a😀'", "[0, -0, NaN, NaN, 1, 1n]", "['a', 'b', 'a']",
  ];
  const keyFns = [
    "x => x % 2", "x => String(x)", "(x, i) => i % 2", "x => typeof x", "x => x", "() => 'k'", "() => undefined", "() => null", "() => -0",
    "x => ({ toString() { return 'obj' } })", "x => Symbol.for('s')", "x => 1n", "x => NaN", "x => [x]", "(x, i) => 'k' + (i % 3)", "x => x > 2 ? 'big' : 'small'",
    "function () { return this === undefined ? 'u' : typeof this }", "() => '__proto__'", "() => 'constructor'", "() => 'hasOwnProperty'", "() => 0.5", "() => 1e21", "() => '01'", "() => 1",
  ];
  for (const input of inputs) {
    for (const fn of keyFns.slice(0, 6)) {
      E(`Object.groupBy(${input}, ${fn})`);
      E(`Map.groupBy(${input}, ${fn})`);
    }
  }
  for (const fn of keyFns) {
    E(`Object.groupBy([1, 2, 3, 4], ${fn})`);
    E(`Map.groupBy([1, 2, 3, 4], ${fn})`);
  }
  for (const bad of ["null", "undefined", "1", "true", "Symbol()", "{}", "{ length: 1 }", "5n"]) {
    E(`Object.groupBy(${bad}, x => x)`);
    E(`Map.groupBy(${bad}, x => x)`);
  }
  for (const badFn of ["undefined", "null", "1", "{}", "'x'", "class {}", "Symbol()"]) {
    E(`Object.groupBy([1], ${badFn})`);
    E(`Map.groupBy([1], ${badFn})`);
  }
  E("Object.getPrototypeOf(Object.groupBy([1], x => x))");
  E("Object.getOwnPropertyDescriptor(Object.groupBy([1], x => 'a'), 'a')");
  E("Object.isExtensible(Object.groupBy([1], x => x))");
  E("Object.isFrozen(Object.groupBy([1], x => x))");
  E("Object.keys(Object.groupBy([3, 1, 2], x => x))");
  E("Object.keys(Object.groupBy(['b', 'a', '2', '1'], x => x))");
  E("Object.getOwnPropertySymbols(Object.groupBy([1, 2], x => Symbol.for('q')))");
  E("Object.groupBy([1, 2], x => ({ toString() { return 'a' } })).a");
  E("Object.groupBy([1, 2], x => ({ toString() { throw new Error('ts') } }))");
  E("Object.groupBy([1, 2], x => ({ [Symbol.toPrimitive]() { return 'p' } }))");
  E("Map.groupBy([1, 2], x => ({ toString() { return 'a' } })).size");
  E("Map.groupBy([-0, 0], x => x).size");
  E("Object.is([...Map.groupBy([1], x => -0).keys()][0], 0)");
  E("[...Map.groupBy([NaN, NaN], x => x).keys()]");
  E("Map.groupBy([1, 2, 3], x => x % 2) instanceof Map");
  E("Object.getPrototypeOf(Map.groupBy([1], x => x)) === Map.prototype");
  E("Object.groupBy([1, 2, 3], function (x) { 'use strict'; return typeof this })");
  E("Object.groupBy([1, 2, 3], (x, i, a) => arguments.length)");
  E("(() => { var n = []; Object.groupBy([5, 6], function () { n.push(arguments.length) }); return n })()");
  E("(() => { var seen = []; Object.groupBy([5, 6], (x, i) => { seen.push([x, i]); return 'k' }); return seen })()");
  E("(() => { var a = [1, 2, 3]; return Object.groupBy(a, x => { if (x === 1) a.push(4); return x % 2 }) })()");
  E("(() => { var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 } }, return() { log.push('ret'); return {} } } } }; try { Object.groupBy(it, () => { throw new Error('x') }) } catch (e) { return log } })()");
  B("var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 } }, return() { log.push('ret'); return {} } } } }; try { Map.groupBy(it, () => { throw new Error('x') }) } catch (e) { return e.message }");
  B("var it = { [Symbol.iterator]() { return { next() { log.push('n'); return { done: log.length > 3, value: 1 } }, return() { log.push('ret'); return {} } } } }; return Map.groupBy(it, x => x)");
  E("[Object.groupBy.name, Object.groupBy.length, Map.groupBy.name, Map.groupBy.length]");
  E("Object.getOwnPropertyDescriptor(Object, 'groupBy')");
  E("Object.getOwnPropertyDescriptor(Map, 'groupBy')");
  E("Map.groupBy.call(null, [1], x => x) instanceof Map");
  E("Map.groupBy.call(class extends Map {}, [1], x => x) instanceof Map");
  E("Object.groupBy.call(null, [1], x => x)");
  E("Reflect.construct(Object.groupBy, [])");
  E("Reflect.construct(Map.groupBy, [])");
}

// ---- Array.fromAsync (assíncrono; o resultado traz a ordem observada em `log`).
if (has("Array.fromAsync")) {
  const items = [
    "[1, 2, 3]", "[Promise.resolve(1), 2, Promise.resolve(3)]", "[]", "new Set([1, 2])", "new Map([[1, 2]])", "'ab'",
    "(function* () { yield 1; yield Promise.resolve(2) })()", "(async function* () { yield 1; yield 2 })()", "{ length: 2, 0: 'a', 1: Promise.resolve('b') }",
    "{ length: 0 }", "{ length: '2', 0: 1, 1: 2 }", "{ length: -1 }", "{ length: Infinity }", "{ length: NaN, 0: 1 }", "{}", "{ 0: 1 }",
    "[Promise.reject(new Error('rej'))]", "[1, Promise.reject(new Error('rej2')), Promise.reject(new Error('rej3'))]",
    "{ [Symbol.asyncIterator]() { return (async function* () { yield 'ai' })() } }",
    "{ [Symbol.asyncIterator]: null, [Symbol.iterator]() { return ['si'].values() } }",
    "{ [Symbol.asyncIterator]: undefined, length: 1, 0: 'al' }",
    "{ [Symbol.asyncIterator]: 1 }", "{ [Symbol.iterator]: 1 }", "{ [Symbol.asyncIterator]() { return 1 } }", "{ [Symbol.iterator]() { return {} } }",
    "{ [Symbol.asyncIterator]() { return { next() { return { done: true } } } } }", "{ [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: true }) } } } }",
    "{ [Symbol.asyncIterator]() { return { next() { return 5 } } } }", "{ [Symbol.asyncIterator]() { return { next() { return Promise.resolve(5) } } } }",
    "{ [Symbol.asyncIterator]() { throw new Error('ai') } }", "null", "undefined", "5", "true", "Symbol()", "10n", "[1, 2, 3].values()", "new Proxy([1, 2], {})",
    "{ then() { log.push('then') }, length: 1, 0: 'x' }", "[{ then(r) { r('thenable') } }]", "[{ then(r, j) { j(new Error('tj')) } }]", "[{ then() { throw new Error('tt') } }]",
    "[[1], [2]]", "[undefined, null]", "[0, -0, NaN]",
  ];
  const mapFns = ["undefined", "x => x", "x => x + 1", "async x => x * 2", "x => Promise.resolve(x)", "(x, i) => [x, i]", "x => { throw new Error('mf') }", "async x => { throw new Error('amf') }", "x => Promise.reject(new Error('mr'))", "null", "1", "{}", "x => ({ then(r) { r('mt') } })"];
  for (const item of items) {
    A(`return Array.fromAsync(${item})`);
    A(`return Array.fromAsync(${item}, x => x)`);
  }
  for (const fn of mapFns) {
    A(`return Array.fromAsync([1, 2, 3], ${fn})`);
    A(`return Array.fromAsync(new Set([1, 2]), ${fn})`);
    A(`return Array.fromAsync({ length: 2, 0: 1, 1: 2 }, ${fn})`);
    A(`return Array.fromAsync((async function* () { yield 1; yield 2 })(), ${fn})`);
  }
  A("return Array.fromAsync([1, 2], function () { return this }, 'ctx')");
  A("return Array.fromAsync([1, 2], function () { 'use strict'; return typeof this }, 'ctx')");
  A("return Array.fromAsync([1, 2], function () { 'use strict'; return this })");
  A("return Array.fromAsync([1, 2], (x, i) => i)");
  A("return Array.fromAsync([1, 2], function () { return arguments.length })");
  A("return Array.fromAsync([1, 2], x => x, { a: 1 })");
  A("var p = Array.fromAsync([1]); log.push(p instanceof Promise); return p");
  A("log.push('a'); var p = Array.fromAsync([1, 2]); log.push('b'); return p.then(v => { log.push('c'); return v })");
  A("log.push('sync'); Promise.resolve().then(() => log.push('p1')); var p = Array.fromAsync([]); return p.then(() => { log.push('done') })");
  A("var p = Array.fromAsync([1, 2, 3]); Promise.resolve().then(() => log.push('t1')).then(() => log.push('t2')).then(() => log.push('t3')).then(() => log.push('t4')); return p.then(v => { log.push('res'); return v })");
  A("function C() { log.push('ctor'); this.tag = 'C' } return Array.fromAsync.call(C, [1, 2])");
  A("function C(n) { log.push('ctor' + n); this.tag = 'C' } return Array.fromAsync.call(C, { length: 2, 0: 'a', 1: 'b' })");
  A("function C(n) { log.push('ctor' + n); this.tag = 'C' } return Array.fromAsync.call(C, { length: 2, 0: 'a', 1: 'b' }, x => x + x)");
  A("class C extends Array {} return Array.fromAsync.call(C, [1, 2]).then(r => [r instanceof C, r.length])");
  A("return Array.fromAsync.call(Object, [1, 2])");
  A("return Array.fromAsync.call(undefined, [1, 2])");
  A("return Array.fromAsync.call(null, [1, 2])");
  A("return Array.fromAsync.call(1, [1, 2])");
  A("return Array.fromAsync.call({}, [1, 2])");
  A("return Array.fromAsync.call(() => {}, [1, 2])");
  A("return Array.fromAsync.call(Math.max, [1, 2])");
  A("return Array.fromAsync.call(function () { return Object.freeze({}) }, [1, 2])");
  A("return Array.fromAsync.call(function () { return 5 }, [1, 2])");
  A("var closed = 0; var it = { [Symbol.asyncIterator]() { return { i: 0, next() { return Promise.resolve({ done: this.i > 2, value: this.i++ }) }, return() { log.push('ret'); return Promise.resolve({}) } } } }; return Array.fromAsync(it, x => { if (x === 1) throw new Error('stop'); return x })");
  A("var it = { [Symbol.asyncIterator]() { return { i: 0, next() { log.push('n'); return Promise.resolve({ done: this.i > 1, value: this.i++ }) }, return() { log.push('ret'); return Promise.resolve({}) } } } }; return Array.fromAsync(it)");
  A("var it = { [Symbol.iterator]() { return { i: 0, next() { log.push('n'); return { done: this.i > 1, value: Promise.resolve(this.i++) } }, return() { log.push('ret'); return {} } } } }; return Array.fromAsync(it, x => { if (x === 1) throw new Error('s'); return x })");
  A("var it = { [Symbol.iterator]() { return { i: 0, next() { log.push('n'); return { done: this.i > 1, value: Promise.reject(new Error('v' + this.i++)) } }, return() { log.push('ret'); return {} } } } }; return Array.fromAsync(it)");
  A("var it = { [Symbol.asyncIterator]() { return { next() { log.push('n'); return Promise.reject(new Error('nx')) }, return() { log.push('ret'); return Promise.resolve({}) } } } }; return Array.fromAsync(it)");
  A("var order = []; var src = { get length() { order.push('length'); return 2 }, get 0() { order.push('0'); return 'a' }, get 1() { order.push('1'); return 'b' } }; return Array.fromAsync(src).then(v => [v, order])");
  A("var order = []; var src = { get [Symbol.asyncIterator]() { order.push('ai'); return undefined }, get [Symbol.iterator]() { order.push('si'); return undefined }, length: 0 }; return Array.fromAsync(src).then(v => [v, order])");
  A("var order = []; var src = { get [Symbol.asyncIterator]() { order.push('ai'); return function () { return (async function* () {})() } }, get [Symbol.iterator]() { order.push('si'); return undefined } }; return Array.fromAsync(src).then(v => [v, order])");
  A("return Array.fromAsync([3, 2, 1].map(n => new Promise(r => r(n))))");
  A("return Array.fromAsync([1, 2, 3].map(n => Promise.resolve(n).then(x => x * 2)))");
  A("var a = [1, 2]; var p = Array.fromAsync(a); a.push(3); return p");
  A("var a = [1, 2]; var p = Array.fromAsync(a, x => { if (x === 1) a.push(9); return x }); return p");
  A("return Array.fromAsync([1, 2, 3], x => x === 2 ? Promise.reject(1) : x).catch(e => 'caught ' + e)");
  A("return Array.fromAsync(new Array(3))");
  A("return Array.fromAsync([, 1])");
  A("return Array.fromAsync({ length: 3, 1: 'x' })");
  A("return Array.fromAsync({ length: 2 ** 32 })");
  A("return Array.fromAsync({ length: 2 ** 53 })");
  A("return Array.fromAsync('😀a')");
  A("return Array.fromAsync(new String('xy'))");
  A("return Array.fromAsync(new Uint8Array([1, 2]))");
  A("return Array.fromAsync(new Uint8Array([1, 2]), x => x * 2)");
  A("return Array.fromAsync(arguments_like())");
  A("function f() { return Array.fromAsync(arguments) } return f(1, 2)");
  A("return [Array.fromAsync.name, Array.fromAsync.length, Object.getOwnPropertyDescriptor(Array, 'fromAsync').enumerable]");
  A("return Reflect.construct(Array.fromAsync, [])");
  A("return Array.fromAsync()");
  A("return Array.fromAsync([1, 2], undefined, undefined)");
}

// ---- Promise.withResolvers e Promise.try.
if (has("Promise.withResolvers")) {
  E("(() => { var r = Promise.withResolvers(); return [Object.keys(r), Object.getPrototypeOf(r) === Object.prototype, r.promise instanceof Promise, typeof r.resolve, typeof r.reject] })()");
  E("(() => { var r = Promise.withResolvers(); return [r.resolve.name, r.resolve.length, r.reject.name, r.reject.length] })()");
  E("(() => { var d = Promise.withResolvers(); return [Object.getOwnPropertyNames(d.resolve).sort(), d.resolve.hasOwnProperty('prototype')] })()");
  E("Object.getOwnPropertyDescriptor(Promise, 'withResolvers')");
  E("[Promise.withResolvers.name, Promise.withResolvers.length]");
  E("Promise.withResolvers.call(undefined)");
  E("Promise.withResolvers.call(1)");
  E("Promise.withResolvers.call({})");
  E("Promise.withResolvers.call(() => {})");
  E("Promise.withResolvers.call(function () {})");
  E("Promise.withResolvers.call(function (ex) { ex(() => {}, () => {}) }).promise");
  E("Reflect.construct(Promise.withResolvers, [])");
  E("(() => { class P extends Promise {} var r = P.withResolvers(); return [r.promise instanceof P, r.promise.constructor === P] })()");
  E("(() => { var seen = []; function P(ex) { seen.push(typeof ex); ex(function res() {}, function rej() {}) } var r = Promise.withResolvers.call(P); return [seen, r.promise instanceof P] })()");
  E("(() => { function P(ex) { ex(undefined, undefined) } return Promise.withResolvers.call(P) })()");
  E("(() => { function P(ex) { ex(() => {}, undefined) } return Promise.withResolvers.call(P) })()");
  E("(() => { function P(ex) { ex(() => {}, () => {}); ex(() => {}, () => {}) } return Promise.withResolvers.call(P) })()");
  E("(() => { function P(ex) { ex(() => {}, () => {}); ex(1, 2) } return Promise.withResolvers.call(P) })()");
  E("(() => { function P(ex) { ex(1, 2) } return Promise.withResolvers.call(P) })()");
  E("(() => { function P(ex) { throw new Error('ctor') } return Promise.withResolvers.call(P) })()");
  A("var r = Promise.withResolvers(); r.resolve(5); return r.promise");
  A("var r = Promise.withResolvers(); r.reject(new Error('no')); return r.promise");
  A("var r = Promise.withResolvers(); r.resolve(1); r.resolve(2); r.reject(3); return r.promise");
  A("var r = Promise.withResolvers(); r.reject(1); r.resolve(2); return r.promise.catch(e => 'c' + e)");
  A("var r = Promise.withResolvers(); r.resolve(Promise.resolve('inner')); return r.promise");
  A("var r = Promise.withResolvers(); r.resolve(Promise.reject(new Error('inner'))); return r.promise");
  A("var r = Promise.withResolvers(); r.resolve({ then(f) { f('th') } }); return r.promise");
  A("var r = Promise.withResolvers(); r.resolve({ then() { throw new Error('thx') } }); return r.promise");
  A("var r = Promise.withResolvers(); r.resolve(r.promise); return r.promise");
  A("var r = Promise.withResolvers(); var f = r.resolve; f(7); return r.promise");
  A("var r = Promise.withResolvers(); var { resolve, reject } = r; log.push(resolve() === undefined, reject === r.reject); return r.promise");
  A("var r = Promise.withResolvers(); r.promise.then(v => log.push('then' + v)); r.resolve(1); log.push('sync'); return r.promise.then(() => log.join())");
  A("var r = Promise.withResolvers(); r.resolve.call(null, 9); return r.promise");
  A("var a = Promise.withResolvers(), b = Promise.withResolvers(); a.resolve(1); return [a.resolve === b.resolve, a.promise === b.promise]");
  A("var r = Promise.withResolvers(); return Promise.race([r.promise, 'fast'])");
  A("var r = Promise.withResolvers(); r.resolve(1); return Promise.all([r.promise, 2])");
  A("class P extends Promise { static get [Symbol.species]() { return Promise } } var r = P.withResolvers(); r.resolve(1); return [r.promise instanceof P, r.promise.then(() => {}) instanceof P]");
}
if (has("Promise.try")) {
  E("[Promise.try.name, Promise.try.length]");
  E("Object.getOwnPropertyDescriptor(Promise, 'try')");
  E("Promise.try() instanceof Promise");
  E("Promise.try.call(undefined, () => 1)");
  E("Promise.try.call(1, () => 1)");
  E("Promise.try.call({}, () => 1)");
  E("Promise.try.call(() => {}, () => 1)");
  E("Reflect.construct(Promise.try, [])");
  E("(() => { class P extends Promise {} var p = P.try(() => 1); return [p instanceof P, p.constructor === P] })()");
  E("(() => { var seen = []; function P(ex) { seen.push(typeof ex); ex(() => {}, () => {}) } var p = Promise.try.call(P, () => 1); return [seen, p instanceof P] })()");
  A("return Promise.try(() => 1)");
  A("return Promise.try(() => { throw new Error('sync') })");
  A("return Promise.try(() => Promise.resolve('p'))");
  A("return Promise.try(() => Promise.reject(new Error('pr')))");
  A("return Promise.try(() => ({ then(f) { f('thenable') } }))");
  A("return Promise.try(() => undefined)");
  A("return Promise.try(() => { log.push('cb') }).then(() => log.push('after')) && log.push('sync')");
  A("log.push('before'); var p = Promise.try(() => { log.push('cb') }); log.push('after'); return p");
  A("return Promise.try(function () { return [this === undefined, typeof this, arguments.length] })");
  A("return Promise.try(function () { 'use strict'; return [this, arguments.length] }, 1, 2, 3)");
  A("return Promise.try((a, b, c) => [a, b, c], 1, 2)");
  A("return Promise.try((...r) => r, 'x', undefined, null)");
  A("return Promise.try(1)");
  A("return Promise.try(null)");
  A("return Promise.try({})");
  A("return Promise.try('s')");
  A("return Promise.try(undefined)");
  A("return Promise.try(class {})");
  A("return Promise.try(class { constructor() { } })");
  A("return Promise.try(function* () { yield 1 }).then(g => g.next())");
  A("return Promise.try(async () => 'a')");
  A("return Promise.try(async () => { throw new Error('async') })");
  A("return Promise.try(Math.max, 1, 5, 3)");
  A("return Promise.try(Array, 3)");
  A("return Promise.try(Symbol, 'd').then(s => typeof s)");
  A("return Promise.try(function () { throw 5 }).catch(e => 'c' + e)");
  A("return Promise.try(() => { throw undefined }).catch(e => e)");
  A("var p = Promise.try(() => 1); return [p instanceof Promise, p.then(() => {}) instanceof Promise]");
  A("var r = Promise.try(() => 1); return r.then(v => v + 1).then(v => v * 10)");
  A("return Promise.all([Promise.try(() => 1), Promise.try(() => { throw 2 }).catch(e => e)])");
  A("return Promise.try(() => ({ get then() { throw new Error('getter') } }))");
  A("var p = Promise.resolve(1); return Promise.try(() => p).then(v => [v, 'x'])");
  A("var p = Promise.resolve(1); return Promise.try(() => p) === p");
  A("var order = []; Promise.resolve().then(() => order.push('m1')); var t = Promise.try(() => { order.push('try') }); Promise.resolve().then(() => order.push('m2')); return t.then(() => order)");
  A("var order = []; Promise.try(() => Promise.resolve(1)).then(() => order.push('a')); Promise.resolve().then(() => order.push('b')).then(() => order.push('c')).then(() => order.push('d')); return new Promise(r => Promise.resolve().then().then().then().then().then(() => r(order)))");
  A("function f() { return Promise.try.apply(this, arguments) } return f.call(Promise, () => 1)");
  A("return Promise.try.bind(Promise)(() => 'bound')");
  A("var t = Promise.try; return t.call(Promise, () => 'detached')");
  A("var t = Promise.try; return t(() => 'unbound')");
}

// ---- Error.isError.
if (has("Error.isError")) {
  const vals = [
    "new Error()", "new TypeError()", "new RangeError('x')", "new SyntaxError()", "new ReferenceError()", "new EvalError()", "new URIError()", "new AggregateError([])",
    "Error('call')", "TypeError('call')", "class E extends Error {}; new E()", "class E2 extends TypeError {}; new E2()", "Object.create(Error.prototype)", "{ __proto__: Error.prototype }",
    "{ name: 'Error', message: 'm', stack: 's' }", "Error.prototype", "TypeError.prototype", "Error", "TypeError", "Error.prototype.constructor",
    "null", "undefined", "1", "'Error'", "Symbol()", "true", "1n", "{}", "[]", "function () {}", "new Proxy(new Error(), {})", "new Proxy({}, {})", "Object.setPrototypeOf(new Error(), null)",
    "Object.setPrototypeOf(new Error(), Array.prototype)", "Object.assign(new Error('a'), { name: 'Z' })", "(() => { var e = new Error(); e[Symbol.toStringTag] = 'Foo'; return e })()",
    "(() => { var e = {}; e[Symbol.toStringTag] = 'Error'; return e })()", "Reflect.construct(Error, [], Object)", "Reflect.construct(Object, [], Error)", "Reflect.construct(Array, [], Error)",
    "Object.freeze(new Error())", "Object.seal(new Error())", "new Error('x', { cause: 1 })", "(() => { try { null.x } catch (e) { return e } })()", "(() => { try { undefinedVar } catch (e) { return e } })()",
    "(() => { try { JSON.parse('{') } catch (e) { return e } })()", "(() => { try { new Array(-1) } catch (e) { return e } })()", "(() => { try { decodeURIComponent('%') } catch (e) { return e } })()",
    "(() => { try { eval('+') } catch (e) { return e } })()", "(() => { try { 1n + 1 } catch (e) { return e } })()", "(() => { try { Symbol() + '' } catch (e) { return e } })()",
    "Promise.reject(1) && new Error()", "Object(new Error())", "new (class extends Error { constructor() { super(); return {} } })()", "new (class { constructor() { return new Error() } })()",
    "new DOMExceptionLike()", "Object.create(new Error())", "Object.create(TypeError.prototype)", "new Error().stack", "Object.getOwnPropertyDescriptors(new Error('m'))",
  ];
  E("[Error.isError.name, Error.isError.length]");
  E("Object.getOwnPropertyDescriptor(Error, 'isError')");
  E("Error.isError()");
  E("Error.isError.call(null, new Error())");
  E("Error.isError.call(undefined, 1)");
  E("Reflect.construct(Error.isError, [])");
  E("new Error.isError()");
  for (const v of vals) E(`Error.isError(${v})`);
  E("[Error, TypeError, RangeError, SyntaxError, ReferenceError, EvalError, URIError, AggregateError].map(C => Error.isError(new C()))");
  E("[Error, TypeError, RangeError, SyntaxError, ReferenceError, EvalError, URIError].map(C => Error.isError(C.prototype))");
  E("(() => { var r = Error.isError; return r(new Error()) })()");
  E("Error.isError(new Error(), 1, 2)");
  E("Error.isError(Object.defineProperty(new Error(), Symbol.toStringTag, { value: 'Array' }))");
  E("(() => { var e = new Error(); Object.setPrototypeOf(e, Object.prototype); return Error.isError(e) })()");
  E("(() => { var e = new Error(); delete e.stack; return Error.isError(e) })()");
}

// ---- Uint8Array base64 e hex.
if (has("Uint8Array.fromBase64")) {
  const b64 = ["''", "'AA=='", "'AAE='", "'AAEC'", "'SGVsbG8='", "'SGVsbG8'", "'SGVsbG8=='", "'SGVsb G8='", "' SGVsbG8= '", "'SGVs\\nbG8='", "'SGVs\\tbG8='", "'SGVs\\u00a0bG8='", "'+/8='", "'-_8='", "'+/8'", "'-_8'",
    "'A'", "'AA'", "'AAA'", "'AAAA'", "'AAAAA'", "'AB=='", "'AAB='", "'AA=A'", "'=AAA'", "'AAA='", "'AA==AA'", "'A==='", "'===='", "'!!!!'", "'AAA\\u0100'", "'/w=='", "'_w=='", "'/w'", "'TWE='", "'TQ=='", "'TQ'", "'TWE'", "'TWFu'", "'TWFuTQ=='", "5", "null", "undefined", "{}", "['AA==']", "new String('AA==')"];
  const alphabets = ["undefined", "'base64'", "'base64url'", "'other'", "null", "1", "''", "'BASE64'"];
  const lasts = ["undefined", "'loose'", "'strict'", "'stop-before-partial'", "'x'", "null", "1"];
  const hex = ["''", "'00'", "'ff'", "'FF'", "'0aFf'", "'abc'", "'a'", "'zz'", "'0g'", "'0x00'", "' 00'", "'00 '", "'00ff00ff'", "5", "null", "undefined", "'é0'", "'00\\u0000'", "'\\uff10\\uff10'"];
  for (const s of b64) {
    E(`Uint8Array.fromBase64(${s})`);
    E(`Uint8Array.fromBase64(${s}, { lastChunkHandling: 'strict' })`);
    E(`Uint8Array.fromBase64(${s}, { lastChunkHandling: 'stop-before-partial' })`);
    E(`Uint8Array.fromBase64(${s}, { alphabet: 'base64url' })`);
    E(`new Uint8Array(8).setFromBase64(${s})`);
    E(`new Uint8Array(2).setFromBase64(${s})`);
    E(`new Uint8Array(2).setFromBase64(${s}, { lastChunkHandling: 'stop-before-partial' })`);
    E(`new Uint8Array(0).setFromBase64(${s})`);
  }
  for (const a of alphabets) {
    E(`Uint8Array.fromBase64('+/8=', { alphabet: ${a} })`);
    E(`Uint8Array.fromBase64('-_8=', { alphabet: ${a} })`);
    E(`new Uint8Array([251, 255]).toBase64({ alphabet: ${a} })`);
  }
  for (const l of lasts) {
    E(`Uint8Array.fromBase64('SGVsbG8', { lastChunkHandling: ${l} })`);
    E(`Uint8Array.fromBase64('SGVsbG8=', { lastChunkHandling: ${l} })`);
    E(`Uint8Array.fromBase64('SGVsbA', { lastChunkHandling: ${l} })`);
    E(`Uint8Array.fromBase64('SGVsbB==', { lastChunkHandling: ${l} })`);
    E(`Uint8Array.fromBase64('SGVs bA', { lastChunkHandling: ${l} })`);
  }
  for (const o of ["null", "1", "'x'", "[]", "{ alphabet: undefined, lastChunkHandling: undefined }", "{ get alphabet() { log.push('alpha'); return 'base64' }, get lastChunkHandling() { log.push('last'); return 'loose' } }"]) {
    B(`return Uint8Array.fromBase64('AA==', ${o})`);
  }
  E("new Uint8Array([72, 101, 108, 108, 111]).toBase64()");
  E("new Uint8Array([]).toBase64()");
  E("new Uint8Array([0]).toBase64()");
  E("new Uint8Array([0, 0]).toBase64()");
  E("new Uint8Array([0, 0, 0]).toBase64()");
  E("new Uint8Array([255, 255, 255, 255]).toBase64()");
  E("new Uint8Array([255, 255, 255, 255]).toBase64({ omitPadding: true })");
  E("new Uint8Array([255, 255]).toBase64({ omitPadding: true, alphabet: 'base64url' })");
  E("new Uint8Array([255]).toBase64({ omitPadding: false })");
  E("new Uint8Array([255]).toBase64({ omitPadding: 'yes' })");
  E("new Uint8Array([255]).toBase64({ omitPadding: 0 })");
  E("new Uint8Array([255]).toBase64({ omitPadding: undefined })");
  E("new Uint8Array(100).fill(255).toBase64().length");
  E("new Uint8Array(3).fill(250).toBase64({ alphabet: 'base64url' })");
  E("Uint8Array.prototype.toBase64.call([1])");
  E("Uint8Array.prototype.toBase64.call(new Uint16Array([1]))");
  E("Uint8Array.prototype.toBase64.call(new Int8Array([1]))");
  E("Uint8Array.prototype.toBase64.call(new Uint8ClampedArray([1]))");
  E("Uint8Array.prototype.toBase64.call(new ArrayBuffer(1))");
  E("Uint8Array.prototype.toBase64.call(null)");
  E("(() => { var u = new Uint8Array([1, 2, 3]); var ab = u.buffer; ab.transfer?.(); return u.toBase64() })()");
  E("(() => { var u = new Uint8Array(new ArrayBuffer(4, { maxByteLength: 8 })); return u.toBase64() })()");
  E("new Uint8Array(new ArrayBuffer(8), 2, 3).toBase64()");
  E("(() => { var u = new Uint8Array(4); var r = u.setFromBase64('AQID'); return [r, u] })()");
  E("(() => { var u = new Uint8Array(2); var r = u.setFromBase64('AQIDBA=='); return [r, u] })()");
  E("(() => { var u = new Uint8Array(4); var r = u.setFromBase64('AQ'); return [r, u] })()");
  E("(() => { var u = new Uint8Array(4); var r = u.setFromBase64('AQ=='); return [r, u] })()");
  E("(() => { var u = new Uint8Array(4); var r = u.setFromBase64('AQ', { lastChunkHandling: 'stop-before-partial' }); return [r, u] })()");
  E("(() => { var u = new Uint8Array(4); var r = u.setFromBase64('AQIDBA', { lastChunkHandling: 'stop-before-partial' }); return [r, u] })()");
  E("(() => { var u = new Uint8Array(4); try { u.setFromBase64('AQ$$') } catch (e) { return [e.name, u] } })()");
  E("(() => { var u = new Uint8Array(4); try { u.setFromBase64('AQIDBAU$') } catch (e) { return [e.name, u] } })()");
  E("(() => { var u = new Uint8Array(3); try { u.setFromBase64('AQIDBA$$') } catch (e) { return [e.name, u] } })()");
  E("Uint8Array.prototype.setFromBase64.call(new Uint16Array(2), 'AA==')");
  E("Uint8Array.prototype.setFromBase64.call({}, 'AA==')");
  E("Uint8Array.fromBase64.call(Array, 'AA==')");
  E("Uint8Array.fromBase64.call(undefined, 'AA==')");
  E("Uint8Array.fromBase64.call(class extends Uint8Array {}, 'AA==') instanceof Uint8Array");
  E("Object.getPrototypeOf(Uint8Array.fromBase64('AA=='))  === Uint8Array.prototype");
  E("[Uint8Array.fromBase64.name, Uint8Array.fromBase64.length, Uint8Array.prototype.toBase64.name, Uint8Array.prototype.toBase64.length, Uint8Array.prototype.setFromBase64.name, Uint8Array.prototype.setFromBase64.length]");
  E("typeof Uint16Array.fromBase64 + typeof Uint16Array.prototype.toBase64");
  E("Object.getOwnPropertyDescriptor(Uint8Array, 'fromBase64')");
  E("Object.getOwnPropertyDescriptor(Uint8Array.prototype, 'toBase64')");
  // Ida e volta.
  E("[0, 1, 2, 3, 4, 5, 6, 7].map(n => Uint8Array.fromBase64(new Uint8Array(n).fill(n * 31).toBase64()).join(''))");
  E("Uint8Array.fromBase64(new Uint8Array(256).map((_, i) => i).toBase64()).every((v, i) => v === i)");
  E("Uint8Array.fromBase64(new Uint8Array(256).map((_, i) => i).toBase64({ alphabet: 'base64url', omitPadding: true }), { alphabet: 'base64url' }).every((v, i) => v === i)");
  E("Uint8Array.fromBase64(new Uint8Array(256).map((_, i) => i).toBase64({ alphabet: 'base64url', omitPadding: true }), { alphabet: 'base64url', lastChunkHandling: 'strict' })");
  E("new Uint8Array(256).map((_, i) => i).toBase64().slice(0, 20)");
  E("new Uint8Array(256).map((_, i) => i).toBase64({ alphabet: 'base64url' }).slice(-12)");
}
if (has("Uint8Array.fromHex")) {
  const hex = ["''", "'00'", "'ff'", "'FF'", "'0aFf'", "'abc'", "'a'", "'zz'", "'0g'", "'0x00'", "' 00'", "'00 '", "'00ff00ff'", "5", "null", "undefined", "'é0'", "'00\\u0000'", "'\\uff10\\uff10'", "'DEADbeef'", "['00']", "new String('0a')", "'0'.repeat(64)"];
  for (const h of hex) {
    E(`Uint8Array.fromHex(${h})`);
    E(`new Uint8Array(8).setFromHex(${h})`);
    E(`new Uint8Array(1).setFromHex(${h})`);
    E(`new Uint8Array(0).setFromHex(${h})`);
  }
  E("new Uint8Array([]).toHex()");
  E("new Uint8Array([0, 1, 15, 16, 255]).toHex()");
  E("new Uint8Array(256).map((_, i) => i).toHex().length");
  E("new Uint8Array(256).map((_, i) => i).toHex().slice(-8)");
  E("Uint8Array.fromHex(new Uint8Array(256).map((_, i) => i).toHex()).every((v, i) => v === i)");
  E("Uint8Array.prototype.toHex.call([1])");
  E("Uint8Array.prototype.toHex.call(new Uint16Array([1]))");
  E("Uint8Array.prototype.toHex.call(new Int8Array([-1]))");
  E("Uint8Array.prototype.toHex.call(null)");
  E("Uint8Array.prototype.toHex.call(new DataView(new ArrayBuffer(1)))");
  E("(() => { var u = new Uint8Array(3); var r = u.setFromHex('0a0b0c0d'); return [r, u] })()");
  E("(() => { var u = new Uint8Array(3); var r = u.setFromHex('0a0b'); return [r, u] })()");
  E("(() => { var u = new Uint8Array(3); try { u.setFromHex('0a0') } catch (e) { return [e.name, u] } })()");
  E("(() => { var u = new Uint8Array(3); try { u.setFromHex('0a0z') } catch (e) { return [e.name, u] } })()");
  E("(() => { var u = new Uint8Array(3); try { u.setFromHex('0azz0b') } catch (e) { return [e.name, u] } })()");
  E("Uint8Array.prototype.setFromHex.call(new Uint16Array(2), '00')");
  E("Uint8Array.fromHex.call(Array, '00')");
  E("Uint8Array.fromHex.call(undefined, '00')");
  E("[Uint8Array.fromHex.name, Uint8Array.fromHex.length, Uint8Array.prototype.toHex.name, Uint8Array.prototype.toHex.length, Uint8Array.prototype.setFromHex.name, Uint8Array.prototype.setFromHex.length]");
  E("Object.getOwnPropertyDescriptor(Uint8Array, 'fromHex')");
  E("Object.getOwnPropertyDescriptor(Uint8Array.prototype, 'toHex')");
  E("Object.getOwnPropertyDescriptor(Uint8Array.prototype, 'setFromHex')");
}

// ---- RegExp.escape.
if (has("RegExp.escape")) {
  const strs = [
    "''", "'a'", "'abc'", "'1'", "'123'", "'a1'", "'_'", "'a_b'", "' '", "'a b'", "'-'", "'a-b'", "','", "'a,b'", "'='", "'<'", "'>'", "'#'", "'&'", "'!'", "'%'", "':'", "';'", "'@'", "'~'", "'`'", "'\"'", "'\\''",
    "'.'", "'*'", "'+'", "'?'", "'^'", "'$'", "'{'", "'}'", "'('", "')'", "'|'", "'['", "']'", "'\\\\'", "'/'", "'a.b*c'", "'^$'", "'[a-z]+'", "'(x|y)'", "'\\\\d'", "'\\n'", "'\\t'", "'\\r'", "'\\v'", "'\\f'", "'\\0'",
    "'\\u2028'", "'\\u2029'", "'\\ufeff'", "'\\u00a0'", "'\\u3000'", "'\\u1680'", "'\\u2000'", "'\\u200b'", "'\\u180e'", "'é'", "'日本'", "'😀'", "'\\ud83d'", "'\\ude00'", "'\\ud83d\\ude00'", "'a\\ud83d'", "'\\ude00a'",
    "'1a'", "'a1b2'", "'0xff'", "'\\x7f'", "'\\x00'", "'\\x1f'", "'~!@#$%^&*()_+-={}[]|:;\"<>,.?/'", "'hello world'", "'a\\nb'", "'\\u0085'", "'\\u200e'", "'\\u200f'", "'\\ud800'", "'\\udc00'", "'\\udc00\\ud800'",
    "'A'", "'z'", "'Z'", "'9'", "'0'", "'a'.repeat(5)", "'.'.repeat(5)", "'1'.repeat(3)",
  ];
  for (const s of strs) {
    E(`RegExp.escape(${s})`);
    E(`new RegExp(RegExp.escape(${s})).test(${s})`);
    E(`new RegExp('^' + RegExp.escape(${s}) + '$', 'v').test(${s})`);
  }
  for (const bad of ["1", "null", "undefined", "{}", "[]", "Symbol()", "true", "new String('a')", "{ toString() { return 'a' } }", "['a']", "1n", "function () {}"]) E(`RegExp.escape(${bad})`);
  E("RegExp.escape()");
  E("[RegExp.escape.name, RegExp.escape.length]");
  E("Object.getOwnPropertyDescriptor(RegExp, 'escape')");
  E("RegExp.escape.call(null, 'a.b')");
  E("RegExp.escape.call(undefined, '*')");
  E("Reflect.construct(RegExp.escape, [])");
  E("Array.from('^$\\\\.*+?()[]{}|/').map(c => RegExp.escape(c)).join(' ')");
  E("'a.b.c'.split(new RegExp(RegExp.escape('.'))).length");
  E("'x+y'.replace(new RegExp(RegExp.escape('+'), 'g'), '-')");
  E("'1+1=2'.match(new RegExp(RegExp.escape('1+1'))).index");
  E("new RegExp(RegExp.escape('a-b'), 'v').test('a-b')");
  E("new RegExp('[' + RegExp.escape('a-z') + ']', 'u').test('-')");
  E("new RegExp('[' + RegExp.escape('^-]') + ']').test('^')");
  E("new RegExp(RegExp.escape('\\n')).test('\\n')");
  E("(() => { var all = ''; for (var i = 0; i < 128; i++) all += RegExp.escape(String.fromCharCode(i)) + '|'; return all })()");
  E("(() => { var out = []; for (var i = 0x2000; i <= 0x200f; i++) out.push(RegExp.escape(String.fromCharCode(i)).length); return out })()");
  E("(() => { var bad = []; for (var i = 0; i < 256; i++) { var c = String.fromCharCode(i); var re = new RegExp('^' + RegExp.escape(c) + '$', 'u'); if (!re.test(c)) bad.push(i) } return bad })()");
  E("(() => { var bad = []; for (var i = 0; i < 256; i++) { var c = String.fromCharCode(i); var re = new RegExp('^' + RegExp.escape(c) + '$', 'v'); if (!re.test(c)) bad.push(i) } return bad })()");
  E("RegExp.escape('x'.repeat(1000)).length");
  E("RegExp.escape('.'.repeat(1000)).length");
}

// ---- Execução (o bun passa arquivo pelo transpilador próprio; vm.runInThisContext roda como script puro do JSC).
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "esnext-golden-"));
const source_file = path.join(dir, "esnext_source.js");
const file = path.join(dir, "esnext_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) { globalThis.R = 'sync-throw ' + (e && e.name) + ': ' + (e && e.message) }\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
let kept = 0;
let dropped = 0;
const programs = [...new Set(pool.resolve())];
for (const body of programs) {
  const source = PRELUDE + " " + body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("esnext", rows));
fs.rmSync(dir, { recursive: true, force: true });
