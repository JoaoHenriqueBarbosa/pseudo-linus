// Gera tests/golden/temporal_math_bun.tsv: ~500 programas de aritmética de Temporal medidos no bun 1.4.2 que os demais
// temporal_*_bun.tsv não cobrem: Duration (add/subtract/round/total/with/negated/abs/compare com relativeTo,
// balanceamento, largestUnit/smallestUnit/roundingMode/roundingIncrement, strings ISO 8601, limites), PlainDate,
// PlainDateTime, PlainYearMonth e PlainMonthDay (add/subtract/until/since com overflow), calendários, ZonedDateTime com
// fusos fixos (gap, fold, disambiguation, offset), Instant, Now (só os tipos, nunca os valores) e toLocaleString.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa grava `R` dentro de try/catch (`Nome: mensagem` quando lança), via `node:vm`.runInThisContext.
// Resultado com caminho da máquina ou programa repetido de outro golden de Temporal é descartado. Determinístico.
// Uso: bun scripts/gen-temporal-math-golden.js > tests/golden/temporal_math_bun.tsv
const fs = require("fs");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const path = require("path");
const vm = require("node:vm");

const golden = path.join(__dirname, "../tests/golden");
const LIMIT = 500;
const programs = [];
const seen = new Set();
const q = (text) => JSON.stringify(text);

