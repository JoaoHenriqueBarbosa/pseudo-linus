// Gera os dados do Intl.DurationFormat das 38 locales medindo o bun (JavaScriptCore + ICU):
//   src/runtime/intl_duration_format_data.rs  (os padrões de cada unidade e os separadores do estilo digital)
//   tests/golden/duration_format_bun.tsv      (o golden de tests/duration_format_bun_golden.rs)
// Padrão de unidade: o formatToParts de uma duração de uma unidade só, com as partes numéricas trocadas
// pelo marcador `n`. Cada padrão vira uma string de partes `tipo:valor` separadas por U+001F, onde o tipo é
// `n` (o número), `l` (literal) ou `u` (o nome da unidade). A categoria de plural vem do Intl.PluralRules.
// Colunas do tsv: locale, style, índice da duração, partes (`tipo:valor:unidade` separadas por U+001F).
// Uso (na raiz do crate): bun scripts/gen-duration-format-data.js
const fs = require("fs");
const path = require("path");
const { escapeDashes, writeRustSource } = require("./rust-escape.js");

const root = path.join(__dirname, "..");
const LOCALES = "en en-GB pt pt-PT es es-MX fr fr-CA de de-AT it ja ko zh zh-TW ar fa he hi th tr pl nl sv da nb fi cs el id vi uk ru ro hu bg hr sr".split(" ");
const UNITS = ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"];
const WIDTHS = ["long", "short", "narrow"];
const STYLES = ["long", "short", "narrow", "digital"];
const SAMPLES = [0, 1, 2, 3, 4, 5, 6, 7, 11, 12, 21, 100, 101, 1000];
const NUMBER_PARTS = new Set(["integer", "group", "decimal", "fraction", "minusSign", "plusSign", "nan", "infinity", "compact", "exponentInteger", "exponentSeparator", "exponentMinusSign"]);
const SEP = "\u001f";
function rs(text) {
  const body = JSON.stringify(text).replace(/\\u([0-9a-f]{4})/gi, (_, hex) => `\\u{${hex}}`);
  return escapeDashes(body);
}

/** Troca as partes numéricas seguidas por um só `n`. */
function template(parts) {
  const out = [];
  for (const part of parts) {
    const kind = NUMBER_PARTS.has(part.type) ? "n" : part.type === "unit" ? "u" : "l";
    const value = kind === "n" ? "" : part.value;
    if (kind === "n" && out.length && out[out.length - 1].startsWith("n:")) continue;
    out.push(`${kind}:${value}`);
  }
  return out.join(SEP);
}

const patterns = []; // { lang, style, unit, category, text }
const separators = []; // { lang, text }
const tsv = [];

for (const locale of LOCALES) {
  const rules = new Intl.PluralRules(locale);
  for (const width of WIDTHS) {
    for (let unit = 0; unit < UNITS.length; unit++) {
      const format = new Intl.DurationFormat(locale, { style: width, [`${UNITS[unit]}Display`]: "always" });
      const seen = new Map();
      for (const sample of SAMPLES) {
        const category = rules.select(sample);
        if (seen.has(category)) continue;
        seen.set(category, true);
        const parts = format.formatToParts({ [UNITS[unit]]: sample });
        patterns.push({ lang: locale, style: WIDTHS.indexOf(width), unit, category, text: template(parts) });
      }
    }
  }
  const digital = new Intl.DurationFormat(locale, { style: "digital" }).formatToParts({ hours: 1, minutes: 2, seconds: 3 });
  const literal = digital.find((part) => part.type === "literal");
  separators.push({ lang: locale, text: literal ? literal.value : ":" });
}

