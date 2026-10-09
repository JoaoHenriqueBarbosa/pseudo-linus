// Gera tests/golden/temporal_calendars_bun.tsv: programas de Temporal com calendários não ISO avaliados no bun 1.4.2.
// Linha: fonte<TAB>resultado, onde resultado é `ok<TAB>valor` ou `error<TAB>name<TAB>message JSON`.
// O harness é o mesmo de tests/golden/temporal_bun_harness.js, que tests/temporal_calendars_bun_golden.rs embute.
// Uso: TZ=UTC timeout 300 bun scripts/gen-temporal-calendars-golden.js
const fs = require("fs");
const { sampleByHash } = require("./golden-prelude.js");
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

const PD = "Temporal.PlainDate";
const PYM = "Temporal.PlainYearMonth";
const PMD = "Temporal.PlainMonthDay";
const D = "Temporal.Duration";

const calendars = [
  "hebrew", "japanese", "chinese", "dangi", "islamic", "islamic-civil", "islamic-tbla", "islamic-umalqura",
  "persian", "coptic", "ethiopic", "ethioaa", "indian", "buddhist", "roc", "gregory",
];

// Datas ISO âncora: início e fim de mês, 29 de fevereiro, ano bissexto hebraico (2024), mês intercalar
// chinês (2023-03-22), fins de era japonesa, fim de ano de calendários de 13 meses, limites de faixa.
const anchors = [
  "2024-01-01", "2024-01-31", "2024-02-29", "2023-02-28", "2024-03-15", "2024-12-31", "2019-04-30", "2019-05-01",
  "1989-01-07", "1989-01-08", "1868-01-01", "1926-12-25", "2023-03-22", "2024-03-09", "2024-09-10", "2024-09-11",
  "2024-07-07", "2024-03-20", "2024-03-19", "1912-01-01", "1900-01-01", "1970-01-01", "1972-12-31", "2100-12-31",
  "1583-01-01", "0001-01-01", "0000-06-01", "-000100-01-01", "2023-09-16", "2025-10-20",
  "+275760-09-13", "-271821-04-19", "+271821-01-01", "-271821-12-31",
];
const monthDayAnchors = ["1972-01-01", "1972-02-29", "1972-03-15", "1972-06-30", "1972-09-01", "1972-12-31", "1971-02-26", "1972-04-29"];

const getters = [
  "year", "month", "monthCode", "day", "era", "eraYear", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear",
  "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek", "calendarId",
];
const ymGetters = ["year", "month", "monthCode", "era", "eraYear", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "calendarId"];
const mdGetters = ["monthCode", "day", "calendarId"];

const dateOf = (iso, c) => `${PD}.from("${iso}").withCalendar("${c}")`;
const fieldList = (type, list) => `[${list.map((g) => `d.${g}`).join(",")}]`;

