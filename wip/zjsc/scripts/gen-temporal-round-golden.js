// Gera tests/golden/temporal_round_bun.tsv: arredondamento e diferença de Temporal medidos no bun 1.4.2.
// Cobre round (PlainTime, PlainDateTime, Instant, ZonedDateTime, Duration com relativeTo), since/until com
// largestUnit, smallestUnit, roundingMode e roundingIncrement, with/add/subtract com overflow, opções de toString,
// compare, from com strings ISO extensas, calendários não ISO e deslocamentos de fuso, incluindo as mensagens
// exatas de RangeError e TypeError. Programas cuja expressão já aparece em qualquer golden de Temporal existente
// são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host, e grava `globalThis.R` (`String(valor)` ou `Nome: mensagem`).
// Uso: bun scripts/gen-temporal-round-golden.js > tests/golden/temporal_round_bun.tsv
const fs = require("fs");
const { emitRow, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = (text) => JSON.stringify(text);

const PT = "Temporal.PlainTime";
const PD = "Temporal.PlainDate";
const PDT = "Temporal.PlainDateTime";
const PYM = "Temporal.PlainYearMonth";
const PMD = "Temporal.PlainMonthDay";
const ZDT = "Temporal.ZonedDateTime";
const INS = "Temporal.Instant";
const DUR = "Temporal.Duration";

const smallTime = ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"];
const modes = ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"];

// ---- 1. PlainTime.round: horários x unidade x modo x incremento.
const times = ["12:34:56.789123456", "00:00:00", "23:59:59.999999999", "01:30:00", "12:00:00.5", "07:07:07.007007007", "15:45:30.250"];
for (const t of times) {
  for (const unit of smallTime) {
    for (const mode of ["ceil", "floor", "expand", "trunc", "halfEven", "halfExpand"]) {
      add(`${PT}.from(${q(t)}).round({smallestUnit:${q(unit)},roundingMode:${q(mode)}})`);
    }
  }
}
for (const t of ["12:34:56.789123456", "23:59:59.999999999", "05:17:00"]) {
  for (const [unit, incs] of [["hour", [1, 2, 3, 4, 6, 8, 12, 24, 5, 0, -1]], ["minute", [1, 5, 10, 15, 20, 30, 60, 7]], ["second", [1, 15, 30, 60, 45]],
    ["millisecond", [1, 100, 250, 500, 1000, 300]], ["microsecond", [1, 500, 1000]], ["nanosecond", [1, 500, 1000, 7]]]) {
    for (const inc of incs) add(`${PT}.from(${q(t)}).round({smallestUnit:${q(unit)},roundingIncrement:${inc}})`);
  }
}
add(`${PT}.from("12:00").round()`, `${PT}.from("12:00").round({})`, `${PT}.from("12:00").round("minute")`, `${PT}.from("12:00").round("day")`,
  `${PT}.from("12:00").round({smallestUnit:"day"})`, `${PT}.from("12:00").round({smallestUnit:"hours"})`, `${PT}.from("12:00").round({smallestUnit:"foo"})`,
  `${PT}.from("12:00").round({smallestUnit:"hour",roundingMode:"nearest"})`, `${PT}.from("12:00").round({smallestUnit:"hour",roundingIncrement:Infinity})`,
  `${PT}.from("12:00").round({smallestUnit:"hour",roundingIncrement:"x"})`, `${PT}.from("12:00").round({smallestUnit:"hour",roundingIncrement:1.9})`,
  `${PT}.from("12:00").round(null)`, `${PT}.from("12:00").round(5)`, `${PT}.from("12:00").round(true)`);

// ---- 2. PlainDateTime.round.
const dts = ["2024-02-29T23:59:59.999999999", "2024-12-31T23:30", "1999-01-01T00:00:00.000000001", "2020-06-15T12:34:56.5", "-000001-12-31T23:59:59.9"];
for (const dt of dts) {
  for (const unit of ["day", ...smallTime]) {
    for (const mode of modes) add(`${PDT}.from(${q(dt)}).round({smallestUnit:${q(unit)},roundingMode:${q(mode)}})`);
  }
}
for (const dt of dts.slice(0, 3)) {
  for (const [unit, inc] of [["day", 1], ["day", 2], ["hour", 5], ["hour", 6], ["minute", 45], ["minute", 30], ["second", 45], ["millisecond", 1000], ["nanosecond", 1000]]) {
    add(`${PDT}.from(${q(dt)}).round({smallestUnit:${q(unit)},roundingIncrement:${inc}})`);
  }
}
add(`${PDT}.from("2020-01-01T00:00").round()`, `${PDT}.from("2020-01-01T00:00").round("days")`, `${PDT}.from("2020-01-01T00:00").round({smallestUnit:"month"})`,
  `${PDT}.from("+275760-09-13T23:59:59.999999999").round({smallestUnit:"day"})`, `${PDT}.from("+275760-09-13T23:59:59.999999999").round({smallestUnit:"second"})`,
  `${PDT}.from("-271821-04-19T00:00:00.000000001").round({smallestUnit:"day",roundingMode:"floor"})`);

// ---- 3. Instant.round e toString com timeZone.
const instants = ["2024-03-10T12:34:56.789123456Z", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59.999999999Z", "2000-02-29T23:59:30Z", "2024-03-10T12:34:56+05:30"];
for (const i of instants) {
  for (const unit of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
    for (const mode of ["ceil", "floor", "expand", "trunc", "halfExpand", "halfEven", "halfCeil", "halfFloor", "halfTrunc"]) {
      add(`${INS}.from(${q(i)}).round({smallestUnit:${q(unit)},roundingMode:${q(mode)}}).toString()`);
    }
  }
  for (const tz of ["UTC", "+05:30", "-08:00", "America/Sao_Paulo", "Asia/Kolkata", "Europe/London"]) add(`${INS}.from(${q(i)}).toString({timeZone:${q(tz)}})`);
  for (const o of ["{fractionalSecondDigits:0}", "{fractionalSecondDigits:3}", "{fractionalSecondDigits:9}", "{fractionalSecondDigits:'auto'}", "{smallestUnit:'minute'}", "{smallestUnit:'second',roundingMode:'ceil'}",
    "{smallestUnit:'millisecond',roundingMode:'floor'}", "{fractionalSecondDigits:10}", "{fractionalSecondDigits:-1}", "{smallestUnit:'hour'}", "{smallestUnit:'day'}"]) {
    add(`${INS}.from(${q(i)}).toString(${o})`);
  }
}
for (const inc of [1, 2, 3, 4, 6, 8, 12, 24]) add(`${INS}.from("2024-03-10T12:34:56Z").round({smallestUnit:"hour",roundingIncrement:${inc}}).toString()`);
for (const inc of [1, 5, 15, 30, 60, 7]) add(`${INS}.from("2024-03-10T12:34:56Z").round({smallestUnit:"minute",roundingIncrement:${inc}}).toString()`);
add(`${INS}.from("2024-03-10T12:34:56Z").round({smallestUnit:"day"})`, `${INS}.from("2024-03-10T12:34:56Z").round()`, `${INS}.from("2024-03-10T12:34:56Z").round("hour").toString()`);

// ---- 4. Duration.round com relativeTo (PlainDate e ZonedDateTime), largestUnit, smallestUnit, modo.
const durs = ["P1Y2M3W4DT5H6M7S", "P400D", "PT100H", "P1M31D", "-P1Y6M", "P2Y11M29DT23H59M59.999S", "PT0.5S", "P7D", "P36500D"];
const units = ["years", "months", "weeks", "days", "hours", "minutes", "seconds"];
for (const d of durs) {
  for (const lu of ["years", "months", "weeks", "days", "hours", "auto"]) {
    for (const su of ["days", "hours", "minutes", "seconds"]) {
      add(`${DUR}.from(${q(d)}).round({largestUnit:${q(lu)},smallestUnit:${q(su)},relativeTo:"2024-01-31"}).toString()`);
    }
  }
  for (const mode of modes) add(`${DUR}.from(${q(d)}).round({smallestUnit:"months",roundingMode:${q(mode)},relativeTo:"2024-01-31"}).toString()`);
  for (const mode of ["ceil", "floor", "halfExpand", "trunc"]) add(`${DUR}.from(${q(d)}).round({smallestUnit:"weeks",largestUnit:"years",roundingMode:${q(mode)},relativeTo:"2023-08-15"}).toString()`);
  for (const u of units) add(`${DUR}.from(${q(d)}).total({unit:${q(u)},relativeTo:"2024-02-10"})`);
  add(`${DUR}.from(${q(d)}).round({largestUnit:"hours"})`, `${DUR}.from(${q(d)}).round({smallestUnit:"hours"}).toString()`, `${DUR}.from(${q(d)}).round({largestUnit:"days"}).toString()`);
  add(`${DUR}.from(${q(d)}).round({largestUnit:"days",relativeTo:Temporal.ZonedDateTime.from("2024-03-09T12:00[America/New_York]")}).toString()`,
    `${DUR}.from(${q(d)}).round({largestUnit:"hours",relativeTo:Temporal.ZonedDateTime.from("2024-03-09T12:00[America/New_York]")}).toString()`,
    `${DUR}.from(${q(d)}).total({unit:"hours",relativeTo:Temporal.ZonedDateTime.from("2024-11-02T12:00[America/New_York]")})`);
}
for (const inc of [1, 2, 5, 10, 15, 30, 59, 60]) add(`${DUR}.from("PT7H33M45S").round({smallestUnit:"minutes",roundingIncrement:${inc}}).toString()`);
for (const inc of [1, 2, 3, 4, 6, 8, 12, 24]) add(`${DUR}.from("P1DT7H33M").round({smallestUnit:"hours",roundingIncrement:${inc}}).toString()`);
add(`${DUR}.from("P1M").round({largestUnit:"days"})`, `${DUR}.from("P1Y").round({smallestUnit:"days"})`, `${DUR}.from("P1W").round({smallestUnit:"days"})`,
  `${DUR}.from("P1D").round({})`, `${DUR}.from("P1D").round()`, `${DUR}.from("P1D").round({smallestUnit:"hours",largestUnit:"minutes"})`,
  `${DUR}.from("P1D").round({smallestUnit:"years",largestUnit:"days",relativeTo:"2024-01-01"})`, `${DUR}.from("P1D").total("hours")`, `${DUR}.from("P1D").total({})`,
  `${DUR}.from("P1D").total({unit:"nanoseconds"})`, `${DUR}.from("P1D").total({unit:"foo"})`, `${DUR}.from("P1Y").total({unit:"days"})`, `${DUR}.from("P1M").total({unit:"weeks"})`,
  `${DUR}.from("P1D").round({relativeTo:"nonsense",smallestUnit:"days"})`, `${DUR}.from("P1D").round({relativeTo:5,smallestUnit:"days"})`,
  `${DUR}.from("P1D").round({relativeTo:{year:2024,month:1,day:1},smallestUnit:"days"}).toString()`, `${DUR}.from("P1D").round({relativeTo:{year:2024,month:1},smallestUnit:"days"})`);

// ---- 5. PlainDate until/since: pares x largestUnit x smallestUnit x modo.
const datePairs = [["2024-01-31", "2024-03-01"], ["2020-02-29", "2024-02-28"], ["2024-03-31", "2024-02-29"], ["1999-12-31", "2000-01-01"], ["2000-01-01", "2100-12-31"],
  ["2024-05-31", "2024-06-30"], ["2024-12-25", "2024-01-01"], ["-000100-01-01", "000100-12-31"]];
for (const [a, b] of datePairs) {
  for (const lu of ["years", "months", "weeks", "days", "auto"]) {
    add(`${PD}.from(${q(a)}).until(${q(b)},{largestUnit:${q(lu)}}).toString()`, `${PD}.from(${q(a)}).since(${q(b)},{largestUnit:${q(lu)}}).toString()`);
  }
  for (const su of ["years", "months", "weeks"]) {
    for (const mode of ["ceil", "floor", "expand", "trunc", "halfExpand", "halfEven"]) {
      add(`${PD}.from(${q(a)}).until(${q(b)},{largestUnit:"years",smallestUnit:${q(su)},roundingMode:${q(mode)}}).toString()`);
    }
  }
  add(`${PD}.from(${q(a)}).until(${q(b)},{largestUnit:"months",smallestUnit:"months",roundingIncrement:3}).toString()`, `${PD}.from(${q(a)}).since(${q(b)},{smallestUnit:"weeks",roundingIncrement:2}).toString()`);
}
add(`${PD}.from("2024-01-01").until("2024-02-01",{largestUnit:"hours"})`, `${PD}.from("2024-01-01").until("2024-02-01",{smallestUnit:"hours"})`, `${PD}.from("2024-01-01").until("2024-02-01",{largestUnit:"days",smallestUnit:"months"})`,
  `${PD}.from("2024-01-01").until("2024-02-01",{largestUnit:"foo"})`, `${PD}.from("2024-01-01").until("2024-02-01",{roundingIncrement:0})`, `${PD}.from("2024-01-01").until("2024-02-01",{roundingIncrement:2,smallestUnit:"days"}).toString()`,
  `${PD}.from("2024-01-01").until("2024-02-01","days")`, `${PD}.from("2024-01-01").until()`, `${PD}.from("2024-01-01").until(undefined)`, `${PD}.from("2024-01-01").until({year:2024,month:3,day:1}).toString()`,
  `${PD}.from("2024-01-01").until("2024-01-01T10:00").toString()`, `${PD}.from("2024-01-01").until("2024-01-01[u-ca=gregory]")`, `${PD}.from("2024-01-01[u-ca=gregory]").until("2024-02-01[u-ca=iso8601]")`,
  `${PD}.from("2024-01-01").until(new Date(0))`, `${PD}.from("2024-01-01").until(5)`, `${PD}.from("2024-01-01").until(null)`);

// ---- 6. PlainDateTime until/since com smallestUnit, modo, incremento.
const dtPairs = [["2024-01-31T10:00", "2024-03-01T09:59:59.999"], ["2020-02-29T23:59", "2024-02-28T00:01"], ["2024-06-15T12:00", "2024-06-15T12:00"], ["2024-12-31T23:59:59.999999999", "2025-01-01T00:00"], ["1970-01-01T00:00", "2024-06-15T12:34:56.789"]];
for (const [a, b] of dtPairs) {
  for (const lu of ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"]) {
    add(`${PDT}.from(${q(a)}).until(${q(b)},{largestUnit:${q(lu)}}).toString()`, `${PDT}.from(${q(a)}).since(${q(b)},{largestUnit:${q(lu)}}).toString()`);
  }
  for (const su of ["days", "hours", "minutes", "seconds", "milliseconds"]) {
    for (const mode of ["ceil", "floor", "expand", "trunc", "halfExpand", "halfEven"]) {
      add(`${PDT}.from(${q(a)}).until(${q(b)},{largestUnit:"days",smallestUnit:${q(su)},roundingMode:${q(mode)}}).toString()`);
    }
  }
  add(`${PDT}.from(${q(a)}).until(${q(b)},{smallestUnit:"minutes",roundingIncrement:15}).toString()`, `${PDT}.from(${q(a)}).since(${q(b)},{smallestUnit:"hours",roundingIncrement:6,roundingMode:"ceil"}).toString()`);
}

// ---- 7. ZonedDateTime round, until/since, startOfDay, hoursInDay, offset em transições.
const zdts = ["2024-03-10T01:59:59.999-05:00[America/New_York]", "2024-11-03T01:30-04:00[America/New_York]", "2024-11-03T01:30-05:00[America/New_York]", "2024-03-31T00:30+00:00[Europe/London]",
  "2024-06-15T12:34:56.789+09:00[Asia/Tokyo]", "2024-10-06T23:30-03:00[America/Sao_Paulo]", "2024-01-01T00:00+05:30[Asia/Kolkata]", "2024-03-30T23:30-04:00[America/Santiago]"];
for (const z of zdts) {
  for (const unit of ["day", "hour", "minute", "second", "millisecond"]) {
    for (const mode of ["ceil", "floor", "expand", "trunc", "halfExpand", "halfEven"]) add(`${ZDT}.from(${q(z)}).round({smallestUnit:${q(unit)},roundingMode:${q(mode)}}).toString()`);
  }
  add(`${ZDT}.from(${q(z)}).startOfDay().toString()`, `${ZDT}.from(${q(z)}).hoursInDay`, `${ZDT}.from(${q(z)}).offset`, `${ZDT}.from(${q(z)}).offsetNanoseconds`, `${ZDT}.from(${q(z)}).epochNanoseconds`,
    `${ZDT}.from(${q(z)}).add({days:1}).toString()`, `${ZDT}.from(${q(z)}).add({hours:24}).toString()`, `${ZDT}.from(${q(z)}).subtract({days:1}).toString()`, `${ZDT}.from(${q(z)}).add({months:1}).toString()`,
    `${ZDT}.from(${q(z)}).with({hour:2,minute:30}).toString()`, `${ZDT}.from(${q(z)}).with({hour:2,minute:30},{disambiguation:"earlier"}).toString()`, `${ZDT}.from(${q(z)}).with({hour:2,minute:30},{disambiguation:"later"}).toString()`,
    `${ZDT}.from(${q(z)}).with({hour:2,minute:30},{disambiguation:"reject"}).toString()`, `${ZDT}.from(${q(z)}).withPlainTime("02:30").toString()`, `${ZDT}.from(${q(z)}).withTimeZone("UTC").toString()`,
    `${ZDT}.from(${q(z)}).toPlainDate().toString()`, `${ZDT}.from(${q(z)}).toPlainTime().toString()`, `${ZDT}.from(${q(z)}).toInstant().toString()`);
  for (const o of ["{offset:'never'}", "{offset:'auto'}", "{timeZoneName:'never'}", "{timeZoneName:'critical'}", "{calendarName:'always'}", "{calendarName:'critical'}", "{calendarName:'never'}",
    "{fractionalSecondDigits:2}", "{smallestUnit:'minute'}", "{smallestUnit:'hour'}", "{offset:'x'}", "{timeZoneName:'x'}", "{calendarName:'x'}"]) add(`${ZDT}.from(${q(z)}).toString(${o})`);
}
for (let i = 0; i < zdts.length - 1; i++) {
  const a = zdts[i], b = zdts[i + 1];
  for (const lu of ["years", "months", "weeks", "days", "hours", "minutes", "seconds"]) add(`${ZDT}.from(${q(a)}).until(${q(b)},{largestUnit:${q(lu)}}).toString()`);
  add(`${ZDT}.from(${q(a)}).until(${q(b)}).toString()`, `${ZDT}.from(${q(a)}).since(${q(b)},{largestUnit:"hours",smallestUnit:"minutes",roundingMode:"ceil"}).toString()`, `${ZDT}.compare(${q(a)},${q(b)})`, `${ZDT}.from(${q(a)}).equals(${q(b)})`);
}
add(`${ZDT}.from("2024-03-10T12:00[America/New_York]").until("2024-03-10T12:00[Europe/London]")`, `${ZDT}.from("2024-03-10T12:00[America/New_York]").until("2024-03-12T12:00[Europe/London]",{largestUnit:"days"})`,
  `${ZDT}.from("2024-03-10T12:00[America/New_York]").until("2024-03-12T12:00[Europe/London]",{largestUnit:"hours"}).toString()`);

// ---- 8. with, add, subtract com overflow.
for (const [y, m, d] of [[2024, 2, 30], [2023, 2, 29], [2024, 13, 1], [2024, 0, 1], [2024, 4, 31], [2024, 1, 0], [2024, 12, 32], [-271821, 4, 19], [275760, 9, 13], [275760, 9, 14]]) {
  for (const ov of ["constrain", "reject", "foo"]) add(`${PD}.from({year:${y},month:${m},day:${d}},{overflow:${q(ov)}}).toString()`);
  add(`${PD}.from({year:${y},month:${m},day:${d}}).toString()`);
}
for (const [h, mi, s] of [[24, 0, 0], [23, 60, 0], [23, 59, 60], [25, 61, 61], [-1, 0, 0], [12, 30, 59]]) {
  for (const ov of ["constrain", "reject"]) add(`${PT}.from({hour:${h},minute:${mi},second:${s}},{overflow:${q(ov)}}).toString()`, `${PDT}.from({year:2024,month:2,day:30,hour:${h},minute:${mi},second:${s}},{overflow:${q(ov)}}).toString()`);
}
for (const d of ["2024-01-31", "2024-03-31", "2024-02-29", "2023-12-31", "2024-05-31"]) {
  for (const dur of ["{months:1}", "{months:-1}", "{months:12}", "{years:1}", "{years:-4}", "{years:1,months:1,days:1}", "{months:1,days:-1}", "{weeks:1,days:7}", "{months:1.5}", "{months:2,days:-62}"]) {
    for (const ov of ["constrain", "reject"]) add(`${PD}.from(${q(d)}).add(${dur},{overflow:${q(ov)}}).toString()`, `${PD}.from(${q(d)}).subtract(${dur},{overflow:${q(ov)}}).toString()`);
  }
  for (const w of ["{day:31}", "{month:2}", "{month:2,day:31}", "{year:2023}", "{monthCode:'M02'}", "{monthCode:'M13'}", "{monthCode:'M02L'}", "{month:2,monthCode:'M03'}", "{day:0}", "{day:-1}", "{foo:1}", "{}", "{era:'ce'}"]) {
    add(`${PD}.from(${q(d)}).with(${w}).toString()`, `${PD}.from(${q(d)}).with(${w},{overflow:"reject"}).toString()`);
  }
}
for (const ym of ["2024-01", "2024-12", "-000001-03", "2024-02"]) {
  for (const dur of ["{months:1}", "{months:-1}", "{years:1}", "{months:13}", "{days:1}", "{days:30}", "{weeks:1}", "{months:1,days:5}", "{hours:48}", "{months:-12}"]) {
    add(`${PYM}.from(${q(ym)}).add(${dur}).toString()`, `${PYM}.from(${q(ym)}).subtract(${dur}).toString()`);
  }
  for (const o of ["{largestUnit:'years'}", "{largestUnit:'months'}", "{largestUnit:'days'}", "{smallestUnit:'years'}", "{smallestUnit:'months',roundingMode:'ceil'}", "{largestUnit:'years',roundingIncrement:2,smallestUnit:'months'}"]) {
    add(`${PYM}.from(${q(ym)}).until("2026-07",${o}).toString()`, `${PYM}.from(${q(ym)}).since("2020-03",${o}).toString()`);
  }
  add(`${PYM}.from(${q(ym)}).toPlainDate({day:31}).toString()`, `${PYM}.from(${q(ym)}).toPlainDate({day:15}).toString()`, `${PYM}.from(${q(ym)}).daysInMonth`, `${PYM}.from(${q(ym)}).daysInYear`, `${PYM}.from(${q(ym)}).monthsInYear`);
}
for (const md of ["--02-29", "02-29", "--12-31", "--04-31", "--13-01", "--00-10", "0229"]) {
  add(`${PMD}.from(${q(md)}).toString()`, `${PMD}.from(${q(md)}).monthCode`, `${PMD}.from(${q(md)}).day`, `${PMD}.from(${q(md)}).toPlainDate({year:2023}).toString()`, `${PMD}.from(${q(md)}).toPlainDate({year:2024}).toString()`);
}
for (const [m, d] of [[2, 30], [13, 1], [4, 31], [0, 5], [12, 32], [2, 29]]) for (const ov of ["constrain", "reject"]) add(`${PMD}.from({month:${m},day:${d}},{overflow:${q(ov)}}).toString()`, `${PMD}.from({monthCode:"M0${m % 10}",day:${d}},{overflow:${q(ov)}}).toString()`);

// ---- 9. toString (calendarName, fractionalSecondDigits, smallestUnit) de PlainTime/Date/DateTime/YearMonth/MonthDay.
for (const t of ["12:34:56.789123456", "00:00", "23:59:59.9", "12:34:56"]) {
  for (const o of ["{}", "{fractionalSecondDigits:0}", "{fractionalSecondDigits:1}", "{fractionalSecondDigits:4}", "{fractionalSecondDigits:9}", "{fractionalSecondDigits:'auto'}", "{smallestUnit:'minute'}", "{smallestUnit:'second'}",
    "{smallestUnit:'millisecond'}", "{smallestUnit:'microsecond'}", "{smallestUnit:'nanosecond'}", "{smallestUnit:'hour'}", "{smallestUnit:'minute',roundingMode:'ceil'}", "{fractionalSecondDigits:2,roundingMode:'floor'}",
    "{fractionalSecondDigits:2,roundingMode:'halfEven'}", "{fractionalSecondDigits:10}", "{fractionalSecondDigits:1.5}", "{fractionalSecondDigits:'2'}", "{smallestUnit:'day'}", "{smallestUnit:'minutes'}"]) {
    add(`${PT}.from(${q(t)}).toString(${o})`, `${PDT}.from("2024-02-29T"+${q(t)}).toString(${o})`);
  }
}
for (const cal of ["auto", "always", "never", "critical", "bogus"]) {
  for (const c of ["", "[u-ca=gregory]", "[u-ca=iso8601]", "[u-ca=japanese]", "[u-ca=hebrew]"]) {
    add(`${PD}.from(${q("2024-02-29" + c)}).toString({calendarName:${q(cal)}})`, `${PDT}.from(${q("2024-02-29T10:00" + c)}).toString({calendarName:${q(cal)}})`);
  }
  add(`${PYM}.from("2024-02").toString({calendarName:${q(cal)}})`, `${PMD}.from("--02-29").toString({calendarName:${q(cal)}})`, `${PYM}.from("2024-02-01[u-ca=gregory]").toString({calendarName:${q(cal)}})`, `${PMD}.from("2024-02-29[u-ca=gregory]").toString({calendarName:${q(cal)}})`);
}

// ---- 10. compare e equals.
const cmpDates = ["2024-01-01", "2024-01-02", "2023-12-31", "-000001-01-01", "+010000-01-01", "2024-01-01[u-ca=gregory]", "2024-01-01T00:00"];
for (const a of cmpDates) for (const b of cmpDates) add(`${PD}.compare(${q(a)},${q(b)})`, `${PD}.from(${q(a)}).equals(${q(b)})`);
const cmpTimes = ["00:00", "00:00:00.000000001", "23:59:59.999999999", "12:00", "12:00:00.000"];
for (const a of cmpTimes) for (const b of cmpTimes) add(`${PT}.compare(${q(a)},${q(b)})`, `${PT}.from(${q(a)}).equals(${q(b)})`);
const cmpDt = ["2024-01-01T00:00", "2024-01-01T00:00:00.000000001", "2023-12-31T23:59:59.999999999", "2024-01-01"];
for (const a of cmpDt) for (const b of cmpDt) add(`${PDT}.compare(${q(a)},${q(b)})`, `${PDT}.from(${q(a)}).equals(${q(b)})`);
const cmpInst = ["1970-01-01T00:00:00Z", "1970-01-01T00:00:00.000000001Z", "1969-12-31T23:59:59.999999999Z", "1970-01-01T01:00:00+01:00"];
for (const a of cmpInst) for (const b of cmpInst) add(`${INS}.compare(${q(a)},${q(b)})`, `${INS}.from(${q(a)}).equals(${q(b)})`);
for (const [a, b] of [["P1D", "PT24H"], ["P1M", "P30D"], ["PT60M", "PT1H"], ["P1W", "P7D"], ["P1Y", "P12M"], ["P1Y", "P365D"], ["-P1D", "P1D"], ["PT0S", "P0D"]]) {
  add(`${DUR}.compare(${q(a)},${q(b)})`, `${DUR}.compare(${q(a)},${q(b)},{relativeTo:"2024-01-01"})`, `${DUR}.compare(${q(a)},${q(b)},{relativeTo:"2023-01-01"})`, `${DUR}.compare(${q(a)},${q(b)},{relativeTo:"2024-02-01[u-ca=gregory]"})`,
    `${DUR}.compare(${q(a)},${q(b)},{relativeTo:Temporal.ZonedDateTime.from("2024-03-09T12:00[America/New_York]")})`);
}
add(`${PYM}.compare("2024-01","2024-02")`, `${PYM}.compare("2024-02","2024-02")`, `${PYM}.compare("2024-02","2024-01")`, `${PYM}.from("2024-02").equals("2024-02")`, `${PYM}.from("2024-02").equals("2024-03")`,
  `${PMD}.from("--02-29").equals("--02-29")`, `${PMD}.from("--02-29").equals("--03-01")`, `${PD}.compare()`, `${PD}.compare("2024-01-01")`, `${PT}.compare(1,2)`, `${INS}.compare(1n,2n)`);

// ---- 11. from com strings ISO extensas.
const dateStrs = ["2024-02-29", "20240229", "2024-W09-4", "+002024-02-29", "-002024-02-29", "−002024-02-29", "-000000-01-01", "+000000-01-01", "2024-02-30", "2023-02-29", "2024-13-01", "2024-00-10", "2024-02-00", "2024", "2024-02", "24-02-29",
  "2024-02-29T", "2024-02-29T10", "2024-02-29T10:00", "2024-02-29T10:00:00", "2024-02-29T10:00:60", "2024-02-29t10:00", "2024-02-29 10:00", "2024-02-29T24:00", "2024-02-29T1000", "2024-02-29T10:0", "2024-02-29T10:00:00.", "2024-02-29T10:00:00.1234567890",
  "2024-02-29T10:00:00,5", "2024-02-29T10:00Z", "2024-02-29T10:00+01:00", "2024-02-29T10:00[UTC]", "2024-02-29T10:00Z[UTC]", "2024-02-29[UTC]", "2024-02-29[u-ca=gregory]", "2024-02-29[u-ca=GREGORY]", "2024-02-29[u-ca=nope]", "2024-02-29[u-ca=iso8601][u-ca=gregory]",
  "2024-02-29[!u-ca=gregory]", "2024-02-29[foo=bar]", "2024-02-29[!foo=bar]", "2024-02-29[_foo=bar]", "2024-02-29[u-ca=gregory][foo=bar]", " 2024-02-29", "2024-02-29 ", "2024-02-29\n", "+275760-09-13", "+275760-09-14", "-271821-04-19", "-271821-04-18", "275760-09-13",
  "2024-02-29T10:00:00+01:00[Europe/Paris]", "2024-02-29T10:00:00+01:00[+01:00]", "2024-02-29T10:00:00+0100", "2024-02-29T10:00:00+01", "2024-02-29T10:00:00+01:00:30", "2024-02-29T10:00:00+01:00:30.5", "2024-W09", "2024W094", "2024-W53-1", "2024-W00-1", "2024-366", "2024-060"];
for (const s of dateStrs) {
  add(`${PD}.from(${q(s)}).toString()`, `${PDT}.from(${q(s)}).toString()`, `${PT}.from(${q(s)}).toString()`, `${PYM}.from(${q(s)}).toString()`, `${PMD}.from(${q(s)}).toString()`, `${INS}.from(${q(s)}).toString()`);
  add(`${ZDT}.from(${q(s)}).toString()`);
}
const timeStrs = ["10", "10:30", "1030", "10:30:15", "103015", "10:30:15.123456789", "10:30:15.1234567891", "T10:30", "t10:30", "T1030", "10:30Z", "10:30+01:00", "10:30[UTC]", "10:30[u-ca=gregory]", "24:00", "23:60", "23:59:60", "23:59:61", "00:00:00.000000000", "1:30", "10:3", "10-30", "10.30", "T", "", " ", "10:30:15,5", "T10:30:15.5Z", "2024-02-29T10:30", "0010:30", "--10:30", "+10:30", "10:30:15.", "10:30:15.1.2", "１０:３０"];
for (const s of timeStrs) add(`${PT}.from(${q(s)}).toString()`, `${PDT}.from(${q(s)}).toString()`);
const monthDayStrs = ["--02-29", "02-29", "0229", "--0229", "--02-30", "--04-31", "--13-01", "2024-02-29", "2023-02-29", "2024-02-29[u-ca=gregory]", "--02-29[u-ca=gregory]", "--02-29[u-ca=iso8601]", "02-29[u-ca=gregory]", "--M02-29", "--02", "-02-29", "1111-02-29"];
for (const s of monthDayStrs) add(`${PMD}.from(${q(s)}).toString()`, `${PMD}.from(${q(s)}).toString({calendarName:"always"})`);
const ymStrs = ["2024-02", "202402", "2024-02-29", "2024-02[u-ca=gregory]", "2024-02-01[u-ca=gregory]", "2024-02-02[u-ca=gregory]", "+002024-02", "-000001-12", "2024-2", "2024-13", "2024-00", "+275760-09", "+275760-10", "-271821-04", "-271821-03", "2024-02-29T10:00", "2024-02[UTC]"];
for (const s of ymStrs) add(`${PYM}.from(${q(s)}).toString()`, `${PYM}.from(${q(s)}).toString({calendarName:"always"})`);
const instStrs = ["2024-02-29T10:00Z", "2024-02-29T10:00:00.123456789Z", "2024-02-29T10:00:00.1234567891Z", "2024-02-29T10:00+01:00", "2024-02-29T10:00-00:00", "2024-02-29T10:00+24:00", "2024-02-29T10:00+23:59", "2024-02-29T10:00+23:59:59.999999999", "2024-02-29T10:00+23:59:60",
  "2024-02-29T10:00[UTC]", "2024-02-29T10:00Z[America/New_York]", "2024-02-29T10:00+01:00[Europe/Paris]", "2024-02-29T10:00+02:00[Europe/Paris]", "2024-02-29T10:00z", "2024-02-29T10Z", "2024-02-29Z", "2024-02-29", "+275760-09-13T00:00:00Z", "+275760-09-13T00:00:00.000000001Z", "-271821-04-20T00:00:00Z",
  "-271821-04-19T23:59:59.999999999Z", "-271821-04-20T00:00:00+00:01", "1970-01-01T00:00:00+00:00:01", "1970-01-01T00:00:00+00:00:01.5", "1970-01-01T00:00:00+01:00:00"];
for (const s of instStrs) add(`${INS}.from(${q(s)}).toString()`, `${INS}.from(${q(s)}).epochMilliseconds`, `${INS}.from(${q(s)}).epochNanoseconds`);
const zdtStrs = ["2024-02-29T10:00[UTC]", "2024-02-29T10:00Z[UTC]", "2024-02-29T10:00+00:00[UTC]", "2024-02-29T10:00+01:00[UTC]", "2024-02-29T10:00[Europe/Paris]", "2024-02-29T10:00+01:00[Europe/Paris]", "2024-02-29T10:00+02:00[Europe/Paris]", "2024-02-29T10:00[+01:00]", "2024-02-29T10:00+01:00[+01:00]",
  "2024-02-29T10:00+01:00[+02:00]", "2024-02-29T10:00Z[+01:00]", "2024-02-29T10:00[-00:00]", "2024-02-29T10:00[+0100]", "2024-02-29T10:00[+01]", "2024-02-29T10:00[Etc/GMT+3]", "2024-02-29T10:00[Etc/GMT-14]", "2024-02-29T10:00[utc]", "2024-02-29T10:00[europe/paris]", "2024-02-29T10:00[EUROPE/PARIS]", "2024-02-29T10:00[Not/AZone]",
  "2024-02-29T10:00[!UTC]", "2024-02-29T10:00[UTC][u-ca=gregory]", "2024-02-29T10:00[u-ca=gregory][UTC]", "2024-02-29T10:00", "2024-02-29T10:00Z", "2024-02-29[UTC]", "2024-02-29T10:00[Asia/Calcutta]", "2024-02-29T10:00[Asia/Kolkata]", "2024-02-29T10:00[US/Eastern]", "2024-02-29T10:00[America/Buenos_Aires]",
  "2024-03-10T02:30[America/New_York]", "2024-11-03T01:30[America/New_York]", "2024-11-03T01:30-04:00[America/New_York]", "2024-11-03T01:30-05:00[America/New_York]", "2024-11-03T01:30-06:00[America/New_York]", "2024-03-10T02:30-05:00[America/New_York]", "2024-03-10T02:30-04:00[America/New_York]",
  "1900-01-01T00:00[Europe/Amsterdam]", "1900-01-01T00:00+00:19:32.13[Europe/Amsterdam]", "1900-01-01T00:00+00:19:32[Europe/Amsterdam]", "+275760-09-13T00:00[UTC]", "+275760-09-13T00:00:00.000000001[UTC]", "-271821-04-20T00:00[UTC]", "-271821-04-19T23:59[UTC]"];
for (const s of zdtStrs) {
  add(`${ZDT}.from(${q(s)}).toString()`, `${ZDT}.from(${q(s)},{offset:"ignore"}).toString()`, `${ZDT}.from(${q(s)},{offset:"use"}).toString()`, `${ZDT}.from(${q(s)},{offset:"prefer"}).toString()`, `${ZDT}.from(${q(s)},{offset:"reject"}).toString()`,
    `${ZDT}.from(${q(s)},{disambiguation:"earlier"}).toString()`, `${ZDT}.from(${q(s)},{disambiguation:"later"}).toString()`, `${ZDT}.from(${q(s)},{disambiguation:"reject"}).toString()`, `${ZDT}.from(${q(s)},{disambiguation:"compatible"}).epochNanoseconds`);
}
const offsets = ["+00:00", "-00:00", "+01:00", "-01:00", "+14:00", "-14:00", "+23:59", "-23:59", "+24:00", "+05:30", "+05:45", "+0530", "+05", "+5", "+05:30:15", "+05:30:15.5", "+05:30:15.123456789", "+05:30:15.1234567891", "+0530:15", "−05:00", "Z", "z", "+", "-", "", "+25:00"];
for (const o of offsets) {
  add(`${ZDT}.from({year:2024,month:6,day:1,timeZone:${q(o)}}).toString()`, `${ZDT}.from({year:2024,month:6,day:1,hour:12,timeZone:${q(o)}}).offset`, `${ZDT}.from("2024-06-01T12:00["+${q(o)}+"]").toString()`,
    `${INS}.from("2024-06-01T12:00:00Z").toZonedDateTimeISO(${q(o)}).toString()`, `${INS}.from("2024-06-01T12:00:00Z").toString({timeZone:${q(o)}})`, `${ZDT}.from("2024-06-01T12:00:00+05:30[Asia/Kolkata]").withTimeZone(${q(o)}).toString()`,
    `${INS}.from("2024-06-01T12:00:00${o}")` .replace(/\n/g, ""), `${ZDT}.from("2024-06-01T12:00:00Z[UTC]").offsetNanoseconds`);
}
for (const tz of ["UTC", "utc", "Utc", "Etc/UTC", "Etc/GMT", "GMT", "Etc/GMT+12", "Etc/GMT-14", "Etc/GMT+13", "Etc/GMT-15", "Europe/Kiev", "Europe/Kyiv", "Asia/Kolkata", "Asia/Calcutta", "America/Argentina/Buenos_Aires", "America/Buenos_Aires", "US/Pacific", "America/Los_Angeles",
  "Asia/Kathmandu", "Asia/Katmandu", "Africa/Cairo", "Pacific/Kiritimati", "Pacific/Apia", "Antarctica/Troll", "Australia/Lord_Howe", "Asia/Pyongyang", "Foo/Bar", "", "America/", "UTC+1", "EST", "EST5EDT", "CET", "MST", "PST8PDT", "Z"]) {
  add(`${ZDT}.from({year:2024,month:7,day:1,hour:12,timeZone:${q(tz)}}).toString()`, `${ZDT}.from({year:2024,month:7,day:1,hour:12,timeZone:${q(tz)}}).timeZoneId`,
    `${ZDT}.from({year:2024,month:1,day:1,hour:12,timeZone:${q(tz)}}).offset`, `${INS}.from("2000-01-01T00:00:00Z").toZonedDateTimeISO(${q(tz)}).toString()`, `${ZDT}.from({year:1850,month:1,day:1,timeZone:${q(tz)}}).offset`);
}

// ---- 12. Calendários não ISO.
const cals = ["gregory", "japanese", "buddhist", "chinese", "coptic", "dangi", "ethioaa", "ethiopic", "hebrew", "indian", "islamic", "islamic-civil", "islamic-rgsa", "islamic-tbla", "islamic-umalqura", "persian", "roc", "iso8601", "GREGORY", "Gregory", "bogus", "islamicc", "ethiopic-amete-alem"];
for (const c of cals) {
  add(`${PD}.from({year:2024,month:3,day:15,calendar:${q(c)}}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").year`, `${PD}.from("2024-03-15[u-ca=${c}]").monthCode`, `${PD}.from("2024-03-15[u-ca=${c}]").month`,
    `${PD}.from("2024-03-15[u-ca=${c}]").day`, `${PD}.from("2024-03-15[u-ca=${c}]").era`, `${PD}.from("2024-03-15[u-ca=${c}]").eraYear`, `${PD}.from("2024-03-15[u-ca=${c}]").daysInMonth`, `${PD}.from("2024-03-15[u-ca=${c}]").daysInYear`,
    `${PD}.from("2024-03-15[u-ca=${c}]").monthsInYear`, `${PD}.from("2024-03-15[u-ca=${c}]").inLeapYear`, `${PD}.from("2024-03-15[u-ca=${c}]").add({months:1}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").add({years:1}).toString()`,
    `${PD}.from("2024-03-15[u-ca=${c}]").subtract({months:13}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").until("2025-03-15[u-ca=${c}]",{largestUnit:"years"}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").until("2024-12-31[u-ca=${c}]",{largestUnit:"months"}).toString()`,
    `${PD}.from("2024-03-15[u-ca=${c}]").since("2020-01-31[u-ca=${c}]",{largestUnit:"years"}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").with({day:1}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").with({monthCode:"M01"}).toString()`,
    `${PD}.from("2024-03-15[u-ca=${c}]").withCalendar("iso8601").toString()`, `${PD}.from("2024-03-15").withCalendar(${q(c)}).toString()`, `${PD}.from("2024-03-15[u-ca=${c}]").weekOfYear`, `${PD}.from("2024-03-15[u-ca=${c}]").dayOfWeek`, `${PD}.from("2024-03-15[u-ca=${c}]").dayOfYear`,
    `${PYM}.from("2024-03-01[u-ca=${c}]").toString()`, `${PYM}.from("2024-03-01[u-ca=${c}]").add({months:6}).toString()`, `${PMD}.from("2024-03-15[u-ca=${c}]").toString()`, `${PMD}.from("2024-03-15[u-ca=${c}]").monthCode`,
    `${PDT}.from("2024-03-15T10:00[u-ca=${c}]").toString()`, `${PDT}.from("2024-03-15T10:00[u-ca=${c}]").add({months:1,hours:30}).toString()`, `${ZDT}.from("2024-03-15T10:00[UTC][u-ca=${c}]").toString()`, `${ZDT}.from("2024-03-15T10:00[UTC][u-ca=${c}]").add({years:1}).toString()`);
}
for (const [c, y, m, d] of [["hebrew", 5784, 13, 1], ["hebrew", 5784, 6, 30], ["hebrew", 5785, 13, 1], ["islamic-civil", 1445, 12, 30], ["islamic-civil", 1446, 12, 30], ["persian", 1403, 12, 30], ["persian", 1404, 12, 30], ["coptic", 1740, 13, 6], ["coptic", 1741, 13, 6],
  ["ethiopic", 2016, 13, 6], ["indian", 1946, 12, 31], ["indian", 1947, 12, 31], ["chinese", 2024, 1, 30], ["dangi", 2024, 13, 1], ["roc", 113, 2, 29], ["buddhist", 2567, 2, 29], ["japanese", 6, 2, 29], ["gregory", 2024, 2, 30]]) {
  for (const ov of ["constrain", "reject"]) add(`${PD}.from({year:${y},month:${m},day:${d},calendar:${q(c)}},{overflow:${q(ov)}}).toString()`);
}
add(`${PD}.from({era:"showa",eraYear:64,monthCode:"M01",day:7,calendar:"japanese"}).toString()`, `${PD}.from({era:"heisei",eraYear:1,monthCode:"M01",day:8,calendar:"japanese"}).toString()`, `${PD}.from({era:"reiwa",eraYear:1,monthCode:"M05",day:1,calendar:"japanese"}).toString()`,
  `${PD}.from({era:"reiwa",eraYear:1,monthCode:"M04",day:30,calendar:"japanese"}).toString()`, `${PD}.from({era:"meiji",eraYear:6,monthCode:"M01",day:1,calendar:"japanese"}).toString()`, `${PD}.from({era:"bce",eraYear:1,monthCode:"M01",day:1,calendar:"gregory"}).toString()`,
  `${PD}.from({era:"ce",eraYear:1,monthCode:"M01",day:1,calendar:"gregory"}).toString()`, `${PD}.from({era:"ce",eraYear:0,monthCode:"M01",day:1,calendar:"gregory"}).toString()`, `${PD}.from({era:"foo",eraYear:1,monthCode:"M01",day:1,calendar:"gregory"}).toString()`,
  `${PD}.from({era:"ce",monthCode:"M01",day:1,calendar:"gregory"}).toString()`, `${PD}.from({eraYear:1,monthCode:"M01",day:1,calendar:"gregory"}).toString()`, `${PD}.from({year:2024,era:"ce",eraYear:2023,monthCode:"M01",day:1,calendar:"gregory"}).toString()`,
  `${PD}.from({era:"minguo",eraYear:1,monthCode:"M01",day:1,calendar:"roc"}).toString()`, `${PD}.from({era:"broc",eraYear:1,monthCode:"M01",day:1,calendar:"roc"}).toString()`, `${PD}.from({era:"be",eraYear:2567,monthCode:"M01",day:1,calendar:"buddhist"}).toString()`,
  `${PD}.from({era:"am",eraYear:5784,monthCode:"M01",day:1,calendar:"hebrew"}).toString()`, `${PD}.from({era:"ah",eraYear:1445,monthCode:"M01",day:1,calendar:"islamic-civil"}).toString()`, `${PD}.from({era:"saka",eraYear:1946,monthCode:"M01",day:1,calendar:"indian"}).toString()`,
  `${PD}.from({year:5784,monthCode:"M05L",day:1,calendar:"hebrew"}).toString()`, `${PD}.from({year:5783,monthCode:"M05L",day:1,calendar:"hebrew"}).toString()`, `${PD}.from({year:5783,monthCode:"M05L",day:1,calendar:"hebrew"},{overflow:"reject"}).toString()`,
  `${PD}.from({year:2023,monthCode:"M06L",day:1,calendar:"chinese"}).toString()`, `${PD}.from({year:2024,monthCode:"M06L",day:1,calendar:"chinese"}).toString()`, `${PD}.from({year:2024,monthCode:"M13",day:1,calendar:"hebrew"}).toString()`,
  `${PD}.from({year:2024,month:13,day:1,calendar:"gregory"}).toString()`, `${PD}.from({year:2024,monthCode:"M00",day:1}).toString()`, `${PD}.from({year:2024,monthCode:"m01",day:1}).toString()`, `${PD}.from({year:2024,monthCode:"M1",day:1}).toString()`, `${PD}.from({year:2024,monthCode:1,day:1}).toString()`,
  `${PD}.from("2024-03-15").withCalendar("hebrew").add({months:6}).toString()`, `${PD}.from("2024-03-15").withCalendar("islamic-civil").add({days:355}).toString()`, `${PD}.from("2024-03-15[u-ca=hebrew]").equals("2024-03-15[u-ca=gregory]")`,
  `${PD}.compare("2024-03-15[u-ca=hebrew]","2024-03-15[u-ca=gregory]")`, `${PD}.from("2024-03-15[u-ca=hebrew]").until("2024-03-15[u-ca=gregory]")`, `${PD}.from("2024-03-15[u-ca=hebrew]").until("2025-03-15[u-ca=hebrew]",{largestUnit:"months"}).toString()`);

// ---- 13. Erros: mensagens exatas de RangeError e TypeError.
const typeNames = [["PlainDate", PD], ["PlainTime", PT], ["PlainDateTime", PDT], ["PlainYearMonth", PYM], ["PlainMonthDay", PMD], ["ZonedDateTime", ZDT], ["Instant", INS], ["Duration", DUR]];
for (const [n, T] of typeNames) {
  add(`${T}.from()`, `${T}.from(undefined)`, `${T}.from(null)`, `${T}.from(1)`, `${T}.from(true)`, `${T}.from(Symbol())`, `${T}.from(1n)`, `${T}.from([])`, `${T}.from({})`, `${T}.from("")`, `${T}.from("x")`, `${T}.from({foo:1})`, `${T}.from(()=>1)`, `${T}()`, `${T}.from("2024-01-01",1)`, `${T}.from("2024-01-01",null)`,
    `${T}.from("2024-01-01",{overflow:"x"})`, `${T}.prototype.toString.call({})`, `${T}.prototype.toString.call(1)`, `Object.prototype.toString.call(${T}.prototype)`, `${T}.name`, `${T}.length`, `typeof ${T}.from`, `${T}.prototype[Symbol.toStringTag]`, `${T}.prototype.valueOf.call(${T}.prototype)`);
  add(`new ${T}()`, `Reflect.ownKeys(${T}).join()`, `Reflect.ownKeys(${T}.prototype).join()`);
}
add(`new ${PT}(24)`, `new ${PT}(0,60)`, `new ${PT}(0,0,60)`, `new ${PT}(0,0,0,1000)`, `new ${PT}(0,0,0,0,1000)`, `new ${PT}(0,0,0,0,0,1000)`, `new ${PT}(-1)`, `new ${PT}(1.5)`, `new ${PT}("12").toString()`, `new ${PT}(Infinity)`, `new ${PT}(NaN)`, `new ${PT}(undefined,undefined,5).toString()`,
  `new ${PD}(2024,2,30)`, `new ${PD}(2024,13,1)`, `new ${PD}(2024,1)`, `new ${PD}(2024,1,1,"gregory").toString()`, `new ${PD}(2024,1,1,"bogus")`, `new ${PD}(2024,1,1,1)`, `new ${PD}(275760,9,14)`, `new ${PD}(275760,9,13).toString()`, `new ${PD}(-271821,4,18)`, `new ${PD}(-271821,4,19).toString()`, `new ${PD}(Infinity,1,1)`,
  `new ${PDT}(2024,2,29,24)`, `new ${PDT}(2024,2,29,0,0,0,0,0,1000)`, `new ${PDT}(275760,9,13,23,59,59,999,999,999).toString()`, `new ${PDT}(275760,9,14)`, `new ${PYM}(2024,13)`, `new ${PYM}(2024,2,"gregory",30)`, `new ${PYM}(2024,2,"iso8601",1).toString()`, `new ${PYM}(275760,9).toString()`, `new ${PYM}(275760,10)`,
  `new ${PMD}(2,29).toString()`, `new ${PMD}(2,30)`, `new ${PMD}(13,1)`, `new ${PMD}(2,29,"iso8601",1972).toString()`, `new ${PMD}(2,29,"iso8601",2023)`, `new ${INS}(0n).toString()`, `new ${INS}(0)`, `new ${INS}("0")`, `new ${INS}(8640000000000000000001n)`, `new ${INS}(8640000000000000000000n).toString()`, `new ${INS}(-8640000000000000000001n)`,
  `new ${ZDT}(0n,"UTC").toString()`, `new ${ZDT}(0n)`, `new ${ZDT}(0n,"bogus")`, `new ${ZDT}(0n,"UTC","gregory").toString()`, `new ${ZDT}(0,"UTC")`, `new ${ZDT}(0n,"+05:30").toString()`, `new ${ZDT}(8640000000000000000001n,"UTC")`, `new ${DUR}(0.5)`, `new ${DUR}(1,-1)`, `new ${DUR}(2**32)`, `new ${DUR}("1").toString()`, `new ${DUR}(-0).sign`);
add(`${INS}.fromEpochMilliseconds(0).toString()`, `${INS}.fromEpochMilliseconds(8.64e15).toString()`, `${INS}.fromEpochMilliseconds(8.64e15+1)`, `${INS}.fromEpochMilliseconds(0.5)`, `${INS}.fromEpochMilliseconds(NaN)`, `${INS}.fromEpochMilliseconds("1000").toString()`, `${INS}.fromEpochMilliseconds(1n)`,
  `${INS}.fromEpochNanoseconds(0n).toString()`, `${INS}.fromEpochNanoseconds(0)`, `${INS}.fromEpochNanoseconds("1")`, `${INS}.fromEpochNanoseconds(1n).epochMicroseconds`, `${INS}.fromEpochNanoseconds(-1n).epochMilliseconds`, `${INS}.fromEpochNanoseconds(-1n).epochMicroseconds`,
  `${INS}.fromEpochNanoseconds(-1n).epochNanoseconds`, `${INS}.fromEpochNanoseconds(1500000n).epochMilliseconds`, `${INS}.fromEpochNanoseconds(-1500000n).epochMilliseconds`, `${INS}.from("1970-01-01T00:00:00Z").add({hours:1}).toString()`, `${INS}.from("1970-01-01T00:00:00Z").add({days:1})`,
  `${INS}.from("1970-01-01T00:00:00Z").add({months:1})`, `${INS}.from("1970-01-01T00:00:00Z").add({weeks:1})`, `${INS}.from("1970-01-01T00:00:00Z").add({years:1})`, `${INS}.from("1970-01-01T00:00:00Z").subtract({nanoseconds:1}).toString()`, `${INS}.from("1970-01-01T00:00:00Z").add("PT1H").toString()`,
  `${INS}.from("1970-01-01T00:00:00Z").until("1971-01-01T00:00:00Z").toString()`, `${INS}.from("1970-01-01T00:00:00Z").until("1971-01-01T00:00:00Z",{largestUnit:"days"})`, `${INS}.from("1970-01-01T00:00:00Z").until("1971-01-01T00:00:00Z",{largestUnit:"hours"}).toString()`,
  `${INS}.from("1970-01-01T00:00:00Z").until("1970-01-01T00:00:00.123456789Z",{largestUnit:"milliseconds"}).toString()`, `${INS}.from("1970-01-01T00:00:00Z").until("1970-01-01T00:00:00.123456789Z",{smallestUnit:"microseconds",roundingMode:"ceil"}).toString()`,
  `${INS}.from("1970-01-01T00:00:00Z").since("1970-01-01T01:30:45Z",{largestUnit:"minutes"}).toString()`, `${INS}.from("1970-01-01T00:00:00Z").since("1970-01-01T01:30:45Z",{smallestUnit:"hours",roundingMode:"halfExpand"}).toString()`,
  `${INS}.from("1970-01-01T00:00:00Z").since("1970-01-01T01:30:45Z",{smallestUnit:"hours",roundingIncrement:2,roundingMode:"ceil"}).toString()`, `${INS}.from("1970-01-01T00:00:00Z").since("1970-01-01T01:30:45Z",{largestUnit:"weeks"})`);

// ---- 14. Execução: bun novo por programa, deduplicando contra os goldens de Temporal já existentes.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const existing = knownPrograms("temporal_round_bun.tsv", (file) => /^temporal.*_bun\.tsv$/.test(file) && file !== "temporal_round_bun.tsv");
const existingText = existing.join("\n\u0000\n");
const seen = new Set();
let kept = 0;
let dropped = 0;
let dup = 0;
for (const expr of exprs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  if (existingText.includes(expr)) { dup++; continue; }
  const source = `globalThis.R = (()=>{try{return String(${expr})}catch(e){return e.name+": "+e.message}})()`;
  const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26 });
  if (child.status !== 0) { dropped++; process.stderr.write("filho falhou: " + expr.slice(0, 120) + "\n"); continue; }
  const result = decodeResult(child.stdout);
  if (result === null) { dropped++; process.stderr.write("resultado ausente: " + expr.slice(0, 120) + "\n"); continue; }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) { dropped++; process.stderr.write("marca no resultado: " + expr.slice(0, 120) + "\n"); continue; }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
