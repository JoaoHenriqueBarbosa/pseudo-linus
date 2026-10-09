// Gera tests/golden/date_utc_bun.tsv: Date com fuso fixo UTC (o bun roda com TZ=UTC; o programa não lê TZ), medido no
// bun 1.4.2. Cobre Date.parse de formatos não ISO (RFC 2822, "Mon Jan 01 2024", "1/2/2024 10:00 PM", GMT+0100, anos
// de dois dígitos, strings inválidas), toString/toUTCString/toISOString/toLocale*String, setters com vários
// argumentos e NaN, Date.UTC com 1 a 7 argumentos, @@toPrimitive, limites de ±8.64e15, getYear/setYear/toGMTString e
// subclasses de Date. Complementa date_core, date_legacy_parse, date_edge e datetime_edge com combinações novas.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-date-core-golden.js.
// Uso: bun scripts/gen-date-utc-golden.js > tests/golden/date_utc_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const T = body => add(`try { ${body} } catch (e) { R = e.name + ': ' + e.message }`);
const PRE = "var S = v => Object.is(v, -0) ? '-0' : typeof v === 'string' ? JSON.stringify(v) : String(v); ";
const E = expr => T(`${PRE}R = S(${expr})`);
const J = JSON.stringify;
const MAX = 8.64e15;
const lit = v => (Object.is(v, -0) ? "-0" : v === undefined ? "undefined" : typeof v === "number" ? String(v) : J(v));

