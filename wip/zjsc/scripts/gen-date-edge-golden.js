// Gera tests/golden/date_edge_bun.tsv: casos de borda de Date medidos no bun 1.4.2. Setters encadeados com
// overflow, getTimezoneOffset em transições de fusos reais, toLocale*String com opções em cinco locales,
// Symbol.toPrimitive, Date.UTC com anos 0..99, valores extremos (+-8.64e15), toISOString/toJSON de data
// inválida, valueOf de objetos e comparação de datas.
// Colunas: fuso, fonte do programa (uma linha), KIND e REPR (serializador de tests/golden/date_bun_harness.js).
// Cada programa roda em cada fuso de ZONES (TZ por processo). Se o resultado é igual em todos, sai com fuso "any".
// Uso: bun scripts/gen-date-edge-golden.js > tests/golden/date_edge_bun.tsv
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const ZONES = ["UTC", "America/Sao_Paulo", "Europe/London", "Asia/Kolkata", "Australia/Lord_Howe", "Pacific/Apia"];
const programs = [];
const add = (...sources) => programs.push(...sources);

// Setters encadeados com overflow.
const base = "new Date(2020, 0, 31, 12, 30, 15, 500)";
for (const month of [-13, -1, 0, 1, 12, 13, 25]) add(`var d = ${base}; d.setMonth(${month}); d.toISOString()`);
for (const day of [-31, -1, 0, 1, 29, 30, 32, 61, 366]) add(`var d = new Date(2021, 1, 15, 8); d.setDate(${day}); d.toISOString()`);
for (const [hours, minutes] of [[0, 90], [23, 61], [24, 0], [25, 120], [-1, -1], [48, 1440], [12, -750], [100, 0]]) {
  add(`var d = new Date(2020, 5, 15, 1, 2, 3); d.setHours(${hours}, ${minutes}); d.toISOString()`);
  add(`var d = new Date(2020, 5, 15, 1, 2, 3); d.setHours(${hours}, ${minutes}, 3600, 1500); d.getTime()`);
}
for (const seconds of [-1, 60, 3600, 86400, 1e9]) add(`var d = new Date(2020, 0, 1); d.setSeconds(${seconds}); d.toISOString()`);
for (const ms of [-1, 1000, 86400000, 1e12]) add(`var d = new Date(2020, 0, 1); d.setMilliseconds(${ms}); d.toISOString()`);
add(
  "var d = new Date(2020, 0, 31); d.setMonth(1); d.setDate(0); d.toISOString()",
  "var d = new Date(2020, 0, 31); d.setMonth(1, 30); d.toISOString()",
  "var d = new Date(2020, 11, 31); d.setMonth(12); d.setMonth(-1); d.toISOString()",
  "var d = new Date(2020, 1, 29); d.setFullYear(2021); d.toISOString()",
  "var d = new Date(2020, 1, 29); d.setFullYear(2021, 1, 29); d.toISOString()",
  "var d = new Date(2020, 1, 29); d.setFullYear(2021, 13, 0); d.toISOString()",
  "var d = new Date(NaN); d.setFullYear(2020); d.toISOString()",
  "var d = new Date(NaN); d.setMonth(1); String(d.getTime())",
  "var d = new Date(NaN); d.setHours(1); String(d.getTime())",
  "var d = new Date(NaN); d.setUTCFullYear(2020, 5, 5); d.toISOString()",
  "var d = new Date(0); d.setMonth(); String(d.getTime())",
  "var d = new Date(0); d.setHours(NaN, 1); String(d.getTime())",
  "var d = new Date(0); d.setDate(Infinity); String(d.getTime())",
  "var d = new Date(0); d.setTime(8.64e15 + 1); String(d.getTime())",
  "var d = new Date(0); d.setTime('12'); d.getTime()",
  "var d = new Date(0); d.setUTCHours(25, 61, 61, 1001); d.toISOString()",
  "var d = new Date(2020, 0, 31); d.setUTCMonth(1); d.toISOString()",
  "var d = new Date(0); d.setMinutes(1.9); d.getTime()",
  "var d = new Date(0); d.setMinutes(-1.9); d.getTime()",
  "var d = new Date(0); d.setSeconds(1, 2, 3); d.getTime()",
  "new Date(0).setMonth(13)", "new Date(0).setDate(0)", "new Date(0).setYear(99)", "new Date(0).setYear(100)", "new Date(0).setYear(NaN)",
  "var d = new Date(2000, 0, 1); d.setYear(99); d.getFullYear()", "var d = new Date(2000, 0, 1); d.setYear(2005); d.getFullYear()",
  "var d = new Date(0); d.setMonth(1).toString().length > 0", "typeof new Date(0).setHours(1)",
);

