// Gera tests/golden/performance_bun.tsv: o núcleo de `performance` (e `Performance`, `PerformanceEntry`,
// `PerformanceMark`, `PerformanceMeasure`) medido no bun 1.4.2: descritores no global, forma dos protótipos e
// construtores, erros exatos, `mark`, `measure`, `getEntries*`, `clear*`, `toJSON`, `this` inválido.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Valores de tempo dependem da máquina e do instante: entram só por tipo e por relação (monotonicidade, ordem,
// diferença contra `Date.now`), nunca pelo número. Cada programa roda num processo `bun` próprio.
// Entram também `timing` (e `PerformanceTiming`), `onresourcetimingbufferfull`, `clearResourceTimings`,
// `setResourceTimingBufferSize`, `markResourceTiming` e o `timing` dentro de `toJSON()`.
// Entra também a herança de `EventTarget` (cadeia de protótipos, chaves de cada nível, `dispatchEvent`).
// Uso: bun scripts/gen-performance-golden.js > tests/golden/performance_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v === undefined) return 'undefined'; if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code };\n" +
  "var D = function (o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x && [typeof x.value, x.writable, x.enumerable, x.configurable, typeof x.get, typeof x.set] };\n" +
  "var P = Performance.prototype, EP = PerformanceEntry.prototype, MP = PerformanceMark.prototype, XP = PerformanceMeasure.prototype;\n" +
  "var G = function (o, k) { return Object.getOwnPropertyDescriptor(o, k).get };\n" +
  "var C = function () { performance.clearMarks(); performance.clearMeasures() };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const stmts = (code) => programs.push(HELPER + `try { ${code} } catch (e) { R = E(e) }`);
const strict = (code) => programs.push(HELPER + `try { (function () { 'use strict'; ${code} })() } catch (e) { R = E(e) }`);

// Globais: descritor, tipo, posição na ordem de chaves.
for (const n of ["performance", "Performance", "PerformanceEntry", "PerformanceMark", "PerformanceMeasure"]) {
  expr(`D(globalThis, '${n}')`);
  expr(`typeof ${n}`);
  expr(`Object.prototype.hasOwnProperty.call(globalThis, '${n}')`);
  expr(`Object.keys(globalThis).indexOf('${n}') >= 0`);
}
expr("Object.getOwnPropertyNames(globalThis).indexOf('performance') - Object.getOwnPropertyNames(globalThis).indexOf('navigator')");
expr("Object.getOwnPropertyNames(globalThis).indexOf('Performance') - Object.getOwnPropertyNames(globalThis).indexOf('MessagePort')");
expr("Object.getOwnPropertyNames(globalThis).filter(function (k) { return /^Performance(Entry|Mark|Measure)?$/.test(k) })");
stmts("var i = Object.getOwnPropertyNames(globalThis).indexOf('performance'); performance = 5; R = S([typeof performance, Object.getOwnPropertyNames(globalThis).indexOf('performance') - i])");
stmts("R = S([delete globalThis.performance, typeof performance])");
stmts("var f = function () { return performance.now() }; R = S(typeof f())");

// A instância `performance`.
expr("Object.getPrototypeOf(performance) === Performance.prototype");
expr("performance instanceof Performance");
expr("Reflect.ownKeys(performance).map(String)");
expr("Object.keys(performance)");
expr("D(performance, 'now')");
expr("[performance.now.name, performance.now.length, 'prototype' in performance.now, performance.now.toString()]");
expr("Object.getOwnPropertyNames(performance.now)");
expr("Object.getPrototypeOf(performance.now) === Function.prototype");
expr("typeof Performance.prototype.now");
expr("String(performance)");
expr("Object.prototype.toString.call(performance)");
expr("Object.prototype.toString.call(Performance.prototype)");
expr("D(P, Symbol.toStringTag)");
expr("P[Symbol.toStringTag]");
expr("performance.constructor === Performance");

// Herança de `EventTarget`.
expr("performance instanceof EventTarget");
expr("Object.getPrototypeOf(Performance) === EventTarget");
expr("Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype");
expr("Object.getPrototypeOf(EventTarget.prototype) === Object.prototype");
expr("Reflect.ownKeys(Performance.prototype).map(String)");
expr("Reflect.ownKeys(EventTarget.prototype).map(String)");
expr("Reflect.ownKeys(Performance).map(String)");
expr("[Performance.length, Performance.name]");
expr("Object.keys(Performance.prototype)");
expr("['addEventListener', 'removeEventListener', 'dispatchEvent'].map(function (k) { return [typeof performance[k], k in performance, Object.prototype.hasOwnProperty.call(performance, k), performance[k] === EventTarget.prototype[k]] })");
expr("[performance.addEventListener.length, performance.removeEventListener.length, performance.dispatchEvent.length]");
stmts("var L = []; var f = function (e) { L.push(e.type, e.target === performance, e.currentTarget === performance, e.isTrusted, this === performance) }; performance.addEventListener('foo', f); var r = performance.dispatchEvent(new Event('foo')); R = S([r, L])");
stmts("var L = []; var f = function (e) { L.push(e.type); e.preventDefault() }; performance.addEventListener('foo', f); R = S([performance.dispatchEvent(new Event('foo', { cancelable: true })), performance.dispatchEvent(new Event('foo')), L])");
stmts("var L = []; var f = function (e) { L.push(e.type) }; performance.addEventListener('foo', f); performance.removeEventListener('foo', f); R = S([performance.dispatchEvent(new Event('foo')), L])");
stmts("var L = []; performance.addEventListener('foo', function () { L.push('once') }, { once: true }); performance.dispatchEvent(new Event('foo')); performance.dispatchEvent(new Event('foo')); R = S(L)");
stmts("var L = []; performance.addEventListener('foo', { handleEvent: function (e) { L.push(e.type) } }); performance.dispatchEvent(new Event('foo')); R = S(L)");
expr("performance.dispatchEvent({})");
expr("performance.dispatchEvent()");
expr("performance.addEventListener()");
expr("EventTarget.prototype.addEventListener.call(Performance.prototype, 'a', function () {})");
expr("Performance.prototype.addEventListener.call({}, 'a', function () {})");
expr("Object.getOwnPropertyDescriptor(performance, 'onresourcetimingbufferfull')");
expr("Object.getPrototypeOf(Performance.prototype) === Object.prototype");

// Construtores.
for (const n of ["Performance", "PerformanceEntry", "PerformanceMark", "PerformanceMeasure"]) {
  expr(`[${n}.name, ${n}.length, Reflect.ownKeys(${n}).map(String)]`);
  expr(`D(${n}, 'prototype')`);
  expr(`[${n}.prototype.constructor === ${n}, D(${n}.prototype, 'constructor')]`);
  expr(`${n}.toString()`);
  expr(`Object.getPrototypeOf(${n}) === Function.prototype`);
}
expr("Object.getPrototypeOf(PerformanceMark) === PerformanceEntry");
expr("Object.getPrototypeOf(PerformanceMeasure) === PerformanceEntry");
expr("Object.getPrototypeOf(MP) === EP");
expr("Object.getPrototypeOf(XP) === EP");
expr("new Performance()");
expr("Performance()");
expr("new PerformanceEntry()");
expr("PerformanceEntry()");
expr("new PerformanceMeasure()");
expr("PerformanceMeasure()");
expr("PerformanceMark('a')");
expr("new PerformanceMark()");
expr("Reflect.construct(Performance, [], Object)");
expr("Reflect.construct(PerformanceMeasure, [], Object)");
expr("Object.getPrototypeOf(Reflect.construct(PerformanceMark, ['x'], Object)) === Object.prototype");
expr("(function () { class M extends PerformanceMark {} var m = new M('x'); return [m instanceof M, m instanceof PerformanceMark, m instanceof PerformanceEntry, m.entryType, m.name] })()");
expr("new PerformanceMark(1).name");
expr("new PerformanceMark(undefined).name");
expr("new PerformanceMark({ toString() { return 'o' } }).name");
expr("new PerformanceMark('x', 5)");
expr("new PerformanceMark('x', 'y')");
expr("new PerformanceMark('x', null).detail");
expr("new PerformanceMark('x', undefined).detail");
expr("new PerformanceMark('x', []).detail");
expr("new PerformanceMark('x', function () {}).detail");
expr("(function () { var m = new PerformanceMark('c', { startTime: 3, detail: 'd' }); C(); return [m.startTime, m.detail, performance.getEntriesByName('c').length] })()");
expr("(function () { C(); new PerformanceMark('c'); return performance.getEntries().length })()");
expr("new PerformanceMark('x', { startTime: -1 })");
expr("new PerformanceMark('x', { startTime: NaN })");
expr("new PerformanceMark('x', { startTime: Infinity })");
expr("new PerformanceMark('x', { startTime: {} })");
expr("(function () { var m = new PerformanceMark('x'); return [m.name, m.entryType, typeof m.startTime, m.duration, m.detail, Reflect.ownKeys(m).map(String)] })()");

