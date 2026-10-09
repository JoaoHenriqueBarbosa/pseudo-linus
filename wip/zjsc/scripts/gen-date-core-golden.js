// Gera tests/golden/date_core_bun.tsv: núcleo de Date (construtor com 0..7 argumentos, valores extremos, NaN e strings
// ISO, Date.UTC, setters locais e UTC, getters UTC, limites de ±8.64e15, anos negativos e acima de 9999, serialização
// toISOString/toJSON/toUTCString/toString/toDateString/toTimeString, Symbol.toPrimitive e hints, aritmética) medido no
// bun 1.4.2. Parse legado e Intl ficam fora (date_edge, date_legacy_parse, datetime_edge já cobrem).
// Independente de fuso: cada programa roda em cinco fusos (UTC, São Paulo, Kolkata, Auckland, Nova York) e só entra
// no golden se o resultado for idêntico em todos. Nada local é impresso diretamente. Valores de construtor local passam por `N`, que desfaz o
// deslocamento (`getTime() - getTimezoneOffset() * 60000`); datas locais ficam em meados de junho, ao meio-dia, longe
// de transição de horário de verão. O texto de toString/toDateString/toTimeString corta o sufixo de fuso.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-date-core-golden.js > tests/golden/date_core_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
// Expressão avaliada e convertida em texto; Object.is distingue -0 de 0 via a função `S`.
const PRE = "var S = v => Object.is(v, -0) ? '-0' : typeof v === 'string' ? JSON.stringify(v) : String(v); " +
  "var N = d => d.getTime() - d.getTimezoneOffset() * 60000; " +
  "var Z = d => d.toString().replace(/ GMT.*$/, ''); ";
const E = expr => T(`${PRE}R = S(${expr})`);

const J = JSON.stringify;
const MAX = 8.64e15;

// ---- Construtor local com 0..7 argumentos (normalizado por N).
const ctorArgs = [
  [2020], [2020, 0], [2020, 5], [2020, 5, 15], [2020, 5, 15, 12], [2020, 5, 15, 12, 30], [2020, 5, 15, 12, 30, 45],
  [2020, 5, 15, 12, 30, 45, 678], [0, 5, 15, 12], [99, 5, 15, 12], [100, 5, 15, 12], [-1, 5, 15, 12], [1969, 5, 15, 12],
  [275760, 8, 13, 0], [275760, 8, 13, 0, 0, 0, 1], [-271821, 3, 20, 0], [-271821, 3, 19, 23, 59, 59, 999],
  [2020, 12, 15, 12], [2020, -1, 15, 12], [2020, 5, 0, 12], [2020, 5, 31, 12], [2020, 5, 15, 25], [2020, 5, 15, 12, 61],
  [2020, 5, 15, 12, 30, 61], [2020, 5, 15, 12, 30, 45, 1500], [2020, 5, 15, 12, 30, 45, -500],
  [2020.9, 5.9, 15.9, 12.9, 30.9, 45.9, 678.9], [-2020.9, 5, 15, 12], [NaN], [2020, NaN], [2020, 5, NaN], [2020, 5, 15, NaN],
  [Infinity], [-Infinity], [2020, Infinity], [2020, 5, 15, 12, 30, 45, -Infinity], [1e20], [-1e20], [1e10, 5, 15],
  [2020, 5, 15, 12, 30, 45, 678, 999], [undefined], [2020, undefined], [2020, 5, undefined], [null], [2020, null],
  [true, false], ["2020", "5", "15", "12"], ["", 5], [{}, 5], [[2020], 5, 15, 12], [2020, 5, 15, 12, 0, 0, 0.5],
  [2020, 5, 1e9], [2020, 5, -1e9], [2020, 1e6], [2020, -1e6], [1e6, 0], [-1e6, 0], [Number.MAX_VALUE], [Number.MIN_VALUE],
  [2020, 5, 15, 12, 30, 45, 2 ** 53], [-0], [0, -0],
];
for (const args of ctorArgs) {
  const list = args.map(a => (typeof a === "number" && Object.is(a, -0) ? "-0" : a === undefined ? "undefined" : typeof a === "number" && !isFinite(a) ? String(a) : J(a))).join(", ");
  E(`N(new Date(${list}))`);
  E(`new Date(${list}).getHours()`);
  E(`new Date(${list}).getFullYear()`);
}
// Date(...) como função devolve texto (só o formato).
T(`${PRE}R = S(typeof Date())`);
T(`${PRE}R = S(typeof Date(2020, 5, 15))`);
T(`${PRE}R = S(Date(2020).length > 20)`);
T(`${PRE}R = S(/^[A-Z][a-z]{2} [A-Z][a-z]{2} \\d\\d \\d{4} \\d\\d:\\d\\d:\\d\\d GMT[+-]\\d{4}/.test(Date()))`);

// ---- Construtor com um argumento (valor de tempo) e extremos.
const timeValues = [0, -0, 1, -1, 1.9, -1.9, 0.5, -0.5, 1e3, MAX, -MAX, MAX + 1, -MAX - 1, MAX - 1, -MAX + 1, MAX + 0.5, MAX + 0.9999,
  2 ** 53, 2 ** 53 + 2, -(2 ** 53), 1e15, 1e16, 8.64e15 + 1, 253402300799999, 253402300800000, -62198755200000, -62198755200001,
  -62167219200000, -62167219200001, 1e12, 1.7e12, 951782400000, 4102444800000, NaN, Infinity, -Infinity, 1e300];
