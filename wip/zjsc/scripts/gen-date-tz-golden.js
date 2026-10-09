// Gera tests/golden/date_tz_bun.tsv: programas de Date avaliados no bun 1.4.2 em oito fusos.
// Colunas: fuso (TZ), fonte do programa (uma linha), KIND e REPR, no formato de date_bun.tsv
// (serializador em tests/golden/date_bun_harness.js, o mesmo que o teste Rust embute).
// O mesmo conjunto de programas roda em cada fuso. Cada fuso roda num subprocesso com TZ definida,
// com timeout.
// Uso: bun scripts/gen-date-tz-golden.js > tests/golden/date_tz_bun.tsv
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const ZONES = [
  "UTC",
  "America/Sao_Paulo",
  "America/New_York",
  "Europe/London",
  "Asia/Kolkata",
  "Australia/Lord_Howe",
  "Pacific/Chatham",
  "Asia/Tehran",
];

const programs = [];
const add = (...sources) => programs.push(...sources);
const q = (text) => JSON.stringify(text);

// Date.parse: formatos variados.
const parseInputs = [
  // ISO
  "2024-03-10", "2024-03", "2024", "2024-03-10T12:00", "2024-03-10T12:00:30", "2024-03-10T12:00:30.5",
  "2024-03-10T12:00:30.123456", "2024-03-10T12:00Z", "2024-03-10T12:00:30Z", "2024-03-10T12:00:30.123Z",
  "2024-03-10T12:00:30+03:00", "2024-03-10T12:00:30-03:00", "2024-03-10T12:00:30+0530", "2024-03-10T12:00:30+05",
  "2024-03-10T12:00:30.5+05:30", "2024-03-10t12:00:30z", "2024-03-10 12:00", "2024-03-10 12:00:30",
  "2024-03-10 12:00:30Z", "2024-03-10 12:00:30 +0300", "2024-03-10T24:00:00", "2024-03-10T24:00:01",
  "2024-03-10T25:00:00", "2024-03-10T12:60:00", "2024-13-10", "2024-02-30", "2024-02-29T00:00:00",
  "2023-02-29", "+002024-03-10", "-000001-01-01T00:00:00Z", "-000000-01-01T00:00:00Z", "+275760-09-13T00:00:00Z",
  "+275760-09-13T00:00:00.001Z", "-271821-04-20T00:00:00Z", "-271821-04-19T23:59:59Z", "0000-01-01", "0001-01-01T00:00:00",
  "1970-01-01T00:00:00Z", "1969-12-31T23:59:59.999Z", "1900-01-01T00:00:00", "1883-11-18T12:00:00",
  "2024-03-10T12", "2024-03-10T", "2024-03-10Z", "2024-3-10", "24-03-10", "2024-03-10T12:00:30,5Z",
  "2024-03-10T01:30:00", "2024-03-10T02:30:00", "2024-03-10T03:30:00", "2024-11-03T01:30:00", "2024-11-03T02:30:00",
  "2024-10-27T01:30:00", "2024-04-07T02:15:00", "2024-10-06T02:15:00", "2024-04-07T03:15:00", "2024-10-06T02:00:00",
  "2018-11-04T00:30:00", "2019-02-17T23:30:00", "2019-02-16T23:30:00", "2024-03-31T01:30:00", "2024-03-31T00:30:00",
  // RFC 2822 e afins
  "Sun, 10 Mar 2024 12:00:00 GMT", "Sun, 10 Mar 2024 12:00:00 +0000", "Sun, 10 Mar 2024 12:00:00 -0300",
  "Sun, 10 Mar 2024 12:00:00 +0530", "Sun, 10 Mar 2024 12:00 GMT", "10 Mar 2024 12:00:00 GMT", "10 Mar 2024",
  "Sun, 10 Mar 2024", "Sun, 10 Mar 24 12:00:00 GMT", "Sun, 10 March 2024 12:00:00 UT", "Sun, 10 Mar 2024 12:00:00 Z",
  "Sun, 10 Mar 2024 12:00:00 EST", "Sun, 10 Mar 2024 12:00:00 EDT", "Sun, 10 Mar 2024 12:00:00 CST",
  "Sun, 10 Mar 2024 12:00:00 CDT", "Sun, 10 Mar 2024 12:00:00 MST", "Sun, 10 Mar 2024 12:00:00 MDT",
  "Sun, 10 Mar 2024 12:00:00 PST", "Sun, 10 Mar 2024 12:00:00 PDT", "Sun, 10 Mar 2024 12:00:00 BRT",
  "Sun, 10 Mar 2024 12:00:00 CET", "Sun, 10 Mar 2024 12:00:00 IST", "Sun, 10 Mar 2024 12:00:00 JST",
  "Sun, 10 Mar 2024 12:00:00 GMT+0300", "Sun, 10 Mar 2024 12:00:00 GMT-0300", "Sun, 10 Mar 2024 12:00:00 GMT+03:00",
  "Sun, 10 Mar 2024 12:00:00 UTC+0100", "Sun, 10 Mar 2024 12:00:00 UTC", "Sun, 10 Mar 2024 12:00:00 (UTC)",
  "Sun, 10 Mar 2024 12:00:00 GMT (Greenwich Mean Time)", "Sun Mar 10 2024", "Sun Mar 10 2024 12:00:00",
  "Sun Mar 10 2024 12:00:00 GMT+0000", "Sun Mar 10 2024 12:00:00 GMT-0300 (Brasilia Standard Time)",
  "Sun Mar 10 2024 12:00:00 GMT+0530 (India Standard Time)", "Mon Jan 01 2024", "Mon Jan 01 2024 00:00:00 GMT+0000",
  "Mon Jan 1 2024", "Mon Jan 01 24", "Mon Jan 01 1970", "Thu Jan 01 1970 00:00:00 GMT+0000",
  "Fri Dec 31 1999 23:59:59 GMT-0500", "Sat Feb 29 2020", "Sun Feb 29 2020",
  // Legadas
  "March 10, 2024", "March 10, 2024 12:00", "March 10, 2024 12:00:00", "March 10, 2024 12:00 PM",
  "March 10, 2024 12:00 AM", "March 10, 2024 1:30 PM", "March 10, 2024 1:30 pm", "March 10, 2024 1:30PM",
  "March 10, 2024 13:30 PM", "March 10, 2024 0:30 AM", "March 10, 2024 11:59:59 pm", "March 10, 2024 12:00:00 am",
  "Mar 10, 2024", "Mar 10 2024", "10 Mar 2024 1:30 pm", "10 March 2024", "10-Mar-2024", "10-Mar-24",
  "10/Mar/2024", "3/10/2024", "03/10/2024", "3/10/24", "3/10/2024 12:00", "3/10/2024 1:30 PM", "03/10/2024 13:30:15",
  "3/10/1999", "3/10/49", "3/10/50", "3/10/69", "3/10/70", "3/10/99", "3/10/00", "3/10/01", "3/10/0", "3/10/100",
  "12/31/1999", "13/01/2024", "1/13/2024", "2/30/2024", "2/29/2023", "02/29/2024", "2024/03/10", "2024/3/10 12:00",
  "2024/03/10 12:00:30", "2024.03.10", "10.03.2024", "1 Jan 70", "1 Jan 49", "1 Jan 50", "1 Jan 99", "1 Jan 00",
  "1 Jan 2000", "1 January 2000", "January 1, 2000", "Jan 1 2000", "Jan 1, 00", "jan 1 2000", "JAN 1 2000", "JANUARY 1 2000",
  "Janu 1 2000", "Ja 1 2000", "Sept 5 2000", "Sep 5 2000", "September 5 2000", "Septembe 5 2000", "Dec 25 2024 9:05 pm",
  "Dec 25 2024 9:05 PM EST", "Dec 25 2024 9:05 PM PST", "Dec 25 2024 9:05 PM GMT", "Dec 25 2024 9:05 PM GMT+0300",
  "Dec 25 2024 9:05 PM +0300", "Dec 25 2024 9:05 PM -0800", "Dec 25 2024 21:05 UTC", "Dec 25 2024 21:05 Z",
  "Dec 25 2024 21:05 EST5EDT", "Dec 25 2024 21:05 America/New_York", "Dec 25 2024 21:05:30.250", "Dec 25 2024 21:05:30.250 GMT",
  "Tuesday, December 25, 2024", "Tuesday December 25 2024 21:05", "Wednesday, December 25, 2024", "Wed Dec 25 2024",
  "Dec 25 2024 24:00", "Dec 25 2024 24:00:01", "Dec 25 2024 25:00", "Dec 25 2024 12:60", "Dec 25 2024 12:00:60",
  "Dec 25 2024 12:00:61", "Dec 32 2024", "Dec 31 2024", "Feb 29 2024", "Feb 30 2024", "Feb 29 2023", "Feb 31 2024",
  "2024 Dec 25", "25 Dec", "Dec 25", "Dec", "2024 Dec", "Dec 2024", "Dec-2024", "12 2024", "12-2024", "2024-12",
  "12:00", "12:00:30", "12:00 PM", "2024", "24", "99", "1999", "20240310", "2024031012", "1e3", "12345", "0",
  "Z", "GMT", "now", "today", "yesterday", "tomorrow", "undefined", "null", "NaN", "Invalid Date",
  "", " ", "   2024-03-10   ", "\t2024-03-10", "2024-03-10 \n", "abc", "2024-03-10abc", "2024-03-10T12:00:30Zabc",
  "Mar 10 2024 abc", "Mar 10 2024 (comentário)", "Mar (x) 10 2024", "Mar 10 (nested (comment)) 2024", "(Mar 10 2024)",
  "Mar 10 2024 12:00 (PM)", "Mar,10,2024", "Mar, 10, 2024", "Mar/10/2024", "Mar 10th 2024", "10th Mar 2024", "Mar 10 2024 AD",
  "Mar 10 2024 BC", "Mar 10 0099", "Mar 10 0100", "Mar 10 1 AD", "Mar 10 -5", "Mar 10 -2024", "Mar 10 +2024",
  "Mar 10 275760", "Sep 13 275760 00:00 UTC", "Sep 13 275760 00:00:01 UTC", "Apr 20 -271821 00:00 UTC",
  "Jan 1 1970 UTC", "Jan 1 1970 GMT", "Jan 1 1970 00:00 +0000", "Jan 1 1970 00:00 -0000", "Jan 1 1970 00:00 +00:00",
  "Jan 1 1970 00:00 +1400", "Jan 1 1970 00:00 +1500", "Jan 1 1970 00:00 -1200", "Jan 1 1970 00:00 -1300", "Jan 1 1970 00:00 +2359",
  "Jan 1 1970 00:00 +2400", "Jan 1 1970 00:00 +0060", "Jan 1 1970 00:00 +01", "Jan 1 1970 00:00 +1", "Jan 1 1970 00:00 +001",
  "Jan 1 1970 00:00 GMT+1", "Jan 1 1970 00:00 GMT+01", "Jan 1 1970 00:00 GMT+0100", "Jan 1 1970 00:00 GMT+01:00",
  "Jan 1 1970 00:00 UTC-5", "Jan 1 1970 00:00 UT+5", "Jan 1 1970 00:00 Z+5", "Jan 1 1970 00:00 EST+1", "Jan 1 1970 00:00 EST EDT",
  "Jan 1 1970 00:00 XYZ", "Jan 1 1970 00:00 A", "Jan 1 1970 00:00 UTCC", "Jan 1 1970 00:00 gmt", "Jan 1 1970 00:00 est", "Jan 1 1970 00:00 Est",
  "1970-01-01T00:00:00.000+00:00", "1970-01-01T00:00:00.000-00:00", "1970-01-01T00:00:00+23:59", "1970-01-01T00:00:00+24:00",
  "1970-01-01T00:00:00+00:60", "1970-01-01T00:00:00Z+01", "1970-01-01T00:00:00 Z", "1970-01-01T00:00:00 +01:00", "1970-01-01T00:00:00 GMT",
  "1970-01-01T00:00:00.1", "1970-01-01T00:00:00.12", "1970-01-01T00:00:00.1234", "1970-01-01T00:00:00.9999999", "1970-01-01T00:00:00.",
  "1970-01-01T00:00:00:00", "1970-01-01T00", "1970-01-01T0:0", "1970-01-01T00:0", "1970-01-01T0:00",
  "2000-01-01T00:00:00.000Z", "2000-02-29T00:00:00Z", "2100-02-29T00:00:00Z", "1900-02-29T00:00:00Z", "2400-02-29T00:00:00Z",
  "Wed Jan 01 2020 00:00:00 GMT+0530", "Sat, 01 Jan 2000 00:00:00 +0545", "Sat, 01 Jan 2000 00:00:00 +1245",
  "Sat, 01 Jan 2000 00:00:00 +1030", "Sat, 01 Jan 2000 00:00:00 +0330", "Sat, 01 Jan 2000 00:00:00 -0330",
];
for (const text of parseInputs) add(`Date.parse(${q(text)})`);