// Protótipos: descritores dos acessores e dos métodos que o porte tem.
expr("['name', 'entryType', 'startTime', 'duration'].map(function (k) { var d = Object.getOwnPropertyDescriptor(EP, k); return [k, d.get.name, d.get.length, d.enumerable, d.configurable, typeof d.set, 'prototype' in d.get] })");
expr("['detail'].map(function (k) { return [D(MP, k), D(XP, k), G(MP, k).name, G(XP, k).name, G(MP, k).length] })");
expr("D(P, 'timeOrigin')");
expr("[G(P, 'timeOrigin').name, G(P, 'timeOrigin').length, 'prototype' in G(P, 'timeOrigin')]");
for (const [obj, k, len] of [
  ["P", "toJSON", 0], ["P", "getEntries", 0], ["P", "getEntriesByType", 1], ["P", "getEntriesByName", 1],
  ["P", "mark", 1], ["P", "clearMarks", 0], ["P", "measure", 1], ["P", "clearMeasures", 0],
  ["EP", "toJSON", 0], ["MP", "toJSON", 0], ["XP", "toJSON", 0],
]) {
  expr(`[D(${obj}, '${k}'), ${obj}.${k}.name, ${obj}.${k}.length, 'prototype' in ${obj}.${k}, Object.getOwnPropertyNames(${obj}.${k}), ${obj}.${k}.toString()]`);
}
expr("[D(EP, Symbol.toStringTag), EP[Symbol.toStringTag], MP[Symbol.toStringTag], XP[Symbol.toStringTag]]");
expr("Reflect.ownKeys(MP).map(String)");
expr("Reflect.ownKeys(XP).map(String)");
expr("Reflect.ownKeys(EP).map(String).filter(function (k) { return k !== 'Symbol(nodejs.util.inspect.custom)' })");
expr("EP.toJSON === MP.toJSON");
expr("MP.toJSON === XP.toJSON");
expr("Object.prototype.toString.call(EP)");

// now(): tipo, monotonia, fração, ligação com Date.now, this irrelevante.
expr("typeof performance.now()");
expr("performance.now() > 0");
expr("performance.now() < 3600000");
expr("(function () { var a = performance.now(), ok = true; for (var i = 0; i < 2000; i++) { var b = performance.now(); if (b < a) ok = false; a = b } return ok })()");
expr("(function () { for (var i = 0; i < 100; i++) if (!Number.isInteger(performance.now())) return true; return false })()");
expr("performance.now.call({}) > 0");
expr("performance.now.call(null) > 0");
expr("performance.now.call(5) > 0");
expr("(0, performance.now)() > 0");
expr("performance.now.length");
expr("Math.abs(performance.timeOrigin + performance.now() - Date.now()) < 100");
expr("typeof performance.timeOrigin");
expr("Number.isInteger(performance.timeOrigin)");
expr("performance.timeOrigin > 1e12");
expr("performance.timeOrigin === performance.timeOrigin");
expr("(function () { var o = performance.timeOrigin; for (var i = 0; i < 1000; i++) performance.now(); return performance.timeOrigin === o })()");
strict("performance.timeOrigin = 1; R = S(typeof performance.timeOrigin)");
expr("G(P, 'timeOrigin').call({})");
expr("P.timeOrigin");
expr("G(P, 'timeOrigin').call(null)");
expr("typeof G(P, 'timeOrigin').call(performance)");
expr("(function () { var d = performance.now(); var m = performance.mark('t'); var e = performance.now(); return d <= m.startTime && m.startTime <= e })()");

// toJSON de performance.
expr("typeof performance.toJSON()");
expr("Object.keys(performance.toJSON())[0]");
expr("typeof performance.toJSON().timeOrigin");
expr("performance.toJSON().timeOrigin === performance.timeOrigin");
expr("Object.getPrototypeOf(performance.toJSON()) === Object.prototype");
expr("D(performance.toJSON(), 'timeOrigin')");
expr("P.toJSON.call({})");
expr("P.toJSON()");
expr("P.toJSON.call(null)");
expr("typeof JSON.stringify(performance)");
expr("JSON.parse(JSON.stringify(performance)).timeOrigin === performance.timeOrigin");

// mark.
expr("(function () { var m = performance.mark('x'); return [m instanceof PerformanceMark, m instanceof PerformanceEntry, m.name, m.entryType, m.duration, typeof m.startTime, m.detail, Reflect.ownKeys(m).map(String), String(m), Object.prototype.toString.call(m)] })()");
expr("JSON.stringify(performance.mark('x')).replace(/\"startTime\":[-0-9.e+]+/, '\"startTime\":N')");
expr("Object.keys(performance.mark('x'))");
expr("(function () { var k = []; for (var n in performance.mark('e')) k.push(n); return k })()");
expr("performance.mark()");
expr("performance.mark(undefined).name");
expr("performance.mark(123, { startTime: '7' }).name");
expr("performance.mark('s', { startTime: '7' }).startTime");
expr("performance.mark('s', { startTime: null }).startTime");
expr("performance.mark(Symbol())");
expr("(function () { var x = performance.mark('c'); x.foo = 1; return [performance.getEntries()[0] === x, performance.getEntriesByName('c')[0] === x, performance.getEntries()[0].foo] })()");
expr("(function () { C(); return performance.measure('g', { start: 5, end: 3 }).duration })()");
expr("(function () { C(); performance.mark('c', { startTime: 4 }); return [performance.measure('g', { start: 'c', end: 'c' }).duration, performance.measure('k', { end: 'c' }).startTime, performance.measure('d', undefined, 'c').startTime] })()");
expr("(function () { C(); return performance.measure('f', { start: undefined }).startTime })()");
expr("(function () { C(); return performance.measure('f', 'c', {}) })()");
expr("(function () { C(); return performance.measure('f', 'yyy', 'zzz') })()");
expr("(function () { C(); return performance.measure('h', { end: 9, duration: 2 }).startTime })()");
expr("(function () { C(); return performance.measure('h', { start: NaN }) })()");
expr("(function () { C(); return performance.measure('h', { start: -1 }) })()");
expr("performance.getEntriesByName('a', null).length");
expr("performance.mark(null).name");
expr("performance.mark(5).name");
expr("performance.mark(true).name");
expr("performance.mark({ toString() { return 'o' } }).name");
expr("performance.mark({ toString() { throw new RangeError('boom') } })");
expr("performance.mark('a', 'b')");
expr("performance.mark('a', 5)");
expr("performance.mark('a', null).name");
expr("performance.mark('a', []).name");
expr("performance.mark('a', function () {}).name");
expr("performance.mark('o', { startTime: 5, detail: { a: 1 } })");
expr("(function () { var m = performance.mark('o', { startTime: 5, detail: { a: 1 } }); return [m.startTime, m.detail, m.duration] })()");
expr("performance.mark('o', { startTime: '5' }).startTime");
expr("performance.mark('o', { startTime: null }).startTime");
expr("performance.mark('o', { startTime: true }).startTime");
expr("performance.mark('o', { startTime: 0 }).startTime");
expr("Object.is(performance.mark('o', { startTime: -0 }).startTime, -0)");
expr("performance.mark('o', { startTime: undefined }).startTime > 0");
expr("performance.mark('o', { startTime: -1 })");
expr("performance.mark('o', { startTime: NaN })");
expr("performance.mark('o', { startTime: Infinity })");
expr("performance.mark('o', { startTime: -Infinity })");
expr("performance.mark('o', { startTime: {} })");
expr("performance.mark('o', {}).detail");
expr("performance.mark('o', { detail: null }).detail");
expr("performance.mark('o', { detail: undefined }).detail");
expr("performance.mark('o', { detail: 7 }).detail");
expr("performance.mark('o', { detail: 'z' }).detail");
expr("performance.mark('o', { detail: () => 1 })");
expr("performance.mark('o', { detail: Symbol() })");
expr("(function () { var d = [1, { a: 2 }]; var m = performance.mark('o', { detail: d }); return [m.detail, m.detail === d, m.detail === m.detail, Array.isArray(m.detail)] })()");
expr("(function () { var d = { a: 1 }; d.self = d; var m = performance.mark('o', { detail: d }); return m.detail.self === m.detail })()");
expr("(function () { var m = performance.mark('o', { detail: 1 }); return [D(m, 'detail'), D(Object.getPrototypeOf(m), 'detail')] })()");
expr("(function () { var m = performance.mark('o', { detail: { a: 1 } }); return JSON.stringify(m.toJSON()).replace(/\"startTime\":[-0-9.e+]+/, '\"startTime\":N') })()");
expr("(function () { var m = performance.mark('j'); return Object.keys(m.toJSON()) })()");
expr("(function () { var m = performance.mark('j'); return [Object.getPrototypeOf(m.toJSON()) === Object.prototype, D(m.toJSON(), 'name'), D(m.toJSON(), 'detail')] })()");
expr("Object.keys(EP.toJSON.call(performance.mark('z3')))");
expr("Object.keys(EP.toJSON.call(performance.measure('z4')))");
expr("(function () { var m = performance.mark('s'); return [m.startTime === m.startTime, m.name === m.name, m.entryType === m.entryType] })()");
expr("P.mark.call({}, 'a')");
expr("P.mark.call(null, 'a')");
expr("P.mark('a')");
expr("P.mark.call(performance, 'a').name");
expr("performance.mark.call(performance)");
expr("performance.mark.call({})");
expr("(function () { var f = performance.mark; return f('a') })()");

