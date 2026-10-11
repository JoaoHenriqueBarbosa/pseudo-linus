const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/intl_misc_bun.tsv: o que os outros goldens de Intl ainda não cobrem, medido no bun 1.4.2.
// ListFormat.format com 2 a 6 itens, PluralRules.selectRange e resolvedOptions, opções do construtor de
// Intl.Locale, Symbol.toStringTag, propriedades próprias e construtores chamados sem new.
// Colunas: programa de uma linha (ASCII) e o resultado em JSON ASCII (exceção vira "Nome: mensagem").
// tests/intl_misc_bun_golden.rs roda cada programa na engine e compara.
// Uso: bun scripts/gen-intl-misc-golden.js > tests/golden/intl_misc_bun.tsv
function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}

const programs = [];
const add = (source) => programs.push(source);
const guarded = (expr) => `(() => { try { return ${expr}; } catch (e) { return e.name + ": " + e.message; } })()`;

const locales = [
  "en", "en-GB", "es", "pt", "pt-PT", "fr", "de", "it", "nl", "sv", "pl", "ru", "uk", "tr", "ar", "he", "hi", "ja", "ko",
  "zh", "zh-TW", "id", "th", "vi", "fi",
];

// ListFormat.format com 2 a 6 itens, todas as combinações de type e style.
const names = ["A", "B", "C", "D", "E", "F"];
for (const locale of locales) {
  for (const type of ["conjunction", "disjunction", "unit"]) {
    for (const style of ["long", "short", "narrow"]) {
      for (const count of [2, 3, 4, 5]) {
        const items = names.slice(0, count).map(q).join(",");
        add(`new Intl.ListFormat(${q(locale)}, { type: ${q(type)}, style: ${q(style)} }).format([${items}])`);
      }
    }
  }
  add(`JSON.stringify(new Intl.ListFormat(${q(locale)}).resolvedOptions())`);
}
add(guarded(`new Intl.ListFormat("en").format([1, 2])`));
add(guarded(`new Intl.ListFormat("en").format("abc")`));
add(guarded(`new Intl.ListFormat("en").format(new Set(["x", "y", "z"]))`));
add(guarded(`new Intl.ListFormat("en", { type: "x" })`));
add(guarded(`new Intl.ListFormat("en", { style: "x" })`));
add(guarded(`new Intl.ListFormat("en").formatToParts(5)`));

// PluralRules: selectRange, resolvedOptions e erros.
const ranges = [[0, 1], [1, 2], [1, 5], [0, 0], [21, 22], [2, 11], [1.5, 2.5], [100, 101]];
for (const locale of locales) {
  for (const type of ["cardinal", "ordinal"]) {
    for (const [a, b] of ranges) {
      add(`new Intl.PluralRules(${q(locale)}, { type: ${q(type)} }).selectRange(${a}, ${b})`);
    }
    add(`JSON.stringify(new Intl.PluralRules(${q(locale)}, { type: ${q(type)} }).resolvedOptions())`);
  }
}
add(guarded(`new Intl.PluralRules("en").selectRange(undefined, 1)`));
add(guarded(`new Intl.PluralRules("en").selectRange(1, undefined)`));
add(guarded(`new Intl.PluralRules("en").selectRange(NaN, 1)`));
add(guarded(`new Intl.PluralRules("en").selectRange(1, NaN)`));
add(guarded(`new Intl.PluralRules("en").selectRange(Infinity, 1)`));
add(guarded(`new Intl.PluralRules("en").selectRange(5, 1)`));
add(guarded(`new Intl.PluralRules("en", { type: "x" })`));
add(guarded(`new Intl.PluralRules("en").select(Infinity)`));
add(guarded(`new Intl.PluralRules("en").select(NaN)`));
add(guarded(`new Intl.PluralRules("en").select(-1)`));
add(guarded(`new Intl.PluralRules("en").select(2n)`));
add(guarded(`new Intl.PluralRules("en").select("1")`));

