// Gera tests/golden/date_parse_bun.tsv: Date.parse e `new Date(string)` em grade de formatos, Date.UTC e setters com
// NaN e overflow, formatadores nos limites (±8.64e15, ano 0, negativo, acima de 9999), Symbol.toPrimitive e aritmética
// com Date, medido no bun 1.4.2 com TZ=UTC no filho.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo (sem APIs de host) e programas cuja fonte já está em outro golden são descartados.
// Uso: bun scripts/gen-date-parse-golden.js > tests/golden/date_parse_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);' +
  'if(v instanceof Date)return "Date("+Object.prototype.toString.call(v)+")"+v.getTime();' +
  'if(Array.isArray(v))return "["+v.map(S).join(",")+"]";return Object.prototype.toString.call(v)}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = (s) => JSON.stringify(s);
const T = (e) => `T(()=>${e})`;

// ---- A. Grade ISO: data x hora x zona.
const isoDates = [
  "2024-01-15", "2024-01", "2024", "+002024-01-15", "-000001-01-01", "-000000-01-01", "+275760-09-13", "-271821-04-20",
  "+275760-09-14", "0000-01-01", "9999-12-31", "10000-01-01", "2024-02-30", "2024-13-01", "2024-00-10", "2024-02-29",
  "2023-02-29", "1970-01-01", "1969-12-31", "0001-01-01", "+010000-01-01", "24-01-15", "2024-1-5", "20240115",
];
const isoTimes = ["", "T00:00", "T12:34:56", "T12:34:56.789", "T24:00:00", "T24:00:01", "T12:34:56.7891234", "T25:00", "T12", "T12:34:56,5"];
const isoZones = ["", "Z", "+01:00", "-05:30", "+0100", "+01", "z", "+24:00"];
for (const d of isoDates) {
  for (const t of isoTimes) {
    for (const z of isoZones) {
      // Zona sem hora só interessa em poucos casos.
      if (t === "" && z !== "" && z !== "Z") continue;
      const s = d + t + z;
      add(T(`Date.parse(${q(s)})`));
    }
  }
}
let counter = 0;
for (const d of isoDates) {
  for (const t of isoTimes) {
    for (const z of ["", "Z", "+01:00"]) {
      if (t === "" && z !== "") continue;
      const s = d + t + z;
      counter++;
      if (counter % 2 === 0) add(T(`new Date(${q(s)}).toISOString()`));
      else add(T(`String(new Date(${q(s)}))`));
    }
  }
}