// Getters de entrada: this inválido.
expr("G(EP, 'name').call({})");
expr("G(EP, 'entryType').call({})");
expr("G(EP, 'startTime').call(null)");
expr("G(EP, 'duration').call(5)");
expr("G(EP, 'name').call(performance)");
expr("G(EP, 'name').call(EP)");
expr("G(EP, 'name').call(new PerformanceMark('q'))");
expr("G(EP, 'name').call(performance.measure('q2'))");
expr("G(MP, 'detail').call({})");
expr("G(XP, 'detail').call({})");
expr("G(MP, 'detail').call(performance.measure('z'))");
expr("G(XP, 'detail').call(performance.mark('z'))");
expr("EP.name");
expr("MP.detail");
expr("EP.toJSON.call({})");
expr("MP.toJSON.call({})");
expr("XP.toJSON.call({})");
expr("MP.toJSON.call(performance.measure('z2'))");
expr("XP.toJSON.call(performance.mark('z2'))");
expr("EP.toJSON()");
strict("var m = performance.mark('r'); m.name = 'x'; R = S(m.name)");
stmts("var m = performance.mark('r'); m.name = 'x'; m.extra = 1; R = S([m.name, m.extra, Object.keys(m)])");

// getEntries, getEntriesByType, getEntriesByName.
expr("(function () { C(); performance.mark('a'); performance.mark('b'); performance.mark('a'); return [performance.getEntries().map(function (e) { return e.name }), performance.getEntriesByType('mark').length, performance.getEntriesByName('a').length, performance.getEntriesByName('a', 'mark').length, performance.getEntriesByName('a', 'measure').length, performance.getEntriesByType('measure').length] })()");
expr("performance.getEntriesByType()");
expr("performance.getEntriesByName()");
expr("(function () { var r = performance.getEntries(); return [Array.isArray(r), r === performance.getEntries(), Object.getPrototypeOf(r) === Array.prototype] })()");
expr("(function () { C(); var m = performance.mark('a'); var e = performance.getEntries()[0]; return [e === m, e instanceof PerformanceMark, e.name, e.entryType] })()");
expr("(function () { C(); performance.mark('a'); return performance.getEntries()[0] === performance.getEntries()[0] })()");
expr("P.getEntries.call({})");
expr("P.getEntriesByType.call({}, 'mark')");
expr("P.getEntriesByName.call({}, 'a')");
expr("P.getEntries.call(performance).length >= 0");
expr("performance.getEntries(1, 2).length >= 0");
expr("(function () { C(); performance.mark('z', { startTime: 10 }); performance.mark('y', { startTime: 5 }); return performance.getEntries().map(function (e) { return e.name }) })()");
expr("(function () { C(); performance.measure('m', { start: 1, end: 2 }); performance.mark('a', { startTime: 1 }); return performance.getEntries().map(function (e) { return e.name }) })()");
expr("(function () { C(); performance.mark('a', { startTime: 1 }); performance.measure('m', { start: 1, end: 2 }); performance.mark('b', { startTime: 1 }); return performance.getEntries().map(function (e) { return e.name }) })()");
expr("(function () { C(); performance.mark('a', { startTime: 7 }); performance.measure('m', { start: 1, end: 2 }); performance.mark('b', { startTime: 0 }); return performance.getEntries().map(function (e) { return e.name }) })()");
expr("(function () { C(); performance.mark('a'); return [performance.getEntriesByType('zzz'), performance.getEntriesByType(5), performance.getEntriesByType('Mark'), performance.getEntriesByType('resource')] })()");
expr("(function () { C(); performance.mark('a'); return [performance.getEntriesByName('b'), performance.getEntriesByName(undefined), performance.getEntriesByName('a', undefined).length] })()");
expr("(function () { C(); performance.mark('1'); return performance.getEntriesByName(1).length })()");
expr("(function () { C(); performance.mark('a'); performance.measure('a', { start: 1, end: 2 }); return [performance.getEntriesByName('a').map(function (e) { return e.entryType }), performance.getEntriesByName('a', 'measure').map(function (e) { return e.entryType }), performance.getEntriesByType('measure').map(function (e) { return e.name })] })()");
expr("(function () { C(); performance.measure('q', { start: 1, end: 2 }); return [performance.getEntries().map(function (e) { return e.entryType + ':' + e.name }), performance.getEntriesByType('measure').length] })()");
expr("(function () { C(); performance.mark('a', { detail: { x: 1 } }); var e = performance.getEntries()[0]; return [e.detail, e.toJSON().detail] })()");
// Identidade: cada entrada gravada é um objeto único (mark, getEntries*, observadores), e propriedades do script sobrevivem.
expr("(function () { C(); var m = performance.mark('a'); return [m === performance.getEntries()[0], performance.getEntriesByName('a')[0] === m, performance.getEntriesByType('mark')[0] === m, performance.getEntries()[0] === performance.getEntries()[0]] })()");
expr("(function () { C(); var m = performance.mark('a'); m.x = 1; var e = performance.getEntries()[0]; e.y = 2; return [e.x, m.y, Object.keys(performance.getEntriesByName('a')[0])] })()");
expr("(function () { C(); var m = performance.measure('q', { start: 1, end: 2 }); return [m === performance.getEntries()[0], performance.getEntriesByType('measure')[0] === m] })()");
expr("(function () { C(); var m = performance.mark('a'); performance.mark('b'); performance.clearMarks('b'); return performance.getEntries()[0] === m })()");
expr("(function () { C(); var m = new PerformanceMark('a'); return performance.getEntries().length })()");
expr("(function () { var o = new PerformanceObserver(function () {}); C(); o.observe({ type: 'mark' }); var m = performance.mark('a'); m.x = 1; var r = o.takeRecords()[0]; return [r === m, r.x, r === performance.getEntries()[0]] })()");

// clearMarks, clearMeasures.
expr("performance.clearMarks()");
expr("performance.clearMeasures()");
expr("performance.clearMarks('a')");
expr("(function () { C(); performance.mark('a'); performance.mark('b'); performance.clearMarks('a'); return performance.getEntries().map(function (e) { return e.name }) })()");
expr("(function () { C(); performance.mark('a'); performance.clearMarks(undefined); return performance.getEntries().length })()");
expr("(function () { C(); performance.mark('a'); performance.clearMarks(null); return performance.getEntries().length })()");
expr("(function () { C(); performance.mark('null'); performance.clearMarks(null); return performance.getEntries().length })()");
expr("(function () { C(); performance.mark('1'); performance.clearMarks(1); return performance.getEntries().length })()");
expr("(function () { C(); performance.mark('a'); performance.clearMarks('b'); return performance.getEntries().length })()");
expr("(function () { C(); performance.mark('a'); performance.measure('m'); performance.clearMarks(); return performance.getEntries().map(function (e) { return e.entryType }) })()");
expr("(function () { C(); performance.mark('a'); performance.measure('m'); performance.clearMeasures(); return performance.getEntries().map(function (e) { return e.entryType }) })()");
expr("(function () { C(); performance.measure('q', { start: 1, end: 2 }); performance.measure('q2', { start: 1, end: 2 }); performance.clearMeasures('q'); return performance.getEntries().map(function (e) { return e.name }) })()");
expr("(function () { C(); performance.mark('a'); performance.measure('a'); performance.clearMarks('a'); return performance.getEntries().map(function (e) { return e.entryType }) })()");
expr("P.clearMarks.call({})");
expr("P.clearMeasures.call({})");
expr("P.clearMarks.call(null)");
expr("performance.clearMarks({ toString() { throw new RangeError('boom') } })");