// O golden: 30 durações por estilo e locale.
const DURATIONS = [
  { hours: 1 }, { hours: 2, minutes: 3 }, { hours: 1, minutes: 2, seconds: 3 }, { years: 1 }, { years: 2, months: 3 },
  { weeks: 1, days: 2 }, { days: 5 }, { hours: 11, minutes: 12, seconds: 21 }, { minutes: 1 }, { seconds: 1, milliseconds: 500 },
  { seconds: 5, milliseconds: 20, microseconds: 3, nanoseconds: 4 }, { milliseconds: 1 }, { microseconds: 2 }, { nanoseconds: 100 },
  { years: 1, months: 2, weeks: 3, days: 4, hours: 5, minutes: 6, seconds: 7, milliseconds: 8, microseconds: 9, nanoseconds: 10 },
  { hours: -1, minutes: -30 }, { days: -3 }, { seconds: 90 }, { hours: 1000 }, { hours: 0 }, { minutes: 0, seconds: 0 },
  { hours: 12, minutes: 0, seconds: 5 }, { milliseconds: 1500 }, { weeks: 2 }, { months: 12 }, { years: 21 }, { days: 101 },
  { hours: 3, seconds: 9 }, { minutes: 45, seconds: 30, milliseconds: 250 }, { years: 5, days: 1 },
];
for (const locale of LOCALES) {
  for (const style of STYLES) {
    const format = new Intl.DurationFormat(locale, { style });
    DURATIONS.forEach((duration, index) => {
      const parts = format.formatToParts(duration).map((part) => `${part.type}:${part.value}:${part.unit ?? ""}`).join(SEP);
      tsv.push([locale, style, index, escapeDashes(parts)].join("\t"));
    });
  }
}
fs.writeFileSync(path.join(root, "tests/golden/duration_format_bun.tsv"), require("./golden-prelude.js").assertPublicResult(tsv.join("\n") + "\n"));
fs.writeFileSync(path.join(root, "tests/golden/duration_format_durations.json"), JSON.stringify(DURATIONS) + "\n");

// O .rs: os padrões repetidos entram uma vez só no pool.
const pool = [];
const poolIndex = new Map();
const intern = (text) => {
  if (!poolIndex.has(text)) { poolIndex.set(text, pool.length); pool.push(text); }
  return poolIndex.get(text);
};
const rows = patterns.map((p) => `    Pattern { lang: ${rs(p.lang)}, style: ${p.style}, unit: ${p.unit}, category: ${rs(p.category)}, text: ${intern(p.text)} },`);
let out = `//! Padrões do \`Intl.DurationFormat\` das 38 locales do escopo, medidos no bun.
//!
//! ARQUIVO GERADO: não edite à mão. Para regenerar, na raiz de \`wip/zjsc\`:
//!
//! \`\`\`text
//! bun scripts/gen-duration-format-data.js
//! \`\`\`
//!
//! O mesmo script escreve \`tests/golden/duration_format_bun.tsv\`, que \`tests/duration_format_bun_golden.rs\` confere.
//! Cada texto de \`POOL\` é uma lista de partes \`tipo:valor\` separadas por U+001F: \`n\` é o número formatado, \`l\` um
//! literal e \`u\` o nome da unidade. Índices de estilo: \`long\` 0, \`short\` 1, \`narrow\` 2; de unidade: \`years\` 0 a
//! \`nanoseconds\` 9.

pub struct Pattern {
    pub lang: &'static str,
    pub style: u8,
    pub unit: u8,
    pub category: &'static str,
    pub text: u16,
}

/// O separador de horas, minutos e segundos do estilo digital de cada locale.
pub struct DigitalSeparator {
    pub lang: &'static str,
    pub text: &'static str,
}

pub const LOCALES: [&str; ${LOCALES.length}] = [${LOCALES.map(rs).join(", ")}];

pub const DIGITAL_SEPARATORS: [DigitalSeparator; ${separators.length}] = [
${separators.map((s) => `    DigitalSeparator { lang: ${rs(s.lang)}, text: ${rs(s.text)} },`).join("\n")}
];

pub const POOL: [&str; ${pool.length}] = [
${pool.map((t) => `    ${rs(t)},`).join("\n")}
];

pub const PATTERNS: [Pattern; ${rows.length}] = [
${rows.join("\n")}
];
`;
writeRustSource(path.join(root, "src/runtime/intl_duration_format_data.rs"), out);
console.log(`padrões ${patterns.length}, pool ${pool.length}, golden ${tsv.length} linhas`);