// Intl.Locale: opções do construtor sobrepõem a tag.
const localeOptions = [
  ["en", {}], ["en", { language: "fr" }], ["en", { script: "Latn" }], ["en", { region: "GB" }],
  ["en-US", { region: "CA" }], ["en-US", { script: "Cyrl" }], ["zh", { script: "Hant", region: "TW" }],
  ["en", { calendar: "buddhist" }], ["en", { collation: "phonebk" }], ["en", { hourCycle: "h23" }],
  ["en", { caseFirst: "upper" }], ["en", { numeric: true }], ["en", { numberingSystem: "arab" }],
  ["en-u-ca-gregory", { calendar: "hebrew" }], ["en-u-nu-latn", { numberingSystem: "deva" }],
  ["en-u-hc-h12", { hourCycle: "h23" }], ["en-u-kn", { numeric: false }], ["en-u-kf-lower", { caseFirst: "upper" }],
  ["de-u-co-phonebk", { collation: "eor" }], ["en", { language: "iw" }], ["en", { language: "x" }],
  ["en", { script: "latn" }], ["en", { region: "gb" }], ["en", { region: "XYZ" }], ["en", { region: "419" }],
  ["en", { calendar: "x" }], ["en", { calendar: "" }], ["en", { calendar: "gregory-x" }], ["en", { hourCycle: "h13" }],
  ["en", { caseFirst: "x" }], ["en", { caseFirst: "false" }], ["en", { numberingSystem: "x" }],
  ["en", { variants: "1996" }], ["en", { language: "und" }], ["und", {}], ["und-Latn", {}], ["und-x-foo", {}],
  ["en-u-ca-japanese-nu-jpan", {}], ["ja-JP-u-ca-japanese", {}], ["en-t-hi-i0-handwrit", {}], ["en-x-private", {}],
  ["en-a-bbb-u-ca-gregory", {}], ["th-TH-u-nu-thai", {}], ["ar-EG-u-nu-latn", {}], ["he-u-ca-hebrew", {}],
  ["en-u-tz-usnyc", {}], ["en-u-rg-gbzzzz", {}], ["en-u-sd-gbeng", {}], ["en-US-POSIX", {}], ["ca-ES-valencia", {}],
  ["sl-rozaj-biske", {}], ["zh-Hans-CN-u-ca-chinese", {}], ["en-Latn-US-u-hc-h23-ca-iso8601", {}],
  ["EN-latn-us", {}], ["i-klingon", {}], ["en-", {}], ["", {}], ["e", {}], ["en_US", {}],
];
for (const [tag, options] of localeOptions) {
  const expr =
    `(l => JSON.stringify([l.toString(), l.baseName, l.language, l.script, l.region, l.calendar, l.collation, ` +
    `l.hourCycle, l.caseFirst, l.numeric, l.numberingSystem, l.variants]))` +
    `(new Intl.Locale(${q(tag)}, ${JSON.stringify(options)}))`;
  add(guarded(expr));
}
add(guarded(`new Intl.Locale()`));
add(guarded(`new Intl.Locale(undefined)`));
add(guarded(`new Intl.Locale(null)`));
add(guarded(`new Intl.Locale(5)`));
add(guarded(`new Intl.Locale({})`));
add(guarded(`new Intl.Locale(new Intl.Locale("pt-BR")).toString()`));
add(guarded(`new Intl.Locale("en", null)`));
add(guarded(`new Intl.Locale("en", 5).toString()`));
add(guarded(`new Intl.Locale("en", { numeric: "false" }).numeric`));
add(guarded(`new Intl.Locale("en", { numeric: 0 }).numeric`));