// ---------- Getters, toString e toJSON ----------
for (const c of calendars) {
  for (const iso of anchors) {
    add(
      `(function () { var d = ${dateOf(iso, c)}; return ${fieldList(PD, getters)}; })()`,
      `${dateOf(iso, c)}.toString()`,
    );
  }
  for (const iso of anchors.slice(0, 26)) {
    add(
      `(function () { var d = ${dateOf(iso, c)}.toPlainYearMonth(); return ${fieldList(PYM, ymGetters)}; })()`,
      `${dateOf(iso, c)}.toPlainYearMonth().toString()`,
    );
  }
  for (const iso of monthDayAnchors.concat(anchors.slice(0, 12))) {
    add(
      `(function () { var d = ${dateOf(iso, c)}.toPlainMonthDay(); return ${fieldList(PMD, mdGetters)}; })()`,
      `${dateOf(iso, c)}.toPlainMonthDay().toString()`,
    );
  }
  add(
    `JSON.stringify(${dateOf("2024-03-15", c)})`, `JSON.stringify(${dateOf("2024-03-15", c)}.toPlainYearMonth())`,
    `JSON.stringify(${dateOf("2024-03-15", c)}.toPlainMonthDay())`,
    `${dateOf("2024-03-15", c)}.toString({calendarName: "never"})`, `${dateOf("2024-03-15", c)}.toString({calendarName: "always"})`,
    `${dateOf("2024-03-15", c)}.toString({calendarName: "critical"})`, `${dateOf("2024-03-15", c)}.toString({calendarName: "auto"})`,
    `${dateOf("2024-03-15", c)}.toPlainYearMonth().toString({calendarName: "never"})`,
    `${dateOf("2024-03-15", c)}.toPlainYearMonth().toString({calendarName: "critical"})`,
    `${dateOf("2024-03-15", c)}.toPlainMonthDay().toString({calendarName: "never"})`,
    `${dateOf("2024-03-15", c)}.toPlainMonthDay().toString({calendarName: "always"})`,
    `${PD}.from("2024-03-15[u-ca=${c}]").toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").calendarId`,
    `${PD}.from("2024-03-15T10:00[u-ca=${c}]").toString()`, `${PD}.from("2024-03-15[!u-ca=${c}]").toString()`,
    `${PYM}.from("2024-03-01[u-ca=${c}]").toString()`, `${PYM}.from("2024-03-15[u-ca=${c}]").toString()`,
    `${PMD}.from("1972-03-15[u-ca=${c}]").toString()`, `${PMD}.from("2024-03-15[u-ca=${c}]").toString()`,
    `${PMD}.from("03-15[u-ca=${c}]").toString()`, `${PMD}.from("--03-15[u-ca=${c}]").toString()`,
    `${PD}.from("2024-03-15[u-ca=${c.toUpperCase()}]").calendarId`,
    `${PD}.from("2024-03-15[u-ca=iso8601][u-ca=${c}]").toString()`,
    `${PD}.from("2024-03-15[u-ca=${c}][u-ca=iso8601]").toString()`,
    `${PD}.from("2024-03-15[u-ca=${c}]").withCalendar("iso8601").toString()`,
    `${PD}.from("2024-03-15").withCalendar("${c}").withCalendar("${c}").toString()`,
    `new ${PD}(2024, 3, 15, "${c}").toString()`, `new ${PD}(2024, 3, 15, "${c}").year`,
    `new ${PYM}(2024, 3, "${c}", 1).toString()`, `new ${PYM}(2024, 3, "${c}", 15).toString()`,
    `new ${PMD}(3, 15, "${c}", 1972).toString()`, `new ${PMD}(3, 15, "${c}").toString()`,
  );
}