// ---- B. Formatos legados, RFC 2822, parênteses, meses abreviados, AM/PM.
const legacy = [
  "Mon Jan 01 2024", "Mon Jan 01 2024 10:20:30", "Mon Jan 01 2024 10:20:30 GMT+0000", "Mon Jan 01 2024 10:20:30 GMT+0100 (CET)",
  "Mon Jan 01 2024 10:20:30 GMT-0330 (Newfoundland)", "Mon Jan 01 2024 10:20:30 UTC", "Mon Jan 01 2024 10:20:30 Z", "Jan 01 2024",
  "January 1, 2024", "January 1, 2024 10:20", "Jan 1, 2024 10:20 PM", "Jan 1, 2024 10:20 AM", "Jan 1, 2024 12:00 AM", "Jan 1, 2024 12:00 PM",
  "Jan 1, 2024 13:00 PM", "Jan 1, 2024 0:00 AM", "Jan 1, 2024 12:60 AM", "1 Jan 2024", "1 January 2024 10:20:30", "01 Jan 2024 10:20:30 GMT",
  "Mon, 01 Jan 2024 10:20:30 GMT", "Mon, 01 Jan 2024 10:20:30 +0000", "Mon, 01 Jan 2024 10:20:30 -0800", "Mon, 01 Jan 2024 10:20:30 EST",
  "Mon, 01 Jan 2024 10:20:30 EDT", "Mon, 01 Jan 2024 10:20:30 PST", "Mon, 01 Jan 2024 10:20:30 PDT", "Mon, 01 Jan 2024 10:20:30 CST",
  "Mon, 01 Jan 2024 10:20:30 MST", "Mon, 01 Jan 2024 10:20:30 UT", "Mon, 01 Jan 2024 10:20:30 XYZ", "Tue, 01 Jan 2024 10:20:30 GMT",
  "Sun, 31 Dec 2023 23:59:59 GMT", "Thu, 01 Jan 1970 00:00:00 GMT", "Fri, 13 Sep 275760 00:00:00 GMT", "Sat, 13 Sep 275760 00:00:00 GMT",
  "Tue, 20 Apr -271821 00:00:00 GMT", "Mon, 01 Jan 0000 00:00:00 GMT", "Mon, 01 Jan 99 00:00:00 GMT", "Mon, 01 Jan 49 00:00:00 GMT",
  "Mon, 01 Jan 50 00:00:00 GMT", "Mon, 01 Jan 00 00:00:00 GMT", "1/2/2024", "01/02/2024", "1/2/24", "1/2/99", "1/2/49", "1/2/50", "12/31/1999",
  "13/1/2024", "2/30/2024", "2/29/2024", "2/29/2023", "1/1/0", "1/1/100", "1/1/1", "1/1/-1", "2024/01/02", "2024/1/2 10:20", "2024/13/02",
  "1-2-2024", "01-02-2024", "2024-01-02 10:20", "2024-01-02 10:20:30", "2024-01-02 10:20:30Z", "2024-01-02 10:20:30+01:00",
  "2024-01-02 10:20:30.123", "2024-01-02T10:20:30 ", " 2024-01-02", "2024-01-02 ", "2024-01-02T10:20:30+0100", "Jan 2024", "2024 Jan", "Jan 2",
  "Jan", "Monday", "Mon", "Mon 1", "10:20", "10:20:30", "noon", "now", "today", "tomorrow", "", " ", "x", "abc 123", "1", "12", "123", "1234",
  "12345", "0", "-1", "+1", "1e3", "1.5", "NaN", "Infinity", "null", "undefined", "true", "[object Object]", "Invalid Date",
  "Jan 1 2024 (comment) 10:20", "(comment) Jan 1 2024", "Jan 1 2024 (nested (comment))", "Jan 1 2024 (unclosed", "Jan 1 2024 )", "Jan (x) 1 2024",
  "Mon Jan 01 2024 (nome do fuso)", "Sept 1 2024", "Sep 1 2024", "September 1 2024", "Sepx 1 2024", "Janu 1 2024", "JAN 1 2024", "jan 1 2024",
  "jAn 1 2024", "MON JAN 01 2024", "Mon Jan 32 2024", "Mon Jan 00 2024", "Feb 29 2024", "Feb 29 2023", "Feb 30 2024", "Apr 31 2024",
  "Dec 31 2024 23:59:59", "Dec 31 2024 24:00:00", "Dec 31 2024 24:00:01", "Dec 31 2024 25:00", "Dec 31 2024 23:60", "Dec 31 2024 23:59:60",
  "Dec 31 2024 23:59:61", "Dec 31 2024 10:20:30.5", "Dec 31 2024 10:20:30.123456", "Jan 1 2024 GMT", "Jan 1 2024 GMT+1", "Jan 1 2024 GMT+01", "Jan 1 2024 GMT+0100",
  "Jan 1 2024 GMT+01:00", "Jan 1 2024 GMT+2400", "Jan 1 2024 GMT+9999", "Jan 1 2024 UTC+1", "Jan 1 2024 +0100", "Jan 1 2024 -0100", "Jan 1 2024 10:00 +0100",
  "Jan 1 2024 10:00 -0100", "Jan 1 2024 10:00 +01:00", "Jan 1 2024 10:00 +1", "Jan 1 2024 10:00 Z", "Jan 1 2024 10:00 z", "Jan 1 2024 10:00 GMT-0000",
  "Jan 1 275760 00:00", "Jan 1 -1 00:00", "Jan 1 0 00:00", "Jan 1 99", "Jan 1 100", "Jan 1 999", "Jan 1 10000", "Jan 1 100000", "Jan 1 275760", "Jan 1 275761",
  "1 1 2024", "1 2 3", "2024 1 1", "1 Jan", "Jan 1", "Jan 1 10:20", "Mon Jan 01 2024 10:20:30 GMT+0000 (Coordinated Universal Time)",
  "Mon Jan 01 2024 10:20:30 GMT+0000 (Coordinated Universal Time) extra", "Thu, 01 Jan 1970 00:00:00 GMT+0100", "Thu, 01 Jan 1970 00:00:00 GMT-0100",
  "Thu Jan 01 1970 00:00:00 GMT+0000", "Thursday, January 1, 1970", "Thursday, 01-Jan-70 00:00:00 GMT", "Thursday, 01-Jan-1970 00:00:00 GMT",
  "01-Jan-1970", "1-Jan-70", "1-Jan-1970 10:20", "Jan-01-1970", "2024-01-02T10:20:30.123456789Z", "2024-01-02T10:20:30.1Z", "2024-01-02T10:20:30.Z",
  "2024-01-02T10:20:30Z+01:00", "2024-01-02T10:20:30+01:00Z", "2024-01-02T10:20:30+1:00", "2024-01-02T10:20:30+01:0", "2024-01-02T10:20:30-00:00",
  "2024-01-02T10:20:30+00:00", "2024-01-02t10:20:30z", "2024-01-02T10:20:30z", "2024-01-02 10:20:30 z", "2024-01-02T1:20:30", "2024-01-02T10:2:30",
  "2024-01-02T10:20:3", "2024-01-02T10:20:30.", "2024-01-02T10", "2024-01-02T", "2024-01-02Tx", "2024-W01-1", "2024-001", "2024-366", "2024-1",
  "+2024-01-02", "-2024-01-02", "+02024-01-02", "+0002024-01-02", "-002024-01-02", "+002024-1-02", "+002024-01", "+002024", "-000000", "-000000-01",
  "-000000-01-01T00:00:00Z", "+275760-09-13T00:00:00.000Z", "+275760-09-13T00:00:00.001Z", "-271821-04-20T00:00:00.000Z", "-271821-04-19T23:59:59.999Z",
];
for (const s of legacy) {
  add(T(`Date.parse(${q(s)})`));
  add(T(`String(new Date(${q(s)}))`));
  add(T(`new Date(${q(s)}).toISOString()`));
}

