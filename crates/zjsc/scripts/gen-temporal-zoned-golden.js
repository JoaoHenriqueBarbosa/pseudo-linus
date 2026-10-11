// Gera tests/golden/temporal_zoned_bun.tsv: programas de Temporal.ZonedDateTime e Temporal.Instant (fusos,
// transições, disambiguation, offset, arredondamento em dias de 23 e 25 horas, toString, erros) avaliados no
// bun 1.4.2. Linha: fonte<TAB>resultado, onde resultado é `ok<TAB>valor` ou `error<TAB>name<TAB>message JSON`.
// O harness é tests/golden/temporal_bun_harness.js, o mesmo texto que tests/temporal_zoned_bun_golden.rs embute.
// Uso: bun scripts/gen-temporal-zoned-golden.js   (determinístico: o conjunto é amostrado por hash)
const fs = require("fs");
const { sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const harness = (0, eval)(fs.readFileSync(path.join(__dirname, "../tests/golden/temporal_bun_harness.js"), "utf8").trimEnd());
const LIMIT = 1500;
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

const Z = "Temporal.ZonedDateTime";
const I = "Temporal.Instant";
const q = (text) => JSON.stringify(text);

// Cada caso: fuso, instante local ambíguo ou inexistente, e datas ao redor das transições.
const zones = [
  { tz: "America/Sao_Paulo", dst: ["2018-11-04T00:00", "2018-11-03T23:30", "2018-02-17T23:30", "2018-02-18T00:00", "2018-02-17T22:00"], plain: "2018-07-01T12:00" },
  { tz: "America/New_York", dst: ["2024-03-10T02:30", "2024-03-10T01:59", "2024-03-10T03:00", "2024-11-03T01:30", "2024-11-03T00:30", "2024-11-03T02:00"], plain: "2024-07-04T12:00" },
  { tz: "Europe/London", dst: ["2024-03-31T01:30", "2024-03-31T00:59", "2024-10-27T01:30", "2024-10-27T00:30", "2024-10-27T02:00"], plain: "2024-06-01T12:00" },
  { tz: "Australia/Lord_Howe", dst: ["2024-10-06T02:15", "2024-10-06T01:59", "2024-04-07T01:45", "2024-04-07T02:15", "2024-04-07T02:30"], plain: "2024-01-15T12:00" },
  { tz: "Pacific/Apia", dst: ["2011-12-30T12:00", "2011-12-29T23:59", "2011-12-31T00:00", "2011-12-29T12:00"], plain: "2012-06-01T12:00" },
  { tz: "Asia/Kolkata", dst: ["2024-03-10T12:00", "2024-12-31T23:59"], plain: "2024-06-01T12:00" },
  { tz: "Africa/Casablanca", dst: ["2024-03-10T02:30", "2024-03-31T02:30", "2024-03-09T12:00", "2024-04-14T02:30"], plain: "2024-06-01T12:00" },
  { tz: "UTC", dst: ["2024-03-10T02:30"], plain: "2024-06-01T12:00" },
  { tz: "+05:30", dst: ["2024-03-10T02:30"], plain: "2024-06-01T12:00" },
  { tz: "-03:00", dst: ["2024-03-10T02:30"], plain: "2024-06-01T12:00" },
];
const dis = ["compatible", "earlier", "later", "reject"];

// ---------- from(objeto) com disambiguation ----------
for (const { tz, dst } of zones) {
  for (const iso of dst) {
    const [date, time] = iso.split("T");
    const [y, mo, d] = date.split("-").map(Number);
    const [h, mi] = time.split(":").map(Number);
    const obj = `{year: ${y}, month: ${mo}, day: ${d}, hour: ${h}, minute: ${mi}, timeZone: ${q(tz)}}`;
    for (const x of dis) {
      add(`${Z}.from(${obj}, {disambiguation: ${q(x)}}).toString()`,
        `${Z}.from(${q(`${iso}[${tz}]`)}, {disambiguation: ${q(x)}}).toString()`,
        `${Z}.from(${obj}, {disambiguation: ${q(x)}}).epochNanoseconds`);
    }
    add(`${Z}.from(${obj}).toString()`, `${Z}.from(${q(`${iso}[${tz}]`)}).offset`,
      `${Z}.from(${q(`${iso}[${tz}]`)}).hoursInDay`, `${Z}.from(${q(`${iso}[${tz}]`)}).startOfDay().toString()`,
      `${Z}.from(${q(`${iso}[${tz}]`)}).toPlainDate().toString()`, `${Z}.from(${q(`${iso}[${tz}]`)}).toPlainTime().toString()`,
      `${Z}.from(${q(`${iso}[${tz}]`)}).toPlainDateTime().toString()`, `${Z}.from(${q(`${iso}[${tz}]`)}).toInstant().toString()`,
      `${Z}.from(${q(`${iso}[${tz}]`)}).offsetNanoseconds`, `${Z}.from(${q(`${iso}[${tz}]`)}).epochMilliseconds`,
      `${Z}.from(${q(`${iso}[${tz}]`)}).dayOfWeek`, `${Z}.from(${q(`${iso}[${tz}]`)}).dayOfYear`);
    for (const dir of ["next", "previous"]) {
      add(`String(${Z}.from(${q(`${iso}[${tz}]`)}).getTimeZoneTransition(${q(dir)}))`,
        `String(${Z}.from(${q(`${iso}[${tz}]`)}).getTimeZoneTransition({direction: ${q(dir)}}))`);
    }
  }
}

// ---------- offset option e strings com offsets inconsistentes ----------
const offsetCases = [
  ["2024-03-10T12:00-05:00[America/New_York]", "America/New_York"],
  ["2024-03-10T12:00-04:00[America/New_York]", "America/New_York"],
  ["2024-11-03T01:30-04:00[America/New_York]", "America/New_York"],
  ["2024-11-03T01:30-05:00[America/New_York]", "America/New_York"],
  ["2024-11-03T01:30-06:00[America/New_York]", "America/New_York"],
  ["2024-07-01T12:00+05:30[Asia/Kolkata]", "Asia/Kolkata"],
  ["2024-07-01T12:00+05:00[Asia/Kolkata]", "Asia/Kolkata"],
  ["2018-02-17T23:30-02:00[America/Sao_Paulo]", "America/Sao_Paulo"],
  ["2018-02-17T23:30-03:00[America/Sao_Paulo]", "America/Sao_Paulo"],
  ["2018-02-17T23:30-01:00[America/Sao_Paulo]", "America/Sao_Paulo"],
  ["2024-10-27T01:30+01:00[Europe/London]", "Europe/London"],
  ["2024-10-27T01:30+00:00[Europe/London]", "Europe/London"],
  ["2024-10-27T01:30Z[Europe/London]", "Europe/London"],
  ["2024-01-01T00:00+01:00[Europe/London]", "Europe/London"],
  ["2024-01-01T00:00:00.123+00:00:00.123[UTC]", "UTC"],
  ["2024-01-01T00:00+00:00:00[UTC]", "UTC"],
  ["2024-01-01T00:00+01:00[+01:00]", "+01:00"],
  ["2024-01-01T00:00+01:30[+01:00]", "+01:00"],
  ["2024-01-01T00:00+01:00[+01:00:30]", "+01:00:30"],
  ["2024-01-01T00:00+01:00:30[+01:00:30]", "+01:00:30"],
  ["2024-01-01T00:00+01:00:30.5[+01:00]", "+01:00"],
  ["2011-12-30T12:00+14:00[Pacific/Apia]", "Pacific/Apia"],
  ["2011-12-29T12:00-10:00[Pacific/Apia]", "Pacific/Apia"],
  ["2024-10-06T02:15+11:00[Australia/Lord_Howe]", "Australia/Lord_Howe"],
  ["2024-04-07T01:45+11:00[Australia/Lord_Howe]", "Australia/Lord_Howe"],
  ["2024-04-07T01:45+10:30[Australia/Lord_Howe]", "Australia/Lord_Howe"],
  ["2024-04-07T01:45+10:30[Australia/Lord_Howe][u-ca=iso8601]", "Australia/Lord_Howe"],
  ["2024-03-31T02:30+01:00[Africa/Casablanca]", "Africa/Casablanca"],
  ["2024-03-31T02:30+00:00[Africa/Casablanca]", "Africa/Casablanca"],
];
for (const [s, tz] of offsetCases) {
  add(`${Z}.from(${q(s)}).toString()`, `${Z}.from(${q(s + "")}).epochNanoseconds`);
  for (const o of ["use", "prefer", "ignore", "reject"]) {
    for (const x of ["compatible", "reject"]) {
      add(`${Z}.from(${q(s)}, {offset: ${q(o)}, disambiguation: ${q(x)}}).toString()`);
    }
    add(`${Z}.from(${q(s)}, {offset: ${q(o)}}).toString()`, `${Z}.from(${q(s)}, {offset: ${q(o)}}).offset`);
  }
  const [dpart, tpart] = s.split("[")[0].split("T");
  const [yy, mm, dd] = dpart.split("-").map(Number);
  const off = tpart.slice(5);
  const [hh, mi] = tpart.slice(0, 5).split(":").map(Number);
  const obj = `{year: ${yy}, month: ${mm}, day: ${dd}, hour: ${hh}, minute: ${mi}, offset: ${q(off)}, timeZone: ${q(tz)}}`;
  for (const o of ["use", "prefer", "ignore", "reject"]) add(`${Z}.from(${obj}, {offset: ${q(o)}}).toString()`);
  add(`${Z}.from(${obj}).toString()`);
}

// ---------- erros de from(string) ----------
const badStrings = [
  "2024-01-01", "2024-01-01T00:00", "2024-01-01T00:00Z", "2024-01-01T00:00+01:00", "2024-01-01T00:00[UTC]x", "2024-01-01T00:00[]",
  "2024-01-01T00:00[Nowhere/Land]", "2024-01-01T00:00[America/New_Yorkk]", "2024-01-01T00:00[america/new_york]", "2024-01-01T00:00[utc]",
  "2024-01-01T00:00[Etc/UTC]", "2024-01-01T00:00[Etc/GMT+3]", "2024-01-01T00:00[GMT]", "2024-01-01T00:00[Z]", "2024-01-01T00:00[+0100]",
  "2024-01-01T00:00[+01]", "2024-01-01T00:00[+24:00]", "2024-01-01T00:00[-00:00]", "2024-01-01T00:00[+01:60]", "2024-01-01T00:00[+01:00:60]",
  "2024-01-01T00:00[+01:00:00.123456789]", "2024-01-01T00:00[+01:00:00.1234567890]", "2024-01-01T00:00[!UTC]", "2024-01-01T00:00[UTC][!u-ca=iso8601]",
  "2024-01-01T00:00[UTC][u-ca=foo]", "2024-01-01T00:00[UTC][u-ca=iso8601][u-ca=iso8601]", "2024-01-01T00:00[UTC][u-ca=hebrew]", "2024-02-30T00:00[UTC]",
  "2024-13-01T00:00[UTC]", "2024-01-01T24:00[UTC]", "2024-01-01T23:60[UTC]", "2024-01-01T23:59:60[UTC]", "2024-01-01T23:59:60.5[UTC]",
  "+275760-09-13T00:00[UTC]", "+275760-09-13T00:00:00.000000001[UTC]", "-271821-04-20T00:00[UTC]", "-271821-04-19T23:59:59.999999999[UTC]",
  "-271821-04-20T00:00:00+00:00[UTC]", "+275760-09-13T00:00:00+00:00[UTC]", "+275760-09-13T00:00:00-01:00[UTC]", "-000000-01-01T00:00[UTC]",
  "2024-01-01T00:00:00,5[UTC]", "20240101T000000[UTC]", "2024-01-01 00:00[UTC]", "2024-01-01t00:00[UTC]", "2024-01-01T00:00z[UTC]",
  "2024-01-01T00:00[UTC][foo=bar]", "2024-01-01T00:00[UTC][!foo=bar]", "2024-01-01T00:00[America/Sao_Paulo]", "2024-W01-1T00:00[UTC]",
  "", " ", "UTC", "[UTC]", "2024-01-01T00:00[UTC] ", " 2024-01-01T00:00[UTC]", "2024-01-01T00:00[UTC]\u0000", "2024-01-01T00:00\u221201:00[UTC]",
  "2024-01-01T00:00+01:00[Europe/London]", "2024-01-01T00:00Z[+01:00]", "2024-01-01T00:00Z[UTC]", "2024-01-01T00:00+00:00[UTC]",
];
for (const s of badStrings) add(`${Z}.from(${q(s)}).toString()`, `${Z}.from(${q(s)}, {offset: "ignore"}).toString()`, `${I}.from(${q(s)}).toString()`);

// ---------- withTimeZone, withPlainTime, with, startOfDay ----------
const base = [
  "2024-03-10T12:00[America/New_York]", "2024-11-03T12:00[America/New_York]", "2018-11-04T12:00[America/Sao_Paulo]",
  "2018-02-18T12:00[America/Sao_Paulo]", "2011-12-31T12:00[Pacific/Apia]", "2024-04-07T12:00[Australia/Lord_Howe]",
  "2024-10-06T12:00[Australia/Lord_Howe]", "2024-10-27T12:00[Europe/London]", "2024-03-31T12:00[Europe/London]",
  "2024-03-31T12:00[Africa/Casablanca]", "2024-07-01T12:00[Asia/Kolkata]", "2024-07-01T12:00[UTC]", "2024-07-01T12:00[+05:30]",
  "2024-07-01T12:00[-00:30]", "2024-07-01T12:00[+00:00:01]",
];
const targets = ["UTC", "America/New_York", "Asia/Kolkata", "Pacific/Apia", "Australia/Lord_Howe", "+01:00", "-03:30", "+00:00:01", "Nowhere/Land", "", "utc", "Etc/UTC"];
for (const b of base) {
  add(`${Z}.from(${q(b)}).startOfDay().toString()`, `${Z}.from(${q(b)}).hoursInDay`, `${Z}.from(${q(b)}).timeZoneId`,
    `${Z}.from(${q(b)}).toString({timeZoneName: "never"})`, `${Z}.from(${q(b)}).toString({offset: "never"})`, `${Z}.from(${q(b)}).toLocaleString === undefined`,
    `${Z}.from(${q(b)}).add({days: 1}).toString()`, `${Z}.from(${q(b)}).add({hours: 24}).toString()`, `${Z}.from(${q(b)}).subtract({days: 1}).toString()`,
    `${Z}.from(${q(b)}).add({months: 1}).toString()`, `${Z}.from(${q(b)}).add({years: 1}).toString()`, `${Z}.from(${q(b)}).add({weeks: 1}).toString()`,
    `${Z}.from(${q(b)}).add({hours: 12}).toString()`, `${Z}.from(${q(b)}).add({hours: -12}).toString()`, `${Z}.from(${q(b)}).add({minutes: 30}).toString()`,
    `${Z}.from(${q(b)}).with({hour: 2, minute: 30}).toString()`, `${Z}.from(${q(b)}).with({hour: 2, minute: 30}, {disambiguation: "earlier"}).toString()`,
    `${Z}.from(${q(b)}).with({hour: 2, minute: 30}, {disambiguation: "later"}).toString()`, `${Z}.from(${q(b)}).with({hour: 2, minute: 30}, {disambiguation: "reject"}).toString()`,
    `${Z}.from(${q(b)}).with({hour: 1, minute: 30}, {disambiguation: "reject"}).toString()`, `${Z}.from(${q(b)}).with({day: 31}).toString()`,
    `${Z}.from(${q(b)}).with({day: 31}, {overflow: "reject"}).toString()`, `${Z}.from(${q(b)}).with({offset: "+00:00"}).toString()`,
    `${Z}.from(${q(b)}).with({offset: "+00:00"}, {offset: "ignore"}).toString()`, `${Z}.from(${q(b)}).with({offset: "+00:00"}, {offset: "reject"}).toString()`,
    `${Z}.from(${q(b)}).with({timeZone: "UTC"}).toString()`, `${Z}.from(${q(b)}).with({calendar: "iso8601"}).toString()`,
    `${Z}.from(${q(b)}).withPlainTime("00:00").toString()`, `${Z}.from(${q(b)}).withPlainTime("02:30").toString()`, `${Z}.from(${q(b)}).withPlainTime().toString()`,
    `${Z}.from(${q(b)}).withPlainTime("01:30").toString()`, `${Z}.from(${q(b)}).withCalendar("iso8601").toString()`, `${Z}.from(${q(b)}).withCalendar("hebrew").toString()`,
    `${Z}.from(${q(b)}).toPlainDateTime().toString()`, `${Z}.from(${q(b)}).toPlainDate().toString()`, `${Z}.from(${q(b)}).toPlainTime().toString()`,
    `${Z}.from(${q(b)}).toInstant().toString()`, `${Z}.from(${q(b)}).toJSON()`, `${Z}.from(${q(b)}).valueOf()`, `${Z}.from(${q(b)}).epochNanoseconds`,
    `${Z}.from(${q(b)}).inLeapYear`, `${Z}.from(${q(b)}).daysInMonth`, `${Z}.from(${q(b)}).weekOfYear`, `${Z}.from(${q(b)}).yearOfWeek`);
  for (const t of targets) add(`${Z}.from(${q(b)}).withTimeZone(${q(t)}).toString()`, `${Z}.from(${q(b)}).withTimeZone(${q(t)}).timeZoneId`);
  for (const u of ["day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
    for (const m of ["ceil", "floor", "trunc", "halfExpand", "halfEven"]) add(`${Z}.from(${q(b)}).round({smallestUnit: ${q(u)}, roundingMode: ${q(m)}}).toString()`);
  }
  add(`${Z}.from(${q(b)}).round("day").toString()`, `${Z}.from(${q(b)}).round({})`, `${Z}.from(${q(b)}).round()`, `${Z}.from(${q(b)}).round({smallestUnit: "month"})`,
    `${Z}.from(${q(b)}).round({smallestUnit: "hour", roundingIncrement: 5})`, `${Z}.from(${q(b)}).round({smallestUnit: "hour", roundingIncrement: 4})`,
    `${Z}.from(${q(b)}).round({smallestUnit: "day", roundingIncrement: 2})`, `${Z}.from(${q(b)}).round({smallestUnit: "minute", roundingIncrement: 90})`);
}

// ---------- until/since com largestUnit em dias de 23 e 25 horas ----------
const pairs = [
  ["2024-03-09T12:00[America/New_York]", "2024-03-10T12:00[America/New_York]"],
  ["2024-03-10T00:00[America/New_York]", "2024-03-11T00:00[America/New_York]"],
  ["2024-03-10T00:00[America/New_York]", "2024-03-10T12:00[America/New_York]"],
  ["2024-11-02T12:00[America/New_York]", "2024-11-03T12:00[America/New_York]"],
  ["2024-11-03T00:00[America/New_York]", "2024-11-04T00:00[America/New_York]"],
  ["2024-11-03T00:00[America/New_York]", "2024-11-03T12:00[America/New_York]"],
  ["2024-03-01T00:00[America/New_York]", "2024-04-01T00:00[America/New_York]"],
  ["2024-01-01T00:00[America/New_York]", "2025-01-01T00:00[America/New_York]"],
  ["2018-11-03T12:00[America/Sao_Paulo]", "2018-11-05T12:00[America/Sao_Paulo]"],
  ["2018-02-17T12:00[America/Sao_Paulo]", "2018-02-19T12:00[America/Sao_Paulo]"],
  ["2024-10-26T12:00[Europe/London]", "2024-10-28T12:00[Europe/London]"],
  ["2024-03-30T12:00[Europe/London]", "2024-04-01T12:00[Europe/London]"],
  ["2024-04-06T12:00[Australia/Lord_Howe]", "2024-04-08T12:00[Australia/Lord_Howe]"],
  ["2024-10-05T12:00[Australia/Lord_Howe]", "2024-10-07T12:00[Australia/Lord_Howe]"],
  ["2011-12-29T12:00[Pacific/Apia]", "2012-01-01T12:00[Pacific/Apia]"],
  ["2011-12-29T00:00[Pacific/Apia]", "2011-12-31T00:00[Pacific/Apia]"],
  ["2024-03-30T12:00[Africa/Casablanca]", "2024-04-01T12:00[Africa/Casablanca]"],
  ["2024-01-01T00:00[Asia/Kolkata]", "2024-01-02T05:30[Asia/Kolkata]"],
  ["2024-03-10T12:00[America/New_York]", "2024-03-10T12:00[America/Sao_Paulo]"],
  ["2024-03-10T12:00[America/New_York]", "2024-03-10T12:00[UTC]"],
  ["2024-03-10T12:00[America/New_York]", "2024-03-10T16:00[UTC]"],
];
for (const [a, b] of pairs) {
  for (const m of ["until", "since"]) {
    add(`${Z}.from(${q(a)}).${m}(${q(b)}).toString()`);
    for (const lu of ["auto", "year", "month", "week", "day", "hour", "minute", "second", "nanosecond"]) {
      add(`${Z}.from(${q(a)}).${m}(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
    }
    for (const su of ["day", "hour", "minute"]) {
      for (const rm of ["trunc", "halfExpand", "ceil"]) {
        add(`${Z}.from(${q(a)}).${m}(${q(b)}, {largestUnit: "day", smallestUnit: ${q(su)}, roundingMode: ${q(rm)}}).toString()`,
          `${Z}.from(${q(a)}).${m}(${q(b)}, {largestUnit: "month", smallestUnit: ${q(su)}, roundingMode: ${q(rm)}}).toString()`);
      }
    }
  }
  add(`${Z}.compare(${q(a)}, ${q(b)})`, `${Z}.from(${q(a)}).equals(${q(b)})`, `${Z}.from(${q(a)}).equals(${q(a)})`,
    `${Z}.from(${q(a)}).add(Temporal.Duration.from("P1D")).equals(${q(b)})`);
}

// ---------- add/subtract com duração em dias vs horas ----------
for (const b of ["2024-03-09T02:30[America/New_York]", "2024-03-10T01:30[America/New_York]", "2024-11-02T01:30[America/New_York]", "2024-11-03T00:30[America/New_York]", "2018-11-03T23:30[America/Sao_Paulo]", "2018-02-17T23:30[America/Sao_Paulo]", "2011-12-29T12:00[Pacific/Apia]"]) {
  for (const d of ["PT24H", "P1D", "P1DT1H", "PT23H", "PT25H", "P1W", "P1M", "-P1D", "-PT24H", "P1Y", "PT1.5S", "PT0.000000001S"]) {
    add(`${Z}.from(${q(b)}).add(${q(d)}).toString()`, `${Z}.from(${q(b)}).subtract(${q(d)}).toString()`,
      `${Z}.from(${q(b)}).add(${q(d)}, {overflow: "reject"}).toString()`);
  }
}
add(`${Z}.from("+275760-09-13T00:00[UTC]").add("PT1S")`, `${Z}.from("+275760-09-12T23:59:59[UTC]").add("PT1S").toString()`, `${Z}.from("+275760-09-12T23:59:59[UTC]").add("PT2S")`,
  `${Z}.from("-271821-04-20T00:00[UTC]").subtract("PT1S")`, `${Z}.from("-271821-04-20T00:00[UTC]").toString()`, `${Z}.from("-271821-04-20T00:00[UTC]").add("PT1S").toString()`,
  `${Z}.from("+275760-09-12T12:00[UTC]").add("P1D")`, `${Z}.from("+275760-09-12T12:00[UTC]").add({hours: 12}).toString()`);

// ---------- toString: timeZoneName, offset, calendarName, fractionalSecondDigits, smallestUnit ----------
const tsBase = ["2024-07-01T12:34:56.123456789[America/New_York]", "2024-07-01T12:34:56[Asia/Kolkata]", "2024-07-01T12:34[UTC]", "2024-07-01T00:00:00.5[+05:30]", "2024-07-01T00:00:00.000000001[-00:00:30.5]"];
for (const b of tsBase) {
  for (const tzn of ["auto", "never", "critical", "bad"]) add(`${Z}.from(${q(b)}).toString({timeZoneName: ${q(tzn)}})`);
  for (const of of ["auto", "never", "bad"]) add(`${Z}.from(${q(b)}).toString({offset: ${q(of)}})`);
  for (const cn of ["auto", "always", "never", "critical", "bad"]) add(`${Z}.from(${q(b)}).toString({calendarName: ${q(cn)}})`, `${Z}.from(${q(b)}).withCalendar("hebrew").toString({calendarName: ${q(cn)}})`);
  for (const f of ["auto", 0, 1, 3, 6, 9, 10, -1, "3", 2.9, null]) add(`${Z}.from(${q(b)}).toString({fractionalSecondDigits: ${JSON.stringify(f)}})`);
  for (const su of ["minute", "second", "millisecond", "microsecond", "nanosecond", "hour", "day"]) {
    add(`${Z}.from(${q(b)}).toString({smallestUnit: ${q(su)}})`);
    for (const rm of ["ceil", "floor", "halfExpand", "trunc"]) add(`${Z}.from(${q(b)}).toString({smallestUnit: ${q(su)}, roundingMode: ${q(rm)}})`);
  }
  add(`${Z}.from(${q(b)}).toString({smallestUnit: "second", fractionalSecondDigits: 3})`, `${Z}.from(${q(b)}).toString({timeZoneName: "never", offset: "never", calendarName: "never"})`,
    `${Z}.from(${q(b)}).toString(null)`, `${Z}.from(${q(b)}).toString("auto")`, `${Z}.from(${q(b)}).toString(5)`);
}
add(`${Z}.from("2024-07-01T23:59:59.9999[UTC]").toString({fractionalSecondDigits: 2})`, `${Z}.from("2024-07-01T23:59:59.999[UTC]").toString({smallestUnit: "second", roundingMode: "ceil"})`,
  `${Z}.from("2024-11-03T01:30-05:00[America/New_York]").toString({smallestUnit: "minute"})`, `${Z}.from("2024-11-03T01:59:59.9-04:00[America/New_York]").toString({smallestUnit: "second", roundingMode: "ceil"})`,
  `${Z}.from("2024-03-10T01:59:59.9-05:00[America/New_York]").toString({smallestUnit: "second", roundingMode: "ceil"})`);

// ---------- from(objeto) e campos ----------
const fields = [
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\"}", "{year: 2024, month: 1, day: 1, timeZone: \"Asia/Kolkata\", hour: 12, minute: 30}",
  "{year: 2024, monthCode: \"M01\", day: 1, timeZone: \"UTC\"}", "{year: 2024, month: 1, monthCode: \"M02\", day: 1, timeZone: \"UTC\"}",
  "{year: 2024, month: 13, day: 1, timeZone: \"UTC\"}", "{year: 2024, month: 1, day: 32, timeZone: \"UTC\"}", "{year: 2024, month: 1, day: 1}",
  "{year: 2024, month: 1, timeZone: \"UTC\"}", "{month: 1, day: 1, timeZone: \"UTC\"}", "{year: 2024, month: 1, day: 1, timeZone: \"Nowhere\"}",
  "{year: 2024, month: 1, day: 1, timeZone: 5}", "{year: 2024, month: 1, day: 1, timeZone: {}}", "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", offset: 5}",
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", offset: \"+1\"}", "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", offset: \"+01:00:00.5\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", hour: 25}", "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", hour: 25, minute: 61, second: 61}",
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", millisecond: 1000}", "{year: 2024, month: 2, day: 30, timeZone: \"UTC\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", era: \"ce\", eraYear: 2024}", "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", calendar: \"iso8601\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", calendar: \"foo\"}", "{year: 275760, month: 9, day: 13, timeZone: \"UTC\"}", "{year: -271821, month: 4, day: 19, hour: 23, timeZone: \"UTC\"}",
  "{year: 1.5, month: 1, day: 1, timeZone: \"UTC\"}", "{year: \"2024\", month: \"1\", day: \"1\", timeZone: \"UTC\"}", "{year: Infinity, month: 1, day: 1, timeZone: \"UTC\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"UTC\", hour: -1}", "{year: 2024, month: 1, day: 1, timeZone: \"+01:00\"}", "{year: 2024, month: 1, day: 1, timeZone: \"-0100\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"+01:00\", offset: \"+02:00\"}", "{year: 2024, month: 1, day: 1, timeZone: \"2024-01-01T00:00Z[UTC]\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"2024-01-01T00:00+05:30\"}", "{year: 2024, month: 1, day: 1, timeZone: \"2024-01-01T00:00Z\"}",
  "{year: 2024, month: 1, day: 1, timeZone: \"Asia/Kolkata\", offset: \"+05:30:00\"}",
];
for (const f of fields) {
  add(`${Z}.from(${f}).toString()`, `${Z}.from(${f}, {overflow: "reject"}).toString()`, `${Z}.from(${f}, {overflow: "constrain"}).toString()`,
    `${Z}.from(${f}, {overflow: "bad"}).toString()`, `${Z}.from(${f}, {offset: "reject"}).toString()`, `${Z}.from(${f}, {disambiguation: "bad"}).toString()`);
}
add(`${Z}.from()`, `${Z}.from(undefined)`, `${Z}.from(null)`, `${Z}.from(5)`, `${Z}.from(true)`, `${Z}.from(Symbol())`, `${Z}.from({})`, `${Z}.from([])`, `${Z}.from("UTC")`,
  `${Z}.from("2024-01-01T00:00[UTC]", 5)`, `${Z}.from("2024-01-01T00:00[UTC]", null)`, `${Z}.from("2024-01-01T00:00[UTC]", {offset: "bad"})`, `${Z}.from("2024-01-01T00:00[UTC]", "reject")`,
  `${Z}.from(${Z}.from("2024-01-01T00:00[UTC]")).toString()`, `${Z}.from(${Z}.from("2024-01-01T00:00[UTC]"), {overflow: "bad"})`, `${Z}.from(${Z}.from("2024-01-01T00:00[UTC]"), {offset: "bad"})`,
  `new ${Z}(0n, "UTC").toString()`, `new ${Z}(0n, "America/Sao_Paulo").toString()`, `new ${Z}(1n, "UTC", "iso8601").toString()`, `new ${Z}(0n)`, `new ${Z}(0, "UTC")`, `new ${Z}("0", "UTC")`,
  `new ${Z}(0n, "Nowhere/Land")`, `new ${Z}(0n, "UTC", "foo")`, `new ${Z}(0n, "+01:00").toString()`, `new ${Z}(0n, "+01:00:30").toString()`, `new ${Z}(0n, "+01:00:30.5").toString()`,
  `new ${Z}(8640000000000000000001n, "UTC")`, `new ${Z}(8640000000000000000000n, "UTC").toString()`, `new ${Z}(-8640000000000000000000n, "UTC").toString()`, `new ${Z}(-8640000000000000000001n, "UTC")`,
  `${Z}(0n, "UTC")`, `new ${Z}(0n, 5)`, `new ${Z}(0n, "UTC", 5)`, `new ${Z}(0n, {})`, `new ${Z}(0n, "utc").timeZoneId`, `new ${Z}(0n, "america/sao_paulo").timeZoneId`, `new ${Z}(0n, "Etc/UTC").timeZoneId`,
  `new ${Z}(0n, "Etc/GMT+3").timeZoneId`, `new ${Z}(0n, "GMT").timeZoneId`, `new ${Z}(0n, "Asia/Calcutta").timeZoneId`, `new ${Z}(0n, "Asia/Kolkata").timeZoneId`, `new ${Z}(0n, "US/Eastern").timeZoneId`,
  `new ${Z}(0n, "+0100").timeZoneId`, `new ${Z}(0n, "+01").timeZoneId`, `new ${Z}(0n, "-00:00")`, `new ${Z}(0n, "+00:00").timeZoneId`, `new ${Z}(0n, "+24:00")`, `new ${Z}(0n, "+23:59").toString()`,
  `${Z}.prototype.toString.call({})`, `${Z}.prototype.hoursInDay`, `Object.prototype.toString.call(${Z}.from("2024-01-01T00:00[UTC]"))`, `${Z}.from("2024-01-01T00:00[UTC]").valueOf()`,
  `${Z}.from("2024-01-01T00:00[UTC]") < ${Z}.from("2024-01-02T00:00[UTC]")`, `${Z}.compare(${Z}.from("2024-01-01T00:00[UTC]"), 5)`, `${Z}.compare("2024-01-01T00:00[UTC]", "2024-01-01T01:00+01:00[+01:00]")`,
  `${Z}.compare("2024-01-01T00:00[UTC]", "2024-01-01T00:00[Asia/Kolkata]")`, `${Z}.from("2024-01-01T00:00[UTC]").equals("2024-01-01T00:00[utc]")`, `${Z}.from("2024-01-01T00:00[UTC]").equals("2024-01-01T00:00[Etc/UTC]")`,
  `${Z}.from("2024-01-01T00:00[UTC]").equals("2024-01-01T00:00[+00:00]")`, `${Z}.from("2024-01-01T00:00[+01:00]").equals("2024-01-01T00:00[+01:00]")`, `${Z}.from("2024-01-01T00:00[UTC]").equals("2024-01-01T00:00[UTC][u-ca=hebrew]")`,
  `${Z}.from("2024-01-01T00:00[UTC]").equals({year: 2024, month: 1, day: 1, timeZone: "UTC"})`, `${Z}.from("2024-01-01T00:00[UTC]").until("2024-01-01T00:00[UTC][u-ca=hebrew]")`,
  `${Z}.from("2024-01-01T00:00[UTC]").until("2024-01-01T00:00[Asia/Kolkata]", {largestUnit: "day"})`, `${Z}.from("2024-01-01T00:00[UTC]").until("2024-01-01T00:00[Asia/Kolkata]", {largestUnit: "hour"}).toString()`,
  `${Z}.from("2024-01-01T00:00[UTC]").until("2024-01-01T00:00[Asia/Kolkata]", {largestUnit: "year"})`, `${Z}.from("2024-01-01T00:00[UTC]").until("2025-01-01T00:00[UTC]", {largestUnit: "year"}).toString()`,
  `${Z}.from("2024-01-01T00:00[UTC]").until("2025-01-01T00:00[UTC]", {largestUnit: "bad"})`, `${Z}.from("2024-01-01T00:00[UTC]").until("2025-01-01T00:00[UTC]", {smallestUnit: "year", largestUnit: "month"})`,
  `${Z}.from("2024-01-01T00:00[UTC]").until("2025-01-01T00:00[UTC]", {roundingIncrement: 0})`, `${Z}.from("2024-01-01T00:00[UTC]").until("2025-01-01T00:00[UTC]", {smallestUnit: "hour", roundingIncrement: 7})`,
  `${Z}.from("2024-01-01T00:00[UTC]").until("2025-01-01T00:00[UTC]", {smallestUnit: "hour", roundingIncrement: 24})`, `${Z}.from("2024-01-01T00:00[UTC]").until("2024-01-02T00:00[UTC]", {smallestUnit: "hour", roundingIncrement: 24})`,
  `${Z}.from("2024-01-01T00:00[UTC]").add({})`, `${Z}.from("2024-01-01T00:00[UTC]").add()`, `${Z}.from("2024-01-01T00:00[UTC]").add(5)`, `${Z}.from("2024-01-01T00:00[UTC]").add("P1D", {overflow: "bad"})`,
  `${Z}.from("2024-01-01T00:00[UTC]").with()`, `${Z}.from("2024-01-01T00:00[UTC]").with({})`, `${Z}.from("2024-01-01T00:00[UTC]").with("2024")`, `${Z}.from("2024-01-01T00:00[UTC]").with(${Z}.from("2024-01-01T00:00[UTC]"))`,
  `${Z}.from("2024-01-01T00:00[UTC]").with({timeZone: "UTC"})`, `${Z}.from("2024-01-01T00:00[UTC]").with({calendar: "iso8601"})`, `${Z}.from("2024-01-01T00:00[UTC]").with({hour: 1}, {offset: "bad"})`,
  `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone()`, `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone(5)`, `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone({})`,
  `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone(${Z}.from("2024-01-01T00:00[Asia/Kolkata]"))`, `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone("2024-01-01T00:00[Asia/Kolkata]").timeZoneId`,
  `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone("2024-01-01T00:00+05:30").timeZoneId`, `${Z}.from("2024-01-01T00:00[UTC]").withTimeZone("2024-01-01T00:00Z")`,
  `${Z}.from("2024-01-01T00:00[UTC]").getTimeZoneTransition()`, `${Z}.from("2024-01-01T00:00[UTC]").getTimeZoneTransition("sideways")`, `${Z}.from("2024-01-01T00:00[UTC]").getTimeZoneTransition({})`,
  `${Z}.from("2024-01-01T00:00[UTC]").getTimeZoneTransition("next")`, `${Z}.from("2024-01-01T00:00[+01:00]").getTimeZoneTransition("previous")`, `${Z}.from("2024-01-01T00:00[UTC]").getTimeZoneTransition({direction: "next"})`,
  `${Z}.from("2024-01-01T00:00[Asia/Kolkata]").getTimeZoneTransition("next")`, `${Z}.from("1900-01-01T00:00[Asia/Kolkata]").getTimeZoneTransition("next").toString()`, `${Z}.from("2024-01-01T00:00[America/New_York]").getTimeZoneTransition("previous").toString()`,
  `${Z}.from("2024-01-01T00:00[America/New_York]").getTimeZoneTransition("next").toString()`, `${Z}.from("2024-01-01T00:00[Africa/Casablanca]").getTimeZoneTransition("next").toString()`,
  `${Z}.from("2024-01-01T00:00[Africa/Casablanca]").getTimeZoneTransition("previous").toString()`, `${Z}.from("2011-12-01T00:00[Pacific/Apia]").getTimeZoneTransition("next").toString()`,
  `${Z}.from("2011-12-31T00:00[Pacific/Apia]").getTimeZoneTransition("previous").toString()`, `${Z}.from("2024-01-01T00:00[Australia/Lord_Howe]").getTimeZoneTransition("next").toString()`,
  `${Z}.from("2018-01-01T00:00[America/Sao_Paulo]").getTimeZoneTransition("next").toString()`, `${Z}.from("2019-06-01T00:00[America/Sao_Paulo]").getTimeZoneTransition("previous").toString()`,
  `${Z}.from("2019-06-01T00:00[America/Sao_Paulo]").getTimeZoneTransition("next")`, `${Z}.from("2024-01-01T00:00[Europe/London]").getTimeZoneTransition("next").toString()`,
  `${Z}.from("2024-03-31T01:00[Europe/London]").getTimeZoneTransition("next").toString()`, `${Z}.from("2024-03-31T01:00[Europe/London]").getTimeZoneTransition("previous").toString()`,
  `${Z}.prototype.getTimeZoneTransition.length`, `${Z}.prototype.getTimeZoneTransition.name`, `${Z}.prototype.startOfDay.length`, `${Z}.prototype.withTimeZone.length`, `${Z}.from.length`, `${Z}.compare.length`, `${Z}.length`,
);

// ---------- Instant ----------
const instants = [
  "1970-01-01T00:00:00Z", "2024-03-10T07:00:00Z", "2024-03-10T06:59:59.999999999Z", "2024-11-03T05:59:59Z", "2024-11-03T06:00:00Z", "1969-12-31T23:59:59.999999999Z",
  "2024-01-01T00:00:00.5Z", "2024-01-01T00:00:00+05:30", "2024-01-01T00:00:00-03:00", "2024-01-01T00:00:00+05:30:15", "2024-01-01T00:00:00.123456789+00:00:00.5",
  "2024-01-01T00:00[UTC]", "2024-01-01T00:00Z[UTC]", "2024-01-01T00:00Z[America/Sao_Paulo]", "2024-01-01T00:00+01:00[Europe/London]", "2024-01-01T00:00Z[u-ca=hebrew]",
  "2024-01-01T00:00", "2024-01-01", "2024-01-01T00:00z", "2024-01-01t00:00Z", "2024-01-01 00:00Z", "-271821-04-20T00:00:00Z", "+275760-09-13T00:00:00Z",
  "-271821-04-19T23:59:59.999999999Z", "+275760-09-13T00:00:00.000000001Z", "2024-01-01T24:00:00Z", "2024-01-01T23:59:60Z", "2024-02-30T00:00Z", "2024-01-01T00:00:00,5Z",
  "20240101T000000Z", "2024-01-01T00Z", "2024-01-01T0000Z", "+002024-01-01T00:00Z", "-002024-01-01T00:00Z", "-000000-01-01T00:00Z", "2024-01-01T00:00:00.1234567891Z", "", "Z", "now",
  "2024-01-01T00:00+24:00", "2024-01-01T00:00+23:59", "2024-01-01T00:00-23:59:59.999999999", "2024-01-01T00:00+01", "2024-01-01T00:00+0100", "2024-01-01T00:00\u221201:00",
];
for (const s of instants) {
  add(`${I}.from(${q(s)}).toString()`, `${I}.from(${q(s)}).epochNanoseconds`, `${I}.from(${q(s)}).epochMilliseconds`);
}
const instBase = ["2024-03-10T07:00:00.123456789Z", "1969-12-31T23:59:59.999999999Z", "2024-11-03T05:30:00Z"];
for (const b of instBase) {
  for (const d of ["PT1H", "-PT1H", "PT0.000000001S", "PT24H", "-PT24H", "PT36500H", "P1D", "P1W", "P1M", "P1Y", "PT0S", "PT1.5S", "PT-1S", "PT1H30M", "-PT1H30M", "P0D"]) {
    add(`${I}.from(${q(b)}).add(${q(d)}).toString()`, `${I}.from(${q(b)}).subtract(${q(d)}).toString()`);
  }
  for (const d of ["{hours: 1}", "{days: 1}", "{weeks: 1}", "{months: 1}", "{years: 1}", "{hours: 1.5}", "{hours: -1}", "{nanoseconds: 1}", "{microseconds: 1}", "{milliseconds: 1}", "{seconds: 1, nanoseconds: 999999999}", "{}", "{foo: 1}", "5", "null"]) {
    add(`${I}.from(${q(b)}).add(${d}).toString()`, `${I}.from(${q(b)}).subtract(${d}).toString()`);
  }
  for (const u of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "day", "week", "month", "year"]) {
    for (const m of ["ceil", "floor", "trunc", "halfExpand", "halfCeil", "halfFloor", "halfTrunc", "halfEven", "expand"]) add(`${I}.from(${q(b)}).round({smallestUnit: ${q(u)}, roundingMode: ${q(m)}}).toString()`);
    for (const inc of [1, 2, 3, 5, 7, 10, 15, 24, 30, 60, 100, 500, 1000, 0]) add(`${I}.from(${q(b)}).round({smallestUnit: ${q(u)}, roundingIncrement: ${inc}}).toString()`);
    add(`${I}.from(${q(b)}).toString({smallestUnit: ${q(u)}})`);
  }
  add(`${I}.from(${q(b)}).round("hour").toString()`, `${I}.from(${q(b)}).round({})`, `${I}.from(${q(b)}).round()`, `${I}.from(${q(b)}).round(5)`, `${I}.from(${q(b)}).round({smallestUnit: "hour", roundingMode: "bad"})`,
    `${I}.from(${q(b)}).toString({timeZone: "Asia/Kolkata"})`, `${I}.from(${q(b)}).toString({timeZone: "+05:30"})`, `${I}.from(${q(b)}).toString({timeZone: "America/Sao_Paulo", fractionalSecondDigits: 3})`,
    `${I}.from(${q(b)}).toString({timeZone: "Nowhere/Land"})`, `${I}.from(${q(b)}).toString({timeZone: 5})`, `${I}.from(${q(b)}).toString({timeZone: "utc"})`, `${I}.from(${q(b)}).toString({timeZone: "+01:00:30"})`,
    `${I}.from(${q(b)}).toString({timeZone: "2024-01-01T00:00Z"})`, `${I}.from(${q(b)}).toString({timeZone: "2024-01-01T00:00+05:30"})`, `${I}.from(${q(b)}).toString({timeZone: "2024-01-01T00:00[Asia/Kolkata]"})`,
    `${I}.from(${q(b)}).toString({fractionalSecondDigits: 0})`, `${I}.from(${q(b)}).toString({fractionalSecondDigits: 4})`, `${I}.from(${q(b)}).toString({fractionalSecondDigits: "auto"})`,
    `${I}.from(${q(b)}).toString({fractionalSecondDigits: 10})`, `${I}.from(${q(b)}).toJSON()`, `${I}.from(${q(b)}).valueOf()`, `${I}.from(${q(b)}).toLocaleString === undefined`,
    `${I}.from(${q(b)}).toZonedDateTimeISO("UTC").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("America/New_York").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("America/Sao_Paulo").toString()`,
    `${I}.from(${q(b)}).toZonedDateTimeISO("Asia/Kolkata").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("Pacific/Apia").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("Australia/Lord_Howe").toString()`,
    `${I}.from(${q(b)}).toZonedDateTimeISO("Africa/Casablanca").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("Europe/London").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("+05:30").toString()`,
    `${I}.from(${q(b)}).toZonedDateTimeISO("-00:00:30.5").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("+01:00:30.123456789").toString()`, `${I}.from(${q(b)}).toZonedDateTimeISO("Nowhere/Land")`,
    `${I}.from(${q(b)}).toZonedDateTimeISO()`, `${I}.from(${q(b)}).toZonedDateTimeISO(5)`, `${I}.from(${q(b)}).toZonedDateTimeISO({timeZone: "UTC"})`, `${I}.from(${q(b)}).toZonedDateTimeISO("utc").timeZoneId`,
    `${I}.from(${q(b)}).toZonedDateTimeISO("2024-01-01T00:00[Asia/Kolkata]").timeZoneId`, `${I}.from(${q(b)}).toZonedDateTimeISO("2024-01-01T00:00+05:30").timeZoneId`, `${I}.from(${q(b)}).toZonedDateTimeISO("2024-01-01T00:00Z")`,
    `${I}.from(${q(b)}).toZonedDateTime("UTC")`, `${I}.from(${q(b)}).toZonedDateTime`, `${I}.from(${q(b)}).epochSeconds`, `${I}.from(${q(b)}).epochMicroseconds`);
  for (const o of instBase.concat(["1970-01-01T00:00:00Z", "2024-03-10T07:00:00+05:30", "2024-03-10T07:00:00Z[Asia/Kolkata]", "+275760-09-13T00:00:00Z", "-271821-04-20T00:00:00Z"])) {
    for (const m of ["until", "since"]) {
      add(`${I}.from(${q(b)}).${m}(${q(o)}).toString()`, `${I}.from(${q(b)}).${m}(${q(o)}, {largestUnit: "second"}).toString()`, `${I}.from(${q(b)}).${m}(${q(o)}, {largestUnit: "nanosecond"}).toString()`,
        `${I}.from(${q(b)}).${m}(${q(o)}, {largestUnit: "minute", smallestUnit: "second", roundingMode: "halfExpand"}).toString()`,
        `${I}.from(${q(b)}).${m}(${q(o)}, {smallestUnit: "hour", roundingMode: "ceil", roundingIncrement: 4}).toString()`);
    }
    add(`${I}.compare(${q(b)}, ${q(o)})`, `${I}.from(${q(b)}).equals(${q(o)})`);
  }
}
for (const lu of ["auto", "day", "week", "month", "year", "hour", "bad"]) {
  add(`${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", {largestUnit: ${q(lu)}}).toString()`, `${I}.from("2024-01-01T00:00Z").since("2024-01-02T00:00Z", {largestUnit: ${q(lu)}}).toString()`);
}
add(`${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", {smallestUnit: "day"})`, `${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", {smallestUnit: "hour", largestUnit: "second"}).toString()`,
  `${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", {smallestUnit: "second", largestUnit: "hour"}).toString()`, `${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", {roundingIncrement: 2, smallestUnit: "hour"}).toString()`,
  `${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", {roundingIncrement: 25, smallestUnit: "hour"})`, `${I}.from("2024-01-01T00:00Z").until("2024-01-02T00:00Z", 5)`, `${I}.from("2024-01-01T00:00Z").until()`,
  `${I}.from("2024-01-01T00:00Z").until(5)`, `${I}.from("2024-01-01T00:00Z").until({})`, `${I}.from("2024-01-01T00:00Z").until(${Z}.from("2024-01-02T00:00[UTC]")).toString()`,
  `${I}.from(${Z}.from("2024-01-02T00:00[UTC]")).toString()`, `${I}.from(5)`, `${I}.from()`, `${I}.from(null)`, `${I}.from({})`, `${I}.from(5n)`, `${I}.from(true)`, `${I}.from(Symbol())`,
  `${I}.fromEpochMilliseconds(0).toString()`, `${I}.fromEpochMilliseconds(1.5)`, `${I}.fromEpochMilliseconds(8.64e15).toString()`, `${I}.fromEpochMilliseconds(8.64e15 + 1)`, `${I}.fromEpochMilliseconds(-8.64e15).toString()`,
  `${I}.fromEpochMilliseconds("5").toString()`, `${I}.fromEpochMilliseconds(5n)`, `${I}.fromEpochMilliseconds()`, `${I}.fromEpochMilliseconds(NaN)`, `${I}.fromEpochMilliseconds(Infinity)`, `${I}.fromEpochMilliseconds(-0).toString()`,
  `${I}.fromEpochNanoseconds(0n).toString()`, `${I}.fromEpochNanoseconds(1n).toString()`, `${I}.fromEpochNanoseconds(-1n).toString()`, `${I}.fromEpochNanoseconds(8640000000000000000000n).toString()`, `${I}.fromEpochNanoseconds(8640000000000000000001n)`,
  `${I}.fromEpochNanoseconds(-8640000000000000000000n).toString()`, `${I}.fromEpochNanoseconds(-8640000000000000000001n)`, `${I}.fromEpochNanoseconds(0)`, `${I}.fromEpochNanoseconds("5")`, `${I}.fromEpochNanoseconds()`,
  `${I}.fromEpochNanoseconds(true)`, `${I}.fromEpochNanoseconds(1.5)`, `${I}.fromEpochSeconds`, `${I}.fromEpochMicroseconds`,
  `new ${I}(0n).toString()`, `new ${I}(0)`, `new ${I}("0")`, `new ${I}()`, `new ${I}(1.5)`, `${I}(0n)`, `new ${I}(8640000000000000000001n)`, `new ${I}(-8640000000000000000001n)`, `new ${I}(true)`, `new ${I}(null)`,
  `new ${I}(0n).epochMilliseconds`, `new ${I}(-1n).epochMilliseconds`, `new ${I}(-1000000n).epochMilliseconds`, `new ${I}(-999999n).epochMilliseconds`, `new ${I}(1999999n).epochMilliseconds`, `new ${I}(-1999999n).epochMilliseconds`,
  `new ${I}(0n).valueOf()`, `new ${I}(0n) < new ${I}(1n)`, `${I}.prototype.toString.call({})`, `Object.prototype.toString.call(new ${I}(0n))`, `${I}.prototype.epochMilliseconds`, `${I}.prototype.add.length`, `${I}.prototype.round.length`,
  `${I}.prototype.until.length`, `${I}.prototype.toZonedDateTimeISO.length`, `${I}.from.length`, `${I}.compare.length`, `${I}.length`, `${I}.fromEpochMilliseconds.length`, `${I}.fromEpochNanoseconds.length`, `${I}.prototype[Symbol.toStringTag]`,
  `${I}.compare()`, `${I}.compare(0n, 0n)`, `${I}.compare("2024-01-01T00:00Z", "2024-01-01T00:00:00.000000001Z")`, `new ${I}(0n).equals()`, `new ${I}(0n).equals("1970-01-01T00:00Z")`, `new ${I}(0n).equals(5)`,
  `new ${I}(0n).add("PT0.0000000001S")`, `new ${I}(0n).add({hours: 2 ** 53})`, `new ${I}(8640000000000000000000n).add("PT1S")`, `new ${I}(-8640000000000000000000n).subtract("PT1S")`, `new ${I}(8640000000000000000000n).add("-PT1S").toString()`,
  `new ${I}(0n).add({nanoseconds: 9007199254740992})`, `new ${I}(0n).add({microseconds: 9007199254740991, nanoseconds: 9007199254740991}).toString()`, `new ${I}(0n).add({milliseconds: -9007199254740991}).toString()`,
  `new ${I}(0n).add(Temporal.Duration.from({hours: 1})).toString()`, `new ${I}(0n).add(Temporal.Duration.from({days: 1}))`, `new ${I}(0n).add(Temporal.Duration.from({months: 1}))`, `new ${I}(0n).add(Temporal.Duration.from({weeks: 1}))`,
  `new ${I}(0n).add(Temporal.Duration.from({years: 1}))`, `new ${I}(0n).add({days: 0}).toString()`,
);

// ---------- Temporal.Now: só forma e tipo ----------
add(`typeof Temporal.Now`, `Object.prototype.toString.call(Temporal.Now)`, `Object.keys(Temporal.Now)`, `Object.getOwnPropertyNames(Temporal.Now).sort()`, `Temporal.Now[Symbol.toStringTag]`,
  `typeof Temporal.Now.instant()`, `Temporal.Now.instant() instanceof ${I}`, `Temporal.Now.instant().constructor === ${I}`, `typeof Temporal.Now.timeZoneId()`, `Temporal.Now.timeZoneId().length > 0`,
  `Temporal.Now.timeZoneId.length`, `Temporal.Now.instant.length`, `Temporal.Now.zonedDateTimeISO.length`, `Temporal.Now.plainDateTimeISO.length`, `Temporal.Now.plainDateISO.length`, `Temporal.Now.plainTimeISO.length`,
  `Temporal.Now.zonedDateTimeISO() instanceof ${Z}`, `Temporal.Now.zonedDateTimeISO().calendarId`, `Temporal.Now.zonedDateTimeISO().timeZoneId === Temporal.Now.timeZoneId()`, `Temporal.Now.zonedDateTimeISO("UTC").timeZoneId`,
  `Temporal.Now.zonedDateTimeISO("Asia/Kolkata").timeZoneId`, `Temporal.Now.zonedDateTimeISO("+05:30").offset`, `Temporal.Now.zonedDateTimeISO("Asia/Kolkata").offset`, `Temporal.Now.zonedDateTimeISO("Nowhere/Land")`,
  `Temporal.Now.zonedDateTimeISO(5)`, `Temporal.Now.zonedDateTimeISO({})`, `Temporal.Now.zonedDateTimeISO("utc").timeZoneId`, `Temporal.Now.zonedDateTimeISO(undefined).calendarId`,
  `Temporal.Now.plainDateTimeISO() instanceof Temporal.PlainDateTime`, `Temporal.Now.plainDateTimeISO("UTC").calendarId`, `Temporal.Now.plainDateTimeISO("Nowhere/Land")`, `Temporal.Now.plainDateISO() instanceof Temporal.PlainDate`,
  `Temporal.Now.plainDateISO("UTC").calendarId`, `Temporal.Now.plainTimeISO() instanceof Temporal.PlainTime`, `Temporal.Now.plainTimeISO("Asia/Kolkata").toString().length >= 8`, `Temporal.Now.plainTimeISO("Nowhere/Land")`,
  `/^\\d{4}-\\d\\d-\\d\\dT\\d\\d:\\d\\d:\\d\\d(\\.\\d+)?Z$/.test(Temporal.Now.instant().toString())`, `/^[+-]\\d\\d:\\d\\d(:\\d\\d(\\.\\d+)?)?$/.test(Temporal.Now.zonedDateTimeISO().offset)`,
  `typeof Temporal.Now.instant().epochNanoseconds`, `Temporal.Now.instant().epochMilliseconds > 1.7e12`, `typeof Temporal.Now.zonedDateTimeISO().epochMilliseconds`, `Temporal.Now.zonedDateTimeISO().hoursInDay > 0`,
  `Temporal.Now.zonedDateTimeISO("+05:30").hoursInDay`, `Temporal.Now.zonedDateTimeISO("UTC").hoursInDay`, `Temporal.Now.zonedDateTimeISO("UTC").startOfDay().toString().endsWith("T00:00:00+00:00[UTC]")`,
  `new Temporal.Now()`, `Temporal.Now()`, `Object.getPrototypeOf(Temporal.Now) === Object.prototype`, `Object.isExtensible(Temporal.Now)`, `Object.getOwnPropertyDescriptor(Temporal.Now, "instant").enumerable`,
  `Object.getOwnPropertyDescriptor(Temporal.Now, "instant").writable`, `Object.getOwnPropertyDescriptor(Temporal.Now, "instant").configurable`, `Temporal.Now.timeZone`, `Temporal.Now.calendar`, `Temporal.Now.plainDate`,
  `Temporal.Now.instant.call(null) instanceof ${I}`, `Temporal.Now.timeZoneId.name`, `Temporal.Now.zonedDateTimeISO.name`);

// ---------- offsets nomeados ±HH:MM:SS.sss ----------
const offs = ["+00:00", "-00:00", "+01", "+0100", "+01:00", "-01:00", "+01:30", "+01:00:00", "+01:00:30", "-01:00:30", "+01:00:30.5", "+01:00:30.123456789", "+01:00:30.1234567891", "+01:00:30,5", "+23:59", "-23:59",
  "+23:59:59.999999999", "-23:59:59.999999999", "+24:00", "-24:00", "+00:60", "+00:00:60", "+1:00", "+01:0", "+01:00:0", "01:00", "+01:00:30.", "+01:00.5", "Z", "z", "+01:00[UTC]", "\u221201:00", "+01 :00", "+00:00:00.000000001", "-00:00:00.000000001"];
for (const o of offs) {
  add(`${Z}.from(${q(`2024-01-01T00:00${o}[UTC]`)}, {offset: "ignore"}).toString()`, `${Z}.from(${q(`2024-01-01T00:00${o}[UTC]`)}).toString()`, `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).toString()`,
    `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).offset`, `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).offsetNanoseconds`, `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).epochNanoseconds`,
    `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).timeZoneId`, `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).hoursInDay`, `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).startOfDay().toString()`,
    `${Z}.from(${q(`2024-01-01T00:00[${o}]`)}).withTimeZone("UTC").toString()`, `${Z}.from(${q(`2024-01-01T00:00[UTC]`)}).withTimeZone(${q(o)}).toString()`,
    `${Z}.from({year: 2024, month: 1, day: 1, timeZone: ${q(o)}}).toString()`, `${Z}.from({year: 2024, month: 1, day: 1, timeZone: "UTC", offset: ${q(o)}}).toString()`,
    `${Z}.from({year: 2024, month: 1, day: 1, timeZone: "UTC", offset: ${q(o)}}, {offset: "ignore"}).toString()`, `new ${Z}(0n, ${q(o)}).toString()`,
    `${I}.from(${q(`2024-01-01T00:00${o}`)}).toString()`, `${I}.from("2024-01-01T00:00Z").toZonedDateTimeISO(${q(o)}).toString()`, `${I}.from("2024-01-01T00:00Z").toString({timeZone: ${q(o)}})`,
    `${Z}.from("2024-01-01T00:00[UTC]").equals(${q(`2024-01-01T00:00[${o}]`)})`, `${Z}.compare(${q(`2024-01-01T00:00[${o}]`)}, ${q(`2024-01-01T00:00[UTC]`)})`);
}

// ---------- Amostragem determinística (sampleByHash) até o LIMIT ----------
const chosen = sampleByHash(programs, LIMIT);
const lines = [];
for (const source of chosen) {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  lines.push(source + "\t" + harness(source));
}
const target = path.join(__dirname, "../tests/golden/temporal_zoned_bun.tsv");
fs.writeFileSync(target, lines.join("\n") + "\n");
console.error(`${lines.length} programas escritos (de ${programs.length} gerados) em tests/golden/temporal_zoned_bun.tsv`);