// ---- Date.parse com formatos não ISO.
const parseStrings = [];
const days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun", "mon", "MON", "Monday", "Xyz"];
for (const d of days) parseStrings.push(`${d}, 01 Jan 2024 10:20:30 GMT`, `${d} Jan 01 2024`);
const rfc = [
  "Mon, 01 Jan 2024 10:20:30 GMT", "Mon, 01 Jan 2024 10:20:30 UT", "Mon, 01 Jan 2024 10:20:30 Z", "Mon, 01 Jan 2024 10:20:30 +0100",
  "Mon, 01 Jan 2024 10:20:30 -0530", "Mon, 01 Jan 2024 10:20:30 +01:00", "Mon, 01 Jan 2024 10:20 GMT", "Mon, 01 Jan 2024 10:20:30.123 GMT",
  "Mon, 1 Jan 2024 10:20:30 GMT", "Mon, 01 Jan 24 10:20:30 GMT", "Mon, 01 Jan 2024 10:20:30 EST", "Mon, 01 Jan 2024 10:20:30 EDT",
  "Mon, 01 Jan 2024 10:20:30 CST", "Mon, 01 Jan 2024 10:20:30 CDT", "Mon, 01 Jan 2024 10:20:30 MST", "Mon, 01 Jan 2024 10:20:30 MDT",
  "Mon, 01 Jan 2024 10:20:30 PST", "Mon, 01 Jan 2024 10:20:30 PDT", "Mon, 01 Jan 2024 10:20:30 BRT", "Mon, 01 Jan 2024 10:20:30 CET",
  "01 Jan 2024 10:20:30 GMT", "01 Jan 2024", "1 Jan 2024", "01 January 2024", "01-Jan-2024", "01-Jan-2024 10:20:30", "1-jan-2024",
  "Mon Jan 01 2024", "Mon Jan 01 2024 10:20:30", "Mon Jan 01 2024 10:20:30 GMT+0000", "Mon Jan 01 2024 10:20:30 GMT+0000 (Coordinated Universal Time)",
  "Mon Jan 01 2024 10:20:30 GMT+0100", "Mon Jan 01 2024 10:20:30 GMT-0300 (Brasilia Standard Time)", "Mon Jan 01 2024 10:20:30 GMT+1", "Mon Jan 01 2024 10:20:30 GMT+01",
  "Mon Jan 01 2024 10:20:30 GMT+01:00", "Mon Jan 01 2024 10:20:30 GMT+0530", "Mon Jan 01 2024 10:20:30 GMT+1400", "Mon Jan 01 2024 10:20:30 GMT+2400",
  "Mon Jan 01 2024 10:20:30 GMT+9999", "Mon Jan 01 2024 10:20:30 GMT-0000", "Mon Jan 01 2024 10:20:30 UTC+0100", "Mon Jan 01 2024 10:20:30 UTC",
  "Mon Jan 01 2024 10:20:30 GMT +0100", "Mon Jan 01 2024 10:20:30 GMT+0100 ()", "Mon Jan 01 2024 10:20:30 GMT+0100 (", "Mon Jan 01 2024 10:20:30 GMT+0100 (a (b) c)",
  "Mon Jan 01 2024 10:20:30 GMT+0100 junk", "Jan 1 2024", "Jan 1, 2024", "January 1, 2024", "January 1, 2024 10:20", "January 1, 2024 10:20:30 PM",
  "Jan 1 2024 12:00 AM", "Jan 1 2024 12:00 PM", "Jan 1 2024 12:30 AM", "Jan 1 2024 12:30 PM", "Jan 1 2024 1:00 AM", "Jan 1 2024 1:00 PM",
  "Jan 1 2024 13:00 PM", "Jan 1 2024 0:00 AM", "Jan 1 2024 0:00 PM", "Jan 1 2024 11:59:59 PM", "Jan 1 2024 10:00 pm", "Jan 1 2024 10:00pm", "Jan 1 2024 10:00 P.M.",
  "Jan 1 2024 10 PM", "1 Jan 2024 10:00 AM GMT", "1/2/2024", "1/2/2024 10:00", "1/2/2024 10:00 PM", "1/2/2024 10:00:30 PM", "1/2/2024 12:00 AM", "1/2/2024 12:00 PM",
  "01/02/2024", "1/2/24", "1/2/49", "1/2/50", "1/2/99", "1/2/00", "1/2/0", "1/2/1", "1/2/100", "1/2/999", "1/2/1000", "13/2/2024", "12/31/2024", "2/29/2024",
  "2/29/2023", "2/30/2024", "0/1/2024", "1/0/2024", "1/32/2024", "1/31/2024", "4/31/2024", "1/2", "1/2/", "1//2024", "/1/2024", "1/2/2024/5",
  "2024/1/2", "2024/01/02", "2024/1/2 10:00", "2024/1/2 10:00 PM", "2024/13/2", "2024/1/32", "2024-1-2", "2024-1-2 10:00", "2024-01-02 10:00", "2024-01-02 10:00:30",
  "2024-01-02 10:00:30Z", "2024-01-02 10:00:30 GMT", "2024-01-02 10:00:30 +0100", "2024-01-02 10:00:30+01:00", "2024-01-02T10:00:30", "2024-01-02T10:00:30 GMT",
  "2024-01-02T10:00:30 +0100", "2024-01-02T10:00:30+0100", "2024-01-02T10:00:30.123456", "2024-01-02T10:00:30,123Z", "2024.01.02", "2.1.2024", "02.01.2024",
  "Jan 1 70", "Jan 1 69", "Jan 1 49", "Jan 1 50", "Jan 1 99", "Jan 1 00", "Jan 1 0", "Jan 1 100", "Jan 1 1", "Jan 1 -1", "Jan 1 -100", "Jan 1 275760", "Jan 1 275761",
  "Jan 1 10000", "Jan 1 99999", "Jan 1 0000", "Jan 1 0001", "Jan 1 12345 10:00", "1 Jan 70", "1 Jan 49", "1 Jan 50", "1 Jan 00", "31 Dec 99", "31 Dec 1969", "31 Dec 69",
  "January", "Jan", "2024", "24", "1 2024", "Jan 2024", "January 2024", "Jan 1", "1 Jan", "10:20", "10:20:30", "10:20 PM", "Jan 1 10:20", "Jan 1 2024 10",
  "Jan 1 2024 10:", "Jan 1 2024 :20", "Jan 1 2024 10:20:", "Jan 1 2024 10:20:30:40", "Jan 1 2024 24:00", "Jan 1 2024 24:00:00", "Jan 1 2024 24:00:01",
  "Jan 1 2024 25:00", "Jan 1 2024 10:60", "Jan 1 2024 10:20:60", "Jan 1 2024 10:20:61", "Jan 1 2024 10:20:30.5", "Jan 1 2024 10:20:30.999",
  "Jan 1 2024 10:20:30.9999", "(comment) Jan 1 2024", "Jan (comment) 1 2024", "Jan 1 2024 (comment)", "Jan 1 2024 (unclosed", "Jan 1 2024 )", "  Jan   1   2024  ",
  "\tJan 1 2024", "Jan\t1\t2024", "Jan,1,2024", "Jan, 1, 2024", "Jan-1-2024", "Jan/1/2024", "jan 1 2024", "JAN 1 2024", "JaN 1 2024", "Janu 1 2024", "Janua 1 2024",
  "Januar 1 2024", "Janxxx 1 2024", "Sept 1 2024", "Sep 1 2024", "September 1 2024", "Febr 1 2024", "Marc 1 2024", "Mayo 1 2024", "Juno 1 2024", "Sat Jan 1 2000 foo",
  "Jan 1 2024 AM", "Jan 1 2024 PM", "Jan 1 2024 Z", "Jan 1 2024 z", "Jan 1 2024 GMT", "Jan 1 2024 gmt", "Jan 1 2024 UT", "Jan 1 2024 UTC", "Jan 1 2024 utc", "Jan 1 2024 +0100",
  "Jan 1 2024 -0100", "Jan 1 2024 +01:00", "Jan 1 2024 +1", "Jan 1 2024 +01", "Jan 1 2024 +100", "Jan 1 2024 +10000", "Jan 1 2024 GMT+5", "Jan 1 2024 GMT-5", "Jan 1 2024 GMT+05",
  "Jan 1 2024 GMT+0500", "Jan 1 2024 GMT+05:00", "Jan 1 2024 GMT+5:00", "Jan 1 2024 GMT+05:30", "Jan 1 2024 GMT+5:30", "Jan 1 2024 GMT+99", "Jan 1 2024 GMT+2359", "Jan 1 2024 GMT+2360",
  "Jan 1 2024 10:20 GMT+0100", "Jan 1 2024 10:20 GMT-0100", "Jan 1 2024 10:20 GMT+0100 (CET)", "Jan 1 2024 10:20 EST", "Jan 1 2024 10:20 EDT", "Jan 1 2024 10:20 CST", "Jan 1 2024 10:20 PST",
  "Jan 1 2024 10:20 PDT", "Jan 1 2024 10:20 XYZ", "Jan 1 2024 10:20 A", "Jan 1 2024 10:20 est", "Jan 1 2024 10:20 Pst", "Jan 1 2024 10:20 MST", "Jan 1 2024 10:20 MDT",
  "-1 Jan 2024", "+1 Jan 2024", "Jan 01 +2024", "Jan 01 -2024", "Jan 01 -0001", "Jan 01 0000", "1 1 2024", "1 1 24", "11 11 11", "1 2 3", "00 00 00", "99 99 99",
  "Thu, 01 Jan 1970 00:00:00 GMT", "Thu, 01 Jan 1970 00:00:00 GMT+0100", "Thu, 01 Jan 1970 00:00:00 +0000", "Wed, 31 Dec 1969 23:59:59 GMT", "Tue, 19 Jan 2038 03:14:07 GMT",
  "Tue, 19 Jan 2038 03:14:08 GMT", "Sat, 13 Sep 275760 00:00:00 GMT", "Sat, 13 Sep 275760 00:00:01 GMT", "Tue, 20 Apr -271821 00:00:00 GMT", "Mon, 29 Feb 2024 00:00:00 GMT",
  "Tue, 29 Feb 2023 00:00:00 GMT", "Sun, 31 Dec 2023 23:59:60 GMT", "Fri, 31 Dec 9999 23:59:59 GMT", "Sat, 01 Jan 10000 00:00:00 GMT", "Sun, 01 Jan 0000 00:00:00 GMT",
  "Fri, 01 Jan 0099 00:00:00 GMT", "Mon, 01 Jan 0100 00:00:00 GMT",
  "", " ", "   ", "x", "Invalid Date", "NaN", "null", "undefined", "Infinity", "0", "-0", "1", "12", "123", "1234", "12345", "123456", "1e3", "0x10", "1,2,2024",
  "2024-01-02T10:00:30Z", "2024-01-02", "2024-01", "2024", "+002024-01-02", "-000001-01-01", "2024-01-02T10:00Z", "20240102", "20240102T100030Z",
];
for (const s of parseStrings) {
  E(`Date.parse(${J(s)})`);
  E(`new Date(${J(s)}).getTime()`);
}
// Tipos não string em Date.parse e no construtor.
for (const v of ["undefined", "null", "true", "false", "0", "-0", "NaN", "1e3", "[]", "{}", "[2024]", "['Jan 1 2024']", "({ toString() { return 'Jan 1 2024' } })",
  "({ toString() { return 'Jan 1 2024' }, valueOf() { return 5 } })", "({ valueOf() { return 5 } })", "new String('Jan 1 2024')", "new Number(5)", "Symbol.iterator"]) {
  E(`Date.parse(${v})`);
  E(`new Date(${v}).getTime()`);
}
E(`Date.parse()`);
E(`Date.parse.length + Date.parse.name`);
// Ida e volta: parse(toString), parse(toUTCString), parse(toISOString) para vários instantes.
const instants = [0, 1, -1, 86399999, 951782400000, 1704067200000, 1709164800000, 4102444799999, -62135596800000, -62167219200000, -62198755200000, 253402300799999, 253402300800000,
  -1e12, 1.7e12, MAX, -MAX, 123456789012, -123456789012, 946684799999, 951868800000, 1e15, -1e15];
