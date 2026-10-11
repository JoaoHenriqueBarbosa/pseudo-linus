// Gera tests/golden/temporal_bun.tsv: programas de Temporal (sem Intl) avaliados no bun 1.4.2.
// Linha: fonte<TAB>resultado, onde resultado é `ok<TAB>valor` ou `error<TAB>name<TAB>message JSON`.
// O harness é tests/golden/temporal_bun_harness.js, o mesmo texto que tests/temporal_bun_golden.rs embute.
// Uso: TZ=UTC bun scripts/gen-temporal-golden.js   (escreve o .tsv ao lado dos outros goldens)
const fs = require("fs");
const path = require("path");

const harness = (0, eval)(fs.readFileSync(path.join(__dirname, "../tests/golden/temporal_bun_harness.js"), "utf8").trimEnd());
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

const D = "Temporal.Duration";
const PD = "Temporal.PlainDate";
const PT = "Temporal.PlainTime";
const PDT = "Temporal.PlainDateTime";
const PYM = "Temporal.PlainYearMonth";
const PMD = "Temporal.PlainMonthDay";
const I = "Temporal.Instant";
const Z = "Temporal.ZonedDateTime";

// ---------- Duration ----------
const durations = [
  "P1Y2M3W4DT5H6M7.008009S", "PT0S", "P1D", "-P1D", "PT1H", "PT90M", "PT36H", "P1Y", "P12M", "P1M", "P4W",
  "PT0.5S", "PT0.000000001S", "-PT1.5S", "P1DT12H", "PT59.999999999S", "P10Y6M", "PT100S", "PT3600S",
];
const unitsAll = ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"];
for (const d of durations) {
  add(`${D}.from("${d}").toString()`, `JSON.stringify(${D}.from("${d}"))`, `${D}.from("${d}").negated().toString()`,
    `${D}.from("${d}").abs().toString()`, `${D}.from("${d}").sign`, `${D}.from("${d}").blank`);
}
for (const u of unitsAll) {
  add(`${D}.from({${u}: 3}).toString()`, `${D}.from({${u}: -3}).toString()`, `${D}.from({${u}: 3}).${u}`,
    `${D}.from({${u}: 1.5}).toString()`, `${D}.from({${u}: Infinity}).toString()`,
    `${D}.from({${u}: 1}).with({${u}: 7}).toString()`, `${D}.from({years: 1, ${u}: 2}).add({${u}: 5}).toString()`);
}
add(
  `${D}.from({}).toString()`, `${D}.from(5).toString()`, `${D}.from("").toString()`, `${D}.from("P").toString()`,
  `${D}.from("PT").toString()`, `${D}.from("P1H").toString()`, `${D}.from("PT1D").toString()`, `${D}.from("p1d").toString()`,
  `${D}.from("P1,5D").toString()`, `${D}.from("PT1,5S").toString()`, `${D}.from("PT0.5H").toString()`, `${D}.from("PT1.5H30M").toString()`,
  `${D}.from("P1Y-1M").toString()`, `${D}.from({years: 1, months: -1})`, `${D}.from({hours: NaN})`, `${D}.from({days: "2"}).toString()`,
  `${D}.from({days: 1.5})`, `${D}.from(undefined)`, `${D}.from(null)`, `${D}.from(true)`, `${D}.from(Symbol())`, `${D}.from({foo: 1})`,
  `new ${D}().toString()`, `new ${D}(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).toString()`, `new ${D}(1.5)`, `new ${D}(-1, 1)`, `${D}(1)`,
  `new ${D}(Infinity)`, `new ${D}("3").toString()`, `new ${D}(0, 0, 0, 0, 0, 0, 0, 0, 0, 9007199254740993).toString()`,
  `${D}.from({years: 2 ** 32}).toString()`, `${D}.from({years: 2 ** 32 - 1}).toString()`, `${D}.from({days: 2 ** 53}).toString()`,
  `${D}.from({seconds: Number.MAX_SAFE_INTEGER}).toString()`, `${D}.from({milliseconds: 9007199254740991, seconds: 9007199254740991}).toString()`,
  `${D}.from({nanoseconds: 9007199254740991, microseconds: 9007199254740991}).toString()`, `${D}.from({hours: 2 ** 53})`,
  `${D}.from({hours: 1}).with({})`, `${D}.from({hours: 1}).with({years: 1, hours: undefined})`, `${D}.from({hours: 1}).with(5)`,
  `${D}.from({hours: 1}).with({sign: 1})`, `${D}.from("PT1H").with("PT2H")`,
);
const addPairs = [
  ["PT1H", "PT30M"], ["PT1H", "-PT90M"], ["P1D", "PT24H"], ["PT0.9S", "PT0.2S"], ["P1Y", "P1M"], ["P1M", "P30D"], ["PT1H", "P1D"],
  ["P1W", "P7D"], ["PT1S", "PT0.999999999S"], ["-PT1S", "PT1S"], ["P1DT1H", "-P1D"], ["PT36H", "PT36H"],
];
for (const [a, b] of addPairs) {
  add(`${D}.from("${a}").add("${b}").toString()`, `${D}.from("${a}").subtract("${b}").toString()`, `${D}.compare("${a}", "${b}")`,
    `${D}.compare("${a}", "${b}", {relativeTo: "2024-01-31"})`, `${D}.compare("${a}", "${b}", {relativeTo: "2024-03-10T00:00:00[America/New_York]"})`,
    `${D}.compare("${a}", "${b}", {relativeTo: "2024-03-10T00:00:00[America/Sao_Paulo]"})`);
}
add(`${D}.from("PT1H").add()`, `${D}.from("PT1H").add({})`, `${D}.from("PT1H").add("xx")`, `${D}.compare()`, `${D}.compare("PT1H")`,
  `${D}.from("P1M").add("P1D")`, `${D}.from("P1M").add("P1M").toString()`, `${D}.compare("P1M", "P30D")`, `${D}.compare("P1Y", "P365D")`,
  `${D}.compare("P1D", "PT24H")`, `${D}.compare("P1W", "P7D")`);
// round
const roundCases = [
  ["PT90M", "{smallestUnit: 'hour'}"], ["PT90M", "{smallestUnit: 'hour', roundingMode: 'floor'}"],
  ["PT90M", "{smallestUnit: 'hour', roundingMode: 'ceil'}"], ["PT90M", "{smallestUnit: 'hour', roundingMode: 'trunc'}"],
  ["PT90M", "{smallestUnit: 'hour', roundingMode: 'halfEven'}"], ["PT150M", "{smallestUnit: 'hour', roundingMode: 'halfEven'}"],
  ["PT150M", "{smallestUnit: 'hour', roundingMode: 'halfExpand'}"], ["PT150M", "{smallestUnit: 'hour', roundingMode: 'halfTrunc'}"],
  ["PT150M", "{smallestUnit: 'hour', roundingMode: 'halfCeil'}"], ["PT150M", "{smallestUnit: 'hour', roundingMode: 'halfFloor'}"],
  ["-PT150M", "{smallestUnit: 'hour', roundingMode: 'halfExpand'}"], ["-PT150M", "{smallestUnit: 'hour', roundingMode: 'halfCeil'}"],
  ["-PT150M", "{smallestUnit: 'hour', roundingMode: 'halfFloor'}"], ["-PT150M", "{smallestUnit: 'hour', roundingMode: 'halfTrunc'}"],
  ["-PT90M", "{smallestUnit: 'hour', roundingMode: 'ceil'}"], ["-PT90M", "{smallestUnit: 'hour', roundingMode: 'floor'}"],
  ["PT1H50M30S", "{smallestUnit: 'minute', roundingIncrement: 15}"], ["PT1H50M30S", "{smallestUnit: 'second', roundingIncrement: 7}"],
  ["PT1H50M30S", "{largestUnit: 'minute'}"], ["PT1H50M30S", "{largestUnit: 'second', smallestUnit: 'second'}"],
  ["PT100S", "{largestUnit: 'minute'}"], ["PT100S", "{largestUnit: 'hour', smallestUnit: 'minute'}"],
  ["PT36H", "{largestUnit: 'day'}"], ["PT36H", "{largestUnit: 'day', smallestUnit: 'day', roundingMode: 'ceil'}"],
  ["PT36H", "{largestUnit: 'auto'}"], ["P1DT12H", "{largestUnit: 'hour'}"], ["PT0.5S", "{smallestUnit: 'millisecond'}"],
  ["PT0.0005S", "{smallestUnit: 'millisecond', roundingMode: 'ceil'}"], ["PT1.123456789S", "{smallestUnit: 'microsecond'}"],
  ["PT1.123456789S", "{smallestUnit: 'nanosecond'}"], ["PT1.123456789S", "{smallestUnit: 'millisecond', roundingIncrement: 500}"],
  ["PT1H", "{smallestUnit: 'hour', roundingIncrement: 24}"], ["PT1H", "{smallestUnit: 'hour', roundingIncrement: 5}"],
  ["PT1H", "{smallestUnit: 'minute', roundingIncrement: 60}"], ["PT1H", "{smallestUnit: 'minute', roundingIncrement: 0}"],
  ["PT1H", "{smallestUnit: 'minute', roundingIncrement: -1}"], ["PT1H", "{smallestUnit: 'minute', roundingIncrement: 1e9}"],
  ["PT1H", "{smallestUnit: 'minute', roundingIncrement: NaN}"], ["PT1H", "{smallestUnit: 'minute', roundingIncrement: 1.9}"],
  ["PT1H", "{}"], ["PT1H", "undefined"], ["PT1H", "'hour'"], ["PT1H", "{smallestUnit: 'hours'}"], ["PT1H", "{smallestUnit: 'bogus'}"],
  ["PT1H", "{smallestUnit: 'hour', largestUnit: 'minute'}"], ["PT1H", "{smallestUnit: 'minute', largestUnit: 'minute'}"],
  ["PT1H", "{roundingMode: 'bogus', smallestUnit: 'hour'}"], ["PT1H", "{smallestUnit: 'hour', roundingMode: 'HALFEXPAND'}"],
  ["P1Y", "{smallestUnit: 'day'}"], ["P1M", "{smallestUnit: 'day'}"], ["P1W", "{smallestUnit: 'day'}"], ["P1D", "{smallestUnit: 'hour', largestUnit: 'day'}"],
  ["P1Y", "{largestUnit: 'month'}"], ["P1Y", "{smallestUnit: 'month'}"], ["P1D", "{largestUnit: 'week'}"], ["PT0S", "{smallestUnit: 'second'}"],
  ["PT1H", "{largestUnit: 'year', smallestUnit: 'year'}"],
];
for (const [d, o] of roundCases) add(`${D}.from("${d}").round(${o}).toString()`);
const rels = [
  "'2024-01-31'", "'2024-02-01'", "'2023-02-01'", "{year: 2024, month: 3, day: 10}", "'2024-03-10T00:00:00[America/New_York]'",
  "'2024-11-03T00:00:00[America/New_York]'", "'2024-03-10T12:00:00'", "Temporal.PlainDate.from('2024-01-31')",
  "Temporal.PlainDateTime.from('2024-01-31T12:00')", "Temporal.ZonedDateTime.from('2024-03-10T00:00[America/New_York]')",
  "Temporal.ZonedDateTime.from('2018-10-21T00:00[America/Sao_Paulo]')", "'2024-01-31T00:00Z'", "'2024-01-31T00:00+05:30[Asia/Kolkata]'",
  "{year: 2024, month: 1, day: 31, timeZone: 'Europe/Berlin'}", "{year: 2024}", "'bogus'", "5", "null", "undefined", "{}",
];
const relDurations = [
  "P1M", "P1Y", "P1Y2M3D", "P30D", "P365D", "PT24H", "P1W", "P1DT25H", "-P1M", "P2M", "P14M", "P90D", "PT100H", "P1MT36H",
];
for (const r of rels) {
  for (const d of ["P1M", "P1Y2M3D", "P30D", "PT24H", "P1DT25H"]) {
    add(`${D}.from("${d}").round({smallestUnit: 'day', relativeTo: ${r}}).toString()`,
      `${D}.from("${d}").round({largestUnit: 'month', relativeTo: ${r}}).toString()`,
      `${D}.from("${d}").total({unit: 'day', relativeTo: ${r}})`);
  }
}
for (const d of relDurations) {
  for (const r of ["'2024-01-31'", "'2024-03-09T12:00[America/New_York]'", "'2024-02-01'"]) {
    for (const u of ["year", "month", "week", "day", "hour", "minute"]) {
      add(`${D}.from("${d}").total({unit: '${u}', relativeTo: ${r}})`);
    }
    add(`${D}.from("${d}").round({largestUnit: 'year', smallestUnit: 'day', relativeTo: ${r}}).toString()`,
      `${D}.from("${d}").round({largestUnit: 'week', smallestUnit: 'day', relativeTo: ${r}}).toString()`,
      `${D}.from("${d}").round({largestUnit: 'day', smallestUnit: 'hour', roundingMode: 'ceil', relativeTo: ${r}}).toString()`,
      `${D}.from("${d}").round({largestUnit: 'month', smallestUnit: 'month', roundingMode: 'halfExpand', relativeTo: ${r}}).toString()`);
  }
}
// total
for (const d of ["PT90M", "PT1H30M45.5S", "PT0.5S", "PT36H", "-PT90M", "PT0S", "PT1.123456789S", "PT100S"]) {
  for (const u of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "day", "hours"]) add(`${D}.from("${d}").total({unit: '${u}'})`);
  add(`${D}.from("${d}").total('minute')`, `${D}.from("${d}").total()`, `${D}.from("${d}").total({})`, `${D}.from("${d}").total({unit: 'week'})`,
    `${D}.from("${d}").total({unit: 'month'})`);
}
add(`${D}.from("P1D").total({unit: 'hour'})`, `${D}.from("P1W").total({unit: 'day'})`, `${D}.from("P1M").total({unit: 'day'})`,
  `${D}.from("P1Y").total({unit: 'month'})`, `${D}.from("P1D").total({unit: 'hour', relativeTo: '2024-03-10[America/New_York]'})`);
