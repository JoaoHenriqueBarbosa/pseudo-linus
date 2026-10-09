// Gera tests/golden/datetime_range_bun.tsv: Intl.DateTimeFormat formatRange e formatRangeToParts, hourCycle,
// calendar, numberingSystem, datas inválidas, formatToParts, resolvedOptions e Date.prototype.toLocale*String,
// medidos no bun 1.4.2. O fuso vai sempre dentro das opções (timeZone), nunca vem do ambiente.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Uso (da raiz da crate zjsc): bun scripts/gen-datetime-range-golden.js > tests/golden/datetime_range_bun.tsv
const { emitFactored } = require("./golden-prelude.js");
const rows = [];
const LOCALES = [
  "en", "pt", "es", "fr", "de", "ja", "ko", "zh", "ru", "ar", "hi", "tr", "it",
  "nl", "pl", "sv", "id", "th", "vi", "he", "uk", "cs", "el", "hu", "fa",
  "am", "my", "km", "lo", "mn", "ps", "sd", "so", "fil", "ha", "yo", "zu", "xh", "cy", "gd", "lb", "mt", "fo", "ky", "tg", "tk", "tt", "ku", "or", "as",
];
const T = (y, mo, d, h, mi, s, ms = 0) => Date.UTC(y, mo - 1, d, h, mi, s, ms);
const PAIRS = {
  sameDay: [T(2024, 3, 5, 7, 8, 9), T(2024, 3, 5, 19, 8, 9)],
  sameMonth: [T(2024, 3, 5, 7, 8, 9), T(2024, 3, 25, 19, 8, 9)],
  sameYear: [T(2024, 3, 5, 7, 8, 9), T(2024, 11, 25, 19, 8, 9)],
  otherYear: [T(2024, 3, 5, 7, 8, 9), T(2026, 11, 25, 19, 8, 9, 456)],
};
const SAMPLE = T(2024, 3, 5, 7, 8, 9, 123);
const SAMPLE_PM = T(2024, 11, 25, 19, 8, 9, 456);

const programs = [];
const j = JSON.stringify;
const withZone = options => ({ timeZone: "UTC", ...options });
const dtf = (locale, options) => `new Intl.DateTimeFormat(${j(locale)}, ${j(withZone(options))})`;
const attempt = body => `try { globalThis.R = ${body}; } catch (e) { globalThis.R = e.name + ": " + e.message; }`;
const addProgram = body => programs.push(attempt(body));

// ---- formatRange e formatRangeToParts.
const RANGE_SETS = [
  { dateStyle: "full" },
  { dateStyle: "medium" },
  { dateStyle: "short", timeStyle: "short" },
  { year: "numeric", month: "long", day: "numeric" },
  { hour: "numeric", minute: "2-digit" },
];
const PARTS_SETS = [{ year: "numeric", month: "short", day: "numeric" }, { dateStyle: "long", timeStyle: "short" }];
const joinParts = "parts.map(p => p.type + '(' + p.source + ')=' + p.value).join('|')";
for (const locale of LOCALES) {
  for (const [, [a, b]] of Object.entries(PAIRS)) {
    for (const set of RANGE_SETS) addProgram(`${dtf(locale, set)}.formatRange(${a}, ${b})`);
    for (const set of PARTS_SETS) {
      addProgram(`(parts => ${joinParts})(${dtf(locale, set)}.formatRangeToParts(${a}, ${b}))`);
    }
  }
}

// ---- Fusos diferentes e timeZoneName.
const ZONES = ["America/Sao_Paulo", "Asia/Tokyo", "Asia/Kolkata", "Europe/Berlin"];
for (const locale of LOCALES) {
  for (const zone of ZONES) {
    const [a, b] = PAIRS.sameDay;
    addProgram(`new Intl.DateTimeFormat(${j(locale)}, ${j({ timeZone: zone, hour: "numeric", minute: "2-digit", timeZoneName: "short" })}).formatRange(${a}, ${b})`);
    addProgram(`new Intl.DateTimeFormat(${j(locale)}, ${j({ timeZone: zone, dateStyle: "medium", timeStyle: "short" })}).formatRange(${PAIRS.sameYear[0]}, ${PAIRS.sameYear[1]})`);
  }
}

