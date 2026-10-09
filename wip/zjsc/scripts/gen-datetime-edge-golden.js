// Gera tests/golden/datetime_edge_bun.tsv: borda de Intl.DateTimeFormat medida no bun (JavaScriptCore + ICU).
// Cobre hourCycle contra hour12 conflitante, fractionalSecondDigits, timeZoneName em fusos de horário de verão e de
// meia hora, nove calendários com formatToParts, numberingSystem, dayPeriod, eras a.C., datas extremas,
// resolvedOptions por locale, supportedLocalesOf e a matriz de erros de opção inválida (mensagens do RangeError).
// Cada linha: a fonte (uma expressão que devolve string, com a tag da seção em comentário) e o resultado medido.
// Os dados do ICU trazem en dash e espaço fino (U+202F): são dados medidos, não texto nosso.
// Programas que já existem em qualquer outro golden de Intl/Date são descartados.
// Uso: bun scripts/gen-datetime-edge-golden.js
process.env.TZ = "UTC";
const fs = require("fs");
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");

const out = [];
const q = s => JSON.stringify(s);
const wrap = (tag, expr) =>
  `/*${tag}*/(()=>{try{return String(${expr})}catch(e){return "throw "+e.name+": "+e.message}})()`;
const emit = (tag, expr) => out.push(wrap(tag, expr));
const dtf = (loc, opts) => `new Intl.DateTimeFormat(${loc === null ? "undefined" : q(loc)}, ${opts})`;

const WINTER = 1705322096789; // 2024-01-15T12:34:56.789Z
const SUMMER = 1721046896789; // 2024-07-15T12:34:56.789Z
const MARCH = 1710025685678; // 2024-03-09T23:08:05.678Z

// ---- 1. hourCycle contra hour12.
const hours = [Date.UTC(2024, 2, 9, 0, 5, 7), Date.UTC(2024, 2, 9, 12, 5, 7), Date.UTC(2024, 2, 9, 23, 59, 59)];
for (const loc of ["en-US", "ja-JP", "de-DE"]) {
  for (const hc of ["h11", "h12", "h23", "h24"]) {
    for (const h12 of ["", ", hour12: true", ", hour12: false"]) {
      for (const t of hours) {
        emit("hc", `${dtf(loc, `{ hour: "numeric", minute: "2-digit", hourCycle: ${q(hc)}${h12}, timeZone: "UTC" }`)}.format(${t})`);
      }
    }
  }
}
for (const loc of ["en-US-u-hc-h23", "en-US-u-hc-h11", "en-US-u-hc-h24", "ja-JP-u-hc-h12"]) {
  for (const h12 of ["", ", hour12: true", ", hour12: false", ", hourCycle: \"h24\""]) {
    for (const t of hours.slice(0, 2)) {
      emit("hc", `${dtf(loc, `{ hour: "numeric", minute: "2-digit"${h12}, timeZone: "UTC" }`)}.format(${t})`);
    }
  }
}
for (const hc of ["h11", "h12", "h23", "h24"]) {
  for (const h12 of ["", ", hour12: true", ", hour12: false"]) {
    emit("hc", `JSON.stringify(${dtf("en-US", `{ hour: "numeric", hourCycle: ${q(hc)}${h12} }`)}.resolvedOptions())`);
    emit("hc", `JSON.stringify(${dtf("ja-JP", `{ hour: "numeric", hourCycle: ${q(hc)}${h12} }`)}.resolvedOptions().hourCycle)`);
  }
}

// ---- 2. fractionalSecondDigits.
for (const n of [1, 2, 3]) {
  for (const t of [MARCH, 1710025685000, 1710025685009, 1710025685999]) {
    emit("frac", `${dtf("en-US", `{ minute: "numeric", second: "numeric", fractionalSecondDigits: ${n}, timeZone: "UTC" }`)}.format(${t})`);
  }
  emit("frac", `JSON.stringify(${dtf("en-US", `{ hour: "numeric", minute: "numeric", second: "numeric", fractionalSecondDigits: ${n}, timeZone: "UTC" }`)}.formatToParts(${MARCH}))`);
  emit("frac", `${dtf("de-DE", `{ second: "numeric", fractionalSecondDigits: ${n}, timeZone: "UTC" }`)}.format(${MARCH})`);
  emit("frac", `${dtf("ar-EG", `{ second: "numeric", fractionalSecondDigits: ${n}, timeZone: "UTC" }`)}.format(${MARCH})`);
  emit("frac", `${dtf("en-US", `{ fractionalSecondDigits: ${n}, timeZone: "UTC" }`)}.format(${MARCH})`);
  emit("frac", `JSON.stringify(${dtf("en-US", `{ fractionalSecondDigits: ${n} }`)}.resolvedOptions())`);
}
for (const v of [0, 4, -1, 1.5, "2", "x", null, undefined, NaN]) {
  emit("frac", `${dtf("en-US", `{ second: "numeric", fractionalSecondDigits: ${v === undefined ? "undefined" : v === null ? "null" : Number.isNaN(v) ? "NaN" : q(v)} }`)}.format(${MARCH})`);
}

