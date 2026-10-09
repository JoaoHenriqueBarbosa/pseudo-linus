// Gera tests/golden/intl_more_bun.tsv a partir do bun (JavaScriptCore real): ListFormat, PluralRules
// (select, selectRange e pluralCategories), RelativeTimeFormat e Collator em 38 locales.
// Cada linha é `expressão<TAB>resultado`; a expressão devolve string (ou `throw`).
//
// Como regenerar (da raiz de wip/zjsc):  bun scripts/gen-intl-more-golden.js
const fs = require("fs");
const path = require("path");

const LOCALES = [
  "en", "en-GB", "pt", "pt-PT", "es", "es-MX", "fr", "fr-CA", "de", "de-AT", "it", "ja", "ko", "zh", "zh-TW",
  "ar", "fa", "he", "hi", "th", "tr", "pl", "nl", "sv", "da", "nb", "fi", "cs", "el", "id", "vi", "uk", "ru",
  "ro", "hu", "bg", "hr",
];
// `sr` completa os 38 locales do escopo (a lista nomeada tem 37).
LOCALES.push("sr");

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

// ListFormat: type x style x 0 a 4 itens, format e formatToParts.
const ITEMS = ["A", "B", "C", "D"];
for (const locale of LOCALES) {
  for (const type of ["conjunction", "disjunction", "unit"]) {
    for (const style of ["long", "short", "narrow"]) {
      for (let count = 0; count <= 4; count++) {
        const ctor = `new Intl.ListFormat(${q(locale)}, { type: ${q(type)}, style: ${q(style)} })`;
        const items = q(ITEMS.slice(0, count));
        emit(`${ctor}.format(${items})`);
        emit(`JSON.stringify(${ctor}.formatToParts(${items}))`);
      }
    }
  }
}

// PluralRules: select e selectRange por tipo, mais as categorias resolvidas.
const NUMBERS = [
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 20, 21, 22, 23, 24, 25, 30, 100, 101, 102, 111, 112, 200,
  1000, 1000000, 0.5, 1.5, 2.5, 0.1, 1.1, 2.1, 10.5, -1, -2, 1e21,
];
const RANGES = [
  [0, 1], [1, 2], [1, 5], [2, 5], [0, 5], [1, 21], [21, 22], [3, 11], [10, 100], [0, 0], [1, 1], [5, 5],
  [0.5, 1], [1, 1.5], [0, 2], [2, 3], [5, 21], [11, 21], [1, 101], [100, 1000], [0, 10], [1, 10], [4, 14],
  [21, 31], [0, 1000], [1, 1000000], [7, 8], [12, 14], [20, 21], [2, 22], [0, 21], [1, 3], [3, 5], [6, 10],
  [9, 19], [101, 111], [102, 112], [1, 0.5], [0.1, 0.5], [2, 1],
];
for (const locale of LOCALES) {
  for (const type of ["cardinal", "ordinal"]) {
    const ctor = `new Intl.PluralRules(${q(locale)}, { type: ${q(type)} })`;
    emit(`${ctor}.resolvedOptions().pluralCategories.join(",")`);
    emit(`${ctor}.resolvedOptions().locale`);
    for (const n of NUMBERS) emit(`${ctor}.select(${n})`);
    for (const [a, b] of RANGES) emit(`${ctor}.selectRange(${a}, ${b})`);
  }
}

// RelativeTimeFormat: numeric x unidades x valores, estilo long.
const UNITS = ["second", "minute", "hour", "day", "week", "month", "quarter", "year", "seconds", "minutes", "hours", "days"];
const VALUES = [-2, -1, 0, 1, 2, 3, 10, 100];
for (const locale of LOCALES) {
  for (const numeric of ["always", "auto"]) {
    const ctor = `new Intl.RelativeTimeFormat(${q(locale)}, { numeric: ${q(numeric)} })`;
    for (const unit of UNITS) for (const value of VALUES) emit(`${ctor}.format(${value}, ${q(unit)})`);
  }
}

// Collator: 60 pares por locale.
const PAIRS = [
  ["a", "b"], ["b", "a"], ["a", "A"], ["A", "a"], ["a", "á"], ["á", "b"], ["e", "é"], ["é", "f"], ["o", "ö"],
  ["ö", "z"], ["u", "ü"], ["ü", "v"], ["a", "ä"], ["ä", "z"], ["a", "å"], ["å", "z"], ["n", "ñ"], ["ñ", "o"],
  ["c", "ç"], ["ç", "d"], ["i", "ı"], ["ı", "j"], ["i", "İ"], ["I", "ı"], ["s", "ş"], ["ş", "t"], ["g", "ğ"],
  ["ß", "ss"], ["ss", "st"], ["ae", "æ"], ["æ", "af"], ["o", "ø"], ["ø", "p"], ["ll", "lz"], ["ch", "cz"],
  ["ch", "d"], ["cs", "d"], ["a1", "a2"], ["a2", "a10"], ["a10", "a9"], ["file2", "file10"], ["x-y", "xy"],
  ["x y", "xy"], ["x.y", "x_y"], ["résumé", "resume"], ["Zebra", "apple"], ["apple", "Apple"], ["あ", "ア"],
  ["か", "が"], ["ア", "イ"], ["漢", "字"], ["中", "文"], ["一", "二"], ["가", "나"], ["α", "β"], ["я", "а"],
  ["б", "в"], ["א", "ב"], ["ب", "ت"], ["ก", "ข"],
];
for (const locale of LOCALES) {
  const ctor = `new Intl.Collator(${q(locale)})`;
  for (const [a, b] of PAIRS) emit(`${ctor}.compare(${q(a)}, ${q(b)})`);
}

const OPTION_LOCALES = ["sv", "de", "es", "tr", "ja", "zh-u-co-pinyin", "de-u-co-phonebk"];
const OPTION_SETS = [
  `{ sensitivity: "base" }`, `{ sensitivity: "accent" }`, `{ sensitivity: "case" }`, `{ sensitivity: "variant" }`,
  `{ numeric: true }`, `{ caseFirst: "upper" }`, `{ caseFirst: "lower" }`, `{ ignorePunctuation: true }`,
  `{ usage: "search" }`, `{ usage: "search", sensitivity: "base" }`,
];
for (const locale of OPTION_LOCALES) {
  for (const options of OPTION_SETS) {
    const ctor = `new Intl.Collator(${q(locale)}, ${options})`;
    for (const [a, b] of PAIRS) emit(`${ctor}.compare(${q(a)}, ${q(b)})`);
    emit(`JSON.stringify(${ctor}.resolvedOptions())`);
  }
}

const file = path.resolve(__dirname, "..", "tests", "golden", "intl_more_bun.tsv");
fs.writeFileSync(file, out.join("\n") + "\n");
console.log(out.length + " linhas em tests/golden/intl_more_bun.tsv");