// toString
const tsOpts = [
  "{}", "{fractionalSecondDigits: 0}", "{fractionalSecondDigits: 3}", "{fractionalSecondDigits: 9}", "{fractionalSecondDigits: 'auto'}",
  "{fractionalSecondDigits: 10}", "{fractionalSecondDigits: -1}", "{fractionalSecondDigits: 2.9}", "{fractionalSecondDigits: 'x'}",
  "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'microsecond'}", "{smallestUnit: 'nanosecond'}",
  "{smallestUnit: 'minute'}", "{smallestUnit: 'hour'}", "{smallestUnit: 'day'}", "{smallestUnit: 'seconds'}",
  "{smallestUnit: 'second', roundingMode: 'ceil'}", "{smallestUnit: 'second', roundingMode: 'floor'}",
  "{smallestUnit: 'second', roundingMode: 'halfExpand'}", "{smallestUnit: 'second', roundingMode: 'trunc'}",
  "{fractionalSecondDigits: 1, roundingMode: 'ceil'}", "{fractionalSecondDigits: 1, roundingMode: 'floor'}",
  "{fractionalSecondDigits: 1, roundingMode: 'halfEven'}", "{fractionalSecondDigits: 0, roundingMode: 'halfExpand'}",
  "{smallestUnit: 'millisecond', fractionalSecondDigits: 1}", "{roundingMode: 'bogus'}", "undefined", "'second'", "5",
];
for (const d of ["PT1.987654321S", "-PT1.987654321S", "P1Y2M3DT4H5M6.5S", "PT0S", "PT59.9999S", "PT1.5S", "PT0.05S", "PT1H", "P1D", "PT90M"]) {
  for (const o of tsOpts) add(`${D}.from("${d}").toString(${o})`);
}
add(`${D}.from("PT1.5S").toJSON()`, `${D}.from("PT1.5S").valueOf()`, `${D}.from("PT1.5S") + ""`, `${D}.from("PT1.5S") < ${D}.from("PT2S")`);

// ---------- PlainDate ----------
const dates = ["2024-01-31", "2024-02-29", "2023-02-28", "2024-12-31", "2000-01-01", "1999-12-31", "0001-01-01", "-000001-01-01", "+010000-01-01", "1970-01-01", "2021-01-03", "2020-12-31", "2026-10-08"];
const dateProps = ["year", "month", "monthCode", "day", "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "calendarId", "era", "eraYear"];
for (const d of dates) {
  for (const p of dateProps) add(`${PD}.from("${d}").${p}`);
  add(`${PD}.from("${d}").toString()`, `JSON.stringify(${PD}.from("${d}"))`, `${PD}.from("${d}").toPlainDateTime().toString()`,
    `${PD}.from("${d}").toPlainYearMonth().toString()`, `${PD}.from("${d}").toPlainMonthDay().toString()`,
    `${PD}.from("${d}").toString({calendarName: 'always'})`, `${PD}.from("${d}").toString({calendarName: 'never'})`,
    `${PD}.from("${d}").toString({calendarName: 'critical'})`, `${PD}.from("${d}").toString({calendarName: 'auto'})`,
    `${PD}.from("${d}").toPlainDateTime("12:30").toString()`, `${PD}.from("${d}").withCalendar("iso8601").toString()`,
    `${PD}.from("${d}").toZonedDateTime("America/Sao_Paulo").toString()`,
    `${PD}.from("${d}").toZonedDateTime({timeZone: "Asia/Kolkata", plainTime: "10:00"}).toString()`);
}
add(
  `${PD}.from("2024-01-31T10:00").toString()`, `${PD}.from("2024-01-31T10:00Z").toString()`, `${PD}.from("2024-01-31T10:00+01:00[Europe/Berlin]").toString()`,
  `${PD}.from("20240131").toString()`, `${PD}.from("2024-1-31")`, `${PD}.from("2024-02-30")`, `${PD}.from("2023-02-29")`, `${PD}.from("2024-13-01")`,
  `${PD}.from("2024-00-10")`, `${PD}.from("2024-01-00")`, `${PD}.from("+275760-09-13").toString()`, `${PD}.from("+275760-09-14")`,
  `${PD}.from("-271821-04-19").toString()`, `${PD}.from("-271821-04-18")`, `${PD}.from("-000000-01-01")`, `${PD}.from("")`, `${PD}.from("bogus")`,
  `${PD}.from("2024-01-31[u-ca=iso8601]").toString()`, `${PD}.from("2024-01-31[u-ca=iso8601]").toString({calendarName: 'always'})`,
  `${PD}.from("2024-01-31[u-ca=bogus]")`, `${PD}.from("2024-01-31[!u-ca=iso8601]").toString()`, `${PD}.from("2024-01-31[foo=bar]")`,
  `${PD}.from("2024-01-31[!foo=bar]")`, `${PD}.from("2024-01-31Z")`, `${PD}.from("2024-01-31T25:00")`, `${PD}.from("2024-01-31T10:60")`,
  `${PD}.from({year: 2024, month: 1, day: 31}).toString()`, `${PD}.from({year: 2024, monthCode: "M02", day: 29}).toString()`,
  `${PD}.from({year: 2024, month: 2, monthCode: "M03", day: 1})`, `${PD}.from({year: 2024, month: 13, day: 1})`,
  `${PD}.from({year: 2024, month: 13, day: 1}, {overflow: 'constrain'}).toString()`, `${PD}.from({year: 2024, month: 13, day: 1}, {overflow: 'reject'})`,
  `${PD}.from({year: 2023, month: 2, day: 31}).toString()`, `${PD}.from({year: 2023, month: 2, day: 31}, {overflow: 'reject'})`,
  `${PD}.from({year: 2023, month: 2, day: 31}, {overflow: 'bogus'})`, `${PD}.from({year: 2023, month: 0, day: 1})`, `${PD}.from({year: 2023, month: 1, day: 0})`,
  `${PD}.from({year: 2023, month: 1, day: 0}, {overflow: 'constrain'})`, `${PD}.from({year: 2023, month: 1})`, `${PD}.from({year: 2023, day: 1})`,
  `${PD}.from({month: 1, day: 1})`, `${PD}.from({year: 2023, monthCode: "M13", day: 1})`, `${PD}.from({year: 2023, monthCode: "M00", day: 1})`,
  `${PD}.from({year: 2023, monthCode: "M01L", day: 1})`, `${PD}.from({year: 2023, monthCode: "m01", day: 1})`, `${PD}.from({year: 2023, monthCode: 1, day: 1})`,
  `${PD}.from({year: 2023.5, month: 1, day: 1}).toString()`, `${PD}.from({year: "2023", month: "1", day: "1"}).toString()`, `${PD}.from({year: Infinity, month: 1, day: 1})`,
  `${PD}.from({year: 2023, month: 1, day: 1, calendar: "iso8601"}).toString()`, `${PD}.from({year: 2023, month: 1, day: 1, calendar: "bogus"})`,
  `${PD}.from({year: 2023, month: 1, day: 1, era: "ce", eraYear: 2023}).toString()`, `${PD}.from(5)`, `${PD}.from(null)`, `${PD}.from(undefined)`, `${PD}.from(true)`,
  `${PD}.from(${PD}.from("2024-01-31"), {overflow: 'bogus'})`, `${PD}.from(${PD}.from("2024-01-31"), {overflow: 'reject'}).toString()`,
  `${PD}.from(${PDT}.from("2024-01-31T10:00")).toString()`, `${PD}.from(${Z}.from("2024-01-31T10:00[Europe/Berlin]")).toString()`,
  `new ${PD}(2024, 1, 31).toString()`, `new ${PD}(2024, 2, 30)`, `new ${PD}(2024, 13, 1)`, `new ${PD}(2024)`, `new ${PD}()`, `${PD}(2024, 1, 1)`,
  `new ${PD}(2024.9, 1.9, 1.9).toString()`, `new ${PD}("2024", "1", "1").toString()`, `new ${PD}(2024, 1, 1, "iso8601").toString()`, `new ${PD}(2024, 1, 1, "bogus")`,
  `new ${PD}(2024, 1, 1, "ISO8601").toString()`, `new ${PD}(275760, 9, 13).toString()`, `new ${PD}(275760, 9, 14)`, `new ${PD}(Infinity, 1, 1)`,
  `${PD}.compare("2024-01-31", "2024-02-01")`, `${PD}.compare("2024-02-01", "2024-01-31")`, `${PD}.compare("2024-01-31", "2024-01-31T12:00")`,
  `${PD}.compare("2024-01-31", {year: 2024, month: 1, day: 31})`, `${PD}.compare("2024-01-31")`, `${PD}.compare()`,
  `${PD}.from("2024-01-31").equals("2024-01-31")`, `${PD}.from("2024-01-31").equals("2024-02-01")`, `${PD}.from("2024-01-31").equals()`,
  `${PD}.from("2024-01-31").valueOf()`, `${PD}.from("2024-01-31") < ${PD}.from("2024-02-01")`, `${PD}.from("2024-01-31").toLocaleString === undefined`,
);
// with
for (const w of ["{year: 2025}", "{month: 2}", "{day: 15}", "{monthCode: 'M02'}", "{month: 2, day: 30}", "{month: 13}", "{day: 0}", "{day: 32}", "{}", "5", "'x'", "{calendar: 'iso8601'}",
  "{timeZone: 'UTC'}", "{year: 2025, era: 'ce'}", "{month: 2, monthCode: 'M03'}", "{day: 40}, {overflow: 'reject'}", "{month: 2, day: 30}, {overflow: 'reject'}",
  "{month: 2, day: 30}, {overflow: 'constrain'}", "{day: 15}, {overflow: 'bogus'}", "{hour: 5}", "{day: 15}, 5", "{day: 15}, null", "{monthCode: 'M01L'}", "{year: 2023}"]) {
  add(`${PD}.from("2024-01-31").with(${w})`, `${PD}.from("2024-02-29").with(${w})`);
}
// add/subtract/since/until
const dateDurs = ["P1D", "P1M", "P1Y", "P1W", "P1Y1M1D", "-P1D", "-P1M", "P1M30D", "PT24H", "P1DT12H", "PT0S", "P100Y", "P12M", "P1M1W", "P2M"];
for (const d of ["2024-01-31", "2024-02-29", "2023-03-31", "2024-12-31", "2000-02-29"]) {
  for (const dur of dateDurs) {
    add(`${PD}.from("${d}").add("${dur}").toString()`, `${PD}.from("${d}").subtract("${dur}").toString()`);
  }
  add(`${PD}.from("${d}").add({months: 1}, {overflow: 'reject'}).toString()`, `${PD}.from("${d}").add({months: 1}, {overflow: 'constrain'}).toString()`,
    `${PD}.from("${d}").add({years: 1}, {overflow: 'reject'}).toString()`, `${PD}.from("${d}").add({days: 1}, {overflow: 'bogus'})`);
}
add(`${PD}.from("2024-01-31").add()`, `${PD}.from("2024-01-31").add({})`, `${PD}.from("2024-01-31").add(5)`, `${PD}.from("2024-01-31").add("bogus")`,
  `${PD}.from("2024-01-31").add({years: 300000})`, `${PD}.from("+275760-09-13").add({days: 1})`, `${PD}.from("-271821-04-19").subtract({days: 1})`,
  `${PD}.from("2024-01-31").add({days: 1e9}).toString()`, `${PD}.from("2024-01-31").add({days: 1e12})`, `${PD}.from("2024-01-31").add({hours: 48}).toString()`);
const pairs = [["2020-01-31", "2024-03-15"], ["2024-03-15", "2020-01-31"], ["2024-01-31", "2024-03-01"], ["2024-03-31", "2024-02-29"], ["2023-12-25", "2024-01-05"],
  ["2024-01-01", "2024-01-01"], ["2000-02-29", "2024-02-29"], ["2024-02-29", "2025-02-28"], ["2024-05-31", "2024-06-30"], ["1970-01-01", "2026-10-08"]];
const diffOpts = ["", "{largestUnit: 'year'}", "{largestUnit: 'month'}", "{largestUnit: 'week'}", "{largestUnit: 'day'}", "{largestUnit: 'auto'}",
  "{smallestUnit: 'month'}", "{largestUnit: 'year', smallestUnit: 'month'}", "{largestUnit: 'year', smallestUnit: 'month', roundingMode: 'ceil'}",
  "{largestUnit: 'year', smallestUnit: 'month', roundingMode: 'halfExpand'}", "{largestUnit: 'year', smallestUnit: 'week', roundingMode: 'halfEven'}",
  "{largestUnit: 'month', smallestUnit: 'week', roundingIncrement: 2}", "{smallestUnit: 'year', roundingMode: 'floor'}", "{smallestUnit: 'year', roundingMode: 'ceil'}",
  "{largestUnit: 'hour'}", "{largestUnit: 'day', smallestUnit: 'hour'}", "{smallestUnit: 'day', roundingIncrement: 10}", "{largestUnit: 'week', smallestUnit: 'day', roundingMode: 'trunc'}",
  "{largestUnit: 'day', smallestUnit: 'month'}", "{largestUnit: 'bogus'}", "{smallestUnit: 'minute'}", "{roundingIncrement: 0}", "5", "{largestUnit: 'years'}"];
for (const [a, b] of pairs) {
  for (const o of diffOpts) add(`${PD}.from("${a}").until("${b}"${o ? ", " + o : ""}).toString()`, `${PD}.from("${a}").since("${b}"${o ? ", " + o : ""}).toString()`);
}
add(`${PD}.from("2024-01-31").until()`, `${PD}.from("2024-01-31").until("bogus")`, `${PD}.from("2024-01-31").until({year: 2025, month: 1, day: 1}).toString()`,
  `${PD}.from("2024-01-31").until("2025-01-31T10:00").toString()`, `${PD}.from("2024-01-31").until(${PDT}.from("2025-01-31T10:00")).toString()`);

// ---------- PlainTime ----------
const times = ["00:00", "12:30", "23:59:59", "23:59:59.999999999", "01:02:03.004005006", "12:00:00.5", "T10", "10", "1030", "103045", "10:30:45,5", "T10:30", "00:00:00.000000001"];
for (const t of times) {
  for (const p of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "calendarId"]) add(`${PT}.from("${t}").${p}`);
  add(`${PT}.from("${t}").toString()`, `JSON.stringify(${PT}.from("${t}"))`);
}
const timeStrOpts = ["{}", "{smallestUnit: 'minute'}", "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'microsecond'}", "{smallestUnit: 'nanosecond'}",
  "{fractionalSecondDigits: 0}", "{fractionalSecondDigits: 2}", "{fractionalSecondDigits: 'auto'}", "{smallestUnit: 'hour'}", "{smallestUnit: 'day'}",
  "{smallestUnit: 'second', roundingMode: 'ceil'}", "{smallestUnit: 'minute', roundingMode: 'floor'}", "{fractionalSecondDigits: 3, roundingMode: 'halfExpand'}",
  "{fractionalSecondDigits: 4}", "{roundingMode: 'bogus'}"];
