// Gera tests/golden/date_proto_bun.tsv: setters, getters e formatadores de Date.prototype avaliados no
// bun 1.4.2 em dois fusos (UTC e America/Sao_Paulo). Colunas: fuso, fonte do programa (uma linha),
// KIND e REPR, no formato de date_bun.tsv (serializador em tests/golden/date_bun_harness.js).
// Não repete o que date_tz_bun.tsv, date_parse_bun.tsv e date_pattern_bun.tsv já cobrem: aqui entram
// argumentos opcionais, NaN, Infinity, strings, valueOf com efeito colateral, limites de ±8.64e15,
// anos 0 a 99, Date.UTC com 0 e 1 argumento, transições de horário de verão e conversões primitivas.
// Uso: bun scripts/gen-date-proto-golden.js > tests/golden/date_proto_bun.tsv
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const ZONES = ["UTC", "America/Sao_Paulo"];

const programs = [];
const add = (...sources) => programs.push(...sources);

const state = (d, r) => `var r = ${r}; r + '|' + d.getTime() + '|' + (isNaN(d) ? 'NaN' : d.toISOString())`;

// 1. Setters com argumentos opcionais, NaN, Infinity, strings e fora do intervalo.
const setters = [
  ["setFullYear", 3], ["setMonth", 2], ["setDate", 1], ["setHours", 4], ["setMinutes", 3], ["setSeconds", 2], ["setMilliseconds", 1],
  ["setUTCFullYear", 3], ["setUTCMonth", 2], ["setUTCDate", 1], ["setUTCHours", 4], ["setUTCMinutes", 3], ["setUTCSeconds", 2],
  ["setUTCMilliseconds", 1],
];
const argSets = [
  "", "undefined", "NaN", "Infinity", "-Infinity", "null", "'7'", "'abc'", "''", "true", "7", "-7", "7.9", "-7.9", "0", "-0", "1e10", "-1e10",
  "8.64e15", "-8.64e15", "8.64e15 + 1", "2 ** 32", "2 ** 31", "-(2 ** 32)", "[5]", "{}", "[]", "1, 2", "1, 2, 3", "1, 2, 3, 4",
  "7, undefined", "7, NaN", "7, null", "7, '2'", "7, Infinity", "undefined, 3", "NaN, 3", "1, 2, 3, 4, 5",
];
const bases = ["new Date(2024, 0, 31, 12, 30, 15, 500)", "new Date(NaN)", "new Date(8.64e15)", "new Date(-8.64e15)"];
for (const [name, max] of setters) {
  for (const args of argSets) {
    const count = args === "" ? 0 : args.split(", ").length;
    if (count > max + 1) continue;
    for (const base of bases) {
      if (base === "new Date(2024, 0, 31, 12, 30, 15, 500)" || ["", "NaN", "7", "1, 2"].includes(args) || name === "setFullYear" || name === "setUTCFullYear") {
        add(`var d = ${base}; ${state("d", `d.${name}(${args})`)}`);
      }
    }
  }
}

// 2. Data inválida: setFullYear usa +0, os demais mantêm NaN.
for (const name of ["setFullYear", "setUTCFullYear"]) {
  for (const args of ["2024", "2024, 5", "2024, 5, 15", "NaN", "undefined", "", "1e9"]) {
    add(`var d = new Date(NaN); ${state("d", `d.${name}(${args})`)}`);
  }
}
for (const [name] of setters.slice(1, 7)) {
  for (const args of ["1", "1, 2", "NaN", ""]) add(`var d = new Date(NaN); ${state("d", `d.${name}(${args})`)}`);
}