for (const t of instants) {
  E(`Date.parse(new Date(${t}).toString()) === Math.trunc(${t} / 1000) * 1000`);
  E(`Date.parse(new Date(${t}).toUTCString()) === Math.floor(${t} / 1000) * 1000`);
  E(`Date.parse(new Date(${t}).toISOString()) === ${t}`);
  E(`Date.parse(new Date(${t}).toDateString())`);
  E(`new Date(new Date(${t}).toString()).getTime()`);
  E(`new Date(new Date(${t}).toUTCString()).getTime()`);
}

// ---- Serialização em UTC: toString, toUTCString, toISOString, toDateString, toTimeString, toGMTString, toJSON.
const serial = [0, 1, -1, 999, 1000, 59999, 3599999, 86399999, 86400000, 951782400000, 951868800000, 1709164799999, 1709164800000, 1704067200000, 1735689599999,
  -62198755200000, -62167219200000, -62167219200001, -62135596800000, -2208988800000, -1e12, 1.7e12, 253402300799999, 253402300800000, 4102444800000, 32503680000000,
  MAX, -MAX, MAX - 1, -MAX + 1, -377705116800000, -377705116800001, -30610224000000, -30610224000001, -61851600000000, 1e14, -1e14, 1e13, 5e14, NaN, 123456789012];
for (const t of serial) {
  for (const m of ["toString", "toUTCString", "toGMTString", "toISOString", "toDateString", "toTimeString", "toJSON", "toLocaleString", "toLocaleDateString", "toLocaleTimeString"]) {
    E(`new Date(${t}).${m}()`);
  }
  E(`String(new Date(${t}))`);
  E(`new Date(${t}) + ''`);
  E(`\`\${new Date(${t})}\``);
  E(`JSON.stringify(new Date(${t}))`);
  E(`JSON.stringify({ d: new Date(${t}) })`);
  E(`new Date(${t}).getTimezoneOffset()`);
  E(`Object.prototype.toString.call(new Date(${t}))`);
}
// toLocale*String com locales e opções.
const locales = ["undefined", "'en-US'", "'en-GB'", "'pt-BR'", "'de-DE'", "'fr-FR'", "'ja-JP'", "'sv-SE'", "'en-CA'", "'xx'", "'en-US-u-hc-h23'", "['pt-BR', 'en']", "'EN-us'", "'es-ES'", "'it-IT'", "'ko-KR'", "'zh-CN'", "'ar'", "'ru-RU'"];
const optSets = ["undefined", "{}", "{ timeZone: 'UTC' }", "{ hour12: false }", "{ hour12: true }", "{ dateStyle: 'short' }", "{ dateStyle: 'medium' }", "{ dateStyle: 'long' }", "{ dateStyle: 'full' }",
  "{ timeStyle: 'short' }", "{ timeStyle: 'medium', timeZone: 'UTC' }", "{ year: 'numeric', month: 'long', day: 'numeric' }", "{ weekday: 'long' }", "{ month: 'short' }",
  "{ year: '2-digit', month: '2-digit', day: '2-digit' }", "{ hour: '2-digit', minute: '2-digit' }", "{ timeZone: 'America/Sao_Paulo' }", "{ timeZone: 'Asia/Kolkata', timeStyle: 'long' }"];
