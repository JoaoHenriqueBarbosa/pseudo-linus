// Mede no bun (JavaScriptCore + ICU) cerca de 700 programas de Intl.Locale, Intl.getCanonicalLocales,
// Intl.supportedValuesOf e resolvedOptions().locale com palavras-chave -u-, e gera o golden.
// Uso, a partir da raiz da crate:
//   bun scripts/gen-intl-locale-golden.js
// Escreve tests/golden/intl_locale_bun.tsv: PROGRAMA, tabulação, resultado. O invólucro (`wrap`) é o de
// tests/intl_locale_bun_golden.rs. O hash das listas de supportedValuesOf é FNV-1a de 32 bits sobre o join
// por vírgula (escrito em JavaScript puro dentro do programa, para rodar igual nos dois motores).
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

const wrap = (expr) =>
  `(function () { try { var v = (${expr}); return v === undefined ? "undefined" : typeof v === "string" ? v : JSON.stringify(v); } catch (e) { return "ERR " + e.name + ": " + e.message; } })()`;

const q = JSON.stringify;
const programs = [];
const add = (expr) => programs.push(expr);

// 1. resolvedOptions().locale com -u- em seis classes, 20 tags.
const KEYED = [
  "ar-u-nu-latn", "th-u-ca-buddhist", "ja-u-ca-japanese", "zh-u-ca-chinese", "hi-IN-u-nu-deva", "de-u-co-phonebk",
  "sv-u-kf-upper", "en-u-hc-h23", "en-US-u-hc-h12", "pt-BR-u-nu-arab", "ar-EG-u-nu-latn", "fa-u-ca-persian",
  "he-u-ca-hebrew", "en-u-kn", "en-u-kn-false", "es-u-co-trad", "zh-u-co-pinyin", "de-u-nu-hanidec-ca-gregory",
  "en-u-foo-bar", "fr-u-ca-xxxx",
];
const CLASSES = ["NumberFormat", "DateTimeFormat", "Collator", "PluralRules", "ListFormat", "RelativeTimeFormat"];
for (const tag of KEYED) {
  for (const name of CLASSES) {
    add(`new Intl.${name}(${q(tag)}).resolvedOptions().locale`);
  }
  add(`new Intl.NumberFormat(${q(tag)}).resolvedOptions().numberingSystem`);
  add(`new Intl.DateTimeFormat(${q(tag)}).resolvedOptions().calendar`);
  add(`new Intl.DateTimeFormat(${q(tag)}).resolvedOptions().hourCycle`);
  add(`new Intl.Collator(${q(tag)}).resolvedOptions().collation`);
  add(`new Intl.Collator(${q(tag)}).resolvedOptions().numeric`);
  add(`new Intl.Collator(${q(tag)}).resolvedOptions().caseFirst`);
}
// Opções que sobrepõem ou combinam com a palavra-chave.
for (const [tag, opts] of [
  ["en-u-nu-arab", { numberingSystem: "latn" }], ["en-u-nu-arab", { numberingSystem: "deva" }], ["en", { numberingSystem: "arab" }],
  ["en-u-nu-arab", { numberingSystem: "bogus" }], ["en-u-nu-xxxxxxxxx", {}],
]) {
  add(`new Intl.NumberFormat(${q(tag)}, ${q(opts)}).resolvedOptions().locale`);
  add(`new Intl.NumberFormat(${q(tag)}, ${q(opts)}).resolvedOptions().numberingSystem`);
}
for (const [tag, opts] of [
  ["en-u-ca-buddhist", { calendar: "japanese" }], ["en-u-ca-buddhist", { calendar: "buddhist" }], ["en-u-hc-h23", { hourCycle: "h12" }],
  ["en-u-hc-h23", { hour12: true }], ["en-u-hc-h23", { hour12: false }], ["ja-u-hc-h11", {}], ["en-u-ca-buddhist-nu-thai", {}],
]) {
  add(`new Intl.DateTimeFormat(${q(tag)}, ${q(opts)}).resolvedOptions().locale`);
  add(`new Intl.DateTimeFormat(${q(tag)}, ${q(opts)}).resolvedOptions().calendar`);
  add(`new Intl.DateTimeFormat(${q(tag)}, ${q(opts)}).resolvedOptions().hourCycle`);
}
for (const [tag, opts] of [
  ["sv-u-kf-upper", { caseFirst: "lower" }], ["sv-u-kf-upper", { caseFirst: "false" }], ["en-u-kn", { numeric: false }],
  ["en-u-kn-false", { numeric: true }], ["de-u-co-phonebk", { collation: "eor" }], ["de-u-co-phonebk", { usage: "search" }],
  ["de-u-co-search", {}], ["de-u-co-standard", {}], ["en-u-co-emoji", {}],
]) {
  add(`new Intl.Collator(${q(tag)}, ${q(opts)}).resolvedOptions().locale`);
  add(`new Intl.Collator(${q(tag)}, ${q(opts)}).resolvedOptions().collation`);
  add(`new Intl.Collator(${q(tag)}, ${q(opts)}).resolvedOptions().caseFirst`);
}
for (const name of CLASSES) {
  add(`Intl.${name}.supportedLocalesOf(["ar-u-nu-latn", "th-u-ca-buddhist", "xx-u-nu-latn", "de-u-co-phonebk"])`);
}