// Construtor com componentes (local) em datas de transição.
const ctorArgs = [
  "2024, 0, 1", "2024, 0, 1, 12", "2024, 0, 1, 12, 30, 15, 500", "99, 0, 1", "100, 0, 1", "0, 0, 1", "49, 0, 1", "70, 0, 1",
  "2024, 12, 1", "2024, -1, 1", "2024, 0, 0", "2024, 0, 32", "2024, 1, 29", "2023, 1, 29", "2024, 0, 1, 24", "2024, 0, 1, 25",
  "2024, 0, 1, 0, 60", "2024, 0, 1, 0, 0, 60", "2024, 0, 1, 0, 0, 0, 1000", "2024, 0, 1, -1", "2024, 0, 1, 0, -1",
  "2024.9, 0.9, 1.9", "NaN, 0", "2024, NaN", "2024, 0, 1, Infinity", "275760, 8, 13", "275760, 8, 13, 0, 0, 0, 1", "-271821, 3, 20",
  "2024, 2, 10, 1, 30", "2024, 2, 10, 2, 30", "2024, 2, 10, 3, 30", "2024, 10, 3, 1, 30", "2024, 10, 3, 2, 30",
  "2024, 9, 27, 1, 30", "2024, 3, 7, 2, 15", "2024, 9, 6, 2, 15", "2024, 3, 7, 3, 15", "2024, 9, 6, 2, 0",
  "2018, 10, 4, 0, 30", "2019, 1, 17, 23, 30", "2019, 1, 16, 23, 30", "2024, 2, 31, 1, 30", "2024, 2, 31, 0, 30",
  "2024, 2, 30, 23, 59", "2024, 2, 31, 2, 30", "1883, 10, 18, 12", "1900, 0, 1", "1970, 0, 1", "1969, 11, 31, 23, 59, 59",
  "1945, 0, 1", "1930, 5, 1", "1985, 10, 1", "1957, 0, 1", "1927, 11, 31, 23, 50", "1940, 5, 1", "1919, 9, 1",
  "1908, 0, 1", "1850, 0, 1", "1800, 0, 1", "1600, 0, 1", "1000, 0, 1", "2037, 5, 1", "2038, 0, 1", "2050, 5, 1", "2100, 5, 1",
];
for (const args of ctorArgs) add(`new Date(${args}).getTime()`, `new Date(${args}).toString()`);

