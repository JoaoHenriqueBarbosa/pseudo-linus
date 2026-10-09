// Gera tests/golden/number_regional_bun.tsv: Intl.NumberFormat em variantes regionais (en-GB, en-IN, pt-PT,
// es-MX, fr-CH, de-AT, zh-TW, hi-IN, ar-EG, th-TH-u-nu-thai...) medido no bun 1.4.2 (ICU completo).
// Cada linha é um programa de uma linha (ASCII puro) com o `typeof` e a serialização do resultado, no formato
// de tests/golden/number_format_more_bun.tsv, avaliado pelo mesmo tests/golden/e2e_values_harness.js.
// O resultado de `formatToParts` sai como `tipo:valor|tipo:valor`, e o de `resolvedOptions` como JSON.
// Cada combinação de locale e opções roda com 15 valores (negativos, 0, -0, NaN, Infinity, 1e21, 0.000001).
// Uso (da raiz do crate): bun scripts/gen-number-regional-golden.js
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
const harness = fs.readFileSync(path.join(root, "tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

const LOCALES = [
  "en-GB", "en-IN", "en-AU", "pt-PT", "es-MX", "es-AR", "fr-CA", "fr-CH", "de-CH", "de-AT", "zh-TW", "hi-IN",
  "ar-EG", "ar-SA", "fa-IR", "th-TH-u-nu-thai", "ja-JP-u-nu-hanidec",
];
const CURRENCY_LOCALES = ["en-GB", "en-IN", "pt-PT", "es-AR", "fr-CH", "de-AT", "zh-TW", "ar-EG"];
const UNIT_LOCALES = ["en-GB", "pt-PT", "es-MX", "fr-CA", "de-CH", "hi-IN"];
const SIGN_LOCALES = ["en-GB", "de-CH", "ar-EG", "hi-IN"];
const CURRENCIES = ["EUR", "USD", "BRL", "JPY", "INR", "CHF"];
const UNITS = [
  "kilometer", "kilogram", "liter", "second", "hour", "day", "celsius", "megabyte", "kilometer-per-hour", "percent",
];
const VALUES = ["0", "-0", "1", "-1", "1234.5", "-1234.5", "12345", "1234567.891", "0.5", "0.256", "0.000001", "1e21", "NaN", "Infinity", "-Infinity"];
const ROUNDING_VALUES = ["2.5", "-2.5", "1.5", "-1.5", "0.5", "-0.5", "5.5", "-5.5", "0.125", "-0.125", "1.005", "1234.5678", "-1234.5678", "0", "10.25"];
const ROUNDING_MODES = ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"];
const INCREMENTS = [2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000];
const GROUPINGS = [true, false, "min2", "always", "auto"];
const SIGN_DISPLAYS = ["auto", "always", "exceptZero", "negative", "never"];

function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}

const programs = [];
function parts(locale, options, values) {
  const make = `new Intl.NumberFormat(${q(locale)}, ${JSON.stringify(options)})`;
  for (const value of values) {
    programs.push(`${make}.formatToParts(${value}).map(p=>p.type+":"+p.value).join("|")`);
  }
}
function resolved(locale, options) {
  programs.push(`JSON.stringify(new Intl.NumberFormat(${q(locale)}, ${JSON.stringify(options)}).resolvedOptions())`);
}

for (const locale of LOCALES) {
  resolved(locale, {});
  resolved(locale, { style: "percent" });
  resolved(locale, { style: "currency", currency: "EUR" });
  resolved(locale, { notation: "compact" });
  resolved(locale, { useGrouping: "min2" });
  parts(locale, {}, VALUES);
  parts(locale, { style: "percent" }, VALUES);
  for (const grouping of GROUPINGS) parts(locale, { useGrouping: grouping }, VALUES);
  for (const notation of ["scientific", "engineering"]) parts(locale, { notation }, VALUES);
  for (const display of ["short", "long"]) {
    parts(locale, { notation: "compact", compactDisplay: display }, VALUES);
  }
}
for (const locale of CURRENCY_LOCALES) {
  for (const currency of CURRENCIES) {
    for (const currencyDisplay of ["symbol", "code", "name", "narrowSymbol"]) {
      resolved(locale, { style: "currency", currency, currencyDisplay });
      parts(locale, { style: "currency", currency, currencyDisplay }, VALUES);
    }
    parts(locale, { style: "currency", currency, currencySign: "accounting" }, VALUES);
  }
}
for (const locale of UNIT_LOCALES) {
  for (const unit of UNITS) {
    for (const unitDisplay of ["short", "long", "narrow"]) {
      parts(locale, { style: "unit", unit, unitDisplay }, VALUES);
    }
  }
}
// Moedas fora das 12 principais de gen-number-format-data.js (as que saem de EXTRA_CURRENCIES): símbolo, código e nome.
const EXTRA_CURRENCY_CODES = [
  "AUD", "NZD", "SEK", "NOK", "DKK", "PLN", "TRY", "ZAR", "SGD", "HKD", "THB", "ILS", "AED", "SAR", "EGP", "CZK", "HUF",
  "CLP", "COP", "TWD",
];
for (const locale of ["es", "fr", "de", "ja", "ar"]) {
  for (const currency of EXTRA_CURRENCY_CODES) {
    for (const currencyDisplay of ["symbol", "code", "name"]) {
      parts(locale, { style: "currency", currency, currencyDisplay }, currencyDisplay === "name" ? ["1", "2", "-1234.5"] : ["1234.5", "-1234.5"]);
    }
  }
}

// formatRange: 60 intervalos com moeda, unidade e percent nos 38 locales de gen-number-format-data.js.
const RANGE_LOCALES = [
  "es", "fr", "de", "it", "ja", "ru", "ar", "hi", "zh", "ko", "fa", "th", "fr-CA", "de-CH", "es-MX", "zh-TW", "pt-PT",
  "es-AR", "fr-CH", "de-AT", "ar-EG", "en-GB", "en-IN", "en-AU", "ar-SA", "tr", "pl", "nl", "sv", "he", "da", "nb", "fi",
  "cs", "el", "id", "vi", "uk",
];
const RANGE_KINDS = [
  { style: "currency", currency: "EUR" },
  { style: "unit", unit: "kilometer", unitDisplay: "long" },
  { style: "percent" },
  { style: "currency", currency: "JPY", currencyDisplay: "code" },
  { style: "unit", unit: "celsius", unitDisplay: "short" },
  { style: "currency", currency: "USD", currencyDisplay: "name" },
];
const RANGE_PAIRS = [["3", "5"], ["0.25", "0.5"], ["1", "1"], ["1234.5", "5678.9"], ["-5", "-3"]];
for (let i = 0; i < 60; i++) {
  const locale = RANGE_LOCALES[(i * 7) % RANGE_LOCALES.length];
  const options = RANGE_KINDS[i % RANGE_KINDS.length];
  const [from, to] = RANGE_PAIRS[(i + Math.floor(i / RANGE_KINDS.length)) % RANGE_PAIRS.length];
  programs.push(`new Intl.NumberFormat(${q(locale)}, ${JSON.stringify(options)}).formatRange(${from}, ${to})`);
}

for (const locale of SIGN_LOCALES) {
  for (const signDisplay of SIGN_DISPLAYS) {
    parts(locale, { signDisplay }, VALUES);
    parts(locale, { style: "currency", currency: "USD", currencySign: "accounting", signDisplay }, VALUES);
  }
}
for (const locale of ["en-GB", "pt-PT", "de-CH", "hi-IN"]) {
  for (const roundingMode of ROUNDING_MODES) {
    parts(locale, { maximumFractionDigits: 0, roundingMode }, ROUNDING_VALUES);
    parts(locale, { maximumFractionDigits: 1, roundingMode }, ROUNDING_VALUES);
  }
  for (const roundingIncrement of INCREMENTS) {
    resolved(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2, roundingIncrement });
    parts(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2, roundingIncrement }, ROUNDING_VALUES);
  }
}

const seen = new Set();
const lines = [];
for (const src of programs) {
  if (seen.has(src)) continue;
  seen.add(src);
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  const result = (0, eval)(`${harness}(${JSON.stringify(src)})`);
  if (typeof result !== "string") throw new Error(`${src}: o harness não devolveu string`);
  lines.push(`${src}\t${result}`);
}
fs.writeFileSync(path.join(root, "tests/golden/number_regional_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.error(`${lines.length} casos`);