// Programa com valor textual: `String(expr)`.
const E = (expr) => `try { R = String(${expr}) } catch (e) { R = e.name + ': ' + e.message }`;
// Programa com valor estruturado: `JSON.stringify(expr)`.
const J = (expr) => `try { R = JSON.stringify(${expr}) } catch (e) { R = e.name + ': ' + e.message }`;
const add = (...sources) => {
  for (const source of sources) {
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};
const str = (...exprs) => add(...exprs.map(E));
const json = (...exprs) => add(...exprs.map(J));

const D = "Temporal.Duration";
const PD = "Temporal.PlainDate";
const PDT = "Temporal.PlainDateTime";
const YM = "Temporal.PlainYearMonth";
const MD = "Temporal.PlainMonthDay";
const Z = "Temporal.ZonedDateTime";
const I = "Temporal.Instant";

// ---------- Duration: from e toString ----------
const durStrings = ["P1Y2M3W4DT5H6M7.008009010S", "PT0S", "P0D", "-P1D", "+PT1H", "PT1.5S", "PT0.000000001S", "P1W", "PT36H", "PT90M", "P1Y", "P1M", "pt1h",
  "P1D1Y", "PT", "P", "P1.5D", "PT1.5H", "PT1.5H30M", "P1Y2M3DT4H5M6,5S", "PT1,5S", "−P1D", "P99999999999D", "P4294967296Y", "P4294967295Y", "PT9007199254740991S", "PT9007199254740992S", "PT2562047788015215H", "PT0.9999999999S", "PT1S2M"];
for (const s of durStrings) str(`${D}.from(${q(s)}).toString()`);
for (const s of durStrings.slice(0, 12)) json(`${D}.from(${q(s)}).toJSON()`, `[${D}.from(${q(s)}).sign, ${D}.from(${q(s)}).blank]`);
for (const s of ["P1Y2M3W4DT5H6M7.008009010S", "PT90M", "PT0.5S", "P1D"]) {
  for (const o of [`{smallestUnit: "seconds"}`, `{smallestUnit: "minutes"}`, `{fractionalSecondDigits: 2}`, `{fractionalSecondDigits: 0, roundingMode: "ceil"}`, `{smallestUnit: "milliseconds", roundingMode: "trunc"}`, `{smallestUnit: "hours"}`, `{smallestUnit: "days"}`]) {
    str(`${D}.from(${q(s)}).toString(${o})`);
  }
}
str(`new ${D}(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).toString()`, `new ${D}(-1, -2).toString()`, `new ${D}(1, -2)`, `new ${D}(1.5)`, `new ${D}(Infinity)`, `new ${D}(NaN).toString()`,
  `new ${D}(0, 0, 0, 0, 0, 0, 0, 0, 0, 1e9).toString()`, `${D}.from({hours: 1, minutes: -1})`, `${D}.from({})`, `${D}.from({days: 1.5})`, `${D}.from({hour: 1})`, `${D}.from(5)`, `${D}.from(null)`,
  `${D}.from({years: 2 ** 32})`, `${D}.from({seconds: Number.MAX_SAFE_INTEGER}).toString()`, `${D}.from({seconds: Number.MAX_SAFE_INTEGER + 1})`, `${D}.from({milliseconds: 1e20}).toString()`, `${D}.from({nanoseconds: 1e30}).toString()`,
  `${D}.from({microseconds: 9007199254740993}).toString()`, `${D}.from({weeks: 1, days: -1})`, `${D}.from({days: -0}).sign`, `${D}.from({days: -0}).toString()`);
// ---------- Duration: with, negated, abs, sign, blank ----------
for (const s of ["P1Y2M3DT4H", "-PT5M", "PT0S", "P1W"]) {
  str(`${D}.from(${q(s)}).negated().toString()`, `${D}.from(${q(s)}).abs().toString()`, `${D}.from(${q(s)}).with({hours: 9}).toString()`, `${D}.from(${q(s)}).with({years: -1})`,
    `${D}.from(${q(s)}).with({})`, `${D}.from(${q(s)}).with({minutes: 1.5})`, `${D}.from(${q(s)}).with({unknown: 1}).toString()`, `${D}.from(${q(s)}).with("PT1H")`,
    `${D}.from(${q(s)}).negated().negated().toString()`, `${D}.from(${q(s)}).abs().sign`);
}
// ---------- Duration: add e subtract ----------
const addPairs = [["PT1H", "PT30M"], ["PT1H", "-PT90M"], ["P1D", "PT24H"], ["PT0.5S", "PT0.5S"], ["PT59M", "PT61M"], ["PT1S", "-PT1.000000001S"], ["PT1H", "{minutes: 15}"], ["P1D", "P1D"],
  ["P1W", "P1D"], ["PT1H", "P1D"], ["P1Y", "P1Y"], ["P1M", "P1M"], ["P1Y", "PT1H"], ["PT0S", "PT0S"], ["-P1D", "-P1D"], ["PT9007199254740991S", "PT1S"], ["PT1H", "5"]];
for (const [a, b] of addPairs) {
  const bx = b.startsWith("{") || /^\d/.test(b) ? b : q(b);
  str(`${D}.from(${q(a)}).add(${bx}).toString()`, `${D}.from(${q(a)}).subtract(${bx}).toString()`);
}
for (const rel of [`"2024-01-31"`, `"2024-02-29T00:00"`, `"2024-03-10T00:00[America/Sao_Paulo]"`, `{year: 2024, month: 1, day: 31}`, `"2024-01-31T00:00[UTC]"`, `"nope"`]) {
  str(`${D}.from("P1M").add("P1M", {relativeTo: ${rel}})`, `${D}.from("P1Y").add("P1D", {relativeTo: ${rel}})`, `${D}.from("P1M").subtract("P1D", {relativeTo: ${rel}})`);
}
str(`${D}.from("P1M").add("P1M").toString()`, `${D}.from("P1M").add("P1M", {relativeTo: "2024-01-31"}).toString()`, `${D}.from("P1M1D").add("P1M", {relativeTo: "2024-01-31"}).toString()`,
  `${D}.from("P1M").add("-P1D", {relativeTo: "2024-03-01"}).toString()`, `${D}.from("P1Y").add("PT24H", {relativeTo: "2024-01-01[UTC]"}).toString()`,
  `${D}.from("P1D").add("PT24H", {relativeTo: "2024-03-09T12:00[America/Sao_Paulo]"}).toString()`, `${D}.from("P1D").add("PT24H", {relativeTo: "2018-11-03T12:00[America/Sao_Paulo]"}).toString()`);
// ---------- Duration: compare ----------
for (const [a, b] of [["PT1H", "PT60M"], ["P1D", "PT24H"], ["PT1H", "PT61M"], ["P1W", "P7D"], ["P1M", "P30D"], ["P1Y", "P365D"], ["P1Y", "P366D"], ["PT0S", "PT0S"], ["-PT1S", "PT1S"], ["P1M", "P31D"]]) {
  str(`${D}.compare(${q(a)}, ${q(b)})`);
  for (const rel of [`"2024-01-01"`, `"2023-01-01"`, `"2024-02-01"`, `"2024-01-01[UTC]"`, `"2024-03-09T12:00[America/Sao_Paulo]"`]) str(`${D}.compare(${q(a)}, ${q(b)}, {relativeTo: ${rel}})`);
}
str(`${D}.compare("PT1H", "PT1H", {relativeTo: "bad"})`, `${D}.compare("PT1H")`, `${D}.compare()`, `${D}.compare({hours: 1}, {minutes: 60})`);
// ---------- Duration: round ----------
const roundSrc = ["PT1H29M59.999999999S", "PT1H30M", "-PT1H30M", "PT2H30M", "PT0.5S", "PT36H", "P1DT12H", "P1Y6M", "P1M15D", "P45D", "PT100M", "P1W3D"];
const modes = ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"];
for (const s of roundSrc.slice(0, 6)) {
  for (const m of modes) str(`${D}.from(${q(s)}).round({smallestUnit: "hours", roundingMode: ${q(m)}}).toString()`);
}
for (const s of roundSrc) {
  for (const lu of ["hours", "minutes", "days", "auto"]) str(`${D}.from(${q(s)}).round({largestUnit: ${q(lu)}}).toString()`);
}
for (const s of ["PT1H29M59.999999999S", "PT100M", "PT3725S"]) {
  for (const inc of [1, 5, 15, 30, 7, 60]) str(`${D}.from(${q(s)}).round({smallestUnit: "minutes", roundingIncrement: ${inc}}).toString()`);
}
for (const rel of [`"2024-01-31"`, `"2024-02-29T00:00"`, `"2024-03-09T12:00[America/Sao_Paulo]"`, `"2024-01-31[u-ca=hebrew]"`]) {
  for (const s of ["P1Y6M", "P45D", "P1M15D", "PT100H", "P400D", "-P40D"]) {
    for (const [lu, su] of [["years", "months"], ["months", "days"], ["weeks", "days"], ["days", "hours"], ["years", "years"]]) {
      str(`${D}.from(${q(s)}).round({largestUnit: ${q(lu)}, smallestUnit: ${q(su)}, relativeTo: ${rel}}).toString()`);
    }
  }
}
str(`${D}.from("P1M").round({largestUnit: "days"})`, `${D}.from("P1Y").round({smallestUnit: "months"})`, `${D}.from("PT1H").round({})`, `${D}.from("PT1H").round()`, `${D}.from("PT1H").round("hours").toString()`, `${D}.from("PT1H").round("bad")`,
  `${D}.from("PT1H").round({smallestUnit: "hours", largestUnit: "minutes"})`, `${D}.from("PT1H").round({smallestUnit: "minutes", roundingIncrement: 60})`, `${D}.from("PT1H").round({smallestUnit: "hours", roundingIncrement: 24})`,
  `${D}.from("PT1H").round({smallestUnit: "hours", roundingIncrement: 0})`, `${D}.from("PT1H").round({smallestUnit: "hours", roundingMode: "nearest"})`, `${D}.from("PT1H").round({smallestUnit: "hour"}).toString()`,
  `${D}.from("PT1H").round({smallestUnit: "nanoseconds", roundingIncrement: 1000}).toString()`, `${D}.from("PT1H").round({smallestUnit: "milliseconds", roundingIncrement: 1000})`);
// ---------- Duration: total ----------
for (const s of ["PT1H30M", "PT90M", "P1D", "P1W", "PT0.5S", "-PT1H30M", "PT1H0.000000001S", "P1M", "P1Y", "P1Y6M", "PT36H", "P1DT12H"]) {
  for (const u of ["hours", "minutes", "seconds", "milliseconds", "days", "weeks", "nanoseconds"]) str(`${D}.from(${q(s)}).total(${q(u)})`);
}
for (const s of ["P1M", "P1Y", "P1Y6M", "P45D", "P1M15D", "-P1M", "PT36H", "P1W"]) {
  for (const rel of [`"2024-01-31"`, `"2024-02-01"`, `"2023-02-01"`, `"2024-03-09T12:00[America/Sao_Paulo]"`, `"2024-03-01[UTC]"`]) {
    for (const u of ["years", "months", "weeks", "days", "hours"]) str(`${D}.from(${q(s)}).total({unit: ${q(u)}, relativeTo: ${rel}})`);
  }
}
str(`${D}.from("P1M").total("days")`, `${D}.from("P1Y").total({unit: "months"})`, `${D}.from("PT1H").total()`, `${D}.from("PT1H").total({})`, `${D}.from("PT1H").total("hour")`, `${D}.from("PT1H").total("bad")`, `${D}.from("PT1H").total({unit: "hours", roundingMode: "ceil"})`,
  `${D}.from("P1D").total({unit: "hours", relativeTo: "2024-03-09T12:00[America/Sao_Paulo]"})`, `${D}.from("P1D").total({unit: "hours", relativeTo: "2018-11-04T00:00[America/Sao_Paulo]"})`, `${D}.from("P1D").total({unit: "hours", relativeTo: "2024-03-10T00:00[America/New_York]"})`,
  `${D}.from("P1D").total({unit: "hours", relativeTo: "2024-11-03T00:00[America/New_York]"})`, `${D}.from("P1D").total({unit: "hours", relativeTo: "2024-03-10"})`);

// ---------- PlainDate ----------
const dates = ["2024-01-31", "2024-02-29", "2023-02-28", "2024-12-31", "2024-03-31", "-000001-12-31", "+275760-09-13", "-271821-04-19", "1970-01-01"];
for (const d of dates) {
  for (const du of ["P1M", "P1Y", "-P1M", "P1M1D", "P13M", "P1Y1M1D", "P1W", "P1W1D", "PT24H", "-P1Y", "P400Y", "P1Y1M", "PT0S"]) {
    for (const o of ["constrain", "reject"]) str(`${PD}.from(${q(d)}).add(${q(du)}, {overflow: ${q(o)}}).toString()`);
  }
  str(`${PD}.from(${q(d)}).subtract({months: 1}).toString()`, `${PD}.from(${q(d)}).subtract({years: 1, months: 1, days: 1}, {overflow: "reject"}).toString()`, `${PD}.from(${q(d)}).add({hours: 25}).toString()`, `${PD}.from(${q(d)}).add({hours: 25, minutes: 1})`,
    `${PD}.from(${q(d)}).add({days: 1.5})`, `${PD}.from(${q(d)}).add({})`, `${PD}.from(${q(d)}).add({day: 1})`);
}
const pairs = [["2024-01-31", "2024-03-01"], ["2024-03-31", "2024-02-29"], ["2020-02-29", "2024-02-28"], ["2020-02-29", "2021-02-28"], ["2024-12-31", "2025-01-01"], ["2023-01-01", "2024-01-01"], ["2024-05-31", "2024-06-30"], ["2000-01-01", "2024-10-08"], ["2024-02-29", "2023-02-28"]];
for (const [a, b] of pairs) {
  for (const lu of ["years", "months", "weeks", "days"]) {
    str(`${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}}).toString()`, `${PD}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
  }
  str(`${PD}.from(${q(a)}).until(${q(b)}).toString()`, `${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: "months", smallestUnit: "months", roundingMode: "ceil"}).toString()`,
    `${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: "years", smallestUnit: "months", roundingMode: "halfExpand", roundingIncrement: 3}).toString()`,
    `${PD}.from(${q(a)}).since(${q(b)}, {smallestUnit: "weeks", roundingMode: "floor"}).toString()`, `${PD}.from(${q(a)}).until(${q(b)}, {largestUnit: "hours"})`, `${PD}.from(${q(a)}).until(${q(b)}, {smallestUnit: "hours"})`);
}
str(`${PD}.from("2024-01-31").until("2024-03-01", {largestUnit: "years", smallestUnit: "years", roundingIncrement: 2})`, `${PD}.from("2024-01-31").until("2024-03-01[u-ca=hebrew]")`, `${PD}.from("2024-01-31").until("2024-03-01T10:00").toString()`,
  `${PD}.from("2024-01-31").until({year: 2024, month: 3, day: 1}).toString()`, `${PD}.from("2024-01-31").until()`, `${PD}.from("2024-01-31").until("bad")`, `${PD}.compare("2024-01-31", "2024-02-01")`, `${PD}.compare("2024-02-01", "2024-01-31")`, `${PD}.compare("2024-01-31", {year: 2024, month: 1, day: 31})`);
// with
for (const [f, o] of [["{month: 2}", "constrain"], ["{month: 2}", "reject"], ["{day: 31}", "constrain"], ["{day: 31}", "reject"], ["{month: 13}", "constrain"], ["{month: 13}", "reject"], ["{year: 2023}", "constrain"], ["{monthCode: \"M02\"}", "reject"], ["{day: 0}", "constrain"], ["{day: -1}", "constrain"]]) {
  for (const d of ["2024-01-31", "2024-02-29", "2023-03-31"]) str(`${PD}.from(${q(d)}).with(${f}, {overflow: ${q(o)}}).toString()`);
}
// from com campos e overflow
for (const [f] of [["{year: 2024, month: 2, day: 30}"], ["{year: 2024, month: 13, day: 1}"], ["{year: 2024, month: 0, day: 1}"], ["{year: 2023, month: 2, day: 29}"], ["{year: 2024, monthCode: \"M02\", day: 31}"], ["{year: 2024, month: 2, monthCode: \"M03\", day: 1}"], ["{year: 2024, month: 1.5, day: 1}"], ["{year: 2024, month: 1}"], ["{year: 2024, month: 1, day: Infinity}"], ["{year: 275760, month: 9, day: 14}"], ["{year: -271821, month: 4, day: 18}"]]) {
  for (const o of ["constrain", "reject"]) str(`${PD}.from(${f}, {overflow: ${q(o)}}).toString()`);
}
str(`${PD}.from("2024-02-29", {overflow: "bad"})`, `${PD}.from("2024-02-30")`, `${PD}.from("2024-02-30", {overflow: "constrain"})`, `${PD}.from("2023-02-29")`, `new ${PD}(2024, 2, 30)`, `new ${PD}(2024, 13, 1)`, `new ${PD}(275760, 9, 14)`, `new ${PD}(275760, 9, 13).toString()`, `new ${PD}(-271821, 4, 19).toString()`, `new ${PD}(-271821, 4, 18)`);
str(`${PD}.from("2024-02-29").dayOfWeek`, `${PD}.from("2024-02-29").dayOfYear`, `${PD}.from("2024-02-29").weekOfYear`, `${PD}.from("2024-12-30").weekOfYear`, `${PD}.from("2024-12-30").yearOfWeek`, `${PD}.from("2021-01-03").weekOfYear`, `${PD}.from("2021-01-03").yearOfWeek`, `${PD}.from("2024-02-29").daysInYear`, `${PD}.from("2100-02-01").daysInMonth`, `${PD}.from("2000-02-01").daysInMonth`, `${PD}.from("2024-02-29").daysInWeek`);

// ---------- PlainDateTime ----------
const dts = ["2024-01-31T23:59:59.999999999", "2024-02-29T12:00", "2024-03-10T02:30", "2024-12-31T00:00:00", "-000001-01-01T00:00", "+275760-09-13T00:00", "-271821-04-19T00:00:00.000000001", "1970-01-01T00:00"];
for (const d of dts) {
  for (const du of ["PT1S", "PT0.000000001S", "-PT1S", "P1M", "P1M1DT1H", "PT24H", "PT25H", "P1Y", "-P1M", "PT1H1.5S"]) {
    str(`${PDT}.from(${q(d)}).add(${q(du)}).toString()`, `${PDT}.from(${q(d)}).subtract(${q(du)}).toString()`);
  }
  str(`${PDT}.from(${q(d)}).add({months: 1}, {overflow: "reject"}).toString()`, `${PDT}.from(${q(d)}).with({hour: 24}).toString()`, `${PDT}.from(${q(d)}).with({hour: 24}, {overflow: "reject"})`,
    `${PDT}.from(${q(d)}).with({minute: 60}).toString()`, `${PDT}.from(${q(d)}).with({millisecond: 1000}, {overflow: "reject"})`, `${PDT}.from(${q(d)}).round("hour").toString()`, `${PDT}.from(${q(d)}).round({smallestUnit: "day"}).toString()`,
    `${PDT}.from(${q(d)}).round({smallestUnit: "minute", roundingIncrement: 15, roundingMode: "floor"}).toString()`, `${PDT}.from(${q(d)}).toString({smallestUnit: "second"})`, `${PDT}.from(${q(d)}).toString({fractionalSecondDigits: 3, roundingMode: "ceil"})`);
}
const dtPairs = [["2024-01-31T10:00", "2024-03-01T09:00"], ["2024-03-01T09:00", "2024-01-31T10:00"], ["2024-01-01T00:00", "2024-01-01T00:00:00.000000001"], ["2020-02-29T23:59:59.999999999", "2024-02-29T00:00"], ["2024-12-31T23:00", "2025-01-01T01:00"]];
for (const [a, b] of dtPairs) {
  for (const lu of ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "nanoseconds"]) {
    str(`${PDT}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}}).toString()`, `${PDT}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
  }
  str(`${PDT}.from(${q(a)}).until(${q(b)}, {smallestUnit: "hours", roundingMode: "halfExpand"}).toString()`, `${PDT}.from(${q(a)}).until(${q(b)}, {largestUnit: "months", smallestUnit: "days", roundingMode: "ceil"}).toString()`,
    `${PDT}.from(${q(a)}).until(${q(b)}, {smallestUnit: "minutes", roundingIncrement: 45})`, `${PDT}.compare(${q(a)}, ${q(b)})`);
}
str(`${PDT}.from("2024-01-31T10:00").until("2024-03-01T09:00[UTC]").toString()`, `${PDT}.from("2024-01-31T10:00Z")`, `${PDT}.from("2024-01-31T24:00")`, `${PDT}.from("2024-01-31T23:60")`, `${PDT}.from("2024-01-31T23:59:60").toString()`, `${PDT}.from("2024-01-31T23:59:61")`,
  `${PDT}.from({year: 2024, month: 1, day: 31, hour: 25}).toString()`, `${PDT}.from({year: 2024, month: 1, day: 31, hour: 25}, {overflow: "reject"})`, `${PDT}.from({year: 2024, month: 1, day: 31, second: 60}).toString()`,
  `new ${PDT}(2024, 1, 31, 24)`, `new ${PDT}(2024, 1, 31, 0, 0, 0, 0, 0, 1000)`, `${PDT}.from("2024-01-31T10:00").toPlainDate().toString()`, `${PDT}.from("2024-01-31T10:00").toPlainTime().toString()`,
  `${PDT}.from("2024-01-31T10:00").toZonedDateTime("UTC").toString()`, `${PDT}.from("2024-03-10T02:30").toZonedDateTime("America/New_York").toString()`, `${PDT}.from("2024-03-10T02:30").toZonedDateTime("America/New_York", {disambiguation: "reject"})`);

