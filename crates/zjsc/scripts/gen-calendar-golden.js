// Gera os dados de calendário do Intl.DateTimeFormat, medidos no bun (JavaScriptCore + ICU).
// Uso:
//   bun scripts/gen-calendar-golden.js golden > tests/golden/calendar_bun.tsv
//   bun scripts/gen-calendar-golden.js names  > src/runtime/intl_calendar_names.rs
// `golden`: colunas TAG, EPOCH_MS, OPÇÕES, LOCALE (resolvedOptions), CALENDAR, NUMBERING_SYSTEM e as partes de
// componente (era, year, relatedYear, yearName, month, day) como `tipo=valor` separados por `|`.
// `names`: nomes de mês (por número do mês no `en`), era (por código de era do icu4x) e yearName (por ano do ciclo)
// de cada calendário não gregoriano, por língua.
// As línguas primárias dos locales do porte (`LOCALES` de `gen-datetime-data.js`, mais `en` e `pt`): os nomes de mês
// de `pt` em calendário não gregoriano não podem cair no inglês.
const LANGS = (() => {
  const source = require("fs").readFileSync(require("path").join(__dirname, "gen-datetime-data.js"), "utf8");
  const body = /const LOCALES = \[([\s\S]*?)\];/.exec(source)[1];
  const tags = ["en", "pt", ...[...body.matchAll(/"([A-Za-z-]+)"/g)].map((match) => match[1])];
  return [...new Set(tags.map((tag) => tag.split("-")[0]))];
})();
const CALENDARS = [
  "buddhist", "chinese", "coptic", "dangi", "ethiopic", "ethioaa", "hebrew", "indian", "islamic", "islamic-civil",
  "islamic-tbla", "islamic-umalqura", "islamic-rgsa", "japanese", "persian", "roc",
];
const COMPONENT = new Set(["era", "year", "relatedYear", "yearName", "month", "day"]);
const DATES = [
  Date.UTC(2024, 2, 15), Date.UTC(2023, 4, 15), Date.UTC(2023, 3, 1), Date.UTC(2024, 1, 15), Date.UTC(2019, 4, 2),
  Date.UTC(1989, 0, 7), Date.UTC(1990, 0, 1), Date.UTC(2000, 8, 30),
];
const OPTIONS = {
  A: { era: "short", year: "numeric", month: "long", day: "numeric" },
  B: { year: "numeric", month: "numeric", day: "numeric" },
};

const tsvQuote = (text) => text.replace(/[\t\n\\]/g, (c) => ({ "\t": "\\t", "\n": "\\n", "\\": "\\\\" })[c]);
const rustStr = (text) => {
  let out = '"';
  for (const ch of text) {
    const cp = ch.codePointAt(0);
    if (ch === '"' || ch === "\\") out += "\\" + ch;
    else if (cp >= 0x20 && cp < 0x7f) out += ch;
    else out += "\\u{" + cp.toString(16) + "}";
  }
  return out + '"';
};
const make = (tag, options) => new Intl.DateTimeFormat(tag, { ...options, timeZone: "UTC" });

function golden() {
  const rows = [];
  const emit = (tag, epoch, optionId) => {
    const format = make(tag, OPTIONS[optionId]);
    const resolved = format.resolvedOptions();
    const parts = format
      .formatToParts(epoch)
      .filter((part) => COMPONENT.has(part.type))
      .map((part) => `${part.type}=${part.value}`)
      .join("|");
    rows.push([tag, epoch, optionId, resolved.locale, resolved.calendar, resolved.numberingSystem, parts].map((c) => tsvQuote(String(c))).join("\t"));
  };
  for (const language of LANGS) {
    for (const calendar of CALENDARS) {
      for (const epoch of DATES) emit(`${language}-u-ca-${calendar}`, epoch, "A");
      emit(`${language}-u-ca-${calendar}`, DATES[0], "B");
    }
  }
  for (const numbering of ["thai", "arab", "deva", "hanidec", "arabext", "beng", "latn"]) {
    for (const language of ["en", "ja", "ar"]) {
      for (const epoch of DATES.slice(0, 2)) emit(`${language}-u-nu-${numbering}`, epoch, "B");
    }
  }
  emit("ja-u-ca-japanese-nu-hanidec", DATES[0], "B");
  console.log(rows.join("\n"));
}

// Códigos de era do icu4x e uma data que cai em cada uma.
const ERA_PROBES = {
  buddhist: [["be", [2000, 0, 1]]],
  roc: [["roc", [2000, 0, 1]], ["broc", [1900, 0, 1]]],
  coptic: [["am", [2000, 0, 1]]],
  ethiopic: [["am", [2000, 0, 1]]],
  ethioaa: [["aa", [2000, 0, 1]]],
  hebrew: [["am", [2000, 0, 1]]],
  indian: [["shaka", [2000, 0, 1]]],
  persian: [["ap", [2000, 0, 1]]],
  islamic: [["ah", [2000, 0, 1]]],
  "islamic-civil": [["ah", [2000, 0, 1]]],
  "islamic-tbla": [["ah", [2000, 0, 1]]],
  "islamic-umalqura": [["ah", [2000, 0, 1]]],
  "islamic-rgsa": [["ah", [2000, 0, 1]]],
  japanese: [
    ["reiwa", [2020, 0, 1]], ["heisei", [2000, 0, 1]], ["showa", [1980, 0, 1]], ["taisho", [1920, 0, 1]],
    ["meiji", [1900, 0, 1]],
  ],
};

