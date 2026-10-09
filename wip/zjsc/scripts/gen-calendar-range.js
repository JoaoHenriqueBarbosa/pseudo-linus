// Gera src/runtime/intl_calendar_range.rs a partir do bun: o que o ICU (DateIntervalFormat) faz em `formatRange`
// com cada calendário não gregoriano e em cada locale. Mede, por (locale, calendário, cenário), os mesmos campos
// que `gen-datetime-data.js` grava em `extras` para o gregoriano (`range|cenário|sep`, `head`, `startlen`,
// `endlen`, `tail`, `collapse`, `hourpad|12`, `hourpad|24`), mas numa tabela densa própria, como
// `intl_calendar_patterns`.
//
// Como regenerar (da raiz da crate zjsc), de preferência em background porque leva alguns minutos:
//   bun scripts/gen-calendar-range.js
//   GEN_CAL_LOCALES=en,pt,he bun scripts/gen-calendar-range.js   (outro conjunto de locales)
//
// Os cenários de data (`same_month`, `same_year`, `other_years` e variantes) pedem a maior diferença NO CALENDÁRIO,
// não no gregoriano: o instante final é procurado, por calendário, de modo que o campo mais alto que difere seja o
// dia, o mês ou o ano do próprio calendário (no hebraico, 9/11/2024 já é outro ano). O porte compara os campos da
// data nativa do mesmo jeito (`largest_difference` em `range.rs`).
const fs = require("fs");
const path = require("path");
const { writeRustSource } = require("./rust-escape.js");

function portLocales() {
  const source = fs.readFileSync(path.join(__dirname, "gen-datetime-data.js"), "utf8");
  const body = /const LOCALES = \[([\s\S]*?)\];/.exec(source)[1];
  return [...new Set(["en", "pt", ...[...body.matchAll(/"([A-Za-z-]+)"/g)].map((match) => match[1])])];
}
const LOCALES = process.env.GEN_CAL_LOCALES ? process.env.GEN_CAL_LOCALES.split(",") : portLocales();
const CALENDARS = [
  "buddhist", "chinese", "coptic", "dangi", "ethiopic", "ethioaa", "hebrew", "indian",
  "islamic-civil", "islamic-tbla", "islamic-umalqura", "japanese", "persian", "roc", "gregory",
];
const FIELDS = ["sep", "collapse", "head", "tail", "startlen", "endlen", "hourpad|12", "hourpad|24"];

const AM = Date.UTC(2024, 2, 5, 7, 8, 9);
const PM = Date.UTC(2024, 2, 5, 19, 8, 9);
const DAY = 86400000;
const make = (locale, calendar, options) =>
  new Intl.DateTimeFormat(`${locale}-u-ca-${calendar}-nu-latn`, { timeZone: "UTC", ...options });

// O maior campo que difere entre dois instantes no calendário, lido do `en` (independe do locale medido).
const levelFormats = new Map();
function nativeLevel(calendar, a, b) {
  if (!levelFormats.has(calendar)) levelFormats.set(calendar, make("en", calendar, { era: "short", year: "numeric", month: "numeric", day: "numeric" }));
  const format = levelFormats.get(calendar);
  const read = (time) => {
    const out = {};
    for (const part of format.formatToParts(time)) out[part.type] = (out[part.type] ?? "") + "|" + part.value;
    return out;
  };
  const [x, y] = [read(a), read(b)];
  const same = (types) => types.every((type) => x[type] === y[type]);
  if (!same(["era"])) return "era";
  if (!same(["year", "relatedYear", "yearName"])) return "year";
  if (!same(["month"])) return "month";
  if (!same(["day"])) return "day";
  return "none";
}

// Os dois instantes de um cenário de data: o início (`AM`, ou um dia depois dele quando o calendário não tem o caso
// a partir de `AM`: o ano indiano vira em 22/3) e o dia perto do deslocamento original (que vale no gregoriano)
// cuja maior diferença no calendário é `want`. Cacheado por calendário e nível.
const toCache = new Map();
function dateTarget(calendar, want, origin) {
  const key = `${calendar}|${want}`;
  if (!toCache.has(key)) {
    let found = null;
    for (let shift = 0; shift < 400 && found === null; shift++) {
      const from = AM + shift * DAY;
      for (let step = 0; step < 800 && found === null; step++) {
        for (const offset of step === 0 ? [origin] : [origin + step, origin - step]) {
          if (offset <= 0) continue;
          const time = Date.UTC(2024, 2, 5 + shift + offset, 12);
          if (nativeLevel(calendar, from, time) === want) {
            found = [from, time];
            break;
          }
        }
      }
    }
    if (found === null) throw new Error(`sem alvo ${want} em ${calendar}`);
    toCache.set(key, found);
  }
  return toCache.get(key);
}

