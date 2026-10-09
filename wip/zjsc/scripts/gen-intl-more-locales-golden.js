// Gera tests/golden/intl_more_locales_bun.tsv a partir do bun (JavaScriptCore real): formatação de Intl nos 25
// locales da terceira leva (am my km lo mn ps sd so fil ha yo zu xh cy gd lb mt fo ky tg tk tt ku or as).
// Serve para medir se o icu4x com dados compilados cobre esses locales. Cada linha é `expressão<TAB>resultado`.
//
// Como regenerar (da raiz de wip/zjsc):  bun scripts/gen-intl-more-locales-golden.js
const fs = require("fs");
const path = require("path");

const LOCALES = [
  "am", "my", "km", "lo", "mn", "ps", "sd", "so", "fil", "ha", "yo", "zu", "xh", "cy", "gd", "lb", "mt", "fo",
  "ky", "tg", "tk", "tt", "ku", "or", "as",
];

const out = [];
const q = (value) => JSON.stringify(value);

function emit(expression) {
  let result;
  try {
    result = String(new Function("return " + expression)());
  } catch (error) {
    result = "throw";
  }
  if (/[\t\n\r]/.test(result) || result.includes(String.fromCharCode(0x2028)) || result.includes(String.fromCharCode(0x2029))) result = q(result);
  out.push(expression + "\t" + result);
}

const NUMBERS = [0, 1, 2, 3, 5, 11, 21, 100, 1.5];
const DATE = 1710000000000 + 7 * 3600 * 1000 + 8 * 60 * 1000;
const PAIRS = [["a", "b"], ["b", "a"], ["a", "A"], ["z", "a"], ["résumé", "resume"]];

for (const locale of LOCALES) {
  const l = q(locale);
  for (const n of [1234567.891, 0.256, -42, 0]) {
    emit(`new Intl.NumberFormat(${l}).format(${n})`);
    emit(`new Intl.NumberFormat(${l}, { style: "percent" }).format(${n})`);
    emit(`new Intl.NumberFormat(${l}, { style: "currency", currency: "USD" }).format(${n})`);
  }
  for (const n of [1234, 1234567, 1500000000]) {
    emit(`new Intl.NumberFormat(${l}, { notation: "compact" }).format(${n})`);
    emit(`new Intl.NumberFormat(${l}, { notation: "compact", compactDisplay: "long" }).format(${n})`);
  }
  emit(`new Intl.NumberFormat(${l}).resolvedOptions().numberingSystem`);

  for (const type of ["cardinal", "ordinal"]) {
    const ctor = `new Intl.PluralRules(${l}, { type: ${q(type)} })`;
    emit(`${ctor}.resolvedOptions().pluralCategories.join(",")`);
    for (const n of NUMBERS) emit(`${ctor}.select(${n})`);
  }

  for (const type of ["conjunction", "disjunction"]) {
    const ctor = `new Intl.ListFormat(${l}, { type: ${q(type)} })`;
    emit(`${ctor}.format(["A", "B"])`);
    emit(`${ctor}.format(["A", "B", "C"])`);
  }

  for (const unit of ["day", "month"]) {
    for (const n of [-1, 0, 1, 2]) {
      emit(`new Intl.RelativeTimeFormat(${l}).format(${n}, ${q(unit)})`);
      emit(`new Intl.RelativeTimeFormat(${l}, { numeric: "auto" }).format(${n}, ${q(unit)})`);
    }
  }

  for (const options of [`{ dateStyle: "full", timeZone: "UTC" }`, `{ dateStyle: "medium", timeZone: "UTC" }`,
    `{ timeStyle: "short", timeZone: "UTC" }`]) {
    emit(`new Intl.DateTimeFormat(${l}, ${options}).format(${DATE})`);
  }
  emit(`new Intl.DateTimeFormat(${l}, { timeZone: "UTC" }).resolvedOptions().calendar`);

  for (const [a, b] of PAIRS) emit(`new Intl.Collator(${l}).compare(${q(a)}, ${q(b)})`);
}

const file = path.resolve(__dirname, "..", "tests", "golden", "intl_more_locales_bun.tsv");
fs.writeFileSync(file, out.join("\n") + "\n");
console.log(out.length + " linhas em tests/golden/intl_more_locales_bun.tsv");