// Date.UTC.
const utcArgs = [
  "2024", "2024, 0", "2024, 0, 1", "2024, 11, 31, 23, 59, 59, 999", "99", "99, 0", "100", "0", "0, 0, 1", "-1", "-1, 0",
  "2024, 12", "2024, -1", "2024, 0, 0", "2024, 0, 32", "2024, 1, 29", "2023, 1, 29", "2024, 0, 1, 24", "2024, 0, 1, 25", "2024, 0, 1, -1",
  "2024.9, 0.9, 1.9", "NaN", "2024, NaN", "275760, 8, 13", "275760, 8, 13, 0, 0, 0, 1", "-271821, 3, 20", "-271821, 3, 19, 23, 59, 59, 999",
  "1970, 0, 1", "1969, 11, 31", "", "undefined", "null", "'2024'", "'2024', '1'", "2024, 0, 1, 0, 0, 0, 0.9", "2024, 0, 1, 0, 0, 0, -0.9",
  "1e10", "8.64e15", "2024, 1e9", "2024, -1e9", "2024, 0, 1e9",
];
for (const args of utcArgs) add(`Date.UTC(${args})`);

// Instantes de referência: getters locais/UTC e formatadores.
const instants = [
  "0", "-1", "1e12", "86400000", "-86400000", "951782400000", "1709164800000", "1710054000000", "1710054000001", "1710061200000",
  "1710064800000", "1710068400000", "1730609400000", "1730613000000", "1730616600000", "1730620200000", "1730179800000",
  "1711930800000", "1712470500000", "1728193500000", "1728190800000", "1728194400000", "1711848600000", "1711852200000",
  "1720000000000", "1735689599999", "4102444800000", "-62135596800000", "-62167219200000", "-2208988800000", "-3786825600000",
  "-5364662400000", "-2209003200000", "-1830384000000", "-1262304000000", "-946771200000", "-631152000000", "-8.64e12",
  "8.64e15", "-8.64e15", "253402300799999", "-30610224000000", "-30610224000001", "-2335219200000", "-2335219200001",
  "-1767225600000", "-1767225600001", "1.5", "-1.5", "NaN",
];
const getters = [
  "getFullYear", "getMonth", "getDate", "getDay", "getHours", "getMinutes", "getSeconds", "getMilliseconds",
  "getUTCFullYear", "getUTCMonth", "getUTCDate", "getUTCDay", "getUTCHours", "getUTCMinutes", "getTimezoneOffset", "getYear",
];
const formatters = [
  "toString", "toDateString", "toTimeString", "toLocaleString", "toLocaleDateString", "toLocaleTimeString",
  "toLocaleString('en-US')", "toLocaleString('pt-BR')", "toLocaleDateString('en-US')", "toLocaleDateString('pt-BR')",
  "toLocaleTimeString('en-US')", "toLocaleTimeString('pt-BR')", "toISOString", "toUTCString", "toJSON", "toGMTString",
];
instants.forEach((instant, index) => {
  // Todos os getters em um só programa por instante, para manter o conjunto compacto, e os
  // formatadores um a um (os lançamentos de RangeError também entram).
  add(`var d = new Date(${instant}); [${getters.map((name) => `d.${name}()`).join(", ")}].join()`);
  for (const formatter of formatters) {
    const call = formatter.includes("(") ? formatter : `${formatter}()`;
    if (index % 2 === 0 || formatter === "toString" || formatter === "toISOString" || formatter === "toLocaleString('en-US')") {
      add(`new Date(${instant}).${call}`);
    }
  }
});