// ---------- from com campos ----------
const fieldSets = [
  "year: 2024, month: 1, day: 31", "year: 2024, month: 1, day: 1", "year: 2024, month: 2, day: 29", "year: 2024, month: 12, day: 31",
  "year: 2024, month: 13, day: 1", "year: 2024, month: 14, day: 1", "year: 2024, month: 6, day: 30", "year: 2024, month: 6, day: 31",
  "year: 2024, month: 12, day: 30", "year: 2024, month: 12, day: 29", "year: 1, month: 1, day: 1", "year: 0, month: 1, day: 1",
  "year: -1, month: 1, day: 1", "year: 5784, month: 6, day: 1", "year: 5783, monthCode: 'M05L', day: 1", "year: 5784, monthCode: 'M05L', day: 30",
  "year: 5784, monthCode: 'M06', day: 30", "year: 5783, month: 13, day: 29", "year: 5784, month: 13, day: 29", "year: 2023, monthCode: 'M03L', day: 15",
  "year: 2023, monthCode: 'M02L', day: 15", "year: 2023, monthCode: 'M04L', day: 29", "year: 2023, month: 3, day: 1", "year: 2023, month: 13, day: 1",
  "year: 2024, monthCode: 'M01', day: 1", "year: 2024, monthCode: 'M12', day: 30", "year: 2024, monthCode: 'M13', day: 5",
  "year: 2024, monthCode: 'M13', day: 6", "year: 2024, monthCode: 'M00', day: 1", "year: 2024, monthCode: 'M14', day: 1",
  "year: 2024, monthCode: 'bogus', day: 1", "year: 2024, monthCode: 'M1', day: 1", "year: 2024, month: 3, monthCode: 'M04', day: 1",
  "year: 2024, month: 3, monthCode: 'M03', day: 1", "year: 2024, month: 0, day: 1", "year: 2024, month: 1, day: 0", "year: 2024, month: 1, day: 32",
  "year: 2024, month: 1", "year: 2024, day: 1", "month: 1, day: 1", "monthCode: 'M01', day: 1", "year: 2024, monthCode: 'M01'",
  "era: 'reiwa', eraYear: 6, month: 1, day: 1", "era: 'reiwa', eraYear: 1, month: 4, day: 30", "era: 'heisei', eraYear: 31, month: 5, day: 1",
  "era: 'heisei', eraYear: 31, month: 4, day: 30", "era: 'showa', eraYear: 64, month: 1, day: 7", "era: 'showa', eraYear: 64, month: 1, day: 8",
  "era: 'taisho', eraYear: 15, month: 12, day: 25", "era: 'meiji', eraYear: 1, month: 1, day: 1", "era: 'meiji', eraYear: 6, month: 1, day: 1",
  "era: 'ce', eraYear: 2024, month: 1, day: 1", "era: 'bce', eraYear: 1, month: 1, day: 1", "era: 'bc', eraYear: 1, month: 1, day: 1",
  "era: 'ad', eraYear: 1, month: 1, day: 1", "era: 'am', eraYear: 5784, month: 1, day: 1", "era: 'aa', eraYear: 5500, month: 1, day: 1",
  "era: 'ah', eraYear: 1445, month: 1, day: 1", "era: 'ap', eraYear: 1403, month: 1, day: 1", "era: 'shaka', eraYear: 1946, month: 1, day: 1",
  "era: 'be', eraYear: 2567, month: 1, day: 1", "era: 'roc', eraYear: 113, month: 1, day: 1", "era: 'broc', eraYear: 1, month: 1, day: 1",
  "era: 'bogus', eraYear: 1, month: 1, day: 1", "era: 'reiwa', month: 1, day: 1", "eraYear: 6, month: 1, day: 1", "era: 'reiwa', eraYear: 6, year: 2025, month: 1, day: 1",
  "era: 'reiwa', eraYear: 6, year: 2024, month: 1, day: 1", "year: 400000, month: 1, day: 1", "year: -400000, month: 1, day: 1",
  "year: 275760, month: 9, day: 13", "year: 275760, month: 9, day: 14", "year: 1403, month: 12, day: 30", "year: 1402, month: 12, day: 30",
  "year: 1445, month: 12, day: 30", "year: 1445, month: 12, day: 29", "year: 1445, month: 1, day: 30", "year: 1445, month: 2, day: 30",
  "year: 2307, month: 13, day: 5", "year: 2307, month: 13, day: 6", "year: 2308, month: 13, day: 6", "year: 2016, month: 13, day: 5",
  "year: 2016, month: 13, day: 6", "year: 1946, month: 1, day: 31", "year: 1946, month: 1, day: 30", "year: 1946, month: 6, day: 31",
  "year: 1947, month: 12, day: 31", "year: 2567, month: 2, day: 29", "year: 113, month: 2, day: 29", "year: 112, month: 2, day: 29",
  "year: '2024', month: '1', day: '31'", "year: 2024.5, month: 1, day: 1", "year: 2024, month: 1.5, day: 1", "year: Infinity, month: 1, day: 1",
  "year: 2024, month: NaN, day: 1", "year: undefined, month: 1, day: 1", "year: 2024, month: 1, day: 1, hour: 5",
];
const overflows = ["", ", {overflow: 'constrain'}", ", {overflow: 'reject'}"];
for (const c of calendars) {
  for (const fs of fieldSets) {
    for (const ov of overflows) {
      add(`${PD}.from({${fs}, calendar: "${c}"}${ov}).toString()`);
    }
  }
  for (const fs of fieldSets.slice(0, 40)) {
    add(`${PYM}.from({${fs.replace(/, day: [-\w']+/, "")}, calendar: "${c}"}).toString()`,
      `${PYM}.from({${fs.replace(/, day: [-\w']+/, "")}, calendar: "${c}"}, {overflow: 'reject'}).toString()`);
    add(`${PMD}.from({${fs}, calendar: "${c}"}).toString()`, `${PMD}.from({${fs}, calendar: "${c}"}, {overflow: 'reject'}).toString()`);
  }
  add(
    `${PYM}.from({year: 2024, month: 3, day: 15, calendar: "${c}"}).toString()`, `${PMD}.from({monthCode: 'M03', day: 15, calendar: "${c}"}).toString()`,
    `${PMD}.from({year: 2024, month: 3, day: 15, calendar: "${c}"}).toString()`, `${PMD}.from({month: 3, day: 15, calendar: "${c}"}).toString()`,
    `${PMD}.from({monthCode: 'M01', day: 30, calendar: "${c}"}).toString()`, `${PMD}.from({monthCode: 'M02', day: 30, calendar: "${c}"}).toString()`,
    `${PMD}.from({monthCode: 'M02', day: 29, calendar: "${c}"}).toString()`, `${PMD}.from({monthCode: 'M05L', day: 15, calendar: "${c}"}).toString()`,
    `${PMD}.from({monthCode: 'M06L', day: 15, calendar: "${c}"}).toString()`, `${PMD}.from({monthCode: 'M13', day: 5, calendar: "${c}"}).toString()`,
    `${PMD}.from({monthCode: 'M12', day: 31, calendar: "${c}"}).toString()`, `${PMD}.from({monthCode: 'M12', day: 30, calendar: "${c}"}).toString()`,
  );
}