// 2. Intl.Locale: construtor, getters e métodos, tags e opções.
const TAGS = [
  "en", "en-US", "pt-BR", "zh-Hans-CN", "zh-CN", "zh-TW", "zh-Hant-TW", "ar-EG", "iw", "he", "iw-IL", "in", "id", "in-ID",
  "sh", "sh-RS", "sr-Latn", "no", "nb", "tl", "fil", "mo", "ro", "jw", "jv", "und", "und-Latn", "und-US", "und-Hans",
  "en-GB-oxendict", "ca-valencia", "sl-rozaj-biske", "de-1996", "en-US-posix", "es-419", "en-u-ca-gregory", "th-u-ca-buddhist-nu-thai",
  "ja-u-ca-japanese-hc-h23", "de-u-co-phonebk-kn-kf-upper", "sv-u-kf-upper", "en-t-hi", "en-t-hi-m0-abc", "ja-t-it-x0-xyz",
  "hi-IN-u-nu-deva", "ar-u-nu-latn", "en-x-private", "x-foo", "en-u-attr-ca-gregory", "en-u-fw-mon", "en-u-rg-gbzzzz",
  "en-u-sd-usca", "en-u-tz-brsao", "fr-CA", "pt-PT", "en-Latn-US", "ZH-hANS-cn", "EN-us", "sr-ME", "sr-Latn-ME", "ru-RU",
];
const GETTERS = [
  "language", "script", "region", "baseName", "calendar", "collation", "hourCycle", "caseFirst", "numeric", "numberingSystem",
  "variants", "firstDayOfWeek", "toString()", "maximize().toString()", "minimize().toString()",
];
for (const tag of TAGS) {
  for (const g of GETTERS) {
    const access = g.endsWith(")") ? g : g;
    add(`new Intl.Locale(${q(tag)}).${access}`);
  }
}
const METHODS = ["getCalendars", "getCollations", "getHourCycles", "getNumberingSystems", "getTextInfo", "getTimeZones", "getWeekInfo"];
const GETTER_FORMS = ["calendars", "collations", "hourCycles", "numberingSystems", "textInfo", "timeZones", "weekInfo"];
for (const tag of ["en-US", "pt-BR", "ar-EG", "th-u-ca-buddhist", "ja-JP", "he", "fa-AF", "de-u-co-phonebk-hc-h23", "und", "zh-Hant-TW"]) {
  for (const m of METHODS) add(`new Intl.Locale(${q(tag)}).${m}()`);
  for (const g of GETTER_FORMS) add(`new Intl.Locale(${q(tag)}).${g}`);
}
// Opções do construtor.
const OPTS = [
  { calendar: "buddhist" }, { calendar: "islamic-civil" }, { calendar: "bad_cal" }, { calendar: "" }, { collation: "phonebk" },
  { collation: "x" }, { hourCycle: "h12" }, { hourCycle: "h24" }, { hourCycle: "h99" }, { caseFirst: "upper" }, { caseFirst: "false" },
  { caseFirst: "bogus" }, { numeric: true }, { numeric: false }, { numeric: "yes" }, { numberingSystem: "arab" }, { numberingSystem: "toolongvalue" },
  { language: "fr" }, { language: "xx-yy" }, { script: "Latn" }, { script: "latin" }, { region: "BR" }, { region: "bra" }, { region: "123" },
  { variants: "1996" }, { firstDayOfWeek: "mon" }, { firstDayOfWeek: 7 }, { firstDayOfWeek: 0 }, { firstDayOfWeek: "xyz" },
  { language: "pt", region: "PT", calendar: "gregory", hourCycle: "h23" }, { script: "Hans", region: "TW" },
];
for (const base of ["en", "en-US", "en-u-ca-gregory-hc-h23", "zh-Hant", "de-u-co-phonebk"]) {
  for (const o of OPTS) add(`new Intl.Locale(${q(base)}, ${q(o)}).toString()`);
}
// Tags e entradas inválidas, mensagens exatas.
const BAD = [
  "", "e", "en_US", "en-", "-en", "en--US", "123", "en-US-US", "en-Latn-Latn", "toolonglanguage", "abcd", "en-u", "en-u-", "en-u-a",
  "en-t", "en-x", "en-x-", "en-a-b", "en-a-bb-a-cc", "en-u-ca-gregory-u-ca-buddhist", "en-1234-1234", "i-klingon", "en-US-a",
  "en-é", " en", "en ", "root", "und-u-ca", "en-u-ca-", "en-u-ca-gregory-ca-buddhist",
];
for (const tag of BAD) add(`new Intl.Locale(${q(tag)})`);
for (const tag of BAD) add(`Intl.getCanonicalLocales(${q(tag)})`);
for (const v of ["undefined", "null", "1", "true", "{}", "[]", "[null]", "[1]", "[undefined]", "Symbol()", "Object('en')", "{toString(){return 'pt-br'}}", "new Intl.Locale('en-us')"]) {
  add(`new Intl.Locale(${v})`);
  add(`Intl.getCanonicalLocales(${v})`);
  add(`new Intl.Locale("en", ${v}).toString()`);
}
add(`Intl.Locale("en")`);
add(`Intl.Locale.length`);
add(`Intl.Locale.name`);
add(`Object.prototype.toString.call(new Intl.Locale("en"))`);
add(`Intl.Locale.prototype[Symbol.toStringTag]`);
add(`Object.getOwnPropertyNames(Intl.Locale.prototype).sort()`);
add(`String(new Intl.Locale("pt-BR"))`);
add(`new Intl.Locale("pt-BR") + ""`);
add(`JSON.stringify(new Intl.Locale("pt-BR"))`);
add(`new Intl.Locale(new Intl.Locale("pt-BR-u-ca-gregory"), {region: "PT"}).toString()`);
add(`Intl.Locale.prototype.maximize.call({})`);
add(`Intl.Locale.prototype.toString.call("en")`);
add(`Object.getOwnPropertyDescriptor(Intl.Locale.prototype, "language").get.name`);
add(`Object.getOwnPropertyDescriptor(Intl.Locale.prototype, "language").set`);
add(`Intl.Locale.prototype.getWeekInfo.length`);
add(`Intl.Locale.prototype.maximize.name`);