// 3. Objetos com valueOf que mudam a data durante a conversão dos argumentos (ordem de avaliação).
for (const [name, max] of setters) {
  add(`var d = new Date(2024, 5, 15, 12, 0, 0, 0); var o = { valueOf() { d.setTime(NaN); return 3; } }; ${state("d", `d.${name}(o)`)}`);
  add(`var d = new Date(2024, 5, 15, 12, 0, 0, 0); var o = { valueOf() { d.setTime(0); return 3; } }; ${state("d", `d.${name}(o)`)}`);
  add(`var d = new Date(2024, 5, 15, 12, 0, 0, 0); var log = []; var o = (n) => ({ valueOf() { log.push(n); return n; } }); d.${name}(${Array.from({ length: max }, (_, i) => `o(${i + 1})`).join(", ")}); log.join() + '|' + d.getTime()`);
  add(`var d = new Date(NaN); var o = { valueOf() { d.setTime(86400000); return 3; } }; ${state("d", `d.${name}(o)`)}`);
  add(`var d = new Date(2024, 5, 15); var o = { valueOf() { throw new RangeError('boom'); } }; try { d.${name}(o); } catch (e) { e.message + '|' + d.getTime() }`);
  add(`var d = new Date(NaN); var n = 0; var o = { valueOf() { n++; return 1; } }; d.${name}(o, o, o, o); n + '|' + d.getTime()`);
}
for (const [name] of setters) add(`var d = new Date(2024, 5, 15); try { d.${name}(Symbol()); } catch (e) { e.name }`);
for (const [name] of setters) add(`try { ${name === "setFullYear" ? "Date.prototype.setFullYear.call({}, 1)" : `Date.prototype.${name}.call({}, 1)`} } catch (e) { e.name + ': ' + e.message }`);
for (const [name] of setters) add(`Date.prototype.${name}.length + '|' + Date.prototype.${name}.name`);

// 4. setTime, valueOf e getTime.
for (const arg of ["0", "-0", "NaN", "Infinity", "8.64e15", "8.64e15 + 1", "-8.64e15", "-8.64e15 - 1", "1.9", "-1.9", "'12'", "''", "null", "undefined", "", "true", "[3]", "{}", "1e300", "2 ** 53"]) {
  add(`var d = new Date(5); ${state("d", `d.setTime(${arg})`)}`);
}
add("var d = new Date(8.64e15); d.setMilliseconds(1); d.getTime()", "var d = new Date(8.64e15); d.setUTCMilliseconds(0); d.getTime()");
add("var d = new Date(-8.64e15); d.setUTCMilliseconds(-1); d.getTime()", "var d = new Date(-8.64e15); d.setUTCDate(1); d.getTime()");
add("new Date(8.64e15).valueOf()", "new Date(-0).valueOf()", "Object.is(new Date(-0).getTime(), 0)", "Object.is(new Date(-0.5).getTime(), 0)", "new Date(NaN).valueOf()");
add("new Date(8.64e15 + 1).getTime()", "new Date(-8.64e15 - 1).getTime()", "new Date(8.64e15).getTime()", "new Date(1.9).getTime()", "new Date(-1.9).getTime()");

// 5. Anos 0 a 99 no construtor com vários argumentos e em Date.UTC.
for (const year of [-1, 0, 1, 49, 50, 69, 70, 99, 100, 101, 0.5, 99.9, -0.5, 100.1, 1899, 1900, 1901]) {
  add(`new Date(${year}, 0).getFullYear()`, `new Date(${year}, 0, 1).getFullYear()`, `new Date(${year}, 11, 31, 23, 59, 59, 999).toISOString()`);
  add(`Date.UTC(${year}, 0)`, `Date.UTC(${year})`, `Date.UTC(${year}, 11, 31, 23, 59, 59, 999)`);
  add(`new Date(${year}).getTime()`);
}
add("Date.UTC()", "Date.UTC(undefined)", "Date.UTC(2024)", "Date.UTC(2024, undefined)", "Date.UTC(NaN)", "Date.UTC(2024, 0, undefined)", "Date.UTC(2024, 0, 1, undefined)");
add("Date.UTC(2024, 0, 1, 0, 0, 0, 0, 99)", "Date.UTC('2024')", "Date.UTC(2024, '0', '1')", "Date.UTC(2024, null)", "Date.UTC(2024, true)", "Date.UTC([2024])", "Date.UTC({})");
add("Date.UTC(275760, 8, 13)", "Date.UTC(275760, 8, 13, 0, 0, 0, 1)", "Date.UTC(-271821, 3, 20)", "Date.UTC(-271821, 3, 19, 23, 59, 59, 999)", "Date.UTC(1e20)", "Date.UTC(2024, 1e20)");
add("Date.UTC(2024, 0, 1, 0, 0, 0, 0.9)", "Date.UTC(2024, 0, 1, 0, 0, 0, -0.9)", "Object.is(Date.UTC(1970, 0, 1, 0, 0, 0, -0.5), 0)", "Date.UTC(Infinity)", "Date.UTC(2024, Infinity)");
add("Date.UTC(2024, 0, 1, { valueOf() { return 5; } })", "var n = []; Date.UTC({ valueOf() { n.push('y'); return 2024; } }, { valueOf() { n.push('m'); return 1; } }); n.join()");
add("var n = []; Date.UTC({ valueOf() { n.push('y'); return NaN; } }, { valueOf() { n.push('m'); return 1; } }); n.join()");
add("var n = []; new Date({ valueOf() { n.push('y'); return NaN; } }, { valueOf() { n.push('m'); return 1; } }); n.join()");
add("new Date(2024, 0, 1, 0, 0, 0, 0, 99).getTime() === new Date(2024, 0, 1).getTime()", "new Date(undefined).getTime()", "new Date(null).getTime()", "new Date(2024, undefined).getTime()");