for (const t of timeValues) {
  const lit = Object.is(t, -0) ? "-0" : String(t);
  E(`new Date(${lit}).getTime()`);
  E(`new Date(${lit}).valueOf()`);
  E(`new Date(${lit}).toISOString()`);
  E(`J(new Date(${lit}))`.replace("J(", "JSON.stringify("));
  E(`new Date(${lit}).toUTCString()`);
  E(`new Date(${lit}).getUTCFullYear()`);
  E(`new Date(${lit}).getUTCDay()`);
  E(`new Date(${lit}).getUTCMilliseconds()`);
}
// ---- Construtor com string ISO, número como texto, boolean, null, objetos e cópia de Date.
const isoStrings = [
  "2020-06-15T12:30:45.678Z", "2020-06-15T12:30:45Z", "2020-06-15T12:30Z", "2020-06-15", "2020-06", "2020", "+002020-06-15T00:00:00Z",
  "-000001-01-01T00:00:00Z", "-000000-01-01T00:00:00Z", "+275760-09-13T00:00:00.000Z", "+275760-09-13T00:00:00.001Z",
  "-271821-04-20T00:00:00.000Z", "-271821-04-19T23:59:59.999Z", "2020-06-15T12:30:45.678+01:00", "2020-06-15T12:30:45-05:30",
  "2020-06-15T24:00:00Z", "2020-06-15T24:00:01Z", "2020-06-15T12:30:45.1Z", "2020-06-15T12:30:45.12345Z", "2020-13-01", "2020-00-01",
  "2020-06-32", "2020-02-30", "2020-02-29", "2019-02-29", "2020-06-15T25:00Z", "2020-06-15T12:60Z", "2020-06-15T12:30:60Z",
  "2020-06-15T12:30:45.678", "+2020-06-15", "-2020-06-15", "20200615", "2020-6-15", "2020-06-15T", "2020-06-15T12", "T12:30",
  "0000-01-01T00:00:00Z", "0001-01-01T00:00:00Z", "9999-12-31T23:59:59.999Z", "10000-01-01T00:00:00Z", "+010000-01-01T00:00:00Z",
  "", " ", "abc", "NaN", "Infinity", "null", "undefined", "2020-06-15T12:30:45.678Zjunk", "2020-06-15T12:30:45z", "2020-06-15t12:30:45Z",
  "2020-06-15 12:30:45Z", "1e3", "12345", "0",
];
for (const s of isoStrings) {
  // Strings sem fuso (forma só-data é UTC; data-hora sem fuso é local): normaliza a segunda.
  E(`isNaN(new Date(${J(s)})) ? 'NaN' : N(new Date(${J(s)})) - (/T\\d\\d:\\d\\d/.test(${J(s)}) && !/(Z|z|[+-]\\d\\d:\\d\\d)$/.test(${J(s)}) ? 0 : new Date(${J(s)}).getTimezoneOffset() * 60000 * 0)`);
  E(`isNaN(Date.parse(${J(s)})) ? 'NaN' : Date.parse(${J(s)}) === new Date(${J(s)}).getTime()`);
  if (/Z$|[+-]\d\d:\d\d$|^[+-]?\d{4,6}(-\d\d(-\d\d)?)?$/.test(s)) {
    E(`isNaN(new Date(${J(s)})) ? 'NaN' : new Date(${J(s)}).toISOString()`);
    E(`isNaN(new Date(${J(s)})) ? 'NaN' : new Date(${J(s)}).getTime()`);
  }
}
for (const v of ["true", "false", "null", "undefined", "{}", "[]", "[5]", "[1, 2]", "'12'", "12n", "Symbol()", "{ valueOf() { return 5 } }",
  "{ toString() { return '2020-06-15T00:00:00Z' } }", "{ valueOf() { return {} }, toString() { return '7' } }",
  "{ [Symbol.toPrimitive]() { return 9 } }", "{ [Symbol.toPrimitive]() { return '2020-01-01T00:00:00Z' } }",
  "{ [Symbol.toPrimitive]: 1 }", "{ [Symbol.toPrimitive]() { return {} } }", "new Date(77)", "new Date(NaN)", "new Number(4)", "new String('2000-01-01T00:00:00Z')",
  "new Boolean(true)", "function () {}", "/x/", "new Date(1e3)"]) {
  T(`${PRE}var d = new Date(${v}); R = S(d.getTime())`);
}
T(`${PRE}var a = new Date(5); var b = new Date(a); a.setTime(9); R = S(b.getTime() + ',' + a.getTime())`);
T(`${PRE}class D extends Date {} var d = new D(5); R = S(d.getTime() + ',' + (d instanceof Date) + ',' + Object.getPrototypeOf(d) === D.prototype)`);
T(`${PRE}var d = Reflect.construct(Date, [5], Object); R = S(Object.getPrototypeOf(d) === Object.prototype) + S(typeof d.getTime)`);
T(`${PRE}var d = Reflect.construct(Date, [5], Object); R = S(Date.prototype.getTime.call(d))`);
T(`${PRE}R = S(Object.prototype.toString.call(new Date(0)))`);
T(`${PRE}R = S(Date.length) + S(Date.name) + S(Date.UTC.length) + S(Date.parse.length) + S(Date.now.length)`);
T(`${PRE}R = S(Object.getOwnPropertyNames(Date).sort().join())`);
T(`${PRE}R = S(Object.getOwnPropertyNames(Date.prototype).sort().join())`);
T(`${PRE}R = S(Object.getOwnPropertyDescriptor(Date, 'prototype').writable) + S(Object.getOwnPropertyDescriptor(Date, 'prototype').configurable)`);
T(`${PRE}R = S(typeof Date.now()) + S(Number.isInteger(Date.now())) + S(Date.now() > 1.6e12) + S(Date.now() < MAX)`.replace("MAX", String(MAX)));
T(`${PRE}var a = Date.now(); var b = new Date().getTime(); R = S(b >= a) + S(b - a < 5000) + S(Number.isInteger(b))`);
T(`${PRE}R = S(new Date().getTime() === +new Date()) `.replace(") ", ") && 1").replace("S(new Date().getTime() === +new Date()) && 1", "S(typeof +new Date())"));
T(`${PRE}R = S(typeof new Date().valueOf()) + S(typeof new Date().toISOString())`);
T(`${PRE}R = S(typeof Date.UTC()) + S(Date.UTC()) `);
T(`${PRE}new Date(5)(); `);
T(`${PRE}R = S(Date.prototype.getTime.call({}))`);
T(`${PRE}R = S(Date.prototype.getTime.call(5))`);
T(`${PRE}R = S(Date.prototype.toISOString.call({}))`);
T(`${PRE}R = S(Date.prototype.toString.call({}))`);
T(`${PRE}R = S(Date.prototype.toUTCString.call({}))`);
T(`${PRE}R = S(Date.prototype.setTime.call({}, 1))`);
T(`${PRE}R = S(Date.prototype.toJSON.call({ toISOString() { return 'x' }, valueOf() { return 1 } }))`);
T(`${PRE}R = S(Date.prototype.toJSON.call({ toISOString() { return 'x' }, valueOf() { return NaN } }))`);
T(`${PRE}R = S(Date.prototype.toJSON.call({ toISOString() { return 'x' }, valueOf() { return Infinity } }))`);
T(`${PRE}R = S(Date.prototype.toJSON.call({ toISOString: 1, valueOf() { return 1 } }))`);
T(`${PRE}R = S(Date.prototype.toJSON.call({ valueOf() { return 1 } }))`);
T(`${PRE}R = S(Date.prototype.toJSON.call({ toISOString() { return 'x' }, [Symbol.toPrimitive]() { return 5 } }))`);
T(`${PRE}R = S(Date.prototype.toJSON.call(5))`);
T(`${PRE}R = S(Date.prototype.toJSON.call(null))`);
T(`${PRE}R = S(Date.prototype.toJSON.call(new Date(NaN)))`);
T(`${PRE}R = S(Date.prototype.toJSON.call(new Date(0)))`);
T(`${PRE}R = S(JSON.stringify({ d: new Date(0), e: new Date(NaN) }))`);
T(`${PRE}R = S(JSON.stringify([new Date(MAX)]))`.replace("MAX", String(MAX)));