// ---------- PlainYearMonth e PlainMonthDay ----------
for (const ym of ["2024-01", "2024-02", "2023-12", "-000001-01", "+275760-09", "-271821-04"]) {
  for (const du of ["P1M", "P13M", "-P1M", "P1Y", "P1Y1M", "P1M31D", "P1D", "PT0S", "-P12M", "P1Y2M3D"]) {
    for (const o of ["constrain", "reject"]) str(`${YM}.from(${q(ym)}).add(${q(du)}, {overflow: ${q(o)}}).toString()`);
  }
  str(`${YM}.from(${q(ym)}).subtract({months: 1}).toString()`, `${YM}.from(${q(ym)}).subtract({years: 1, days: 1}).toString()`);
}
for (const [a, b] of [["2024-01", "2025-03"], ["2025-03", "2024-01"], ["2024-02", "2024-02"], ["2020-12", "2024-01"], ["2024-12", "2025-01"]]) {
  for (const lu of ["years", "months"]) str(`${YM}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}}).toString()`, `${YM}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
  str(`${YM}.from(${q(a)}).until(${q(b)}, {largestUnit: "years", smallestUnit: "months", roundingIncrement: 6, roundingMode: "ceil"}).toString()`, `${YM}.from(${q(a)}).until(${q(b)}, {smallestUnit: "years", roundingMode: "halfExpand"}).toString()`);
}
for (const [md, o] of [["02-29", "constrain"], ["04-31", "constrain"], ["04-31", "reject"], ["02-30", "reject"], ["13-01", "constrain"]]) {
  str(`${MD}.from(${q(md)}, {overflow: ${q(o)}}).toString()`, `${MD}.from({monthCode: ${q("M" + md.slice(0, 2))}, day: ${Number(md.slice(3))}}, {overflow: ${q(o)}}).toString()`);
}
str(`${MD}.from({month: 2, day: 29}).toString()`, `${MD}.from({month: 2, day: 29, year: 2023}).toString()`, `${MD}.from({month: 2, day: 29, year: 2023}, {overflow: "reject"})`, `${MD}.from({month: 2, day: 30, year: 2024}).toString()`,
  `${MD}.from({month: 2, day: 30, year: 2024}, {overflow: "reject"})`, `${MD}.from("02-29").toPlainDate({year: 2023}).toString()`, `${MD}.from("02-29").add`, `${MD}.from("02-29").until`, `${MD}.from("02-29").with({day: 30}).toString()`, `${MD}.from("02-29").with({day: 30}, {overflow: "reject"})`);

// ---------- ZonedDateTime ----------
const zones = ["UTC", "+05:30", "-03:00", "America/Sao_Paulo", "Asia/Tokyo", "America/New_York", "Europe/London", "Asia/Kolkata", "Australia/Lord_Howe"];
for (const z of zones) {
  str(`${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).toString()`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).offset`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).offsetNanoseconds`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).epochMilliseconds`,
    `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).epochNanoseconds`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).hoursInDay`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).startOfDay().toString()`,
    `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).add({months: 1, hours: 12}).toString()`, `${Z}.from(${q(`2024-01-31T12:00[${z}]`)}).add({months: 1}).toString()`, `${Z}.from(${q(`2024-01-31T12:00[${z}]`)}).subtract({months: 11, days: 1}).toString()`,
    `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).until(${q(`2025-01-01T00:00[${z}]`)}, {largestUnit: "months"}).toString()`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).until(${q(`2025-01-01T00:00[${z}]`)}).toString()`,
    `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).round({smallestUnit: "day"}).toString()`, `${Z}.from(${q(`2024-06-15T12:34:56[${z}]`)}).round({smallestUnit: "hour", roundingMode: "ceil"}).toString()`,
    `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).withTimeZone("UTC").toString()`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).toInstant().toString()`, `${Z}.from(${q(`2024-06-15T12:00[${z}]`)}).toPlainDateTime().toString()`);
}
// gap e fold
const trans = [["America/New_York", "2024-03-10T02:30", "2024-11-03T01:30"], ["Europe/London", "2024-03-31T01:30", "2024-10-27T01:30"], ["America/Sao_Paulo", "2018-11-04T00:30", "2018-02-17T23:30"], ["Australia/Lord_Howe", "2024-10-06T02:15", "2024-04-07T01:45"], ["America/Sao_Paulo", "2019-06-01T12:00", "2019-06-01T12:00"]];
for (const [z, gap, fold] of trans) {
  for (const dis of ["compatible", "earlier", "later", "reject"]) {
    str(`${Z}.from(${q(`${gap}[${z}]`)}, {disambiguation: ${q(dis)}}).toString()`, `${Z}.from(${q(`${fold}[${z}]`)}, {disambiguation: ${q(dis)}}).toString()`,
      `${PDT}.from(${q(gap)}).toZonedDateTime(${q(z)}, {disambiguation: ${q(dis)}}).toString()`, `${PDT}.from(${q(fold)}).toZonedDateTime(${q(z)}, {disambiguation: ${q(dis)}}).offset`);
  }
  for (const off of ["use", "prefer", "ignore", "reject"]) {
    str(`${Z}.from(${q(`${fold}-03:00[${z}]`)}, {offset: ${q(off)}}).toString()`, `${Z}.from(${q(`${fold}+00:00[${z}]`)}, {offset: ${q(off)}}).toString()`, `${Z}.from(${q(`${fold}+01:00[${z}]`)}, {offset: ${q(off)}}).toString()`, `${Z}.from(${q(`${fold}-04:00[${z}]`)}, {offset: ${q(off)}}).toString()`);
  }
  str(`${Z}.from(${q(`${gap}[${z}]`)}).add({hours: 1}).toString()`, `${Z}.from(${q(`${gap}[${z}]`)}).add({days: 1}).toString()`, `${Z}.from(${q(`${fold}[${z}]`)}).add({hours: 1}).toString()`, `${Z}.from(${q(`${fold}[${z}]`)}).add({days: 1}).toString()`,
    `${Z}.from(${q(`${fold}[${z}]`)}).with({hour: 12}).toString()`, `${Z}.from(${q(`${gap}[${z}]`)}).with({minute: 0}, {disambiguation: "later"}).toString()`, `${Z}.from(${q(`${gap}[${z}]`)}).hoursInDay`, `${Z}.from(${q(`${fold}[${z}]`)}).hoursInDay`,
    `${Z}.from(${q(`${fold}[${z}]`)}).startOfDay().toString()`, `${Z}.from(${q(`${gap}[${z}]`)}).round({smallestUnit: "day"}).toString()`, `${Z}.from(${q(`${gap}[${z}]`)}).until(${q(`${fold}[${z}]`)}, {largestUnit: "days"}).toString()`,
    `${Z}.from(${q(`${gap}[${z}]`)}).getTimeZoneTransition("next").toString()`, `${Z}.from(${q(`${gap}[${z}]`)}).getTimeZoneTransition("previous").toString()`);
}
str(`${Z}.from("2024-06-15T12:00[UTC]").until("2024-06-15T12:00[Asia/Tokyo]")`, `${Z}.from("2024-06-15T12:00[UTC]").until("2024-06-15T12:00[Asia/Tokyo]", {largestUnit: "hours"}).toString()`, `${Z}.from("2024-06-15T12:00[UTC]").until("2024-06-15T12:00[UTC]", {largestUnit: "years"}).toString()`,
  `${Z}.from("2024-06-15T12:00+05:30[+05:30]").toString()`, `${Z}.from("2024-06-15T12:00+05:00[+05:30]")`, `${Z}.from("2024-06-15T12:00[+05:30]").timeZoneId`, `${Z}.from("2024-06-15T12:00[+0530]").timeZoneId`, `${Z}.from("2024-06-15T12:00[Etc/UTC]").timeZoneId`,
  `${Z}.from("2024-06-15T12:00[asia/tokyo]").timeZoneId`, `${Z}.from("2024-06-15T12:00[Mars/Base]")`, `${Z}.from("2024-06-15T12:00Z[UTC]").toString()`, `${Z}.from("2024-06-15T12:00Z")`, `${Z}.from("2024-06-15T12:00")`,
  `${Z}.from("2024-06-15T12:00[UTC]").equals("2024-06-15T12:00:00+00:00[UTC]")`, `${Z}.compare("2024-06-15T12:00[UTC]", "2024-06-15T09:00[America/Sao_Paulo]")`, `${Z}.compare("2024-06-15T12:00[UTC]", "2024-06-15T12:00[Asia/Tokyo]")`,
  `new ${Z}(0n, "UTC").toString()`, `new ${Z}(8640000000000000000000n, "UTC").toString()`, `new ${Z}(8640000000000000000001n, "UTC")`, `new ${Z}(0, "UTC")`, `new ${Z}(0n, "Bad/Zone")`, `new ${Z}(-8640000000000000000000n, "UTC").toString()`,
  `${Z}.from("2024-06-15T12:00[UTC]").toString({offset: "never", timeZoneName: "critical", calendarName: "always"})`, `${Z}.from("2024-06-15T12:00[UTC]").toString({timeZoneName: "never"})`, `${Z}.from("2024-06-15T12:00[UTC]").toString({smallestUnit: "minute"})`,
  `${Z}.from("2024-06-15T12:00[UTC]").with({offset: "+01:00"}).toString()`, `${Z}.from("2024-06-15T12:00[UTC]").with({offset: "+01:00"}, {offset: "use"}).toString()`, `${Z}.from("2024-06-15T12:00[UTC]").with({timeZone: "UTC"})`,
  `${Z}.from("2024-06-15T12:00[UTC]").valueOf()`, `${Z}.from("2024-06-15T12:00[UTC]").withPlainTime("03:00").toString()`, `${Z}.from("2024-06-15T12:00[UTC]").withCalendar("gregory").toString()`);
