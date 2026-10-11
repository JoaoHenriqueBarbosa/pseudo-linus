// Gera tests/golden/temporal_duration_bun.tsv: programas de Temporal.Duration (total, round, add, subtract, negated,
// abs, with, compare, from, toString, strings ISO 8601, limites, erros) avaliados no bun 1.4.2. Linha:
// fonte<TAB>resultado, onde resultado é `ok<TAB>valor` ou `error<TAB>name<TAB>message JSON`.
// O harness é tests/golden/temporal_bun_harness.js, o mesmo texto que tests/temporal_duration_bun_golden.rs embute.
// Programas já presentes em temporal_bun.tsv e temporal_calendars_bun.tsv são descartados para não duplicar.
// Uso: bun scripts/gen-temporal-duration-golden.js   (determinístico: o conjunto é amostrado por passo fixo e
// nenhum programa depende do fuso do ambiente, os fusos aparecem escritos nas strings de relativeTo)
const fs = require("fs");
const { knownPrograms, sampleByHash } = require("./golden-prelude.js");
const path = require("path");

const harness = (0, eval)(fs.readFileSync(path.join(__dirname, "../tests/golden/temporal_bun_harness.js"), "utf8").trimEnd());
const LIMIT = 1500;
const existing = new Set();
for (const program of knownPrograms("temporal_duration_bun.tsv", ["temporal_bun.tsv", "temporal_calendars_bun.tsv"])) existing.add(program);
const groups = [];
let current = null;
const group = (name) => {
  current = { name, programs: [] };
  groups.push(current);
};
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (seen.has(source)) continue;
    seen.add(source);
    current.programs.push(source);
  }
};

const D = "Temporal.Duration";
const q = (text) => JSON.stringify(text);
const dur = (text) => `${D}.from(${q(text)})`;

const units = ["year", "month", "week", "day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond"];
const plural = (u) => u + "s";
const modes = ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"];
const dates = ["2024-01-31", "2024-02-29", "2023-02-28", "2024-03-31", "2024-12-31", "2019-01-31", "2000-02-29", "1999-12-31", "2024-05-15", "2024-10-31"];
const zdts = [
  "2024-03-09T12:00[America/New_York]", "2024-11-02T12:00[America/New_York]", "2018-11-03T12:00[America/Sao_Paulo]",
  "2018-02-16T12:00[America/Sao_Paulo]", "2024-01-31T00:00[UTC]", "2024-03-30T01:30[Europe/London]", "2024-01-31T10:00+05:30[+05:30]",
];
const durs = ["P1Y", "P1M", "P2M", "P1Y2M", "P1Y2M3D", "P1M15D", "P13M", "P1W", "P5W", "P1M1W", "P40D", "P400D", "P1Y1D", "P2Y30D", "P1Y11M30D", "-P1M", "-P1Y2M3D", "P1DT25H", "P1DT12H", "P3DT36H", "PT100H", "PT25H", "PT1H", "PT90M", "PT3661S", "PT0.5S", "P1M1DT1H1M1S", "PT36H"];
const clock = ["PT1H", "PT90M", "PT3661S", "PT1.123456789S", "PT0.5S", "PT36H", "PT59M59.999999999S", "PT1M", "PT3600.001S", "PT0.001002003S", "-PT1.5S", "PT100H", "PT86399S"];

// ---------- strings ISO 8601 ----------
group("iso");
const isoStrings = [
  "P1Y2M3W4DT5H6M7.008009001S", "P1Y", "P1M", "P1W", "P1D", "PT1H", "PT1M", "PT1S", "PT0S", "P0D", "P0Y", "-P1Y", "+P1Y", "−P1Y", "-PT0S", "+PT0S",
  "P1Y2M3W4DT5H6M7S", "PT0.1S", "PT0,1S", "PT0.123456789S", "PT0,123456789S", "PT0.1234567891S", "PT.5S", "PT5.S", "PT1.S", "PT1,S", "PT0.000000001S", "PT0.000000000S",
  "P1.5Y", "P1,5Y", "P1.5M", "P1.5W", "P1.5D", "PT1.5H", "PT1,5H", "PT1.5M", "PT1,5M", "PT1H1.5M", "PT1.5H1M", "PT1.5M1S", "PT1H1.5S",
  "p1y", "P1y2m3d", "pt1h", "P1Y2M3DT", "PT", "P", "-P", "PT1H2", "P1Y2", "PT1H1H", "P1M1Y", "P1D1W", "P1DT1H1Y", "PT1S1M", "P1Y1Y", "PT1M1H",
  "P-1Y", "PT-1H", "P1Y-1M", "P+1Y", "PT+1H", "P 1Y", " P1Y", "P1Y ", "P1Y\n", "", " ", "1Y", "T1H", "PT1H\u0000", "P1Y2M3DT4H5M6S7", "P1YT", "P1Y T1H",
  "P99999999999Y", "P4294967295Y", "P4294967296Y", "P4294967295M", "P4294967296M", "P4294967295W", "P4294967296W", "P9007199254740991D", "P9007199254740992D", "P9007199254740993D",
  "PT9007199254740991S", "PT9007199254740992S", "PT9007199254740991.999999999S", "PT2562047788015215H", "PT2562047788015216H", "PT153722867280912930M", "PT153722867280912931M",
  "PT9007199254740991M", "PT9007199254740991H", "PT9007199254740992H", "-PT9007199254740991S", "-PT9007199254740992S", "-P4294967295Y", "-P4294967296Y",
  "PT9007199254740.991S", "PT9007199254740991.001S", "P1Y2M3W4DT5H6M7.008009001S ", "P1Y2M3W4DT5H6M7.0080090019S", "PT1.1234567890123S",
  "P1Y2M3W4DT5H6M7,008S", "P1Y2M3W4DT5H6M7.8S", "PT0.5H", "PT0.5M", "P0.5D", "PT1H0.5M", "PT1H0.5S", "PT1H1M0.5S", "PT1.0S", "PT1.00S", "PT1.10S", "PT100.000S",
  "P1Y2M3W4DT5H6M7.008009001+00:00", "P1Y[UTC]", "P1Y2M3DT4H5M6S[u-ca=iso8601]", "PT1H−", "P−1Y", "−PT1H", "−P1Y2M", "-pt1h", "+p1y",
  "P1W1D", "P1W1Y", "P1M1W1D", "P1Y1W", "P1Y1M1W1DT1H1M1S", "PT1H1M1S", "PT1M1S", "P1DT1S", "P1DT1.5S", "P1DT1,5S", "P1Y1DT1H", "PT0H", "PT0M", "P0W", "P0M", "-P0D", "P00Y", "P001Y", "PT001H",
  "PT1S\n", "PT1H1M1S1", "P1Y1M1W1DT1H1M1.1S", "P1Y1M1W1DT1.1H", "P1Y1M1W1.1D", "PT0.1H", "PT0.1M", "PT1H0.1M", "PT1H1M0.1S",
];
for (const s of isoStrings) {
  add(`${D}.from(${q(s)}).toString()`, `${D}.from(${q(s)}).sign`, `${D}.from(${q(s)}).blank`, `${D}.from(${q(s)}).toJSON()`, `JSON.stringify(${D}.from(${q(s)}))`,
    `${D}.from(${q(s)}).seconds`, `${D}.from(${q(s)}).nanoseconds`, `${D}.from(${q(s)}).years`, `${D}.from(${q(s)}).hours`, `${D}.from(${q(s)}).milliseconds`,
    `${D}.from(${q(s)}).negated().toString()`, `${D}.from(${q(s)}).abs().toString()`, `${D}.from(${q(s)}).total("second")`, `${D}.from(${q(s)}).total("nanosecond")`,
    `${D}.from(${q(s)}).round("second").toString()`, `${D}.from(${q(s)}).round({largestUnit: "nanosecond"}).toString()`, `${D}.from(${q(s)}).round({largestUnit: "hour"}).toString()`);
}

// ---------- toString: fractionalSecondDigits e smallestUnit ----------
group("tostring");
const tsDurs = ["PT0S", "PT1S", "PT1.5S", "PT1.123456789S", "PT0.000000001S", "-PT1.999999999S", "PT59.9995S", "PT1H2M3.456S", "P1Y2M3W4DT5H6M7.008009001S", "PT9007199254740991.999999999S", "PT0.9999999995S", "PT0.5S", "PT1M", "PT100H", "-P1D", "P1D"];
const fsd = ["auto", 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, -1, "3", 1.9, NaN, Infinity, "x", null, undefined, true];
const smalls = ["second", "millisecond", "microsecond", "nanosecond", "seconds", "milliseconds", "microseconds", "nanoseconds", "minute", "hour", "day", "week", "month", "year", "auto", "x", "", null];
for (const s of tsDurs) {
  for (const f of fsd) add(`${D}.from(${q(s)}).toString({fractionalSecondDigits: ${typeof f === "number" || typeof f === "boolean" || f === null || f === undefined ? String(f) : q(f)}})`);
  for (const u of smalls) add(`${D}.from(${q(s)}).toString({smallestUnit: ${u === null ? "null" : q(u)}})`);
  for (const m of modes) {
    add(`${D}.from(${q(s)}).toString({fractionalSecondDigits: 2, roundingMode: ${q(m)}})`, `${D}.from(${q(s)}).toString({smallestUnit: "millisecond", roundingMode: ${q(m)}})`,
      `${D}.from(${q(s)}).toString({smallestUnit: "second", roundingMode: ${q(m)}})`, `${D}.from(${q(s)}).toString({fractionalSecondDigits: 0, roundingMode: ${q(m)}})`);
  }
  add(`${D}.from(${q(s)}).toString({smallestUnit: "second", fractionalSecondDigits: 3})`, `${D}.from(${q(s)}).toString(null)`, `${D}.from(${q(s)}).toString(1)`, `${D}.from(${q(s)}).toString("x")`,
    `${D}.from(${q(s)}).toString({roundingMode: "bad"})`, `${D}.from(${q(s)}).toString({roundingMode: "ceil"})`, `${D}.from(${q(s)}).toString(undefined)`, `${D}.from(${q(s)}).toString({})`, `${D}.from(${q(s)}).toLocaleString === undefined`);
}