// ---- Date.UTC.
const utcArgs = [
  [], [2020], [2020, 0], [2020, 5, 15], [2020, 5, 15, 12], [2020, 5, 15, 12, 30], [2020, 5, 15, 12, 30, 45], [2020, 5, 15, 12, 30, 45, 678],
  [2020, 5, 15, 12, 30, 45, 678, 999], [0], [0, 0], [99], [100], [-1], [50], [1900], [1970], [1970, 0, 1], [1969, 11, 31, 23, 59, 59, 999],
  [275760, 8, 13], [275760, 8, 13, 0, 0, 0, 1], [275760, 8, 14], [-271821, 3, 20], [-271821, 3, 19, 23, 59, 59, 999], [-271821, 3, 19],
  [2020, 12], [2020, -1], [2020, 0, 0], [2020, 0, -1], [2020, 0, 32], [2020, 1, 30], [2020, 1, 29], [2019, 1, 29], [2020, 5, 15, 24], [2020, 5, 15, -1],
  [2020, 5, 15, 12, 60], [2020, 5, 15, 12, -1], [2020, 5, 15, 12, 30, 60], [2020, 5, 15, 12, 30, 45, 1000], [2020, 5, 15, 12, 30, 45, -1],
  [NaN], [2020, NaN], [2020, 5, NaN], [2020, 5, 15, NaN], [2020, 5, 15, 12, NaN], [2020, 5, 15, 12, 30, NaN], [2020, 5, 15, 12, 30, 45, NaN],
  [Infinity], [-Infinity], [2020, Infinity], [2020, 5, -Infinity], [1e20], [1e10, 0], [-1e10, 0], [2020.9], [-2020.9], [2020.9, 5.9, 15.9, 12.9, 30.9, 45.9, 678.9],
  [-0], [0, -0], [undefined], [2020, undefined], [2020, 5, undefined], [null], [2020, null], [2020, 5, null], [true], [true, true, true], ["2020", "5", "15"],
  ["abc"], [2020, "abc"], [{}], [[2020, 5]], [2020, 5, 15, 12, 30, 45, 2 ** 53], [2020, 0, 1e8], [2020, 0, 1e9], [2020, 0, -1e9], [2020, 1e5], [2020, -1e5],
  [1e5, 0], [270000, 0], [-270000, 0], [275761], [-271822], [275760, 0], [275760, 11], [275760, 9], [-271821, 0], [-271821, 3], [-271821, 4],
  [2020, 5, 15, 12, 30, 45, 678.5], [2020, 5, 15, 12, 30, 45, -0.5], [1970, 0, 1, 0, 0, 0, -1], [1970, 0, 1, 0, 0, 0, -0.5],
  [2020, 5, 15, 12, 30, 45, 0, 7, 8, 9],
];
for (const args of utcArgs) {
  const list = args.map(a => (typeof a === "number" && Object.is(a, -0) ? "-0" : a === undefined ? "undefined" : typeof a === "number" && !isFinite(a) ? String(a) : J(a))).join(", ");
  E(`Date.UTC(${list})`);
  E(`new Date(Date.UTC(${list})).toISOString()`);
}
T(`${PRE}R = S(Date.UTC.call(null, 2020))`);
T(`${PRE}R = S(Date.UTC.call(undefined, 2020, 5))`);
T(`${PRE}R = S(Date.UTC(2020, { valueOf() { return 5 } }))`);
T(`${PRE}var log = []; Date.UTC({ valueOf() { log.push('y'); return 2020 } }, { valueOf() { log.push('m'); return 1 } }, { valueOf() { log.push('d'); return 1 } }); R = S(log.join())`);
T(`${PRE}var log = []; Date.UTC({ valueOf() { log.push('y'); return NaN } }, { valueOf() { log.push('m'); return 1 } }); R = S(log.join())`);
T(`${PRE}R = S(Date.UTC(Symbol()))`);
T(`${PRE}R = S(Date.UTC(1n))`);
T(`${PRE}R = S(Date.UTC(2020, 1n))`);