// getTimezoneOffset em transições: instantes UTC ao redor das mudanças de offset.
const transitions = [
  // Sao_Paulo: DST de 2018-11-04 03:00Z, fim em 2019-02-17 02:00Z
  Date.UTC(2018, 10, 4, 2, 59, 59), Date.UTC(2018, 10, 4, 3, 0, 0), Date.UTC(2019, 1, 17, 1, 59, 59), Date.UTC(2019, 1, 17, 2, 0, 0),
  // Londres: 2021-03-28 01:00Z e 2021-10-31 01:00Z
  Date.UTC(2021, 2, 28, 0, 59, 59), Date.UTC(2021, 2, 28, 1, 0, 0), Date.UTC(2021, 9, 31, 0, 59, 59), Date.UTC(2021, 9, 31, 1, 0, 0),
  // Lord Howe (30 min): 2021-04-03 15:00Z e 2021-10-02 15:00Z
  Date.UTC(2021, 3, 3, 14, 59, 59), Date.UTC(2021, 3, 3, 15, 0, 0), Date.UTC(2021, 9, 2, 14, 59, 59), Date.UTC(2021, 9, 2, 15, 0, 0),
  // Apia: virada da linha de data em 2011-12-30 10:00Z e fim do DST em 2021-04-03 11:00Z
  Date.UTC(2011, 11, 30, 9, 59, 59), Date.UTC(2011, 11, 30, 10, 0, 0), Date.UTC(2021, 3, 3, 10, 59, 59), Date.UTC(2021, 3, 3, 11, 0, 0),
  // Kolkata: sem DST hoje, 1945 e 1941-1942 tinham
  Date.UTC(1945, 9, 14, 0, 0, 0), Date.UTC(1990, 0, 1), Date.UTC(2020, 5, 1),
  // Datas distantes e de antes de 1900 (LMT)
  Date.UTC(1800, 0, 1), Date.UTC(1900, 0, 1), Date.UTC(1969, 11, 31, 23, 59, 59), Date.UTC(2037, 11, 31), Date.UTC(2100, 6, 1), Date.UTC(3000, 0, 1),
];
for (const instant of transitions) {
  add(
    `new Date(${instant}).getTimezoneOffset()`,
    `new Date(${instant}).toString()`,
    `new Date(${instant}).getHours() + ':' + new Date(${instant}).getMinutes()`,
  );
}
// Hora local inexistente (salto de verão) e ambígua (volta), construídas por componentes locais.
add(
  "new Date(2018, 10, 4, 0, 30).toISOString()", "new Date(2018, 10, 4, 1, 0).toISOString()", "new Date(2019, 1, 16, 23, 30).toISOString()",
  "new Date(2021, 2, 28, 1, 30).toISOString()", "new Date(2021, 9, 31, 1, 30).toISOString()", "new Date(2021, 3, 4, 2, 15).toISOString()",
  "new Date(2021, 9, 3, 2, 15).toISOString()", "new Date(2011, 11, 30, 12).toISOString()", "new Date(2011, 11, 31, 12).toISOString()",
  "var d = new Date(2021, 2, 28, 0, 30); d.setHours(1, 30); d.toISOString()", "var d = new Date(2021, 9, 31, 0, 30); d.setHours(d.getHours() + 1); d.toISOString()",
  "var d = new Date(2018, 10, 3, 23, 59); d.setMinutes(d.getMinutes() + 1); d.toISOString()", "var d = new Date(2021, 2, 27, 1, 30); d.setDate(28); d.toISOString()",
  "new Date(2021, 2, 28, 1, 30).getTimezoneOffset()", "new Date(2021, 9, 31, 1, 30).getTimezoneOffset()", "new Date(2018, 10, 4, 0, 30).getTimezoneOffset()",
  "new Date('2021-03-28T01:30:00').toISOString()", "new Date('2021-10-31T01:30:00').toISOString()", "new Date('2018-11-04T00:30:00').toISOString()",
  "new Date(1941, 5, 1).getTimezoneOffset()", "new Date(1995, 0, 1).getTimezoneOffset()", "new Date(1995, 6, 1).getTimezoneOffset()", "new Date(2020, 0, 1).getTimezoneOffset()", "new Date(2020, 6, 1).getTimezoneOffset()",
  "new Date(NaN).getTimezoneOffset()", "String(new Date(NaN).getTimezoneOffset())",
);