// ---------- from com objetos e propriedades inválidas ----------
group("from");
const props = ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"];
const vals = ["0", "1", "-1", "1.5", "-0", "NaN", "Infinity", "-Infinity", "'3'", "'x'", "'1.5'", "''", "null", "undefined", "true", "{}", "[]", "[5]", "1n", "Symbol()", "{valueOf(){return 2}}", "9007199254740991", "9007199254740992", "-9007199254740991", "4294967295", "4294967296", "2**32", "1e21", "1e-7", "0.1", "' 5 '", "'0x10'", "'1e2'"];
for (const p of props) {
  for (const v of vals) add(`${D}.from({${p}: ${v}}).toString()`, `new ${D}(${p === "years" ? v : "0"}).toString()`);
  add(`${D}.from({${p}: 1, ${props[(props.indexOf(p) + 1) % 10]}: -1}).toString()`, `${D}.from({${p}: 1, ${props[(props.indexOf(p) + 3) % 10]}: 2}).sign`, `${D}.from({${p}: -1, ${props[(props.indexOf(p) + 4) % 10]}: -2}).sign`,
    `${D}.from({${p}: 0}).blank`, `${D}.from({${p}: -0}).sign`, `${D}.from({${p}: 1}).blank`);
}
add(`${D}.from({})`, `${D}.from({foo: 1})`, `${D}.from({year: 1})`, `${D}.from({day: 1, hour: 2})`, `${D}.from({days: 1, day: 2}).toString()`, `${D}.from([])`, `${D}.from(null)`, `${D}.from(undefined)`, `${D}.from()`,
  `${D}.from(1)`, `${D}.from(true)`, `${D}.from(Symbol())`, `${D}.from(1n)`, `${D}.from(function () {})`, `${D}.from(new Date(0))`, `${D}.from(new Map())`, `${D}.from(Object.create({days: 3})).toString()`,
  `${D}.from({get days() { return 4; }}).toString()`, `${D}.from(new Proxy({days: 1}, {})).toString()`, `${D}.from({days: 1, toString() { return "P2D"; }}).toString()`,
  `${D}.from(${dur("P1Y")}).toString()`, `${D}.from(${dur("P1Y")}) === ${dur("P1Y")}`, `(() => { const d = ${dur("P1Y")}; return ${D}.from(d) === d; })()`, `${D}.from(new ${D}(1, 2, 3)).toString()`,
  `${D}.from({days: 1}, {overflow: "reject"}).toString()`, `${D}.from("P1Y", {x: 1}).toString()`, `${D}.from("P1Y", 5).toString()`,
  `new ${D}().toString()`, `new ${D}(0).blank`, `new ${D}(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).toString()`, `new ${D}(-1, -2, -3, -4, -5, -6, -7, -8, -9, -10).toString()`, `new ${D}(1, -2)`, `new ${D}(undefined, 1).toString()`,
  `${D}(1)`, `new ${D}(1.5)`, `new ${D}("1").years`, `new ${D}(Infinity)`, `new ${D}(NaN)`, `new ${D}(null).toString()`, `new ${D}(true).years`, `new ${D}(Symbol())`, `new ${D}(1n)`, `new ${D}(2 ** 32).years`, `new ${D}(0, 0, 0, 0, 0, 0, 0, 0, 0, 2 ** 53)`,
  `new ${D}(0, 0, 0, 2 ** 53)`, `new ${D}(0, 0, 0, 0, 2 ** 53)`, `new ${D}(0, 0, 0, 0, 0, 2 ** 53)`, `new ${D}(0, 0, 0, 0, 0, 0, 2 ** 53)`, `new ${D}(0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER).toString()`,
  `new ${D}(0, 0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER).toString()`, `new ${D}(0, 0, 0, 0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER).toString()`,
  `new ${D}(0, 0, 0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER).toString()`, `new ${D}(2 ** 32 - 1, 2 ** 32 - 1, 2 ** 32 - 1).toString()`, `new ${D}(2 ** 32, 0, 0)`, `new ${D}(0, 2 ** 32)`, `new ${D}(0, 0, 2 ** 32)`,
  `new ${D}(-(2 ** 32))`, `new ${D}(-(2 ** 32 - 1)).toString()`, `new ${D}(0, 0, 0, 0, 0, 0, 0, 0, 0, -(2 ** 53))`, `new ${D}(0, 0, 0, -(2 ** 53) + 1).toString()`,
  `${D}.length`, `${D}.name`, `${D}.prototype[Symbol.toStringTag]`, `Object.prototype.toString.call(${dur("P1Y")})`, `typeof ${dur("P1Y")}`, `${dur("P1Y")} instanceof ${D}`,
  `Object.keys(${dur("P1Y")}).length`, `JSON.stringify(Object.getOwnPropertyNames(${D}.prototype))`, `${dur("P1Y")}.valueOf()`, `${dur("P1Y")} + 1`, `${dur("P1Y")} < ${dur("P2Y")}`,
  `${D}.prototype.toString.call({})`, `${D}.prototype.years`, `Object.getOwnPropertyDescriptor(${D}.prototype, "years").get.call({})`, `${D}.prototype.sign`, `${D}.prototype.blank`);

// ---------- with ----------
group("with");
const withBases = ["P1Y2M3W4DT5H6M7.008009001S", "-P1Y2M3W4DT5H6M7.008009001S", "PT0S", "P1D", "-PT1H"];
const withArgs = ["{}", "{years: 0}", "{years: 5}", "{years: -5}", "{months: 2, days: 3}", "{hours: 25}", "{nanoseconds: 1}", "{nanoseconds: -1}", "{weeks: 1.5}", "{years: Infinity}", "{years: NaN}", "{years: undefined}",
  "{years: null}", "{foo: 1}", "{year: 1}", "{years: 1, foo: 2}", "{days: '3'}", "{days: 'x'}", "{years: 2 ** 32}", "{days: 2 ** 53}", "{seconds: 2 ** 53}", "{milliseconds: 2 ** 53}", "{years: 1, months: -1}", "{years: -0}",
  "5", "'P1Y'", "null", "undefined", "[]", "true", "Symbol()", "{years: 0, months: 0, weeks: 0, days: 0, hours: 0, minutes: 0, seconds: 0, milliseconds: 0, microseconds: 0, nanoseconds: 0}",
  "{get years() { return 7; }}", "{valueOf() { return 1; }}", "new Proxy({days: 9}, {})", `${D}.from("P3D")`, "{days: 1, hours: -1}"];
for (const b of withBases) for (const a of withArgs) add(`${dur(b)}.with(${a}).toString()`);
add(`${dur("P1Y")}.with()`, `${D}.prototype.with.call({}, {})`, `${dur("P1Y")}.with({years: 1}, {x: 1}).toString()`, `${dur("P1Y")}.with({years: 1}) === ${dur("P1Y")}`);

// ---------- negated, abs, sign, blank ----------
group("negabs");
for (const s of ["P1Y", "-P1Y", "PT0S", "-PT0S", "P1Y2M3W4DT5H6M7.008009001S", "-P1Y2M3W4DT5H6M7.008009001S", "PT0.000000001S", "-PT0.000000001S", "P4294967295Y", "-P4294967295Y", "PT9007199254740991S", "-PT9007199254740991S", "PT9007199254740991.999999999S"]) {
  add(`${dur(s)}.negated().negated().toString()`, `${dur(s)}.abs().abs().toString()`, `${dur(s)}.negated().abs().toString()`, `${dur(s)}.abs().negated().toString()`, `Object.is(${dur(s)}.negated().years, -0)`, `Object.is(${dur(s)}.abs().years, -0)`,
    `Object.is(${dur(s)}.negated().nanoseconds, -0)`, `${dur(s)}.negated().sign`, `${dur(s)}.abs().sign`, `${dur(s)}.negated().blank`, `${dur(s)}.negated() === ${dur(s)}`, `JSON.stringify(${dur(s)}.negated())`,
    `Object.is(${dur(s)}.negated().days, -0)`, `Object.is(${dur(s)}.negated().weeks, -0)`, `${D}.compare(${dur(s)}, ${dur(s)}.negated().negated())`);
}
add(`${D}.prototype.negated.call({})`, `${D}.prototype.abs.call(null)`, `${dur("P1Y")}.negated(1).toString()`);