for (const m of ["toLocaleString", "toLocaleDateString", "toLocaleTimeString"]) {
  for (const l of locales) E(`new Date(1704067200000 + 37230123)[${J(m)}](${l})`);
  for (const o of optSets) E(`new Date(1704067200000 + 37230123)[${J(m)}](undefined, ${o})`);
  for (const l of ["'en-US'", "'pt-BR'", "'de-DE'", "'ja-JP'"]) for (const o of optSets.slice(2, 12)) E(`new Date(1709164800000 + 82800000)[${J(m)}](${l}, ${o})`);
  E(`new Date(NaN)[${J(m)}]()`);
  E(`new Date(NaN)[${J(m)}]('pt-BR', { dateStyle: 'full' })`);
  E(`new Date(MAX = ${MAX})[${J(m)}]('en-US')`);
  E(`${m === "toLocaleDateString" ? "new Date(1e12).toLocaleDateString('en-US', { timeStyle: 'short' })" : m === "toLocaleTimeString" ? "new Date(1e12).toLocaleTimeString('en-US', { dateStyle: 'short' })" : "new Date(1e12).toLocaleString('en-US', { dateStyle: 'short', year: 'numeric' })"}`);
  E(`Date.prototype[${J(m)}].call({})`);
  E(`Date.prototype[${J(m)}].call(1)`);
  E(`Date.prototype[${J(m)}].length`);
  E(`new Date(0)[${J(m)}]('bad_locale')`);
  E(`new Date(0)[${J(m)}]('en-US', { timeZone: 'Nowhere/City' })`);
}

// ---- Setters com vários argumentos e NaN (instante-base em UTC).
const BASE = 1709213445678; // 2024-02-29T13:10:45.678Z
const setterArgs = {
  setMilliseconds: [[0], [999], [1000], [-1], [NaN], [Infinity], [1.9], [-1.9], [undefined], [], ["12"], [null]],
  setUTCMilliseconds: [[0], [1500], [-1500], [NaN], [], [undefined]],
  setSeconds: [[0], [59], [60], [-1], [30, 500], [30, 500, 99], [NaN], [30, NaN], [NaN, 5], [], [undefined, 5], [1e10], [30, undefined], [30.9, 500.9], [Infinity]],
  setUTCSeconds: [[0], [61], [-61], [30, 500], [30, NaN], [NaN], [1, 2, 3], []],
  setMinutes: [[0], [59], [60], [-1], [30, 15], [30, 15, 500], [30, 15, 500, 9], [NaN], [30, NaN], [30, 15, NaN], [], [undefined], [1e9], [30.5, 15.5, 500.5]],
  setUTCMinutes: [[0], [1440], [-1], [30, 15], [30, 15, 500], [NaN], [30, NaN, 1], [], ["5", "6", "7"]],
  setHours: [[0], [23], [24], [25], [-1], [12, 30], [12, 30, 15], [12, 30, 15, 500], [12, 30, 15, 500, 9], [NaN], [12, NaN], [12, 30, NaN], [12, 30, 15, NaN], [], [undefined], [1e9], [24, 60, 60, 1000], [12.9, 30.9, 15.9, 500.9]],
  setUTCHours: [[0], [48], [-25], [12, 30], [12, 30, 15], [12, 30, 15, 500], [NaN], [12, undefined], [], [null, null, null, null]],
  setDate: [[1], [0], [-1], [31], [32], [60], [366], [29], [30], [NaN], [], [undefined], [1.9], [-0.5], [1e9], [-1e9], [Infinity], ["15"], [true]],
  setUTCDate: [[1], [0], [-30], [31], [100], [NaN], [], [1e8]],
  setMonth: [[0], [1], [11], [12], [13], [-1], [-13], [1, 15], [1, 0], [1, 31], [1, 32], [1, NaN], [NaN], [NaN, 1], [], [undefined], [1.9, 1.9], [1e9], [2, 31, 99], [Infinity]],
  setUTCMonth: [[0], [12], [-1], [1, 15], [1, 0], [1, NaN], [NaN], [], [5, 31], [13, 1]],
  setFullYear: [[2024], [2023], [2000], [1900], [0], [-1], [99], [100], [275760], [275761], [-271821], [-271822], [2025, 1], [2025, 1, 29], [2025, 1, 28], [2024, 12, 32], [2024, NaN], [2024, 1, NaN], [NaN], [NaN, 1, 1], [], [undefined], [2024.9, 1.9, 1.9], [1e6], [Infinity], [2023, 1, 29, 99]],
  setUTCFullYear: [[2024], [2023, 1, 29], [0], [-1], [275760, 8, 13], [275760, 8, 14], [NaN], [2020, NaN], [], [100000]],
  setYear: [[99], [100], [0], [-1], [70], [1970], [2024], [24], [1899], [NaN], [], [undefined], [2000.9], [150], [275760 - 1900], [275761 - 1900], ["95"], ["x"], [null]],
  setTime: [[0], [MAX], [MAX + 1], [-MAX], [-MAX - 1], [NaN], [], [undefined], [1.9], [-0.9], [Infinity], ["5"], [null], [true], [1e300], [2 ** 53]],
};
for (const [method, argLists] of Object.entries(setterArgs)) {
  for (const args of argLists) {
    const a = args.map(lit).join(", ");
    E(`(function () { var d = new Date(${BASE}); var r = d.${method}(${a}); return S(r) + ' ' + S(d.getTime()) })()`);
    E(`(function () { var d = new Date(NaN); var r = d.${method}(${a}); return S(r) + ' ' + S(d.getTime()) })()`);
  }
  E(`${J(method)} in Date.prototype && Date.prototype.${method}.length`);
}
// Setters na data inválida e retornos que encadeiam.
for (const m of ["setUTCFullYear", "setFullYear"]) E(`(function () { var d = new Date(NaN); d.${m}(2024); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(NaN); d.setYear(99); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(NaN); d.setYear(2024, 5); return S(d.toISOString()) })()`);
E(`(function () { var d = new Date(${BASE}); d.setHours(1).valueOf; return 1 })()`);
E(`(function () { var d = new Date(${BASE}); return S(d.setMinutes(5) === d.getTime()) })()`);
E(`(function () { var d = new Date(${BASE}); var o = { valueOf() { d.setTime(0); return 5 } }; var r = d.setMinutes(o); return S(r) + ' ' + S(d.getTime()) })()`);
E(`(function () { var d = new Date(${BASE}); var o = { valueOf() { d.setTime(NaN); return 5 } }; var r = d.setMinutes(o); return S(r) + ' ' + S(d.getTime()) })()`);
E(`(function () { var d = new Date(${BASE}); var log = []; var mk = n => ({ valueOf() { log.push(n); return 1 } }); d.setHours(mk('h'), mk('m'), mk('s'), mk('ms')); return log.join() })()`);
E(`(function () { var d = new Date(NaN); var log = []; var mk = n => ({ valueOf() { log.push(n); return 1 } }); d.setHours(mk('h'), mk('m'), mk('s'), mk('ms')); return log.join() + ' ' + S(d.getTime()) })()`);
E(`(function () { var d = new Date(NaN); var log = []; var mk = n => ({ valueOf() { log.push(n); return 1 } }); d.setMonth(mk('mo'), mk('d')); return log.join() + ' ' + S(d.getTime()) })()`);
E(`(function () { var d = new Date(NaN); var log = []; var mk = n => ({ valueOf() { log.push(n); return 2024 } }); d.setFullYear(mk('y'), mk('mo'), mk('d')); return log.join() + ' ' + S(d.getTime()) })()`);
E(`Date.prototype.setHours.call({}, 1)`);
E(`Date.prototype.setTime.call(1, 1)`);
E(`Date.prototype.setYear.call('x', 1)`);