// measure: formas posicional e com objeto.
expr("performance.measure()");
expr("(function () { C(); performance.mark('s', { startTime: 1 }); performance.mark('e', { startTime: 4 }); var m = performance.measure('m', 's', 'e'); return [m instanceof PerformanceMeasure, m instanceof PerformanceEntry, m.name, m.entryType, m.startTime, m.duration, m.detail, Reflect.ownKeys(m).map(String), String(m)] })()");
expr("(function () { C(); performance.mark('s', { startTime: 1 }); performance.mark('e', { startTime: 4 }); return JSON.stringify(performance.measure('m', 's', 'e')) })()");
expr("(function () { var m = performance.measure('m'); return [m.startTime, m.duration > 0, m.entryType] })()");
expr("(function () { var m = performance.measure('m', undefined, undefined); return m.startTime })()");
expr("(function () { var m = performance.measure('m', null); return [m.startTime, m.duration > 0] })()");
expr("performance.measure('m', 'nope')");
expr("(function () { C(); performance.mark('s'); return performance.measure('m', 's', 'nope') })()");
expr("performance.measure('m', undefined, 'nope')");
expr("performance.measure('m', 'nope1', 'nope2')");
expr("performance.measure('m', 3)");
expr("performance.measure('m', 3, 6)");
expr("performance.measure('m', '3', '6')");
expr("performance.measure('m', 'it\\'s')");
expr("(function () { C(); performance.mark('E', { startTime: 9 }); var m = performance.measure('n', undefined, 'E'); return [m.startTime, m.duration] })()");
expr("(function () { C(); performance.mark('S', { startTime: 2 }); var m = performance.measure('n', 'S'); return [m.startTime, m.duration > 0] })()");
expr("(function () { C(); performance.mark('L', { startTime: 2 }); performance.mark('L', { startTime: 6 }); return performance.measure('n', 'L').startTime })()");
expr("(function () { C(); performance.mark('L', { startTime: 2 }); performance.mark('L', { startTime: 6 }); performance.clearMarks('L'); return performance.measure('n', 'L') })()");
expr("(function () { C(); performance.mark('4', { startTime: 4 }); return performance.measure('n', '4').startTime })()");
expr("(function () { C(); performance.mark('S', { startTime: 5 }); performance.mark('E', { startTime: 2 }); var m = performance.measure('n', 'S', 'E'); return [m.startTime, m.duration] })()");
expr("(function () { var m = performance.measure('m', { start: 2, end: 7, detail: 3 }); return [m.startTime, m.duration, m.detail] })()");
expr("(function () { var m = performance.measure('m', { start: 2, duration: 7 }); return [m.startTime, m.duration] })()");
expr("(function () { var m = performance.measure('m', { end: 9, duration: 7 }); return [m.startTime, m.duration] })()");
expr("(function () { var m = performance.measure('m', { start: 5, end: 2 }); return [m.startTime, m.duration] })()");
expr("(function () { var m = performance.measure('m', { end: 3 }); return [m.startTime, m.duration] })()");
expr("(function () { var m = performance.measure('m', { start: 3 }); return [m.startTime, m.duration === m.duration, Math.abs(performance.now() - 3 - m.duration) < 1000] })()");
expr("(function () { var m = performance.measure('m', {}); return [m.startTime, m.duration > 0] })()");
expr("(function () { var m = performance.measure('m', { duration: 1 }); return [m.startTime, m.duration > 0] })()");
expr("(function () { var m = performance.measure('m', { start: 3, end: undefined }); return m.startTime })()");
expr("performance.measure('m', { start: 2, end: 3, duration: 1 })");
expr("performance.measure('m', { start: 1 }, 'e')");
expr("(function () { C(); performance.mark('e', { startTime: 10 }); var m = performance.measure('m', {}, 'e'); return [m.startTime, m.duration] })()");
expr("(function () { C(); performance.mark('e', { startTime: 10 }); var m = performance.measure('m', { duration: 1 }, 'e'); return [m.startTime, m.duration] })()");
expr("(function () { C(); performance.mark('e', { startTime: 10 }); var m = performance.measure('m', { detail: 1 }, 'e'); return [m.startTime, m.duration, m.detail] })()");
expr("(function () { C(); performance.mark('e', { startTime: 10 }); return performance.measure('m', { end: 1 }, 'e') })()");
expr("performance.measure('m', { start: -1 })");
expr("performance.measure('m', { end: -1 })");
expr("performance.measure('m', { start: 1, duration: -1 })");
expr("performance.measure('m', { start: NaN })");
expr("performance.measure('m', { end: NaN })");
expr("performance.measure('m', { start: 1, duration: NaN })");
expr("performance.measure('m', { start: Infinity })");
expr("performance.measure('m', { start: null })");
expr("performance.measure('m', { start: 'nope' })");
expr("(function () { C(); performance.mark('s', { startTime: 1 }); return performance.measure('m', { start: 's' }).startTime })()");
expr("(function () { C(); performance.mark('S2', { startTime: 2 }); var m = performance.measure('n', { start: 'S2', end: 10 }); return [m.startTime, m.duration] })()");
expr("(function () { return performance.measure('n', { start: 3, duration: '4' }).duration })()");
expr("(function () { return performance.measure('n', { detail: { a: [1, 2] } }).detail })()");
expr("(function () { var d = { a: 1 }; return performance.measure('n', { detail: d }).detail === d })()");
expr("performance.measure('n', { detail: () => 1 })");
expr("(function () { var m = performance.measure('n', { detail: undefined }); return m.detail })()");
expr("(function () { var m = performance.measure('n', { detail: 3 }); return [D(m, 'detail'), D(XP, 'detail')] })()");
expr("performance.measure('m', 5, { start: 1 })");
expr("(function () { var m = performance.measure('m', function () {}); return [m.startTime, m.duration > 0] })()");
expr("(function () { var m = performance.measure('m', { foo: 1 }); return [m.startTime, m.duration > 0] })()");
expr("(function () { var m = performance.measure('m', { detail: 1 }); return [m.startTime, m.duration > 0, m.detail] })()");
expr("(function () { var m = performance.measure('m', []); return [m.startTime, m.duration > 0] })()");
expr("performance.measure({ toString() { return 'o' } }).name");
expr("performance.measure(undefined).name");
expr("performance.measure(5).name");
expr("performance.measure({ toString() { throw new RangeError('boom') } })");
expr("P.measure.call({}, 'a')");
expr("P.measure.call(null, 'a')");
expr("P.measure('a')");
expr("(function () { C(); var m = performance.measure('j', { start: 1, end: 4, detail: { k: 1 } }); return JSON.stringify(m.toJSON()) })()");
expr("(function () { C(); var m = performance.measure('j', { start: 1, end: 4 }); return [Object.keys(m.toJSON()), m.toJSON().detail] })()");
expr("(function () { C(); var m = performance.measure('j', { start: 1, end: 4 }); return JSON.stringify(m) })()");
expr("(function () { var k = []; for (var n in performance.measure('e')) k.push(n); return k })()");