// toLocale*String com opções em 5 locales.
const locales = ["en-US", "pt-BR", "de-DE", "ja-JP", "ar-EG"];
const instants = [0, 1609459200000, 1700000000123];
const optionSets = [
  "{}", "{ timeZone: 'UTC' }", "{ timeZone: 'America/Sao_Paulo', hour12: false }", "{ dateStyle: 'full', timeZone: 'UTC' }", "{ dateStyle: 'short', timeStyle: 'short', timeZone: 'UTC' }",
  "{ year: 'numeric', month: 'long', day: 'numeric', timeZone: 'UTC' }", "{ weekday: 'long', timeZone: 'UTC' }", "{ month: 'short', timeZone: 'UTC' }",
  "{ hour: '2-digit', minute: '2-digit', timeZone: 'Asia/Kolkata' }", "{ timeZone: 'Pacific/Apia', timeZoneName: 'short' }", "{ era: 'short', year: 'numeric', timeZone: 'UTC' }",
  "{ hourCycle: 'h23', hour: 'numeric', timeZone: 'UTC' }",
];
for (const locale of locales) {
  for (const options of optionSets) add(`new Date(${instants[locale.length % 3]}).toLocaleString('${locale}', ${options})`);
  for (const instant of instants) {
    add(
      `new Date(${instant}).toLocaleDateString('${locale}', { timeZone: 'UTC' })`,
      `new Date(${instant}).toLocaleTimeString('${locale}', { timeZone: 'UTC' })`,
      `new Date(${instant}).toLocaleDateString('${locale}', { dateStyle: 'medium', timeZone: 'Europe/London' })`,
    );
  }
  add(`new Date(NaN).toLocaleString('${locale}')`, `new Date(0).toLocaleDateString('${locale}', { timeZone: 'Nowhere/Land' })`);
}
add(
  "new Date(0).toLocaleString('xx-invalid-locale-zz')", "new Date(0).toLocaleDateString(undefined, { timeZone: 'UTC' })", "new Date(0).toLocaleString(['pt-BR', 'en-US'], { timeZone: 'UTC' })",
  "new Date(0).toLocaleString('en-US', { dateStyle: 'full', year: 'numeric' })", "new Date(0).toLocaleTimeString('en-US', { dateStyle: 'short' })", "new Date(0).toLocaleDateString('en-US', { timeStyle: 'short' })",
  "new Date(8.64e15).toLocaleString('en-US', { timeZone: 'UTC' })", "new Date(-8.64e15).toLocaleString('en-US', { timeZone: 'UTC' })", "new Date(0).toLocaleString('en-US', { hour12: true, hourCycle: 'h23', timeZone: 'UTC' })",
);