for (const t of ["12:30:45.123456789", "23:59:59.9999", "00:00:00.5", "12:30:30"]) for (const o of timeStrOpts) add(`${PT}.from("${t}").toString(${o})`);
add(
  `${PT}.from("24:00")`, `${PT}.from("23:60")`, `${PT}.from("23:59:60").toString()`, `${PT}.from("23:59:61")`, `${PT}.from("12")`.replace('"12"', '"1"'),
  `${PT}.from("12:30Z").toString()`, `${PT}.from("12:30+01:00").toString()`, `${PT}.from("2024-01-31T12:30").toString()`, `${PT}.from("2024-01-31").toString()`,
  `${PT}.from("2024-01-31T12:30[Europe/Berlin]").toString()`, `${PT}.from("12:30[u-ca=iso8601]").toString()`, `${PT}.from("")`, `${PT}.from("bogus")`,
  `${PT}.from({hour: 5}).toString()`, `${PT}.from({hour: 5, minute: 6, second: 7, millisecond: 8, microsecond: 9, nanosecond: 10}).toString()`,
  `${PT}.from({}).toString()`, `${PT}.from({foo: 1})`, `${PT}.from({hour: 24})`, `${PT}.from({hour: 24}, {overflow: 'constrain'}).toString()`,
  `${PT}.from({hour: 24}, {overflow: 'reject'})`, `${PT}.from({hour: -1})`, `${PT}.from({hour: -1}, {overflow: 'constrain'}).toString()`, `${PT}.from({minute: 61}, {overflow: 'constrain'}).toString()`,
  `${PT}.from({second: 60}).toString()`, `${PT}.from({millisecond: 1000}, {overflow: 'constrain'}).toString()`, `${PT}.from({hour: 1.9}).toString()`, `${PT}.from({hour: "5"}).toString()`,
  `${PT}.from({hour: NaN})`, `${PT}.from({hour: Infinity})`, `${PT}.from(5)`, `${PT}.from(null)`, `${PT}.from(undefined)`, `${PT}.from(${PDT}.from("2024-01-31T10:20:30")).toString()`,
  `${PT}.from(${Z}.from("2024-01-31T10:20:30[Asia/Kolkata]")).toString()`, `${PT}.from("12:30", {overflow: 'bogus'})`,
  `new ${PT}().toString()`, `new ${PT}(1).toString()`, `new ${PT}(1, 2, 3, 4, 5, 6).toString()`, `new ${PT}(24)`, `new ${PT}(0, 60)`, `new ${PT}(0, 0, 0, 1000)`, `new ${PT}(1.9).toString()`, `${PT}(1)`,
  `new ${PT}(-1)`, `new ${PT}(Infinity)`, `${PT}.compare("12:30", "12:31")`, `${PT}.compare("12:31", "12:30")`, `${PT}.compare("12:30", "12:30")`,
  `${PT}.compare("12:30", {hour: 12, minute: 30})`, `${PT}.compare("12:30")`, `${PT}.from("12:30").equals("12:30")`, `${PT}.from("12:30").equals("12:31")`,
  `${PT}.from("12:30").valueOf()`, `${PT}.from("12:30").toPlainDateTime("2024-01-31").toString()`, `${PT}.from("12:30").toPlainDateTime()`,
  `${PT}.from("12:30").toZonedDateTime`, `typeof ${PT}.from("12:30").toLocaleString`,
);
for (const w of ["{hour: 1}", "{minute: 59}", "{second: 30, millisecond: 5}", "{hour: 25}", "{hour: 25}, {overflow: 'constrain'}", "{hour: 25}, {overflow: 'reject'}", "{}", "5", "'x'",
  "{nanosecond: 999}", "{calendar: 'iso8601'}", "{timeZone: 'UTC'}", "{minute: -1}", "{minute: 5}, {overflow: 'bogus'}"]) add(`${PT}.from("12:30:45").with(${w})`);
for (const d of ["PT1H", "PT30M", "-PT1H", "PT12H", "PT24H", "PT25H", "PT0.5S", "PT0.000000001S", "-PT0.000000001S", "P1D", "P1D", "PT1H1M1.001001001S", "-PT36H", "PT48H30M"]) {
  for (const t of ["12:30", "23:59:59.999999999", "00:00"]) add(`${PT}.from("${t}").add("${d}").toString()`, `${PT}.from("${t}").subtract("${d}").toString()`);
}
add(`${PT}.from("12:30").add("P1Y")`, `${PT}.from("12:30").add("P1M")`, `${PT}.from("12:30").add("P1W")`, `${PT}.from("12:30").add({days: 1}).toString()`, `${PT}.from("12:30").add()`);
const tpairs = [["12:30", "14:45:30.5"], ["14:45:30.5", "12:30"], ["00:00", "23:59:59.999999999"], ["12:00", "12:00"], ["10:00:00.123456789", "10:00:00.987654321"]];
const tdiff = ["", "{largestUnit: 'hour'}", "{largestUnit: 'minute'}", "{largestUnit: 'second'}", "{largestUnit: 'millisecond'}", "{largestUnit: 'nanosecond'}",
  "{smallestUnit: 'minute'}", "{smallestUnit: 'hour', roundingMode: 'ceil'}", "{smallestUnit: 'hour', roundingMode: 'halfExpand'}", "{smallestUnit: 'second', roundingIncrement: 15}",
  "{smallestUnit: 'millisecond', roundingMode: 'floor'}", "{largestUnit: 'minute', smallestUnit: 'minute', roundingMode: 'halfEven'}", "{largestUnit: 'day'}", "{largestUnit: 'week'}",
  "{smallestUnit: 'day'}", "{largestUnit: 'second', smallestUnit: 'minute'}", "{roundingIncrement: 24, smallestUnit: 'hour'}"];