str(`${Z}.from("2024-06-15T12:00[UTC]").getTimeZoneTransition("next")`, `${Z}.from("2024-06-15T12:00[America/Sao_Paulo]").getTimeZoneTransition("next")`, `${Z}.from("2018-06-15T12:00[America/Sao_Paulo]").getTimeZoneTransition("next").toString()`,
  `${Z}.from("2024-06-15T12:00[Asia/Tokyo]").getTimeZoneTransition("previous").toString()`, `${Z}.from("2024-06-15T12:00[America/New_York]").getTimeZoneTransition({direction: "next"}).toString()`, `${Z}.from("2024-06-15T12:00[America/New_York]").getTimeZoneTransition("sideways")`);

// ---------- Instant ----------
const instants = ["1970-01-01T00:00:00Z", "2024-06-15T12:34:56.789123456Z", "-000001-01-01T00:00:00Z", "+275760-09-13T00:00:00Z", "-271821-04-20T00:00:00Z", "2024-06-15T12:00:00+05:30", "2024-06-15T12:00:00-03:00", "2024-06-15T12:00Z[UTC]", "2024-06-15T12:00", "2024-06-15T24:00:00Z", "2024-06-15T12:00:60Z"];
for (const s of instants) {
  str(`${I}.from(${q(s)}).toString()`, `${I}.from(${q(s)}).epochMilliseconds`, `${I}.from(${q(s)}).epochNanoseconds`);
}
for (const s of instants.slice(0, 3)) {
  for (const du of ["PT1H", "-PT1H", "PT0.000000001S", "PT36500000H", "PT24H", "P1D", "P1M", "PT1M1S", "-PT0.5S"]) str(`${I}.from(${q(s)}).add(${q(du)}).toString()`, `${I}.from(${q(s)}).subtract(${q(du)}).toString()`);
  for (const u of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) for (const m of ["floor", "halfExpand", "ceil"]) str(`${I}.from(${q(s)}).round({smallestUnit: ${q(u)}, roundingMode: ${q(m)}}).toString()`);
  str(`${I}.from(${q(s)}).toZonedDateTimeISO("Asia/Tokyo").toString()`, `${I}.from(${q(s)}).toZonedDateTimeISO("America/Sao_Paulo").toString()`, `${I}.from(${q(s)}).toZonedDateTimeISO("+05:30").toString()`, `${I}.from(${q(s)}).toZonedDateTimeISO("UTC").toString()`,
    `${I}.from(${q(s)}).toString({timeZone: "Asia/Tokyo"})`, `${I}.from(${q(s)}).toString({timeZone: "-03:00", fractionalSecondDigits: 4})`, `${I}.from(${q(s)}).toString({smallestUnit: "minute"})`, `${I}.from(${q(s)}).epochSeconds`, `${I}.from(${q(s)}).epochMicroseconds`);
}
for (const [a, b] of [["2020-01-01T00:00:00Z", "2024-06-15T12:34:56.789Z"], ["2024-06-15T12:34:56.789Z", "2020-01-01T00:00:00Z"], ["1970-01-01T00:00:00Z", "1970-01-01T00:00:00.000000001Z"]]) {
  for (const lu of ["hours", "minutes", "seconds", "milliseconds", "nanoseconds", "days", "auto"]) str(`${I}.from(${q(a)}).until(${q(b)}, {largestUnit: ${q(lu)}}).toString()`, `${I}.from(${q(a)}).since(${q(b)}, {largestUnit: ${q(lu)}}).toString()`);
  str(`${I}.from(${q(a)}).until(${q(b)}, {smallestUnit: "hours", roundingMode: "ceil", roundingIncrement: 12}).toString()`, `${I}.compare(${q(a)}, ${q(b)})`, `${I}.from(${q(a)}).equals(${q(b)})`);
}
str(`${I}.fromEpochMilliseconds(0).toString()`, `${I}.fromEpochMilliseconds(8.64e15).toString()`, `${I}.fromEpochMilliseconds(8.64e15 + 1)`, `${I}.fromEpochMilliseconds(1.5)`, `${I}.fromEpochMilliseconds(0n)`,
  `${I}.fromEpochNanoseconds(0n).toString()`, `${I}.fromEpochNanoseconds(-1n).toString()`, `${I}.fromEpochNanoseconds(8640000000000000000000n).toString()`, `${I}.fromEpochNanoseconds(8640000000000000000001n)`, `${I}.fromEpochNanoseconds(0)`,
  `${I}.fromEpochSeconds`, `${I}.fromEpochMicroseconds`, `new ${I}(1n).toString()`, `new ${I}(1)`, `${I}.from("1970-01-01T00:00:00Z").add({years: 1})`, `${I}.from("1970-01-01T00:00:00Z").add({hours: 1.5})`,
  `${I}.from("1970-01-01T00:00:00Z").toZonedDateTimeISO()`, `${I}.from("1970-01-01T00:00:00Z").toZonedDateTime`, `${I}.from("1970-01-01T00:00:00Z").valueOf()`, `${I}.from("1970-01-01T00:00:00Z").round("day").toString()`,
  `${I}.from("1970-01-01T00:00:00Z").round({smallestUnit: "day"}).toString()`, `${I}.from("1970-01-01T00:00:00Z").round({smallestUnit: "hour", roundingIncrement: 7})`, `${I}.from("1970-01-01T00:00:00Z").round({smallestUnit: "hour", roundingIncrement: 8}).toString()`);