// Symbol.toPrimitive e conversões.
add(
  "typeof Date.prototype[Symbol.toPrimitive]", "Date.prototype[Symbol.toPrimitive].length", "Date.prototype[Symbol.toPrimitive].name",
  "new Date(0)[Symbol.toPrimitive]('number')", "new Date(0)[Symbol.toPrimitive]('string').length > 0", "new Date(0)[Symbol.toPrimitive]('default') === new Date(0).toString()",
  "new Date(0)[Symbol.toPrimitive]('bogus')", "new Date(0)[Symbol.toPrimitive]()", "new Date(0)[Symbol.toPrimitive](1)", "Date.prototype[Symbol.toPrimitive].call({}, 'number')",
  "Date.prototype[Symbol.toPrimitive].call({ valueOf() { return 7 }, toString() { return 's' } }, 'number')", "Date.prototype[Symbol.toPrimitive].call({ valueOf() { return 7 }, toString() { return 's' } }, 'default')",
  "Date.prototype[Symbol.toPrimitive].call(1, 'number')", "Date.prototype[Symbol.toPrimitive].call(undefined, 'number')",
  "new Date(5) + 1 === new Date(5).toString() + '1'", "new Date(5) - 1", "+new Date(5)", "`${new Date(NaN)}`", "new Date(5) * 2", "[new Date(5)] + ''.length",
  "new Date(5) == new Date(5).toString()", "new Date(5) == 5", "new Date(5) < 6", "new Date(5) > new Date(4)",
  "var d = new Date(5); d[Symbol.toPrimitive] = () => 9; d + 1", "var d = new Date(5); d[Symbol.toPrimitive] = undefined; d - 1", "var d = new Date(5); d[Symbol.toPrimitive] = null; d + ''.length",
  "var d = new Date(5); d[Symbol.toPrimitive] = 3; d + 1", "var d = new Date(5); d.valueOf = () => 99; d - 0", "var d = new Date(5); d.toString = () => 'x'; d + 1",
  "var d = new Date(5); d[Symbol.toPrimitive] = () => ({}); d + 1", "JSON.stringify(Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive))",
  "Object.prototype.toString.call(new Date(0))", "Object.prototype.toString.call(Date.prototype)", "String(Date.prototype)", "Date.prototype.getTime.call(Date.prototype)",
);

// Date.UTC e construtor com anos 0..99.
for (const year of [0, 1, 49, 50, 99, 100, -1, 99.9, 0.5, -0.5, 1900, 1899]) {
  add(`Date.UTC(${year})`, `Date.UTC(${year}, 0, 1)`, `Date.UTC(${year}, 11, 31, 23, 59, 59, 999)`, `new Date(${year}, 0).getFullYear()`, `new Date(${year}, 0).getTime() === new Date(1900 + Math.trunc(${year}), 0).getTime()`);
}
add(
  "Date.UTC()", "Date.UTC(NaN)", "Date.UTC(2020, NaN)", "Date.UTC(275760, 8, 13)", "Date.UTC(275760, 8, 13, 0, 0, 0, 1)", "Date.UTC(-271821, 3, 20)", "Date.UTC(-271821, 3, 19, 23, 59, 59, 999)",
  "Date.UTC(2020, 12)", "Date.UTC(2020, -1)", "Date.UTC(2020, 0, 0)", "Date.UTC(2020, 0, 32)", "Date.UTC(2020, 0, 1, 24)", "Date.UTC(2020, 0, 1, 0, 60)", "Date.UTC(2020, 0, 1, 0, 0, 60)", "Date.UTC(2020, 0, 1, 0, 0, 0, 1000)",
  "Date.UTC('2020', '1', '1')", "Date.UTC(null)", "Date.UTC(undefined, 0)", "Date.UTC(2020.9, 1.9, 1.9)", "Date.UTC(Infinity)", "Date.UTC(1e20)", "Date.UTC.length", "Date.UTC(2020, 0, 1, 0, 0, 0, 0.9)",
  "new Date(Date.UTC(99, 0, 1)).toISOString()", "new Date(Date.UTC(100, 0, 1)).toISOString()", "new Date(Date.UTC(0, 0, 1)).toISOString()", "new Date(Date.UTC(-1, 0, 1)).toISOString()",
  "new Date(99, 11, 31).getFullYear()", "new Date(0, 0, 1, 0, 0, 0, 0).getFullYear()", "new Date(100, 0).getFullYear()", "new Date(99, 0, 1).getYear()", "new Date(2000, 0, 1).getYear()", "new Date(1899, 0, 1).getYear()",
  "new Date(0, 0).toISOString()", "new Date(-1, 0).getFullYear()", "new Date(2020, 0, 1, 0, 0, 0, 0, 99).getTime() === new Date(2020, 0, 1).getTime()",
  "var d = new Date(0); d.setFullYear(50); d.getFullYear()", "var d = new Date(0); d.setUTCFullYear(99); d.toISOString()", "var d = new Date(0); d.setUTCFullYear(0); d.toISOString()", "var d = new Date(0); d.setUTCFullYear(-1); d.toISOString()",
  "new Date('0099-01-01T00:00:00Z').getTime()", "new Date('0000-01-01T00:00:00Z').toISOString()", "new Date('-000001-01-01T00:00:00Z').toISOString()", "new Date('-000000-01-01T00:00:00Z').getTime()", "new Date('+275760-09-13T00:00:00Z').getTime()", "new Date('+275760-09-13T00:00:00.001Z').getTime()",
);