// ---- C. Date.UTC, construtor numérico e setters com NaN e overflow.
const numArgs = [
  "2024", "2024,0", "2024,11,31", "2024,12,1", "2024,-1,1", "2024,0,0", "2024,0,32", "2024,0,1,24", "2024,0,1,25", "2024,0,1,-1", "2024,0,1,0,60",
  "2024,0,1,0,-1", "2024,0,1,0,0,60", "2024,0,1,0,0,-1", "2024,0,1,0,0,0,1000", "2024,0,1,0,0,0,-1", "0", "0,0", "99", "99,0", "100", "100,0", "-1", "-1,0",
  "1899,0", "1900,0", "1900.9,0", "99.9,0", "NaN", "2024,NaN", "2024,0,NaN", "2024,0,1,NaN", "2024,0,1,0,NaN", "2024,0,1,0,0,NaN", "2024,0,1,0,0,0,NaN",
  "Infinity", "-Infinity", "2024,Infinity", "2024,0,-Infinity", "275760,8,13", "275760,8,14", "-271821,3,20", "-271821,3,19", "275760,8,13,0,0,0,1",
  "-271821,3,20,0,0,0,-1", "2024,0.9", "2024,-0.9", "2024,0,1.9", "2024,0,1,0.9", "'2024'", "'2024','1'", "'x'", "null", "undefined", "2024,undefined",
  "2024,null", "true", "2024,true", "{valueOf(){return 2024}}", "{valueOf(){return 2024}},{valueOf(){return 1}}", "1e20", "2024,1e20", "2024,0,1e20", "2024,0,1,1e20",
  "1e10,0", "2024,1e10", "2024,0,1e10", "2024,0,1,0,0,0,1e20", "2024,0,1,0,0,0,8.64e15", "1970,0,1,0,0,0,-1", "1969,11,31,23,59,59,999", "2024,1,29", "2023,1,29",
  "2024,0,1,0,0,0,0.9", "2024,0,1,0,0,0,-0.9", "-0", "-0,0,1", "2024,2,31", "2024,3,31", "2024,0,1,23,59,59,999", "275760,8,13,0,0,0,0", "275760,8,13,23,59,59,999",
];
for (const a of numArgs) {
  add(T(`Date.UTC(${a})`));
  add(T(`new Date(${a}).getTime()`));
  add(T(`new Date(${a}).toISOString()`));
}
add(T("Date.UTC()"), T("Date.UTC.length"), T("Date.length"), T("Date.name"), T("Date.parse.length"), T("Date.now.length"), T("typeof Date.now()"),
  T("typeof Date()"),T("Date(0).length"), T("new Date().constructor===Date"), T("new Date(undefined).getTime()"),
  T("new Date(null).getTime()"), T("new Date(NaN).getTime()"), T("new Date(true).getTime()"), T("new Date(false).getTime()"), T("new Date([]).getTime()"),
  T("new Date([5]).getTime()"), T("new Date([1,2]).getTime()"), T("new Date({}).getTime()"), T("new Date({valueOf(){return 7}}).getTime()"),
  T("new Date({toString(){return '2024-01-01'}}).getTime()"), T("new Date({toString(){return '2024-01-01'},valueOf(){return 3}}).getTime()"),
  T("new Date(new Date(5)).getTime()"), T("new Date(Symbol())"), T("new Date(1n)"), T("new Date(8.64e15).getTime()"), T("new Date(8.64e15+1).getTime()"),
  T("new Date(-8.64e15).getTime()"), T("new Date(-8.64e15-1).getTime()"), T("new Date(1.9).getTime()"), T("new Date(-1.9).getTime()"), T("new Date(-0).getTime()"),
  T("Object.is(new Date(-0).getTime(),0)"), T("new Date(Infinity).getTime()"), T("new Date('5').getTime()"), T("new Date('  ').getTime()"),
  T("Reflect.construct(Date,[0],Object).constructor===Object"), T("Object.getPrototypeOf(Reflect.construct(Date,[0],Object))===Object.prototype"),
  T("Object.prototype.toString.call(new Date(0))"), T("Object.prototype.toString.call(Object.create(Date.prototype))"),
  T("Date.prototype.getTime.call({})"), T("Date.prototype.toString.call({})"), T("Date.prototype.toISOString.call(0)"), T("Date.prototype.valueOf.call(Object.create(Date.prototype))"),
  T("Date.prototype.getTime.call(new Proxy(new Date(0),{}))"), T("Date.prototype.toString()"), T("Date.prototype.getTime()"), T("Object.prototype.toString.call(Date.prototype)"));