// ---- 3. timeZoneName em fusos de horário de verão e de meia hora.
const zones = ["Asia/Kolkata", "Asia/Kathmandu", "Australia/Adelaide", "America/St_Johns", "Europe/London", "UTC"];
const names = ["short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"];
for (const loc of ["en-US", "de-DE"]) {
  for (const z of zones) {
    for (const n of names) {
      for (const t of [WINTER, SUMMER]) {
        emit("tzname", `${dtf(loc, `{ hour: "numeric", minute: "numeric", timeZone: ${q(z)}, timeZoneName: ${q(n)} }`)}.format(${t})`);
      }
    }
  }
}
for (const z of ["Asia/Kolkata", "Australia/Adelaide", "America/St_Johns"]) {
  for (const n of ["shortOffset", "longOffset", "long"]) {
    emit("tzname", `JSON.stringify(${dtf("en-US", `{ year: "numeric", timeZone: ${q(z)}, timeZoneName: ${q(n)} }`)}.formatToParts(${SUMMER}))`);
    emit("tzname", `${dtf("ja-JP", `{ dateStyle: "short", timeStyle: "long", timeZone: ${q(z)}, timeZoneName: ${q(n)} }`)}.format(${SUMMER})`);
  }
  emit("tzname", `${dtf("en-US", `{ dateStyle: "full", timeStyle: "full", timeZone: ${q(z)} }`)}.format(${WINTER})`);
}

// ---- 4. Calendários com formatToParts.
const cals = ["buddhist", "japanese", "roc", "islamic-umalqura", "hebrew", "chinese", "persian", "coptic", "ethiopic"];
for (const c of cals) {
  for (const t of [MARCH, -2208988800000]) {
    for (const loc of ["en-US", "fr-FR"]) {
      emit("cal", `JSON.stringify(${dtf(`${loc}-u-ca-${c}`, `{ dateStyle: "full", timeZone: "UTC" }`)}.formatToParts(${t}))`);
    }
    emit("cal", `${dtf("en-US", `{ calendar: ${q(c)}, year: "numeric", month: "numeric", day: "numeric", era: "short", timeZone: "UTC" }`)}.format(${t})`);
    emit("cal", `${dtf("en-US", `{ calendar: ${q(c)}, year: "numeric", month: "long", timeZone: "UTC" }`)}.format(${t})`);
  }
  emit("cal", `${dtf(`en-US-u-ca-${c}`, `{}`)}.resolvedOptions().calendar`);
  emit("cal", `${dtf("en-US", `{ calendar: ${q(c)}, era: "long", year: "numeric", timeZone: "UTC" }`)}.format(${MARCH})`);
  emit("cal", `JSON.stringify(${dtf("ar-SA", `{ calendar: ${q(c)}, dateStyle: "medium", timeZone: "UTC" }`)}.formatToParts(${MARCH}))`);
}
emit("cal", `${dtf("en-US-u-ca-islamic-umalqura", `{ calendar: "hebrew" }`)}.resolvedOptions().calendar`);
emit("cal", `${dtf("th-TH", `{}`)}.resolvedOptions().calendar`);
emit("cal", `${dtf("ja-JP-u-ca-japanese", `{ dateStyle: "long", timeZone: "UTC" }`)}.format(${MARCH})`);
emit("cal", `${dtf("ja-JP-u-ca-japanese", `{ dateStyle: "long", timeZone: "UTC" }`)}.format(-1500000000000)`);