// Valores extremos.
for (const value of [8.64e15, -8.64e15, 8.64e15 + 1, -8.64e15 - 1, 8.64e15 - 1, -8.64e15 + 1, 8.64e15 + 0.5, 1e300, -0, 0.9, -0.9, Infinity, -Infinity]) {
  const lit = Object.is(value, -0) ? "-0" : String(value);
  add(
    `new Date(${lit}).getTime()`, `String(new Date(${lit}).getTime())`, `new Date(${lit}).getTime() === 0`, `Object.is(new Date(${lit}).getTime(), -0)`,
    `new Date(${lit}).toUTCString()`, `new Date(${lit}).toString()`, `Date.prototype.toISOString.call(new Date(${lit}))`, `new Date(${lit}).getUTCFullYear()`, `new Date(${lit}).getFullYear()`,
    `String(new Date(${lit}).toJSON())`, `JSON.stringify({ d: new Date(${lit}) })`, `new Date(${lit}).getDay()`, `new Date(${lit}).getTimezoneOffset()`,
  );
}
add(
  "new Date(8.64e15).toISOString()", "new Date(-8.64e15).toISOString()", "new Date(8.64e15).toString()", "new Date(-8.64e15).toString()", "new Date(8.64e15).toDateString()", "new Date(-8.64e15).toTimeString()",
  "new Date(8.64e15).getMonth() + ',' + new Date(8.64e15).getDate()", "new Date(-8.64e15).getMonth() + ',' + new Date(-8.64e15).getDate()", "new Date(-8.64e15).getUTCDay()",
  "var d = new Date(8.64e15); d.setUTCMilliseconds(1); String(d.getTime())", "var d = new Date(8.64e15); d.setUTCMilliseconds(-1); d.getTime()", "var d = new Date(-8.64e15); d.setUTCMilliseconds(-1); String(d.getTime())",
  "var d = new Date(8.64e15); d.setUTCDate(d.getUTCDate() + 1); String(d.getTime())", "var d = new Date(8.64e15 - 1); d.setUTCMonth(0); d.getTime()", "var d = new Date(0); d.setUTCFullYear(275760, 8, 14); String(d.getTime())",
  "var d = new Date(0); d.setUTCFullYear(275760, 8, 13); d.getTime()", "var d = new Date(0); d.setFullYear(275760, 8, 13); String(d.getTime())", "var d = new Date(0); d.setFullYear(-271821, 3, 20); String(d.getTime())",
  "new Date(8.64e15, 0).getTime()", "new Date(275760, 8, 13).getTime()", "new Date(275760, 8, 13, 0, 0, 0, 1).getTime()", "new Date(-271821, 3, 19).getTime()", "new Date(-271821, 3, 20).getTime()",
  "new Date(8.64e15).getTime() - new Date(-8.64e15).getTime()", "Date.UTC(275760, 8, 13) === 8.64e15", "new Date(8.64e15).valueOf() === 8.64e15",
  "new Date('275760-09-13T00:00:00.000Z').getTime()", "new Date('+275760-09-13T00:00:00.000Z').toISOString()", "new Date('+275760-09-13T00:00:00.000+01:00').getTime()", "new Date('-271821-04-20T00:00:00.000Z').getTime()",
  "new Date('-271821-04-20T00:00:00.000Z').toISOString()", "new Date('-271821-04-19T23:59:59.999Z').getTime()", "new Date(2 ** 53).getTime()", "new Date(2 ** 31).getTime()", "new Date(-(2 ** 31)).getTime()", "new Date(2 ** 32).getUTCFullYear()",
);