// 6. Getters em datas inválidas e nos limites.
const getters = [
  "getTime", "getFullYear", "getMonth", "getDate", "getDay", "getHours", "getMinutes", "getSeconds", "getMilliseconds", "getUTCFullYear",
  "getUTCMonth", "getUTCDate", "getUTCDay", "getUTCHours", "getUTCMinutes", "getUTCSeconds", "getUTCMilliseconds", "getTimezoneOffset", "getYear", "valueOf",
];
for (const name of getters) {
  for (const arg of ["NaN", "8.64e15", "-8.64e15", "-1", "0.5", "-0.5", "-62198755200000", "-62167219200001", "253402300800000", "8.64e15 - 1"]) {
    add(`new Date(${arg}).${name}()`);
  }
  add(`try { Date.prototype.${name}.call({}) } catch (e) { e.name + ': ' + e.message }`, `try { Date.prototype.${name}.call(1) } catch (e) { e.name }`);
  add(`Date.prototype.${name}.length + '|' + Date.prototype.${name}.name`);
}

// 7. getTimezoneOffset em transições (Sao_Paulo 2018, New_York 2024) e em torno delas, em cada fuso.
for (const ms of [1541386800000, 1541386799999, 1541383200000, 1541383199999, 1519613999999, 1519614000000, 1519610400000, 1541300400000]) {
  add(`new Date(${ms}).getTimezoneOffset()`, `new Date(${ms}).getHours()`, `new Date(${ms}).toString()`);
}
// Sao_Paulo 2018: DST começa 4 nov 00:00 (vira 01:00), termina 17 fev 2019 23:59 (volta a 23:00 do dia 16).
for (const [y, m, d, h, mi] of [[2018, 10, 3, 23, 59], [2018, 10, 4, 0, 0], [2018, 10, 4, 0, 30], [2018, 10, 4, 1, 0], [2019, 1, 16, 22, 59], [2019, 1, 16, 23, 0], [2019, 1, 16, 23, 59], [2019, 1, 17, 0, 0], [2019, 1, 17, 0, 30]]) {
  add(`new Date(${y}, ${m}, ${d}, ${h}, ${mi}).getTimezoneOffset()`, `new Date(${y}, ${m}, ${d}, ${h}, ${mi}).toString()`, `new Date(${y}, ${m}, ${d}, ${h}, ${mi}).getHours()`);
}
// New_York 2024: 10 mar 02:00 vira 03:00, 3 nov 02:00 volta a 01:00.
for (const [y, m, d, h, mi] of [[2024, 2, 10, 1, 59], [2024, 2, 10, 2, 0], [2024, 2, 10, 2, 30], [2024, 2, 10, 3, 0], [2024, 10, 3, 0, 59], [2024, 10, 3, 1, 0], [2024, 10, 3, 1, 59], [2024, 10, 3, 2, 0]]) {
  add(`new Date(${y}, ${m}, ${d}, ${h}, ${mi}).getTimezoneOffset()`, `new Date(${y}, ${m}, ${d}, ${h}, ${mi}).toString()`);
}
for (const iso of ["2024-03-10T06:59:59Z", "2024-03-10T07:00:00Z", "2024-11-03T05:59:59Z", "2024-11-03T06:00:00Z", "2018-11-04T02:59:59Z", "2018-11-04T03:00:00Z", "2019-02-17T01:59:59Z", "2019-02-17T02:00:00Z"]) {
  add(`new Date('${iso}').getTimezoneOffset()`, `new Date('${iso}').toString()`, `new Date('${iso}').getHours()`);
}
// Setters que atravessam essas transições.
for (const [dateArgs, call] of [
  ["2018, 10, 3, 23, 30", "setHours(0, 30)"], ["2018, 10, 3, 23, 30", "setMinutes(90)"], ["2018, 10, 4, 12", "setHours(0)"], ["2018, 10, 4, 12", "setHours(0, 59, 59, 999)"],
  ["2019, 1, 16, 12", "setHours(23, 30)"], ["2019, 1, 16, 12", "setHours(24)"], ["2019, 1, 16, 12", "setDate(17)"], ["2018, 10, 3, 12", "setDate(4)"],
  ["2024, 2, 9, 12", "setDate(10)"], ["2024, 2, 10, 12", "setHours(2, 30)"], ["2024, 2, 10, 12", "setHours(1, 59, 59, 999)"], ["2024, 10, 3, 12", "setHours(1, 30)"],
  ["2024, 10, 3, 12", "setHours(2)"], ["2024, 10, 2, 12", "setDate(3)"], ["2018, 10, 4, 12", "setMonth(1, 17)"], ["2018, 10, 4, 12", "setFullYear(2019, 1, 17)"],
]) {
  add(`var d = new Date(${dateArgs}); ${state("d", `d.${call}`)}`);
}