// ---- Getters UTC e locais sobre a mesma data (valores de campo, não de instante).
const probeTimes = [0, -1, 1, 86399999, 86400000, -86400000, 951782400000, 951868800000, 1582934400000, 1709164800000, 1e12, -1e12, 1.7e12, 4102444799999,
  MAX, -MAX, MAX - 86400000, -MAX + 86400000, -62167219200000, -62167219200001, 253402300799999, 253402300800000, -62198755200000, -377705116800000,
  -8.64e15 + 1, 1000000000000000, -1000000000000000, 31536000000, -31536000000, 1e14, 78e11, 951782400001];
const getters = ["FullYear", "Month", "Date", "Day", "Hours", "Minutes", "Seconds", "Milliseconds"];
for (const t of probeTimes) {
  for (const g of getters) E(`new Date(${t}).getUTC${g}()`);
  E(`new Date(${t}).getYear() - new Date(${t}).getFullYear()`);
  E(`new Date(${t}).toISOString()`);
  E(`new Date(${t}).toUTCString()`);
  E(`new Date(${t}).toJSON()`);
  E(`new Date(${t}).getTime() % 86400000`);
  // toString local: só o prefixo do dia da semana depende do fuso, então compara pela forma.
  E(`new Date(${t}).toString().replace(/^[A-Z][a-z]{2} [A-Z][a-z]{2} \\d\\d -?\\d{4,6} \\d\\d:\\d\\d:\\d\\d GMT[+-]\\d{4}.*$/, 'shape-ok')`);
  E(`new Date(${t}).toDateString().replace(/^[A-Z][a-z]{2} [A-Z][a-z]{2} \\d\\d -?\\d{4,6}$/, 'shape-ok')`);
  E(`new Date(${t}).toTimeString().replace(/^\\d\\d:\\d\\d:\\d\\d GMT[+-]\\d{4}( \\(.*\\))?$/, 'shape-ok')`);
}
E(`new Date(NaN).toString()`);
E(`new Date(NaN).toDateString()`);
E(`new Date(NaN).toTimeString()`);
E(`new Date(NaN).toUTCString()`);
E(`new Date(NaN).toGMTString()`);
E(`new Date(NaN).toJSON()`);
E(`new Date(NaN).toISOString()`);
E(`new Date(NaN).getTimezoneOffset()`);
E(`new Date(NaN).getYear()`);
E(`new Date(NaN).getTime()`);
E(`new Date(NaN).valueOf()`);
E(`new Date(NaN).getUTCDay()`);
E(`new Date(NaN).getFullYear()`);
E(`Date.prototype.toGMTString === Date.prototype.toUTCString`);
E(`Date.prototype.toString.name + Date.prototype.toGMTString.name`);
E(`Date.prototype.getYear.length + ',' + Date.prototype.setYear.length + ',' + Date.prototype[Symbol.toPrimitive].length + ',' + Date.prototype[Symbol.toPrimitive].name`);

// Serialização com campos construídos (independe de fuso) em anos fora do comum.
const serYears = [0, 1, 9, 99, 100, 999, 1000, 1900, 1970, 9999, 10000, 12345, 99999, 100000, 275760, -1, -9, -99, -999, -1000, -9999, -10000, -99999, -271821];
for (const y of serYears) {
  E(`new Date(Date.UTC(${y}, 5, 15, 12, 30, 45)).toISOString()`);
  E(`new Date(Date.UTC(${y}, 5, 15, 12, 30, 45)).toUTCString()`);
  E(`new Date(Date.UTC(${y}, 5, 15, 12, 30, 45)).toJSON()`);
  E(`Z(new Date(${y}, 5, 15, 12, 30, 45))`);
  E(`new Date(${y}, 5, 15, 12, 30, 45).toDateString()`);
  E(`new Date(${y}, 5, 15, 12, 30, 45).toTimeString().replace(/ GMT.*$/, '')`);
  E(`new Date(${y}, 5, 15, 12, 30, 45).getFullYear()`);
}
// Extremos de ISO e UTC: o último milissegundo válido e o primeiro inválido.
for (const [y, m, d, h, mi, s, ms] of [[275760, 8, 13, 0, 0, 0, 0], [275760, 8, 12, 23, 59, 59, 999], [-271821, 3, 20, 0, 0, 0, 0], [-271821, 3, 19, 23, 59, 59, 999], [9999, 11, 31, 23, 59, 59, 999], [10000, 0, 1, 0, 0, 0, 0], [-1, 11, 31, 23, 59, 59, 999], [0, 0, 1, 0, 0, 0, 0]]) {
  E(`new Date(Date.UTC(${y}, ${m}, ${d}, ${h}, ${mi}, ${s}, ${ms})).toISOString()`);
  E(`new Date(Date.UTC(${y}, ${m}, ${d}, ${h}, ${mi}, ${s}, ${ms})).toUTCString()`);
  E(`new Date(Date.UTC(${y}, ${m}, ${d}, ${h}, ${mi}, ${s}, ${ms})).getTime()`);
}