// ---------- Now: apenas tipos ----------
str(`typeof Temporal.Now.instant().epochMilliseconds`, `typeof Temporal.Now.instant().epochNanoseconds`, `Temporal.Now.instant() instanceof ${I}`, `typeof Temporal.Now.timeZoneId()`, `Temporal.Now.zonedDateTimeISO("UTC").timeZoneId`,
  `Temporal.Now.zonedDateTimeISO("Asia/Tokyo").calendarId`, `Temporal.Now.plainDateISO("UTC") instanceof ${PD}`, `Temporal.Now.plainDateTimeISO("UTC") instanceof ${PDT}`, `Temporal.Now.plainTimeISO("UTC") instanceof Temporal.PlainTime`,
  `typeof Temporal.Now.plainDateISO("Asia/Tokyo").year`, `Temporal.Now.plainDateISO("Bad/Zone")`, `Temporal.Now.zonedDateTimeISO("+05:30").offset`, `Object.prototype.toString.call(Temporal.Now)`, `typeof Temporal.Now.zonedDateTimeISO().timeZoneId`,
  `Object.keys(Temporal.Now).length`, `Object.getOwnPropertyNames(Temporal.Now).sort().join()`, `Temporal.Now.instant().epochMilliseconds > 1.7e12`, `Temporal.Now.plainDateISO("UTC").calendarId`, `new Temporal.Now`, `Temporal.Now()`,
  `Temporal.Now.zonedDateTimeISO("UTC").hoursInDay`, `Temporal.Now.zonedDateTimeISO("UTC").offsetNanoseconds`, `Temporal.Now.zonedDateTimeISO("UTC").toInstant() instanceof ${I}`, `Temporal.Now.plainTimeISO("UTC").toString().length >= 8`);