const bases = [
  "new Date(2024,0,31,10,20,30,400)", "new Date(NaN)", "new Date(0)", "new Date(8.64e15)", "new Date(-8.64e15)", "new Date(2024,1,29)", "new Date(1e12)",
];
const setters = [
  ["setMilliseconds", ["0", "999", "1000", "-1", "NaN", "1e20", "Infinity", "undefined", "'5'", "", "1.9", "-1.9", "8.64e15"]],
  ["setSeconds", ["0", "59", "60", "-1", "NaN", "30,500", "30,NaN", "1e20", "undefined", "", "3600", "86400,1000"]],
  ["setMinutes", ["0", "59", "60", "-1", "NaN", "30,15,500", "1,2,NaN", "1e20", "", "1440", "undefined"]],
  ["setHours", ["0", "23", "24", "-1", "NaN", "10,20,30,400", "10,NaN", "1e20", "", "48", "undefined", "10,20,30,NaN"]],
  ["setDate", ["1", "0", "-1", "31", "32", "366", "NaN", "1e20", "", "undefined", "29", "30", "1.9", "-1.9", "100000000"]],
  ["setMonth", ["0", "11", "12", "-1", "NaN", "1,15", "1,NaN", "1e20", "", "undefined", "13,31", "1,0", "-13", "1200000"]],
  ["setFullYear", ["2024", "0", "-1", "99", "NaN", "2025,1,29", "2023,1,29", "2024,12,1", "2024,NaN", "1e20", "", "undefined", "275760", "275761", "-271821", "-271822", "2024,0,NaN", "10000", "100000"]],
  ["setUTCMilliseconds", ["0", "1000", "-1", "NaN", "", "1e20"]],
  ["setUTCSeconds", ["0", "60", "-1", "NaN", "", "30,500"]],
  ["setUTCMinutes", ["0", "60", "-1", "NaN", "", "30,15,500"]],
  ["setUTCHours", ["0", "24", "-1", "NaN", "", "10,20,30,400"]],
  ["setUTCDate", ["0", "32", "-1", "NaN", "", "1"]],
  ["setUTCMonth", ["0", "12", "-1", "NaN", "", "1,15"]],
  ["setUTCFullYear", ["2024", "-1", "NaN", "", "2025,1,29", "2023,1,29", "275760,8,13", "275760,8,14"]],
  ["setTime", ["0", "NaN", "8.64e15", "8.64e15+1", "-8.64e15", "-8.64e15-1", "1.9", "-1.9", "", "undefined", "'5'", "-0", "Infinity", "1e20", "null"]],
  ["setYear", ["99", "100", "2024", "0", "-1", "NaN", "", "1999.9", "undefined"]],
];
for (const base of bases) {
  for (const [name, argList] of setters) {
    for (const a of argList) {
      add(T(`(()=>{var d=${base};var r=d.${name}(${a});return S(r)+"|"+d.getTime()})()`));
    }
  }
}
// Setter em Date inválida com NaN e retorno.
add(T("(()=>{var d=new Date(NaN);d.setFullYear(2024);return d.toISOString()})()"), T("(()=>{var d=new Date(NaN);d.setMonth(1);return d.getTime()})()"),
  T("(()=>{var d=new Date(NaN);d.setHours(1);return d.getTime()})()"), T("(()=>{var d=new Date(NaN);d.setUTCFullYear(2024,1,2);return d.toISOString()})()"),
  T("(()=>{var d=new Date(NaN);d.setTime(5);return d.getTime()})()"), T("(()=>{var d=new Date(0);d.setHours({valueOf(){d.setTime(NaN);return 1}});return d.getTime()})()"),
  T("(()=>{var d=new Date(0);d.setFullYear({valueOf(){d.setTime(NaN);return 2000}});return d.toISOString()})()"),
  T("(()=>{var d=new Date(NaN);var log=[];d.setHours({valueOf(){log.push('a');return 1}},{valueOf(){log.push('b');return 2}});return log.join()})()"),
  T("(()=>{var d=new Date(0);var log=[];d.setMonth({valueOf(){log.push('a');return 1}},{valueOf(){log.push('b');return 2}});return log.join()+d.getTime()})()"),
  T("(()=>{var d=new Date(NaN);var log=[];d.setMinutes({valueOf(){log.push('a');return 1}},{valueOf(){log.push('b');return 2}},{valueOf(){log.push('c');return 3}});return log.join()})()"),
  T("Date.prototype.setHours.call({},1)"), T("Date.prototype.setTime.call(0,1)"), T("Date.prototype.setHours.length+','+Date.prototype.setMinutes.length+','+Date.prototype.setSeconds.length+','+Date.prototype.setMonth.length+','+Date.prototype.setFullYear.length+','+Date.prototype.setUTCHours.length+','+Date.prototype.setUTCFullYear.length"));