// ---------- compare ----------
group("compare");
const cmpDurs = ["PT0S", "PT1S", "PT60S", "PT1M", "PT1H", "PT60M", "PT24H", "P1D", "P1D1S", "P7D", "P1W", "-P1D", "-PT1H", "PT1.000000001S", "PT0.999999999S", "P1M", "P30D", "P31D", "P28D", "P1Y", "P365D", "P366D", "P12M", "P52W", "P53W",
  "PT90M", "PT1H30M", "PT5400S", "P1DT1H", "PT25H", "PT23H", "PT23H59M59.999999999S"];
for (const a of cmpDurs) {
  for (const b of cmpDurs) {
    const hasCal = /[YMW]/.test(a.split("T")[0]) || /[YMW]/.test(b.split("T")[0]);
    if (!hasCal) add(`${D}.compare(${q(a)}, ${q(b)})`);
  }
}
for (const [a, b] of [["P1M", "P30D"], ["P1M", "P31D"], ["P1M", "P28D"], ["P1M", "P29D"], ["P1Y", "P365D"], ["P1Y", "P366D"], ["P12M", "P1Y"], ["P1W", "P7D"], ["P2W", "P14D"], ["P1M", "P4W"], ["P1M", "P5W"], ["P1Y", "P52W"], ["P1Y", "P53W"], ["P1Y1M", "P13M"], ["P2M", "P59D"], ["P2M", "P60D"], ["P2M", "P61D"], ["P1Y", "P12M1D"], ["-P1M", "-P30D"], ["P1M", "-P1M"]]) {
  add(`${D}.compare(${q(a)}, ${q(b)})`, `${D}.compare(${q(a)}, ${q(b)}, {relativeTo: "2024-01-31"})`);
  for (const d of dates) add(`${D}.compare(${q(a)}, ${q(b)}, {relativeTo: ${q(d)}})`);
  for (const z of zdts) add(`${D}.compare(${q(a)}, ${q(b)}, {relativeTo: ${q(z)}})`);
  add(`${D}.compare(${q(a)}, ${q(b)}, {relativeTo: Temporal.PlainDate.from("2024-02-01")})`, `${D}.compare(${q(a)}, ${q(b)}, {relativeTo: {year: 2024, month: 2, day: 1}})`,
    `${D}.compare(${q(a)}, ${q(b)}, {relativeTo: Temporal.ZonedDateTime.from("2024-03-09T12:00[America/New_York]")})`, `${D}.compare(${q(a)}, ${q(b)}, {relativeTo: {year: 2024, month: 3, day: 9, timeZone: "America/New_York"}})`);
}
add(`${D}.compare()`, `${D}.compare("PT1S")`, `${D}.compare("PT1S", "bad")`, `${D}.compare({}, "PT1S")`, `${D}.compare(null, "PT1S")`, `${D}.compare({days: 1}, {hours: 24})`, `${D}.compare({days: 1}, {hours: 25})`,
  `${D}.compare("P1D", "PT24H", {relativeTo: "2024-03-10T00:00[America/New_York]"})`, `${D}.compare("P1D", "PT24H", {relativeTo: "2024-03-09T12:00[America/New_York]"})`, `${D}.compare("P1D", "PT24H", {relativeTo: "2024-11-03T00:00[America/New_York]"})`,
  `${D}.compare("P1D", "PT23H", {relativeTo: "2024-03-10T00:00[America/New_York]"})`, `${D}.compare("P1D", "PT25H", {relativeTo: "2024-11-03T00:00[America/New_York]"})`, `${D}.compare("P1D", "PT24H", {relativeTo: "2018-11-04T00:00[America/Sao_Paulo]"})`,
  `${D}.compare("P1D", "PT24H", {relativeTo: "2018-11-03T00:00[America/Sao_Paulo]"})`, `${D}.compare("P1D", "PT24H", {relativeTo: "2024-01-01"})`, `${D}.compare("P1D", "PT24H", {relativeTo: "2024-01-01T00:00"})`,
  `${D}.compare("P1D", "PT24H", {relativeTo: "bad"})`, `${D}.compare("P1D", "PT24H", {relativeTo: 5})`, `${D}.compare("P1D", "PT24H", {relativeTo: null})`, `${D}.compare("P1D", "PT24H", {relativeTo: undefined})`,
  `${D}.compare("P1D", "PT24H", {relativeTo: {}})`, `${D}.compare("P1D", "PT24H", {relativeTo: {year: 2024}})`, `${D}.compare("P1D", "PT24H", {relativeTo: {year: 2024, month: 1, day: 1, timeZone: "Nope/Zone"}})`,
  `${D}.compare("P1D", "PT24H", 5)`, `${D}.compare("P1D", "PT24H", null)`, `${D}.compare("P1D", "PT24H", {})`, `${D}.compare("P1M", "P30D", {})`, `${D}.compare("P1M", "P30D", undefined)`,
  `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-31T00:00Z"})`, `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-31T00:00+00:00"})`, `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-31[UTC]"})`,
  `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-31T00:00[u-ca=hebrew]"})`, `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-31[u-ca=iso8601]"})`, `${D}.compare("P1M", "P30D", {relativeTo: "2024-01-31[+01:00]"})`,
  `${D}.compare("P1M", "P30D", {relativeTo: "+275760-09-13"})`, `${D}.compare("P1M", "P30D", {relativeTo: "-271821-04-19"})`, `${D}.compare("P1M", "P30D", {relativeTo: "-271821-04-20"})`, `${D}.compare("P1M", "P30D", {relativeTo: "+275760-09-14"})`,
  `${D}.compare("P9007199254740991D", "PT0S")`, `${D}.compare("P4294967295Y", "P4294967295Y", {relativeTo: "2024-01-31"})`, `${D}.compare("P1000000Y", "PT0S", {relativeTo: "2024-01-31"})`, `${D}.compare("P1Y", "P1Y")`,
  `${D}.compare("PT9007199254740991S", "PT9007199254740991.000000001S")`, `${D}.compare("PT9007199254740991S", "PT9007199254740990S")`, `${D}.compare("PT0.000000001S", "PT0S")`);