// timing, PerformanceTiming, onresourcetimingbufferfull e os métodos de ResourceTiming.
const TK = ["navigationStart", "unloadEventStart", "unloadEventEnd", "redirectStart", "redirectEnd", "fetchStart", "domainLookupStart", "domainLookupEnd", "connectStart", "connectEnd", "secureConnectionStart", "requestStart", "responseStart", "responseEnd", "domLoading", "domInteractive", "domContentLoadedEventStart", "domContentLoadedEventEnd", "domComplete", "loadEventStart", "loadEventEnd"];
const OF = "Object.getOwnPropertyDescriptor(P, 'onresourcetimingbufferfull')";
expr("Reflect.ownKeys(P).map(String)");
expr("D(P, 'timing')");
expr("[G(P, 'timing').name, G(P, 'timing').length, 'prototype' in G(P, 'timing'), Object.getOwnPropertyNames(G(P, 'timing')), G(P, 'timing').toString(), G(P, 'timing').bind().name]");
expr("[Object.getOwnPropertyDescriptor(P, 'timing').enumerable, Object.getOwnPropertyDescriptor(P, 'timing').configurable, typeof Object.getOwnPropertyDescriptor(P, 'timing').set]");
expr("G(P, 'timing').call({})");
expr("G(P, 'timing').call(null)");
expr("P.timing");
expr("G(P, 'timing').call(Object.create(performance))");
expr("performance.timing === performance.timing");
expr("G(P, 'timing').call(performance) === performance.timing");
expr("Reflect.ownKeys(performance).map(String)");
expr("[typeof performance.timing, Object.prototype.toString.call(performance.timing), Reflect.ownKeys(performance.timing).map(String), Object.isExtensible(performance.timing)]");
expr("[Object.getPrototypeOf(performance.timing) === PerformanceTiming.prototype, performance.timing instanceof PerformanceTiming, performance.timing.constructor === PerformanceTiming]");
expr("(function () { performance.timing = 1; return typeof performance.timing })()");
strict("performance.timing = 1; R = S(typeof performance.timing)");
expr("JSON.stringify(performance.timing)");
expr("D(globalThis, 'PerformanceTiming')");
expr("[PerformanceTiming.name, PerformanceTiming.length, Reflect.ownKeys(PerformanceTiming).map(String), Object.getPrototypeOf(PerformanceTiming) === Function.prototype, PerformanceTiming.toString()]");
expr("D(PerformanceTiming, 'prototype')");
expr("[PerformanceTiming.prototype.constructor === PerformanceTiming, D(PerformanceTiming.prototype, 'constructor')]");
expr("Reflect.ownKeys(PerformanceTiming.prototype).map(String)");
expr("Object.getPrototypeOf(PerformanceTiming.prototype) === Object.prototype");
expr("[D(PerformanceTiming.prototype, Symbol.toStringTag), PerformanceTiming.prototype[Symbol.toStringTag], Object.prototype.toString.call(PerformanceTiming.prototype)]");
expr("new PerformanceTiming()");
expr("PerformanceTiming()");
expr("Reflect.construct(PerformanceTiming, [], Object)");
expr("[D(PerformanceTiming.prototype, 'toJSON'), PerformanceTiming.prototype.toJSON.name, PerformanceTiming.prototype.toJSON.length, 'prototype' in PerformanceTiming.prototype.toJSON, PerformanceTiming.prototype.toJSON.toString()]");
expr("JSON.stringify(PerformanceTiming.prototype.toJSON.call(performance.timing))");
expr("PerformanceTiming.prototype.toJSON.call(performance.timing) === PerformanceTiming.prototype.toJSON.call(performance.timing)");
expr("Object.getPrototypeOf(performance.timing.toJSON()) === Object.prototype");
expr("Object.keys(performance.timing.toJSON())");
expr("D(performance.timing.toJSON(), 'loadEventEnd')");
expr("PerformanceTiming.prototype.toJSON.call({})");
expr("PerformanceTiming.prototype.toJSON.call(null)");
expr("PerformanceTiming.prototype.toJSON()");
expr("PerformanceTiming.prototype.toJSON.call(performance)");
for (const k of TK) {
  expr(`(function () { var d = Object.getOwnPropertyDescriptor(PerformanceTiming.prototype, '${k}'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length, 'prototype' in d.get, d.get.toString(), performance.timing.${k}, d.get.call(performance.timing)] })()`);
  expr(`Object.getOwnPropertyDescriptor(PerformanceTiming.prototype, '${k}').get.call({})`);
}
expr("PerformanceTiming.prototype.navigationStart");
expr("Object.getOwnPropertyDescriptor(PerformanceTiming.prototype, 'navigationStart').get.call(performance)");
expr("Object.getOwnPropertyDescriptor(PerformanceTiming.prototype, 'loadEventEnd').get.call(null)");
expr("(function () { var t = performance.timing; t.navigationStart = 5; return [t.navigationStart, Object.keys(t)] })()");
expr("Object.keys(performance.toJSON())");
expr("Reflect.ownKeys(performance.toJSON()).map(String)");
expr("performance.toJSON().timing === performance.timing");
expr("Object.getPrototypeOf(performance.toJSON().timing) === PerformanceTiming.prototype");
expr("D(performance.toJSON(), 'timing')");
expr("JSON.stringify(performance.toJSON().timing)");
expr("Object.keys(JSON.parse(JSON.stringify(performance)))");
expr("JSON.stringify(JSON.parse(JSON.stringify(performance)).timing)");

expr("D(P, 'onresourcetimingbufferfull')");
expr(`[${OF}.get.name, ${OF}.get.length, 'prototype' in ${OF}.get, Object.getOwnPropertyNames(${OF}.get), ${OF}.get.toString(), ${OF}.get.bind().name]`);
expr(`[${OF}.set.name, ${OF}.set.length, 'prototype' in ${OF}.set, Object.getOwnPropertyNames(${OF}.set), ${OF}.set.toString(), ${OF}.set.bind().name]`);
expr(`[${OF}.enumerable, ${OF}.configurable]`);
expr("performance.onresourcetimingbufferfull");
expr("P.onresourcetimingbufferfull");
expr(`${OF}.get.call({})`);
expr(`${OF}.get.call(null)`);
expr(`${OF}.set.call({}, 1)`);
expr(`${OF}.set.call(null, 1)`);
expr(`${OF}.set.call(Object.create(performance), 1)`);
expr(`${OF}.set.call(performance)`);
expr(`${OF}.set.call(performance, () => 1)`);
expr("(function () { P.onresourcetimingbufferfull = 1; return typeof P.onresourcetimingbufferfull })()");
expr("(function () { var f = function () {}; performance.onresourcetimingbufferfull = f; return [performance.onresourcetimingbufferfull === f, Reflect.ownKeys(performance).map(String)] })()");
expr("(function () { var o = {}; performance.onresourcetimingbufferfull = o; return performance.onresourcetimingbufferfull === o })()");
expr("(function () { var o = []; performance.onresourcetimingbufferfull = o; return performance.onresourcetimingbufferfull === o })()");
expr("(function () { var r = []; for (var v of [5, 'a', true, undefined, null, Symbol(), 1n]) { performance.onresourcetimingbufferfull = function () {}; performance.onresourcetimingbufferfull = v; r.push(performance.onresourcetimingbufferfull) } return r })()");
expr("(function () { performance.onresourcetimingbufferfull = function () {}; performance.onresourcetimingbufferfull = 5; return performance.onresourcetimingbufferfull })()");
strict("performance.onresourcetimingbufferfull = 3; R = S(performance.onresourcetimingbufferfull)");
expr("(function () { var r = (performance.onresourcetimingbufferfull = 7); return r })()");
expr("(function () { performance.onresourcetimingbufferfull = function () {}; return Object.keys(performance) })()");
expr("(function () { performance.onresourcetimingbufferfull = function () {}; return JSON.stringify(performance.toJSON()).indexOf('onresource') })()");