// toISOString / toJSON de data inválida.
add(
  "new Date(NaN).toISOString()", "new Date(NaN).toJSON()", "JSON.stringify(new Date(NaN))", "JSON.stringify({ a: new Date(NaN), b: [new Date(NaN)] })", "String(new Date(NaN))", "new Date(NaN).toString()",
  "new Date(NaN).toDateString()", "new Date(NaN).toTimeString()", "new Date(NaN).toUTCString()", "new Date(NaN).toGMTString()", "String(new Date('x').getTime())", "new Date('x').getFullYear()", "new Date().setTime(NaN)",
  "Date.prototype.toJSON.call({ toISOString() { return 'z' } })", "Date.prototype.toJSON.call({ valueOf() { return NaN }, toISOString() { return 'z' } })", "Date.prototype.toJSON.call({ valueOf() { return Infinity }, toISOString() { return 'z' } })",
  "Date.prototype.toJSON.call({ valueOf() { return 1 }, toISOString() { return 'z' } })", "Date.prototype.toJSON.call({ valueOf() { return 1 } })", "Date.prototype.toJSON.call({ valueOf() { return 1 }, toISOString: 3 })",
  "Date.prototype.toJSON.call(1)", "Date.prototype.toJSON.call(undefined)", "Date.prototype.toJSON.call(null)", "Date.prototype.toJSON.call('2020')", "Date.prototype.toJSON.call({ toISOString() { return 'q' }, valueOf() { return 'abc' } })",
  "Date.prototype.toISOString.call({})", "Date.prototype.toISOString.call(1)", "Date.prototype.toISOString.call(new Date(0))", "Date.prototype.toJSON.length", "Date.prototype.toISOString.length",
  "new Date(0).toJSON()", "new Date(-1).toISOString()", "new Date(-62198755200000).toISOString()", "new Date(-62167219200001).toISOString()", "new Date(253402300800000).toISOString()", "new Date(253402300799999).toISOString()",
  "new Date(-62198755200001).toISOString()", "new Date(1e12).toISOString()", "new Date(1.9).toISOString()", "new Date(-1.9).toISOString()", "new Date(0.5).getMilliseconds()",
  "Date.parse('2020-13-01')", "Date.parse('2020-02-30')", "Date.parse('2020-02-29T24:00:00Z')", "Date.parse('2020-02-29T24:00:01Z')", "Date.parse('2020-02-29T25:00:00Z')", "Date.parse('')", "Date.parse()",
  "JSON.parse(JSON.stringify({ d: new Date(0) })).d", "new Date(JSON.parse(JSON.stringify(new Date(123456789)))).getTime()",
);