// ---- D. Formatadores e getters nos limites.
const times = [
  "8.64e15", "-8.64e15", "8.64e15-1", "-8.64e15+1", "-62167219200000", "-62167219200001", "-62167219199999", "-62198755200000", "-62198755200001",
  "253402300800000", "253402300799999", "253402300800001", "0", "-1", "1", "NaN", "-1e14", "1e14", "2.5e14", "-3e13", "-2e14", "-6e13", "-6.3e13",
  "1.7e15", "-1.7e15", "8.639999999999999e15", "86399999", "-86399999", "951782400000", "951868800000", "-2208988800000", "-2209075200000",
  "1704067200000", "1709164800000", "1e12", "-1e12", "9.4e15", "4.102444800e12",
];
const methods = [
  "toISOString()", "toJSON()", "toString()", "toUTCString()", "toGMTString()", "toDateString()", "toTimeString()", "valueOf()", "getTime()",
  "getTimezoneOffset()", "getFullYear()", "getUTCFullYear()", "getYear()", "getMonth()", "getUTCDate()", "getDay()", "getUTCDay()", "getHours()",
  "getUTCMinutes()", "getSeconds()", "getMilliseconds()", "toJSON('x')", "[Symbol.toPrimitive]('default')", "[Symbol.toPrimitive]('number')",
  "[Symbol.toPrimitive]('string')", "toLocaleString('en-US',{timeZone:'UTC'})", "toLocaleDateString('en-US',{timeZone:'UTC'})", "toLocaleTimeString('en-US',{timeZone:'UTC'})",
];
for (const t of times) {
  for (const m of methods) add(T(`new Date(${t})${m[0] === "[" ? "" : "."}${m}`));
  add(T(`JSON.stringify(new Date(${t}))`), T(`JSON.stringify({d:new Date(${t})})`), T(`String(new Date(${t}))`), T(`new Date(${t})+''`),
    T(`Date.parse(new Date(${t}).toString())`), T(`Date.parse(new Date(${t}).toUTCString())`), T(`Date.parse(new Date(${t}).toISOString())`),
    T(`new Date(new Date(${t}).toString()).getTime()`), T(`new Date(new Date(${t}).toUTCString()).getTime()`), T(`new Date(new Date(${t}).toDateString()).getTime()`));
}
add(T("Date.prototype.toJSON.call({toISOString(){return 'x'}})"), T("Date.prototype.toJSON.call({valueOf(){return NaN},toISOString(){return 'x'}})"),
  T("Date.prototype.toJSON.call({valueOf(){return 1},toISOString(){return 'x'}})"), T("Date.prototype.toJSON.call({valueOf(){return 1}})"),
  T("Date.prototype.toJSON.call({valueOf(){return 1},toISOString:1})"), T("Date.prototype.toJSON.call(null)"), T("Date.prototype.toJSON.call('x')"),
  T("Date.prototype.toJSON.call(1)"), T("Date.prototype.toJSON.call({toISOString(){return this===undefined}})"), T("Date.prototype.toJSON.length"),
  T("Date.prototype.toGMTString===Date.prototype.toUTCString"), T("Date.prototype.toISOString.length+Date.prototype.toISOString.name"),
  T("Object.getOwnPropertyNames(Date.prototype).sort().join()"), T("Object.getOwnPropertyNames(Date).sort().join()"),
  T("Date.prototype[Symbol.toPrimitive].name+Date.prototype[Symbol.toPrimitive].length"), T("JSON.stringify(Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive))"),
  T("Date.prototype.toLocaleString.length"), T("Date.prototype.constructor===Date"), T("Date.prototype.getYear.length"));