// ---------- total ----------
group("total");
const totalUnits = [...units, ...units.map(plural), "x", "", "auto", "Day", "DAY", "days "];
for (const s of clock) for (const u of units) add(`${dur(s)}.total(${q(u)})`, `${dur(s)}.total({unit: ${q(u)}})`);
for (const s of durs) {
  for (const u of units) {
    add(`${dur(s)}.total({unit: ${q(u)}})`, `${dur(s)}.total({unit: ${q(u)}, relativeTo: "2024-01-31"})`, `${dur(s)}.total({unit: ${q(u)}, relativeTo: "2024-02-29"})`, `${dur(s)}.total({unit: ${q(u)}, relativeTo: "2023-02-28"})`,
      `${dur(s)}.total({unit: ${q(u)}, relativeTo: "2024-03-09T12:00[America/New_York]"})`, `${dur(s)}.total({unit: ${q(u)}, relativeTo: "2024-11-02T12:00[America/New_York]"})`, `${dur(s)}.total({unit: ${q(u)}, relativeTo: "2018-11-03T12:00[America/Sao_Paulo]"})`,
      `${dur(s)}.total({unit: ${q(u)}, relativeTo: Temporal.PlainDate.from("2024-02-01")})`, `${dur(s)}.total({unit: ${q(u)}, relativeTo: Temporal.ZonedDateTime.from("2024-03-09T12:00[America/New_York]")})`);
  }
}
for (const u of totalUnits) add(`${dur("PT36H")}.total(${q(u)})`, `${dur("PT36H")}.total({unit: ${q(u)}})`);
add(`${dur("P1Y")}.total()`, `${dur("P1Y")}.total(undefined)`, `${dur("P1Y")}.total(null)`, `${dur("P1Y")}.total(5)`, `${dur("P1Y")}.total({})`, `${dur("PT1H")}.total({unit: "hour", relativeTo: "bad"})`, `${dur("PT1H")}.total({unit: "hour", relativeTo: 5})`,
  `${dur("PT1H")}.total({unit: "hour", relativeTo: null})`, `${dur("PT1H")}.total({unit: "hour", relativeTo: {}})`, `${dur("PT1H")}.total({unit: "hour", relativeTo: {year: 2024, month: 1, day: 1}})`, `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2024, month: 1}})`,
  `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2024, month: 3, day: 10, timeZone: "America/New_York"}})`, `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2024, month: 3, day: 10, timeZone: "America/New_York", hour: 12}})`,
  `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2024, month: 11, day: 3, timeZone: "America/New_York"}})`, `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2018, month: 11, day: 4, timeZone: "America/Sao_Paulo"}})`,
  `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2018, month: 11, day: 3, timeZone: "America/Sao_Paulo"}})`, `${dur("P1D")}.total({unit: "hour", relativeTo: {year: 2024, month: 3, day: 10, timeZone: "UTC", offset: "+01:00"}})`,
  `${dur("P1D")}.total({unit: "hour", relativeTo: "2024-03-10T00:00:00+01:00[UTC]"})`, `${dur("P1D")}.total({unit: "hour", relativeTo: "2024-03-10T00:00:00+00:00[UTC]"})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, day: 31, calendar: "iso8601"}})`,
  `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, day: 31, calendar: "hebrew"}})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, day: 31, calendar: "bad"}})`,
  `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, monthCode: "M01", day: 31}})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, monthCode: "M02", day: 31}})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 13, day: 31}})`,
  `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 2, day: 31}})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 0, day: 1}})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, day: 0}})`,
  `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, day: 1, hour: 25}})`, `${dur("P1M")}.total({unit: "day", relativeTo: {year: 2024, month: 1, day: 1, hour: 12, minute: 30}})`,
  `${dur("PT0S")}.total("year")`, `${dur("PT0S")}.total({unit: "year", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.total({unit: "year", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.total({unit: "year"})`, `${dur("P1M")}.total({unit: "month"})`, `${dur("P1W")}.total({unit: "week"})`,
  `${dur("P1W")}.total({unit: "day"})`, `${dur("P1W")}.total("hour")`, `${dur("P1D")}.total("hour")`, `${dur("P1D")}.total("week")`, `${dur("P1D")}.total({unit: "week", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.total({unit: "day"})`, `${dur("P1M")}.total({unit: "hour"})`,
  `${dur("P1D")}.total({unit: "hour", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.total({unit: "second", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.total({unit: "nanosecond", relativeTo: "2024-01-01"})`, `${dur("P100000Y")}.total({unit: "nanosecond", relativeTo: "2024-01-01"})`,
  `${dur("P1000000Y")}.total({unit: "day", relativeTo: "2024-01-01"})`, `${dur("P300000Y")}.total({unit: "day", relativeTo: "2024-01-01"})`, `${dur("P271821Y")}.total({unit: "day", relativeTo: "2024-01-01"})`, `${dur("-P271821Y")}.total({unit: "day", relativeTo: "2024-01-01"})`,
  `${dur("PT9007199254740991S")}.total("nanosecond")`, `${dur("PT9007199254740991S")}.total("microsecond")`, `${dur("PT9007199254740991S")}.total("millisecond")`, `${dur("PT9007199254740991S")}.total("day")`, `${dur("PT9007199254740991S")}.total("minute")`,
  `${dur("PT9007199254740991.999999999S")}.total("second")`, `${dur("PT9007199254740991.999999999S")}.total("nanosecond")`, `${dur("PT2562047788015215H")}.total("nanosecond")`, `${dur("PT2562047788015216H")}.total("nanosecond")`, `${dur("P104249991374D")}.total("nanosecond")`,
  `${dur("P104249991375D")}.total("nanosecond")`, `${dur("P104249991374D")}.total("hour")`, `${dur("P104249991375D")}.total("second")`, `${dur("P9007199254740991D")}.total("day")`, `${dur("P9007199254740991D")}.total("hour")`, `${dur("P9007199254740991D")}.total("nanosecond")`,
  `${dur("PT0.000000001S")}.total("year")`, `${dur("PT0.000000001S")}.total({unit: "year", relativeTo: "2024-01-01"})`, `${dur("-PT0.000000001S")}.total({unit: "day"})`, `${dur("PT1.5S")}.total("second")`, `${dur("PT1.5S")}.total("minute")`, `${dur("PT0.1S")}.total("millisecond")`,
  `${dur("PT0.3S")}.total("millisecond")`, `${dur("PT0.7S")}.total("microsecond")`, `${dur("PT1H1M1.001001001S")}.total("hour")`, `${dur("PT1H1M1.001001001S")}.total("minute")`, `${dur("PT1H1M1.001001001S")}.total("second")`, `${dur("PT1H1M1.001001001S")}.total("nanosecond")`,
  `${dur("PT1H1M1.001001001S")}.total("millisecond")`, `${dur("PT1H1M1.001001001S")}.total("microsecond")`, `${dur("PT1H1M1.001001001S")}.total("day")`, `${dur("-PT1H1M1.001001001S")}.total("hour")`, `${dur("-PT1H1M1.001001001S")}.total("day")`);