// 8. Formatadores em anos negativos, > 9999 e 0.
const years = [-271821, -100000, -10000, -1000, -100, -10, -1, 0, 1, 9, 99, 100, 999, 1000, 9999, 10000, 99999, 100000, 275760];
for (const year of years) {
  const make = `new Date(Date.UTC(${year}, 5, 15, 12, 30, 45, 123))`;
  const makeLocal = `new Date(${year}, 5, 15, 12, 30, 45, 123)`;
  for (const fmt of ["toString", "toDateString", "toTimeString", "toUTCString", "toISOString", "toJSON", "toGMTString"]) {
    add(`${make}.${fmt}()`);
  }
  add(`${makeLocal}.toString()`, `${makeLocal}.toDateString()`, `${makeLocal}.getYear()`, `${makeLocal}.getFullYear()`);
  add(`${make}.toJSON() === ${make}.toISOString()`, `JSON.stringify({ d: ${make} })`);
}
for (const ms of ["8.64e15", "-8.64e15", "-62198755200000", "-62167219200000", "-62167219200001", "253402300799999", "253402300800000", "-1", "0"]) {
  for (const fmt of ["toString", "toDateString", "toTimeString", "toUTCString", "toISOString", "toJSON"]) add(`new Date(${ms}).${fmt}()`);
}
add("new Date(NaN).toString()", "new Date(NaN).toDateString()", "new Date(NaN).toTimeString()", "new Date(NaN).toUTCString()", "new Date(NaN).toJSON()");
add("try { new Date(NaN).toISOString() } catch (e) { e.name + ': ' + e.message }");
for (const fmt of ["toString", "toDateString", "toTimeString", "toUTCString", "toISOString", "toJSON", "toGMTString", "toLocaleString", "toLocaleDateString", "toLocaleTimeString"]) {
  add(`try { Date.prototype.${fmt}.call({}) } catch (e) { e.name + ': ' + e.message }`, `try { Date.prototype.${fmt}.call(5) } catch (e) { e.name }`);
  add(`Date.prototype.${fmt}.length + '|' + Date.prototype.${fmt}.name`);
}
// toJSON é genérico.
add("Date.prototype.toJSON.call({ toISOString() { return 'x'; } })", "Date.prototype.toJSON.call({ valueOf() { return NaN; }, toISOString() { return 'x'; } })");
add("Date.prototype.toJSON.call({ valueOf() { return Infinity; }, toISOString() { return 'x'; } })", "Date.prototype.toJSON.call({ valueOf() { return 1; }, toISOString() { return 'ok'; } })");
add("try { Date.prototype.toJSON.call({ valueOf() { return 1; } }) } catch (e) { e.name }", "Date.prototype.toJSON.call({ valueOf() { return 1; }, toISOString: 5 })");
add("try { Date.prototype.toJSON.call({ valueOf() { return 1; }, toISOString: 5 }) } catch (e) { e.name }");
add("Date.prototype.toJSON.call({ valueOf() { return '1'; }, toISOString() { return 'str'; } })", "Date.prototype.toJSON.call({ toISOString() { return 'x'; }, valueOf() { return {}; }, toString() { return 'a'; } })");
add("Date.prototype.toJSON.call(1)", "try { Date.prototype.toJSON.call(null) } catch (e) { e.name }", "try { Date.prototype.toJSON.call(undefined) } catch (e) { e.name }");