// ---------- Erros de calendário ----------
const badCalendars = ["bogus", "islamic-rgsa", "islamic-civil ", "", "iso", "gregorian", "HEBREW", "Hebrew", "japanese-x", "u-ca=hebrew", "ISO8601", "islamicc", "ethiopic-amete-alem", "2024", "-ca"];
for (const b of badCalendars) {
  add(`${PD}.from({year: 2024, month: 1, day: 1, calendar: ${JSON.stringify(b)}}).toString()`, `${PD}.from("2024-01-01[u-ca=${b}]").toString()`,
    `${PD}.from("2024-01-01").withCalendar(${JSON.stringify(b)}).toString()`, `new ${PD}(2024, 1, 1, ${JSON.stringify(b)}).toString()`,
    `new ${PYM}(2024, 1, ${JSON.stringify(b)}, 1).toString()`, `new ${PMD}(1, 1, ${JSON.stringify(b)}, 1972).toString()`,
    `${PYM}.from({year: 2024, month: 1, calendar: ${JSON.stringify(b)}}).toString()`, `${PMD}.from({monthCode: 'M01', day: 1, calendar: ${JSON.stringify(b)}}).toString()`);
}
add(
  `${PD}.from({year: 2024, month: 1, day: 1, calendar: 5}).toString()`, `${PD}.from({year: 2024, month: 1, day: 1, calendar: null}).toString()`,
  `${PD}.from({year: 2024, month: 1, day: 1, calendar: {}}).toString()`, `${PD}.from({year: 2024, month: 1, day: 1, calendar: undefined}).toString()`,
  `${PD}.from("2024-01-01").withCalendar()`, `${PD}.from("2024-01-01").withCalendar(5)`, `${PD}.from("2024-01-01").withCalendar(null)`,
  `${PD}.from("2024-01-01").withCalendar("2024-01-01[u-ca=hebrew]").toString()`, `${PD}.from("2024-01-01").withCalendar("2024-01-01T00:00[u-ca=hebrew]").toString()`,
  `${PD}.from("2024-01-01").withCalendar("2024-01-01[u-ca=bogus]").toString()`, `${PD}.from("2024-01-01").withCalendar("hebrew").calendarId`,
  `${PD}.from("2024-01-01").withCalendar("islamic-civil").calendarId`, `${PD}.from("2024-01-01").withCalendar("islamicc")`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").equals("2024-01-01")`, `${PD}.from("2024-01-01[u-ca=hebrew]").equals("2024-01-01[u-ca=hebrew]")`,
  `${PD}.compare("2024-01-01[u-ca=hebrew]", "2024-01-01")`, `${PD}.compare("2024-01-01[u-ca=hebrew]", "2024-01-02[u-ca=chinese]")`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").until("2024-01-02")`, `${PD}.from("2024-01-01[u-ca=hebrew]").since("2024-01-02[u-ca=chinese]")`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").add("P1D").toString()`, `${PD}.from("2024-01-01[u-ca=hebrew]").with({calendar: "chinese"})`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").with({day: 1}, {overflow: "bogus"})`, `${PD}.from("2024-01-01[u-ca=hebrew]").with({})`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").with({timeZone: "UTC"})`, `${PD}.from("2024-01-01[u-ca=hebrew]").with(5)`, `${PD}.from("2024-01-01[u-ca=hebrew]").with("x")`,
  `${PYM}.from("2024-01-01[u-ca=hebrew]").equals("2024-01-01")`, `${PYM}.compare("2024-01-01[u-ca=hebrew]", "2024-01-01")`,
  `${PMD}.from("1972-01-01[u-ca=hebrew]").equals("1972-01-01")`, `${PD}.from("-271821-04-18").withCalendar("hebrew")`,
  `${PD}.from("+275760-09-14").withCalendar("hebrew")`, `${PD}.from("2024-02-30[u-ca=hebrew]")`, `${PD}.from("2024-02-30[u-ca=hebrew]", {overflow: "reject"})`,
  `${PD}.from("2024-13-01[u-ca=hebrew]")`, `${PD}.from("2024-01-01[u-ca=hebrew]", {overflow: "bogus"})`, `${PD}.from("2024-01-01[u-ca=hebrew]", 5)`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").toString({calendarName: "bogus"})`, `${PD}.from("2024-01-01[u-ca=hebrew]").toString({calendarName: 5})`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").toPlainDateTime().toString()`, `${PD}.from("2024-01-01[u-ca=hebrew]").toPlainDateTime("10:00").toString()`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").toZonedDateTime("UTC").toString()`, `${PD}.from("2024-01-01[u-ca=hebrew]").toZonedDateTime({timeZone: "UTC"}).calendarId`,
  `${PD}.from("2024-01-01[u-ca=hebrew]").valueOf()`, `${PD}.from("2024-01-01[u-ca=hebrew]").toLocaleString === undefined`,
  `Temporal.PlainDateTime.from("2024-01-01T10:00[u-ca=hebrew]").toString()`, `Temporal.PlainDateTime.from("2024-01-01T10:00[u-ca=hebrew]").year`,
  `Temporal.PlainDateTime.from("2024-01-01T10:00[u-ca=hebrew]").toPlainDate().toString()`,
  `Temporal.ZonedDateTime.from("2024-01-01T10:00[UTC][u-ca=hebrew]").toString()`, `Temporal.ZonedDateTime.from("2024-01-01T10:00[UTC][u-ca=hebrew]").year`,
  `Temporal.ZonedDateTime.from("2024-01-01T10:00[UTC]").withCalendar("hebrew").monthCode`, `Temporal.PlainDateTime.from("2024-01-01T10:00").withCalendar("chinese").toString()`,
  `Temporal.PlainDateTime.from("2024-03-15T10:00").withCalendar("chinese").monthCode`, `Temporal.Instant.from("2024-03-15T10:00Z").toZonedDateTimeISO("UTC").withCalendar("persian").year`,
);

