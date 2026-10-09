// Gera tests/golden/date_pattern_bun.tsv: a ordem e a pontuação do padrão de data e hora de cada locale,
// medidas no bun (JavaScriptCore + ICU) com `formatToParts`. Tudo em UTC e com instantes fixos.
// Uso (da raiz da crate zjsc):
//   timeout 120 bun scripts/gen-date-pattern-golden.js
// Colunas: tag pedida, instante (ms), opções (JSON), `resolvedOptions().locale`, `numberingSystem`, e todas as
// partes na ordem, como `tipo=valor` separados por `|` (os literais entram, com o tipo `literal`).
const fs = require("fs");
const path = require("path");

const LOCALES = [
  "en", "pt-BR", "es", "fr", "de", "it", "ja", "zh", "ar", "ko", "ru", "nl",
  "en-GB", "en-AU", "en-CA", "en-IN", "pt-PT", "es-MX", "es-AR", "fr-CA", "de-AT", "de-CH", "zh-TW", "zh-HK",
  "hi", "th", "tr", "pl", "sv", "da", "nb", "fi", "cs", "el", "he", "id", "vi", "uk",
];
// O dia 25 desfaz a ambiguidade entre dia e mês; a manhã e a tarde cobrem AM e PM.
const EPOCHS = [Date.UTC(2024, 2, 5, 7, 8, 9), Date.UTC(2024, 10, 25, 19, 8, 9)];
const STYLES = ["full", "long", "medium", "short"];

const SETS = [];
for (const style of STYLES) SETS.push({ dateStyle: style });
for (const style of STYLES) SETS.push({ timeStyle: style });
for (const dateStyle of STYLES) for (const timeStyle of STYLES) SETS.push({ dateStyle, timeStyle });

const Y = { year: "numeric" };
const D = { day: "numeric" };
const HM = { hour: "numeric", minute: "2-digit" };
const HMS = { hour: "2-digit", minute: "2-digit", second: "2-digit" };
SETS.push(
  { ...Y, month: "numeric", ...D },
  { year: "numeric", month: "2-digit", day: "2-digit" },
  { ...Y, month: "long", ...D },
  { ...Y, month: "short", ...D },
  { month: "long", ...D },
  { month: "short", ...D },
  { ...Y, month: "long" },
  { ...Y, month: "short" },
  { ...Y, month: "numeric" },
  { month: "numeric", ...D },
  { weekday: "long", ...Y, month: "long", ...D },
  { weekday: "short", ...Y, month: "short", ...D },
  { weekday: "long" },
  { weekday: "short", month: "numeric", ...D },
  { ...Y },
  { month: "long" },
  { ...D },
  { weekday: "long", month: "long", ...D },
  { weekday: "narrow" },
  { month: "narrow" },
  { ...HM },
  { ...HM, hour12: true },
  { ...HM, hour12: false },
  { ...HMS },
  { ...HMS, hour12: true },
  { ...HMS, hour12: false },
  { hour: "numeric", hour12: true },
  { hour: "numeric", hour12: false },
  { minute: "2-digit", second: "2-digit" },
  { ...Y, month: "numeric", ...D, ...HM },
  { ...Y, month: "long", ...D, ...HMS, hour12: false },
  { weekday: "short", ...HM },
  { month: "short", ...D, ...HM, hour12: true },
  { ...HM, timeZoneName: "short" },
);

const tsvQuote = (text) => text.replace(/[\t\n\\]/g, (c) => ({ "\t": "\\t", "\n": "\\n", "\\": "\\\\" })[c]);

const rows = [];
for (const tag of LOCALES) {
  for (const options of SETS) {
    for (const epoch of EPOCHS) {
      const format = new Intl.DateTimeFormat(tag, { ...options, timeZone: "UTC" });
      const resolved = format.resolvedOptions();
      const parts = format.formatToParts(epoch).map((part) => {
        if (part.value.includes("|")) throw new Error(`parte com barra vertical: ${tag} ${JSON.stringify(options)}`);
        return `${part.type}=${part.value}`;
      });
      const cells = [tag, String(epoch), JSON.stringify(options), resolved.locale, resolved.numberingSystem, parts.join("|")];
      rows.push(cells.map(tsvQuote).join("\t"));
    }
  }
}
const out = path.join(__dirname, "..", "tests", "golden", "date_pattern_bun.tsv");
fs.writeFileSync(out, rows.join("\n") + "\n");
console.error(`${rows.length} linhas em tests/golden/date_pattern_bun.tsv`);