// 9. Symbol.toPrimitive com hints, Date() sem new, constructor e subclasses.
const toPrim = Symbol;
for (const hint of ["'number'", "'string'", "'default'", "'x'", "''", "undefined", "null", "1", "Symbol.iterator", "{}", ""]) {
  add(`try { new Date(0)[Symbol.toPrimitive](${hint}) } catch (e) { e.name + ': ' + e.message }`);
}
add("try { Date.prototype[Symbol.toPrimitive].call({ valueOf() { return 7; }, toString() { return 's'; } }, 'number') } catch (e) { e.name }");
add("Date.prototype[Symbol.toPrimitive].call({ valueOf() { return 7; }, toString() { return 's'; } }, 'number')", "Date.prototype[Symbol.toPrimitive].call({ valueOf() { return 7; }, toString() { return 's'; } }, 'string')");
add("Date.prototype[Symbol.toPrimitive].call({ valueOf() { return 7; }, toString() { return 's'; } }, 'default')");
add("Date.prototype[Symbol.toPrimitive].call({ valueOf() { return {}; }, toString() { return 's'; } }, 'number')");
add("try { Date.prototype[Symbol.toPrimitive].call({ valueOf() { return {}; }, toString() { return {}; } }, 'number') } catch (e) { e.name + ': ' + e.message }");
add("try { Date.prototype[Symbol.toPrimitive].call(1, 'number') } catch (e) { e.name + ': ' + e.message }", "try { Date.prototype[Symbol.toPrimitive].call(undefined, 'number') } catch (e) { e.name }");
add("Date.prototype[Symbol.toPrimitive].length", "Date.prototype[Symbol.toPrimitive].name", "Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).writable", "Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).configurable", "Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).enumerable");
add("new Date(5) + 1", "new Date(5) - 1", "+new Date(5)", "`${new Date(0)}` === new Date(0).toString()", "new Date(5) == new Date(5).toString()", "new Date(5) == 5", "new Date(5) > 4", "[new Date(5)] + ''.length > 0");
add("var d = new Date(0); d.valueOf = () => 42; d + 1", "var d = new Date(0); d.valueOf = () => 42; +d", "var d = new Date(0); d.toString = () => 'x'; d + 1", "var d = new Date(0); d.toString = () => 'x'; `${d}`");
add("var d = new Date(0); d[Symbol.toPrimitive] = undefined; d + 1 === d.toString() + 1", "var d = new Date(0); d[Symbol.toPrimitive] = () => 3; d + 1", "var d = new Date(0); d[Symbol.toPrimitive] = null; typeof (d + 1)");
add("var d = new Date(0); d[Symbol.toPrimitive] = (h) => h; [d + '', +d, `${d}`].join()", "var d = new Date(0); d[Symbol.toPrimitive] = (h) => h; d == 'default'");
add("typeof Date()", "Date().length > 20", "Date(0) === Date()", "typeof Date(1, 2, 3)", "Date(NaN).length > 20", "typeof Date.now()", "Date.now.length", "Date.now() > 1.7e12", "Number.isInteger(Date.now())");
add("Date.prototype.constructor === Date", "Object.getOwnPropertyDescriptor(Date.prototype, 'constructor').enumerable", "Object.getOwnPropertyDescriptor(Date.prototype, 'constructor').writable", "Object.getOwnPropertyDescriptor(Date.prototype, 'constructor').configurable");
add("Object.prototype.toString.call(Date.prototype)", "typeof Date.prototype.getTime", "try { Date.prototype.getTime() } catch (e) { e.name + ': ' + e.message }", "try { Date.prototype.toString() } catch (e) { e.name + ': ' + e.message }");
add("Object.getPrototypeOf(Date.prototype) === Object.prototype", "Date.prototype.toString.call(new Date(NaN))", "Date.name", "Date.length", "Date.prototype.toGMTString === Date.prototype.toUTCString");
add("class D extends Date {} new D(0).getTime()", "class D extends Date {} new D(0) instanceof D", "class D extends Date {} Object.prototype.toString.call(new D(0))");
add("class D extends Date { constructor() { super(2024, 0, 1); } } new D().getFullYear()", "class D extends Date { get x() { return this.getTime(); } } new D(7).x");
add("class D extends Date {} new D(NaN).toString()", "class D extends Date {} String(new D(0)) === String(new Date(0))", "class D extends Date {} new D(0).constructor === D", "class D extends Date {} D.UTC === Date.UTC");
add("class D extends Date {} D.now === Date.now", "class D extends Date {} Object.getPrototypeOf(D) === Date", "class D extends Date {} var d = new D(0); d.setFullYear(2000); d.getFullYear()");
add("class D extends Date { setHours(h) { return super.setHours(h + 1); } } var d = new D(2024, 0, 1); d.setHours(1); d.getHours()");
add("function F() {} F.prototype = Date.prototype; try { new F().getTime() } catch (e) { e.name }", "Reflect.construct(Date, [0], Object).getTime ? 1 : 0", "Reflect.construct(Date, [0], Object) instanceof Date");
add("Reflect.construct(Date, [5], Array).constructor === Array", "Object.prototype.toString.call(Reflect.construct(Date, [5], Array))", "Reflect.construct(Date, [5], Array).getTime === undefined");
add("try { Date.call(new Date(0)) === String(new Date(0)) } catch (e) { e.name }", "new Date(new Date(5)).getTime()", "new Date(new Date(NaN)).getTime()", "new Date(Object(5)).getTime()", "new Date(Object('2024')).getFullYear()");
add("new Date({ valueOf() { return 5; } }).getTime()", "new Date({ toString() { return '2024-03-10T00:00:00Z'; }, valueOf: undefined }).getTime()", "new Date({ [Symbol.toPrimitive]() { return 9; } }).getTime()", "new Date({ [Symbol.toPrimitive]() { return '2000-01-01T00:00:00Z'; } }).getTime()");
add("new Date([2024]).getTime()", "new Date([]).getTime()", "new Date(true).getTime()", "new Date(false).getTime()", "new Date('').getTime()", "new Date(' ').getTime()", "new Date(1n === 1n ? 1 : 0).getTime()");
add("try { new Date(1n) } catch (e) { e.name + ': ' + e.message }", "try { new Date(Symbol()) } catch (e) { e.name + ': ' + e.message }", "try { new Date(2024, 0, 1n) } catch (e) { e.name }", "try { Date.UTC(1n) } catch (e) { e.name }");
add("typeof structuredClone", "typeof new Date(0).toTemporalInstant", "typeof Date.prototype.toLocaleString", "Object.getOwnPropertyNames(Date.prototype).length");
add("Object.getOwnPropertyNames(Date.prototype).sort().join()", "Object.getOwnPropertyNames(Date).sort().join()", "Object.getOwnPropertySymbols(Date.prototype).length");