// ---------- round: unidades, incrementos e modos ----------
group("round");
const incs = [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 24, 25, 30, 50, 60, 100, 250, 500, 1000, 0, -1, 1.5, 2.9, 1e9, NaN, Infinity];
const roundUnitsAll = ["day", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond"];
for (const s of clock) {
  for (const u of roundUnitsAll) {
    add(`${dur(s)}.round(${q(u)}).toString()`, `${dur(s)}.round({smallestUnit: ${q(u)}}).toString()`, `${dur(s)}.round({smallestUnit: ${q(plural(u))}}).toString()`);
    for (const m of modes) add(`${dur(s)}.round({smallestUnit: ${q(u)}, roundingMode: ${q(m)}}).toString()`);
    for (const i of incs) add(`${dur(s)}.round({smallestUnit: ${q(u)}, roundingIncrement: ${i}}).toString()`);
    for (const lu of roundUnitsAll) add(`${dur(s)}.round({smallestUnit: ${q(u)}, largestUnit: ${q(lu)}}).toString()`);
  }
  for (const lu of ["auto", ...roundUnitsAll]) add(`${dur(s)}.round({largestUnit: ${q(lu)}}).toString()`);
}
for (const s of ["PT5H", "PT7H", "PT100M", "PT125S", "PT1.5S", "PT2.5S", "PT3.5S", "-PT2.5S", "-PT1.5S", "PT0.0015S", "PT0.0025S", "PT0.0005S", "PT0.5S", "PT12H", "PT36H", "PT0.000000500S", "PT0.000001500S", "PT0.000002500S"]) {
  for (const m of modes) {
    for (const u of ["hour", "minute", "second", "millisecond", "microsecond", "day"]) {
      for (const i of [1, 2, 5, 10, 60]) add(`${dur(s)}.round({smallestUnit: ${q(u)}, roundingMode: ${q(m)}, roundingIncrement: ${i}}).toString()`);
    }
  }
}
const incLimits = [["day", [1, 2]], ["hour", [23, 24, 25, 12, 8, 6]], ["minute", [59, 60, 61, 30, 20]], ["second", [59, 60, 61]], ["millisecond", [999, 1000, 1001, 500, 250]], ["microsecond", [999, 1000, 1001]], ["nanosecond", [999, 1000, 1001, 1e9]]];
for (const [u, list] of incLimits) for (const i of list) add(`${dur("PT100H")}.round({smallestUnit: ${q(u)}, roundingIncrement: ${i}}).toString()`, `${dur("PT100H")}.round({smallestUnit: ${q(u)}, largestUnit: ${q(u)}, roundingIncrement: ${i}}).toString()`,
  `${dur("P1DT1H1M1.001001001S")}.round({smallestUnit: ${q(u)}, largestUnit: "day", roundingIncrement: ${i}}).toString()`);
add(`${dur("PT1H")}.round()`, `${dur("PT1H")}.round({})`, `${dur("PT1H")}.round(undefined)`, `${dur("PT1H")}.round(null)`, `${dur("PT1H")}.round(5)`, `${dur("PT1H")}.round("x")`, `${dur("PT1H")}.round("")`, `${dur("PT1H")}.round({smallestUnit: "x"})`, `${dur("PT1H")}.round({largestUnit: "x"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", largestUnit: "minute"})`, `${dur("PT1H")}.round({smallestUnit: "hour", largestUnit: "second"})`, `${dur("PT1H")}.round({roundingMode: "ceil"})`, `${dur("PT1H")}.round({roundingIncrement: 2})`, `${dur("PT1H")}.round({roundingMode: "bad", smallestUnit: "hour"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", roundingIncrement: "x"})`, `${dur("PT1H")}.round({smallestUnit: "hour", roundingMode: null})`, `${dur("PT1H")}.round({smallestUnit: "hour", largestUnit: null})`, `${dur("PT1H")}.round({smallestUnit: null})`, `${dur("PT1H")}.round({smallestUnit: undefined, largestUnit: undefined})`,
  `${dur("PT1H")}.round({largestUnit: "auto", smallestUnit: "auto"})`, `${dur("PT1H")}.round({smallestUnit: "auto"})`, `${dur("PT1H")}.round({smallestUnit: "year"})`, `${dur("PT1H")}.round({smallestUnit: "month"})`, `${dur("PT1H")}.round({smallestUnit: "week"})`, `${dur("PT1H")}.round({largestUnit: "year"})`, `${dur("PT1H")}.round({largestUnit: "week"})`,
  `${dur("P1D")}.round({largestUnit: "week"})`, `${dur("P1D")}.round({largestUnit: "month"})`, `${dur("P1D")}.round({smallestUnit: "week"})`, `${dur("P1D")}.round({smallestUnit: "week", relativeTo: "2024-01-01"})`, `${dur("P1W")}.round({smallestUnit: "day"})`, `${dur("P1W")}.round({largestUnit: "day"})`,
  `${dur("P1W")}.round({largestUnit: "day", relativeTo: "2024-01-01"})`, `${dur("P1W")}.round({smallestUnit: "hour"})`, `${dur("P1M")}.round({smallestUnit: "hour"})`, `${dur("P1Y")}.round({smallestUnit: "hour"})`, `${dur("P1Y")}.round({smallestUnit: "second", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.round("day")`, `${dur("P1Y")}.round({largestUnit: "year"})`,
  `${dur("P1Y")}.round({largestUnit: "year", relativeTo: "2024-01-01"})`, `${dur("P1Y")}.round({largestUnit: "month", relativeTo: "2024-01-01"})`, `${dur("P1M")}.round({largestUnit: "month"})`, `${dur("P1M")}.round({largestUnit: "month", relativeTo: "2024-01-31"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "bad"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: 5})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: {}})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00[Nope/Zone]"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+01:00"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+01:00[UTC]"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00Z"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00Z[UTC]"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00[UTC]"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00[+01:00]"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+01:00[+01:00]"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+02:00[+01:00]"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+01:00[Europe/Paris]"})`,
  `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+02:00[Europe/Paris]"})`, `${dur("PT1H")}.round({smallestUnit: "hour", relativeTo: "2024-01-31T10:00:00+00:00:30[UTC]"})`,
  `${dur("PT90M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfExpand"}).toString()`, `${dur("PT90M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfEven"}).toString()`, `${dur("PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfEven"}).toString()`,
  `${dur("PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfTrunc"}).toString()`, `${dur("PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfCeil"}).toString()`, `${dur("-PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfCeil"}).toString()`,
  `${dur("-PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfFloor"}).toString()`, `${dur("-PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfEven"}).toString()`, `${dur("-PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfExpand"}).toString()`,
  `${dur("-PT150M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "halfTrunc"}).toString()`, `${dur("-PT90M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "ceil"}).toString()`, `${dur("-PT90M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "floor"}).toString()`,
  `${dur("-PT90M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "trunc"}).toString()`, `${dur("-PT90M")}.round({largestUnit: "hour", smallestUnit: "hour", roundingMode: "expand"}).toString()`,
  `${dur("PT9007199254740991S")}.round({largestUnit: "nanosecond"})`, `${dur("PT9007199254740991S")}.round({largestUnit: "microsecond"})`, `${dur("PT9007199254740991S")}.round({largestUnit: "millisecond"})`, `${dur("PT9007199254740991S")}.round({largestUnit: "second"}).toString()`,
  `${dur("PT9007199254740991S")}.round({largestUnit: "day"}).toString()`, `${dur("PT9007199254740991S")}.round({largestUnit: "hour"}).toString()`, `${dur("PT9007199254740991S")}.round({largestUnit: "minute"}).toString()`, `${dur("PT9007199254740991S")}.round({smallestUnit: "day"}).toString()`,
  `${dur("PT9007199254740991.999999999S")}.round({smallestUnit: "second"})`, `${dur("PT9007199254740991.999999999S")}.round({smallestUnit: "second", roundingMode: "floor"}).toString()`, `${dur("PT9007199254740991.999999999S")}.round({smallestUnit: "second", roundingMode: "halfExpand"})`,
  `${dur("P104249991374D")}.round({largestUnit: "nanosecond"})`, `${dur("P104249991374D")}.round({largestUnit: "hour"}).toString()`, `${dur("P104249991374D")}.round({largestUnit: "day"}).toString()`, `${dur("P104249991375D")}.round({smallestUnit: "hour"}).toString()`,
  `${dur("P9007199254740991D")}.round({smallestUnit: "hour"})`, `${dur("P9007199254740991D")}.round({largestUnit: "hour"})`, `${dur("P9007199254740991D")}.round({largestUnit: "day"}).toString()`, `${dur("P9007199254740991D")}.round({smallestUnit: "day"}).toString()`);

// ---------- round com relativeTo: balanceamento de dias, meses, anos ----------
group("round-relative");
const relDurs = ["P1M", "P1Y", "P13M", "P40D", "P45D", "P400D", "P1Y2M3D", "P1M1W", "P5W", "P1Y11M30D", "P2M30D", "P1M15D", "P100D", "P1DT25H", "PT100H", "PT800H", "P1Y1DT12H", "P1M1DT1H1M1S", "-P1M15D", "-P1Y2M3D", "P20D", "P60D", "P3M", "P6M"];
const largest = ["auto", "year", "month", "week", "day"];
for (const s of relDurs) {
  for (const d of dates) {
    for (const lu of largest) add(`${dur(s)}.round({largestUnit: ${q(lu)}, relativeTo: ${q(d)}}).toString()`);
    for (const su of ["year", "month", "week", "day"]) add(`${dur(s)}.round({smallestUnit: ${q(su)}, relativeTo: ${q(d)}}).toString()`, `${dur(s)}.round({smallestUnit: ${q(su)}, largestUnit: "year", relativeTo: ${q(d)}}).toString()`);
  }
}
for (const s of relDurs.slice(0, 16)) {
  for (const d of dates.slice(0, 5)) {
    for (const m of modes) {
      add(`${dur(s)}.round({smallestUnit: "month", roundingMode: ${q(m)}, relativeTo: ${q(d)}}).toString()`, `${dur(s)}.round({smallestUnit: "year", roundingMode: ${q(m)}, relativeTo: ${q(d)}}).toString()`,
        `${dur(s)}.round({smallestUnit: "week", largestUnit: "week", roundingMode: ${q(m)}, relativeTo: ${q(d)}}).toString()`, `${dur(s)}.round({smallestUnit: "day", largestUnit: "month", roundingMode: ${q(m)}, relativeTo: ${q(d)}}).toString()`);
    }
    for (const i of [2, 3, 4, 5, 6, 10, 12]) add(`${dur(s)}.round({smallestUnit: "month", roundingIncrement: ${i}, relativeTo: ${q(d)}}).toString()`, `${dur(s)}.round({smallestUnit: "year", roundingIncrement: ${i}, relativeTo: ${q(d)}}).toString()`,
      `${dur(s)}.round({smallestUnit: "week", roundingIncrement: ${i}, relativeTo: ${q(d)}}).toString()`, `${dur(s)}.round({smallestUnit: "day", roundingIncrement: ${i}, largestUnit: "month", relativeTo: ${q(d)}}).toString()`);
  }
}
for (const s of relDurs) {
  for (const z of zdts) {
    for (const lu of largest) add(`${dur(s)}.round({largestUnit: ${q(lu)}, relativeTo: ${q(z)}}).toString()`);
    for (const su of ["month", "week", "day", "hour"]) add(`${dur(s)}.round({smallestUnit: ${q(su)}, largestUnit: "year", relativeTo: ${q(z)}}).toString()`, `${dur(s)}.round({smallestUnit: ${q(su)}, relativeTo: ${q(z)}}).toString()`);
  }
}
for (const s of ["PT100H", "PT25H", "PT23H", "PT24H", "PT47H", "PT49H", "P1DT1H", "PT72H", "PT30H"]) {
  for (const z of zdts) {
    for (const lu of ["day", "week", "hour"]) add(`${dur(s)}.round({largestUnit: ${q(lu)}, smallestUnit: "hour", relativeTo: ${q(z)}}).toString()`, `${dur(s)}.round({largestUnit: ${q(lu)}, relativeTo: ${q(z)}}).toString()`);
    for (const m of ["ceil", "floor", "halfExpand", "trunc", "expand"]) add(`${dur(s)}.round({largestUnit: "day", smallestUnit: "day", roundingMode: ${q(m)}, relativeTo: ${q(z)}}).toString()`);
  }
}
add(`${dur("P1M")}.round({largestUnit: "month", relativeTo: {year: 2024, month: 1, day: 31}}).toString()`, `${dur("P1M")}.round({largestUnit: "week", relativeTo: {year: 2024, month: 1, day: 31}}).toString()`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: {year: 2024, month: 1, day: 31}}).toString()`,
  `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.PlainDate.from("2024-01-31")}).toString()`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.PlainDateTime.from("2024-01-31T12:00")}).toString()`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.PlainDateTime.from("2024-01-31T12:00:00")}).toString()`,
  `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.ZonedDateTime.from("2024-01-31T12:00[UTC]")}).toString()`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: new Date(0)})`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.Instant.from("2024-01-31T12:00Z")})`,
  `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.PlainYearMonth.from("2024-01")})`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.PlainMonthDay.from("01-31")})`, `${dur("P30D")}.round({largestUnit: "month", relativeTo: Temporal.PlainTime.from("12:00")})`,
  `${dur("P1M")}.round({largestUnit: "year", relativeTo: "2024-01-31"}).toString()`, `${dur("P12M")}.round({largestUnit: "year", relativeTo: "2024-01-31"}).toString()`, `${dur("P12M")}.round({largestUnit: "year", relativeTo: "2024-02-29"}).toString()`, `${dur("P365D")}.round({largestUnit: "year", relativeTo: "2024-01-01"}).toString()`,
  `${dur("P366D")}.round({largestUnit: "year", relativeTo: "2024-01-01"}).toString()`, `${dur("P365D")}.round({largestUnit: "year", relativeTo: "2023-01-01"}).toString()`, `${dur("P366D")}.round({largestUnit: "year", relativeTo: "2023-01-01"}).toString()`, `${dur("P1Y")}.round({largestUnit: "day", relativeTo: "2024-02-29"}).toString()`,
  `${dur("P1Y")}.round({largestUnit: "day", relativeTo: "2023-02-28"}).toString()`, `${dur("P1Y")}.round({largestUnit: "month", relativeTo: "2024-02-29"}).toString()`, `${dur("P1Y")}.round({largestUnit: "week", relativeTo: "2024-02-29"}).toString()`, `${dur("-P1Y")}.round({largestUnit: "day", relativeTo: "2024-02-29"}).toString()`,
  `${dur("-P1Y")}.round({largestUnit: "day", relativeTo: "2025-02-28"}).toString()`, `${dur("-P1M")}.round({largestUnit: "day", relativeTo: "2024-03-31"}).toString()`, `${dur("-P1M")}.round({largestUnit: "day", relativeTo: "2024-03-01"}).toString()`, `${dur("P1M")}.round({largestUnit: "day", relativeTo: "+275760-08-13"}).toString()`,
  `${dur("P1M")}.round({largestUnit: "day", relativeTo: "+275760-09-13"}).toString()`, `${dur("P1D")}.round({largestUnit: "day", relativeTo: "+275760-09-13"}).toString()`, `${dur("-P1D")}.round({largestUnit: "day", relativeTo: "-271821-04-19"}).toString()`, `${dur("P1Y")}.round({largestUnit: "day", relativeTo: "-271821-04-19"}).toString()`,
  `${dur("-P1Y")}.round({largestUnit: "day", relativeTo: "-271821-04-19"}).toString()`, `${dur("P1D")}.round({largestUnit: "day", relativeTo: "-271821-04-19T00:00[UTC]"}).toString()`, `${dur("-P1D")}.round({largestUnit: "day", relativeTo: "-271821-04-20T00:00[UTC]"}).toString()`,
  `${dur("P1D")}.round({largestUnit: "day", relativeTo: "+275760-09-13T00:00[UTC]"}).toString()`, `${dur("PT24H")}.round({largestUnit: "day", relativeTo: "+275760-09-12T00:00[UTC]"}).toString()`, `${dur("PT24H")}.round({largestUnit: "day", relativeTo: "+275760-09-13T00:00[UTC]"}).toString()`,
  `${dur("P1Y1M1W1DT1H1M1S")}.round({largestUnit: "year", smallestUnit: "second", relativeTo: "2024-01-31"}).toString()`, `${dur("P1Y1M1W1DT1H1M1S")}.round({largestUnit: "year", smallestUnit: "hour", relativeTo: "2024-01-31T12:00[UTC]"}).toString()`,
  `${dur("P1Y1M1W1DT1H1M1S")}.round({largestUnit: "year", smallestUnit: "year", relativeTo: "2024-01-31"}).toString()`, `${dur("P1Y1M1W1DT1H1M1S")}.round({largestUnit: "year", smallestUnit: "year", roundingMode: "expand", relativeTo: "2024-01-31"}).toString()`);