// ---- E. Symbol.toPrimitive, hints inválidos, aritmética.
const hints = ["'default'", "'number'", "'string'", "''", "'Number'", "'String'", "'DEFAULT'", "' number'", "undefined", "null", "1", "true", "{}", "[]", "Symbol()", "{toString(){return 'number'}}", "{toString(){return 'string'}}", "'number\\0'", "'strin'", "'numbers'", "'defaul'"];
for (const h of hints) {
  add(T(`new Date(5)[Symbol.toPrimitive](${h})`));
  add(T(`Date.prototype[Symbol.toPrimitive].call({toString(){return 'S'},valueOf(){return 7}},${h})`));
  add(T(`Date.prototype[Symbol.toPrimitive].call(new Date(0),${h})`));
  add(T(`Date.prototype[Symbol.toPrimitive].call({toString(){return {}},valueOf(){return 7}},${h})`));
  add(T(`Date.prototype[Symbol.toPrimitive].call({toString(){return {}},valueOf(){return {}}},${h})`));
}
add(T("new Date(5)[Symbol.toPrimitive]()"), T("Date.prototype[Symbol.toPrimitive].call(1,'number')"), T("Date.prototype[Symbol.toPrimitive].call(null,'number')"),
  T("Date.prototype[Symbol.toPrimitive].call(undefined,'number')"), T("Date.prototype[Symbol.toPrimitive].call('s','number')"), T("Date.prototype[Symbol.toPrimitive].call(Symbol(),'number')"),
  T("Date.prototype[Symbol.toPrimitive].call(function(){},'number')"), T("Date.prototype[Symbol.toPrimitive].call({},'string')"),
  T("Date.prototype[Symbol.toPrimitive].call({toString:1,valueOf:2},'string')"), T("Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3}},'string')"),
  T("Date.prototype[Symbol.toPrimitive].call({toString(){return 'a'}},'number')"), T("Date.prototype[Symbol.toPrimitive].call(Object.create(null),'number')"));