const numeric = { year: "numeric", month: "numeric", day: "numeric" };
const short = { year: "numeric", month: "short", day: "numeric" };
const weekdayShort = { weekday: "short", ...short };
// `to` é um instante, ou `[nível, deslocamento original em dias]` para o alvo por calendário.
const SCENARIOS = [
  { name: "same_day", options: { ...short, hour: "numeric", minute: "2-digit" }, to: PM },
  { name: "same_month", options: short, to: ["day", 4] },
  { name: "other_years", options: short, to: ["year", 400] },
  { name: "time_only", options: { hour: "numeric", minute: "2-digit" }, to: PM },
  { name: "same_month_weekday", options: weekdayShort, to: ["day", 4] },
  { name: "same_year_weekday", options: weekdayShort, to: ["month", 249] },
  { name: "other_years_weekday", options: weekdayShort, to: ["year", 400] },
  { name: "same_year", options: short, to: ["month", 249] },
  { name: "same_month_numeric", options: numeric, to: ["day", 4] },
  { name: "same_year_numeric", options: numeric, to: ["month", 249] },
  { name: "other_years_numeric", options: numeric, to: ["year", 400] },
  { name: "same_period", options: { ...short, hour: "numeric", minute: "2-digit", hourCycle: "h12" }, to: Date.UTC(2024, 2, 5, 9, 38, 9) },
  { name: "same_period_time", options: { hour: "numeric", minute: "2-digit", hourCycle: "h12" }, to: Date.UTC(2024, 2, 5, 9, 38, 9) },
  { name: "same_day_seconds", options: { ...short, hour: "numeric", minute: "2-digit", second: "2-digit" }, to: PM },
  { name: "time_only_seconds", options: { hour: "numeric", minute: "2-digit", second: "2-digit" }, to: PM },
  { name: "days_time", options: { hour: "numeric", minute: "2-digit" }, to: Date.UTC(2024, 2, 25, 19, 8, 9) },
  { name: "days_hour", options: { hour: "numeric" }, to: Date.UTC(2024, 2, 25, 19, 8, 9) },
];

/** Os campos de um cenário: `[campo, texto]` ou nada se o ICU não devolve intervalo. */
function measure(locale, calendar, scenario) {
  const [from, to] = Array.isArray(scenario.to) ? dateTarget(calendar, scenario.to[0], scenario.to[1]) : [AM, scenario.to];
  const out = {};
  let parts;
  try {
    parts = make(locale, calendar, scenario.options).formatRangeToParts(from, to);
  } catch {
    return out;
  }
  const sources = parts.map((part) => part.source);
  const first = sources.indexOf("endRange");
  const last = sources.lastIndexOf("startRange");
  if (first < 0 || last < 0) return out;
  const between = parts.slice(last + 1, first).filter((part) => part.source === "shared");
  out.sep = between.map((part) => part.value).join("");
  out.collapse = sources.some((kind, index) => kind === "shared" && (index < sources.indexOf("startRange") || index > sources.lastIndexOf("endRange"))) ? "1" : "0";
  out.head = String(sources.indexOf("startRange"));
  out.tail = String(sources.length - 1 - sources.lastIndexOf("endRange"));
  out.startlen = String(sources.filter((kind) => kind === "startRange").length);
  out.endlen = String(sources.filter((kind) => kind === "endRange").length);
  if (scenario.options.hour) {
    for (const [cycle, label] of [["h12", "12"], ["h23", "24"]]) {
      const cycled = make(locale, calendar, { ...scenario.options, hourCycle: cycle }).formatRangeToParts(from, to);
      const startHour = cycled.find((part) => part.type === "hour" && part.source === "startRange");
      if (startHour) out[`hourpad|${label}`] = [...startHour.value].length >= 2 ? "1" : "0";
    }
  }
  return out;
}