// ---- Setters UTC e locais (campos normalizados por N/UTC).
const base = "var d = new Date(Date.UTC(2020, 5, 15, 12, 30, 45, 678)); ";
const baseLocal = "var d = new Date(2020, 5, 15, 12, 30, 45, 678); ";
const setterCalls = [
  "setUTCHours(1)", "setUTCHours(1, 2)", "setUTCHours(1, 2, 3)", "setUTCHours(1, 2, 3, 4)", "setUTCHours(25)", "setUTCHours(-1)", "setUTCHours(NaN)",
  "setUTCHours(1, NaN)", "setUTCHours(1, 2, 3, 4, 5)", "setUTCHours()", "setUTCHours(undefined)", "setUTCHours(1.9, 2.9, 3.9, 4.9)", "setUTCHours(24, 60, 60, 1000)",
  "setUTCMinutes(5)", "setUTCMinutes(5, 6)", "setUTCMinutes(5, 6, 7)", "setUTCMinutes(61)", "setUTCMinutes(-1)", "setUTCMinutes()", "setUTCMinutes(5, NaN)",
  "setUTCSeconds(5)", "setUTCSeconds(5, 6)", "setUTCSeconds(61)", "setUTCSeconds(-1)", "setUTCSeconds()", "setUTCSeconds(Infinity)",
  "setUTCMilliseconds(5)", "setUTCMilliseconds(1500)", "setUTCMilliseconds(-1)", "setUTCMilliseconds()", "setUTCMilliseconds(5.9)", "setUTCMilliseconds(-0.5)",
  "setUTCDate(1)", "setUTCDate(0)", "setUTCDate(-1)", "setUTCDate(31)", "setUTCDate(32)", "setUTCDate(366)", "setUTCDate(1e9)", "setUTCDate()", "setUTCDate(NaN)",
  "setUTCMonth(0)", "setUTCMonth(11)", "setUTCMonth(12)", "setUTCMonth(13)", "setUTCMonth(-1)", "setUTCMonth(-13)", "setUTCMonth(1, 30)", "setUTCMonth(1, 29)",
  "setUTCMonth(0, 0)", "setUTCMonth(100)", "setUTCMonth(-100)", "setUTCMonth()", "setUTCMonth(NaN)", "setUTCMonth(1e9)", "setUTCMonth(1, 1, 1)",
  "setUTCFullYear(2021)", "setUTCFullYear(2021, 1)", "setUTCFullYear(2021, 1, 29)", "setUTCFullYear(2019, 1, 29)", "setUTCFullYear(0)", "setUTCFullYear(-1)",
  "setUTCFullYear(275760)", "setUTCFullYear(275760, 8, 13)", "setUTCFullYear(275760, 8, 14)", "setUTCFullYear(-271821, 3, 20)", "setUTCFullYear(-271821, 3, 19)",
  "setUTCFullYear(NaN)", "setUTCFullYear()", "setUTCFullYear(2020, NaN)", "setUTCFullYear(2020.9, 1.9, 1.9)", "setUTCFullYear(1e9)", "setUTCFullYear(99)",
  "setTime(0)", "setTime(-0)", "setTime(MAX)", "setTime(-MAX)", "setTime(MAX + 1)", "setTime(-MAX - 1)", "setTime(NaN)", "setTime()", "setTime('5')", "setTime(1.9)", "setTime(-1.9)",
  "setTime(Infinity)", "setTime(null)", "setTime(undefined)", "setTime({ valueOf() { return 7 } })", "setTime(MAX + 0.9)", "setTime(new Date(33))", "setTime(5, 6)", "setTime(true)",
].map(c => c.replace(/MAX/g, String(MAX)));
for (const call of setterCalls) {
  T(`${PRE}${base}var r = d.${call}; R = S(r) + '|' + S(d.getTime()) + '|' + (isNaN(d) ? 'invalid' : d.toISOString())`);
}
// Os mesmos setters sobre data inválida: só setFullYear/setUTCFullYear revivem, via t = +0.
for (const call of ["setUTCHours(1)", "setUTCMinutes(5)", "setUTCSeconds(5)", "setUTCMilliseconds(5)", "setUTCDate(1)", "setUTCMonth(1)", "setUTCFullYear(2021)",
  "setUTCFullYear(2021, 1, 1)", "setUTCFullYear(2021, 12)", "setUTCFullYear(NaN)", "setUTCFullYear(2021, NaN)", "setFullYear(2021)", "setFullYear(2021, 1, 1)", "setFullYear(2021, 5, 15)",
  "setFullYear(NaN)", "setMonth(1)", "setDate(1)", "setHours(1)", "setMinutes(1)", "setSeconds(1)", "setMilliseconds(1)", "setTime(5)", "setTime(NaN)", "setYear(2021)", "setYear(21)", "setYear(NaN)"]) {
  T(`${PRE}var d = new Date(NaN); var r = d.${call}; R = S(r) + '|' + (isNaN(d) ? 'invalid' : N(d))`);
}
// Setters locais, com leitura pelos getters locais (campos), independente de fuso.
const localCalls = [
  "setHours(1)", "setHours(1, 2)", "setHours(1, 2, 3)", "setHours(1, 2, 3, 4)", "setHours(10, 20, 30, 400)", "setHours(25)", "setHours(-1)", "setHours(NaN)", "setHours()",
  "setHours(5, 5, 5, 5, 5)", "setHours(11, 59, 59, 999)", "setHours(12, 61, 61, 1001)", "setHours(10, -1)", "setHours(10, 0, -1)", "setHours(10, 0, 0, -1)",
  "setMinutes(5)", "setMinutes(5, 6)", "setMinutes(5, 6, 7)", "setMinutes(90)", "setMinutes(-30)", "setMinutes()",
  "setSeconds(5)", "setSeconds(5, 6)", "setSeconds(90)", "setSeconds(-30)", "setSeconds()",
  "setMilliseconds(5)", "setMilliseconds(2500)", "setMilliseconds(-1)", "setMilliseconds()",
  "setDate(1)", "setDate(0)", "setDate(-1)", "setDate(31)", "setDate(32)", "setDate(45)", "setDate(-45)", "setDate()", "setDate(1e5)",
  "setMonth(0)", "setMonth(11)", "setMonth(12)", "setMonth(13)", "setMonth(-1)", "setMonth(-12)", "setMonth(1, 30)", "setMonth(1, 29)", "setMonth(2, 0)", "setMonth(24)", "setMonth(-25)",
  "setMonth(0, 31)", "setMonth(3, 31)", "setMonth(5, 31)", "setMonth(1e5)", "setMonth()", "setMonth(NaN)",
  "setFullYear(2021)", "setFullYear(2021, 1)", "setFullYear(2021, 1, 29)", "setFullYear(2019, 1, 29)", "setFullYear(2024, 1, 29)", "setFullYear(0)", "setFullYear(-1)",
  "setFullYear(10000)", "setFullYear(99)", "setFullYear(275760)", "setFullYear(-271821)", "setFullYear(NaN)", "setFullYear()", "setFullYear(2020, 11, 31)", "setFullYear(2020, 12, 1)",
  "setFullYear(2020, -1, 1)", "setFullYear(2020.5)",
];
for (const call of localCalls) {
  T(`${PRE}${baseLocal}var r = d.${call}; R = S(isNaN(r) ? r : N(new Date(r))) + '|' + (isNaN(d) ? 'invalid' : [d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()].join())`);
}
// setYear/getYear.
for (const call of ["setYear(0)", "setYear(21)", "setYear(99)", "setYear(100)", "setYear(1999)", "setYear(2021)", "setYear(-1)", "setYear(NaN)", "setYear(50.9)", "setYear()", "setYear(275760)"]) {
  T(`${PRE}${baseLocal}var r = d.${call}; R = S(isNaN(d) ? 'invalid' : [d.getFullYear(), d.getMonth(), d.getDate(), d.getHours()].join()) + '|' + S(isNaN(r) ? r : 'num')`);
}
for (const y of [0, 1, 99, 100, 1899, 1900, 1999, 2000, 2020, 10000, -1, -1900]) E(`new Date(${y}, 5, 15, 12).getYear()`);
// Ordem de avaliação e coerção nos setters.
T(`${PRE}${base}var log = []; d.setUTCHours({ valueOf() { log.push('h'); return 1 } }, { valueOf() { log.push('m'); return 2 } }, { valueOf() { log.push('s'); return 3 } }, { valueOf() { log.push('ms'); return 4 } }); R = S(log.join())`);
T(`${PRE}var d = new Date(NaN); var log = []; d.setUTCHours({ valueOf() { log.push('h'); return 1 } }, { valueOf() { log.push('m'); return 2 } }); R = S(log.join())`);
T(`${PRE}var d = new Date(NaN); var log = []; d.setUTCFullYear({ valueOf() { log.push('y'); return 2020 } }, { valueOf() { log.push('m'); return 2 } }); R = S(log.join() + '|' + d.toISOString())`);
T(`${PRE}${base}var log = []; d.setUTCMonth({ valueOf() { log.push('mo'); d.setTime(0); return 1 } }); R = S(d.toISOString())`);
T(`${PRE}${base}d.setUTCDate({ valueOf() { d.setTime(NaN); return 1 } }); R = S(String(d.getTime()))`);
T(`${PRE}${base}d.setUTCHours({ valueOf() { d.setTime(NaN); return 1 } }); R = S(String(d.getTime()))`);
T(`${PRE}var d = new Date(NaN); d.setUTCFullYear({ valueOf() { d.setTime(0); return 2000 } }); R = S(d.toISOString())`);
T(`${PRE}${base}d.setTime({ valueOf() { throw new RangeError('boom') } })`);
T(`${PRE}${base}d.setUTCHours(1, { valueOf() { throw new RangeError('boom') } })`);
T(`${PRE}${base}d.setUTCHours(Symbol())`);
T(`${PRE}${base}d.setUTCHours(1n)`);
T(`${PRE}${base}R = S(d.setUTCHours('2') + ',' + d.setUTCHours(null) + ',' + d.setUTCHours(true))`);
T(`${PRE}var d = new Date(0); R = S(d.setUTCMonth(1) === d.getTime()) + S(d.setTime(5) === 5) + S(d.setUTCDate() )`);

