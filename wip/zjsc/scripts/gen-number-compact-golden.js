const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/number_compact_bun.tsv: `Intl.NumberFormat` com `notation: "compact"` em 25 locales, medido no bun 1.4.2.
// Cobre curto e longo, valores de 0 a 1e21 e negativos, dígitos mínimos e significativos, `roundingMode`, `signDisplay`,
// moeda e unidade compactas, `useGrouping` (min2, always, true, false), `formatToParts` e `formatRange`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Uso: bun scripts/gen-number-compact-golden.js > tests/golden/number_compact_bun.tsv
const locales = [
  "en", "pt", "es", "fr", "de", "ja", "ko", "zh", "ru", "ar", "hi", "tr", "it",
  "nl", "pl", "sv", "id", "th", "vi", "he", "uk", "cs", "el", "hu", "fa",
];
const values = ["0", "999", "1000", "1500", "12345", "999999", "1e6", "1.5e9", "1e12", "1e15", "1e21", "-1500", "-1234567", "0.5", "1234.5678"];
const partValues = ["1500", "12345", "-1234567", "1e12"];
const rangeValues = [["1000", "2000"], ["1500", "1e6"], ["999", "1e9"]];

const optionSets = [
  { minimumFractionDigits: 2 },
  { maximumFractionDigits: 0 },
  { maximumSignificantDigits: 2 },
  { minimumSignificantDigits: 3, maximumSignificantDigits: 4 },
  { roundingMode: "ceil" },
  { roundingMode: "floor", maximumFractionDigits: 1 },
  { roundingMode: "trunc", maximumFractionDigits: 0 },
  { roundingMode: "halfEven", maximumFractionDigits: 0 },
  { signDisplay: "always" },
  { signDisplay: "exceptZero" },
  { signDisplay: "negative" },
  { useGrouping: "min2" },
  { useGrouping: "always" },
  { useGrouping: true },
  { useGrouping: false },
  { style: "currency", currency: "USD" },
  { style: "currency", currency: "EUR", currencyDisplay: "name" },
  { style: "currency", currency: "JPY", currencyDisplay: "code" },
  { style: "currency", currency: "BRL", currencySign: "accounting" },
  { style: "unit", unit: "kilometer" },
  { style: "unit", unit: "kilogram", unitDisplay: "long" },
  { style: "unit", unit: "byte", unitDisplay: "narrow" },
  { style: "percent" },
  { trailingZeroDisplay: "stripIfInteger", minimumFractionDigits: 2 },
  { minimumIntegerDigits: 3 },
];

const programs = [];
const literal = (options) => JSON.stringify({ notation: "compact", ...options });
const make = (locale, options, tail) => `new Intl.NumberFormat(${JSON.stringify(locale)}, ${literal(options)})${tail}`;

for (const locale of locales) {
  for (const compactDisplay of ["short", "long"]) {
    for (const value of values) programs.push(make(locale, { compactDisplay }, `.format(${value})`));
    for (const value of partValues) {
      programs.push(make(locale, { compactDisplay }, `.formatToParts(${value}).map(p => p.type + ":" + p.value).join("|")`));
    }
    for (const [from, to] of rangeValues) programs.push(make(locale, { compactDisplay }, `.formatRange(${from}, ${to})`));
  }
  optionSets.forEach((options, index) => {
    for (let step = 0; step < 3; step++) {
      const value = values[(index * 3 + step * 5 + locales.indexOf(locale)) % values.length];
      const compactDisplay = (index + step) % 2 === 0 ? "short" : "long";
      programs.push(make(locale, { ...options, compactDisplay }, `.format(${value})`));
    }
    programs.push(make(locale, options, `.formatToParts(-1234567.891).map(p => p.type + ":" + p.value).join("|")`));
  });
  programs.push(make(locale, {}, `.resolvedOptions().useGrouping + ":" + new Intl.NumberFormat(${JSON.stringify(locale)}, { notation: "compact", useGrouping: "always" }).resolvedOptions().useGrouping`));
}

const seen = new Set();
let kept = 0;
for (const expression of programs) {
  if (seen.has(expression)) continue;
  seen.add(expression);
  const source = `"use strict";\ntry { globalThis.R = String(${expression}) } catch (e) { globalThis.R = "throws " + e.name }`;
  globalThis.R = undefined;
  (0, eval)(source.replace('"use strict";\n', ""));
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(globalThis.R));
}
process.stderr.write(`mantidos ${kept}\n`);
