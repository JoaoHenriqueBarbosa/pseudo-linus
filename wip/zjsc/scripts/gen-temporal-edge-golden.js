// Gera tests/golden/temporal_edge_bun.tsv: ~500 programas de borda de Temporal avaliados no bun 1.4.2, que os demais
// goldens de Temporal não cobrem: PlainYearMonth, PlainMonthDay, Instant.round/until/since, ZonedDateTime.round,
// startOfDay e getTimeZoneTransition, Duration.total e round com relativeTo, Now.* (só forma), comparação, with() com
// overflow e calendários não ISO (hebrew, chinese, islamic, japanese) em from/toString/add.
// Linha: fonte<TAB>resultado, onde resultado é `ok<TAB>valor` ou `error<TAB>name<TAB>message JSON`.
// O harness é tests/golden/temporal_bun_harness.js, o mesmo texto que tests/temporal_edge_bun_golden.rs embute.
// Programas já presentes em outros temporal_*_bun.tsv são descartados. Determinístico (amostragem por passo fixo).
// Uso: bun scripts/gen-temporal-edge-golden.js
const fs = require("fs");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const golden = path.join(__dirname, "../tests/golden");
const harness = (0, eval)(fs.readFileSync(path.join(golden, "temporal_bun_harness.js"), "utf8").trimEnd());
const LIMIT = 500;
const programs = [];
const seen = new Set();
const known = new Set(knownPrograms("temporal_edge_bun.tsv", (file) => /^temporal_.*_bun\.tsv$/.test(file) && file !== "temporal_edge_bun.tsv"));
const add = (...sources) => {
  for (const source of sources) {
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};
const q = (text) => JSON.stringify(text);
const YM = "Temporal.PlainYearMonth";
const MD = "Temporal.PlainMonthDay";
const I = "Temporal.Instant";
const Z = "Temporal.ZonedDateTime";
const D = "Temporal.Duration";
const PD = "Temporal.PlainDate";

// ---------- PlainYearMonth ----------
const yms = ["2024-02", "2023-02", "2024-12", "-000001-01", "+275760-09", "-271821-04", "2024-01-31", "2024-02[u-ca=iso8601]", "202402"];
for (const s of yms) {
  add(`${YM}.from(${q(s)}).toString()`, `${YM}.from(${q(s)}).daysInMonth`, `${YM}.from(${q(s)}).monthsInYear`, `${YM}.from(${q(s)}).inLeapYear`,
    `${YM}.from(${q(s)}).add({months: 13}).toString()`, `${YM}.from(${q(s)}).subtract({months: 25}).toString()`, `${YM}.from(${q(s)}).add({days: 1})`,
    `${YM}.from(${q(s)}).toPlainDate({day: 31}).toString()`, `${YM}.from(${q(s)}).until("2030-06").toString()`, `${YM}.from(${q(s)}).since("2030-06", {largestUnit: "months"}).toString()`);
}
for (const o of ["constrain", "reject"]) {
  add(`${YM}.from({year: 2024, month: 13}, {overflow: ${q(o)}}).toString()`, `${YM}.from({year: 2024, month: 0}, {overflow: ${q(o)}}).toString()`,
    `${YM}.from("2024-02").with({month: 14}, {overflow: ${q(o)}}).toString()`, `${YM}.from("2024-02").with({monthCode: "M13"}, {overflow: ${q(o)}}).toString()`,
    `${YM}.from("2024-02").toPlainDate({day: 30}).toString()`, `${YM}.from("2024-02").toPlainDate({day: 0})`);
}
add(`${YM}.compare("2024-02", "2024-03")`, `${YM}.compare("2024-03", "2024-02")`, `${YM}.compare("2024-02", {year: 2024, month: 2})`, `${YM}.compare("2024-02", "2024-02-15")`,
  `${YM}.from("2024-02").equals("2024-02")`, `${YM}.from("2024-02").equals("2024-03")`, `new ${YM}(2024, 2, "iso8601", 31).toString()`, `new ${YM}(2024, 2, "iso8601", 31).toPlainDate({day: 1}).toString()`,
  `${YM}.from("2024-02").until("2024-02", {largestUnit: "years"}).toString()`, `${YM}.from("2024-02").until("2025-04", {largestUnit: "years", smallestUnit: "years", roundingMode: "ceil"}).toString()`,
  `${YM}.from("2024-02").until("2025-04", {largestUnit: "days"})`, `${YM}.from("2024-02").until("2025-04", {smallestUnit: "days"})`, `${YM}.from("2024-02").valueOf()`,
  `${YM}.from({year: 2024, monthCode: "M02"}).toString()`, `${YM}.from({year: 2024, month: 2, monthCode: "M03"})`, `${YM}.from({year: 2024})`, `${YM}.from({month: 2})`, `${YM}.from("2024-02").toJSON()`,
  `${YM}.from("2024-02").toLocaleString("en-US", {timeZone: "UTC"})`, `${YM}.from("2024-02").calendarId`, `${YM}.from("2024-02").era`, `${YM}.from("2024-02").eraYear`, `JSON.stringify(${YM}.from("2024-02"))`);

// ---------- PlainMonthDay ----------
const mds = ["02-29", "--02-29", "12-31", "01-01", "2024-02-29", "02-30", "04-31", "13-01", "00-10", "--0229", "0229"];
for (const s of mds) {
  add(`${MD}.from(${q(s)}).toString()`, `${MD}.from(${q(s)}).monthCode`, `${MD}.from(${q(s)}).day`, `${MD}.from(${q(s)}).toPlainDate({year: 2023}).toString()`,
    `${MD}.from(${q(s)}).toPlainDate({year: 2024}).toString()`, `${MD}.from(${q(s)}).with({day: 28}).toString()`, `${MD}.from(${q(s)}).equals("02-29")`);
}
for (const o of ["constrain", "reject"]) {
  add(`${MD}.from({monthCode: "M02", day: 30}, {overflow: ${q(o)}}).toString()`, `${MD}.from({month: 2, day: 30, year: 2023}, {overflow: ${q(o)}}).toString()`,
    `${MD}.from({month: 2, day: 29, year: 2023}, {overflow: ${q(o)}}).toString()`, `${MD}.from({monthCode: "M13", day: 1}, {overflow: ${q(o)}}).toString()`,
    `${MD}.from("02-29").with({monthCode: "M04", day: 31}, {overflow: ${q(o)}}).toString()`, `${MD}.from("02-29").with({day: 0}, {overflow: ${q(o)}}).toString()`);
}
add(`${MD}.from({month: 2, day: 29}).toString()`, `${MD}.from({month: 2, day: 29}).monthCode`, `${MD}.from({monthCode: "M02", day: 29}).toPlainDate({year: 2023}).toString()`,
  `${MD}.from("02-29").toPlainDate({})`, `${MD}.from("02-29").valueOf()`, `${MD}.from("02-29").toJSON()`, `${MD}.from("02-29").toLocaleString("en-US")`, `new ${MD}(2, 29, "iso8601", 2020).toString()`,
  `new ${MD}(2, 29, "iso8601", 2023)`, `${MD}.from("02-29").calendarId`, `${MD}.from("02-29").year`, `${MD}.from("12-25[u-ca=iso8601]").toString({calendarName: "always"})`);

// ---------- Instant.round / until / since ----------
const insts = ["2024-03-10T12:34:56.789123456Z", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59.999999999Z", "2024-02-29T23:59:59.5Z", "-000001-01-01T00:00:00Z", "+275760-09-13T00:00:00Z", "2024-03-10T12:34:56+05:30"];
for (const s of insts) {
  for (const unit of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
    for (const mode of ["ceil", "floor", "trunc", "expand", "halfEven", "halfExpand"]) {
      if (programs.length > 4000) break;
      add(`${I}.from(${q(s)}).round({smallestUnit: ${q(unit)}, roundingMode: ${q(mode)}}).toString()`);
    }
  }
  add(`${I}.from(${q(s)}).round({smallestUnit: "hour", roundingIncrement: 7})`, `${I}.from(${q(s)}).round({smallestUnit: "hour", roundingIncrement: 8}).toString()`,
    `${I}.from(${q(s)}).round({smallestUnit: "minute", roundingIncrement: 90})`, `${I}.from(${q(s)}).round({smallestUnit: "day"})`, `${I}.from(${q(s)}).round("hour").toString()`,
    `${I}.from(${q(s)}).round({})`, `${I}.from(${q(s)}).round()`, `${I}.from(${q(s)}).until("2025-01-01T00:00Z", {largestUnit: "hours"}).toString()`,
    `${I}.from(${q(s)}).since("2025-01-01T00:00Z", {largestUnit: "minutes", smallestUnit: "seconds", roundingMode: "ceil"}).toString()`, `${I}.from(${q(s)}).until("2025-01-01T00:00Z", {largestUnit: "days"})`,
    `${I}.from(${q(s)}).until("2025-01-01T00:00Z", {largestUnit: "auto", smallestUnit: "milliseconds", roundingIncrement: 250}).toString()`, `${I}.from(${q(s)}).since(${q(s)}).toString()`);
}
add(`${I}.compare("2024-01-01T00:00Z", "2024-01-01T01:00+01:00")`, `${I}.compare("2024-01-01T00:00Z", "2023-12-31T23:59:59.999999999Z")`, `${I}.from("2024-01-01T00:00Z").equals("2024-01-01T01:00+01:00")`,
  `${I}.from("2024-01-01T00:00Z").add({hours: 1}).toString()`, `${I}.from("2024-01-01T00:00Z").add({days: 1})`, `${I}.from("2024-01-01T00:00Z").subtract({nanoseconds: 1}).toString()`,
  `${I}.fromEpochMilliseconds(8.64e15).toString()`, `${I}.fromEpochMilliseconds(8.64e15 + 1)`, `${I}.fromEpochNanoseconds(-8640000000000000000000n).toString()`, `${I}.fromEpochNanoseconds(8640000000000000000001n)`,
  `${I}.from("2024-01-01T00:00Z").toString({fractionalSecondDigits: 4})`, `${I}.from("2024-01-01T00:00:00.123456789Z").toString({smallestUnit: "millisecond", roundingMode: "ceil"})`,
  `${I}.from("2024-01-01T00:00Z").toString({timeZone: "Asia/Kolkata"})`, `${I}.from("2024-01-01T00:00Z").toZonedDateTimeISO("Asia/Kathmandu").toString()`, `${I}.from("2024-01-01T00:00Z").valueOf()`);

// ---------- ZonedDateTime.round / startOfDay / getTimeZoneTransition ----------
const zdts = ["2024-03-10T01:30[America/New_York]", "2024-03-10T12:00[America/New_York]", "2024-11-03T00:30[America/New_York]", "2024-11-03T12:00[America/New_York]", "2018-11-04T12:00[America/Sao_Paulo]",
  "2024-10-06T12:00[Australia/Lord_Howe]", "2024-04-07T12:00[Australia/Lord_Howe]", "2011-12-30T12:00[Pacific/Apia]", "2024-06-01T12:00[Asia/Kolkata]", "2024-06-01T12:00[UTC]", "2024-06-01T12:00+05:45[+05:45]"];
for (const s of zdts) {
  for (const unit of ["day", "hour", "minute", "second"]) {
    for (const mode of ["ceil", "floor", "halfExpand", "halfEven", "trunc", "expand"]) add(`${Z}.from(${q(s)}).round({smallestUnit: ${q(unit)}, roundingMode: ${q(mode)}}).toString()`);
  }
  add(`${Z}.from(${q(s)}).startOfDay().toString()`, `${Z}.from(${q(s)}).hoursInDay`, `${Z}.from(${q(s)}).getTimeZoneTransition("next")?.toString()`, `${Z}.from(${q(s)}).getTimeZoneTransition("previous")?.toString()`,
    `${Z}.from(${q(s)}).getTimeZoneTransition({direction: "next"})?.toString()`, `${Z}.from(${q(s)}).getTimeZoneTransition()`, `${Z}.from(${q(s)}).getTimeZoneTransition("sideways")`,
    `${Z}.from(${q(s)}).round({smallestUnit: "week"})`, `${Z}.from(${q(s)}).round({smallestUnit: "hour", roundingIncrement: 5})`, `${Z}.from(${q(s)}).round("day").toString()`,
    `${Z}.from(${q(s)}).until(${q(s.replace("12:00", "13:00"))}, {largestUnit: "hours"}).toString()`, `${Z}.from(${q(s)}).since("2020-01-01T00:00[UTC]")`,
    `${Z}.from(${q(s)}).with({hour: 2, minute: 30}).toString()`, `${Z}.from(${q(s)}).with({hour: 2, minute: 30}, {disambiguation: "later"}).toString()`, `${Z}.from(${q(s)}).with({day: 31}, {overflow: "reject"})`);
}
add(`${Z}.from("2024-06-01T12:00[UTC]").getTimeZoneTransition("next")`, `${Z}.from("2024-06-01T12:00+01:00[+01:00]").getTimeZoneTransition("previous")`,
  `${Z}.from("1900-01-01T00:00[America/New_York]").getTimeZoneTransition("previous")?.toString()`, `${Z}.compare("2024-03-10T02:30[America/New_York]", "2024-03-10T03:30-04:00[America/New_York]")`,
  `${Z}.compare("2024-01-01T00:00[UTC]", "2024-01-01T01:00[Europe/Paris]")`, `${Z}.from("2024-01-01T00:00[UTC]").equals("2024-01-01T01:00[Europe/Paris]")`, `${Z}.from("2024-01-01T00:00[UTC]").equals("2024-01-01T00:00[Etc/UTC]")`);

// ---------- Duration.total e round com relativeTo ----------
const durs = ["P1Y2M3DT4H5M6S", "P1M", "P30D", "P1Y", "PT36H", "P1W", "P-1M", "P1Y-1M", "P2M31D", "PT0.5S", "P1M1DT25H"];
const rels = ["2024-01-31", "2023-01-31", "2024-02-01", "2024-03-10T00:00[America/New_York]", "2024-11-03T00:00[America/New_York]", "2024-02-29"];
for (const d of durs) {
  for (const r of rels) {
    add(`${D}.from(${q(d)}).total({unit: "days", relativeTo: ${q(r)}})`, `${D}.from(${q(d)}).total({unit: "hours", relativeTo: ${q(r)}})`, `${D}.from(${q(d)}).total({unit: "months", relativeTo: ${q(r)}})`,
      `${D}.from(${q(d)}).round({largestUnit: "years", smallestUnit: "days", relativeTo: ${q(r)}}).toString()`, `${D}.from(${q(d)}).round({largestUnit: "months", relativeTo: ${q(r)}}).toString()`,
      `${D}.from(${q(d)}).round({largestUnit: "hours", relativeTo: ${q(r)}}).toString()`, `${D}.from(${q(d)}).round({smallestUnit: "months", roundingMode: "halfExpand", relativeTo: ${q(r)}}).toString()`);
  }
  add(`${D}.from(${q(d)}).total({unit: "days"})`, `${D}.from(${q(d)}).total("hours")`, `${D}.from(${q(d)}).round({largestUnit: "days"})`, `${D}.from(${q(d)}).round({smallestUnit: "weeks"})`,
    `${D}.from(${q(d)}).total({unit: "weeks", relativeTo: {year: 2024, month: 1, day: 1}})`, `${D}.from(${q(d)}).total({unit: "months", relativeTo: {year: 2024, month: 1, day: 1, calendar: "hebrew"}})`);
}
add(`${D}.from("P1M").total({unit: "days", relativeTo: "2024-01-31", extra: 1})`, `${D}.from("P1M").total({unit: "days", relativeTo: null})`, `${D}.from("P1M").total({unit: "days", relativeTo: 5})`,
  `${D}.compare("P1M", "P30D")`, `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-01"})`, `${D}.compare("P1M", "P30D", {relativeTo: "2024-02-01"})`, `${D}.compare("PT24H", "P1D")`,
  `${D}.compare("PT24H", "P1D", {relativeTo: "2024-03-10T00:00[America/New_York]"})`, `${D}.compare("PT24H", "P1D", {relativeTo: "2024-11-03T00:00[America/New_York]"})`, `${D}.compare("P1Y", "P365D", {relativeTo: "2024-01-01"})`);

// ---------- Temporal.Now (forma, sem valor) ----------
add(`Object.getOwnPropertyNames(Temporal.Now).sort().join()`, `typeof Temporal.Now.instant().epochMilliseconds`, `Temporal.Now.plainDateTimeISO("UTC").year >= 2024`, `Temporal.Now.plainDateISO("Pacific/Kiritimati").calendarId`,
  `Temporal.Now.instant().until(Temporal.Now.instant()).sign <= 0`, `Temporal.Now.zonedDateTimeISO("Pacific/Apia").offset`, `Temporal.Now.zonedDateTimeISO("Etc/GMT+12").offset`,
  `Temporal.Now.instant().round({smallestUnit: "day"}).toString().endsWith("T00:00:00Z")`, `Temporal.Now.plainTimeISO("UTC").round("hour").minute`, `Temporal.Now.zonedDateTimeISO("UTC").startOfDay().hour`);

// ---------- Comparação cruzada e with() com overflow ----------
add(`Temporal.PlainDate.compare("2024-02-29", "2024-03-01")`, `Temporal.PlainDateTime.compare("2024-02-29T00:00", "2024-02-29T00:00:00.000000001")`, `Temporal.PlainTime.compare("23:59:59.999999999", "00:00")`,
  `Temporal.PlainDate.from("2024-01-31").with({month: 2}).toString()`, `Temporal.PlainDate.from("2024-01-31").with({month: 2}, {overflow: "reject"})`, `Temporal.PlainDate.from("2024-02-29").with({year: 2023}).toString()`,
  `Temporal.PlainDate.from("2024-02-29").with({year: 2023}, {overflow: "reject"})`, `Temporal.PlainDateTime.from("2024-01-31T10:00").with({month: 4, hour: 25}).toString()`,
  `Temporal.PlainDateTime.from("2024-01-31T10:00").with({month: 4, hour: 25}, {overflow: "reject"})`, `Temporal.PlainTime.from("10:00").with({hour: 25}).toString()`, `Temporal.PlainTime.from("10:00").with({minute: 60}, {overflow: "reject"})`,
  `Temporal.PlainTime.from("10:00").with({second: 99, millisecond: 5000}).toString()`, `Temporal.PlainDate.from("2024-01-31").with({day: 0})`, `Temporal.PlainDate.from("2024-01-31").with({day: -1}, {overflow: "reject"})`,
  `Temporal.PlainDate.from("2024-01-31").with({})`, `Temporal.PlainDate.from("2024-01-31").with({calendar: "iso8601"})`, `Temporal.PlainDate.from("2024-01-31").with({timeZone: "UTC"})`);

// ---------- Calendários não ISO ----------
const cals = ["hebrew", "chinese", "islamic-civil", "islamic-umalqura", "islamic-tbla", "islamic-rgsa", "islamic", "japanese", "dangi", "indian", "persian", "roc", "buddhist", "coptic", "ethiopic"];
const isoDates = ["2024-02-29", "2023-09-16", "2025-01-29", "1989-01-07", "2019-05-01", "2019-04-30", "1900-01-01", "2100-12-31", "2024-03-10"];
for (const c of cals) {
  for (const d of isoDates) {
    add(`${PD}.from(${q(`${d}[u-ca=${c}]`)}).toString()`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).monthCode`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).year`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).era`,
      `${PD}.from(${q(`${d}[u-ca=${c}]`)}).add({months: 1}).toString()`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).add({years: 1, days: 1}).toString()`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).subtract({months: 13}).toString()`,
      `${PD}.from(${q(`${d}[u-ca=${c}]`)}).daysInMonth`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).monthsInYear`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).inLeapYear`, `${PD}.from(${q(d)}).withCalendar(${q(c)}).toString()`,
      `${PD}.from(${q(`${d}[u-ca=${c}]`)}).until("2030-06-15", {largestUnit: "months"})`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).toPlainYearMonth().toString()`, `${PD}.from(${q(`${d}[u-ca=${c}]`)}).toPlainMonthDay().toString()`);
  }
  add(`${YM}.from(${q(`2024-02-01[u-ca=${c}]`)}).toString()`, `${YM}.from(${q(`2024-02-01[u-ca=${c}]`)}).add({months: 6}).toString()`, `${MD}.from(${q(`2024-02-29[u-ca=${c}]`)}).toString()`,
    `${MD}.from({monthCode: "M05L", day: 1, calendar: ${q(c)}}).toString()`, `${PD}.from({year: 5784, monthCode: "M05L", day: 1, calendar: ${q(c)}}).toString()`, `${PD}.from({year: 1445, month: 13, day: 1, calendar: ${q(c)}}).toString()`,
    `${PD}.from({year: 2024, month: 1, day: 30, calendar: ${q(c)}}, {overflow: "reject"}).toString()`, `${Z}.from(${q(`2024-02-29T12:00[UTC][u-ca=${c}]`)}).toString()`,
    `${Z}.from(${q(`2024-02-29T12:00[UTC][u-ca=${c}]`)}).add({months: 1}).toString()`, `${Z}.from(${q(`2024-02-29T12:00[UTC][u-ca=${c}]`)}).year`);
}
add(`${PD}.from({era: "reiwa", eraYear: 1, month: 5, day: 1, calendar: "japanese"}).toString()`, `${PD}.from({era: "heisei", eraYear: 31, month: 4, day: 30, calendar: "japanese"}).toString()`,
  `${PD}.from({era: "heisei", eraYear: 31, month: 5, day: 1, calendar: "japanese"}).toString()`, `${PD}.from({era: "showa", eraYear: 64, month: 1, day: 7, calendar: "japanese"}).toString()`,
  `${PD}.from({era: "showa", eraYear: 64, month: 1, day: 8, calendar: "japanese"}).toString()`, `${PD}.from({era: "meiji", eraYear: 6, month: 1, day: 1, calendar: "japanese"}).toString()`,
  `${PD}.from({era: "ce", eraYear: 1800, month: 1, day: 1, calendar: "japanese"}).toString()`, `${PD}.from({era: "bce", eraYear: 1, month: 1, day: 1, calendar: "japanese"}).toString()`,
  `${PD}.from({year: 2019, month: 5, day: 1, calendar: "japanese"}).era`, `${PD}.from({year: 2019, month: 4, day: 30, calendar: "japanese"}).eraYear`, `${PD}.from({era: "nope", eraYear: 1, month: 1, day: 1, calendar: "japanese"})`,
  `Temporal.Calendar`, `${PD}.from("2024-01-01[u-ca=bogus]")`, `${PD}.from("2024-01-01[u-ca=HEBREW]").calendarId`, `${PD}.from("2024-01-01[u-ca=islamic-civil]").calendarId`, `${PD}.from("2024-01-01[!u-ca=hebrew]").calendarId`);

// ---------- Amostragem determinística (sampleByHash) até o LIMIT, antes de descontar os goldens vizinhos ----------
const chosen = sampleByHash(programs, LIMIT).filter((source) => !known.has(source));
const lines = [];
for (const source of chosen) {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  lines.push(source + "\t" + harness(source));
}
fs.writeFileSync(path.join(golden, "temporal_edge_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.error(`${lines.length} programas escritos (de ${programs.length} gerados) em tests/golden/temporal_edge_bun.tsv`);