// ---------- Aritmética ----------
const arithAnchors = ["2024-01-31", "2024-02-29", "2024-03-15", "2023-03-22", "2024-09-10", "2024-12-31", "2019-04-30", "2023-02-28", "1900-01-01"];
const durations = [
  "P1D", "P1W", "P1M", "P2M", "P6M", "P12M", "P13M", "P1Y", "P2Y", "P1Y1M", "P1Y2M3W4D", "P30D", "P365D", "P400D", "P10Y", "-P1D", "-P1M", "-P1Y", "-P1Y1M1D", "PT24H", "P1MT36H",
];
for (const c of calendars) {
  for (const iso of arithAnchors) {
    for (const d of durations) {
      add(`${dateOf(iso, c)}.add("${d}").toString()`, `${dateOf(iso, c)}.subtract("${d}").toString()`);
    }
    for (const d of ["P1M", "P1Y", "P1Y1M"]) {
      add(`${dateOf(iso, c)}.add("${d}", {overflow: "reject"}).toString()`, `${dateOf(iso, c)}.subtract("${d}", {overflow: "reject"}).toString()`,
        `${dateOf(iso, c)}.add("${d}", {overflow: "constrain"}).toString()`);
    }
  }
  // until/since
  const pairs = [
    ["2024-01-31", "2025-03-20"], ["2024-01-31", "2024-02-29"], ["2024-03-15", "2023-03-22"], ["2023-03-22", "2024-09-10"], ["2024-12-31", "2024-01-01"],
    ["1900-01-01", "2024-03-15"], ["2024-02-29", "2028-02-29"], ["2019-04-30", "2019-05-01"], ["2024-01-01", "2024-01-01"], ["2024-09-10", "2024-09-11"],
  ];
  for (const [a, b] of pairs) {
    for (const lu of ["year", "month", "week", "day"]) {
      add(`${dateOf(a, c)}.until(${dateOf(b, c)}, {largestUnit: "${lu}"}).toString()`, `${dateOf(a, c)}.since(${dateOf(b, c)}, {largestUnit: "${lu}"}).toString()`);
    }
    add(`${dateOf(a, c)}.until(${dateOf(b, c)}).toString()`, `${dateOf(a, c)}.since(${dateOf(b, c)}).toString()`,
      `${dateOf(a, c)}.until("${b}[u-ca=${c}]", {largestUnit: "month"}).toString()`);
  }
  const [a, b] = ["2024-01-31", "2025-03-20"];
  const roundOpts = [
    "largestUnit: 'year', smallestUnit: 'month'", "largestUnit: 'year', smallestUnit: 'month', roundingMode: 'ceil'", "largestUnit: 'year', smallestUnit: 'month', roundingMode: 'floor'",
    "largestUnit: 'year', smallestUnit: 'month', roundingMode: 'halfExpand'", "largestUnit: 'year', smallestUnit: 'year'", "largestUnit: 'year', smallestUnit: 'year', roundingMode: 'ceil'",
    "largestUnit: 'month', smallestUnit: 'month', roundingIncrement: 3", "largestUnit: 'month', smallestUnit: 'month', roundingIncrement: 3, roundingMode: 'ceil'",
    "largestUnit: 'year', smallestUnit: 'week'", "largestUnit: 'year', smallestUnit: 'week', roundingMode: 'ceil'", "largestUnit: 'month', smallestUnit: 'week'",
    "largestUnit: 'week', smallestUnit: 'week'", "largestUnit: 'day', smallestUnit: 'day', roundingIncrement: 10", "smallestUnit: 'day', roundingIncrement: 7",
    "largestUnit: 'year', smallestUnit: 'day'", "largestUnit: 'year', smallestUnit: 'month', roundingIncrement: 2, roundingMode: 'trunc'",
    "smallestUnit: 'month'", "smallestUnit: 'year'", "smallestUnit: 'week'", "largestUnit: 'month', smallestUnit: 'year'", "smallestUnit: 'hour'",
    "largestUnit: 'day', smallestUnit: 'year'", "roundingIncrement: 0", "roundingIncrement: 1000", "smallestUnit: 'bogus'", "largestUnit: 'bogus'", "roundingMode: 'bogus'",
  ];
  for (const o of roundOpts) {
    add(`${dateOf(a, c)}.until(${dateOf(b, c)}, {${o}}).toString()`, `${dateOf(a, c)}.since(${dateOf(b, c)}, {${o}}).toString()`,
      `${dateOf(b, c)}.until(${dateOf(a, c)}, {${o}}).toString()`);
  }
  // with
  const withs = [
    "{day: 1}", "{day: 31}", "{day: 30}", "{day: 29}", "{month: 1}", "{month: 12}", "{month: 13}", "{monthCode: 'M01'}", "{monthCode: 'M06'}", "{monthCode: 'M05L'}", "{monthCode: 'M13'}",
    "{year: 2023}", "{year: 2025}", "{year: 5783}", "{year: 1}", "{year: 1445}", "{era: 'reiwa', eraYear: 1}", "{eraYear: 3}", "{era: 'ce'}", "{year: 2024, month: 2, day: 29}",
    "{year: 2023, month: 2, day: 29}", "{monthCode: 'M02', day: 29}", "{month: 14}", "{day: 0}", "{month: 0}", "{day: -1}", "{monthCode: 'M00'}", "{monthCode: 'M3'}", "{year: Infinity}",
    "{month: 2, monthCode: 'M03'}", "{calendar: 'iso8601'}", "{timeZone: 'UTC'}", "{hour: 5}", "{foo: 1}", "{day: undefined}", "{}",
  ];
  for (const iso of ["2024-01-31", "2024-03-15", "2023-03-22", "2024-02-29"]) {
    for (const w of withs) {
      add(`${dateOf(iso, c)}.with(${w}).toString()`);
    }
    for (const w of ["{day: 31}", "{month: 14}", "{month: 13}", "{monthCode: 'M05L'}", "{year: 2023}", "{day: 30}"]) {
      add(`${dateOf(iso, c)}.with(${w}, {overflow: 'reject'}).toString()`, `${dateOf(iso, c)}.with(${w}, {overflow: 'constrain'}).toString()`);
    }
    // PlainYearMonth e PlainMonthDay
    const ym = `${dateOf(iso, c)}.toPlainYearMonth()`;
    const md = `${dateOf(iso, c)}.toPlainMonthDay()`;
    for (const w of ["{year: 2023}", "{month: 1}", "{month: 13}", "{monthCode: 'M05L'}", "{monthCode: 'M02'}", "{era: 'reiwa', eraYear: 1}", "{year: 2024, month: 12}", "{month: 14}", "{day: 1}", "{}"]) {
      add(`${ym}.with(${w}).toString()`, `${ym}.with(${w}, {overflow: 'reject'}).toString()`);
    }
    for (const w of ["{day: 1}", "{day: 30}", "{day: 31}", "{monthCode: 'M01'}", "{monthCode: 'M05L'}", "{monthCode: 'M13'}", "{year: 2023}", "{month: 2}", "{}", "{day: 29}"]) {
      add(`${md}.with(${w}).toString()`, `${md}.with(${w}, {overflow: 'reject'}).toString()`);
    }
    add(`${ym}.add("P1M").toString()`, `${ym}.add("P1Y").toString()`, `${ym}.subtract("P1M").toString()`, `${ym}.subtract("P1Y").toString()`, `${ym}.add("P13M").toString()`,
      `${ym}.subtract("-P1M").toString()`, `${ym}.add("P1D")`, `${ym}.add("P1M1D").toString()`, `${ym}.add("-P1M", {overflow: 'reject'}).toString()`,
      `${ym}.until(${dateOf("2025-03-20", c)}.toPlainYearMonth()).toString()`, `${ym}.until(${dateOf("2025-03-20", c)}.toPlainYearMonth(), {largestUnit: 'month'}).toString()`,
      `${ym}.since(${dateOf("2025-03-20", c)}.toPlainYearMonth()).toString()`, `${ym}.until(${dateOf("2025-03-20", c)}.toPlainYearMonth(), {smallestUnit: 'year', roundingMode: 'halfExpand'}).toString()`,
      `${ym}.toPlainDate({day: 1}).toString()`, `${ym}.toPlainDate({day: 15}).toString()`, `${ym}.toPlainDate({day: 30}).toString()`, `${ym}.toPlainDate({day: 31}).toString()`,
      `${ym}.toPlainDate({day: 1}).monthCode`, `${ym}.toPlainDate({day: 40}).toString()`, `${ym}.toPlainDate({}).toString()`, `${ym}.toPlainDate()`,
      `${md}.toPlainDate({year: 2024}).toString()`, `${md}.toPlainDate({year: 2023}).toString()`, `${md}.toPlainDate({year: 1900}).toString()`, `${md}.toPlainDate({}).toString()`,
      `${md}.toPlainDate({era: 'reiwa', eraYear: 6}).toString()`, `${md}.toPlainDate()`);
  }
  // equals e compare
  add(`${dateOf("2024-03-15", c)}.equals(${dateOf("2024-03-15", c)})`, `${dateOf("2024-03-15", c)}.equals(${dateOf("2024-03-16", c)})`,
    `${dateOf("2024-03-15", c)}.equals("2024-03-15")`, `${dateOf("2024-03-15", c)}.equals("2024-03-15[u-ca=${c}]")`,
    `${dateOf("2024-03-15", c)}.equals({year: 2024, month: 1, day: 1, calendar: "${c}"})`,
    `${PD}.compare(${dateOf("2024-03-15", c)}, ${dateOf("2024-03-16", c)})`, `${PD}.compare(${dateOf("2024-03-16", c)}, ${dateOf("2024-03-15", c)})`,
    `${PD}.compare(${dateOf("2024-03-15", c)}, ${dateOf("2024-03-15", c)})`, `${PD}.compare(${dateOf("2024-03-15", c)}, "2024-03-15")`,
    `${PD}.compare(${dateOf("2024-03-15", c)}, "2024-03-15[u-ca=gregory]")`,
    `${dateOf("2024-03-15", c)}.toPlainYearMonth().equals(${dateOf("2024-03-20", c)}.toPlainYearMonth())`,
    `${dateOf("2024-03-15", c)}.toPlainYearMonth().equals(${dateOf("2024-04-20", c)}.toPlainYearMonth())`,
    `${PYM}.compare(${dateOf("2024-03-15", c)}.toPlainYearMonth(), ${dateOf("2024-04-20", c)}.toPlainYearMonth())`,
    `${PYM}.compare(${dateOf("2024-04-15", c)}.toPlainYearMonth(), ${dateOf("2024-03-20", c)}.toPlainYearMonth())`,
    `${dateOf("2024-03-15", c)}.toPlainMonthDay().equals(${dateOf("2024-03-15", c)}.toPlainMonthDay())`,
    `${dateOf("2024-03-15", c)}.toPlainMonthDay().equals(${dateOf("2025-03-15", c)}.toPlainMonthDay())`,
    `${dateOf("2024-03-15", c)}.toPlainMonthDay().equals(${dateOf("2024-03-16", c)}.toPlainMonthDay())`,
    `${dateOf("2024-03-15", c)}.withCalendar("iso8601").toString()`, `${dateOf("2024-03-15", c)}.withCalendar("gregory").toString()`,
    `${dateOf("2024-03-15", c)}.withCalendar("hebrew").toString()`, `${dateOf("2024-03-15", c)}.withCalendar("japanese").toString()`,
    `${dateOf("2024-03-15", c)}.toPlainYearMonth().toPlainDate({day: 1}).withCalendar("iso8601").toString()`);
}