for (const [k, len] of [["clearResourceTimings", 0], ["setResourceTimingBufferSize", 1], ["markResourceTiming", 7]]) {
  expr(`[D(P, '${k}'), P.${k}.name, P.${k}.length, 'prototype' in P.${k}, Object.getOwnPropertyNames(P.${k}), P.${k}.toString()]`);
  expr(`Object.getPrototypeOf(P.${k}) === Function.prototype`);
  expr(`typeof performance.${k}`);
  expr(`Object.keys(performance).indexOf('${k}')`);
}
expr("performance.clearResourceTimings()");
expr("performance.clearResourceTimings(1, 2)");
expr("performance.clearResourceTimings({ toString() { throw new RangeError('boom') } })");
expr("P.clearResourceTimings.call({})");
expr("P.clearResourceTimings.call(null)");
expr("P.clearResourceTimings()");
expr("P.clearResourceTimings.call(performance)");
expr("P.clearResourceTimings.call(Object.create(performance))");
expr("(function () { C(); performance.mark('a'); performance.clearResourceTimings(); return performance.getEntries().length })()");
expr("performance.setResourceTimingBufferSize()");
expr("performance.setResourceTimingBufferSize(5)");
expr("performance.setResourceTimingBufferSize(0)");
expr("performance.setResourceTimingBufferSize(-1)");
expr("performance.setResourceTimingBufferSize(1.5)");
expr("performance.setResourceTimingBufferSize(NaN)");
expr("performance.setResourceTimingBufferSize(Infinity)");
expr("performance.setResourceTimingBufferSize(2 ** 33)");
expr("performance.setResourceTimingBufferSize('a')");
expr("performance.setResourceTimingBufferSize(undefined)");
expr("performance.setResourceTimingBufferSize(null)");
expr("performance.setResourceTimingBufferSize({})");
expr("performance.setResourceTimingBufferSize(Symbol())");
expr("performance.setResourceTimingBufferSize(1n)");
expr("performance.setResourceTimingBufferSize({ valueOf() { throw new RangeError('b') } })");
expr("performance.setResourceTimingBufferSize(1, 2, 3)");
expr("P.setResourceTimingBufferSize.call({})");
expr("P.setResourceTimingBufferSize.call({}, 1)");
expr("P.setResourceTimingBufferSize.call({}, { valueOf() { throw new RangeError('b') } })");
expr("P.setResourceTimingBufferSize.call(null, 1)");
expr("P.setResourceTimingBufferSize()");
expr("P.setResourceTimingBufferSize.call(performance, 1)");
expr("(function () { var n = 0; performance.setResourceTimingBufferSize({ valueOf() { n++; return 1 } }); return n })()");
expr("performance.markResourceTiming()");
expr("performance.markResourceTiming(1, 2, 3, 4, 5, 6, 7)");
expr("performance.markResourceTiming({})");
expr("performance.markResourceTiming({ toString() { throw new RangeError('boom') } })");
expr("performance.markResourceTiming(Symbol())");
expr("P.markResourceTiming.call({})");
expr("P.markResourceTiming.call(null)");
expr("P.markResourceTiming()");
expr("P.markResourceTiming.call(5, 1, 2)");
expr("(function () { C(); performance.markResourceTiming(1, 2, 3, 4, 5, 6, 7); return [performance.getEntries().length, performance.getEntriesByType('resource').length] })()");

// PerformanceObserver, PerformanceObserverEntryList, PerformanceResourceTiming, PerformanceServerTiming.
const NEW = ["PerformanceObserver", "PerformanceObserverEntryList", "PerformanceResourceTiming", "PerformanceServerTiming"];
for (const n of NEW) {
  expr(`D(globalThis, '${n}')`);
  expr(`[typeof ${n}, Object.prototype.hasOwnProperty.call(globalThis, '${n}'), Object.keys(globalThis).indexOf('${n}') >= 0]`);
  expr(`[${n}.name, ${n}.length, Reflect.ownKeys(${n}).map(String)]`);
  expr(`Reflect.ownKeys(${n}).map(function (k) { return String(k) + '=' + S(D(${n}, k)) })`);
  expr(`[${n}.prototype.constructor === ${n}, D(${n}.prototype, 'constructor')]`);
  expr(`${n}.toString()`);
  expr(`Reflect.ownKeys(${n}.prototype).map(String)`);
  expr(`Reflect.ownKeys(${n}.prototype).map(function (k) { var d = Object.getOwnPropertyDescriptor(${n}.prototype, k); return [String(k), D(${n}.prototype, k), d.get ? [d.get.name, d.get.length, d.get.toString()] : typeof d.value === 'function' ? [d.value.name, d.value.length, d.value.toString()] : String(d.value)] })`);
  expr(`Object.prototype.toString.call(${n}.prototype)`);
  expr(`Object.getPrototypeOf(${n}) === Function.prototype`);
  expr(`Object.getPrototypeOf(${n}.prototype) === Object.prototype`);
  expr(`Object.getPrototypeOf(${n}.prototype) === PerformanceEntry.prototype`);
  expr(`Object.getPrototypeOf(${n}) === PerformanceEntry`);
  expr(`Object.getOwnPropertyNames(globalThis).indexOf('${n}') - Object.getOwnPropertyNames(globalThis).indexOf('MessagePort')`);
}
expr("Object.getOwnPropertyNames(globalThis).filter(function (k) { return /^Performance/.test(k) })");
expr("Object.getOwnPropertyNames(globalThis).indexOf('PerformanceTiming') - Object.getOwnPropertyNames(globalThis).indexOf('PerformanceServerTiming')");
for (const n of ["PerformanceObserverEntryList", "PerformanceResourceTiming", "PerformanceServerTiming"]) {
  expr(`new ${n}()`);
  expr(`${n}()`);
  expr(`Reflect.construct(${n}, [], Object)`);
  const proto = `${n}.prototype`;
  const getters = { PerformanceResourceTiming: ["initiatorType", "nextHopProtocol", "workerStart", "redirectStart", "redirectEnd", "fetchStart", "domainLookupStart", "domainLookupEnd", "connectStart", "connectEnd", "secureConnectionStart", "requestStart", "responseStart", "responseEnd", "transferSize", "encodedBodySize", "decodedBodySize", "serverTiming"], PerformanceServerTiming: ["name", "duration", "description"], PerformanceObserverEntryList: [] }[n];
  for (const g of getters) {
    for (const t of ["{}", "null", "5", "Object.create(" + proto + ")", "performance"]) expr(`G(${proto}, '${g}').call(${t})`);
    expr(`G(${proto}, '${g}').call()`);
  }
  const methods = { PerformanceResourceTiming: ["toJSON"], PerformanceServerTiming: ["toJSON"], PerformanceObserverEntryList: ["getEntries", "getEntriesByType", "getEntriesByName"] }[n];
  for (const m of methods) {
    for (const t of ["{}", "null", "5", "Object.create(" + proto + ")", "performance"]) expr(`${proto}.${m}.call(${t})`);
    expr(`${proto}.${m}()`);
  }
}
expr("PerformanceObserver.supportedEntryTypes");
expr("Object.isFrozen(PerformanceObserver.supportedEntryTypes)");
expr("Object.getPrototypeOf(PerformanceObserver.supportedEntryTypes) === Array.prototype");
expr("(function () { PerformanceObserver.supportedEntryTypes = 1; return PerformanceObserver.supportedEntryTypes })()");
expr("PerformanceObserver.supportedEntryTypes.push(1)");
strict("PerformanceObserver.supportedEntryTypes = 1");
expr("new PerformanceObserver()");
expr("PerformanceObserver()");
expr("PerformanceObserver(function () {})");
expr("PerformanceObserver.call({}, function () {})");
for (const cb of ["1", "{}", "null", "undefined", "'f'", "Symbol()", "[]", "class {}", "function () {}", "() => {}", "async function () {}", "Math.max", "{ call() {} }"]) expr(`new PerformanceObserver(${cb}) instanceof PerformanceObserver`);
expr("new PerformanceObserver(1, 2, 3)");
expr("new PerformanceObserver(function () {}, 2, 3) instanceof PerformanceObserver");
expr("Reflect.ownKeys(new PerformanceObserver(function () {}))");
expr("String(new PerformanceObserver(function () {}))");
expr("Object.getPrototypeOf(new PerformanceObserver(function () {})) === PerformanceObserver.prototype");
expr("(function () { class A extends PerformanceObserver {} var a = new A(function () {}); return [a instanceof A, a instanceof PerformanceObserver, Object.getPrototypeOf(a) === A.prototype] })()");
expr("Reflect.construct(PerformanceObserver, [function () {}], Object).constructor === Object");
expr("Reflect.construct(PerformanceObserver, [function () {}], function () {}) instanceof PerformanceObserver");
for (const m of ["observe", "disconnect", "takeRecords"]) {
  for (const t of ["{}", "null", "5", "Object.create(PerformanceObserver.prototype)", "performance"]) expr(`PerformanceObserver.prototype.${m}.call(${t}, {})`);
  expr(`PerformanceObserver.prototype.${m}()`);
}
// observe: validação, ordem de leitura, modo.
const O = "var o = new PerformanceObserver(function () {}); ";
for (const opt of [
  "", "undefined", "null", "{}", "[]", "1", "'mark'", "true", "Symbol()", "1n", "function () {}",
  "{ entryTypes: ['mark'] }", "{ entryTypes: [] }", "{ entryTypes: ['foo'] }", "{ entryTypes: ['foo', 'mark'] }", "{ entryTypes: 'mark' }",
  "{ entryTypes: null }", "{ entryTypes: 5 }", "{ entryTypes: {} }", "{ entryTypes: { length: 1, 0: 'mark' } }", "{ entryTypes: [1, {}] }",
  "{ entryTypes: [{ toString() { throw new RangeError('b') } }] }", "{ entryTypes: ['mark', 'mark', 'measure', 'resource'] }",
  "{ type: 'mark' }", "{ type: 'foo' }", "{ type: 5 }", "{ type: null }", "{ type: undefined }", "{ type: {} }", "{ type: Symbol() }",
  "{ type: { toString() { throw new RangeError('b') } } }", "{ type: 'resource' }", "{ type: 'measure' }",
  "{ type: 'mark', entryTypes: ['mark'] }", "{ type: 'foo', entryTypes: ['mark'] }", "{ type: undefined, entryTypes: ['mark'] }", "{ type: 'mark', entryTypes: undefined }",
  "{ buffered: true }", "{ buffered: true, type: 'mark' }", "{ buffered: true, entryTypes: ['mark'] }", "{ type: 'mark', durationThreshold: 5 }",
  "{ type: 'mark', durationThreshold: -5 }", "{ type: 'mark', durationThreshold: 'x' }", "{ type: 'mark', durationThreshold: Symbol() }",
  "{ get buffered() { throw new RangeError('b') }, type: 'mark' }", "{ get entryTypes() { throw new RangeError('e') } }", "{ get type() { throw new RangeError('t') } }",
]) expr(`(function () { ${O} return o.observe(${opt}) })()`);
expr("(function () { var l = []; new PerformanceObserver(function () {}).observe({ get type() { l.push('type'); return 'mark' }, get entryTypes() { l.push('entryTypes'); return undefined }, get buffered() { l.push('buffered') }, get durationThreshold() { l.push('durationThreshold') } }); return l })()");
expr("(function () { var l = []; try { new PerformanceObserver(function () {}).observe({ get type() { l.push('type'); return 'mark' }, get entryTypes() { l.push('entryTypes'); return 5 }, get buffered() { l.push('buffered') } }) } catch (e) {} return l })()");
for (const [a, b] of [
  ["{ entryTypes: ['mark'] }", "{ entryTypes: ['measure'] }"], ["{ type: 'mark' }", "{ type: 'measure' }"],
  ["{ type: 'mark' }", "{ entryTypes: ['measure'] }"], ["{ entryTypes: ['mark'] }", "{ type: 'measure' }"],
  ["{ entryTypes: [] }", "{ type: 'measure' }"], ["{ type: 'foo' }", "{ entryTypes: ['measure'] }"],
])
  expr(`(function () { ${O} o.observe(${a}); return o.observe(${b}) })()`);