for (const [a, b] of tpairs) for (const o of tdiff) add(`${PT}.from("${a}").until("${b}"${o ? ", " + o : ""}).toString()`, `${PT}.from("${a}").since("${b}"${o ? ", " + o : ""}).toString()`);
const trounds = ["{smallestUnit: 'hour'}", "{smallestUnit: 'minute'}", "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'microsecond'}", "{smallestUnit: 'nanosecond'}",
  "'hour'", "'minute'", "{smallestUnit: 'hour', roundingIncrement: 4}", "{smallestUnit: 'minute', roundingIncrement: 15, roundingMode: 'ceil'}", "{smallestUnit: 'minute', roundingIncrement: 15, roundingMode: 'floor'}",
  "{smallestUnit: 'second', roundingIncrement: 30, roundingMode: 'halfExpand'}", "{smallestUnit: 'hour', roundingIncrement: 5}", "{smallestUnit: 'day'}", "{}", "undefined", "{smallestUnit: 'bogus'}",
  "{smallestUnit: 'minute', roundingMode: 'halfEven'}", "{smallestUnit: 'minute', roundingMode: 'halfTrunc'}", "{smallestUnit: 'minute', roundingMode: 'halfFloor'}", "{smallestUnit: 'minute', roundingMode: 'halfCeil'}", "{smallestUnit: 'minute', roundingMode: 'trunc'}", "{smallestUnit: 'hour', roundingIncrement: 24}"];
for (const t of ["12:34:56.789123456", "23:59:59.999999999", "00:30", "13:30", "12:30:30"]) for (const o of trounds) add(`${PT}.from("${t}").round(${o}).toString()`);

// ---------- PlainDateTime ----------
const dts = ["2024-01-31T12:30:45.123456789", "2024-02-29T00:00", "2023-12-31T23:59:59.999999999", "1999-12-31T23:59", "2024-03-10T02:30", "2024-01-31 12:30", "2024-01-31t12:30", "20240131T123045", "+275760-09-13T23:59:59.999999999", "-271821-04-19T00:00:00.000000001"];
const dtProps = ["year", "month", "monthCode", "day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInMonth", "inLeapYear"];
for (const d of dts) {
  for (const p of dtProps) add(`${PDT}.from("${d}").${p}`);
  add(`${PDT}.from("${d}").toString()`, `JSON.stringify(${PDT}.from("${d}"))`, `${PDT}.from("${d}").toPlainDate().toString()`, `${PDT}.from("${d}").toPlainTime().toString()`,
    `${PDT}.from("${d}").toString({calendarName: 'always'})`, `${PDT}.from("${d}").toString({smallestUnit: 'minute'})`, `${PDT}.from("${d}").toString({fractionalSecondDigits: 2})`,
    `${PDT}.from("${d}").toString({smallestUnit: 'second', roundingMode: 'ceil'})`, `${PDT}.from("${d}").withPlainTime("10:00").toString()`, `${PDT}.from("${d}").withPlainTime().toString()`,
    `${PDT}.from("${d}").toZonedDateTime("UTC").toString()`, `${PDT}.from("${d}").toZonedDateTime("America/New_York").toString()`);
}
add(
  `${PDT}.from("2024-01-31").toString()`, `${PDT}.from("2024-01-31T10:00Z").toString()`, `${PDT}.from("2024-01-31T10:00+01:00[Europe/Berlin]").toString()`, `${PDT}.from("2024-01-31T24:00")`,
  `${PDT}.from("2024-01-31T23:59:60").toString()`, `${PDT}.from("2024-02-30T10:00")`, `${PDT}.from("bogus")`, `${PDT}.from("")`, `${PDT}.from("10:00")`,
  `${PDT}.from({year: 2024, month: 1, day: 31}).toString()`, `${PDT}.from({year: 2024, month: 1, day: 31, hour: 5, minute: 6, second: 7}).toString()`, `${PDT}.from({year: 2024, month: 1})`,
  `${PDT}.from({year: 2024, month: 2, day: 31, hour: 25}).toString()`, `${PDT}.from({year: 2024, month: 2, day: 31, hour: 25}, {overflow: 'reject'})`, `${PDT}.from({hour: 5})`,
  `${PDT}.from(${PD}.from("2024-01-31")).toString()`, `${PDT}.from(${Z}.from("2024-01-31T10:00[Europe/Berlin]")).toString()`, `${PDT}.from(5)`, `${PDT}.from(null)`,
  `new ${PDT}(2024, 1, 31).toString()`, `new ${PDT}(2024, 1, 31, 1, 2, 3, 4, 5, 6).toString()`, `new ${PDT}(2024, 1, 31, 24)`, `new ${PDT}(2024, 1)`, `new ${PDT}()`, `${PDT}(2024, 1, 1)`,
  `new ${PDT}(275760, 9, 13, 23, 59, 59, 999, 999, 999).toString()`, `new ${PDT}(275760, 9, 14)`, `new ${PDT}(-271821, 4, 19).toString()`, `new ${PDT}(-271821, 4, 18, 23, 59)`,
  `${PDT}.compare("2024-01-31T10:00", "2024-01-31T10:00:00.000000001")`, `${PDT}.compare("2024-01-31T10:00", "2024-01-31")`, `${PDT}.compare("2024-02-01", "2024-01-31T23:59")`,
  `${PDT}.from("2024-01-31T10:00").equals("2024-01-31T10:00")`, `${PDT}.from("2024-01-31T10:00").equals("2024-01-31T10:01")`, `${PDT}.from("2024-01-31T10:00").valueOf()`,
  `${PDT}.from("2024-01-31T10:00").toZonedDateTime("bogus")`, `${PDT}.from("2024-01-31T10:00").toZonedDateTime()`, `${PDT}.from("2024-01-31T10:00").toZonedDateTime({timeZone: "UTC"})`,
  `${PDT}.from("2024-01-31T10:00").toZonedDateTime("UTC", {disambiguation: 'bogus'})`, `${PDT}.from("2024-01-31T10:00").withPlainTime("bogus")`,
);
for (const w of ["{year: 2025}", "{hour: 23, minute: 59}", "{month: 2, day: 30}", "{month: 2, day: 30}, {overflow: 'reject'}", "{hour: 25}, {overflow: 'reject'}", "{}", "{monthCode: 'M12'}", "{nanosecond: 5}", "'x'", "{timeZone: 'UTC'}"]) {
  add(`${PDT}.from("2024-01-31T12:30:45.5").with(${w})`);
}
const dtDurs = ["P1D", "P1M", "P1Y", "PT1H", "PT24H", "-PT1H", "P1M1DT1H", "PT0.000000001S", "-P1Y1M", "P1W", "PT36H", "-PT12H30M"];
for (const d of ["2024-01-31T12:30", "2024-03-10T02:30", "2023-12-31T23:59:59.999999999", "2024-02-29T00:00"]) {
  for (const dur of dtDurs) add(`${PDT}.from("${d}").add("${dur}").toString()`, `${PDT}.from("${d}").subtract("${dur}").toString()`);
  add(`${PDT}.from("${d}").add({months: 1}, {overflow: 'reject'}).toString()`);
}
add(`${PDT}.from("+275760-09-13T23:59:59.999999999").add("PT0.000000001S")`, `${PDT}.from("-271821-04-19T00:00:00").subtract("PT1S")`, `${PDT}.from("2024-01-31T12:30").add()`);
const dtpairs = [["2020-01-31T10:00", "2024-03-15T08:30:15.5"], ["2024-03-15T08:30", "2020-01-31T10:00"], ["2024-01-31T23:00", "2024-02-01T01:00"], ["2024-03-09T12:00", "2024-03-10T12:00"], ["2024-01-01T00:00", "2024-01-01T00:00"]];
const dtdiff = ["", "{largestUnit: 'year'}", "{largestUnit: 'month'}", "{largestUnit: 'week'}", "{largestUnit: 'day'}", "{largestUnit: 'hour'}", "{largestUnit: 'minute'}", "{largestUnit: 'second'}", "{largestUnit: 'nanosecond'}",
  "{smallestUnit: 'hour'}", "{largestUnit: 'year', smallestUnit: 'day', roundingMode: 'ceil'}", "{largestUnit: 'month', smallestUnit: 'hour', roundingIncrement: 4, roundingMode: 'halfExpand'}",
  "{smallestUnit: 'minute', roundingMode: 'floor'}", "{largestUnit: 'week', smallestUnit: 'week', roundingMode: 'halfEven'}", "{smallestUnit: 'year'}", "{largestUnit: 'day', smallestUnit: 'month'}"];
for (const [a, b] of dtpairs) for (const o of dtdiff) add(`${PDT}.from("${a}").until("${b}"${o ? ", " + o : ""}).toString()`, `${PDT}.from("${a}").since("${b}"${o ? ", " + o : ""}).toString()`);
const dtround = ["{smallestUnit: 'day'}", "{smallestUnit: 'hour'}", "{smallestUnit: 'minute', roundingIncrement: 15}", "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'nanosecond'}",
  "'hour'", "{smallestUnit: 'day', roundingMode: 'ceil'}", "{smallestUnit: 'day', roundingMode: 'floor'}", "{smallestUnit: 'hour', roundingMode: 'halfEven'}", "{smallestUnit: 'day', roundingIncrement: 2}", "{}", "{smallestUnit: 'month'}", "{smallestUnit: 'hour', roundingIncrement: 7}"];
for (const d of ["2024-01-31T12:00", "2024-01-31T11:59:59.999999999", "2024-12-31T23:59:59.999999999", "2024-01-31T13:45:30.5", "+275760-09-13T23:59:59.999999999"]) for (const o of dtround) add(`${PDT}.from("${d}").round(${o}).toString()`);

