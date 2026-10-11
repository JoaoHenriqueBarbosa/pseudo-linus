// Gera tests/golden/temporal_plain_bun.tsv: Temporal.PlainDate, PlainDateTime, PlainYearMonth e PlainMonthDay com o
// calendário iso8601, medido no bun 1.4.2. Cobre from() com strings ISO de fronteira e inválidas (RangeError exato),
// with() com campos fora de faixa e overflow constrain/reject, add/subtract de Duration em grade (anos, meses, semanas,
// dias, horas; fim de mês, ano bissexto), until/since com largestUnit/smallestUnit/roundingIncrement/roundingMode em
// grade, compare, equals, toString com opções (calendarName, fractionalSecondDigits, smallestUnit) e Duration
// from/round/total com relativeTo PlainDate. Nada de Temporal.Now (depende do relógio).
// Cada programa grava em `globalThis.R` o texto do resultado ou `Nome: mensagem` quando lança. Um bun filho por programa,
// no máximo 6 em paralelo, com timeout de 8 s. Programas já presentes em tests/golden/*.tsv são descartados.
// Uso: bun scripts/gen-temporal-plain-golden.js > tests/golden/temporal_plain_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE = 'function T(f){try{return String(f())}catch(e){return e.name+": "+e.message}}\n';
const q = (text) => JSON.stringify(text);
const exprs = [];
const seen = new Set();
const add = (...list) => { for (const e of list) if (!seen.has(e)) { seen.add(e); exprs.push(e); } };

const PD = "Temporal.PlainDate";
const PDT = "Temporal.PlainDateTime";
const YM = "Temporal.PlainYearMonth";
const MD = "Temporal.PlainMonthDay";
const D = "Temporal.Duration";
const types = { PD, PDT, YM, MD };

// ---------- from(): strings ----------
const dateStrings = [
  "2024-02-29", "2023-02-29", "2024-02-30", "2024-13-01", "2024-00-10", "2024-01-00", "2024-01-32", "0000-01-01", "-000001-01-01", "+000000-01-01", "-000000-01-01",
  "+275760-09-13", "+275760-09-14", "-271821-04-19", "-271821-04-20", "-271821-04-18", "+275760-09-12", "9999-12-31", "10000-01-01", "+010000-01-01", "20240229",
  "2024-0229", "202402-29", "2024-02-29T", "2024-02-29T10", "2024-02-29T10:30", "2024-02-29T10:30:45", "2024-02-29T10:30:45.123456789", "2024-02-29T10:30:45.1234567891",
  "2024-02-29T24:00", "2024-02-29T23:59:60", "2024-02-29T23:59:59.999999999", "2024-02-29 10:30", "2024-02-29t10:30", "2024-02-29T1030", "2024-02-29T103045", "2024-02-29T10:30:45,5",
  "2024-02-29T10:30Z", "2024-02-29T10:30+01:00", "2024-02-29T10:30-03:00[America/Sao_Paulo]", "2024-02-29[UTC]", "2024-02-29[u-ca=iso8601]", "2024-02-29[u-ca=gregory]",
  "2024-02-29[U-CA=iso8601]", "2024-02-29[!u-ca=iso8601]", "2024-02-29[u-ca=iso8601][u-ca=iso8601]", "2024-02-29[!u-ca=iso8601][u-ca=iso8601]", "2024-02-29[u-ca=nope]",
  "2024-02-29[foo=bar]", "2024-02-29[!foo=bar]", "2024-02-29[Bad/Zone]", "2024-02-29T10:30[+05:30]", "2024-02-29T10:30[UTC][u-ca=iso8601]", "2024-02-29T10:30Z[UTC]",
  "2024-02-29Z", "2024-02-29+01:00", "T10:30", "10:30", "", " 2024-02-29", "2024-02-29 ", "2024-2-29", "24-02-29", "２０２４-02-29", "2024\\u221202-29", "\\u22122024-02-29",
  "2024-02-29T10:30:45.", "2024-02-29T10:30:45.1Z", "2024-W09-4", "2024-060", "2024-02", "02-29", "--02-29", "--0229", "2024-02-29T10:30:45+01:00:30", "2024-02-29T10:30:45+0100",
  "1900-02-29", "2000-02-29", "2100-02-29", "1600-02-29", "1970-01-01", "1969-12-31", "2024-12-31T23:59:59.999999999", "2024-12-31T00:00:00.000000001",
];
const ymStrings = [
  "2024-02", "2024-13", "2024-00", "+275760-09", "+275760-10", "-271821-04", "-271821-03", "-000000-01", "+000000-01", "0000-12", "2024-02-29", "2024-02-30", "2024-02-31", "202402",
  "2024-2", "2024-02[u-ca=iso8601]", "2024-02[u-ca=gregory]", "2024-02-01[u-ca=gregory]", "2024-02-01[u-ca=iso8601]", "2024-02-15T10:30", "2024-02-15T10:30Z", "2024-02T10:30",
  "2024-02-01[UTC]", "", "02", "--02", "2024-02-29T24:00", "10000-01", "+010000-01", "2024-W09", "2024-02[foo=bar]", "2024-02-01[!u-ca=iso8601]",
];
const mdStrings = [
  "02-29", "--02-29", "--0229", "0229", "02-30", "02-31", "04-31", "13-01", "00-10", "01-00", "01-32", "2024-02-29", "2023-02-29", "2024-02-30", "02-29[u-ca=iso8601]",
  "02-29[u-ca=gregory]", "2024-02-29[u-ca=gregory]", "--02-29[u-ca=iso8601]", "2-29", "02-9", "", "12-31", "01-01", "2024-02-29T10:30", "02-29T10:30", "--02-29T10:30",
  "1972-02-29", "1971-02-29", "-000001-02-29", "+275760-09-13", "+275760-09-14", "2024-02", "0000-02-29", "10000-02-29",
];
for (const s of dateStrings) {
  add(`${PD}.from(${q(s)}).toString()`, `${PDT}.from(${q(s)}).toString()`);
  add(`${PD}.from(${q(s)}, {overflow: "reject"}).toString()`, `${PD}.from(${q(s)}, {overflow: "constrain"}).toString()`);
}
for (const s of dateStrings.slice(0, 40)) add(`${YM}.from(${q(s)}).toString()`, `${MD}.from(${q(s)}).toString()`);
for (const s of ymStrings) add(`${YM}.from(${q(s)}).toString()`, `${YM}.from(${q(s)}, {overflow: "reject"}).toString()`, `${YM}.from(${q(s)}).toString({calendarName: "always"})`);
for (const s of mdStrings) add(`${MD}.from(${q(s)}).toString()`, `${MD}.from(${q(s)}, {overflow: "reject"}).toString()`, `${MD}.from(${q(s)}).toString({calendarName: "always"})`);
for (const s of ymStrings.slice(0, 10)) add(`${PD}.from(${q(s)}).toString()`);
for (const s of mdStrings.slice(0, 10)) add(`${PD}.from(${q(s)}).toString()`);
for (const o of ["undefined", "null", "{}", `{overflow: "bad"}`, `{overflow: "Constrain"}`, `{overflow: undefined}`, `{overflow: 1}`, `{overflow: null}`, `"reject"`, "5", `{overflow: {toString(){return "reject"}}}`]) {
  add(`${PD}.from("2024-02-30", ${o}).toString()`, `${PDT}.from("2024-02-29T10:30", ${o}).toString()`, `${YM}.from("2024-02", ${o}).toString()`, `${MD}.from("02-29", ${o}).toString()`,
    `${PD}.from({year: 2024, month: 2, day: 30}, ${o}).toString()`, `${PD}.from({year: 2024, month: 13, day: 1}, ${o}).toString()`);
}