const ar = [
  "new Date(5)+1", "new Date(5)-1", "new Date(5)*2", "new Date(5)/2", "new Date(5)%3", "new Date(5)**2", "+new Date(5)", "-new Date(5)", "~new Date(5)", "!new Date(5)", "!new Date(NaN)",
  "new Date(0)+new Date(0)", "new Date(5)-new Date(3)", "new Date(5)>new Date(3)", "new Date(5)<new Date(3)", "new Date(5)>=new Date(5)", "new Date(5)==new Date(5)",
  "new Date(5)===new Date(5)", "new Date(5)==5", "new Date(5)=='5'", "(d=>d==d)(new Date(5))", "new Date(5)==new Date(5).toString()", "new Date(0)==new Date(0).toString()",
  "new Date(0)==Date(0)", "new Date(0)>'1'", "new Date(NaN)==new Date(NaN)", "new Date(NaN)<new Date(NaN)", "new Date(NaN)+1", "new Date(NaN)-1", "new Date(NaN)+''",
  "`${new Date(0)}`", "`${new Date(NaN)}`", "[new Date(0)]+''", "[new Date(NaN)]+''", "String(new Date(0))==new Date(0).toString()", "new Date(0)+null", "new Date(0)+undefined",
  "new Date(0)+true", "new Date(0)-null", "new Date(0)-undefined", "new Date(0)-'1'", "new Date(0)+[]", "new Date(0)+{}", "new Date(0)-[]", "new Date(0)-{}", "1+new Date(0)",
  "'x'+new Date(0)", "5-new Date(3)", "new Date(0)|0", "new Date(8.64e15)|0", "new Date(8.64e15)>>>0", "new Date(1e12)|0", "Math.max(new Date(5),new Date(7))", "Math.min(new Date(5),NaN)",
  "isNaN(new Date(NaN))", "isNaN(new Date(0))", "Number(new Date(5))", "Number(new Date(NaN))", "String(new Date(NaN))", "Object(new Date(5))-0", "BigInt(new Date(5).getTime())",
  "parseInt(new Date(5))", "parseFloat(new Date(0))", "Number.isNaN(+new Date('x'))", "[new Date(3),new Date(1),new Date(2)].sort((a,b)=>a-b).map(Number).join()",
  "[new Date(3),new Date(1),new Date(2)].sort().map(Number).join()", "new Date(5)<'6'", "new Date(5)<'x'", "new Date(5)>=null", "typeof (new Date(5)+1)", "typeof (new Date(5)-1)",
  "typeof new Date(5)[Symbol.toPrimitive]", "new Date(5).valueOf()===new Date(5).getTime()", "Object.is(new Date(-0).valueOf(),0)", "Object.is(new Date(-0.5).valueOf(),-0)",
  "JSON.stringify([new Date(0),new Date(NaN)])", "JSON.stringify({d:new Date(NaN)})", "JSON.stringify(new Date(8.64e15))", "JSON.stringify(new Date(-8.64e15))",
  "JSON.parse(JSON.stringify(new Date(0)))", "new Date(JSON.parse(JSON.stringify(new Date(123456789))))-0",
  "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=()=>42;return d+1})()", "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=undefined;return d+1})()",
  "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=null;return d+1})()", "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=1;return d+1})()",
  "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=()=>({});return d+1})()", "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=(h)=>h;return d+1})()",
  "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=(h)=>h;return d-1})()", "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=(h)=>h;return `${d}`})()",
  "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=(h)=>h;return d==1})()", "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=(h)=>h;return d<1})()",
  "(()=>{var d=new Date(0);d[Symbol.toPrimitive]=(h)=>h;return d==='default'})()", "(()=>{var d=new Date(0);d.valueOf=()=>9;return d+1})()",
  "(()=>{var d=new Date(0);d.toString=()=>'t';return d+1})()", "(()=>{var d=new Date(0);d.toString=()=>'t';return `${d}`})()", "(()=>{var d=new Date(0);d.valueOf=()=>9;return `${d}`})()",
  "(()=>{var d=new Date(0);d.valueOf=()=>9;return d-1})()", "(()=>{var d=new Date(0);d.toString=undefined;return d+1})()",
  "(()=>{var old=Date.prototype[Symbol.toPrimitive];Date.prototype[Symbol.toPrimitive]=undefined;try{return new Date(0)+1}finally{Date.prototype[Symbol.toPrimitive]=old}})()",
  "(()=>{class D extends Date{}var d=new D(5);return (d+1)+','+(d-1)+','+(d instanceof Date)+','+Object.prototype.toString.call(d)})()",
  "(()=>{class D extends Date{[Symbol.toPrimitive](h){return h}}return new D(5)+1})()", "(()=>{class D extends Date{}return D.parse('1970')+','+D.UTC(1970)+','+(D.now()>0)})()",
  "(()=>{class D extends Date{constructor(){super(3,'x')}}return String(new D)})()", "(()=>{class D extends Date{}return new D('Jan 1 2000').getTime()})()",
  "new Date(2024,0,1).getTimezoneOffset()", "new Date(NaN).getTimezoneOffset()", "new Date(8.64e15).getTimezoneOffset()", "new Date(-8.64e15).getTimezoneOffset()",
  "typeof new Date(0).getTimezoneOffset()", "Object.is(new Date(0).getTimezoneOffset(),0)", "Object.is(new Date(0).getTimezoneOffset(),-0)", "Date.prototype.getTimezoneOffset.length",
  "new Date(0).toString()", "new Date(0).toTimeString()", "new Date(1e12).toString().length", "new Date(-62167219200000).toString()", "new Date(-62198755200000).toString()",
  "new Date(253402300800000).toString()", "new Date(-8.64e15).toString()", "new Date(8.64e15).toString()", "new Date(8.64e15).toUTCString()", "new Date(-8.64e15).toUTCString()",
  "new Date(-62167219200000).toUTCString()", "new Date(-62198755200000).toUTCString()", "new Date(253402300800000).toUTCString()", "new Date(8.64e15).toDateString()",
  "new Date(-8.64e15).toDateString()", "new Date(-62198755200000).toDateString()", "new Date(8.64e15).toISOString()", "new Date(-8.64e15).toISOString()",
  "new Date(-62198755200000).toISOString()", "new Date(-62167219200000).toISOString()", "new Date(253402300800000).toISOString()", "new Date(-1).toISOString()",
  "new Date('0000-01-01T00:00:00Z').toISOString()", "new Date('-000001-01-01T00:00:00Z').toISOString()", "new Date('+010000-01-01T00:00:00Z').toISOString()",
  "Date.now()-Date.now()<1000", "Date.now()>1.7e12", "typeof Date.now.call(null)", "Date.now.call(null)>0", "new Date(Date.now()).getTime()>0",
];
for (const e of ar) add(T(e));