// ---------- PlainYearMonth / PlainMonthDay ----------
for (const ym of ["2024-01", "2024-02", "2023-12", "-000001-06", "+275760-09", "-271821-04", "2024-01-31", "202401", "2024-01-31T10:00", "2024-01[u-ca=iso8601]"]) {
  for (const p of ["year", "month", "monthCode", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "calendarId", "era", "eraYear"]) add(`${PYM}.from("${ym}").${p}`);
  add(`${PYM}.from("${ym}").toString()`, `JSON.stringify(${PYM}.from("${ym}"))`, `${PYM}.from("${ym}").toString({calendarName: 'always'})`, `${PYM}.from("${ym}").toPlainDate({day: 15}).toString()`,
    `${PYM}.from("${ym}").toPlainDate({day: 31}).toString()`, `${PYM}.from("${ym}").toPlainDate({day: 40})`, `${PYM}.from("${ym}").toPlainDate()`);
}
add(
  `${PYM}.from("2024-13")`, `${PYM}.from("2024-00")`, `${PYM}.from("bogus")`, `${PYM}.from("")`, `${PYM}.from("2024")`, `${PYM}.from({year: 2024, month: 5}).toString()`, `${PYM}.from({year: 2024, monthCode: "M05"}).toString()`,
  `${PYM}.from({year: 2024})`, `${PYM}.from({month: 5})`, `${PYM}.from({year: 2024, month: 13})`, `${PYM}.from({year: 2024, month: 13}, {overflow: 'constrain'}).toString()`, `${PYM}.from({year: 2024, month: 13}, {overflow: 'reject'})`,
  `${PYM}.from({year: 2024, month: 5, calendar: 'iso8601'}).toString()`, `${PYM}.from(${PYM}.from("2024-05")).toString()`, `${PYM}.from(5)`, `${PYM}.from(null)`,
  `new ${PYM}(2024, 5).toString()`, `new ${PYM}(2024, 5, "iso8601", 15).toString({calendarName: 'always'})`, `new ${PYM}(2024, 5, "iso8601", 15).toString()`, `new ${PYM}(2024)`, `new ${PYM}(2024, 13)`, `new ${PYM}(2024, 5, "iso8601", 32)`,
  `new ${PYM}(275760, 9).toString()`, `new ${PYM}(275760, 10)`, `new ${PYM}(-271821, 4).toString()`, `new ${PYM}(-271821, 3)`, `${PYM}(2024, 5)`,
  `${PYM}.compare("2024-05", "2024-06")`, `${PYM}.compare("2024-06", "2024-05")`, `${PYM}.compare("2024-05", "2024-05")`, `${PYM}.from("2024-05").equals("2024-05")`, `${PYM}.from("2024-05").equals("2024-06")`, `${PYM}.from("2024-05").valueOf()`,
  `${PYM}.from("2024-05").with({month: 7}).toString()`, `${PYM}.from("2024-05").with({year: 2025}).toString()`, `${PYM}.from("2024-05").with({monthCode: "M12"}).toString()`, `${PYM}.from("2024-05").with({month: 13})`,
  `${PYM}.from("2024-05").with({month: 13}, {overflow: 'constrain'}).toString()`, `${PYM}.from("2024-05").with({})`, `${PYM}.from("2024-05").with(5)`, `${PYM}.from("2024-05").with({day: 1}).toString()`,
);
for (const d of ["P1M", "P1Y", "-P1M", "P12M", "P1Y1M", "P1D", "-P1D", "P31D", "P1W", "PT24H", "P0D", "P100Y", "P1M1D"]) {
  for (const ym of ["2024-01", "2024-12", "2024-02"]) add(`${PYM}.from("${ym}").add("${d}").toString()`, `${PYM}.from("${ym}").subtract("${d}").toString()`);
}
add(`${PYM}.from("+275760-09").add("P1M")`, `${PYM}.from("-271821-04").subtract("P1M")`, `${PYM}.from("2024-01").add({days: 1}, {overflow: 'reject'}).toString()`);
for (const [a, b] of [["2020-01", "2024-03"], ["2024-03", "2020-01"], ["2024-01", "2024-01"], ["2023-12", "2024-01"], ["2000-02", "2024-11"]]) {
  for (const o of ["", "{largestUnit: 'year'}", "{largestUnit: 'month'}", "{smallestUnit: 'year'}", "{smallestUnit: 'year', roundingMode: 'ceil'}", "{largestUnit: 'year', smallestUnit: 'month', roundingIncrement: 3, roundingMode: 'halfExpand'}", "{largestUnit: 'day'}", "{smallestUnit: 'day'}", "{largestUnit: 'week'}"]) {
    add(`${PYM}.from("${a}").until("${b}"${o ? ", " + o : ""}).toString()`, `${PYM}.from("${a}").since("${b}"${o ? ", " + o : ""}).toString()`);
  }
}
for (const md of ["01-31", "02-29", "12-25", "--12-25", "0229", "1225", "2024-02-29", "2023-02-29", "02-30", "2024-05-10T10:00", "01-31[u-ca=iso8601]"]) {
  for (const p of ["monthCode", "day", "calendarId"]) add(`${PMD}.from("${md}").${p}`);
  add(`${PMD}.from("${md}").toString()`, `JSON.stringify(${PMD}.from("${md}"))`, `${PMD}.from("${md}").toString({calendarName: 'always'})`, `${PMD}.from("${md}").toPlainDate({year: 2024}).toString()`,
    `${PMD}.from("${md}").toPlainDate({year: 2023}).toString()`, `${PMD}.from("${md}").toPlainDate()`);
}
add(
  `${PMD}.from("13-01")`, `${PMD}.from("00-01")`, `${PMD}.from("01-00")`, `${PMD}.from("bogus")`, `${PMD}.from("")`, `${PMD}.from({month: 5, day: 10}).toString()`, `${PMD}.from({monthCode: "M05", day: 10}).toString()`,
  `${PMD}.from({year: 2023, month: 2, day: 29}).toString()`, `${PMD}.from({year: 2023, month: 2, day: 29}, {overflow: 'reject'})`, `${PMD}.from({month: 2, day: 29}).toString()`, `${PMD}.from({month: 2, day: 30}).toString()`,
  `${PMD}.from({month: 2, day: 30}, {overflow: 'reject'})`, `${PMD}.from({month: 13, day: 1})`, `${PMD}.from({month: 13, day: 1}, {overflow: 'constrain'}).toString()`, `${PMD}.from({month: 5})`, `${PMD}.from({day: 5})`,
  `${PMD}.from({monthCode: "M02", day: 29, year: 2023}).toString()`, `${PMD}.from(${PMD}.from("05-10")).toString()`, `${PMD}.from(5)`, `${PMD}.from(null)`,
  `new ${PMD}(5, 10).toString()`, `new ${PMD}(2, 29).toString()`, `new ${PMD}(2, 30)`, `new ${PMD}(13, 1)`, `new ${PMD}(5)`, `new ${PMD}(5, 10, "iso8601", 2023).toString({calendarName: 'always'})`, `new ${PMD}(2, 29, "iso8601", 2023)`,
  `new ${PMD}(5, 10, "iso8601", 2024).toString({calendarName: 'always'})`, `${PMD}(5, 10)`, `${PMD}.from("05-10").equals("05-10")`, `${PMD}.from("05-10").equals("05-11")`, `${PMD}.from("05-10").valueOf()`, `typeof ${PMD}.compare`,
  `${PMD}.from("05-10").with({day: 11}).toString()`, `${PMD}.from("05-10").with({month: 6}).toString()`, `${PMD}.from("05-10").with({monthCode: "M06"}).toString()`, `${PMD}.from("05-10").with({day: 40})`, `${PMD}.from("05-10").with({day: 40}, {overflow: 'constrain'}).toString()`,
  `${PMD}.from("05-10").with({year: 2020}).toString()`, `${PMD}.from("05-10").with({})`, `${PMD}.from("02-29").with({day: 28}).toString()`,
);

// ---------- Instant ----------
const instants = ["2024-01-31T12:30:45.123456789Z", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59.999999999Z", "2024-03-10T07:00:00Z", "2024-11-03T06:00:00Z", "2024-01-31T12:30+05:30", "2024-01-31T12:30:00-03:00",
  "2024-01-31T12:30Z[America/New_York]", "2024-01-31 12:30z", "-271821-04-20T00:00:00Z", "+275760-09-13T00:00:00Z", "2024-01-31T24:00Z", "2024-01-31T23:59:60Z", "2024-01-31T12:30+05:30:15", "2024-01-31T12:30+0530"];
for (const s of instants) {
  add(`${I}.from("${s}").toString()`, `${I}.from("${s}").epochMilliseconds`, `${I}.from("${s}").epochNanoseconds`, `JSON.stringify(${I}.from("${s}"))`);
}
add(
  `${I}.from("2024-01-31")`, `${I}.from("2024-01-31T12:30")`, `${I}.from("bogus")`, `${I}.from("")`, `${I}.from(5)`, `${I}.from(null)`, `${I}.from({})`, `${I}.from(1n)`, `${I}.from(${I}.from("2024-01-31T12:30Z")).toString()`,
  `${I}.from(${Z}.from("2024-01-31T12:30[Europe/Berlin]")).toString()`, `${I}.from("+275760-09-13T00:00:00.000000001Z")`, `${I}.from("-271821-04-19T23:59:59.999999999Z")`,
  `${I}.fromEpochMilliseconds(0).toString()`, `${I}.fromEpochMilliseconds(1706704245123).toString()`, `${I}.fromEpochMilliseconds(-1).toString()`, `${I}.fromEpochMilliseconds(8.64e15).toString()`, `${I}.fromEpochMilliseconds(8.64e15 + 1)`,
  `${I}.fromEpochMilliseconds(1.5)`, `${I}.fromEpochMilliseconds(NaN)`, `${I}.fromEpochMilliseconds("0").toString()`, `${I}.fromEpochMilliseconds(0n)`, `${I}.fromEpochMilliseconds()`, `${I}.fromEpochMilliseconds(-8.64e15).toString()`,
  `${I}.fromEpochNanoseconds(0n).toString()`, `${I}.fromEpochNanoseconds(1706704245123456789n).toString()`, `${I}.fromEpochNanoseconds(-1n).toString()`, `${I}.fromEpochNanoseconds(8640000000000000000000n).toString()`,
  `${I}.fromEpochNanoseconds(8640000000000000000001n)`, `${I}.fromEpochNanoseconds(-8640000000000000000000n).toString()`, `${I}.fromEpochNanoseconds(-8640000000000000000001n)`, `${I}.fromEpochNanoseconds(0)`, `${I}.fromEpochNanoseconds("5").toString()`,
  `${I}.fromEpochNanoseconds()`, `${I}.fromEpochNanoseconds(1.5)`, `new ${I}(0n).toString()`, `new ${I}(0)`, `new ${I}()`, `new ${I}(1n).epochNanoseconds`, `${I}(0n)`, `new ${I}(8640000000000000000001n)`,
  `${I}.compare("2024-01-31T12:30Z", "2024-01-31T12:30:00.000000001Z")`, `${I}.compare("2024-01-31T12:30Z", "2024-01-31T12:30+00:00")`, `${I}.compare("2024-01-31T12:30Z", "2024-01-31T12:30+01:00")`, `${I}.compare("2024-01-31T12:30Z")`,
  `${I}.from("2024-01-31T12:30Z").equals("2024-01-31T12:30:00Z")`, `${I}.from("2024-01-31T12:30Z").equals("2024-01-31T13:30+01:00")`, `${I}.from("2024-01-31T12:30Z").valueOf()`, `typeof ${I}.from("2024-01-31T12:30Z").epochSeconds`,
  `typeof ${I}.from("2024-01-31T12:30Z").epochMicroseconds`, `${I}.from("2024-01-31T12:30Z").toJSON()`,
);
const iDurs = ["PT1H", "PT1S", "PT0.000000001S", "-PT1H", "PT24H", "PT36H30M", "PT1.5S", "P1D", "P1W", "P1M", "P1Y", "PT0S", "-PT0.000000001S", "PT8640000000000000S"];
for (const d of iDurs) add(`${I}.from("2024-01-31T12:30:45.5Z").add("${d}").toString()`, `${I}.from("2024-01-31T12:30:45.5Z").subtract("${d}").toString()`);
add(`${I}.from("2024-01-31T12:30:45.5Z").add({hours: 1}).toString()`, `${I}.from("2024-01-31T12:30:45.5Z").add()`, `${I}.from("2024-01-31T12:30:45.5Z").add({days: 1})`,
  `${I}.from("+275760-09-13T00:00:00Z").add("PT1S")`, `${I}.from("-271821-04-20T00:00:00Z").subtract("PT1S")`);