// 10. toString e toGMTString nos fusos, setYear/getYear e toJSON em Date.now tipo.
for (const ms of ["0", "-1", "1710054000000", "1541386800000", "-2208988800000", "8.64e15", "-8.64e15"]) {
  add(`new Date(${ms}).toString().length`, `new Date(${ms}).toTimeString()`, `new Date(${ms}).toDateString()`, `new Date(${ms}).toUTCString()`);
  add(`new Date(${ms}).getYear()`, `var d = new Date(${ms}); d.setYear(d.getYear()); d.getTime() === new Date(${ms}).getTime()`);
}
for (const arg of ["0", "50", "99", "100", "1900", "1999", "2000", "2024", "-1", "NaN", "'99'", "99.9", "-0.5", "275760", "1e9"]) {
  add(`var d = new Date(2024, 5, 15, 12); ${state("d", `d.setYear(${arg})`)}`, `var d = new Date(NaN); ${state("d", `d.setYear(${arg})`)}`);
}
add("Date.prototype.setYear.length", "Date.prototype.getYear.length", "Date.prototype.setYear.name");

// 11. Composição: encadeamento e arredondamento de milissegundos fracionários.
for (const [name] of setters) {
  add(`var d = new Date(2024, 0, 31, 12, 30, 15, 500); d.${name}(1.5); d.getTime()`, `var d = new Date(2024, 0, 31, 12, 30, 15, 500); d.${name}(-1.5); d.getTime()`);
  add(`var d = new Date(2024, 0, 31, 12, 30, 15, 500); d.${name}(0.9999); d.getTime()`, `var d = new Date(2024, 0, 31, 12, 30, 15, 500); d.${name}(-0.0001); d.getTime()`);
}
add("var d = new Date(2024, 0, 31); d.setMonth(1); d.getDate()", "var d = new Date(2024, 0, 31); d.setMonth(1, 31); d.toISOString()", "var d = new Date(2023, 11, 31); d.setMonth(12); d.toISOString()");
add("var d = new Date(2024, 1, 29); d.setFullYear(2023); d.toISOString()", "var d = new Date(2024, 1, 29); d.setFullYear(2025, 1, 29); d.toISOString()", "var d = new Date(2024, 1, 29); d.setFullYear(2023, 1, 28); d.toISOString()");
add("var d = new Date(2024, 0, 1); d.setDate(0); d.toISOString()", "var d = new Date(2024, 0, 1); d.setDate(-1); d.toISOString()", "var d = new Date(2024, 0, 1); d.setDate(1e9); d.toISOString()");
add("var d = new Date(2024, 0, 1); d.setHours(1e9); d.getTime()", "var d = new Date(2024, 0, 1); d.setHours(2 ** 53); d.getTime()", "var d = new Date(2024, 0, 1); d.setUTCHours(2 ** 53); d.getTime()");
add("var d = new Date(2024, 0, 1); d.setMonth(1e9); d.getTime()", "var d = new Date(2024, 0, 1); d.setFullYear(1e9); d.getTime()", "var d = new Date(2024, 0, 1); d.setFullYear(275760, 8, 13); d.getTime()");
add("var d = new Date(0); d.setUTCFullYear(275760, 8, 13); d.setUTCHours(0, 0, 0, 1); d.getTime()", "var d = new Date(0); d.setUTCFullYear(275760, 8, 13); d.getTime()", "var d = new Date(0); d.setUTCFullYear(-271821, 3, 20); d.getTime()");
add("var d = new Date(0); d.setUTCFullYear(-271821, 3, 19); d.getTime()", "var d = new Date(0); d.setUTCFullYear(-271821, 3, 20); d.setUTCMilliseconds(-1); d.getTime()");

if (process.argv[2] === "child") {
  const fs = require("fs");
  const path = require("path");
  const harness = eval(fs.readFileSync(path.join(__dirname, "../tests/golden/date_bun_harness.js"), "utf8").trim());
  for (const source of new Set(programs)) {
    if (/[\t\n\r]/.test(source)) throw new Error("programa com tabulação ou quebra de linha: " + source);
    emitRow(`${process.env.TZ}\t${source}\t${harness(source)}`);
  }
} else {
  for (const zone of ZONES) {
    const child = spawnSync(process.execPath, [__filename, "child"], {
      env: { ...process.env, TZ: zone },
      encoding: "utf8",
      maxBuffer: 1 << 28,
      timeout: 120000,
    });
    if (child.status !== 0) throw new Error(`fuso ${zone}: ${child.stderr || child.error}`);
    process.stdout.write(child.stdout);
  }
  console.error(`${new Set(programs).size} programas, ${ZONES.length} fusos`);
}