// ---- Getters em UTC (iguais aos locais com TZ=UTC), getYear legado.
for (const t of [0, -1, 1e12, BASE, -62198755200000, -62167219200000, 253402300800000, MAX, -MAX, NaN, 951782400000, -2208988800000, 4102444800000, 946684800000, -1e11]) {
  for (const g of ["getYear", "getFullYear", "getUTCFullYear", "getMonth", "getDate", "getDay", "getHours", "getMinutes", "getSeconds", "getMilliseconds", "getUTCDay", "getUTCHours",
    "getTimezoneOffset", "valueOf", "getTime"]) E(`new Date(${t}).${g}()`);
}
E(`Date.prototype.getYear.length`);
E(`Date.prototype.toGMTString === Date.prototype.toUTCString`);
E(`Date.prototype.toGMTString.name`);
E(`Date.prototype.setYear.name + Date.prototype.getYear.name`);
E(`Object.getOwnPropertyDescriptor(Date.prototype, 'getYear').enumerable`);
E(`Date.prototype.getYear.call({})`);
E(`Date.prototype.toGMTString.call({})`);

// ---- Date.UTC com 1 a 7 argumentos e extremos.
const utcArgs = [
  [2024], [2024, 0], [2024, 1], [2024, 1, 29], [2024, 1, 29, 13], [2024, 1, 29, 13, 10], [2024, 1, 29, 13, 10, 45], [2024, 1, 29, 13, 10, 45, 678],
  [2024, 1, 29, 13, 10, 45, 678, 999], [0], [99], [100], [-1], [1900], [1970], [69], [70], [0, 0], [99, 11, 31], [100, 0, 1], [-1, 0, 1], [275760, 8, 13], [275760, 8, 13, 0, 0, 0, 1],
  [275760, 8, 14], [-271821, 3, 20], [-271821, 3, 19, 23, 59, 59, 999], [2024, 12], [2024, -1], [2024, 11, 32], [2024, 0, 0], [2024, 0, -1], [2024, 0, 1, 24], [2024, 0, 1, 25],
  [2024, 0, 1, 0, 60], [2024, 0, 1, 0, 0, 60], [2024, 0, 1, 0, 0, 0, 1000], [2024, 0, 1, 0, 0, 0, -1], [2024.9, 1.9, 1.9, 1.9, 1.9, 1.9, 1.9], [-2024.9], [NaN], [2024, NaN],
  [2024, 0, NaN], [2024, 0, 1, NaN], [2024, 0, 1, 0, NaN], [2024, 0, 1, 0, 0, NaN], [2024, 0, 1, 0, 0, 0, NaN], [Infinity], [-Infinity], [2024, Infinity], [2024, 0, 1, 0, 0, 0, Infinity],
  [undefined], [2024, undefined], [2024, 0, undefined], [null], [2024, null], [true], [true, true, true, true, true, true, true], ["2024"], ["2024", "1", "29"], ["x"], [""],
  [2024, "x"], [{}], [[2024]], [[2024, 1]], [new Date(5)], [1e10, 0], [1e6, 0], [-1e6, 0], [2024, 1e9], [2024, -1e9], [2024, 0, 1e9], [2024, 0, 1, 1e9], [2024, 0, 1, 0, 1e9],
  [2024, 0, 1, 0, 0, 1e9], [2024, 0, 1, 0, 0, 0, 1e15], [-0], [0, -0], [2024, 0, 1, 0, 0, 0, -0], [Number.MAX_VALUE], [Number.MIN_VALUE], [2 ** 53], [2024, 0, 1, 0, 0, 0, 2 ** 53],
  [1970, 0, 1], [1970, 0, 1, 0, 0, 0, 0.9], [1969, 11, 31, 23, 59, 59, 999], [1969, 11, 31, 23, 59, 59, 999.9], [1600, 1, 29], [1900, 1, 29], [2000, 1, 29], [2100, 1, 29],
];
for (const args of utcArgs) {
  const a = args.map(lit).join(", ");
  E(`Date.UTC(${a})`);
}
E(`Date.UTC()`);
E(`Date.UTC.length + Date.UTC.name`);
E(`Date.UTC(2024, { valueOf() { return 1 } }, 29)`);
E(`Date.UTC(2024, 0, 1, 0, 0, 0, 0, 5, 6)`);
E(`Date.UTC(Symbol())`);
E(`Date.UTC(2024, 1n)`);
E(`(function () { var log = []; var mk = n => ({ valueOf() { log.push(n); return NaN } }); Date.UTC(mk('y'), mk('mo'), mk('d'), mk('h'), mk('mi'), mk('s'), mk('ms')); return log.join() })()`);
E(`new Date(Date.UTC(2024, 1, 29)).toISOString()`);
E(`new Date(Date.UTC(99, 0)).toISOString()`);
E(`new Date(Date.UTC(2024, 1, 29, 13, 10, 45, 678)).toUTCString()`);
// Mesmo com TZ=UTC, new Date(a, b, ...) coincide com Date.UTC (exceto o ano de dois dígitos, igual nos dois).
for (const args of utcArgs.slice(0, 60)) {
  if (args.some(a => typeof a === "object" || typeof a === "symbol")) continue;
  const a = args.map(lit).join(", ");
  E(`new Date(${a}).getTime() === Date.UTC(${a}) || (S(new Date(${a}).getTime()) + ' ' + S(Date.UTC(${a})))`);
}