// ---------- add e subtract ----------
group("add-subtract");
const arith = ["PT0S", "P1D", "PT24H", "PT1H", "PT90M", "PT0.5S", "-PT1.5S", "PT1.000000001S", "P1W", "-P1D", "PT36H", "P1DT1H1M1.001001001S", "PT9007199254740991S", "-PT9007199254740991S", "PT2562047788015215H", "P104249991374D"];
for (const a of arith) {
  for (const b of arith) {
    add(`${dur(a)}.add(${q(b)}).toString()`, `${dur(a)}.subtract(${q(b)}).toString()`);
  }
}
for (const a of ["P1M", "P1Y", "P1Y2M3D", "P1W", "P1D", "-P1M", "P1M15D", "P40D", "P2M", "P1Y11M", "PT1H"]) {
  for (const b of ["P1M", "P1Y", "P1D", "P1W", "PT1H", "-P1M", "P1M15D", "P40D", "P1Y2M", "-P1D", "PT36H", "P30D"]) {
    const nonCal = !/[YM]/.test(a.split("T")[0]) && !/[YM]/.test(b.split("T")[0]);
    if (nonCal) continue;
    add(`${dur(a)}.add(${q(b)}).toString()`, `${dur(a)}.subtract(${q(b)}).toString()`);
    for (const d of dates.slice(0, 6)) add(`${dur(a)}.add(${q(b)}, {relativeTo: ${q(d)}}).toString()`, `${dur(a)}.subtract(${q(b)}, {relativeTo: ${q(d)}}).toString()`);
    for (const z of zdts.slice(0, 4)) add(`${dur(a)}.add(${q(b)}, {relativeTo: ${q(z)}}).toString()`, `${dur(a)}.subtract(${q(b)}, {relativeTo: ${q(z)}}).toString()`);
  }
}
for (const [a, b] of [["P1M", "P1M"], ["P1M", "-P1M"], ["P1Y", "-P1M"], ["P1M", "-P1Y"], ["P1Y", "P12M"], ["P1Y", "-P12M"], ["P1M", "P1D"], ["P1M", "-P1D"], ["P1D", "-P1M"], ["P13M", "-P1Y"], ["P2M", "-P30D"], ["P2M", "-P59D"], ["P2M", "-P60D"], ["P1Y2M3D", "-P1Y2M3D"]]) {
  for (const d of ["2024-01-31", "2024-02-29", "2023-01-31", "2024-12-31", "2000-01-01"]) add(`${dur(a)}.add(${q(b)}, {relativeTo: ${q(d)}}).toString()`, `${dur(a)}.subtract(${q(b)}, {relativeTo: ${q(d)}}).toString()`);
}
add(`${dur("PT1H")}.add()`, `${dur("PT1H")}.add(null)`, `${dur("PT1H")}.add(undefined)`, `${dur("PT1H")}.add({})`, `${dur("PT1H")}.add("bad")`, `${dur("PT1H")}.add(5)`, `${dur("PT1H")}.add({hours: 1}).toString()`, `${dur("PT1H")}.add({hours: -1}).toString()`, `${dur("PT1H")}.add({hours: 1, foo: 2}).toString()`,
  `${dur("PT1H")}.add(${dur("PT1H")}) === ${dur("PT1H")}`, `${dur("PT1H")}.add("PT1H", {relativeTo: "bad"})`, `${dur("PT1H")}.add("PT1H", {relativeTo: 5})`, `${dur("PT1H")}.add("PT1H", 5).toString()`, `${dur("PT1H")}.subtract("PT1H").toString()`, `${dur("PT1H")}.subtract("PT1H").sign`,
  `${dur("PT1H")}.subtract("PT2H").toString()`, `${dur("PT1H")}.subtract("PT2H").sign`, `${dur("PT0S")}.subtract("PT0S").toString()`, `Object.is(${dur("PT0S")}.subtract("PT0S").hours, -0)`, `Object.is(${dur("PT1H")}.subtract("PT1H").hours, -0)`,
  `${dur("PT9007199254740991S")}.add("PT1S")`, `${dur("PT9007199254740991S")}.add("PT1S").toString()`, `${dur("PT9007199254740991S")}.add("PT0.999999999S").toString()`, `${dur("PT9007199254740991S")}.add("PT1M")`, `${dur("PT9007199254740991S")}.add("-PT1M").toString()`,
  `${dur("-PT9007199254740991S")}.subtract("PT1S")`, `${dur("-PT9007199254740991S")}.subtract("PT0.999999999S").toString()`, `${dur("P9007199254740991D")}.add("P1D")`, `${dur("P9007199254740991D")}.add("PT24H")`, `${dur("P9007199254740991D")}.add("PT23H59M59S").toString()`,
  `${dur("P9007199254740991D")}.add("-P1D").toString()`, `${dur("P4294967295Y")}.add("P1Y")`, `${dur("P4294967295Y")}.add("P1Y", {relativeTo: "2024-01-01"})`, `${dur("P4294967295Y")}.add("-P1Y").toString()`, `${dur("P4294967295M")}.add("P1M")`, `${dur("P4294967295W")}.add("P1W")`,
  `${dur("P4294967295Y")}.add("P1M")`, `${dur("P4294967295Y")}.add("PT1S")`, `${dur("P4294967295Y")}.add("PT1S").toString()`, `${dur("P1Y")}.add("P1Y", {relativeTo: "+275760-01-01"})`, `${dur("P1M")}.add("P1M", {relativeTo: "+275760-09-01"})`, `${dur("P1D")}.add("P1D", {relativeTo: "+275760-09-12"})`,
  `${dur("P1D")}.add("P1D", {relativeTo: "+275760-09-13"})`, `${dur("-P1D")}.add("-P1D", {relativeTo: "-271821-04-20"})`, `${dur("-P1D")}.add("-P1D", {relativeTo: "-271821-04-19"})`, `${dur("P1D")}.add("PT1H", {relativeTo: "2024-03-10T00:00[America/New_York]"}).toString()`,
  `${dur("P1D")}.add("PT1H", {relativeTo: "2024-03-09T12:00[America/New_York]"}).toString()`, `${dur("P1D")}.add("PT25H", {relativeTo: "2024-11-03T00:00[America/New_York]"}).toString()`, `${dur("P1D")}.add("PT25H", {relativeTo: "2024-11-02T00:00[America/New_York]"}).toString()`,
  `${dur("P1M")}.add("PT25H", {relativeTo: "2024-11-02T00:00[America/New_York]"}).toString()`, `${dur("P1M")}.add("PT25H", {relativeTo: "2024-11-02T00:00[America/New_York]"}).toString()`, `${dur("P1D")}.subtract("PT25H", {relativeTo: "2024-11-04T00:00[America/New_York]"}).toString()`);