// ---------- from(): property bags ----------
const bags = [
  "{year: 2024, month: 2, day: 29}", "{year: 2023, month: 2, day: 29}", "{year: 2024, month: 2, day: 30}", "{year: 2024, month: 13, day: 1}", "{year: 2024, month: 0, day: 1}",
  "{year: 2024, month: 1, day: 0}", "{year: 2024, month: 1, day: 32}", "{year: 2024, month: 1}", "{year: 2024, day: 1}", "{month: 1, day: 1}", "{year: 2024}", "{}", "{year: 2024, monthCode: 'M02', day: 29}",
  "{year: 2024, monthCode: 'M13', day: 1}", "{year: 2024, monthCode: 'M00', day: 1}", "{year: 2024, monthCode: 'M01L', day: 1}", "{year: 2024, monthCode: 'm01', day: 1}", "{year: 2024, monthCode: 'M1', day: 1}",
  "{year: 2024, month: 2, monthCode: 'M02', day: 1}", "{year: 2024, month: 2, monthCode: 'M03', day: 1}", "{year: 2024.9, month: 2.9, day: 3.9}", "{year: '2024', month: '2', day: '3'}",
  "{year: Infinity, month: 1, day: 1}", "{year: NaN, month: 1, day: 1}", "{year: 275760, month: 9, day: 13}", "{year: 275760, month: 9, day: 14}", "{year: -271821, month: 4, day: 19}", "{year: -271821, month: 4, day: 20}",
  "{year: 2024, month: 1, day: 1, hour: 5}", "{year: 2024, month: 1, day: 1, calendar: 'iso8601'}", "{year: 2024, month: 1, day: 1, calendar: 'nope'}", "{year: 2024, month: 1, day: 1, era: 'ce', eraYear: 2024}",
  "{year: 2024, month: -1, day: 1}", "{year: 2024, month: 1, day: -1}", "{year: undefined, month: 1, day: 1}", "{year: 2024, month: null, day: 1}", "{year: 2024, month: 1n, day: 1}", "{year: Symbol(), month: 1, day: 1}",
  "{year: 99999999, month: 1, day: 1}", "{year: 2024, month: 4, day: 31}", "{year: 2024, month: 12, day: 31}", "{year: 1900, month: 2, day: 29}", "{year: 2000, month: 2, day: 29}",
];
for (const b of bags) {
  for (const o of ["", `, {overflow: "reject"}`, `, {overflow: "constrain"}`]) add(`${PD}.from(${b}${o}).toString()`, `${YM}.from(${b}${o}).toString()`, `${MD}.from(${b}${o}).toString()`);
}
const dtBags = [
  "{year: 2024, month: 2, day: 29, hour: 10, minute: 30}", "{year: 2024, month: 2, day: 29, hour: 24}", "{year: 2024, month: 2, day: 29, hour: 23, minute: 60}", "{year: 2024, month: 2, day: 29, second: 60}",
  "{year: 2024, month: 2, day: 29, millisecond: 1000}", "{year: 2024, month: 2, day: 29, microsecond: 1000}", "{year: 2024, month: 2, day: 29, nanosecond: 1000}", "{year: 2024, month: 2, day: 29, hour: -1}",
  "{year: 2024, month: 2, day: 29, hour: 1.9, minute: 2.9}", "{year: 2024, month: 2, day: 29}", "{year: 2024, month: 2, day: 30, hour: 25}", "{year: 2024, month: 2, hour: 5}", "{year: 2024, month: 2, day: 29, nanosecond: 999}",
  "{year: 275760, month: 9, day: 13, hour: 23, minute: 59, second: 59}", "{year: 275760, month: 9, day: 14}", "{year: -271821, month: 4, day: 19}", "{year: -271821, month: 4, day: 19, hour: 0, nanosecond: 1}",
];
for (const b of dtBags) for (const o of ["", `, {overflow: "reject"}`, `, {overflow: "constrain"}`]) add(`${PDT}.from(${b}${o}).toString()`);
// construtores
for (const args of ["2024, 2, 29", "2024, 2, 30", "2023, 2, 29", "2024, 13, 1", "2024, 0, 1", "2024, 1, 0", "2024.5, 1, 1", "'2024', '2', '3'", "2024, 1", "2024", "", "275760, 9, 13", "275760, 9, 14", "-271821, 4, 19", "-271821, 4, 18",
  "2024, 1, 1, 'iso8601'", "2024, 1, 1, 'gregory'", "2024, 1, 1, 'nope'", "2024, 1, 1, undefined", "2024, 1, 1, 5", "Infinity, 1, 1", "NaN, 1, 1", "-0, 1, 1", "2024n, 1, 1"]) {
  add(`new ${PD}(${args}).toString()`);
}
for (const args of ["2024, 2, 29", "2024, 2, 29, 10, 30, 45, 123, 456, 789", "2024, 2, 29, 24", "2024, 2, 29, 0, 60", "2024, 2, 29, 0, 0, 60", "2024, 2, 29, 0, 0, 0, 1000", "2024, 2, 29, 0, 0, 0, 0, 1000", "2024, 2, 29, 0, 0, 0, 0, 0, 1000",
  "2024, 2, 29, -1", "2024, 2, 29, 1.5", "2024, 2, 29, 'x'", "2024, 2", "2024, 2, 29, 1, 2, 3, 4, 5, 6, 'gregory'", "2024, 2, 29, 1, 2, 3, 4, 5, 6, 'iso8601'", "275760, 9, 13, 23, 59, 59, 999, 999, 999", "275760, 9, 14", "-271821, 4, 19, 0, 0, 0, 0, 0, 0", "-271821, 4, 19, 0, 0, 0, 0, 0, 1"]) {
  add(`new ${PDT}(${args}).toString()`);
}
for (const args of ["2024, 2", "2024, 2, 'iso8601', 29", "2024, 2, 'iso8601', 30", "2023, 2, 'iso8601', 29", "2024, 13", "2024, 0", "2024", "275760, 9", "275760, 10", "-271821, 4", "-271821, 3", "2024, 2, undefined, 1", "2024, 2, 'gregory'", "2024, 2, 'nope'", "2024, 2, undefined, 0", "2024, 2, undefined, 32", "2024, 2, null"]) {
  add(`new ${YM}(${args}).toString()`);
}
for (const args of ["2, 29", "2, 30", "13, 1", "0, 1", "1, 0", "1, 32", "2, 29, 'iso8601', 2023", "2, 29, 'iso8601', 1972", "2, 29, 'iso8601', 1971", "2, 29, undefined, 2024", "2, 29, 'gregory'", "2, 29, 'nope'", "2", "", "4, 31", "12, 31, 'iso8601', 275760", "9, 14, 'iso8601', 275760", "4, 18, 'iso8601', -271821", "1, 1, 'iso8601', 99999999"]) {
  add(`new ${MD}(${args}).toString()`);
}
add(`${PD}.from(5)`, `${PD}.from(null)`, `${PD}.from(undefined)`, `${PD}.from(true)`, `${PD}.from(Symbol())`, `${PD}.from(1n)`, `${PD}.from([])`, `${PD}()`, `${PDT}()`, `${YM}()`, `${MD}()`,
  `${PD}.from(${PD}.from("2024-02-29")).toString()`, `${PD}.from(${PDT}.from("2024-02-29T10:30")).toString()`, `${PDT}.from(${PD}.from("2024-02-29")).toString()`, `${YM}.from(${PD}.from("2024-02-29")).toString()`,
  `${MD}.from(${PD}.from("2024-02-29")).toString()`, `${YM}.from(${PDT}.from("2024-02-29T10:30")).toString()`, `${MD}.from(${YM}.from("2024-02"))`, `${PD}.from(${YM}.from("2024-02"))`,
  `${PD}.from(${MD}.from("02-29"))`, `${PD}.from({year: 2024, month: 2, day: 29, toString(){return "2000-01-01"}}).toString()`, `${PD}.from({toString(){return "2000-01-01"}})`,
  `${PD}.from(new String("2024-02-29")).toString()`, `${PD}.from({year: 2024, month: 2, day: 29, [Symbol.toPrimitive]: 5}).toString()`, `${PD}.prototype.toString.call({})`, `${PD}.prototype.year`, `${YM}.prototype.month`,
  `Object.prototype.toString.call(${PD}.from("2024-02-29"))`, `Object.prototype.toString.call(${PDT}.from("2024-02-29"))`, `Object.prototype.toString.call(${YM}.from("2024-02"))`, `Object.prototype.toString.call(${MD}.from("02-29"))`,
  `${PD}.from("2024-02-29").valueOf()`, `${PDT}.from("2024-02-29").valueOf()`, `${YM}.from("2024-02").valueOf()`, `${MD}.from("02-29").valueOf()`, `${PD}.from("2024-02-29") < ${PD}.from("2024-03-01")`,
  `${PD}.from("2024-02-29") + 1`, `JSON.stringify(${PD}.from("2024-02-29"))`, `JSON.stringify({a: ${PDT}.from("2024-02-29T10:30")})`, `JSON.stringify(${YM}.from("2024-02"))`, `JSON.stringify(${MD}.from("02-29"))`);