expr("(function () { " + O + " o.observe({ entryTypes: ['mark'] }); o.disconnect(); return o.observe({ type: 'measure' }) })()");
expr("(function () { " + O + " o.observe({ type: 'mark' }); o.disconnect(); return o.observe({ entryTypes: ['measure'] }) })()");
expr("(function () { " + O + " o.observe({ type: 'mark' }); try { o.observe({ entryTypes: ['mark'] }) } catch (e) { return [e instanceof DOMException, e.name, e.code, e.message, e.constructor === DOMException, Object.prototype.toString.call(e)] } })()");
expr("(function () { " + O + " return [o.disconnect(), o.disconnect(), o.takeRecords()] })()");
expr("(function () { " + O + " var r = o.takeRecords(); return [Array.isArray(r), r.length, Object.getPrototypeOf(r) === Array.prototype] })()");
// takeRecords: o que as marcas e medidas entregam.
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); performance.mark('a'); performance.mark('b'); performance.measure('m'); var r = o.takeRecords(); var again = o.takeRecords(); return [r.map(function (e) { return e.entryType + ':' + e.name }), again.length, r[0] instanceof PerformanceMark, r[0] === performance.getEntries()[0]] })()");
expr("(function () { " + O + " C(); o.observe({ type: 'measure' }); performance.mark('a'); performance.measure('m'); return o.takeRecords().map(function (e) { return e.entryType + ':' + e.name }) })()");
expr("(function () { " + O + " C(); o.observe({ type: 'mark' }); o.observe({ type: 'measure' }); performance.mark('a'); performance.measure('m'); return o.takeRecords().map(function (e) { return e.entryType + ':' + e.name }) })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark', 'measure'] }); o.observe({ entryTypes: ['measure'] }); performance.mark('a'); performance.measure('m'); return o.takeRecords().map(function (e) { return e.entryType + ':' + e.name }) })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['resource'] }); performance.mark('a'); return o.takeRecords().length })()");
expr("(function () { " + O + " C(); performance.mark('before'); o.observe({ entryTypes: ['mark'] }); return o.takeRecords().length })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); performance.mark('a'); o.disconnect(); return o.takeRecords().length })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); o.disconnect(); performance.mark('a'); return o.takeRecords().length })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); new PerformanceMark('x'); return o.takeRecords().length })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); performance.mark('a'); performance.clearMarks(); return o.takeRecords().length })()");
expr("(function () { " + O + " var p = new PerformanceObserver(function () {}); C(); o.observe({ entryTypes: ['mark'] }); p.observe({ entryTypes: ['mark'] }); performance.mark('a'); return [o.takeRecords().length, p.takeRecords().length] })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); performance.mark('a'); var r = o.takeRecords()[0]; return [r.name, r.entryType, r.duration, typeof r.startTime, r.detail, JSON.stringify(r.toJSON()).replace(/\"startTime\":[-0-9.e+]+/, '\"startTime\":N')] })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: ['mark'] }); performance.mark('a', { detail: { k: 1 } }); return JSON.stringify(o.takeRecords()[0].detail) })()");

// supportedEntryTypes: um array novo e congelado a cada leitura.
expr("PerformanceObserver.supportedEntryTypes === PerformanceObserver.supportedEntryTypes");
expr("(function () { var a = PerformanceObserver.supportedEntryTypes, b = PerformanceObserver.supportedEntryTypes; return [a === b, Object.isFrozen(a), Object.isFrozen(b), JSON.stringify(a) === JSON.stringify(b), Array.isArray(b)] })()");
expr("D(PerformanceObserver, 'supportedEntryTypes')");
expr("Object.getOwnPropertyDescriptor(PerformanceObserver, 'supportedEntryTypes').value === Object.getOwnPropertyDescriptor(PerformanceObserver, 'supportedEntryTypes').value");
// entryTypes: qualquer iterável (o que não é objeto, ou não tem @@iterator chamável, falha).
for (const v of [
  "new Set(['mark'])", "new Set(['measure', 'mark'])", "new Map()", "new Map([['mark', 1]])", "(function* () { yield 'mark' })()",
  "{ [Symbol.iterator]: function* () { yield 'measure' } }", "{ [Symbol.iterator]: 5 }", "{ [Symbol.iterator]: null }",
  "[1]", "'mark'", "new String('mark')", "{ length: 1, 0: 'mark' }", "{}", "[]", "new Set([{ toString() { throw new RangeError('b') } }])",
  "{ [Symbol.iterator]() { return { next() { throw new RangeError('n') } } } }",
])
  expr(`(function () { ${O} try { return [o.observe({ entryTypes: ${v} }), o.takeRecords().length] } catch (e) { return E(e) } })()`);
expr("(function () { " + O + " C(); o.observe({ entryTypes: new Set(['measure']) }); performance.mark('a'); performance.measure('m'); return o.takeRecords().map(function (e) { return e.entryType + ':' + e.name }) })()");
expr("(function () { " + O + " C(); o.observe({ entryTypes: (function* () { yield 'mark'; yield 'measure' })() }); performance.mark('a'); performance.measure('m'); return o.takeRecords().map(function (e) { return e.entryType + ':' + e.name }) })()");