// 3. Canonicalização de aliases e getCanonicalLocales.
const CANON = [
  "iw", "he", "in", "id", "ji", "yi", "jw", "mo", "sh", "tl", "fil", "no", "nb", "nn", "cmn", "cmn-CN", "zh-CN", "zh-cmn-Hans-CN",
  "und", "und-u-ca-gregory", "en-US-u-ca-gregory", "EN-US", "en-us", "En-uS", "zh-hans-cn", "sr-latn-rs", "sr-yu", "sr-cs", "sr-CS",
  "en-GB", "en-gb-oed", "i-klingon", "i-enochian", "art-lojban", "zh-min-nan", "zh-hakka", "sgn-be-fr", "de-DE-1901", "de-1901-1996",
  "de-1996-1901", "en-u-ca-islamicc", "en-u-ca-ethiopic-amete-alem", "en-u-ms-imperial", "en-u-ms-uksystem", "en-u-tz-cnckg",
  "en-u-tz-eire", "en-u-kb-yes", "en-u-kb-true", "en-u-kn-true", "en-u-kn-yes", "en-u-kn", "en-u-kn-", "en-u-ca-gregorian", "en-u-co-dict",
  "en-u-co-direct", "en-u-ks-primary", "en-u-ks-level1", "en-u-nu-latn", "en-u-ca-iso8601", "en-u-ca-gregory-fw-mon", "en-u-fw-MON",
  "en-U-CA-GREGORY", "en-Latn-US-t-ja-Jpan", "en-t-ja-m0-names", "en-t-m0-names-ja", "en-t-ja-a-bb", "en-z-ab-a-cd", "en-a-cd-z-ab",
  "en-x-foo-a-bar", "ja-Latn-t-it", "ca-ES-valencia", "ca-valencia-ES", "pt-BR-x-private", "sl-nedis-rozaj", "sl-rozaj-nedis",
  "en-u-rg-GBZZZZ", "en-u-sd-USCA", "en-u-vt-0020", "en-u-lb-loose", "en-u-lw-phrase", "en-u-ss-none", "en-u-em-emoji", "en-u-dx-latn",
  "ar-u-nu-arab", "ar-u-nu-arabext", "th-u-nu-thai", "en-u-nu-fullwide", "hi-u-nu-deva", "ro-MD", "ro-md", "mo-MD", "tl-PH", "iw-IL",
  "in-ID", "ji-US", "sh-RS", "sh-Latn", "sr-Latn-RS", "scc", "scr", "swh", "ar-arb", "arb", "arb-SA", "hbs", "hbs-Latn",
];
for (const tag of CANON) {
  add(`Intl.getCanonicalLocales(${q(tag)})`);
  add(`new Intl.Locale(${q(tag)}).toString()`);
}
for (const list of [
  ["en", "EN", "en-us", "en-US"], ["pt-BR", "pt-br", "PT-BR"], ["iw", "he"], ["zh-CN", "zh-Hans-CN"], [], ["en", "xx-invalid"],
  ["en", 1], ["en", null], "en", ["en", "fr", "en", "de", "fr"], ["de", "en-US", "und", "ja"], { length: 2, 0: "en", 1: "pt" },
  { length: 1, 0: "en-x" }, new Array(3), [undefined],
]) {
  add(`Intl.getCanonicalLocales(${JSON.stringify(list)})`);
}
add(`Intl.getCanonicalLocales.length`);
add(`Intl.getCanonicalLocales.name`);
add(`Intl.getCanonicalLocales()`);
add(`Intl.getCanonicalLocales(undefined)`);
add(`Intl.getCanonicalLocales(new Intl.Locale("en-US"))`);
add(`Intl.getCanonicalLocales([new Intl.Locale("en-US"), "pt"])`);
add(`Intl.getCanonicalLocales(new String("en"))`);