// valueOf de objetos e conversões do construtor.
add(
  "new Date({ valueOf() { return 5 } }).getTime()", "new Date({ valueOf() { return '5' } }).getTime()", "new Date({ toString() { return '2020-01-01T00:00:00Z' } }).getTime()",
  "new Date({ valueOf() { return {} }, toString() { return '1970-01-01T00:00:01Z' } }).getTime()", "new Date({ valueOf() { return {} }, toString() { return {} } }).getTime()",
  "new Date({ [Symbol.toPrimitive]() { return 42 } }).getTime()", "new Date({ [Symbol.toPrimitive](hint) { return hint } }).getTime()", "new Date({ [Symbol.toPrimitive](hint) { return 'x' + hint } }).getTime()",
  "new Date(new Date(7)).getTime()", "new Date(new Date(NaN)).getTime()", "new Date(new Number(9)).getTime()", "new Date(new String('1970-01-01T00:00:00.008Z')).getTime()", "new Date(true).getTime()", "new Date(null).getTime()", "new Date(undefined).getTime()",
  "new Date([]).getTime()", "new Date([5]).getTime()", "new Date('5').getTime()", "new Date(1n === 1n ? 3 : 0).getTime()", "(() => { try { return new Date(1n).getTime() } catch (e) { return e.name } })()", "(() => { try { return new Date(Symbol()).getTime() } catch (e) { return e.name } })()",
  "(() => { try { return new Date(0, 0, Symbol()).getTime() } catch (e) { return e.name } })()", "(() => { try { return Date.UTC(0n) } catch (e) { return e.name } })()",
  "typeof Date()", "Date(0) === Date(1)", "typeof Date.now()", "Date.now() > 1.6e12", "Date.length", "new Date(0, 0).constructor === Date", "Object.getPrototypeOf(new Date(0)) === Date.prototype",
  "var calls = []; new Date({ valueOf() { calls.push('v'); return 1 }, toString() { calls.push('s'); return '2' } }); calls.join('')",
  "var calls = []; var d = new Date(0); d.setHours({ valueOf() { calls.push('h'); return 1 } }, { valueOf() { calls.push('m'); return 2 } }); calls.join('')",
  "var calls = []; var d = new Date(NaN); d.setHours({ valueOf() { calls.push('h'); return 1 } }, { valueOf() { calls.push('m'); return 2 } }); calls.join('')",
  "var calls = []; Date.UTC({ valueOf() { calls.push('y'); return 1 } }, { valueOf() { calls.push('m'); return 1 } }, { valueOf() { calls.push('d'); return 1 } }); calls.join('')",
  "var d = new Date(0); d.setTime({ valueOf() { return 77 } }); d.getTime()", "var d = new Date(0); d.setMonth({ valueOf() { d.setTime(NaN); return 1 } }); String(d.getTime())",
  "var d = new Date(0); d.setHours({ valueOf() { d.setTime(86400000 * 10); return 1 } }); d.getTime()",
  "Date.prototype.valueOf.call({})", "Date.prototype.valueOf.call(new Date(3))", "Date.prototype.getTime.call(1)", "Date.prototype.setTime.call({}, 1)", "Date.prototype.getFullYear.call('x')",
  "new Date(0).valueOf() === new Date(0).getTime()", "typeof new Date(0).valueOf()", "Object.is(new Date(-0).valueOf(), 0)",
);