// ---- Limites de +-8.64e15.
for (const t of [MAX, -MAX, MAX - 1, -MAX + 1, MAX + 1, -MAX - 1, MAX + 0.5, -MAX - 0.5, MAX - 0.5, MAX - 0.9, 0.9, -0.9, 1e16, -1e16, 2 ** 53, -(2 ** 53), Number.MAX_SAFE_INTEGER, 8.64e15 + 1]) {
  E(`new Date(${t}).getTime()`);
  E(`new Date(${t}).toISOString()`);
  E(`new Date(${t}).toUTCString()`);
  E(`new Date(${t}).toString()`);
  E(`new Date(${t}).getUTCFullYear() + ' ' + new Date(${t}).getUTCMonth() + ' ' + new Date(${t}).getUTCDate() + ' ' + new Date(${t}).getUTCDay()`);
  E(`(function () { var d = new Date(0); var r = d.setTime(${t}); return S(r) + ' ' + S(d.getTime()) })()`);
  E(`(function () { var d = new Date(${MAX}); var r = d.setMilliseconds(${t === MAX + 1 ? 1 : 0}); return S(r) })()`);
  E(`new Date(${t}).getYear()`);
  E(`Date.parse(new Date(${t}).toISOString())`);
}
E(`(function () { var d = new Date(${MAX}); d.setMilliseconds(1); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(${MAX}); d.setUTCDate(d.getUTCDate() + 1); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(${-MAX}); d.setUTCDate(d.getUTCDate() - 1); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(${MAX}); d.setUTCFullYear(275760, 8, 13); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(${MAX}); d.setUTCFullYear(275760, 8, 14); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(${-MAX}); d.setUTCFullYear(-271821, 3, 20); return S(d.getTime()) })()`);
E(`(function () { var d = new Date(${-MAX}); d.setUTCFullYear(-271821, 3, 19); return S(d.getTime()) })()`);
E(`new Date(${MAX}).getTime() - new Date(${-MAX}).getTime()`);
E(`new Date(8.64e15 - 1).toISOString()`);
E(`new Date('+275760-09-13T00:00:00.000Z').getTime()`);
E(`new Date('+275760-09-13T00:00:00.001Z').getTime()`);
E(`new Date('-271821-04-20T00:00:00.000Z').getTime()`);
E(`new Date('-271821-04-19T23:59:59.999Z').getTime()`);
E(`Date.parse('Sat, 13 Sep 275760 00:00:00 GMT')`);
E(`Date.parse('Sat, 13 Sep 275760 00:00:00 GMT+0100')`);
E(`Date.parse('Sat, 13 Sep 275760 00:00:00 GMT-0100')`);
E(`Date.parse('Sat, 13 Sep 275760 01:00:00 GMT+0100')`);