// ---- Execução.
const baseSources = new Set();
for (const file of fs.readdirSync(path.join(__dirname, "..", "tests", "golden"))) {
  if (!file.endsWith(".tsv") || file === "date_parse_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8").split("\n")) {
    if (!line) continue;
    baseSources.add(line.split("\t")[0]);
  }
}
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const env = { ...process.env, TZ: "UTC" };
const BAD = new RegExp("/home/|/tmp/|/Users/|\\.js:\\d|bun|\\u2014|\\u2013", "i");

// Processo fresco por programa, igual aos outros geradores: o estado do Date (cache de fuso, tabelas) não vaza.
// Rodam em paralelo (pool), mas a saída sai na ordem da lista.
function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { env, stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    child.stdout.on("data", d => (out += d));
    child.stderr.resume();
    child.on("close", code => resolve(code === 0 ? decodeResult(out) : null));
    child.stdin.end(source);
  });
}

async function main() {
  const jobs = [];
  let dup = 0;
  for (const expr of unique) {
    const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
    const literal = JSON.stringify(source);
    if (baseSources.has(literal)) { dup++; continue; }
    jobs.push({ expr, source, literal, result: undefined });
  }
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const job = jobs[next++];
      job.result = await runChild(job.source);
    }
  };
  await Promise.all(Array.from({ length: 12 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (const job of jobs) {
    if (job.result === null) {
      process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      dropped++;
    } else if (BAD.test(job.result)) {
      // Rejeita o que carrega caminho da máquina, marca do runtime ou travessão.
      process.stderr.write("caminho, marca ou travessão no resultado: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      dropped++;
    } else {
      kept++;
      lines.push(job.literal + "\t" + JSON.stringify(job.result));
    }
  }
  process.stdout.write(emitFactoredLines("date_parse", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