// ---- 5. numberingSystem.
for (const nu of ["arab", "deva", "thai"]) {
  for (const loc of ["en-US", "fr-FR"]) {
    emit("nu", `${dtf(`${loc}-u-nu-${nu}`, `{ dateStyle: "short", timeStyle: "medium", timeZone: "UTC" }`)}.format(${MARCH})`);
    emit("nu", `${dtf(loc, `{ numberingSystem: ${q(nu)}, year: "numeric", month: "long", day: "numeric", timeZone: "UTC" }`)}.format(${MARCH})`);
    emit("nu", `JSON.stringify(${dtf(loc, `{ numberingSystem: ${q(nu)}, hour: "numeric", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 2, timeZone: "UTC" }`)}.formatToParts(${MARCH}))`);
    emit("nu", `${dtf(loc, `{ numberingSystem: ${q(nu)} }`)}.resolvedOptions().numberingSystem`);
  }
  emit("nu", `${dtf(`ar-EG-u-nu-latn`, `{ numberingSystem: ${q(nu)}, dateStyle: "short", timeZone: "UTC" }`)}.format(${MARCH})`);
  emit("nu", `${dtf(`en-US-u-nu-${nu}`, `{ numberingSystem: "latn", dateStyle: "short", timeZone: "UTC" }`)}.format(${MARCH})`);
  emit("nu", `${dtf(`en-US-u-nu-${nu}`, `{}`)}.resolvedOptions().locale`);
}
emit("nu", `${dtf("th-TH-u-nu-thai", `{ dateStyle: "long", timeZone: "UTC" }`)}.format(${MARCH})`);
emit("nu", `${dtf("hi-IN-u-nu-deva", `{ dateStyle: "full", timeZone: "UTC" }`)}.format(${MARCH})`);

// ---- 6. dayPeriod.
for (const dp of ["narrow", "short", "long"]) {
  for (const h of [0, 3, 6, 9, 12, 13, 15, 18, 21]) {
    const t = Date.UTC(2024, 2, 9, h, 30);
    emit("dp", `${dtf("en-US", `{ hour: "numeric", dayPeriod: ${q(dp)}, timeZone: "UTC" }`)}.format(${t})`);
    emit("dp", `${dtf("fr-FR", `{ hour: "numeric", minute: "numeric", dayPeriod: ${q(dp)}, hourCycle: "h12", timeZone: "UTC" }`)}.format(${t})`);
  }
  emit("dp", `JSON.stringify(${dtf("en-US", `{ hour: "numeric", dayPeriod: ${q(dp)}, timeZone: "UTC" }`)}.formatToParts(${Date.UTC(2024, 2, 9, 15)}))`);
  emit("dp", `${dtf("zh-CN", `{ hour: "numeric", dayPeriod: ${q(dp)}, timeZone: "UTC" }`)}.format(${Date.UTC(2024, 2, 9, 5)})`);
  emit("dp", `${dtf("en-US", `{ hour: "numeric", hour12: false, dayPeriod: ${q(dp)}, timeZone: "UTC" }`)}.format(${Date.UTC(2024, 2, 9, 5)})`);
}

// ---- 7. Eras a.C.
const bc = [-62198755200000, -62167219200000, -62135596800000, -62135596800001, -86400000 * 365.25 * 100 - 62135596800000, -30610224000000, -8.64e15];
for (const t of bc) {
  for (const era of ["narrow", "short", "long"]) {
    emit("bc", `${dtf("en-US", `{ era: ${q(era)}, year: "numeric", month: "short", day: "numeric", timeZone: "UTC" }`)}.format(${t})`);
  }
  emit("bc", `JSON.stringify(${dtf("en-US", `{ era: "short", year: "2-digit", month: "numeric", day: "numeric", timeZone: "UTC" }`)}.formatToParts(${t}))`);
  emit("bc", `${dtf("pt-BR", `{ era: "long", year: "numeric", timeZone: "UTC" }`)}.format(${t})`);
  emit("bc", `${dtf("en-US", `{ dateStyle: "full", timeZone: "UTC" }`)}.format(${t})`);
  emit("bc", `${dtf("ja-JP-u-ca-japanese", `{ era: "long", year: "numeric", timeZone: "UTC" }`)}.format(${t})`);
}