function names() {
  const entries = [];
  const seen = new Set();
  const add = (language, calendar, kind, key, text) => {
    const id = [language, calendar, kind, key].join("\u0001");
    if (seen.has(id) || text === undefined || text === "") return;
    seen.add(id);
    entries.push([[language, calendar, kind, key], `    (${rustStr(language)}, ${rustStr(calendar)}, ${rustStr(kind)}, ${rustStr(key)}, ${rustStr(text)}),`]);
  };
  const part = (format, epoch, type) => format.formatToParts(epoch).find((p) => p.type === type)?.value;
  const start = Date.UTC(1995, 0, 1);
  const end = Date.UTC(2026, 0, 1);
  for (const calendar of CALENDARS) {
    const numeric = make(`en-u-ca-${calendar}`, { month: "numeric" });
    for (const language of LANGS) {
      const tag = `${language}-u-ca-${calendar}`;
      for (const [width, kind] of [["long", "ml"], ["short", "ms"], ["narrow", "mn"]]) {
        const monthFormat = make(tag, { month: width, day: "numeric" });
        const yearMonthFormat = make(tag, { month: width, day: "numeric", year: "numeric" });
        for (let epoch = start; epoch < end; epoch += 7 * 86400000) {
          let key = numeric.format(epoch);
          if (calendar === "hebrew") {
            // O número é a posição no ano: Adar (comum), Adar I e Adar II (bissexto) colidem com Nisan; o ano
            // bissexto leva `L` em todos os meses.
            const hebrewYear = Number(part(make("en-u-ca-hebrew", { year: "numeric" }), epoch, "year"));
            if ([0, 3, 6, 8, 11, 14, 17].includes(hebrewYear % 19)) key += "L";
            /* fim do ajuste do hebraico */
          }
          const text = part(monthFormat, epoch, "month");
          add(language, calendar, kind, key, text);
          // A forma do mês junto do ano (`d MMMM y`) pode diferir da do mês com dia (`fa`: `مهٔ` e `مه`): só entra
          // quando difere, no tipo `m?y`.
          const withYear = part(yearMonthFormat, epoch, "month");
          if (withYear !== text && !/^\d/.test(withYear)) add(language, calendar, `${kind}y`, key, withYear);
        }
      }
      for (const [width, kind] of [["long", "el"], ["short", "es"], ["narrow", "en"]]) {
        const eraFormat = make(tag, { era: width, year: "numeric" });
        for (const [code, [year, month, day]] of ERA_PROBES[calendar] ?? []) {
          add(language, calendar, kind, code, part(eraFormat, Date.UTC(year, month, day), "era"));
        }
      }
      if (calendar === "chinese" || calendar === "dangi") {
        const yearFormat = make(tag, { era: "short", year: "numeric", month: "long", day: "numeric" });
        for (let cycle = 1; cycle <= 60; cycle++) {
          add(language, calendar, "yn", String(cycle), part(yearFormat, Date.UTC(1983 + cycle, 5, 15), "yearName"));
        }
      }
    }
  }
  const compare = (a, b) => {
    for (let i = 0; i < 4; i++) if (a[0][i] !== b[0][i]) return a[0][i] < b[0][i] ? -1 : 1;
    return 0;
  };
  const sortedEntries = entries.sort(compare).map((entry) => entry[1]);
  console.log(`//! Nomes de mês, era e ano cíclico por calendário e língua, medidos no bun (ICU).
//! GERADO por scripts/gen-calendar-golden.js (\`names\`): não editar à mão.
//! Colunas: língua, calendário, tipo, chave, texto. Tipos: \`ml\`/\`ms\`/\`mn\` mês longo, curto e estreito (chave: o mês
//! numérico do \`en\`, \`3bis\` para o bissexto; \`mly\`/\`msy\`/\`mny\` a forma junto do ano, quando difere); \`el\`/\`es\`/\`en\` era (chave: o código de era do icu4x); \`yn\` nome do
//! ano do ciclo chinês (chave: 1 a 60).

pub static ENTRIES: &[(&str, &str, &str, &str, &str)] = &[
${sortedEntries.join("\n")}
];

/// O texto medido para (língua, calendário, tipo, chave); \`ENTRIES\` está ordenada por essa quádrupla (busca binária).
pub fn lookup(language: &str, calendar: &str, kind: &str, key: &str) -> Option<&'static str> {
    ENTRIES
        .binary_search_by(|entry| (entry.0, entry.1, entry.2, entry.3).cmp(&(language, calendar, kind, key)))
        .ok()
        .map(|index| ENTRIES[index].4)
}`);
}

const mode = process.argv[2];
if (mode === "golden") golden();
else if (mode === "names") names();
else {
  console.error("uso: bun scripts/gen-calendar-golden.js golden|names");
  process.exit(1);
}