// ---- Fuso no intervalo: com a data diferente e com segundos o fuso fica nas duas pontas; sem segundos sai uma vez
// (molde do locale: ` GMT-3` em de, `(GMT-3)` em ja). E `dateStyle` com `timeStyle: "short"` no mesmo dia (` Uhr` em de).
for (const locale of LOCALES) {
  const [a, b] = PAIRS.sameDay;
  const sao = { timeZone: "America/Sao_Paulo" };
  const zoneSets = [
    { ...sao, year: "numeric", month: "short", day: "numeric", timeZoneName: "short" },
    { ...sao, year: "numeric", month: "long", day: "numeric", timeZoneName: "long" },
    { ...sao, weekday: "short", year: "numeric", month: "short", day: "numeric", timeZoneName: "shortGeneric" },
    { ...sao, year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", hourCycle: "h12", timeZoneName: "short" },
    { ...sao, year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", hourCycle: "h23", timeZoneName: "shortOffset" },
    { ...sao, hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "short" },
    { ...sao, hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "longOffset" },
    { ...sao, year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "short" },
    { ...sao, hour: "numeric", minute: "2-digit", timeZoneName: "short" },
    { ...sao, hour: "numeric", timeZoneName: "short" },
    { ...sao, year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", timeZoneName: "long" },
    { ...sao, dateStyle: "medium", timeStyle: "long" },
    { ...sao, dateStyle: "full", timeStyle: "full" },
  ];
  for (const set of zoneSets) {
    for (const [from, to] of [PAIRS.sameDay, PAIRS.sameMonth, PAIRS.otherYear, [a, a + 1800000]]) {
      const f = `new Intl.DateTimeFormat(${j(locale)}, ${j(set)})`;
      addProgram(`${f}.formatRange(${from}, ${to})`);
      addProgram(`(parts => ${joinParts})(${f}.formatRangeToParts(${from}, ${to}))`);
    }
  }
  for (const dateStyle of ["full", "long", "medium", "short"]) {
    for (const hourCycle of [undefined, "h12", "h23"]) {
      const set = { dateStyle, timeStyle: "short", ...(hourCycle ? { hourCycle } : {}) };
      for (const to of [b, a + 1800000, a + 7200000]) {
        addProgram(`${dtf(locale, set)}.formatRange(${a}, ${to})`);
        addProgram(`(parts => ${joinParts})(${dtf(locale, set)}.formatRangeToParts(${a}, ${to}))`);
      }
    }
  }
}

// ---- hourCycle e hour12.
for (const locale of LOCALES) {
  for (const hourCycle of ["h11", "h12", "h23", "h24"]) {
    const options = { hour: "numeric", minute: "2-digit", hourCycle };
    addProgram(`${dtf(locale, options)}.format(${SAMPLE})`);
    addProgram(`${dtf(locale, options)}.format(${T(2024, 3, 5, 0, 8, 9)})`);
    addProgram(`${dtf(locale, options)}.formatRange(${PAIRS.sameDay[0]}, ${PAIRS.sameDay[1]})`);
    addProgram(`(o => o.hourCycle + '|' + o.hour12 + '|' + o.hour)(${dtf(locale, options)}.resolvedOptions())`);
  }
}

// ---- calendar e numberingSystem.
const CALENDARS = ["gregory", "buddhist", "japanese", "islamic", "chinese", "hebrew", "persian"];
const NUMBERINGS = ["latn", "arab", "deva", "hanidec"];
for (const locale of LOCALES) {
  for (const calendar of CALENDARS) {
    addProgram(`${dtf(locale, { calendar, dateStyle: "long" })}.format(${SAMPLE_PM})`);
    addProgram(`(parts => ${"parts.map(p => p.type + '=' + p.value).join('|')"})(${dtf(locale, { calendar, era: "short", year: "numeric", month: "numeric", day: "numeric" })}.formatToParts(${SAMPLE_PM}))`);
    addProgram(`(parts => ${"parts.map(p => p.type + '=' + p.value).join('|')"})(${dtf(locale, { calendar, year: "numeric", month: "long", day: "numeric" })}.formatToParts(${SAMPLE_PM}))`);
    addProgram(`${dtf(locale, { calendar, dateStyle: "short" })}.formatRange(${PAIRS.sameYear[0]}, ${PAIRS.sameYear[1]})`);
  }
  for (const numberingSystem of NUMBERINGS) {
    addProgram(`${dtf(locale, { numberingSystem, dateStyle: "medium", timeStyle: "short" })}.format(${SAMPLE})`);
    addProgram(`(o => o.numberingSystem + '|' + o.locale)(${dtf(locale, { numberingSystem })}.resolvedOptions())`);
  }
}
// era, yearName e relatedYear: só aparecem em calendários não gregorianos.
for (const locale of LOCALES.slice(0, 12)) {
  for (const calendar of ["chinese", "japanese", "buddhist", "islamic", "hebrew", "persian"]) {
    for (const key of ["yearName", "relatedYear"]) {
      addProgram(`(parts => parts.map(p => p.type + '=' + p.value).join('|'))(${dtf(locale, { calendar, [key]: "long", year: "numeric", month: "numeric", day: "numeric" })}.formatToParts(${SAMPLE_PM}))`);
    }
  }
}

// ---- Data inválida e mensagens de RangeError.
const INVALID = ["NaN", "Infinity", "undefined", "new Date(NaN)", "8.64e15 + 1"];
for (const locale of LOCALES.slice(0, 12)) {
  const f = dtf(locale, { dateStyle: "short" });
  for (const bad of INVALID) {
    addProgram(`${f}.formatRange(${bad}, 0)`);
    addProgram(`${f}.formatRange(0, ${bad})`);
    addProgram(`${f}.format(${bad})`);
    addProgram(`${f}.formatToParts(${bad}).length`);
  }
  addProgram(`${f}.formatRange(1)`);
  addProgram(`${f}.formatRangeToParts()`);
  addProgram(`new Date(NaN).toLocaleString(${j(locale)}, ${j(withZone({}))})`);
}
addProgram(`new Intl.DateTimeFormat("en", { timeZone: "Nope/Zone" })`);
addProgram(`new Intl.DateTimeFormat("en", { hourCycle: "h99" })`);
addProgram(`new Intl.DateTimeFormat("en", { calendar: "x" })`);
addProgram(`new Intl.DateTimeFormat("en", { dateStyle: "full", year: "numeric" })`);
addProgram(`new Intl.DateTimeFormat("en", { timeStyle: "short", month: "long" })`);
addProgram(`new Intl.DateTimeFormat("en", { fractionalSecondDigits: 4 })`);
addProgram(`new Intl.DateTimeFormat("en", { dayPeriod: "x" })`);
addProgram(`new Intl.DateTimeFormat("e", {})`);

// ---- formatToParts: tipos de parte por conjunto de componentes.
const COMPONENT_SETS = [
  { weekday: "long", year: "numeric", month: "long", day: "numeric" },
  { era: "long", year: "numeric", month: "short", day: "numeric" },
  { hour: "numeric", minute: "numeric", second: "numeric", fractionalSecondDigits: 3 },
  { hour: "numeric", dayPeriod: "long" },
  { hour: "numeric", minute: "2-digit", timeZoneName: "long" },
  { hour: "numeric", minute: "2-digit", timeZoneName: "shortOffset" },
  { year: "2-digit", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" },
  { weekday: "short", month: "narrow", day: "numeric", timeZoneName: "longGeneric" },
];
for (const locale of LOCALES) {
  for (const set of COMPONENT_SETS) {
    addProgram(`(parts => parts.map(p => p.type + '=' + p.value).join('|'))(${dtf(locale, set)}.formatToParts(${SAMPLE_PM}))`);
  }
}

// ---- resolvedOptions por combinação de opções.
const RESOLVED_SETS = [
  {}, { dateStyle: "full" }, { timeStyle: "short" }, { dateStyle: "short", timeStyle: "medium" },
  { hour: "numeric" }, { hour: "2-digit", minute: "2-digit", hour12: false }, { weekday: "long", era: "short" },
  { fractionalSecondDigits: 2, second: "numeric" }, { month: "long", day: "numeric", timeZoneName: "short" },
  { year: "numeric", calendar: "buddhist", numberingSystem: "thai" },
];
for (const locale of LOCALES) {
  for (const set of RESOLVED_SETS) {
    addProgram(`JSON.stringify(${dtf(locale, set)}.resolvedOptions())`);
  }
}

// ---- Date.prototype.toLocaleString, toLocaleDateString e toLocaleTimeString com as mesmas opções.
const LOCALE_SETS = [
  { dateStyle: "medium", timeStyle: "short" },
  { year: "numeric", month: "long", day: "numeric", hour: "numeric", minute: "2-digit", hour12: false },
  { weekday: "short", hour: "numeric", minute: "2-digit", second: "2-digit", timeZoneName: "short" },
];
for (const locale of LOCALES) {
  for (const set of LOCALE_SETS) {
    for (const method of ["toLocaleString", "toLocaleDateString", "toLocaleTimeString"]) {
      const options = method === "toLocaleString" || !set.dateStyle ? set : { year: "numeric", month: "short", day: "numeric" };
      addProgram(`new Date(${SAMPLE_PM}).${method}(${j(locale)}, ${j(withZone(options))})`);
    }
  }
}

// ---- era, timeZoneName e dayPeriod no intervalo: o ICU usa o padrão do intervalo sem o campo e o
// acrescenta uma vez, ou cai no fallback com as duas pontas inteiras (era diferente, data diferente).
const dateAt = (year, month, day) => {
  const d = new Date(0);
  d.setUTCFullYear(year, month - 1, day);
  return d.getTime();
};
const ERA_PAIRS = [[dateAt(-5, 3, 5), dateAt(-3, 3, 5)], [dateAt(-5, 3, 5), dateAt(5, 3, 5)], [dateAt(2020, 3, 5), dateAt(2024, 3, 5)], [dateAt(2024, 3, 5), dateAt(2024, 3, 8)]];
const RANGE_LOCALES = ["en", "pt", "es", "fr", "de", "ja", "zh", "ru", "ko", "ar", "hi", "it"];
for (const locale of RANGE_LOCALES) {
  for (const era of ["short", "long"]) {
    for (const [a, b] of ERA_PAIRS) {
      addProgram(`${dtf(locale, { era, year: "numeric" })}.formatRange(${a}, ${b})`);
      addProgram(`${dtf(locale, { era, year: "numeric", month: "short", day: "numeric" })}.formatRange(${a}, ${b})`);
      addProgram(`(parts => ${joinParts})(${dtf(locale, { era, year: "numeric" })}.formatRangeToParts(${a}, ${b}))`);
    }
  }
  const [t1, t2] = PAIRS.sameDay;
  const nextDay = T(2024, 3, 8, 19, 8, 9);
  for (const timeZoneName of ["short", "long", "shortOffset", "longOffset", "shortGeneric"]) {
    for (const timeZone of ["America/Sao_Paulo", "UTC"]) {
      const options = { timeZone, hour: "numeric", minute: "2-digit", timeZoneName };
      const f = `new Intl.DateTimeFormat(${j(locale)}, ${j(options)})`;
      addProgram(`${f}.formatRange(${t1}, ${t2})`);
      addProgram(`${f}.formatRange(${t1}, ${nextDay})`);
      addProgram(`(parts => ${joinParts})(${f}.formatRangeToParts(${t1}, ${t2}))`);
    }
  }
  for (const dayPeriod of ["short", "long", "narrow"]) {
    const f = dtf(locale, { hour: "numeric", hour12: true, dayPeriod });
    addProgram(`${f}.formatRange(${t1}, ${t2})`);
    addProgram(`${f}.formatRange(${t1}, ${t1 + 3600000})`);
    addProgram(`${f}.formatRange(${t1}, ${T(2024, 3, 8, 19, 8, 9)})`);
    addProgram(`(parts => ${joinParts})(${f}.formatRangeToParts(${t1}, ${t1 + 3600000}))`);
  }
}

// ---- Execução: cada programa roda por eval indireto e a variável R sai como JSON.
const seen = new Set();
let kept = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = '"use strict";\n' + body;
  globalThis.R = undefined;
  (0, eval)(source);
  const result = globalThis.R === undefined ? "<undefined>" : String(globalThis.R);
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}\n`);
process.stdout.write(emitFactored("datetime_range", rows));