// ---------- propriedades derivadas ----------
for (const s of ["2024-02-29", "2023-12-31", "2024-12-31", "2021-01-03", "2020-12-31", "2026-01-01", "2024-01-01", "1900-03-01", "0000-01-01", "-000001-12-31", "+275760-09-13", "-271821-04-19", "2100-02-28", "2000-12-31", "2019-12-30", "2021-01-04", "2015-12-31", "2016-01-03"]) {
  for (const p of ["year", "month", "monthCode", "day", "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "calendarId", "era", "eraYear"]) add(`${PD}.from(${q(s)}).${p}`);
  add(`${PD}.from(${q(s)}).toPlainYearMonth().toString()`, `${PD}.from(${q(s)}).toPlainMonthDay().toString()`, `${PD}.from(${q(s)}).toPlainDateTime().toString()`, `${PD}.from(${q(s)}).toPlainDateTime("13:45:07.5").toString()`,
    `${PD}.from(${q(s)}).toPlainDateTime("24:00")`, `${PD}.from(${q(s)}).toPlainDateTime(5)`, `${PD}.from(${q(s)}).withCalendar("iso8601").toString()`, `${PD}.from(${q(s)}).withCalendar("nope")`);
}
for (const s of ["2024-02", "2024-12", "2023-02", "1900-02", "2000-02", "-000001-12", "+275760-09", "-271821-04", "0000-01"]) {
  for (const p of ["year", "month", "monthCode", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "calendarId", "era", "eraYear"]) add(`${YM}.from(${q(s)}).${p}`);
  for (const d of [1, 15, 28, 29, 30, 31, 0, 32]) add(`${YM}.from(${q(s)}).toPlainDate({day: ${d}}).toString()`);
  add(`${YM}.from(${q(s)}).toPlainDate()`, `${YM}.from(${q(s)}).toPlainDate(5)`, `${YM}.from(${q(s)}).toPlainDate({})`);
}
for (const s of ["02-29", "12-31", "01-01", "04-30", "02-28", "09-14"]) {
  for (const p of ["monthCode", "day", "calendarId"]) add(`${MD}.from(${q(s)}).${p}`);
  for (const y of [2024, 2023, 1972, 1971, 0, -1, 275760, -271821, 100000]) add(`${MD}.from(${q(s)}).toPlainDate({year: ${y}}).toString()`);
  add(`${MD}.from(${q(s)}).toPlainDate()`, `${MD}.from(${q(s)}).month`, `${MD}.from(${q(s)}).year`, `${MD}.from(${q(s)}).toPlainDate({month: 1, year: 2024})`);
}
for (const s of ["2024-02-29T10:30:45.123456789", "2023-12-31T23:59:59.999999999", "2024-01-01T00:00", "2024-06-15T12:00:00.5"]) {
  for (const p of ["year", "month", "day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "dayOfWeek", "dayOfYear", "weekOfYear", "daysInMonth", "inLeapYear"]) add(`${PDT}.from(${q(s)}).${p}`);
  add(`${PDT}.from(${q(s)}).toPlainDate().toString()`, `${PDT}.from(${q(s)}).toPlainTime().toString()`, `${PDT}.from(${q(s)}).toPlainYearMonth().toString()`, `${PDT}.from(${q(s)}).toPlainMonthDay().toString()`,
    `${PDT}.from(${q(s)}).toZonedDateTime("UTC").toString()`, `${PDT}.from(${q(s)}).toZonedDateTime("Bad/Zone")`, `${PDT}.from(${q(s)}).withPlainTime("01:02").toString()`, `${PDT}.from(${q(s)}).withPlainTime().toString()`,
    `${PDT}.from(${q(s)}).withCalendar("iso8601").toString()`);
}

// ---------- with() ----------
const withBases = ["2024-02-29", "2023-01-31", "2024-12-31", "2000-01-01"];
const withBags = [
  "{year: 2023}", "{year: 2025}", "{month: 2}", "{month: 4}", "{month: 13}", "{month: 0}", "{month: -1}", "{month: 12}", "{day: 30}", "{day: 31}", "{day: 32}", "{day: 0}", "{day: -1}", "{day: 1}", "{day: 28}",
  "{year: 2023, month: 2}", "{year: 2023, month: 2, day: 29}", "{month: 2, day: 30}", "{monthCode: 'M02'}", "{monthCode: 'M06'}", "{monthCode: 'M13'}", "{monthCode: 'M02L'}", "{monthCode: 'M00'}", "{monthCode: 'bad'}",
  "{month: 2, monthCode: 'M03'}", "{month: 3, monthCode: 'M03'}", "{}", "{foo: 1}", "{year: undefined}", "{year: Infinity}", "{year: 275760, month: 9, day: 14}", "{year: -271821, month: 4, day: 18}", "{year: 275760}",
  "{day: 1.9}", "{day: '15'}", "{day: null}", "{day: Symbol()}", "{hour: 5}", "{calendar: 'iso8601'}", "{timeZone: 'UTC'}", "{era: 'ce'}", "5", "'2024-01-01'", "null", "undefined", "[]",
];
for (const b of withBases) for (const w of withBags) {
  for (const o of ["", `, {overflow: "reject"}`, `, {overflow: "constrain"}`]) add(`${PD}.from(${q(b)}).with(${w}${o}).toString()`);
}
for (const b of withBases) for (const w of ["{day: 31}", "{day: 99}", "{month: 99}", "{month: 2, day: 31}", "{year: 2023, month: 2, day: 29}"]) {
  for (const o of ["undefined", "null", "{}", `{overflow: "bad"}`, `{overflow: undefined}`, "5", `"reject"`]) add(`${PD}.from(${q(b)}).with(${w}, ${o}).toString()`);
}
const ymBases = ["2024-02", "2023-12", "2000-01"];
for (const b of ymBases) for (const w of ["{year: 2023}", "{month: 13}", "{month: 0}", "{month: 6}", "{monthCode: 'M13'}", "{monthCode: 'M05'}", "{day: 5}", "{}", "{year: 275760, month: 10}", "{year: -271821, month: 3}", "{year: 275760, month: 9}", "{year: 99999999}", "{month: 1.5}", "{year: 'x'}"]) {
  for (const o of ["", `, {overflow: "reject"}`, `, {overflow: "constrain"}`]) add(`${YM}.from(${q(b)}).with(${w}${o}).toString()`);
}
const mdBases = ["02-29", "12-31", "04-30"];
for (const b of mdBases) for (const w of ["{month: 2}", "{month: 4}", "{month: 13}", "{day: 31}", "{day: 30}", "{day: 0}", "{monthCode: 'M02'}", "{monthCode: 'M04'}", "{month: 2, day: 30}", "{month: 2, day: 29}", "{year: 2023}", "{year: 2023, month: 2, day: 29}", "{}", "{day: -1}", "{monthCode: 'M13'}", "{day: 1.5}"]) {
  for (const o of ["", `, {overflow: "reject"}`, `, {overflow: "constrain"}`]) add(`${MD}.from(${q(b)}).with(${w}${o}).toString()`);
}
const dtBases = ["2024-02-29T10:30:45.123456789", "2023-01-31T00:00", "2024-12-31T23:59:59.999999999"];
for (const b of dtBases) for (const w of ["{hour: 24}", "{hour: 23}", "{minute: 60}", "{second: 60}", "{millisecond: 1000}", "{microsecond: 1000}", "{nanosecond: 1000}", "{hour: -1}", "{minute: 59, second: 59}", "{day: 31, month: 4}", "{month: 2, day: 30}", "{year: 2023, month: 2}", "{hour: 5.9}", "{}", "{hour: 'x'}", "{nanosecond: 0, microsecond: 0, millisecond: 0}", "{hour: 12, minute: 0, second: 0}"]) {
  for (const o of ["", `, {overflow: "reject"}`, `, {overflow: "constrain"}`]) add(`${PDT}.from(${q(b)}).with(${w}${o}).toString()`);
}

// ---------- add/subtract: grade de durações ----------
const addBases = ["2024-01-31", "2024-02-29", "2023-02-28", "2024-03-31", "2024-12-31", "2023-12-31", "2000-02-29", "1900-01-31", "2024-05-31", "2024-08-31", "2024-10-31", "2023-01-01", "0000-01-31", "9999-12-31", "2024-02-28"];
const durs = [
  "{years: 1}", "{years: -1}", "{years: 4}", "{years: 100}", "{months: 1}", "{months: -1}", "{months: 12}", "{months: 13}", "{months: -13}", "{months: 24}", "{months: 25}", "{weeks: 1}", "{weeks: -1}", "{weeks: 5}", "{weeks: 52}",
  "{days: 1}", "{days: -1}", "{days: 7}", "{days: 29}", "{days: 30}", "{days: 31}", "{days: 365}", "{days: 366}", "{days: -366}", "{days: 1000}", "{hours: 24}", "{hours: 23}", "{hours: 25}", "{hours: -24}", "{hours: 48}", "{hours: 1}", "{hours: -1}",
  "{minutes: 1440}", "{minutes: 1439}", "{seconds: 86400}", "{seconds: 86399}", "{milliseconds: 86400000}", "{microseconds: 86400000000}", "{nanoseconds: 86400000000000}", "{nanoseconds: 86399999999999}",
  "{years: 1, months: 1}", "{years: 1, months: 1, days: 1}", "{years: 1, months: 1, weeks: 1, days: 1}", "{years: 1, days: -1}", "{months: 1, days: -1}", "{months: 1, days: 1}", "{years: -1, months: 1}", "{months: -1, days: -1}",
  "{years: 1, months: 1, weeks: 1, days: 1, hours: 25}", "{years: 1, hours: 24, minutes: 60}", "{months: 2, weeks: 3, days: 4}", "{weeks: 1, days: 7}", "{weeks: 1, days: -7}", "{days: 1, hours: -25}", "{days: 1, hours: 25}",
  "{years: 1, hours: -1}", "{hours: 24, nanoseconds: 1}", "{hours: 24, nanoseconds: -1}", "{}", "{years: 0}", "{days: 0.5}", "{days: 1.5}", "{years: Infinity}", "{years: NaN}", "{years: 'x'}",
  "{years: 1, months: -1}", "{years: 1000000}", "{months: 1000000}", "{weeks: 100000000}", "{days: 100000000}", "{days: 1e9}", "{hours: 1e9}", "{years: 275760}", "{years: -275760}", "{years: 274000}", "{months: 12 * 275760}",
  "'P1Y'", "'P1M'", "'-P1M'", "'P1W'", "'P1D'", "'PT24H'", "'PT23H59M'", "'P1Y1M1W1DT1H'", "'PT0S'", "'P1Y2M3DT4H5M6S'", "'bad'", "''", "'P1.5D'", "'PT1.5H'", "'P1M1'", "5", "null", "undefined", "true", "[]",
  `${D}.from("P1M")`, `${D}.from("-P1Y1M")`, `new ${D}(0, 0, 0, 1)`, `new ${D}(0, 0, 0, 0, 1)`,
];
for (const b of addBases) {
  for (const d of durs) {
    add(`${PD}.from(${q(b)}).add(${d}).toString()`, `${PD}.from(${q(b)}).subtract(${d}).toString()`);
  }
}
const ovDurs = ["{months: 1}", "{months: 12}", "{years: 1}", "{years: 4}", "{years: 1, months: 1}", "{months: -1}", "{years: -1}", "{months: 1, days: 1}", "{months: 1, days: -1}", "{years: 1, months: 1, days: -1}"];
for (const b of addBases) for (const d of ovDurs) {
  for (const o of ["{overflow: 'reject'}", "{overflow: 'constrain'}", "{overflow: 'bad'}", "{overflow: undefined}", "{}", "undefined", "null", "5", "'reject'", "{overflow: 1}", "{overflow: 'REJECT'}"]) {
    add(`${PD}.from(${q(b)}).add(${d}, ${o}).toString()`, `${PD}.from(${q(b)}).subtract(${d}, ${o}).toString()`);
  }
}
// PlainDateTime
const dtAddBases = ["2024-01-31T10:30:45.123456789", "2024-02-29T23:59:59.999999999", "2024-03-31T00:00", "2023-12-31T12:00", "2024-02-28T23:30", "2000-02-29T00:00:00.000000001"];
const dtDurs = ["{years: 1}", "{months: 1}", "{months: -1}", "{weeks: 1}", "{days: 1}", "{days: -1}", "{hours: 1}", "{hours: -1}", "{hours: 24}", "{hours: 25}", "{hours: -25}", "{hours: 13}", "{hours: 12, minutes: 30}", "{minutes: 30}", "{minutes: -30}", "{minutes: 1}",
  "{seconds: 1}", "{seconds: -1}", "{milliseconds: 1}", "{microseconds: 1}", "{nanoseconds: 1}", "{nanoseconds: -1}", "{nanoseconds: 999999999}", "{years: 1, months: 1, weeks: 1, days: 1, hours: 1, minutes: 1, seconds: 1, milliseconds: 1, microseconds: 1, nanoseconds: 1}",
  "{years: -1, months: -1, weeks: -1, days: -1, hours: -1, minutes: -1, seconds: -1}", "{years: 1, hours: -1}", "{months: 1, hours: 24}", "{days: 1, hours: -25}", "{hours: 24, minutes: 1440}", "{hours: 1e9}", "{days: 1e8}", "{years: 275760}",
  "{months: 12}", "{months: 13}", "{years: 4}", "{weeks: 52}", "{days: 366}", "{minutes: 1440}", "{seconds: 86400}", "{milliseconds: 86400000}", "{nanoseconds: 86400000000000}", "{}", "'PT1H'", "'P1D'", "'bad'", "5"];
for (const b of dtAddBases) for (const d of dtDurs) {
  add(`${PDT}.from(${q(b)}).add(${d}).toString()`, `${PDT}.from(${q(b)}).subtract(${d}).toString()`);
}
for (const b of dtAddBases) for (const d of ["{months: 1}", "{years: 1}", "{months: 12}", "{years: 1, months: 1}", "{months: -1}"]) for (const o of ["{overflow: 'reject'}", "{overflow: 'constrain'}", "{overflow: 'bad'}"]) {
  add(`${PDT}.from(${q(b)}).add(${d}, ${o}).toString()`, `${PDT}.from(${q(b)}).subtract(${d}, ${o}).toString()`);
}
// PlainYearMonth
for (const b of ["2024-01", "2024-02", "2023-12", "2000-02", "-000001-12", "+275760-09", "-271821-04", "9999-12"]) {
  for (const d of ["{years: 1}", "{years: -1}", "{months: 1}", "{months: -1}", "{months: 12}", "{months: 13}", "{months: -13}", "{years: 1, months: 1}", "{years: 1, months: -1}", "{weeks: 1}", "{days: 1}", "{days: 30}", "{days: 31}", "{days: 28}", "{days: 29}", "{days: -1}", "{days: -28}", "{days: -31}",
    "{hours: 24}", "{hours: 1}", "{hours: 24 * 31}", "{months: 1, days: 1}", "{months: 1, days: -1}", "{}", "'P1M'", "'-P1Y'", "'P1D'", "'PT1H'", "{years: 275760}", "{years: -275760}", "{months: 1000000}", "{days: 100000000}", "{years: 1, days: 1}", "{years: 1, days: -1}"]) {
    for (const o of ["", ", {overflow: 'reject'}", ", {overflow: 'constrain'}"]) add(`${YM}.from(${q(b)}).add(${d}${o}).toString()`, `${YM}.from(${q(b)}).subtract(${d}${o}).toString()`);
  }
}
// PlainMonthDay não tem add/subtract
add(`${MD}.from("02-29").add`, `${MD}.from("02-29").subtract`, `${MD}.from("02-29").until`, `${MD}.from("02-29").since`, `${MD}.prototype.add`, `${MD}.compare`);

// ---------- until/since ----------
const pairs = [
  ["2024-01-31", "2024-03-31"], ["2024-01-31", "2024-02-29"], ["2024-02-29", "2025-02-28"], ["2024-02-29", "2028-02-29"], ["2024-03-31", "2024-02-29"], ["2024-12-31", "2025-01-01"], ["2000-01-01", "2024-02-29"],
  ["2024-02-29", "2024-02-29"], ["2024-05-31", "2024-06-30"], ["2023-01-31", "2023-03-01"], ["2023-03-01", "2023-01-31"], ["2020-02-29", "2021-02-28"], ["2021-02-28", "2020-02-29"], ["2024-01-01", "2024-12-31"],
  ["1999-12-31", "2000-01-01"], ["2024-08-31", "2024-11-30"], ["2024-11-30", "2024-08-31"], ["0000-01-01", "9999-12-31"], ["-000001-01-01", "0001-01-01"], ["2024-01-31", "2025-03-01"], ["2024-02-28", "2024-03-28"],
];
const units = ["auto", "years", "months", "weeks", "days", "year", "month", "week", "day", "hours", "minutes", "seconds", "bad", "", "milliseconds"];
for (const [a, b] of pairs) {
  add(`${PD}.from(${q(a)}).until(${q(b)}).toString()`, `${PD}.from(${q(a)}).since(${q(b)}).toString()`);
  for (const u of units) {
    add(`${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(u)}}).toString()`, `${PD}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(u)}}).toString()`);
  }
  for (const lu of ["years", "months", "weeks", "days"]) {
    for (const su of ["years", "months", "weeks", "days", "hours"]) {
      add(`${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}, smallestUnit: ${q(su)}}).toString()`, `${PD}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}, smallestUnit: ${q(su)}}).toString()`);
    }
  }
}
const modes = ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven", "bad"];
for (const [a, b] of pairs.slice(0, 14)) {
  for (const su of ["years", "months", "weeks", "days"]) {
    for (const m of modes) {
      add(`${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: "years", smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`,
        `${PD}.from(${q(a)}).since(${q(b)}, {largestUnit: "years", smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`);
    }
    for (const inc of [1, 2, 3, 5, 6, 7, 10, 12, 13, 0, -1, 1.5, 100, "2", "x"]) {
      add(`${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: "years", smallestUnit: ${q(su)}, roundingIncrement: ${inc === "x" || inc === "2" ? q(inc) : inc}, roundingMode: "halfExpand"}).toString()`,
        `${PD}.from(${q(a)}).since(${q(b)}, {largestUnit: "years", smallestUnit: ${q(su)}, roundingIncrement: ${inc === "x" || inc === "2" ? q(inc) : inc}, roundingMode: "ceil"}).toString()`);
    }
  }
}
for (const [a, b] of pairs.slice(0, 8)) {
  for (const o of ["{smallestUnit: 'months'}", "{smallestUnit: 'weeks'}", "{largestUnit: 'months', smallestUnit: 'years'}", "{largestUnit: 'days', smallestUnit: 'years'}", "{largestUnit: 'weeks', smallestUnit: 'months'}", "{largestUnit: 'hours'}",
    "{smallestUnit: 'hours'}", "{smallestUnit: 'nanoseconds'}", "{largestUnit: 'auto', smallestUnit: 'days'}", "{largestUnit: 'years', smallestUnit: 'months', roundingMode: 'halfExpand', roundingIncrement: 4}", "{roundingMode: 'floor'}",
    "{roundingMode: 'ceil', smallestUnit: 'months'}", "{roundingIncrement: 2}", "{roundingIncrement: 2, smallestUnit: 'days'}", "{smallestUnit: 'days', roundingIncrement: 30, largestUnit: 'days'}", "{smallestUnit: 'weeks', roundingIncrement: 2, roundingMode: 'trunc'}", "undefined", "null", "5", "'days'", "{}"]) {
    add(`${PD}.from(${q(a)}).until(${q(b)}, ${o}).toString()`, `${PD}.from(${q(a)}).since(${q(b)}, ${o}).toString()`);
  }
}
const badOthers = ["null", "undefined", "5", "true", "''", "'bad'", "{}", "{year: 2024}", "{year: 2024, month: 1, day: 1}", "{year: 2024, month: 13, day: 1}", "'2024-01-01T10:30'", "'2024-01-01[u-ca=gregory]'", "[]", "Symbol()",
  `${PDT}.from("2024-01-01T10:30")`, `${YM}.from("2024-01")`, `${MD}.from("01-01")`, `${PD}.from("2024-01-01")`, "'+275760-09-14'", "'2024-02-30'"];
for (const o of badOthers) add(`${PD}.from("2024-02-29").until(${o}).toString()`, `${PD}.from("2024-02-29").since(${o}).toString()`, `${PD}.from("2024-02-29").equals(${o})`, `${PD}.compare(${o}, "2024-02-29")`, `${PD}.compare("2024-02-29", ${o})`);
// PlainDateTime until/since
const dtPairs = [
  ["2024-01-31T10:30", "2024-03-31T09:30"], ["2024-02-29T23:59:59.999999999", "2024-03-01T00:00"], ["2024-03-01T00:00", "2024-02-29T23:59:59.999999999"], ["2023-01-01T00:00", "2024-02-29T12:34:56.789123456"],
  ["2024-02-29T12:00", "2024-02-29T12:00"], ["2024-01-01T00:00", "2024-01-01T23:59:59.999999999"], ["2024-01-31T12:00", "2024-02-29T11:00"], ["2024-02-29T11:00", "2024-01-31T12:00"], ["2020-02-29T10:00", "2021-02-28T10:00"],
  ["2024-12-31T23:00", "2025-01-01T01:00"], ["2024-05-31T00:00", "2024-06-30T23:59"], ["2000-01-01T00:00", "2024-02-29T23:59:59.999999999"],
];
const dtUnits = ["auto", "years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"];
for (const [a, b] of dtPairs) {
  add(`${PDT}.from(${q(a)}).until(${q(b)}).toString()`, `${PDT}.from(${q(a)}).since(${q(b)}).toString()`);
  for (const lu of dtUnits) {
    add(`${PDT}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}}).toString()`, `${PDT}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
  }
  for (const su of dtUnits.slice(1)) {
    for (const m of ["trunc", "halfExpand", "ceil", "floor", "halfEven", "expand"]) {
      add(`${PDT}.from(${q(a)}).until(${q(b)}, {smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`, `${PDT}.from(${q(a)}).since(${q(b)}, {largestUnit: "years", smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`);
    }
  }
  for (const [su, incs] of [["hours", [1, 2, 3, 4, 6, 8, 12, 24, 5]], ["minutes", [1, 5, 15, 30, 60, 7]], ["seconds", [1, 10, 30, 60]], ["milliseconds", [1, 100, 500, 1000]], ["microseconds", [1, 250, 1000]], ["nanoseconds", [1, 500, 1000, 3]], ["days", [1, 2, 7]], ["months", [1, 2, 3, 6, 12]]]) {
    for (const inc of incs) add(`${PDT}.from(${q(a)}).until(${q(b)}, {largestUnit: "years", smallestUnit: ${q(su)}, roundingIncrement: ${inc}, roundingMode: "halfExpand"}).toString()`);
  }
}
// YearMonth until/since
const ymPairs = [["2024-01", "2025-03"], ["2025-03", "2024-01"], ["2024-02", "2024-02"], ["2000-01", "2024-12"], ["2024-12", "2000-01"], ["-000001-01", "0001-01"], ["2023-11", "2024-02"], ["2024-01", "2024-12"], ["2020-02", "2021-01"], ["+275760-09", "-271821-04"]];
for (const [a, b] of ymPairs) {
  add(`${YM}.from(${q(a)}).until(${q(b)}).toString()`, `${YM}.from(${q(a)}).since(${q(b)}).toString()`);
  for (const lu of ["auto", "years", "months", "weeks", "days", "hours", "bad"]) add(`${YM}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}}).toString()`, `${YM}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
  for (const su of ["years", "months", "weeks", "days"]) for (const m of ["trunc", "ceil", "floor", "halfExpand", "expand", "halfEven"]) {
    add(`${YM}.from(${q(a)}).until(${q(b)}, {smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`, `${YM}.from(${q(a)}).since(${q(b)}, {smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`);
  }
  for (const inc of [1, 2, 3, 4, 5, 6, 12, 0]) add(`${YM}.from(${q(a)}).until(${q(b)}, {smallestUnit: "months", roundingIncrement: ${inc}, roundingMode: "halfExpand"}).toString()`, `${YM}.from(${q(a)}).since(${q(b)}, {smallestUnit: "months", roundingIncrement: ${inc}, roundingMode: "trunc"}).toString()`);
}
for (const o of badOthers) add(`${YM}.from("2024-02").until(${o}).toString()`, `${YM}.from("2024-02").since(${o}).toString()`, `${YM}.from("2024-02").equals(${o})`, `${YM}.compare(${o}, "2024-02")`, `${MD}.from("02-29").equals(${o})`);
for (const o of badOthers) add(`${PDT}.from("2024-02-29T10:30").until(${o}).toString()`, `${PDT}.from("2024-02-29T10:30").since(${o}).toString()`, `${PDT}.from("2024-02-29T10:30").equals(${o})`, `${PDT}.compare(${o}, "2024-02-29T10:30")`);

// ---------- compare e equals ----------
const cmpDates = ["2024-02-29", "2024-02-28", "2024-03-01", "2023-02-28", "2024-02-29T00:00", "2024-02-29T10:30", "+275760-09-13", "-271821-04-19", "0000-01-01", "2024-02-29[u-ca=gregory]", "2024-02-29[u-ca=iso8601]"];
for (const a of cmpDates) for (const b of cmpDates) add(`${PD}.compare(${q(a)}, ${q(b)})`, `${PD}.from(${q(a)}).equals(${q(b)})`);
const cmpDt = ["2024-02-29T10:30", "2024-02-29T10:30:00.000000001", "2024-02-29T10:29:59.999999999", "2024-02-29", "2024-03-01T00:00", "2024-02-28T23:59:59.999999999", "-271821-04-19T00:00", "+275760-09-13T23:59:59.999999999", "2024-02-29T10:30[u-ca=gregory]"];
for (const a of cmpDt) for (const b of cmpDt) add(`${PDT}.compare(${q(a)}, ${q(b)})`, `${PDT}.from(${q(a)}).equals(${q(b)})`);
const cmpYm = ["2024-02", "2024-03", "2023-12", "2024-02-01", "2024-02-29", "-271821-04", "+275760-09", "0000-01", "2024-02[u-ca=gregory]", "2024-02-01[u-ca=gregory]"];
for (const a of cmpYm) for (const b of cmpYm) add(`${YM}.compare(${q(a)}, ${q(b)})`, `${YM}.from(${q(a)}).equals(${q(b)})`);
const cmpMd = ["02-29", "02-28", "03-01", "--02-29", "2024-02-29", "2023-02-28", "12-31", "01-01", "02-29[u-ca=gregory]", "1972-02-29"];
for (const a of cmpMd) for (const b of cmpMd) add(`${MD}.from(${q(a)}).equals(${q(b)})`);
add(`${PD}.compare()`, `${PD}.compare("2024-01-01")`, `${PD}.compare({year: 2024, month: 1, day: 1}, {year: 2024, month: 1, day: 2})`, `${PD}.compare({year: 2024, month: 1, day: 1}, {year: 2024, month: 13, day: 1})`,
  `${PD}.from("2024-02-29").equals({year: 2024, month: 2, day: 29})`, `${PD}.from("2024-02-29").equals({year: 2024, month: 2, day: 29, calendar: "iso8601"})`, `${PD}.from("2024-02-29").equals({year: 2024, monthCode: "M02", day: 29})`,
  `${PDT}.compare({year: 2024, month: 1, day: 1, hour: 1}, {year: 2024, month: 1, day: 1, hour: 2})`, `${PDT}.from("2024-02-29T10:30").equals({year: 2024, month: 2, day: 29, hour: 10, minute: 30})`,
  `${YM}.compare({year: 2024, month: 1}, {year: 2024, month: 2})`, `${YM}.from("2024-02").equals({year: 2024, month: 2})`, `${MD}.from("02-29").equals({month: 2, day: 29})`, `${MD}.from("02-29").equals({monthCode: "M02", day: 29})`,
  `${MD}.from("02-29").equals({month: 2, day: 29, year: 2023})`, `[${PD}.from("2024-03-01"), ${PD}.from("2024-02-29"), ${PD}.from("2023-12-31")].sort(${PD}.compare).join()`,
  `[${PDT}.from("2024-03-01T10:00"), ${PDT}.from("2024-03-01T09:00")].sort(${PDT}.compare).join()`);

// ---------- toString com opções ----------
const dtStrs = ["2024-02-29T10:30:45.123456789", "2024-02-29T10:30:45.12", "2024-02-29T10:30", "2024-02-29T00:00:00.000000001", "2024-02-29T23:59:59.999999999", "2024-02-29T10:30:45.5", "2024-02-29T10:30:00", "2024-02-29T10:30:45.999999999", "2024-02-29T12:00:00.0005"];
const strOpts = [
  "", "{}", "{calendarName: 'auto'}", "{calendarName: 'always'}", "{calendarName: 'never'}", "{calendarName: 'critical'}", "{calendarName: 'bad'}", "{calendarName: undefined}", "{calendarName: 1}",
  "{fractionalSecondDigits: 'auto'}", "{fractionalSecondDigits: 0}", "{fractionalSecondDigits: 1}", "{fractionalSecondDigits: 2}", "{fractionalSecondDigits: 3}", "{fractionalSecondDigits: 4}", "{fractionalSecondDigits: 5}", "{fractionalSecondDigits: 6}",
  "{fractionalSecondDigits: 7}", "{fractionalSecondDigits: 8}", "{fractionalSecondDigits: 9}", "{fractionalSecondDigits: 10}", "{fractionalSecondDigits: -1}", "{fractionalSecondDigits: 1.9}", "{fractionalSecondDigits: '3'}", "{fractionalSecondDigits: 'x'}", "{fractionalSecondDigits: NaN}", "{fractionalSecondDigits: null}",
  "{smallestUnit: 'minute'}", "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'microsecond'}", "{smallestUnit: 'nanosecond'}", "{smallestUnit: 'minutes'}", "{smallestUnit: 'seconds'}", "{smallestUnit: 'hour'}", "{smallestUnit: 'day'}", "{smallestUnit: 'auto'}", "{smallestUnit: 'bad'}", "{smallestUnit: undefined}",
  "{smallestUnit: 'minute', fractionalSecondDigits: 3}", "{smallestUnit: 'second', fractionalSecondDigits: 3}", "{smallestUnit: 'millisecond', fractionalSecondDigits: 0}",
  "{roundingMode: 'trunc'}", "{roundingMode: 'ceil', fractionalSecondDigits: 2}", "{roundingMode: 'floor', fractionalSecondDigits: 2}", "{roundingMode: 'halfExpand', fractionalSecondDigits: 2}", "{roundingMode: 'halfEven', fractionalSecondDigits: 1}",
  "{roundingMode: 'expand', smallestUnit: 'minute'}", "{roundingMode: 'ceil', smallestUnit: 'minute'}", "{roundingMode: 'halfTrunc', smallestUnit: 'second'}", "{roundingMode: 'halfCeil', smallestUnit: 'second'}", "{roundingMode: 'halfFloor', smallestUnit: 'second'}", "{roundingMode: 'bad'}",
  "{calendarName: 'always', fractionalSecondDigits: 3}", "{calendarName: 'critical', smallestUnit: 'minute'}", "5", "null", "'always'", "[]",
];
for (const s of dtStrs) for (const o of strOpts) add(`${PDT}.from(${q(s)}).toString(${o})`);
for (const s of ["2024-02-29", "+275760-09-13", "-271821-04-19", "0000-01-01", "-000001-12-31"]) {
  for (const o of ["", "{calendarName: 'auto'}", "{calendarName: 'always'}", "{calendarName: 'never'}", "{calendarName: 'critical'}", "{calendarName: 'bad'}", "{fractionalSecondDigits: 3}", "{smallestUnit: 'second'}", "5", "null", "{calendarName: ''}", "{calendarName: 'ALWAYS'}"]) {
    add(`${PD}.from(${q(s)}).toString(${o})`, `${PD}.from(${q(s)}).toJSON(${o})`, `${PD}.from(${q(s)}).toLocaleString === undefined`);
  }
}
for (const s of ["2024-02", "-000001-12", "+275760-09", "0000-01", "-271821-04"]) for (const o of ["", "{calendarName: 'always'}", "{calendarName: 'critical'}", "{calendarName: 'never'}", "{calendarName: 'bad'}", "{calendarName: 'auto'}", "5"]) add(`${YM}.from(${q(s)}).toString(${o})`);
for (const s of ["02-29", "12-31", "01-01"]) for (const o of ["", "{calendarName: 'always'}", "{calendarName: 'critical'}", "{calendarName: 'never'}", "{calendarName: 'bad'}", "{calendarName: 'auto'}", "5"]) add(`${MD}.from(${q(s)}).toString(${o})`);
add(`${PDT}.from("2024-02-29T10:30").toJSON()`, `${PDT}.from("2024-02-29T10:30:45.5").toJSON()`, `${PDT}.from("+275760-09-13T23:59:59.999999999").toString()`, `${PDT}.from("-000001-01-01T00:00").toString()`, `${PDT}.from("0000-01-01T00:00").toString()`, `${PDT}.from("10000-01-01T00:00").toString()`);

// ---------- PlainDateTime round ----------
for (const s of ["2024-02-29T10:30:45.123456789", "2024-02-29T23:59:59.999999999", "2024-12-31T12:00", "2024-02-29T11:59:59.5", "2024-02-29T00:00"]) {
  for (const su of ["day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "days", "month", "bad"]) {
    for (const m of ["halfExpand", "ceil", "floor", "trunc", "expand", "halfEven"]) add(`${PDT}.from(${q(s)}).round({smallestUnit: ${q(su)}, roundingMode: ${q(m)}}).toString()`);
    add(`${PDT}.from(${q(s)}).round(${q(su)}).toString()`);
  }
  for (const [su, incs] of [["hour", [1, 2, 3, 4, 6, 8, 12, 24, 5]], ["minute", [1, 5, 15, 30, 60, 7]], ["second", [1, 30, 60]], ["millisecond", [1, 500, 1000]], ["day", [1, 2]]]) for (const inc of incs) add(`${PDT}.from(${q(s)}).round({smallestUnit: ${q(su)}, roundingIncrement: ${inc}}).toString()`);
  add(`${PDT}.from(${q(s)}).round()`, `${PDT}.from(${q(s)}).round({})`, `${PDT}.from(${q(s)}).round(5)`, `${PDT}.from(${q(s)}).round(null)`);
}

// ---------- Duration com relativeTo PlainDate ----------
const rels = [`"2024-01-31"`, `"2024-02-29"`, `"2023-02-28"`, `"2024-03-01"`, `"2023-12-31"`, `"2000-01-01"`, `{year: 2024, month: 1, day: 31}`, `${PD}.from("2024-01-31")`, `"2024-01-31T12:00"`, `${PDT}.from("2024-01-31T12:00")`, `"2024-01-31[u-ca=gregory]"`, `"2024-01-31T00:00[UTC]"`, `"bad"`, `"2024-02-30"`, `{year: 2024}`, `5`, `null`, `undefined`];
const roundDurs = ["P1Y2M3W4DT5H6M7S", "P1M", "P12M", "P13M", "P1Y", "P1Y12M", "P30D", "P31D", "P365D", "P366D", "P10W", "PT36H", "PT24H", "PT25H", "P1M30D", "P1Y1M1D", "-P1M", "-P1Y2M3D", "PT0S", "P1M1DT12H", "P2M15D", "P100D", "P400D", "P1Y6M", "P5M20D", "PT100H"];
const roundOpts = [
  "{smallestUnit: 'years'}", "{smallestUnit: 'months'}", "{smallestUnit: 'weeks'}", "{smallestUnit: 'days'}", "{smallestUnit: 'hours'}", "{largestUnit: 'years'}", "{largestUnit: 'months'}", "{largestUnit: 'weeks'}", "{largestUnit: 'days'}", "{largestUnit: 'hours'}",
  "{largestUnit: 'years', smallestUnit: 'months'}", "{largestUnit: 'months', smallestUnit: 'weeks'}", "{largestUnit: 'years', smallestUnit: 'days'}", "{largestUnit: 'months', smallestUnit: 'days', roundingMode: 'floor'}", "{largestUnit: 'years', smallestUnit: 'years', roundingMode: 'ceil'}",
  "{smallestUnit: 'months', roundingMode: 'trunc'}", "{smallestUnit: 'months', roundingMode: 'halfExpand'}", "{smallestUnit: 'years', roundingMode: 'halfEven'}", "{smallestUnit: 'weeks', roundingIncrement: 2}", "{smallestUnit: 'months', roundingIncrement: 4}", "{smallestUnit: 'days', roundingIncrement: 10}",
  "{largestUnit: 'auto', smallestUnit: 'days'}", "{largestUnit: 'days', smallestUnit: 'months'}", "{}", "'months'", "'years'",
];
for (const d of roundDurs) {
  for (const o of roundOpts) {
    add(`${D}.from(${q(d)}).round({...${o}, relativeTo: ${rels[0]}}).toString()`, `${D}.from(${q(d)}).round({...${o}, relativeTo: ${rels[1]}}).toString()`);
  }
  for (const r of rels) {
    add(`${D}.from(${q(d)}).round({largestUnit: "months", relativeTo: ${r}}).toString()`, `${D}.from(${q(d)}).round({largestUnit: "years", smallestUnit: "days", relativeTo: ${r}}).toString()`, `${D}.from(${q(d)}).total({unit: "days", relativeTo: ${r}})`);
  }
  for (const u of ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "nanoseconds", "year", "bad"]) {
    add(`${D}.from(${q(d)}).total({unit: ${q(u)}, relativeTo: ${rels[0]}})`, `${D}.from(${q(d)}).total({unit: ${q(u)}, relativeTo: ${rels[1]}})`, `${D}.from(${q(d)}).total({unit: ${q(u)}, relativeTo: ${rels[2]}})`);
  }
}
for (const d of roundDurs.slice(0, 12)) {
  for (const u of ["years", "months", "weeks", "days", "hours"]) {
    for (const r of rels.slice(0, 12)) add(`${D}.from(${q(d)}).total({unit: ${q(u)}, relativeTo: ${r}})`);
    add(`${D}.from(${q(d)}).total(${q(u)})`);
  }
  add(`${D}.from(${q(d)}).total()`, `${D}.from(${q(d)}).total({})`, `${D}.from(${q(d)}).total(5)`, `${D}.from(${q(d)}).round()`, `${D}.from(${q(d)}).round({})`, `${D}.from(${q(d)}).round(5)`, `${D}.from(${q(d)}).round({relativeTo: ${rels[0]}})`);
}
for (const [a, b] of [["P1M", "P30D"], ["P1M", "P31D"], ["P1Y", "P365D"], ["P1Y", "P366D"], ["P1M", "P28D"], ["P1M", "P29D"], ["P12M", "P1Y"], ["P4W", "P28D"], ["P1W", "P7D"], ["-P1M", "-P30D"], ["P1Y1M", "P13M"], ["P2M", "P60D"], ["P1M", "P1M"]]) {
  for (const r of rels.slice(0, 12)) add(`${D}.compare(${q(a)}, ${q(b)}, {relativeTo: ${r}})`);
  add(`${D}.compare(${q(a)}, ${q(b)})`);
  for (const r of rels.slice(0, 5)) add(`${D}.from(${q(a)}).add(${q(b)}, {relativeTo: ${r}}).toString()`, `${D}.from(${q(a)}).subtract(${q(b)}, {relativeTo: ${r}}).toString()`);
}
// Duration.from e Duration em combinação com PlainDate
for (const d of ["P1Y2M3W4DT5H6M7.008009010S", "P1Y", "-P1Y1M", "PT0S", "P99999999D", "PT1.5S", "P1.5Y", "P1Y2M3W", "+P1D", "p1d", "P1D2Y", "PT", "P", "-PT1H30M", "PT0.000000001S", "PT9007199254740991S", "P4294967296Y", "P4294967295Y"]) {
  add(`${D}.from(${q(d)}).toString()`, `${D}.from(${q(d)}).negated().toString()`, `${PD}.from("2024-01-31").add(${q(d)}).toString()`, `${PD}.from("2024-01-31").subtract(${q(d)}).toString()`, `${PDT}.from("2024-01-31T12:00").add(${q(d)}).toString()`);
}
// larguras e alvos relativos que cruzam fim de mês e ano bissexto
for (const r of ["2024-01-31", "2024-02-29", "2023-03-31", "2023-12-31", "2020-02-29", "2021-02-28"]) {
  for (const d of ["P1M", "P2M", "P12M", "P1Y", "P4Y", "P1M30D", "P1M31D", "P29D", "P30D", "P31D", "P59D", "P60D", "P365D", "P366D", "P1461D", "P1Y1M"]) {
    for (const lu of ["years", "months", "weeks", "days"]) add(`${D}.from(${q(d)}).round({largestUnit: ${q(lu)}, relativeTo: ${q(r)}}).toString()`);
    add(`${D}.from(${q(d)}).total({unit: "months", relativeTo: ${q(r)}})`, `${D}.from(${q(d)}).total({unit: "years", relativeTo: ${q(r)}})`, `${D}.from(${q(d)}).total({unit: "weeks", relativeTo: ${q(r)}})`);
  }
}

// ---------- execução ----------
const programs = exprs.filter((e) => !/Temporal\.Now/.test(e)).map((e) => PRELUDE + `globalThis.R = T(()=>${e})`).filter((p) => !usesHostApi(p));
const known = new Set();
for (const file of fs.readdirSync(require("path").join(__dirname, "..", "tests", "golden"))) {
  if (!/^temporal_.*_bun\.tsv$/.test(file) || file === "temporal_plain_bun.tsv") continue;
  for (const p of knownPrograms("temporal_plain_bun.tsv", [file])) known.add(p);
}
let dup = 0;
const todo = programs.filter((p) => (known.has(p) ? (dup++, false) : true));

function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 8000);
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", () => {});
    child.on("close", (code) => { clearTimeout(timer); const decoded = code === 0 && !timedOut ? decodeResult(out) : null; resolve({ ok: decoded !== null, out: decoded, timedOut }); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(todo.length);
  let next = 0;
  async function worker() {
    while (next < todo.length) {
      const i = next++;
      results[i] = await runChild(todo[i]);
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < todo.length; i++) {
    const r = results[i];
    if (!r.ok || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || r.out.includes(String.fromCharCode(0x2014))) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(todo[i].slice(todo[i].indexOf("globalThis.R"))).slice(0, 160) + (r.timedOut ? " timeout" : "") + "\n");
      continue;
    }
    lines.push(JSON.stringify(todo[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("temporal_plain", lines));
  process.stderr.write(`mantidos ${lines.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