// getTimezoneOffset em varredura (históricas e transições de DST em vários anos).
for (const year of [1883, 1900, 1918, 1927, 1931, 1945, 1957, 1970, 1985, 1995, 2008, 2019, 2024, 2037, 2050, 2100]) {
  for (const month of [0, 2, 3, 6, 9, 10]) {
    add(`new Date(${year}, ${month}, 15, 12).getTimezoneOffset()`);
  }
}
for (const day of [9, 10, 11, 30, 31]) for (const hour of [0, 1, 2, 3, 4]) add(`new Date(2024, 2, ${day}, ${hour}, 30).getTimezoneOffset()`);
for (const day of [2, 3, 4, 5, 6, 7, 26, 27, 28]) for (const hour of [0, 1, 2, 3]) add(`new Date(2024, ${day < 10 ? 10 : 9}, ${day}, ${hour}, 30).getTimezoneOffset()`);
for (const day of [6, 7, 8]) for (const hour of [1, 2, 3]) add(`new Date(2024, ${day === 6 ? 9 : 3}, ${day}, ${hour}, 15).getTimezoneOffset()`);

// Setters (local e UTC), com overflow, a partir de uma data fixa.
const base = "var d = new Date(2024, 0, 31, 12, 30, 15, 500); ";
const setterCalls = [
  "setFullYear(2023)", "setFullYear(2023, 1)", "setFullYear(2023, 1, 29)", "setFullYear(2024, 1, 29)", "setFullYear(NaN)",
  "setMonth(1)", "setMonth(12)", "setMonth(-1)", "setMonth(13, 40)", "setMonth(1, 29)", "setDate(0)", "setDate(32)", "setDate(-31)", "setDate(366)",
  "setHours(24)", "setHours(25)", "setHours(-1)", "setHours(2, 30)", "setHours(2, 30, 0, 0)", "setHours(0, 0, 0, 0)", "setHours(12, 60)", "setHours(1e4)",
  "setMinutes(60)", "setMinutes(-1)", "setMinutes(1440)", "setSeconds(60)", "setSeconds(-1)", "setSeconds(86400)", "setMilliseconds(1000)",
  "setMilliseconds(-1)", "setMilliseconds(86400000)", "setTime(0)", "setTime(NaN)", "setTime(8.64e15)", "setTime(8.64e15 + 1)",
  "setUTCFullYear(2023)", "setUTCMonth(12)", "setUTCDate(0)", "setUTCHours(24)", "setUTCHours(-1)", "setUTCMinutes(60)", "setUTCSeconds(60)",
  "setUTCMilliseconds(1000)", "setUTCHours(0, 0, 0, 0)", "setYear(99)", "setYear(100)", "setYear(2024)", "setYear(0)", "setYear(NaN)",
];
for (const call of setterCalls) add(`${base}var r = d.${call}; r + '|' + d.toString() + '|' + d.toISOString()`.replace(/toISOString\(\)$/, "toISOString()"));
// Setters que atravessam transições de DST.
for (const [dateArgs, call] of [
  ["2024, 2, 10, 0, 30", "setHours(1, 30)"], ["2024, 2, 10, 0, 30", "setHours(2, 30)"], ["2024, 2, 10, 0, 30", "setHours(3, 30)"],
  ["2024, 2, 9, 2, 30", "setDate(10)"], ["2024, 10, 2, 1, 30", "setDate(3)"], ["2024, 10, 3, 0, 30", "setHours(1, 30)"],
  ["2024, 9, 27, 0, 30", "setHours(1, 30)"], ["2024, 3, 7, 1, 30", "setHours(2, 15)"], ["2024, 9, 6, 1, 30", "setHours(2, 15)"],
  ["2024, 2, 31, 0, 30", "setHours(1, 30)"], ["2024, 2, 10, 12", "setMonth(10)"], ["2024, 0, 1, 12", "setMonth(6)"], ["2024, 6, 1, 12", "setMonth(0)"],
  ["2024, 2, 9, 23", "setHours(24 + 2, 30)"], ["2024, 2, 10, 12", "setMinutes(-720)"], ["2024, 10, 3, 12", "setMinutes(-660)"],
]) {
  add(`var d = new Date(${dateArgs}); var r = d.${call}; r + '|' + d.toString() + '|' + d.toISOString()`);
}
// Setters sobre Date inválida.
for (const call of ["setHours(1)", "setFullYear(2024)", "setMonth(0)", "setUTCDate(1)", "setMilliseconds(1)"]) {
  add(`var d = new Date(NaN); var r = d.${call}; r + '|' + d.getTime()`);
}