// 4. supportedValuesOf: lista inteira (tamanho, hash, 10 primeiros e 10 últimos).
const FNV = (list) => `(function (l) { var h = 2166136261, s = l.join(","); for (var i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; } return h.toString(16); })(${list})`;
for (const key of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"]) {
  const call = `Intl.supportedValuesOf(${q(key)})`;
  add(`${call}.length`);
  add(FNV(call));
  add(`${call}.slice(0, 10)`);
  add(`${call}.slice(-10)`);
  add(`${call}.join() === ${call}.slice().sort().join()`);
  add(`new Set(${call}).size === ${call}.length`);
  add(`Object.isFrozen(${call})`);
  add(`${call} === ${call}`);
  add(`Array.isArray(${call})`);
}
for (const key of ["", "Calendar", "calendars", "numberingsystem", "timezone", "script", "region", "language", "locale", "units", "currencies", "undefined", "null"]) {
  add(`Intl.supportedValuesOf(${q(key)})`);
}
add(`Intl.supportedValuesOf()`);
add(`Intl.supportedValuesOf(null)`);
add(`Intl.supportedValuesOf({toString(){return "unit"}}).length`);
add(`Intl.supportedValuesOf.length`);
add(`Intl.supportedValuesOf.name`);

const lines = programs.map((expr) => {
  if (/[\t\r\n]/.test(expr)) throw new Error("programa com tabulação ou quebra: " + expr);
  const source = expr.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const result = (0, eval)(wrap(source));
  return source + "\t" + String(result).replace(/[\r\n\t]/g, " ");
});
const unique = [...new Set(lines)];
writeFileSync(join(ROOT, "tests/golden/intl_locale_bun.tsv"), require("./golden-prelude.js").assertPublicResult(unique.join("\n") + "\n"));
console.log(unique.length + " programas");