const NONE = 0xffff;
const pool = [];
const poolIndex = new Map();
const indexOf = (text) => {
  if (!poolIndex.has(text)) {
    poolIndex.set(text, pool.length);
    pool.push(text);
  }
  return poolIndex.get(text);
};
const SLOTS = SCENARIOS.length * FIELDS.length;
const blockIndex = new Map();
const blocks = [];
const blockOf = [];
for (const locale of LOCALES) {
  for (const calendar of CALENDARS) {
    const block = [];
    for (const scenario of SCENARIOS) {
      const values = measure(locale, calendar, scenario);
      for (const field of FIELDS) block.push(values[field] === undefined ? NONE : indexOf(values[field]));
    }
    const text = block.join(",");
    if (!blockIndex.has(text)) {
      blockIndex.set(text, blocks.length);
      blocks.push(block);
    }
    blockOf.push(blockIndex.get(text));
  }
  process.stderr.write(`${locale} `);
}
if (pool.length >= NONE || blocks.length >= NONE) throw new Error("não cabe em u16");
const quote = (text) => JSON.stringify(text);
const list = (values) => `[${values.map(quote).join(", ")}]`;
const rows = (values) => {
  const lines = [];
  for (let at = 0; at < values.length; at += 24) lines.push(`    ${values.slice(at, at + 24).join(", ")},`);
  return lines.join("\n");
};

const source = `//! O que o \`DateIntervalFormat\` do ICU faz em \`Intl.DateTimeFormat.prototype.formatRange\` nos calendários não
//! gregorianos, por locale e cenário.
//!
//! GERADO por \`scripts/gen-calendar-range.js\`: não edite à mão. Para regenerar, da raiz da crate:
//! \`bun scripts/gen-calendar-range.js\`. Os campos são os de \`range|cenário|campo\` de \`intl_date_time_data\` (\`sep\`,
//! \`collapse\`, \`head\`, \`tail\`, \`startlen\`, \`endlen\`, \`hourpad|12\`, \`hourpad|24\`), medidos por calendário porque o
//! padrão e o separador de intervalo mudam com ele (\`3/5/2567 \\u{2013} 11/25/2567 BE\` no budista).
//!
//! Formato denso: \`VALUES\` guarda cada texto uma vez, \`TABLE\` os blocos distintos de \`SLOTS\` índices \`u16\` (\`NONE\` =
//! não medido; o bloco tem \`SCENARIOS\` x \`FIELDS\` entradas) e \`BLOCK_OF\` o bloco de cada (locale, calendário).

/// Textos distintos; \`TABLE\` guarda o índice.
static VALUES: [&str; ${pool.length}] = [
${pool.map((text) => `    ${quote(text)},`).join("\n")}
];

/// Entrada de \`TABLE\` sem medida.
const NONE: u16 = ${NONE};

/// Os locales de que a tabela tem blocos.
pub const LOCALES: [&str; ${LOCALES.length}] = ${list(LOCALES)};

/// Os calendários da tabela (o \`resolvedOptions().calendar\`).
static CALENDARS: [&str; ${CALENDARS.length}] = ${list(CALENDARS)};

/// Os cenários de intervalo (os nomes de \`range|cenário|campo\`).
static SCENARIOS: [&str; ${SCENARIOS.length}] = ${list(SCENARIOS.map((scenario) => scenario.name))};

/// Os campos medidos por cenário.
static FIELDS: [&str; ${FIELDS.length}] = ${list(FIELDS)};

const SLOTS: usize = ${SLOTS};

/// Para cada (locale, calendário), na ordem de \`LOCALES\` x \`CALENDARS\`, o índice do bloco em \`TABLE\`.
static BLOCK_OF: [u16; ${blockOf.length}] = [
${rows(blockOf)}
];

/// Os ${blocks.length} blocos distintos de \`SLOTS\` índices cada, um depois do outro.
static TABLE: [u16; ${blocks.length * SLOTS}] = [
${rows(blocks.flat())}
];

fn position(list: &[&str], value: &str) -> Option<usize> {
    list.iter().position(|item| *item == value)
}

/// O campo \`field\` do cenário \`scenario\` no calendário \`calendar\` do locale exato \`locale\` (\`en\`, \`pt\`, \`he\`...).
pub fn value(locale: &str, calendar: &str, scenario: &str, field: &str) -> Option<&'static str> {
    let entry = position(&LOCALES, locale)? * CALENDARS.len() + position(&CALENDARS, calendar)?;
    let slot = position(&SCENARIOS, scenario)? * FIELDS.len() + position(&FIELDS, field)?;
    match TABLE[usize::from(BLOCK_OF[entry]) * SLOTS + slot] {
        NONE => None,
        index => Some(VALUES[usize::from(index)]),
    }
}
`;
writeRustSource(path.join(__dirname, "../src/runtime/intl_calendar_range.rs"), source);
console.log(`\n${pool.length} textos, ${blockOf.length} blocos de locale x calendário, ${blocks.length} únicos, ${SLOTS} slots por bloco`);