// Ida e volta (parse do que o próprio Date imprime) e Date.parse de formatos de saída.
for (const instant of ["0", "1710054000000", "1730609400000", "-2208988800000", "1e12", "951782400000", "1711848600000"]) {
  for (const formatter of ["toString", "toUTCString", "toISOString", "toDateString", "toLocaleString('en-US')"]) {
    add(`var d = new Date(${instant}); Date.parse(d.${formatter}()) - d.getTime()`);
  }
  add(`var d = new Date(${instant}); new Date(d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()).getTime() - d.getTime()`);
  add(`var d = new Date(${instant}); JSON.stringify({ d })`);
  add(`var d = new Date(${instant}); String(d) === d.toString()`);
  add(`var d = new Date(${instant}); d.valueOf() === +d && typeof (d + 1)`);
}

// Construtor a partir de string em cada formato crítico, devolvendo ISO (sensível ao fuso local).
for (const text of [
  "2024-03-10", "2024-03-10T12:00", "2024-03-10 12:00", "Mar 10 2024", "Mar 10 2024 12:00", "3/10/2024", "3/10/2024 2:30 AM",
  "11/3/2024 1:30 AM", "11/3/2024 1:30 PM", "10/27/2024 1:30", "4/7/2024 2:15", "10/6/2024 2:15", "10/6/2024 2:00", "2/17/2019 23:30",
  "1/1/1900", "1/1/1883", "11/18/1883 12:00", "1/1/1970", "12/31/1969 23:59:59", "Jan 1 1970 00:00:00 GMT+0300", "1 Jan 2000 EST",
]) {
  add(`new Date(${q(text)}).toISOString()`, `new Date(${q(text)}).toString()`, `new Date(${q(text)}).getTimezoneOffset()`);
}