// Entrega ao callback: uma tarefa do host, depois das microtasks e antes dos timers. O log `L` vira `R` num timer de 30 ms.
const task = (code) => programs.push(HELPER + "var L = []; " + code + " setTimeout(function () { R = L.join(';') }, 30);");
const NAMES = "function (l) { return l.getEntries().map(function (e) { return e.name }).join(',') }";
task("var n = " + NAMES + "; var o = new PerformanceObserver(function (l, ob) { L.push('cb:' + n(l) + ':' + (ob === o) + ':' + (this === o) + ':' + arguments.length) }); o.observe({ type: 'mark' }); performance.mark('a'); performance.mark('b'); L.push('sync');");
task("var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'mark' }); performance.mark('a'); Promise.resolve().then(function () { L.push('promise') }); queueMicrotask(function () { L.push('qm') }); L.push('sync');");
task("var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'mark' }); setTimeout(function () { L.push('timeout') }, 0); setImmediate(function () { L.push('immediate') }); performance.mark('a');");
task("setTimeout(function () { L.push('timeout-before') }, 0); var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'mark' }); performance.mark('a'); setTimeout(function () { L.push('timeout-after') }, 0);");
task("var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'mark' }); setTimeout(function () { L.push('t1'); performance.mark('b') }, 1); setTimeout(function () { L.push('t1b') }, 1); setTimeout(function () { L.push('t2') }, 2);");
task("var o = new PerformanceObserver(function (l) { L.push('cb' + l.getEntries().length) }); o.observe({ type: 'mark' }); setImmediate(function () { performance.mark('a'); L.push('imm') }); setTimeout(function () { L.push('t5') }, 5);");
task("var n = " + NAMES + "; var a = new PerformanceObserver(function (l) { L.push('A:' + n(l)) }), b = new PerformanceObserver(function (l) { L.push('B:' + n(l)) }), c = new PerformanceObserver(function (l) { L.push('C:' + n(l)) }); c.observe({ type: 'mark' }); b.observe({ entryTypes: ['mark', 'measure'] }); a.observe({ type: 'measure' }); performance.mark('m1'); performance.measure('s1'); performance.mark('m2');");
task("var n = " + NAMES + "; var a = new PerformanceObserver(function (l) { L.push('A:' + n(l)) }), b = new PerformanceObserver(function (l) { L.push('B:' + n(l)) }), d = new PerformanceObserver(function (l) { L.push('D:' + n(l)) }), e = new PerformanceObserver(function (l) { L.push('E:' + n(l)) }); a.observe({ type: 'mark' }); b.observe({ type: 'mark' }); d.observe({ type: 'mark' }); e.observe({ type: 'mark' }); performance.mark('x1'); L.push('take:' + d.takeRecords().length); performance.mark('x2'); e.disconnect(); L.push('sync');");
task("var n = " + NAMES + "; var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)); if (!this.done) { this.done = true; performance.mark('inner') } }); o.observe({ type: 'mark' }); performance.mark('a');");
task("var o = new PerformanceObserver(function (l) { L.push('cb'); queueMicrotask(function () { L.push('micro-in-cb') }) }), p = new PerformanceObserver(function (l) { L.push('p') }); o.observe({ type: 'mark' }); p.observe({ type: 'mark' }); performance.mark('a');");
task("var o = new PerformanceObserver(function () { L.push('cb1'); throw new Error('boom') }), p = new PerformanceObserver(function () { L.push('cb2') }); o.observe({ type: 'mark' }); p.observe({ type: 'mark' }); performance.mark('a'); setTimeout(function () { L.push('after') }, 1);");
task("var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'mark' }); performance.mark('a'); o.takeRecords(); L.push('taken');");
task("var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'mark' }); performance.mark('a'); o.disconnect(); o.observe({ type: 'mark' }); L.push('reobserved');");
task("var o = new PerformanceObserver(function () { L.push('cb') }); o.observe({ type: 'resource' }); performance.mark('a'); performance.measure('m'); L.push('sync');");

// buffered: true (só com `type`): o buffer do tipo, depois a fila, callback síncrono dentro de `observe`.
const BUF = "var n = " + NAMES + "; ";
task(BUF + "performance.mark('pre'); performance.measure('pre-m'); var o = new PerformanceObserver(function (l, ob) { L.push('cb:' + n(l) + ':' + (this === o) + ':' + (ob === o)) }); o.observe({ type: 'mark', buffered: true }); L.push('observed');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark', buffered: true }); performance.mark('post'); L.push('observed');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ entryTypes: ['mark'], buffered: true }); L.push('observed');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark', buffered: false }); L.push('observed');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'measure', buffered: true }); L.push('observed');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'resource', buffered: true }); L.push('observed');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'foo', buffered: true }); L.push('observed');");
task(BUF + "performance.mark('pre1'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark' }); performance.mark('x'); o.observe({ type: 'mark', buffered: true }); L.push('end');");
task(BUF + "var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark', buffered: true }); L.push('empty'); performance.mark('a'); o.observe({ type: 'mark', buffered: true }); L.push('end');");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark', buffered: 1 }); L.push('observed-' + o.takeRecords().length);");
task(BUF + "performance.mark('pre'); performance.measure('pre-m'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark', buffered: true }); o.observe({ type: 'measure', buffered: true }); L.push('observed');");
task(BUF + "performance.mark('a', { startTime: 5 }); performance.mark('b', { startTime: 1 }); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)) }); o.observe({ type: 'mark', buffered: true });");
task("var m; var o = new PerformanceObserver(function (l) { var e = l.getEntries()[0]; L.push([e === m, e.x, l.getEntriesByName('b')[0] === m, l.getEntries() === l.getEntries()].join(',')) }); o.observe({ type: 'mark' }); m = performance.mark('b'); m.x = 7;");
task("var m = performance.mark('pre'); m.x = 3; var o = new PerformanceObserver(function (l) { var e = l.getEntries()[0]; L.push([e === m, e.x].join(',')) }); o.observe({ type: 'mark', buffered: true });");
task(BUF + "performance.mark('pre'); var o = new PerformanceObserver(function (l) { L.push('cb:' + n(l)); L.push('take:' + o.takeRecords().length) }); o.observe({ type: 'mark', buffered: true }); L.push('take-after:' + o.takeRecords().length);");

// A PerformanceObserverEntryList entregue ao callback.
const LIST = (body) => task("var o = new PerformanceObserver(function (l) { " + body + " }); o.observe({ type: 'measure' }); performance.measure('q3'); performance.measure('q4'); performance.mark('ignored');");
LIST("var x = l.getEntries(); L.push([x.length, Array.isArray(x), Object.isFrozen(x), x === l.getEntries(), x[0] instanceof PerformanceMeasure, x[0].name, x[1].name].join(','));");
LIST("L.push([Object.prototype.toString.call(l), l.constructor === PerformanceObserverEntryList, l instanceof PerformanceObserverEntryList, Reflect.ownKeys(l).length, Object.getPrototypeOf(l) === PerformanceObserverEntryList.prototype].join(','));");
LIST("L.push([l.getEntries.length, l.getEntriesByType.length, l.getEntriesByName.length].join(','));");
LIST("L.push(l.getEntriesByType('measure').length, l.getEntriesByType('mark').length, l.getEntriesByType('zzz').length, l.getEntriesByType(5).length, l.getEntriesByType('measure') === l.getEntriesByType('measure'));");
LIST("L.push(l.getEntriesByName('q3').length, l.getEntriesByName('q3', 'measure').length, l.getEntriesByName('q3', 'mark').length, l.getEntriesByName('q3', undefined).length, l.getEntriesByName('nope').length, l.getEntriesByName(undefined).length, l.getEntriesByName(null, null).length);");
LIST("var f = function (g) { try { return String(g()) } catch (e) { return E(e) } }; L.push(f(function () { return l.getEntriesByType() }), f(function () { return l.getEntriesByName() }));");
LIST("var f = function (g) { try { return String(g()) } catch (e) { return E(e) } }; L.push(f(function () { return l.getEntries.call({}) }), f(function () { return l.getEntriesByType.call(null, 'mark') }), f(function () { return l.getEntriesByName.call(PerformanceObserverEntryList.prototype, 'q3') }));");
LIST("var x = l.getEntries(); x.push(1); x.length = 0; L.push(l.getEntries().length, l.getEntriesByType('measure').length);");
LIST("L.push(JSON.stringify(l.getEntries().map(function (e) { return [e.name, e.entryType, e.duration >= 0] })));");
LIST("L.push(typeof l.getEntries()[0].toJSON, l.getEntries()[0].toJSON().name);");
LIST("var k = l; setTimeout(function () { L.push('later:' + k.getEntries().length) }, 1);");

// O callback do observador que lança (casos acima) vira exceção não capturada: o gerador a engole com um ouvinte
// de `uncaughtException`, e `R` só é lido na saída, depois que o laço de eventos esvaziou.
for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pf-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `process.on("uncaughtException", () => {});\nprocess.on("exit", () => require("fs").writeSync(1, JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R))));\n(0, eval)("var R");\n(0, eval)(${JSON.stringify(sourceAscii)});\n`,
  );
  const run = spawnSync(process.execPath, [file], { encoding: "utf8" });
  fs.rmSync(dir, { recursive: true, force: true });
  if (run.status !== 0) throw new Error("bun falhou em: " + sourceAscii + "\n" + run.stderr);
  emitRow(JSON.stringify(sourceAscii) + "\t" + run.stdout);
}