// ---- getTimezoneOffset e relações locais/UTC (invariantes sem imprimir fuso).
for (const t of [0, 951782400000, 1.7e12, 1.72e12, 4e12, -1e12, 8.64e15, -8.64e15]) {
  E(`Number.isInteger(new Date(${t}).getTimezoneOffset() * 60) `);
  E(`Math.abs(new Date(${t}).getTimezoneOffset()) <= 14 * 60 + 30 `.replace("14 * 60 + 30", "24 * 60"));
  E(`typeof new Date(${t}).getTimezoneOffset()`);
  E(`new Date(${t}).getTime() === new Date(${t}).valueOf()`);
  E(`new Date(${t}).getHours() === new Date(new Date(${t}).getTime() - new Date(${t}).getTimezoneOffset() * 60000).getUTCHours() || 'dst-gap-or-lmt'`);
}
E(`new Date(NaN).getTimezoneOffset()`);
E(`Date.prototype.getTimezoneOffset.length`);
E(`N(new Date(2020, 5, 15, 12)) === Date.UTC(2020, 5, 15, 12)`);
E(`N(new Date(2020, 5, 15)) === Date.UTC(2020, 5, 15)`);
E(`N(new Date(2020, 0, 20)) === Date.UTC(2020, 0, 20)`);
E(`N(new Date(1970, 5, 15)) === Date.UTC(1970, 5, 15)`);
E(`N(new Date(1999, 11, 31, 12)) === Date.UTC(1999, 11, 31, 12)`);
E(`Math.round(N(new Date(2020, 5, 15, 12, 0, 0, 500)) - Date.UTC(2020, 5, 15, 12))`);