// ---------- instância e protótipo: this inválido, getters, toLocaleString, valueOf ----------
group("proto");
for (const m of ["toString", "toJSON", "toLocaleString", "valueOf", "negated", "abs", "with", "add", "subtract", "round", "total"]) {
  add(`${D}.prototype.${m}.call(undefined)`, `${D}.prototype.${m}.call(null)`, `${D}.prototype.${m}.call(1)`, `${D}.prototype.${m}.call({years: 1})`, `${D}.prototype.${m}.call(${q("P1Y")})`, `${D}.prototype.${m}.call(new Temporal.PlainDate(2024, 1, 1))`,
    `${D}.prototype.${m}.name`, `${D}.prototype.${m}.length`, `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(m)}).enumerable`, `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(m)}).writable`, `new ${D}.prototype.${m}()`);
}
for (const p of [...props, "sign", "blank"]) {
  add(`Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).get.name`, `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).set`, `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).enumerable`,
    `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).configurable`, `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).get.call(new Temporal.PlainDate(2024, 1, 1))`, `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).get.call(5)`,
    `Object.getOwnPropertyDescriptor(${D}.prototype, ${q(p)}).get.length`, `${dur("P1Y2M3W4DT5H6M7.008009001S")}.${p}`, `${dur("-P1Y2M3W4DT5H6M7.008009001S")}.${p}`, `Object.is(${dur("PT0S")}.${p}, 0)`, `Object.is(${dur("-PT0S")}.${p}, -0)`);
}
add(`${dur("P1Y")}.toLocaleString()`, `${dur("PT1H30M")}.toLocaleString()`, `${dur("PT1H30M")}.toLocaleString("en-US")`, `${dur("PT1H30M")}.toLocaleString("pt-BR")`, `${dur("P1Y2M")}.toLocaleString("en")`, `${dur("PT0S")}.toLocaleString()`, `${dur("-P1D")}.toLocaleString()`,
  `${dur("PT1H30M")}.toLocaleString("en-US", {style: "narrow"})`, `${dur("PT1H30M")}.toLocaleString("en-US", {style: "long"})`, `${dur("PT1H30M")}.toLocaleString("xx-invalid-locale-tag-zz")`, `${dur("PT1H30M")}.toLocaleString(undefined, {style: "x"})`);

// ---------- strings por unidade e casas decimais ----------
group("digits");
for (let digits = 1; digits <= 9; digits++) {
  const frac = String(123456789).slice(0, digits);
  for (const u of ["S"]) {
    add(`${dur(`PT1.${frac}S`)}.toString()`, `${dur(`PT1.${frac}S`)}.milliseconds`, `${dur(`PT1.${frac}S`)}.microseconds`, `${dur(`PT1.${frac}S`)}.nanoseconds`, `${dur(`-PT1.${frac}S`)}.nanoseconds`, `${dur(`PT1.${frac}S`)}.total("second")`,
      `${dur(`PT1.${frac}S`)}.total("millisecond")`, `${dur(`PT1.${frac}S`)}.total("nanosecond")`, `${dur(`PT1,${frac}S`)}.toString()`, `${dur(`PT1.${frac}S`)}.round({smallestUnit: "millisecond"}).toString()`, `${dur(`PT1.${frac}S`)}.round({smallestUnit: "microsecond"}).toString()`,
      `${dur(`PT1.${frac}S`)}.round({smallestUnit: "second", roundingMode: "ceil"}).toString()`, `${dur(`PT1.${frac}S`)}.toString({fractionalSecondDigits: ${digits}})`, `${dur(`PT1.${frac}S`)}.toString({fractionalSecondDigits: ${Math.max(0, digits - 1)}})`,
      `${dur(`PT1.${frac}S`)}.toString({fractionalSecondDigits: ${Math.min(9, digits + 1)}})`, `${dur(`PT${frac}S`)}.toString()`, `${dur(`PT0.${frac}S`)}.nanoseconds`, `${D}.from({seconds: 1, nanoseconds: ${Number(frac.padEnd(9, "0"))}}).toString()`,
      `${D}.from({milliseconds: ${Number(frac)}}).toString()`, `${D}.from({microseconds: ${Number(frac)}}).toString()`, `${D}.from({nanoseconds: ${Number(frac)}}).toString()`, `${D}.from({seconds: ${Number(frac)}}).toString()`, `${D}.from({minutes: ${Number(frac)}}).toString()`,
      `${D}.from({hours: ${Number(frac)}}).toString()`, `${D}.from({milliseconds: -${Number(frac)}, nanoseconds: -1}).toString()`, `${D}.from({seconds: 1, milliseconds: ${Number(frac)}, microseconds: ${Number(frac)}, nanoseconds: ${Number(frac)}}).toString()`);
  }
}
for (const [u, f] of [["years", 1], ["months", 1], ["weeks", 1], ["days", 1], ["hours", 1], ["minutes", 1], ["seconds", 1], ["milliseconds", 1], ["microseconds", 1], ["nanoseconds", 1]]) {
  for (const n of [1, 59, 60, 999, 1000, 1001, 999999, 1000000, 1000001, 999999999, 1000000000, 1000000001, 86399, 86400, 86401, 3599, 3600, 3601, 4294967295, 4294967296, 9007199254740991, 9007199254740992]) {
    add(`${D}.from({${u}: ${n}}).toString()`, `${D}.from({${u}: -${n}}).toString()`, `${D}.from({${u}: ${n}}).round({largestUnit: "nanosecond"}).toString()`, `${D}.from({${u}: ${n}}).round({largestUnit: "hour"}).toString()`);
  }
}