const ipairs = [["2024-01-31T12:30:45.5Z", "2024-03-15T08:00Z"], ["2024-03-15T08:00Z", "2024-01-31T12:30:45.5Z"], ["1970-01-01T00:00Z", "2024-01-31T12:30:45.123456789Z"], ["2024-01-31T12:30Z", "2024-01-31T12:30Z"]];
const idiff = ["", "{largestUnit: 'hour'}", "{largestUnit: 'minute'}", "{largestUnit: 'second'}", "{largestUnit: 'millisecond'}", "{largestUnit: 'nanosecond'}", "{smallestUnit: 'minute'}", "{smallestUnit: 'hour', roundingMode: 'ceil'}",
  "{largestUnit: 'second', smallestUnit: 'second', roundingMode: 'halfExpand'}", "{smallestUnit: 'second', roundingIncrement: 30}", "{largestUnit: 'day'}", "{largestUnit: 'auto'}", "{largestUnit: 'year'}", "{largestUnit: 'month'}", "{largestUnit: 'week'}", "{smallestUnit: 'day'}"];
for (const [a, b] of ipairs) for (const o of idiff) add(`${I}.from("${a}").until("${b}"${o ? ", " + o : ""}).toString()`, `${I}.from("${a}").since("${b}"${o ? ", " + o : ""}).toString()`);
const iround = ["{smallestUnit: 'hour'}", "{smallestUnit: 'minute'}", "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'microsecond'}", "{smallestUnit: 'nanosecond'}", "'hour'", "{smallestUnit: 'day'}",
  "{smallestUnit: 'hour', roundingIncrement: 6}", "{smallestUnit: 'hour', roundingIncrement: 5}", "{smallestUnit: 'minute', roundingIncrement: 15, roundingMode: 'ceil'}", "{smallestUnit: 'second', roundingMode: 'floor'}",
  "{smallestUnit: 'second', roundingMode: 'halfEven'}", "{smallestUnit: 'hour', roundingIncrement: 24}", "{smallestUnit: 'hour', roundingIncrement: 25}", "{}", "undefined", "{smallestUnit: 'month'}", "{smallestUnit: 'millisecond', roundingIncrement: 500}"];
for (const s of ["2024-01-31T12:30:45.123456789Z", "2024-01-31T12:30:30Z", "1969-12-31T23:59:59.5Z", "2024-01-31T23:59:59.999999999Z"]) for (const o of iround) add(`${I}.from("${s}").round(${o}).toString()`);
const zones = ["UTC", "America/Sao_Paulo", "America/New_York", "Europe/Berlin", "Asia/Kolkata", "+05:30", "-03:00", "+00:00", "bogus", "Asia/Kathmandu", "Australia/Lord_Howe", "America/St_Johns", "Etc/GMT+3", "utc", "AMERICA/new_york"];
for (const z of zones) {
  for (const s of ["2024-01-31T12:30:45.123Z", "2024-07-15T12:30Z", "2024-03-10T07:30Z", "2024-11-03T06:30Z"]) {
    add(`${I}.from("${s}").toString({timeZone: "${z}"})`, `${I}.from("${s}").toZonedDateTimeISO("${z}").toString()`);
  }
}
for (const o of ["{}", "{fractionalSecondDigits: 3}", "{fractionalSecondDigits: 0}", "{smallestUnit: 'minute'}", "{smallestUnit: 'second', roundingMode: 'ceil'}", "{smallestUnit: 'millisecond', timeZone: 'Europe/Berlin'}",
  "{timeZone: 'Europe/Berlin', fractionalSecondDigits: 9}", "{timeZone: 'Europe/Berlin', smallestUnit: 'hour'}", "{timeZone: 5}", "{timeZone: null}", "{timeZone: 'Z'}", "{timeZone: '+05:30[x]'}", "{timeZone: '2024-01-31T00:00+01:00'}",
  "{timeZone: '2024-01-31T00:00[Europe/Berlin]'}", "{timeZone: '+0530'}", "{timeZone: '+05'}", "{timeZone: '+05:30:15'}", "{timeZone: 'GMT'}", "{timeZone: 'Etc/UTC'}", "{timeZone: 'Asia/Calcutta'}", "{timeZone: 'Asia/Kolkata'}"]) {
  add(`${I}.from("2024-01-31T12:30:45.987654321Z").toString(${o})`);
}
add(`${I}.from("2024-01-31T12:30Z").toZonedDateTimeISO()`, `${I}.from("2024-01-31T12:30Z").toZonedDateTimeISO({timeZone: "UTC"})`, `${I}.from("2024-01-31T12:30Z").toZonedDateTimeISO("UTC").calendarId`,
  `typeof ${I}.from("2024-01-31T12:30Z").toZonedDateTime`, `${I}.from("2024-01-31T12:30Z").toString({timeZone: "UTC", calendarName: 'always'})`);

// ---------- ZonedDateTime ----------
const zdts = [
  "2024-01-31T12:30:45.123456789[America/Sao_Paulo]", "2024-07-15T12:30[America/New_York]", "2024-01-31T12:30[Europe/Berlin]", "2024-01-31T12:30[Asia/Kolkata]", "2024-01-31T12:30[UTC]",
  "2024-01-31T12:30+05:30[+05:30]", "2024-01-31T12:30-03:00[-03:00]", "2024-01-31T12:30:00+01:00[Europe/Berlin]", "2024-01-31T12:30Z[UTC]", "2024-03-10T01:59:59[America/New_York]", "2024-03-10T03:00[America/New_York]",
  "2024-11-03T00:30[America/New_York]", "2024-11-03T05:30Z[America/New_York]", "2018-11-04T00:00[America/Sao_Paulo]", "2018-02-17T23:59[America/Sao_Paulo]", "2024-03-31T01:59[Europe/Berlin]", "2024-10-27T02:30+01:00[Europe/Berlin]", "2024-10-27T02:30+02:00[Europe/Berlin]",
];
const zProps = ["year", "month", "monthCode", "day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInMonth", "inLeapYear", "calendarId", "timeZoneId", "offset", "offsetNanoseconds",
  "epochMilliseconds", "epochNanoseconds", "hoursInDay"];