// Comparação de datas.
add(
  "new Date(1) < new Date(2)", "new Date(1) <= new Date(1)", "new Date(1) >= new Date(2)", "new Date(1) == new Date(1)", "new Date(1) === new Date(1)", "new Date(1) != new Date(1)", "+new Date(1) === +new Date(1)",
  "new Date(NaN) < new Date(1)", "new Date(NaN) > new Date(1)", "new Date(NaN) == new Date(NaN)", "+new Date(NaN) === +new Date(NaN)", "Object.is(+new Date(NaN), NaN)", "new Date(NaN) <= new Date(NaN)",
  "new Date(0) - new Date(1000)", "new Date(2020, 0, 2) - new Date(2020, 0, 1)", "new Date(2021, 2, 29) - new Date(2021, 2, 28)", "new Date(2021, 10, 1) - new Date(2021, 9, 31)", "new Date(2018, 10, 5) - new Date(2018, 10, 4)",
  "new Date(2019, 1, 18) - new Date(2019, 1, 17)", "new Date(2011, 11, 31) - new Date(2011, 11, 30)", "new Date(2021, 3, 5) - new Date(2021, 3, 4)", "new Date(2021, 9, 3) - new Date(2021, 9, 2)",
  "[new Date(3), new Date(1), new Date(2)].sort((a, b) => a - b).map(Number).join()", "[new Date(3), new Date(NaN), new Date(1)].sort((a, b) => a - b).map(Number).join()", "Math.max(new Date(3), new Date(5))", "Math.min(new Date(3), new Date(NaN))",
  "new Date(0) > null", "new Date(0) >= null", "new Date(0) > undefined", "new Date(0) < '1'", "new Date(1) > '0'", "new Date(0) + new Date(0) === new Date(0).toString() + new Date(0).toString()",
  "new Date(2020, 0, 1).getTime() === new Date('2020-01-01T00:00:00').getTime()", "new Date('2020-01-01').getTime() === Date.UTC(2020, 0, 1)", "new Date('2020-01-01T00:00').getTime() === new Date(2020, 0, 1).getTime()",
  "new Date(2020, 0, 1, 12).getDay()", "new Date(2020, 0, 1).getUTCDate()", "new Date(2020, 0, 1).getUTCHours()", "new Date(2020, 0, 1).getUTCDay()", "new Date(2021, 2, 28, 12).getUTCHours()", "new Date(2021, 9, 31, 12).getUTCHours()",
  "new Date(Date.UTC(2020, 0, 1)).getDate()", "new Date(Date.UTC(2020, 0, 1)).getHours()", "new Date(Date.UTC(2020, 0, 1)).getMinutes()", "new Date(Date.UTC(2020, 0, 1)).getDay()", "new Date(Date.UTC(2020, 5, 30, 23, 59)).getDate()",
  "new Date(Date.UTC(2011, 11, 30, 10)).getDate()", "new Date(Date.UTC(2011, 11, 30, 9)).getDate()", "new Date(Date.UTC(2011, 11, 31)).getDate()", "new Date(Date.UTC(2021, 3, 3, 14, 30)).getMinutes()", "new Date(Date.UTC(2021, 9, 2, 15, 30)).getMinutes()",
  "new Date(Date.UTC(2021, 6, 1)).getMinutes()", "new Date(Date.UTC(2021, 6, 1)).getHours()", "new Date(Date.UTC(1970, 0, 1)).getTimezoneOffset()", "new Date(Date.UTC(1900, 0, 1)).getTimezoneOffset()",
  "new Date(0).toDateString()", "new Date(0).toTimeString()", "new Date(0).toString()", "new Date(0).toLocaleString()", "new Date(0).toLocaleDateString()", "new Date(0).toLocaleTimeString()", "new Date(1e12).toString()", "new Date(-1e12).toString()",
);

if (process.argv[2] === "child") {
  const fs = require("fs");
  const path = require("path");
  const harness = eval(fs.readFileSync(path.join(__dirname, "../tests/golden/date_bun_harness.js"), "utf8").trim());
  for (const source of new Set(programs)) {
    if (/[\t\n\r]/.test(source)) throw new Error("programa com tabulação ou quebra de linha: " + source);
    emitRow(`${source}\t${harness(source)}`);
  }
} else {
  const byZone = ZONES.map((zone) => {
    const child = spawnSync(process.execPath, [__filename, "child"], {
      env: { ...process.env, TZ: zone },
      encoding: "utf8",
      maxBuffer: 1 << 28,
      timeout: 120000,
    });
    if (child.status !== 0) throw new Error(`fuso ${zone}: ${child.stderr || child.error}`);
    return child.stdout.split("\n").filter((line) => line);
  });
  let independent = 0;
  let dependent = 0;
  for (let index = 0; index < byZone[0].length; index++) {
    const lines = byZone.map((zoneLines) => zoneLines[index]);
    if (lines.every((line) => line === lines[0])) {
      emitRow(`any\t${lines[0]}`);
      independent++;
    } else {
      ZONES.forEach((zone, zoneIndex) => emitRow(`${zone}\t${lines[zoneIndex]}`));
      dependent++;
    }
  }
  console.error(`${new Set(programs).size} programas, ${independent} independentes de fuso, ${dependent} dependentes`);
}