// ---------- Duration.from com unidades que somam além do limite (balanceamento) ----------
group("limits");
add(`${D}.from({hours: 2562047788015215, minutes: 30}).toString()`, `${D}.from({hours: 2562047788015216}).toString()`, `${D}.from({hours: 2562047788015215, minutes: 60}).toString()`, `${D}.from({minutes: 153722867280912930}).toString()`,
  `${D}.from({seconds: 9007199254740991, milliseconds: 999}).toString()`, `${D}.from({seconds: 9007199254740991, milliseconds: 1000}).toString()`, `${D}.from({seconds: 9007199254740991, milliseconds: 999, microseconds: 999, nanoseconds: 999}).toString()`,
  `${D}.from({seconds: 9007199254740991, milliseconds: 999, microseconds: 999, nanoseconds: 1000}).toString()`, `${D}.from({seconds: 9007199254740991, nanoseconds: 999999999}).toString()`, `${D}.from({seconds: 9007199254740991, nanoseconds: 1000000000}).toString()`,
  `${D}.from({seconds: 9007199254740991, nanoseconds: 1e9}).toString()`, `${D}.from({milliseconds: 9007199254740991}).toString()`, `${D}.from({milliseconds: 9007199254740991, microseconds: 9007199254740991}).toString()`, `${D}.from({microseconds: 9007199254740991, nanoseconds: 9007199254740991}).toString()`,
  `${D}.from({milliseconds: 9007199254740991, microseconds: 9007199254740991, nanoseconds: 9007199254740991}).toString()`, `${D}.from({days: 9007199254740991, hours: 9007199254740991}).toString()`, `${D}.from({days: 9007199254740991, hours: 24}).toString()`,
  `${D}.from({days: 104249991374, hours: 7}).toString()`, `${D}.from({days: 104249991374, hours: 8}).toString()`, `${D}.from({days: 104249991374, hours: 7, minutes: 12, seconds: 55, milliseconds: 807}).toString()`,
  `${D}.from({days: 104249991374, hours: 7, minutes: 12, seconds: 56}).toString()`, `${D}.from({days: 1, hours: 9007199254740991}).toString()`, `${D}.from({weeks: 9007199254740991}).toString()`, `${D}.from({weeks: 4294967296}).toString()`, `${D}.from({weeks: 4294967295, years: 4294967295, months: 4294967295}).toString()`,
  `${D}.from({years: 4294967295, days: 9007199254740991}).toString()`, `${D}.from({years: -4294967295, days: -9007199254740991}).toString()`, `${D}.from({years: 4294967295, days: -9007199254740991}).toString()`, `${D}.from({years: 1, days: 2 ** 53})`, `${D}.from({years: 1, hours: 2 ** 53})`,
  `${D}.from({years: 1, seconds: 2 ** 53})`, `${D}.from({years: 1, nanoseconds: 2 ** 53})`, `${D}.from({years: 1, nanoseconds: Number.MAX_VALUE})`, `${D}.from({nanoseconds: Number.MAX_VALUE})`, `${D}.from({nanoseconds: 1e300})`, `${D}.from({nanoseconds: 1e20})`, `${D}.from({nanoseconds: 1e16})`,
  `${D}.from({nanoseconds: 9.007199254740992e15})`, `${D}.from({nanoseconds: 9.007199254740991e15}).toString()`, `${D}.from({microseconds: 9.007199254740992e15})`, `${D}.from({milliseconds: 9.007199254740992e15})`, `${D}.from({seconds: 9.007199254740992e15})`,
  `${D}.from({minutes: 9.007199254740992e15})`, `${D}.from({hours: 9.007199254740992e15})`, `${D}.from({days: 9.007199254740992e15})`, `${D}.from({weeks: 4294967296})`, `${D}.from({months: 4294967296})`, `${D}.from({years: 4294967296})`, `${D}.from({years: 4294967295.5})`, `${D}.from({years: 4294967295.9})`,
  `${D}.from("P4294967295Y4294967295M4294967295WT0S").toString()`, `${D}.from("P4294967296Y")`, `${D}.from("P1Y4294967296M")`, `${D}.from("P1Y1M4294967296W")`, `${D}.from("P1Y1M1W9007199254740992D")`, `${D}.from("P1Y1M1W1DT2562047788015216H")`, `${D}.from("P1Y1M1W1DT153722867280912931M")`,
  `${D}.from("P1Y1M1W1DT9007199254740992S")`, `${D}.from("P1Y1M1W1DT9007199254740991.999999999S").toString()`, `${D}.from("P1Y1M1W1DT9007199254740991.9999999999S").toString()`, `${D}.from("PT9007199254740992S")`, `${D}.from("PT9007199254740993S")`, `${D}.from("PT18014398509481984S")`,
  `${D}.from("PT9007199254740991S").add("PT9007199254740991S")`, `${D}.from("PT9007199254740991S").add("PT9007199254740991S").toString()`, `${D}.from("PT9007199254740991S").negated().toString()`, `${D}.from("PT9007199254740991S").abs().toString()`,
  `${D}.from("PT2562047788015215H30M").toString()`, `${D}.from("PT2562047788015215H30M7.999999999S").toString()`, `${D}.from("PT2562047788015215H30M8S").toString()`, `${D}.from("PT2562047788015215H59M59.999999999S")`, `${D}.from("PT2562047788015215H59M59.999999999S").toString()`,
  `${D}.from("P104249991374DT7H12M55.999999999S")`, `${D}.from("P104249991374DT7H12M56S")`, `${D}.from("P104249991374DT7H12M56S").toString()`, `${D}.from("P104249991374DT7H12M56S").total("second")`, `${D}.from("P104249991374DT7H12M56S").round({largestUnit: "hour"}).toString()`);

// ---------- Calendários não ISO (medidos no bun 1.4.2, 2026-10-08) ----------
// Fora da amostragem e do LIMIT: entram inteiros, ao fim do tsv, para que as linhas anteriores não mudem.
// `islamic` puro não é identificador válido no bun 1.4.2 (RangeError), por isso só `islamic-umalqura` aqui.
// `add`/`subtract` com unidade de calendário e `relativeTo` dão sempre o RangeError de unidades de calendário.
const nonIsoPrograms = [];
const addNonIso = (...sources) => {
  for (const source of sources) {
    if (seen.has(source) || existing.has(source)) continue;
    seen.add(source);
    nonIsoPrograms.push(source);
  }
};
const nonIsoCalendars = ["hebrew", "chinese", "islamic-umalqura", "japanese", "persian", "buddhist", "coptic", "ethiopic"];
const nonIsoPlain = ["2024-03-10", "2024-01-01", "2024-12-31"];
const nonIsoZoned = ["2024-03-09T12:00[America/New_York]", "2024-11-02T12:00[America/New_York]"];
const nonIsoDurs = ["P1M", "P13M", "P1Y2M", "P400D", "PT36H"];
for (const cal of nonIsoCalendars) {
  for (const date of nonIsoPlain) {
    const rel = q(`${date}[u-ca=${cal}]`);
    for (const d of ["P13M", "P400D", "P1Y2M", "P200D"]) {
      addNonIso(
        `${dur(d)}.total({unit: "month", relativeTo: ${rel}})`,
        `${dur(d)}.total({unit: "day", relativeTo: ${rel}})`,
        `${dur(d)}.total({unit: "year", relativeTo: ${rel}})`,
        `${dur(d)}.round({largestUnit: "year", relativeTo: ${rel}}).toString()`,
        `${dur(d)}.round({largestUnit: "month", relativeTo: ${rel}}).toString()`,
        `${dur(d)}.round({smallestUnit: "year", relativeTo: ${rel}}).toString()`,
        `${dur(d)}.round({smallestUnit: "month", roundingMode: "ceil", relativeTo: ${rel}}).toString()`,
      );
    }
    addNonIso(
      `${D}.compare(${dur("P1M")}, ${dur("P30D")}, {relativeTo: ${rel}})`,
      `${D}.compare(${dur("P1M")}, ${dur("P29D")}, {relativeTo: ${rel}})`,
      `${D}.compare(${dur("P1Y")}, ${dur("P365D")}, {relativeTo: ${rel}})`,
      `${D}.compare(${dur("P13M")}, ${dur("P1Y")}, {relativeTo: ${rel}})`,
      `${dur("P1Y")}.add("P1M", {relativeTo: ${rel}}).toString()`,
      `${dur("P1D")}.add("P1M", {relativeTo: ${rel}}).toString()`,
      `${dur("P1M")}.subtract("P1D", {relativeTo: ${rel}}).toString()`,
      `${dur("P1D")}.add("PT36H", {relativeTo: ${rel}}).toString()`,
    );
  }
  for (const zdt of nonIsoZoned) {
    const rel = q(`${zdt}[u-ca=${cal}]`);
    for (const d of nonIsoDurs) {
      addNonIso(
        `${dur(d)}.total({unit: "month", relativeTo: ${rel}})`,
        `${dur(d)}.total({unit: "day", relativeTo: ${rel}})`,
        `${dur(d)}.round({largestUnit: "year", relativeTo: ${rel}}).toString()`,
        `${dur(d)}.round({smallestUnit: "month", relativeTo: ${rel}}).toString()`,
        `${dur(d)}.round({smallestUnit: "day", largestUnit: "month", relativeTo: ${rel}}).toString()`,
      );
    }
    addNonIso(
      `${D}.compare(${dur("P1M")}, ${dur("P30D")}, {relativeTo: ${rel}})`,
      `${dur("P1D")}.add("PT36H", {relativeTo: ${rel}}).toString()`,
      `${dur("P1M")}.add("P1M", {relativeTo: ${rel}}).toString()`,
    );
  }
}
addNonIso(
  `${dur("P400D")}.round({largestUnit: "month", relativeTo: "2024-01-01[u-ca=islamic]"}).toString()`,
  `${dur("P1Y")}.total({unit: "month", relativeTo: "2024-03-10[u-ca=hebrew]"})`,
  `${dur("P1Y2M")}.total({unit: "day", relativeTo: "2024-01-01[u-ca=chinese]"})`,
  `${dur("P400D")}.round({largestUnit: "year", relativeTo: "2024-01-01[u-ca=chinese]"}).toString()`,
);

// ---------- Amostragem determinística (sampleByHash) até o LIMIT, proporcional por grupo ----------
let total = 0;
for (const g of groups) total += g.programs.length;
const chosen = [];
let remaining = LIMIT;
let remainingTotal = total;
for (const g of groups) {
  const share = Math.min(g.programs.length, Math.round((g.programs.length * remaining) / remainingTotal));
  remaining -= share;
  remainingTotal -= g.programs.length;
  // sampleByHash devolve o grupo inteiro quando share >= tamanho; os vizinhos (existing) saem depois da amostra.
  chosen.push(...sampleByHash(g.programs, share).filter((source) => !existing.has(source)));
}
const lines = [];
chosen.push(...nonIsoPrograms);
for (const source of chosen) {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  lines.push(source + "\t" + harness(source));
}
const target = path.join(__dirname, "../tests/golden/temporal_duration_bun.tsv");
fs.writeFileSync(target, lines.join("\n") + "\n");
console.error(`${lines.length} programas escritos (de ${total} gerados, ${existing.size} já existentes descartados) em tests/golden/temporal_duration_bun.tsv`);
for (const g of groups) console.error(`  ${g.name}: ${g.programs.length}`);
console.error(`  nonIso (fora da amostragem, ao fim): ${nonIsoPrograms.length}`);