for (const z of zdts) {
  for (const p of zProps) add(`${Z}.from("${z}").${p}`);
  add(`${Z}.from("${z}").toString()`, `JSON.stringify(${Z}.from("${z}"))`, `${Z}.from("${z}").startOfDay().toString()`, `${Z}.from("${z}").toInstant().toString()`, `${Z}.from("${z}").toPlainDate().toString()`,
    `${Z}.from("${z}").toPlainTime().toString()`, `${Z}.from("${z}").toPlainDateTime().toString()`, `${Z}.from("${z}").withPlainTime("10:00").toString()`, `${Z}.from("${z}").withPlainTime().toString()`,
    `${Z}.from("${z}").withTimeZone("Asia/Kolkata").toString()`, `${Z}.from("${z}").withTimeZone("UTC").toString()`, `${Z}.from("${z}").withCalendar("iso8601").toString()`,
    `${Z}.from("${z}").getTimeZoneTransition("next")?.toString()`, `${Z}.from("${z}").getTimeZoneTransition("previous")?.toString()`, `${Z}.from("${z}").getTimeZoneTransition({direction: "next"})?.toString()`,
    `${Z}.from("${z}").toString({offset: 'never'})`, `${Z}.from("${z}").toString({timeZoneName: 'never'})`, `${Z}.from("${z}").toString({timeZoneName: 'critical'})`, `${Z}.from("${z}").toString({calendarName: 'always'})`,
    `${Z}.from("${z}").toString({smallestUnit: 'minute'})`, `${Z}.from("${z}").toString({fractionalSecondDigits: 3, roundingMode: 'floor'})`, `${Z}.from("${z}").toString({smallestUnit: 'hour', roundingMode: 'ceil'})`,
    `${Z}.from("${z}").round({smallestUnit: 'hour'}).toString()`, `${Z}.from("${z}").round({smallestUnit: 'day'}).toString()`, `${Z}.from("${z}").round({smallestUnit: 'minute', roundingIncrement: 15, roundingMode: 'ceil'}).toString()`,
    `${Z}.from("${z}").round({smallestUnit: 'day', roundingMode: 'floor'}).toString()`, `${Z}.from("${z}").round({smallestUnit: 'day', roundingMode: 'ceil'}).toString()`);
}
add(
  `${Z}.from("2024-01-31T12:30")`, `${Z}.from("2024-01-31T12:30Z")`, `${Z}.from("2024-01-31[UTC]").toString()`, `${Z}.from("2024-01-31T12:30[bogus]")`, `${Z}.from("2024-01-31T12:30[Z]")`, `${Z}.from("2024-01-31T12:30[+05:30]").toString()`,
  `${Z}.from("2024-01-31T12:30+05:30[UTC]")`, `${Z}.from("2024-01-31T12:30+05:30[UTC]", {offset: 'ignore'}).toString()`, `${Z}.from("2024-01-31T12:30+05:30[UTC]", {offset: 'use'}).toString()`, `${Z}.from("2024-01-31T12:30+05:30[UTC]", {offset: 'prefer'}).toString()`,
  `${Z}.from("2024-01-31T12:30+05:30[UTC]", {offset: 'reject'})`, `${Z}.from("2024-01-31T12:30+05:30[UTC]", {offset: 'bogus'})`, `${Z}.from("2024-01-31T12:30[UTC][u-ca=iso8601]").toString()`, `${Z}.from("2024-01-31T12:30[!UTC]").toString()`,
  `${Z}.from("2024-01-31T12:30[UTC]", {disambiguation: 'bogus'})`, `${Z}.from("2024-01-31T12:30[UTC]", {overflow: 'bogus'})`, `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "UTC"}).toString()`, `${Z}.from({year: 2024, month: 1, day: 31})`,
  `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "bogus"})`, `${Z}.from({year: 2024, month: 1, day: 31, hour: 5, timeZone: "Asia/Kolkata"}).toString()`, `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "UTC", offset: "+01:00"})`,
  `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "UTC", offset: "+01:00"}, {offset: 'ignore'}).toString()`, `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "UTC", offset: "bogus"})`, `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "UTC", offset: 1})`,
  `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "+05:30"}).toString()`, `${Z}.from({year: 2024, month: 2, day: 31, timeZone: "UTC"}).toString()`, `${Z}.from({year: 2024, month: 2, day: 31, timeZone: "UTC"}, {overflow: 'reject'})`,
  `${Z}.from({year: 2024, month: 1, day: 31, timeZone: "UTC", calendar: "iso8601"}).toString()`, `${Z}.from(5)`, `${Z}.from(null)`, `${Z}.from("")`,
  `${Z}.from(${Z}.from("2024-01-31T12:30[UTC]")).toString()`, `new ${Z}(0n, "UTC").toString()`, `new ${Z}(0n, "America/Sao_Paulo").toString()`, `new ${Z}(0n)`, `new ${Z}(0, "UTC")`, `new ${Z}(0n, "bogus")`, `new ${Z}(0n, "UTC", "bogus")`, `new ${Z}(0n, "+05:30").toString()`,
  `new ${Z}(0n, "utc").toString()`, `new ${Z}(0n, "america/sao_paulo").timeZoneId`, `new ${Z}(0n, "Z")`, `new ${Z}(0n, "2024-01-31T00:00+01:00")`, `new ${Z}(0n, "Asia/Calcutta").timeZoneId`, `new ${Z}(0n, "Etc/UTC").timeZoneId`, `new ${Z}(0n, "GMT").timeZoneId`, `new ${Z}(0n, "Etc/GMT+3").toString()`,
  `${Z}(0n, "UTC")`, `new ${Z}(8640000000000000000000n, "UTC").toString()`, `new ${Z}(8640000000000000000001n, "UTC")`, `new ${Z}(0n, "UTC", "iso8601").toString({calendarName: 'always'})`,
  `${Z}.compare("2024-01-31T12:30[UTC]", "2024-01-31T12:30[Europe/Berlin]")`, `${Z}.compare("2024-01-31T12:30[UTC]", "2024-01-31T13:30[Europe/Berlin]")`, `${Z}.compare("2024-01-31T12:30[UTC]")`,
  `${Z}.from("2024-01-31T12:30[UTC]").equals("2024-01-31T12:30[UTC]")`, `${Z}.from("2024-01-31T12:30[UTC]").equals("2024-01-31T13:30[Europe/Berlin]")`, `${Z}.from("2024-01-31T12:30[UTC]").equals("2024-01-31T13:30+01:00[+01:00]")`,
  `${Z}.from("2024-01-31T12:30[UTC]").valueOf()`, `typeof ${Z}.from("2024-01-31T12:30[UTC]").getTimeZoneTransition`, `${Z}.from("2024-01-31T12:30[UTC]").getTimeZoneTransition("next")`, `${Z}.from("2024-01-31T12:30[UTC]").getTimeZoneTransition()`,
  `${Z}.from("2024-01-31T12:30[UTC]").getTimeZoneTransition("bogus")`, `${Z}.from("2024-01-31T12:30[+05:30]").getTimeZoneTransition("previous")`, `${Z}.from("2024-01-31T12:30[Asia/Kolkata]").getTimeZoneTransition("next")`,
  `${Z}.from("2024-01-31T12:30[Europe/Berlin]").getTimeZoneTransition("next").toString()`, `${Z}.from("2024-01-31T12:30[America/Sao_Paulo]").getTimeZoneTransition("next")`, `${Z}.from("2024-01-31T12:30[America/Sao_Paulo]").getTimeZoneTransition("previous").toString()`,
  `${Z}.from("2024-01-31T12:30[America/New_York]").getTimeZoneTransition("next").toString()`, `${Z}.from("2024-01-31T12:30[America/New_York]").getTimeZoneTransition("previous").toString()`,
);
// DST: disambiguation e offset
const dstCases = [
  ["2024-03-10T02:30", "America/New_York"], ["2024-11-03T01:30", "America/New_York"], ["2024-03-31T02:30", "Europe/Berlin"], ["2024-10-27T02:30", "Europe/Berlin"],
  ["2018-11-04T00:30", "America/Sao_Paulo"], ["2018-02-17T23:30", "America/Sao_Paulo"], ["2018-02-18T00:30", "America/Sao_Paulo"], ["2024-04-07T02:15", "Australia/Lord_Howe"], ["2024-10-06T02:15", "Australia/Lord_Howe"],
  ["2024-03-10T02:00", "America/New_York"], ["2024-03-10T03:00", "America/New_York"], ["2024-11-03T02:00", "America/New_York"], ["2024-11-03T00:59:59.999999999", "America/New_York"],
];
const disamb = ["compatible", "earlier", "later", "reject", "bogus"];
for (const [t, z] of dstCases) {
  add(`${Z}.from("${t}[${z}]").toString()`, `${Z}.from("${t}[${z}]").hoursInDay`, `${Z}.from("${t}[${z}]").startOfDay().toString()`);
  for (const dd of disamb) {
    add(`${Z}.from("${t}[${z}]", {disambiguation: '${dd}'}).toString()`, `${PDT}.from("${t}").toZonedDateTime("${z}", {disambiguation: '${dd}'}).toString()`,
      `${Z}.from({year: ${t.slice(0, 4)}, month: ${+t.slice(5, 7)}, day: ${+t.slice(8, 10)}, hour: ${+t.slice(11, 13)}, minute: ${+t.slice(14, 16)}, timeZone: "${z}"}, {disambiguation: '${dd}'}).toString()`);
  }
  for (const off of ["+00:00", "-05:00", "-04:00", "+01:00", "+02:00", "-02:00", "-03:00", "+10:30", "+11:00"]) {
    for (const mode of ["use", "prefer", "ignore", "reject"]) add(`${Z}.from("${t}${off}[${z}]", {offset: '${mode}'}).toString()`);
  }
  for (const dd of ["earlier", "later"]) add(`${Z}.from("${t}[${z}]").add("PT1H", {}).toString()`, `${Z}.from("${t}[${z}]").add({days: 1}).toString()`, `${Z}.from("${t}[${z}]").subtract({days: 1}).toString()`, `${Z}.from("${t}[${z}]").with({hour: 2}, {disambiguation: '${dd}'}).toString()`);
  add(`${Z}.from("${t}[${z}]").round({smallestUnit: 'day'}).toString()`, `${Z}.from("${t}[${z}]").round({smallestUnit: 'hour'}).toString()`, `${Z}.from("${t}[${z}]").toPlainDateTime().toZonedDateTime("${z}").toString()`);
}
// with
for (const w of ["{hour: 3}", "{day: 10}", "{month: 3, day: 10, hour: 2, minute: 30}", "{offset: '+00:00'}", "{offset: '-05:00'}", "{offset: 'bogus'}", "{timeZone: 'UTC'}", "{calendar: 'iso8601'}", "{}", "5", "'x'", "{year: 2025}", "{monthCode: 'M06'}",
  "{hour: 25}", "{hour: 25}, {overflow: 'reject'}", "{hour: 2}, {disambiguation: 'earlier'}", "{hour: 2}, {disambiguation: 'later'}", "{hour: 2}, {disambiguation: 'reject'}", "{offset: '+01:00'}, {offset: 'ignore'}", "{offset: '+01:00'}, {offset: 'reject'}", "{offset: '+01:00'}, {offset: 'use'}", "{offset: '+01:00'}, {offset: 'prefer'}"]) {
  add(`${Z}.from("2024-03-10T01:30[America/New_York]").with(${w})`, `${Z}.from("2024-11-03T01:30-04:00[America/New_York]").with(${w})`, `${Z}.from("2024-07-15T12:30[Europe/Berlin]").with(${w})`);
}
// add/subtract/until/since
const zDurs = ["PT1H", "PT24H", "P1D", "P1W", "P1M", "P1Y", "-P1D", "PT0S", "P1DT1H", "PT36H", "-PT24H", "P1M1D", "PT0.000000001S", "P2D"];
for (const [t, z] of [["2024-03-09T12:00", "America/New_York"], ["2024-11-02T12:00", "America/New_York"], ["2024-03-30T02:30", "Europe/Berlin"], ["2018-11-03T00:30", "America/Sao_Paulo"], ["2024-01-31T12:30", "Asia/Kolkata"], ["2024-01-31T12:30", "UTC"], ["2024-01-31T12:30", "+05:30"]]) {
  for (const d of zDurs) add(`${Z}.from("${t}[${z}]").add("${d}").toString()`, `${Z}.from("${t}[${z}]").subtract("${d}").toString()`);
  add(`${Z}.from("${t}[${z}]").add({days: 1}, {overflow: 'reject'}).toString()`, `${Z}.from("${t}[${z}]").add({days: 1}, {overflow: 'bogus'})`);
}
add(`${Z}.from("2024-01-31T12:30[UTC]").add()`, `${Z}.from("2024-01-31T12:30[UTC]").add("bogus")`, `${Z}.from("+275760-09-13T00:00[UTC]").add("PT1S")`, `${Z}.from("2024-01-31T12:30[UTC]").add({years: 300000})`);
const zpairs = [["2024-03-09T12:00[America/New_York]", "2024-03-11T12:00[America/New_York]"], ["2024-03-11T12:00[America/New_York]", "2024-03-09T12:00[America/New_York]"],
  ["2024-11-02T12:00[America/New_York]", "2024-11-04T12:00[America/New_York]"], ["2024-01-31T12:30[Europe/Berlin]", "2024-06-15T08:15:30.5[Europe/Berlin]"], ["2024-01-31T12:30[UTC]", "2024-01-31T12:30[Europe/Berlin]"],
  ["2024-01-31T12:30[UTC]", "2024-02-01T12:30[UTC]"], ["2018-11-03T12:00[America/Sao_Paulo]", "2018-11-05T12:00[America/Sao_Paulo]"], ["2020-01-31T12:30[Asia/Kolkata]", "2024-03-15T08:00[Asia/Kolkata]"]];
const zdiff = ["", "{largestUnit: 'year'}", "{largestUnit: 'month'}", "{largestUnit: 'week'}", "{largestUnit: 'day'}", "{largestUnit: 'hour'}", "{largestUnit: 'minute'}", "{largestUnit: 'second'}", "{smallestUnit: 'hour'}", "{largestUnit: 'day', smallestUnit: 'day', roundingMode: 'ceil'}",
  "{largestUnit: 'year', smallestUnit: 'month', roundingMode: 'halfExpand'}", "{largestUnit: 'month', smallestUnit: 'hour', roundingIncrement: 6}", "{smallestUnit: 'minute', roundingMode: 'floor'}", "{largestUnit: 'hour', smallestUnit: 'hour', roundingMode: 'halfEven'}", "{largestUnit: 'nanosecond'}"];
for (const [a, b] of zpairs) for (const o of zdiff) add(`${Z}.from("${a}").until("${b}"${o ? ", " + o : ""}).toString()`, `${Z}.from("${a}").since("${b}"${o ? ", " + o : ""}).toString()`);
// startOfDay / round / hoursInDay mais
for (const z of zones.filter((x) => x !== "bogus")) {
  add(`new ${Z}(1710054000000000000n, "${z}").startOfDay().toString()`, `new ${Z}(1710054000000000000n, "${z}").hoursInDay`, `new ${Z}(1710054000000000000n, "${z}").toString()`, `new ${Z}(1730613600000000000n, "${z}").toString()`,
    `new ${Z}(1730613600000000000n, "${z}").hoursInDay`, `new ${Z}(1730613600000000000n, "${z}").getTimeZoneTransition("next")?.toString()`, `new ${Z}(1730613600000000000n, "${z}").getTimeZoneTransition("previous")?.toString()`,
    `new ${Z}(-1000000000000n, "${z}").toString()`, `new ${Z}(1530000000000000000n, "${z}").offsetNanoseconds`);
}
for (const o of ["{smallestUnit: 'day'}", "{smallestUnit: 'hour', roundingIncrement: 4}", "{smallestUnit: 'second'}", "{smallestUnit: 'millisecond'}", "{smallestUnit: 'nanosecond'}", "'hour'", "{}", "undefined", "{smallestUnit: 'month'}", "{smallestUnit: 'day', roundingIncrement: 2}"]) {
  add(`${Z}.from("2024-01-31T12:34:56.789[Europe/Berlin]").round(${o}).toString()`, `${Z}.from("2024-03-10T23:59:59.999[America/New_York]").round(${o}).toString()`);
}
// toString/timeZone com todas as opções de ZDT
for (const o of ["{}", "{offset: 'auto'}", "{offset: 'never'}", "{offset: 'bogus'}", "{timeZoneName: 'auto'}", "{timeZoneName: 'never'}", "{timeZoneName: 'critical'}", "{timeZoneName: 'bogus'}", "{calendarName: 'bogus'}", "{fractionalSecondDigits: 'auto'}", "{fractionalSecondDigits: 1}",
  "{smallestUnit: 'second'}", "{smallestUnit: 'minute', offset: 'never', timeZoneName: 'never'}", "{smallestUnit: 'day'}", "undefined", "'second'", "{calendarName: 'critical', timeZoneName: 'critical'}"]) {
  add(`${Z}.from("2024-01-31T12:30:45.123456789[America/Sao_Paulo]").toString(${o})`, `${Z}.from("2024-01-31T12:30:45.123456789+05:30[+05:30]").toString(${o})`);
}