// Symbol.toStringTag, propriedades próprias, protótipo e construtores chamados sem new.
const ctors = ["Collator", "DateTimeFormat", "DisplayNames", "DurationFormat", "ListFormat", "Locale", "NumberFormat", "PluralRules", "RelativeTimeFormat", "Segmenter"];
add(`Intl[Symbol.toStringTag]`);
add(`Object.prototype.toString.call(Intl)`);
add(`String(Intl)`);
add(`JSON.stringify(Object.getOwnPropertyDescriptor(Intl, Symbol.toStringTag))`);
add(`Object.getOwnPropertyNames(Intl)`);
add(`Object.getOwnPropertySymbols(Intl).map(String)`);
add(`JSON.stringify(Object.getOwnPropertyDescriptor(Intl, "ListFormat"))`);
add(`JSON.stringify(Object.getOwnPropertyDescriptor(Intl, "getCanonicalLocales"))`);
add(`typeof Intl`);
add(`Object.getPrototypeOf(Intl) === Object.prototype`);
add(guarded(`Intl()`));
add(guarded(`new Intl()`));
for (const name of ctors) {
  add(`${q(name)} in Intl`);
  add(`typeof Intl.${name}`);
  add(`Intl.${name}.length`);
  add(`Intl.${name}.name`);
  add(`Intl.${name}.prototype[Symbol.toStringTag]`);
  add(`Object.prototype.toString.call(Intl.${name}.prototype)`);
  add(`Object.getOwnPropertyNames(Intl.${name}.prototype)`);
  add(`Object.getOwnPropertyNames(Intl.${name})`);
  add(`JSON.stringify(Object.getOwnPropertyDescriptor(Intl.${name}, "prototype"))`);
  add(`JSON.stringify(Object.getOwnPropertyDescriptor(Intl.${name}.prototype, Symbol.toStringTag))`);
  add(`JSON.stringify(Object.getOwnPropertyDescriptor(Intl.${name}.prototype, "constructor"))`);
  add(`Intl.${name}.prototype.constructor === Intl.${name}`);
  add(`Object.getPrototypeOf(Intl.${name}) === Function.prototype`);
  add(guarded(`Intl.${name}()`));
  add(guarded(`Intl.${name}("en")`));
  add(guarded(`Intl.${name}.call({}, "en")`));
  add(guarded(`new Intl.${name}("en") instanceof Intl.${name}`));
  add(guarded(`Object.prototype.toString.call(new Intl.${name}("en"))`));
  add(guarded(`Object.getOwnPropertyNames(new Intl.${name}("en"))`));
  add(guarded(`Intl.${name}.supportedLocalesOf.length`));
  add(guarded(`Object.getOwnPropertyDescriptor(Intl.${name}, "supportedLocalesOf") === undefined`));
  add(guarded(`Intl.${name}.prototype.resolvedOptions.call({})`));
  add(guarded(`Intl.${name}.prototype.resolvedOptions.call(Intl.${name}.prototype)`));
  add(guarded(`Intl.${name}.prototype.resolvedOptions.length`));
  add(guarded(`new Intl.${name}("en", null)`));
  add(guarded(`Reflect.construct(Intl.${name}, ["en"], Object).constructor === Object`));
}
for (const method of ["format", "formatToParts"]) {
  add(guarded(`Intl.ListFormat.prototype.${method}.call({}, [])`));
  add(guarded(`Intl.ListFormat.prototype.${method}.length`));
}
add(guarded(`Intl.PluralRules.prototype.select.call({}, 1)`));
add(guarded(`Intl.PluralRules.prototype.selectRange.call({}, 1, 2)`));
add(guarded(`Intl.PluralRules.prototype.selectRange.length`));
add(guarded(`Intl.PluralRules.prototype.select.length`));
for (const getter of ["language", "script", "region", "baseName", "calendar", "collation", "hourCycle", "caseFirst", "numeric", "numberingSystem", "variants"]) {
  add(guarded(`Object.getOwnPropertyDescriptor(Intl.Locale.prototype, ${q(getter)}).get.name`));
  add(guarded(`Object.getOwnPropertyDescriptor(Intl.Locale.prototype, ${q(getter)}).get.call({})`));
  add(guarded(`typeof Object.getOwnPropertyDescriptor(Intl.Locale.prototype, ${q(getter)}).set`));
}
for (const method of ["maximize", "minimize", "toString", "getCalendars", "getCollations", "getHourCycles", "getNumberingSystems", "getTextInfo", "getTimeZones", "getWeekInfo"]) {
  add(guarded(`Intl.Locale.prototype.${method}.length`));
  add(guarded(`Intl.Locale.prototype.${method}.name`));
  add(guarded(`Intl.Locale.prototype.${method}.call({})`));
}
for (const accessor of ["calendars", "collations", "hourCycles", "numberingSystems", "textInfo", "timeZones", "weekInfo"]) {
  add(guarded(`typeof Object.getOwnPropertyDescriptor(Intl.Locale.prototype, ${q(accessor)}).get`));
}

for (const source of programs) {
  let result;
  try {
    const value = (0, eval)(`JSON.stringify(${source})`);
    result = value === undefined ? "undefined" : value;
  } catch (e) {
    result = "throw";
  }
  emitRow(source + "\t" + result.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0")));
}