// ---- 8. Datas extremas.
for (const t of [8.64e15, -8.64e15, 8.64e15 + 1, -8.64e15 - 1, 0, -1, 253402300799999, 253402300800000, 1e14, -1e14, NaN, Infinity]) {
  const lit = Number.isNaN(t) ? "NaN" : String(t);
  emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.format(${lit})`);
  emit("ext", `${dtf("en-US", `{ dateStyle: "full", timeStyle: "full", timeZone: "UTC" }`)}.format(${lit})`);
  emit("ext", `${dtf("en-US", `{ timeZone: "Asia/Kolkata", timeStyle: "long" }`)}.format(${lit})`);
  emit("ext", `JSON.stringify(${dtf("en-US", `{ timeZone: "UTC", year: "numeric", month: "numeric", day: "numeric" }`)}.formatToParts(${lit}))`);
}
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.formatRange(0, 8.64e15)`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.formatRange(8.64e15, 0)`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.formatRange(NaN, 0)`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.formatRange()`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.formatRange(1)`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.format(new Date(NaN))`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.format("x")`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.format(null)`);
emit("ext", `${dtf("en-US", `{ timeZone: "UTC" }`)}.format(true)`);

// ---- 9. resolvedOptions por locale.
const locales = ["en-US", "en-GB", "de-DE", "fr-FR", "ja-JP", "ar-EG", "ar-SA", "th-TH", "hi-IN", "fa-IR", "he-IL", "zh-TW", "ko-KR", "pt-BR", "ru-RU", "es-MX"];
for (const l of locales) {
  emit("resolved", `JSON.stringify(${dtf(l, `{ timeZone: "UTC" }`)}.resolvedOptions())`);
  emit("resolved", `JSON.stringify(${dtf(l, `{ hour: "numeric", timeZone: "UTC" }`)}.resolvedOptions())`);
  emit("resolved", `JSON.stringify(${dtf(l, `{ dateStyle: "short", timeStyle: "short", timeZone: "UTC" }`)}.resolvedOptions())`);
}
emit("resolved", `JSON.stringify(${dtf("en-US-u-ca-hebrew-nu-arab-hc-h23", `{ timeZone: "UTC", hour: "numeric" }`)}.resolvedOptions())`);
emit("resolved", `JSON.stringify(${dtf("en-US-u-tz-usnyc", `{}`)}.resolvedOptions().timeZone === undefined)`);

// ---- 10. supportedLocalesOf.
const lists = [`"en-US"`, `["en-US","xx"]`, `["xx","yy"]`, `["en-US-u-ca-hebrew","de-u-nu-arab"]`, `["EN-us","DE-de"]`, `[]`, `"pt-PT"`,
  `["zh-Hant-TW","zh-Hans-CN"]`, `["tlh","en"]`, `["ja-JP-u-ca-japanese","ar-u-nu-latn"]`, `"sr-Latn"`, `["und"]`, `["i-klingon"]`, `["en","en"]`];
for (const l of lists) {
  emit("supported", `JSON.stringify(Intl.DateTimeFormat.supportedLocalesOf(${l}))`);
  emit("supported", `JSON.stringify(Intl.DateTimeFormat.supportedLocalesOf(${l}, { localeMatcher: "best fit" }))`);
}
for (const l of [`"en_US"`, `"en-"`, `"x"`, `[1]`, `[null]`, `""`, `"en-US-u-"`, `{ length: 1, 0: "en" }`]) {
  emit("supported", `JSON.stringify(Intl.DateTimeFormat.supportedLocalesOf(${l}))`);
}
emit("supported", `Intl.DateTimeFormat.supportedLocalesOf("en", { localeMatcher: "x" })`);
emit("supported", `Intl.DateTimeFormat.supportedLocalesOf.length + ":" + Intl.DateTimeFormat.supportedLocalesOf.name`);

// ---- 11. Opções inválidas (mensagens de erro medidas).
const bad = [
  `{ timeZone: "Foo/Bar" }`, `{ timeZone: "" }`, `{ timeZone: "UTC+3" }`, `{ timeZone: "+25:00" }`, `{ timeZone: "+05:30" }`, `{ timeZone: "-0330" }`,
  `{ timeZone: "GMT" }`, `{ timeZone: "asia/kolkata" }`, `{ timeZone: 5 }`, `{ timeZone: null }`, `{ timeZone: Symbol("x") }`,
  `{ hourCycle: "h13" }`, `{ hourCycle: "H12" }`, `{ hourCycle: "" }`, `{ hourCycle: null }`,
  `{ calendar: "foo" }`, `{ calendar: "gregorian" }`, `{ calendar: "ISO8601" }`, `{ calendar: "x" }`, `{ calendar: "islamicc" }`, `{ calendar: "a_b" }`,
  `{ numberingSystem: "xyz" }`, `{ numberingSystem: "ARAB" }`, `{ numberingSystem: "" }`, `{ numberingSystem: "toolongvalue" }`,
  `{ dateStyle: "x" }`, `{ timeStyle: "x" }`, `{ dateStyle: null }`, `{ dateStyle: "full", weekday: "long" }`, `{ timeStyle: "short", hour: "numeric" }`, `{ dateStyle: "short", timeZoneName: "short" }`,
  `{ timeZoneName: "x" }`, `{ timeZoneName: "Short" }`, `{ era: "x" }`, `{ weekday: "x" }`, `{ year: "x" }`, `{ month: "x" }`, `{ day: "x" }`, `{ hour: "x" }`,
  `{ minute: "x" }`, `{ second: "x" }`, `{ dayPeriod: "x" }`, `{ formatMatcher: "x" }`, `{ localeMatcher: "x" }`, `{ hour12: Symbol("x") }`, `{ minute: "numeric", hour: "2-digit", second: "narrow" }`,
  `{ hour: "narrow" }`, `{ year: "narrow" }`, `{ day: "long" }`, `{ minute: "long" }`, `{ second: "short" }`,
];
for (const o of bad) {
  emit("err", `${dtf("en-US", o)}.resolvedOptions().locale`);
  emit("err", `JSON.stringify(${dtf("en-US", o)}.formatToParts(${MARCH}).map(p=>p.type))`);
}
for (const l of [`"en_US"`, `"xx-"`, `"en-US-u-ca"`, `"x"`, `"en-u-u-ca-hebrew"`, `[1]`, `[null]`, `{}`, `"en-US-u-ca-hebrew-ca-roc"`, `"en-a-b"`, `"toolonglanguage"`, `""`, `"en-US-x-private"`, `"i-default"`]) {
  emit("err", `new Intl.DateTimeFormat(${l}).resolvedOptions().locale`);
}
emit("err", `Intl.DateTimeFormat.prototype.format.call({}, 0)`);
emit("err", `Intl.DateTimeFormat.prototype.formatToParts.call({}, 0)`);
emit("err", `Object.getOwnPropertyDescriptor(Intl.DateTimeFormat.prototype, "format").get.call({})`);
emit("err", `Intl.DateTimeFormat.prototype.resolvedOptions.call(1)`);
emit("err", `new Intl.DateTimeFormat("en", { timeZone: "UTC" }).formatRangeToParts()`);
emit("err", `new Intl.DateTimeFormat("en", { timeZone: "UTC" }).formatRange(1, undefined)`);
emit("err", `Intl.DateTimeFormat.call(undefined, "en") instanceof Intl.DateTimeFormat`);
emit("err", `new Intl.DateTimeFormat("en", { timeZone: "UTC", hour12: "yes", hour: "numeric" }).format(${MARCH})`);

// ---- Execução: descarta o que já existe em outro golden e o que o bun não respondeu como string.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
const existing = new Set();
for (const program of knownPrograms("datetime_edge_bun.tsv", (f) => f !== "datetime_edge_bun.tsv")) existing.add(JSON.stringify(program));
const lines = [];
const seen = new Set();
let dup = 0;
for (const source of out) {
  if (seen.has(source) || existing.has(source)) { dup++; continue; }
  seen.add(source);
  const result = (0, eval)(source);
  if (typeof result !== "string" || /[\t\n\r]/.test(result)) { process.stderr.write("descartado: " + source.slice(0, 120) + "\n"); continue; }
  if (/\/home\/|\/tmp\/|\.js:\d/.test(result)) { process.stderr.write("caminho: " + source.slice(0, 120) + "\n"); continue; }
  lines.push(source + "\t" + result);
}
fs.writeFileSync(path.join(goldenDir, "datetime_edge_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${lines.length} linhas em tests/golden/datetime_edge_bun.tsv (repetidas ${dup})\n`);