// ---------- toLocaleString com timeZone fixo ----------
const zdt = `${Z}.from("2024-06-15T12:34:56[UTC]")`;
for (const loc of ["en-US", "pt-BR", "ja-JP", "de-DE"]) {
  for (const z of ["UTC", "America/Sao_Paulo", "Asia/Tokyo"]) {
    str(`${I}.from("2024-06-15T12:34:56Z").toLocaleString(${q(loc)}, {timeZone: ${q(z)}})`, `${zdt}.toLocaleString(${q(loc)})`, `${PD}.from("2024-06-15").toLocaleString(${q(loc)}, {timeZone: ${q(z)}})`,
      `${PDT}.from("2024-06-15T12:34:56").toLocaleString(${q(loc)}, {timeZone: ${q(z)}})`, `${Z}.from("2024-06-15T12:34:56[${z}]").toLocaleString(${q(loc)}, {timeZoneName: "short"})`);
  }
  str(`${YM}.from("2024-06").toLocaleString(${q(loc)}, {timeZone: "UTC"})`, `${MD}.from("06-15").toLocaleString(${q(loc)}, {timeZone: "UTC"})`, `${I}.from("2024-06-15T12:34:56Z").toLocaleString(${q(loc)}, {timeZone: "UTC", dateStyle: "full", timeStyle: "long"})`,
    `${I}.from("2024-06-15T12:34:56Z").toLocaleString(${q(loc)}, {timeZone: "UTC", hour12: false, hour: "2-digit", minute: "2-digit"})`);
}
str(`${PD}.from("2024-06-15").toLocaleString("en-US", {timeStyle: "short"})`, `${PD}.from("2024-06-15").toLocaleString("en-US", {hour: "numeric"})`, `${Z}.from("2024-06-15T12:34:56[UTC]").toLocaleString("en-US", {timeZone: "Asia/Tokyo"})`,
  `${I}.from("2024-06-15T12:34:56Z").toLocaleString("en-US", {timeZone: "Bad/Zone"})`, `${PD}.from("2024-06-15[u-ca=hebrew]").toLocaleString("en-US")`, `${PD}.from("2024-06-15[u-ca=gregory]").toLocaleString("en-US", {timeZone: "UTC"})`);