// ---- Symbol.toPrimitive e hints.
const prim = Symbol.toPrimitive;
for (const hint of ["'default'", "'string'", "'number'", "'Default'", "'STRING'", "''", "'x'", "'numbers'", "undefined", "null", "1", "{}", "Symbol()", "'string '", "[]", "true", "new String('number')"]) {
  T(`${PRE}var v = new Date(5)[Symbol.toPrimitive](${hint}); R = S(typeof v) + S(typeof v === 'string' ? v.replace(/ GMT.*$/, '').replace(/^[A-Z][a-z]{2} [A-Z][a-z]{2} \\d\\d \\d{4}.*$/, 'string-shape') : v)`);
}
T(`${PRE}R = S(new Date(5)[Symbol.toPrimitive]())`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString() { return 's' }, valueOf() { return 7 } }, 'string'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString() { return 's' }, valueOf() { return 7 } }, 'number'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString() { return 's' }, valueOf() { return 7 } }, 'default'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString() { return {} }, valueOf() { return 7 } }, 'string'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString() { return {} }, valueOf() { return {} } }, 'string'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString: 1, valueOf() { return 7 } }, 'string'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call({ toString: 1, valueOf: 2 }, 'number'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call(5, 'number'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call('x', 'string'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call(undefined, 'number'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call(null, 'number'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call(Symbol(), 'number'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call(Symbol(), 'string'))`);
T(`${PRE}R = S(Date.prototype[Symbol.toPrimitive].call(true, 'default'))`);
T(`${PRE}var d = new Date(5); d.valueOf = () => 11; d.toString = () => 'ts'; R = S(d + 1) + S(d - 1) + S(\`\${d}\`) + S(+d) + S(d * 2) + S(String(d)) + S(d == 'ts') + S(d == 11) + S(d < 12)`);
T(`${PRE}var d = new Date(5); d[Symbol.toPrimitive] = undefined; d.valueOf = () => 11; d.toString = () => 'ts'; R = S(d + 1) + S(d - 1) + S(\`\${d}\`)`);
T(`${PRE}var d = new Date(5); d[Symbol.toPrimitive] = () => 42; R = S(d + 1) + S(d - 1) + S(\`\${d}\`) + S(d == 42) + S(+d)`);
T(`${PRE}var d = new Date(5); d[Symbol.toPrimitive] = () => ({}); R = S(d + 1)`);
T(`${PRE}var d = new Date(5); d[Symbol.toPrimitive] = 5; R = S(d + 1)`);
T(`${PRE}var d = new Date(5); d[Symbol.toPrimitive] = null; d.valueOf = () => 11; d.toString = () => 'ts'; R = S(d + 1)`);
T(`${PRE}var p = Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive); R = S(p.writable) + S(p.enumerable) + S(p.configurable)`);
T(`${PRE}var p = Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive); R = S(String(p.value.name))`);
T(`${PRE}var hints = []; var o = { [Symbol.toPrimitive](h) { hints.push(h); return 1 } }; new Date(o); o + 1; +o; \`\${o}\`; o == 1; o < 2; new Date(2020, o); Date.UTC(o); R = S(hints.join())`);
T(`${PRE}R = S(new Date(5) + 1 === new Date(5).toString() + '1')`.replace("S(new Date(5) + 1 === new Date(5).toString() + '1')", "S(typeof (new Date(5) + 1)) + S((new Date(5) + 1).endsWith('1'))"));
T(`${PRE}R = S(typeof (new Date(5) - 1)) + S(new Date(5) - 1) + S(new Date(5) * 2) + S(+new Date(5)) + S(-new Date(5)) + S(new Date(5) / 5)`);
T(`${PRE}R = S(new Date(10) - new Date(4)) + S(new Date(4) - new Date(10)) + S(new Date(NaN) - new Date(4)) + S(new Date(4) - new Date(4))`);
T(`${PRE}R = S(new Date(10) > new Date(4)) + S(new Date(10) < new Date(4)) + S(new Date(4) >= new Date(4)) + S(new Date(4) == new Date(4)) + S(new Date(4) <= new Date(4)) + S(new Date(NaN) < new Date(4))`);
T(`${PRE}var d = new Date(4); R = S(d == d) + S(d === d) + S(d == 4) + S(d == '4') + S(d != 4) + S(+d === 4)`);
T(`${PRE}R = S(typeof (new Date(0) + new Date(0))) + S((new Date(0) + new Date(0)).length > 0)`);
T(`${PRE}R = S(new Date(0) + 0 === new Date(0).toString() + '0')`);
T(`${PRE}R = S(new Date(0) + '' === new Date(0).toString()) + S(\`\${new Date(0)}\` === new Date(0).toString()) + S(String(new Date(0)) === new Date(0).toString()) + S([new Date(0)] + '' === new Date(0).toString())`);
T(`${PRE}R = S(new Date(NaN) + '') + S(String(new Date(NaN))) + S(\`\${new Date(NaN)}\`) + S(new Date(NaN) + 1) + S(new Date(NaN) - 1)`);
T(`${PRE}R = S(Number(new Date(7))) + S(Number(new Date(NaN))) + S(Math.max(new Date(7), new Date(9))) + S(Math.min(new Date(7), 3)) + S(isNaN(new Date(NaN))) + S(isFinite(new Date(7)))`);
T(`${PRE}R = S(JSON.stringify(new Date(7)) ) + S(JSON.stringify({ a: new Date(7) }))`);
T(`${PRE}R = S(new Date(7).toJSON()) + S(new Date(MAX).toJSON()) + S(new Date(NaN).toJSON())`.replace("MAX", String(MAX)));
T(`${PRE}R = S(Object(new Date(7)) == 7) + S(new Date(7) == '7') + S(new Date(0) == new Date(0).toString())`);
T(`${PRE}R = S(new Date(2).valueOf() + new Date(3).valueOf()) + S(+new Date(2) + +new Date(3)) + S(new Date(2) | 0) + S(new Date(2.7) | 0) + S(~~new Date(1e12))`);
T(`${PRE}var d1 = new Date(1e12); var d2 = new Date(1e12 + 86400000 * 3); R = S((d2 - d1) / 86400000) + S(d2 > d1) + S(Math.round((d2 - d1) / 36e5))`);
T(`${PRE}var a = new Date(Date.UTC(2020, 0, 31)); a.setUTCMonth(a.getUTCMonth() + 1); R = S(a.toISOString())`);
T(`${PRE}var a = new Date(Date.UTC(2020, 0, 31)); a.setUTCDate(a.getUTCDate() + 30); R = S(a.toISOString())`);
T(`${PRE}var a = new Date(Date.UTC(2020, 1, 29)); a.setUTCFullYear(a.getUTCFullYear() + 1); R = S(a.toISOString())`);
T(`${PRE}var a = new Date(Date.UTC(2020, 11, 31, 23, 59, 59, 999)); a.setUTCMilliseconds(a.getUTCMilliseconds() + 1); R = S(a.toISOString())`);
T(`${PRE}var a = new Date(0); a.setUTCDate(a.getUTCDate() - 1); R = S(a.toISOString()) + S(a.getUTCDay())`);
T(`${PRE}var a = new Date(MAX); a.setUTCMilliseconds(1); R = S(a.getTime())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(MAX); a.setUTCMilliseconds(a.getUTCMilliseconds() + 1); R = S(a.getTime())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(MAX); a.setUTCDate(a.getUTCDate() - 1); R = S(a.toISOString())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(-MAX); a.setUTCDate(a.getUTCDate() - 1); R = S(a.getTime())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(-MAX); a.setUTCHours(0); R = S(a.toISOString())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(-MAX); a.setUTCHours(-1); R = S(a.getTime())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(-MAX); a.setUTCFullYear(-271821, 3, 20); R = S(a.getTime())`.replace("MAX", String(MAX)));
T(`${PRE}var a = new Date(0); a.setUTCFullYear(-271821, 3, 19); R = S(a.getTime())`);
T(`${PRE}var a = new Date(0); a.setUTCFullYear(275760, 8, 13); R = S(a.getTime())`);
T(`${PRE}var a = new Date(0); a.setUTCFullYear(275760, 8, 14); R = S(a.getTime())`);

// ---- Gerador de combinações pequeno: setters UTC encadeados e difs.
const chains = [
  "setUTCFullYear(2021, 0, 31); d.setUTCMonth(1)", "setUTCMonth(1, 31)", "setUTCDate(0); d.setUTCDate(0)", "setUTCHours(24); d.setUTCMinutes(60)",
  "setUTCMilliseconds(-1); d.setUTCSeconds(-1)", "setUTCMonth(-1); d.setUTCMonth(12)", "setUTCFullYear(0); d.setUTCFullYear(-1)", "setTime(NaN); d.setUTCFullYear(1970)",
  "setTime(NaN); d.setUTCMonth(0)", "setUTCDate(1); d.setUTCMonth(11, 31); d.setUTCDate(32)",
];
for (const c of chains) T(`${PRE}${base}d.${c}; R = S(isNaN(d) ? 'invalid' : d.toISOString())`);
for (const [a, b] of [[0, 1], [0, -1], [MAX, -MAX], [MAX, MAX], [NaN, 1], [1, NaN], [1e12, 1.7e12], [-1e12, 1e12]]) {
  T(`${PRE}R = S(new Date(${a}) - new Date(${b})) + S(new Date(${a}).getTime() + new Date(${b}).getTime()) + S(Math.abs(new Date(${a}) - new Date(${b})))`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "date-core-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (o transpilador do bun muda a semântica de script).
const source_file = path.join(dir, "date_source.js");
const file = path.join(dir, "date_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
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
let unstable = 0;
const ZONES = ["UTC", "America/Sao_Paulo", "Asia/Kolkata", "Pacific/Auckland", "America/New_York"];
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  // Cada programa roda em vários fusos; só fica o que dá o mesmo resultado em todos (o golden não pode depender do
  // fuso da máquina que roda o teste).
  const results = ZONES.map(zone => {
    const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: zone } });
    const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
    return marked ? JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("") : null;
  });
  const marked = results[0] !== null;
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  if (results.some(r => r !== results[0])) {
    dropped++;
    unstable++;
    continue;
  }
  const result = results[0];
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped} (dependentes de fuso: ${unstable})\n`);
fs.rmSync(dir, { recursive: true, force: true });