// ---------- Temporal.Now ----------
add(`typeof Temporal.Now`, `Object.prototype.toString.call(Temporal.Now)`, `Object.getOwnPropertyNames(Temporal.Now)`, `Object.getOwnPropertySymbols(Temporal.Now).map(String)`, `typeof Temporal.Now.instant()`,
  `Temporal.Now.instant() instanceof ${I}`, `typeof Temporal.Now.timeZoneId()`, `Temporal.Now.timeZoneId().length > 0`, `Temporal.Now.plainDateISO() instanceof ${PD}`, `Temporal.Now.plainDateTimeISO() instanceof ${PDT}`,
  `Temporal.Now.plainTimeISO() instanceof ${PT}`, `Temporal.Now.zonedDateTimeISO() instanceof ${Z}`, `Temporal.Now.zonedDateTimeISO("Asia/Kolkata").timeZoneId`, `Temporal.Now.zonedDateTimeISO("bogus")`,
  `Temporal.Now.plainDateISO("Europe/Berlin") instanceof ${PD}`, `Temporal.Now.plainDateISO("bogus")`, `Temporal.Now.plainDateTimeISO("+05:30").calendarId`, `Temporal.Now.plainTimeISO("UTC").calendarId`,
  `Temporal.Now.instant().toString().endsWith("Z")`, `Temporal.Now.zonedDateTimeISO("UTC").offset`, `Temporal.Now.zonedDateTimeISO().calendarId`, `new Temporal.Now()`, `Temporal.Now()`, `Temporal.Now.instant.length`,
  `Temporal.Now.zonedDateTimeISO.length`, `Temporal.Now.instant.name`, `Object.getOwnPropertyDescriptor(Temporal.Now, "instant").enumerable`, `Object.getOwnPropertyDescriptor(Temporal.Now, Symbol.toStringTag).value`,
  `Temporal.Now.plainDateISO().calendarId`, `Temporal.Now.plainDateTimeISO().toString().length >= 19`, `Temporal.Now.instant().epochNanoseconds > 0n`, `typeof Temporal.Now.instant().epochMilliseconds`);

// ---------- Estáticas e protótipos ----------
const classes = {Duration: D, PlainDate: PD, PlainTime: PT, PlainDateTime: PDT, PlainYearMonth: PYM, PlainMonthDay: PMD, Instant: I, ZonedDateTime: Z};
add(`Object.getOwnPropertyNames(Temporal)`, `Object.getOwnPropertySymbols(Temporal).map(String)`, `Object.prototype.toString.call(Temporal)`, `typeof Temporal`, `Object.getOwnPropertyDescriptor(globalThis, "Temporal").enumerable`,
  `Object.getOwnPropertyDescriptor(globalThis, "Temporal").writable`, `Object.getOwnPropertyDescriptor(globalThis, "Temporal").configurable`, `Object.getPrototypeOf(Temporal) === Object.prototype`, `new Temporal()`, `Temporal()`,
  `Object.getOwnPropertyDescriptor(Temporal, "Duration").enumerable`, `Object.getOwnPropertyDescriptor(Temporal, "Duration").writable`, `Object.getOwnPropertyDescriptor(Temporal, "Duration").configurable`, `Object.isExtensible(Temporal)`, `Object.isFrozen(Temporal)`);
for (const [name, c] of Object.entries(classes)) {
  add(`Object.getOwnPropertyNames(${c})`, `Object.getOwnPropertyNames(${c}.prototype)`, `Object.getOwnPropertySymbols(${c}.prototype).map(String)`, `Object.prototype.toString.call(${c}.prototype)`,
    `${c}.length`, `${c}.name`, `typeof ${c}`, `Object.getPrototypeOf(${c}) === Function.prototype`, `${c}.prototype.constructor === ${c}`, `Object.getOwnPropertyDescriptor(${c}, "prototype").writable`,
    `Object.getOwnPropertyDescriptor(${c}, "prototype").enumerable`, `Object.getOwnPropertyDescriptor(${c}, "prototype").configurable`, `Object.getOwnPropertyDescriptor(${c}.prototype, Symbol.toStringTag).value`,
    `Object.getOwnPropertyDescriptor(${c}.prototype, Symbol.toStringTag).writable`, `Object.getOwnPropertyDescriptor(${c}.prototype, Symbol.toStringTag).configurable`, `Object.getOwnPropertyDescriptor(${c}.prototype, "constructor").enumerable`,
    `Object.getOwnPropertyNames(${c}.prototype).filter(function (k) { return typeof Object.getOwnPropertyDescriptor(${c}.prototype, k).value === "function"; }).map(function (k) { return k + ":" + ${c}.prototype[k].length; })`,
    `Object.getOwnPropertyNames(${c}).filter(function (k) { return typeof ${c}[k] === "function"; }).map(function (k) { return k + ":" + ${c}[k].length; })`,
    `Object.getOwnPropertyNames(${c}.prototype).filter(function (k) { return Object.getOwnPropertyDescriptor(${c}.prototype, k).get; }).map(function (k) { return k + ":" + Object.getOwnPropertyDescriptor(${c}.prototype, k).get.name; })`,
    `Object.getOwnPropertyNames(${c}.prototype).filter(function (k) { var d = Object.getOwnPropertyDescriptor(${c}.prototype, k); return d.get && (d.set !== undefined || d.enumerable || !d.configurable); })`,
    `Object.getOwnPropertyNames(${c}.prototype).filter(function (k) { var d = Object.getOwnPropertyDescriptor(${c}.prototype, k); return d.value && (d.enumerable || !d.configurable || !d.writable); })`,
    `Object.getOwnPropertyDescriptor(Temporal, "${name}").value === ${c}`, `${c}.prototype.toString.call({})`, `${c}.prototype.toString.call(undefined)`, `${c}.prototype.toString.call(Object.create(${c}.prototype))`,
    `Object.getOwnPropertyDescriptor(${c}.prototype, "toString").value.name`, `Object.getOwnPropertyDescriptor(${c}.prototype, "toString").value.toString().includes("native code")`, `Function.prototype.toString.call(${c}).includes("native code")`,
    `Reflect.construct(function () {}, [], ${c}) instanceof ${c}`, `${c}.prototype.valueOf.call({})`, `new (class extends ${c} {})(...${JSON.stringify({Duration: [1], PlainDate: [2024, 1, 1], PlainTime: [1], PlainDateTime: [2024, 1, 1], PlainYearMonth: [2024, 1], PlainMonthDay: [1, 1], Instant: ["0n"], ZonedDateTime: ["0n", "UTC"]}[name]).replace(/"0n"/, "0n")}).constructor.name`);
}
// protótipos: acessor chamado em this errado
for (const [c, props] of [[D, ["years", "sign", "blank"]], [PD, ["year", "dayOfWeek", "calendarId"]], [PT, ["hour", "nanosecond"]], [PDT, ["year", "hour"]], [PYM, ["year", "monthCode"]], [PMD, ["day", "monthCode"]], [I, ["epochMilliseconds", "epochNanoseconds"]], [Z, ["timeZoneId", "offset", "hoursInDay"]]]) {
  for (const p of props) add(`Object.getOwnPropertyDescriptor(${c}.prototype, "${p}").get.call({})`, `Object.getOwnPropertyDescriptor(${c}.prototype, "${p}").get.call(undefined)`, `${c}.prototype.${p}`);
}
add(`${D}.prototype.add.call({}, "PT1H")`, `${PD}.prototype.add.call(${PT}.from("12:30"), "P1D")`, `${PT}.prototype.equals.call(${PD}.from("2024-01-01"), "12:30")`, `${I}.prototype.round.call(5, "hour")`, `${Z}.prototype.startOfDay.call({})`);
// erros: construtores com argumentos errados e Symbol / valores exóticos
add(`${D}.from({years: Symbol()})`, `${D}.from({years: 1n})`, `${PD}.from({year: 1n, month: 1, day: 1})`, `${PD}.from({year: 2024, month: Symbol(), day: 1})`, `${PT}.from({hour: {valueOf() { return 7; }}}).toString()`,
  `${PD}.from({year: 2024, month: 1, day: {valueOf() { return 15; }}}).toString()`, `${PD}.from({get year() { return 2024; }, month: 1, day: 1}).toString()`, `${PD}.from({year: 2024, month: 1, day: 1}, {get overflow() { throw new RangeError("boom"); }})`,
  `${PD}.from({year: 2024, month: 1, day: 1}, {overflow: {toString() { return "reject"; }}}).toString()`, `(function () { var log = []; ${PD}.from(new Proxy({year: 2024, month: 1, day: 1}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PT}.from(new Proxy({hour: 1}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${D}.from(new Proxy({years: 1}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PDT}.from(new Proxy({year: 2024, month: 1, day: 1}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${Z}.from(new Proxy({year: 2024, month: 1, day: 1, timeZone: "UTC"}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PYM}.from(new Proxy({year: 2024, month: 1}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PMD}.from(new Proxy({month: 1, day: 1}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PD}.from("2024-01-01").with(new Proxy({day: 2}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PD}.from("2024-01-01").add("P1D", new Proxy({}, {get(t, k) { log.push(String(k)); return undefined; }})); return log; })()`,
  `(function () { var log = []; ${PD}.from("2024-01-01").until("2024-02-01", new Proxy({}, {get(t, k) { log.push(String(k)); return undefined; }})); return log; })()`,
  `(function () { var log = []; ${D}.from("PT1H").round(new Proxy({smallestUnit: "hour"}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${D}.from("PT1H").total(new Proxy({unit: "hour"}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${Z}.from("2024-01-01T00:00[UTC]").toString(new Proxy({}, {get(t, k) { log.push(String(k)); return undefined; }})); return log; })()`,
  `(function () { var log = []; ${I}.from("2024-01-01T00:00Z").round(new Proxy({smallestUnit: "hour"}, {get(t, k) { log.push(String(k)); return t[k]; }})); return log; })()`,
  `(function () { var log = []; ${PDT}.from("2024-01-01T00:00").until("2024-02-01", new Proxy({}, {get(t, k) { log.push(String(k)); return undefined; }})); return log; })()`,
  `(function () { var log = []; ${Z}.from("2024-01-01T00:00[UTC]", new Proxy({}, {get(t, k) { log.push(String(k)); return undefined; }})); return log; })()`,
);

// ---------- Execução ----------
const lines = [];
for (const source of programs) {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  const result = harness(source);
  lines.push(source + "\t" + result);
}
const target = path.join(__dirname, "../tests/golden/temporal_bun.tsv");
fs.writeFileSync(target, lines.join("\n") + "\n");
console.error(`${lines.length} programas escritos em tests/golden/temporal_bun.tsv`);