// ---------- Calendários ----------
for (const c of ["iso8601", "gregory", "japanese", "buddhist", "chinese", "hebrew", "islamic-civil", "islamic-umalqura"]) {
  for (const d of ["2024-02-29", "2024-01-31", "2023-10-08"]) {
    const p = `${PD}.from(${q(`${d}[u-ca=${c}]`)})`;
    str(`${p}.toString()`, `${p}.year`, `${p}.month`, `${p}.day`, `${p}.monthCode`, `${p}.era`, `${p}.add({months: 1}).toString()`, `${p}.add({years: 1}).toString()`, `${p}.add({months: 1}, {overflow: "reject"}).toString()`,
      `${p}.subtract({months: 13}).toString()`, `${p}.until("2030-06-15[u-ca=${c}]", {largestUnit: "months"}).toString()`, `${p}.until("2030-06-15[u-ca=${c}]", {largestUnit: "years"}).toString()`, `${p}.with({day: 30}).toString()`, `${p}.with({month: 12}).toString()`, `${p}.daysInYear`, `${p}.monthsInYear`);
  }
  str(`${PD}.from({year: 2024, month: 2, day: 30, calendar: ${q(c)}}).toString()`, `${PD}.from({year: 2024, month: 2, day: 30, calendar: ${q(c)}}, {overflow: "reject"}).toString()`, `${PD}.from({year: 2024, month: 13, day: 1, calendar: ${q(c)}}).toString()`);
}
str(`${PD}.from("2024-02-29[u-ca=gregory]").until("2024-03-01")`, `${PD}.compare("2024-02-29[u-ca=gregory]", "2024-02-29")`, `${PD}.from("2024-02-29[u-ca=gregory]").equals("2024-02-29")`, `${PD}.from("2024-02-29[u-ca=gregory]").add({months: 1}).calendarId`);

// ---------- Amostragem determinística até o LIMIT ----------
const known = new Set(knownPrograms("temporal_math_bun.tsv", (file) => /^temporal_.*_bun\.tsv$/.test(file) && file !== "temporal_math_bun.tsv"));
const lines = [];
const measured = programs.map((source) => {
  globalThis.R = undefined;
  try {
    vm.runInThisContext(source);
  } catch (e) {
    globalThis.R = "host " + e.name + ": " + e.message;
  }
  return [source, globalThis.R];
}).filter(([, result]) => typeof result === "string" && !/\/home\/|\/Users\/|bun/i.test(result));
// Amostra por hash do programa (sampleByHash), do conjunto medido inteiro, antes de descontar os goldens vizinhos.
const chosen = sampleByHash(measured, LIMIT, ([source]) => source).filter(([source]) => !known.has(source));
for (const [source, result] of chosen) lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result));
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.error(`${lines.length} programas escritos (de ${measured.length} medidos, ${programs.length} gerados)`);