// ---- @@toPrimitive e conversões.
const hints = ["'default'", "'string'", "'number'", "'x'", "''", "undefined", "null", "1", "{}", "'DEFAULT'", "'String'"];
for (const h of hints) {
  E(`new Date(${BASE})[Symbol.toPrimitive](${h})`);
  E(`new Date(NaN)[Symbol.toPrimitive](${h})`);
}
E(`new Date(${BASE})[Symbol.toPrimitive]()`);
E(`Date.prototype[Symbol.toPrimitive].length`);
E(`Date.prototype[Symbol.toPrimitive].name`);
E(`Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).writable + ' ' + Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).configurable + ' ' + Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).enumerable`);
E(`Date.prototype[Symbol.toPrimitive].call({ toString() { return 's' }, valueOf() { return 7 } }, 'default')`);
E(`Date.prototype[Symbol.toPrimitive].call({ toString() { return 's' }, valueOf() { return 7 } }, 'number')`);
E(`Date.prototype[Symbol.toPrimitive].call({ toString() { return 's' }, valueOf() { return 7 } }, 'string')`);
E(`Date.prototype[Symbol.toPrimitive].call({ toString() { return {} }, valueOf() { return {} } }, 'string')`);
E(`Date.prototype[Symbol.toPrimitive].call({ toString: null, valueOf() { return 7 } }, 'string')`);
E(`Date.prototype[Symbol.toPrimitive].call(1, 'number')`);
E(`Date.prototype[Symbol.toPrimitive].call('x', 'number')`);
E(`Date.prototype[Symbol.toPrimitive].call(undefined, 'number')`);
E(`Date.prototype[Symbol.toPrimitive].call(null, 'number')`);
E(`Date.prototype[Symbol.toPrimitive].call(Symbol(), 'number')`);
E(`new Date(5) + 1`);
E(`new Date(5) - 1`);
E(`new Date(5) * 2`);
E(`+new Date(5)`);
E(`-new Date(5)`);
E(`new Date(5) + new Date(6)`);
E(`new Date(6) - new Date(5)`);
E(`new Date(5) == 5`);
E(`new Date(5) == new Date(5).toString()`);
E(`new Date(5) < new Date(6)`);
E(`new Date(5) <= new Date(5)`);
E(`new Date(5) == new Date(5)`);
E(`new Date(NaN) == new Date(NaN)`);
E(`new Date(NaN) + 1`);
E(`new Date(NaN) - 1`);
E(`\`\${new Date(NaN)}\``);
E(`[new Date(0)] + ''`);
E(`'' + new Date(0)`);
E(`new Date(0) + 0`);
E(`typeof (new Date(0) + 1)`);
E(`typeof (new Date(0) - 1)`);
E(`Math.max(new Date(5), new Date(6))`);
E(`Number(new Date(7))`);
E(`String(new Date(NaN))`);
E(`isNaN(new Date(NaN))`);
E(`isNaN(new Date('x'))`);
E(`new Date(0) ? 1 : 2`);
E(`!new Date(NaN)`);
E(`(function () { var d = new Date(0); d[Symbol.toPrimitive] = function () { return 42 }; return S(d + 1) + ' ' + S(d - 1) + ' ' + S(String(d)) })()`);
E(`(function () { var d = new Date(0); d.toString = function () { return 'T' }; d.valueOf = function () { return 9 }; return S(d + 1) + ' ' + S(`+"`${d}`"+`) + ' ' + S(d - 1) })()`);
E(`(function () { var d = new Date(0); d[Symbol.toPrimitive] = undefined; d.toString = function () { return 'T' }; d.valueOf = function () { return 9 }; return S(d + 1) + ' ' + S(d - 1) })()`);
E(`(function () { var d = new Date(0); d[Symbol.toPrimitive] = null; return S(d + 1) })()`);
E(`(function () { var d = new Date(0); d[Symbol.toPrimitive] = 1; return S(d + 1) })()`);
E(`new Date(new Date(5)).getTime()`);
E(`new Date(new Date(NaN)).getTime()`);
E(`new Date({ valueOf() { return 7 } }).getTime()`);
E(`new Date({ toString() { return '2024-01-02' }, valueOf() { return {} } }).getTime()`);
E(`new Date({ [Symbol.toPrimitive]() { return 11 } }).getTime()`);
E(`new Date({ [Symbol.toPrimitive]() { return 'Jan 1 2024' } }).getTime()`);
E(`new Date({ [Symbol.toPrimitive](h) { return h } }).getTime()`);
E(`new Date(new Number(9)).getTime()`);
E(`new Date(Symbol()).getTime()`);
E(`new Date(1n).getTime()`);
E(`new Date(2024, 1n).getTime()`);
E(`new Date(0).toJSON.call({ toISOString() { return 'X' }, valueOf() { return 1 } })`);
E(`new Date(0).toJSON.call({ toISOString: 1, valueOf() { return 1 } })`);
E(`new Date(0).toJSON.call({ toISOString() { return 'X' }, valueOf() { return NaN } })`);
E(`new Date(0).toJSON.call({ toISOString() { return 'X' }, valueOf() { return Infinity } })`);
E(`new Date(0).toJSON.call({ toISOString() { return 'X' }, toString() { return 'a' }, valueOf() { return {} } })`);
E(`Date.prototype.toJSON.call(1)`);
E(`Date.prototype.toJSON.length`);
E(`Date.prototype.valueOf.call({})`);
E(`Date.prototype.toISOString.call({})`);
E(`Date.prototype.toString.call({})`);
E(`Date.prototype.toString.call(Date.prototype)`);
E(`Date.prototype.getTime.call(Date.prototype)`);
E(`Object.prototype.toString.call(Date.prototype)`);
E(`Date.prototype.constructor === Date`);
E(`typeof Date.now() + ' ' + (Date.now() > 1.7e12) + ' ' + Date.now.length`);
E(`Date.length + ' ' + Date.name`);
E(`Object.getOwnPropertyNames(Date).sort().join()`);
E(`Object.getOwnPropertyNames(Date.prototype).sort().join()`);
E(`Date.prototype.toLocaleString.name + ' ' + Date.prototype.toLocaleDateString.name + ' ' + Date.prototype.toLocaleTimeString.name`);
E(`Date.prototype.toISOString.call(new Date(NaN))`);
E(`new Date(NaN).toJSON()`);
E(`new Date(NaN).toString()`);
E(`new Date(NaN).toDateString() + new Date(NaN).toTimeString() + new Date(NaN).toUTCString()`);