// Comportamentos estáticos e de conversão.
add(
  "typeof Date()", "Date().length > 20", "typeof Date.now()", "Date.length", "Date.UTC.length", "Date.parse.length",
  "new Date(2024, 0).getTime() === new Date(2024, 0, 1).getTime()", "new Date(0, 0).getFullYear()", "new Date(99, 0).getFullYear()",
  "new Date(100, 0).getFullYear()", "new Date(2024, 0, 1) < new Date(2024, 0, 2)", "new Date(NaN) + ''", "String(new Date(NaN))",
  "new Date(NaN).toISOString()", "new Date(NaN).toJSON()", "new Date(NaN).toUTCString()", "new Date(NaN).toLocaleString()",
  "new Date(NaN).toLocaleString('pt-BR')", "new Date(NaN).toDateString()", "new Date(NaN).toTimeString()", "new Date(NaN).getTimezoneOffset()",
  "new Date(0)[Symbol.toPrimitive]('number')", "new Date(0)[Symbol.toPrimitive]('default')", "new Date(0)[Symbol.toPrimitive]('string')",
  "new Date(0)[Symbol.toPrimitive]('x')", "Object.prototype.toString.call(new Date(0))", "Date.prototype.toGMTString === Date.prototype.toUTCString",
  "new Date(2024, 0, 1).toLocaleDateString('en-US', { timeZone: 'UTC' })", "new Date(0).toLocaleString('en-US', { timeZone: 'America/New_York' })",
  "new Date(0).toLocaleString('pt-BR', { timeZone: 'America/Sao_Paulo' })", "new Date(0).toLocaleString('en-US', { timeZone: 'Asia/Kolkata' })",
  "new Intl.DateTimeFormat().resolvedOptions().timeZone", "new Intl.DateTimeFormat('en-US', { timeZoneName: 'short' }).format(new Date(0))",
  "new Intl.DateTimeFormat('en-US', { timeZoneName: 'long' }).format(new Date(1710054000000))",
  "new Date(1710054000000).toLocaleString('en-US', { timeZoneName: 'short' })",
  "new Date(1730609400000).toLocaleString('pt-BR', { timeZoneName: 'short' })",
  "new Date(0).toLocaleTimeString('en-US', { hour12: false })", "new Date(0).toLocaleTimeString('pt-BR', { hour12: true })",
);

// Cada fuso roda os mesmos programas num subprocesso com TZ definida.
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