// ---------- Programas com duração e relativeTo não ISO ----------
for (const c of calendars) {
  for (const d of ["P1M", "P1Y", "P1Y2M3D", "P30D", "P13M", "P400D"]) {
    for (const u of ["year", "month", "week", "day"]) {
      add(`${D}.from("${d}").total({unit: "${u}", relativeTo: ${dateOf("2024-01-31", c)}})`);
    }
    add(`${D}.from("${d}").round({largestUnit: "month", relativeTo: ${dateOf("2024-01-31", c)}}).toString()`,
      `${D}.from("${d}").round({largestUnit: "year", relativeTo: ${dateOf("2024-03-15", c)}}).toString()`,
      `${D}.from("${d}").round({smallestUnit: "month", relativeTo: ${dateOf("2024-01-31", c)}}).toString()`);
  }
  add(`${D}.compare("P1M", "P30D", {relativeTo: ${dateOf("2024-01-31", c)}})`, `${D}.compare("P1Y", "P365D", {relativeTo: ${dateOf("2024-03-15", c)}})`);
}

// ---------- Execução ----------
// A combinação completa passa de 29 mil programas; a amostra por hash (sampleByHash) mantém 2500 e é determinística.
const LIMIT = 2500;
const selected = sampleByHash(programs, LIMIT);
const lines = [];
for (const source of selected) {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  lines.push(source + "\t" + harness(source));
}
const target = path.join(__dirname, "../tests/golden/temporal_calendars_bun.tsv");
fs.writeFileSync(target, lines.join("\n") + "\n");
console.error(`${lines.length} programas escritos em tests/golden/temporal_calendars_bun.tsv`);