// ---- Subclasses de Date.
const sub = "class D extends Date { foo() { return this.getUTCFullYear() } }; ";
E(`(function () { ${sub} var d = new D(${BASE}); return S(d instanceof D) + S(d instanceof Date) + S(d.foo()) + S(d.toISOString()) })()`);
E(`(function () { ${sub} var d = new D(2024, 1, 29); return S(d.getTime()) + ' ' + S(d.constructor === D) + ' ' + S(Object.getPrototypeOf(d) === D.prototype) })()`);
E(`(function () { ${sub} var d = new D('Jan 1 2024 10:00 PM'); return S(d.toISOString()) })()`);
E(`(function () { ${sub} var d = new D(NaN); return S(d.getTime()) + S(String(d)) })()`);
E(`(function () { ${sub} var d = new D(); return S(typeof d.getTime()) + S(d instanceof D) })()`);
E(`(function () { ${sub} return S(D.UTC(2024, 0)) + ' ' + S(D.parse('Jan 1 2024')) + ' ' + S(typeof D.now()) + ' ' + S(Object.getPrototypeOf(D) === Date) })()`);
E(`(function () { ${sub} var d = new D(0); d.setUTCFullYear(2000); return S(d.toISOString()) + S(JSON.stringify(d)) + S(Object.prototype.toString.call(d)) })()`);
E(`(function () { ${sub} var d = new D(0); return S(d + 1) + ' ' + S(d - 1) + ' ' + S(d[Symbol.toPrimitive]('number')) })()`);
E(`(function () { ${sub} return S(D(0)) })()`);
E(`(function () { class E2 extends Date { constructor(...a) { super(...a); this.tag = 'x' } } var d = new E2(5); return S(d.getTime()) + S(d.tag) + S(d instanceof Date) })()`);
E(`(function () { class E2 extends Date { constructor() { super(2024, 0, 1) } } var d = new E2(); return S(d.getTime()) })()`);
E(`(function () { class E2 extends Date { constructor() { super(NaN); } } return S(new E2().toString()) })()`);
E(`(function () { class E2 extends Date { constructor() { return {} } } return S(typeof new E2().getTime) })()`);
E(`(function () { class E2 extends Date { constructor() { return super(7), new Date(8) } } return S(new E2().getTime()) })()`);
E(`(function () { class E2 extends Date { getTime() { return 99 } } var d = new E2(5); return S(d.getTime()) + S(Date.prototype.getTime.call(d)) + S(d.valueOf()) + S(+d) })()`);
E(`(function () { class E2 extends Date { valueOf() { return 99 } } var d = new E2(5); return S(+d) + S(d - 1) + S(d + 1 === String(d) + 1) + S(JSON.stringify(d)) })()`);
E(`(function () { class E2 extends Date { toString() { return 'custom' } } var d = new E2(5); return S(String(d)) + S(d + '') + S(\`\${d}\`) + S(d.toUTCString()) })()`);
E(`(function () { class E2 extends Date { toISOString() { return 'iso' } } var d = new E2(5); return S(JSON.stringify(d)) + S(d.toJSON()) })()`);
E(`(function () { class E2 extends Date { static get [Symbol.species]() { return Date } } return S(typeof E2.prototype.getTime) })()`);
E(`(function () { ${sub} var d = Reflect.construct(Date, [5], D); return S(Object.getPrototypeOf(d) === D.prototype) + S(d.getTime()) + S(d instanceof D) })()`);
E(`(function () { var d = Reflect.construct(Date, [5], Object); return S(Object.getPrototypeOf(d) === Object.prototype) + S(typeof d.getTime) })()`);
E(`(function () { function F() {} F.prototype = null; var d = Reflect.construct(Date, [5], F); return S(Object.getPrototypeOf(d) === Object.prototype) })()`);
E(`(function () { var d = Reflect.construct(Date, [2024, 1], Array); return S(Object.getPrototypeOf(d) === Array.prototype) + S(Date.prototype.getTime.call(d)) })()`);
E(`(function () { function F() {} var d = Reflect.construct(Date, [5], F); return S(d instanceof F) + S(d instanceof Date) + S(Date.prototype.getTime.call(d)) })()`);
E(`(function () { var d = Object.create(Date.prototype); return S(d.getTime()) })()`);
E(`(function () { var d = Object.setPrototypeOf({}, Date.prototype); return S(d.toISOString()) })()`);
E(`Date.call({}).length > 20`);
E(`typeof Date.call(null, 5)`);
E(`new Date(2024, 0).constructor === Date`);
E(`new (Date.bind(null, 2024, 1))().getTime()`);
E(`new (Date.bind(null, 2024))(2).getTime()`);
E(`Date.apply(null, [2024, 1]) === Date(2024, 1)`);
E(`Reflect.apply(Date, null, []).length > 20`);
E(`Reflect.construct(Date, [2024, 1, 29]).getTime()`);
E(`new Date(...[2024, 1, 29, 1, 2, 3, 4]).toISOString()`);
E(`Date.UTC(...[2024, 1, 29, 1, 2, 3, 4])`);

// ---- Cruzamentos: parse de saída de toString/toUTCString com anos extremos e arredondamento.
for (const y of [-271821, -100000, -10000, -1000, -100, -10, -1, 0, 1, 9, 10, 99, 100, 999, 1000, 1969, 1970, 1999, 2000, 2024, 9999, 10000, 99999, 100000, 275760]) {
  E(`(function () { var d = new Date(0); d.setUTCFullYear(${y}); return S(d.getTime()) + ' ' + S(d.toISOString()) + ' ' + S(d.toUTCString()) + ' ' + S(d.toDateString()) + ' ' + S(d.getYear()) })()`);
  E(`(function () { var d = new Date(0); d.setYear(${y}); return S(d.getTime()) + ' ' + S(d.getUTCFullYear()) })()`);
  E(`Date.UTC(${y}, 0, 1)`);
  E(`Date.UTC(${y}, 5)`);
  E(`Date.parse(\`Jan 1 \${${y}}\`)`);
  E(`Date.parse(\`Mon, 01 Jan \${${y}} 00:00:00 GMT\`)`);
  E(`Date.parse(new Date(Date.UTC(${y}, 6, 4)).toUTCString())`);
  E(`Date.parse(new Date(Date.UTC(${y}, 6, 4)).toString())`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "date-utc-golden-"));
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
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  // Fuso fixo: o bun roda com TZ=UTC e o programa não lê TZ.
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000, env: { ...process.env, TZ: "UTC" } });
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
  // Instante atual muda a cada execução: fora do golden.
  if (/Date\.now\(\)|new Date\(\)\.|Date\(\)/.test(body) && /getTime|length > 20/.test(body) === false) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
