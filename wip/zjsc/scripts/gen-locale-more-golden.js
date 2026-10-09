// Mede no bun (JavaScriptCore + ICU) 1500 programas de Intl.Locale e Intl.getCanonicalLocales e gera o golden.
// Uso, a partir da raiz da crate:
//   bun scripts/gen-locale-more-golden.js
// Escreve tests/golden/locale_more_bun.tsv: PROGRAMA, tabulação, resultado. O programa é uma expressão sem
// tabulação nem quebra de linha; o resultado é a serialização dela, ou `ERR Nome: mensagem` se lançar.
// O mesmo invólucro (`wrap`) está em tests/locale_more_bun_golden.rs.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

const wrap = (expr) =>
  `(function () { try { var v = (${expr}); return v === undefined ? "undefined" : typeof v === "string" ? v : JSON.stringify(v); } catch (e) { return "ERR " + e.name + ": " + e.message; } })()`;

const q = JSON.stringify;
const programs = [];
const add = (expr) => programs.push(expr);

// 1. maximize e minimize, 200 tags.
const MAX_TAGS = `und und-Latn und-US und-BR und-419 und-Cyrl und-Arab und-Hans und-Hant und-Deva und-Jpan und-Kore
zh zh-TW zh-CN zh-HK zh-MO zh-SG zh-Hant zh-Hans zh-Hant-TW zh-Hans-CN zh-Latn zh-Hant-HK sr sr-ME sr-RS sr-Latn
sr-Cyrl sr-Latn-ME sr-Cyrl-RS sr-Latn-BA sr-BA en en-US en-GB en-AU en-CA en-IN en-Latn en-Latn-US en-Shaw en-Dsrt
en-Latn-US-u-ca-gregory en-US-u-ca-buddhist en-u-hc-h23 en-US-posix en-GB-oxendict en-US-u-fw-mon pt pt-BR pt-PT pt-AO
pt-MZ pt-BR-u-hc-h23 pt-BR-t-en pt-BR-t-es-ES pt-BR-t-en-us-h0-hybrid pt-PT-t-en-US es es-ES es-MX es-419 es-AR es-US
es-Latn-ES fr fr-CA fr-CH fr-FR fr-BE de de-AT de-CH de-DE de-1996 de-CH-1901 it it-CH ja ja-JP ja-Jpan ja-Latn ko
ko-KR ko-Kore ko-KP ru ru-RU ru-UA ru-Cyrl ar ar-EG ar-SA ar-AE ar-Latn ar-Arab-EG he he-IL iw iw-IL fa fa-AF fa-IR hi
hi-IN hi-Latn th th-TH tr tr-TR tr-DE pl nl nl-BE sv da nb no nn fi cs el hu ro uk vi id in ms bn ta sw fil tl ur ca
sh sh-Latn sh-Cyrl cmn cmn-Hans yue yue-Hant yue-HK az az-AZ az-Cyrl az-Arab uz uz-AF uz-Cyrl uz-Latn ug ks pa pa-PK
pa-Arab pa-Guru mn mn-Mong mn-CN ku ku-Arab ku-TR kk kk-CN ky ky-CN tg tg-Arab sd sd-IN sd-Deva ff ff-Adlm ff-SN
ha ha-Arab ha-NG yo ig zu xh af am ti ti-ER so om ps ps-PK dz ne si my km lo bo bo-IN fy gd gv ga cy br eu gl oc co
sc sq mk be bs hy ka lt lv et is mt la tlh x-private i-klingon en-x-foo`
  .split(/\s+/);
for (const tag of MAX_TAGS.slice(0, 200)) {
  add(`new Intl.Locale(${q(tag)}).maximize().toString()`);
}
// 2. minimize das mesmas tags (mais as maximizadas), 120 delas.
for (const tag of MAX_TAGS.slice(0, 120)) add(`new Intl.Locale(${q(tag)}).minimize().toString()`);
for (const tag of ["en-Latn-US", "zh-Hant-TW", "zh-Hans-CN", "sr-Latn-RS", "sr-Cyrl-RS", "pt-Latn-BR", "ja-Jpan-JP",
  "ko-Kore-KR", "ru-Cyrl-RU", "ar-Arab-EG", "hi-Deva-IN", "und-Latn-US", "und-Cyrl-RU", "de-Latn-DE-1996",
  "en-Latn-US-u-ca-gregory-hc-h12", "fr-Latn-CA", "es-Latn-419", "az-Latn-AZ", "uz-Latn-UZ", "pa-Arab-PK"]) {
  add(`new Intl.Locale(${q(tag)}).minimize().toString()`);
  add(`new Intl.Locale(${q(tag)}).maximize().minimize().toString()`);
}
// maximize e minimize preservando extensões e opções.
for (const tag of ["en-u-ca-gregory", "zh-TW-u-nu-hanidec", "pt-t-en", "sr-ME-u-co-phonebk", "und-u-hc-h23"]) {
  add(`new Intl.Locale(${q(tag)}).maximize().toString()`);
  add(`new Intl.Locale(${q(tag)}).minimize().toString()`);
  add(`new Intl.Locale(${q(tag)}).maximize().baseName`);
}

// 3. Getters de string e de propriedade para 120 locales.
const LOCALES = `en en-US en-GB en-AU en-CA en-IN pt pt-BR pt-PT es es-ES es-MX es-419 fr fr-CA fr-CH de de-AT de-CH
it ja ja-JP ko zh zh-TW zh-CN zh-HK zh-Hant zh-Hans ru ar ar-EG ar-SA he iw fa hi th tr pl nl sv da nb nn fi cs el hu
ro uk vi id in ms bn ta sw fil ur ca sr sr-ME sr-Latn sh az uz kk ky mn ne si my km lo bo am ti so ha yo ig zu xh af
eu gl cy ga gd br sq mk be bs hy ka lt lv et is mt und und-Latn und-US en-u-ca-buddhist en-US-u-ca-japanese
ja-JP-u-ca-japanese th-TH-u-ca-gregory fa-IR-u-ca-persian he-IL-u-ca-hebrew zh-CN-u-ca-chinese en-US-u-hc-h23
pt-BR-u-hc-h12 ja-u-hc-h11 de-u-hc-h24 en-u-nu-thai ar-u-nu-latn ar-EG-u-nu-arab hi-IN-u-nu-deva de-u-co-phonebk
es-u-co-trad zh-u-co-pinyin en-US-u-fw-mon en-u-kf-upper en-u-kn en-u-kn-false de-u-fw-sun`
  .split(/\s+/)
  .slice(0, 120);
const PROPS = ["toString()", "baseName", "language", "script", "region", "calendar", "collation", "hourCycle",
  "caseFirst", "numeric", "numberingSystem", "firstDayOfWeek"];
for (const tag of LOCALES) {
  add(`(function (l) { return [${PROPS.map((prop) => `l.${prop}`).join(", ")}]; })(new Intl.Locale(${q(tag)}))`);
}
// 4. Os getters de método (e as versões antigas) para os mesmos 120 locales, em lotes de dois por programa.
const GETTERS = ["getCalendars", "getCollations", "getHourCycles", "getNumberingSystems", "getTextInfo",
  "getTimeZones", "getWeekInfo"];
const OLD = ["calendars", "collations", "hourCycles", "numberingSystems", "textInfo", "timeZones", "weekInfo"];
for (const tag of LOCALES) {
  add(`(function (l) { return [${GETTERS.map((g) => `typeof l.${g} === "function" ? l.${g}() : "n/a"`).join(", ")}]; })(new Intl.Locale(${q(tag)}))`);
  add(`(function (l) { return [${OLD.map((g) => `l.${g}`).join(", ")}]; })(new Intl.Locale(${q(tag)}))`);
}

// 5. Opções do construtor sobrepondo extensões.
const OPTION_CASES = [
  ["en-u-ca-buddhist", { calendar: "gregory" }], ["en-u-ca-buddhist", { calendar: "japanese" }],
  ["en-u-hc-h23", { hourCycle: "h12" }], ["en-u-hc-h23", { hourCycle: "h11" }], ["en", { hourCycle: "h24" }],
  ["en-u-kf-upper", { caseFirst: "lower" }], ["en-u-kf-upper", { caseFirst: "false" }], ["en", { caseFirst: "upper" }],
  ["en-u-kn", { numeric: false }], ["en-u-kn-false", { numeric: true }], ["en", { numeric: true }],
  ["en", { numeric: "false" }], ["en-u-nu-thai", { numberingSystem: "latn" }], ["en", { numberingSystem: "arab" }],
  ["en-u-co-phonebk", { collation: "emoji" }], ["de", { collation: "phonebk" }], ["en-u-fw-mon", { firstDayOfWeek: "sun" }],
  ["en-u-fw-mon", { firstDayOfWeek: "1" }], ["en-u-fw-mon", { firstDayOfWeek: 0 }], ["en", { firstDayOfWeek: 7 }],
  ["en", { firstDayOfWeek: "tue" }], ["en", { firstDayOfWeek: "wed" }], ["en", { firstDayOfWeek: "thu" }],
  ["en", { firstDayOfWeek: "fri" }], ["en", { firstDayOfWeek: "sat" }], ["en", { firstDayOfWeek: "6" }],
  ["en-US", { language: "pt" }], ["en-US", { script: "Latn" }], ["en-US", { region: "GB" }], ["en-US", { region: "br" }],
  ["en", { language: "FR", script: "latn", region: "ca" }], ["und", { language: "ja" }], ["zh-TW", { script: "Hans" }],
  ["en-US", { variants: "posix" }], ["de", { variants: "1996" }], ["de", { variants: "1996-1901" }],
  ["en-US-u-ca-gregory", { calendar: "chinese", hourCycle: "h23", numeric: true }],
  ["en", { calendar: "islamic-civil" }], ["en", { calendar: "islamicc" }], ["en", { calendar: "ethiopic-amete-alem" }],
  ["en", { calendar: "gregorian" }], ["en", { collation: "dictionary" }], ["en", { numberingSystem: "hanidec" }],
  ["en", { calendar: "gregory-x" }], ["en", { language: "x" }], ["en", { region: "123" }], ["en", { script: "Lat" }],
  ["en", { region: "419" }], ["en", { variants: "abc" }], ["en", { variants: "abcde" }], ["en", { variants: "" }],
  ["en", { calendar: "" }], ["en", { calendar: "a" }], ["en", { calendar: "abcdefghi" }],
  ["en", { hourCycle: "h13" }], ["en", { hourCycle: "H12" }], ["en", { hourCycle: "" }], ["en", { hourCycle: 12 }],
  ["en", { caseFirst: "both" }], ["en", { caseFirst: "" }], ["en", { firstDayOfWeek: "" }], ["en", { firstDayOfWeek: 8 }],
  ["en", { firstDayOfWeek: "xyz" }], ["en", { firstDayOfWeek: "monday" }], ["en", { firstDayOfWeek: -1 }],
  ["en", { numberingSystem: "" }], ["en", { numberingSystem: "abcdefghi" }], ["en", { collation: "a" }],
  ["en", { collation: "standard" }], ["en", { collation: "search" }], ["en", { calendar: undefined }],
  ["en", undefined], ["en", null], ["en", 5], ["en", "str"], ["en", true], ["en", {}],
];
for (const [tag, options] of OPTION_CASES) {
  const o = JSON.stringify(options) ?? "undefined";
  add(`new Intl.Locale(${q(tag)}, ${o}).toString()`);
  add(`(function (l) { return [l.calendar, l.collation, l.hourCycle, l.caseFirst, l.numeric, l.numberingSystem, l.firstDayOfWeek, l.baseName]; })(new Intl.Locale(${q(tag)}, ${o}))`);
}

// 6. Erros do construtor e getters.
const BAD_TAGS = ["", "e", "en_US", "en-", "-en", "en--US", "123", "en-US-", "english", "abcdefghi", "en-Latn-Latn",
  "en-US-US", "en-u", "en-u-", "en-u-ca-", "en-t", "en-x", "en-a", "en-a-b", "en-u-ca-gregory-u-hc-h12",
  "en-x-foo-x-bar", "en-1996-1996", "und-en-US", "en-U", "i-default", "en-GB-oed", "root", "en-u-ca-ca", "en-abcde-abcde",
  " en", "en ", "e n", "ja-é", "en-u-ca-abcdefghi", "x-foo", "*", "en-*", "en-US@calendar=gregory", "en.US", "en-0-aa"];
for (const tag of BAD_TAGS) add(`new Intl.Locale(${q(tag)}).toString()`);
add(`new Intl.Locale()`);
add(`new Intl.Locale(undefined)`);
add(`new Intl.Locale(null)`);
add(`new Intl.Locale(5)`);
add(`new Intl.Locale({})`);
add(`new Intl.Locale({ toString() { return "pt-BR"; } }).toString()`);
add(`new Intl.Locale(new Intl.Locale("de-AT")).toString()`);
add(`new Intl.Locale(new Intl.Locale("de-AT"), { region: "CH" }).toString()`);
add(`Intl.Locale("en")`);
add(`Intl.Locale.length`);
add(`Intl.Locale.name`);
add(`Object.prototype.toString.call(new Intl.Locale("en"))`);
add(`Intl.Locale.prototype[Symbol.toStringTag]`);
add(`Object.getOwnPropertyNames(Intl.Locale.prototype).sort()`);
add(`Intl.Locale.prototype.toString.call({})`);
add(`Object.getOwnPropertyDescriptor(Intl.Locale.prototype, "language").get.call({})`);
add(`Intl.Locale.prototype.maximize.call(5)`);

// 7. Intl.getCanonicalLocales, 150 tags.
const CANON = `EN en-us EN-US en-Us EN-us-POSIX en-latn-us en-LATN-us ZH-hant-tw zh-hans-cn SR-latn-rs iw iw-IL in in-ID
ji ji-US jw jw-ID mo mo-MD sh sh-Latn sh-Cyrl sh-RS tl tl-PH tl-ph no no-NO no-bok nb-NO nn-no ar-arab-eg
und-Latn-DD und-DD en-DD DD en-BU en-ZR en-TP en-YD en-YU en-CS en-SU en-NT en-FX en-AN en-UK en-HV en-RH en-QU en-SF
en-FX-u-ca-gregory pt-br pt-BR pt-PT-u-hc-h23 en-u-hc-h12-ca-gregory en-u-ca-gregory-hc-h12 en-u-hc-h12-hc-h23
en-u-nu-thai-ca-buddhist en-U-CA-Gregory en-t-en-us en-T-EN-US en-t-es-es-h0-hybrid en-t-h0-hybrid en-t-es-h0-hybrid
en-a-bbb-ccc en-b-ccc-a-bbb en-z-zzz-a-aaa-u-ca-gregory en-x-foo en-X-FOO en-x-a-b-c en-u-ca-gregory-x-foo
en-u-kn en-u-kn-true en-u-kn-false en-u-kf en-u-kf-true en-u-kf-false en-u-ks-level1 en-u-ks-level2 en-u-ca-islamicc
en-u-ca-ethiopic-amete-alem en-u-ca-gregorian en-u-co-dictionary en-u-co-direct en-u-co-phonebook en-u-tz-aqams
en-u-tz-cnckg en-u-tz-eire en-u-tz-est en-u-tz-utcw01 en-u-ms-imperial en-u-ms-uksystem en-u-va-posix en-u-rg-uszzzz
en-u-sd-usca en-u-fw-mon en-u-fw-MON en-u-attr-ca-gregory en-u-attr1-attr2 en-u-x en-u-aa-bb-cc
de-1996 de-DE-1996 de-1996-1901 de-1901-1996 de-CH-1901 sl-rozaj sl-rozaj-biske sl-biske-rozaj sl-nedis sl-1994
ca-valencia ca-ES-valencia ca-ES-VALENCIA oc-lengadoc zh-pinyin zh-wadegile zh-Latn-pinyin zh-Latn-wadegile
und und-US und-419 und-Latn-US und-u-ca-gregory und-x-private i-klingon i-ami i-bnn i-hak i-lux i-navajo i-pwn i-tao
i-tay i-tsu art-lojban zh-guoyu zh-hakka zh-xiang zh-min-nan sgn-BE-FR sgn-BE-NL sgn-CH-DE no-bok no-nyn en-GB-oed
en-scouse cel-gaulish zh-min`.split(/\s+/).filter(Boolean);
for (const tag of CANON.slice(0, 150)) add(`Intl.getCanonicalLocales(${q(tag)})`);
add(`Intl.getCanonicalLocales(["en-us", "EN-US", "pt-br", "en-US"])`);
add(`Intl.getCanonicalLocales(["iw", "he", "in", "id"])`);
add(`Intl.getCanonicalLocales()`);
add(`Intl.getCanonicalLocales(undefined)`);
add(`Intl.getCanonicalLocales([])`);
add(`Intl.getCanonicalLocales("")`);
add(`Intl.getCanonicalLocales(null)`);
add(`Intl.getCanonicalLocales(5)`);
add(`Intl.getCanonicalLocales([5])`);
add(`Intl.getCanonicalLocales([null])`);
add(`Intl.getCanonicalLocales([undefined])`);
add(`Intl.getCanonicalLocales(["en_US"])`);
add(`Intl.getCanonicalLocales(["en", "xx-invalid-tag-"])`);
add(`Intl.getCanonicalLocales(new Intl.Locale("de-at"))`);
add(`Intl.getCanonicalLocales([new Intl.Locale("de-at"), "FR-ca"])`);
add(`Intl.getCanonicalLocales({ length: 2, 0: "en-us", 1: "pt-br" })`);
add(`Intl.getCanonicalLocales({ length: 1, 0: {} })`);
add(`Intl.getCanonicalLocales.length`);
add(`Intl.getCanonicalLocales.name`);

// 8. Intl.supportedValuesOf, todas as chaves.
const KEYS = ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"];
for (const key of KEYS) {
  add(`Intl.supportedValuesOf(${q(key)})`);
  add(`Intl.supportedValuesOf(${q(key)}).length`);
}
for (const bad of ["", "locale", "Calendar", "calendars", "numbering", "region", "script", "timezone", "x", "undefined"]) {
  add(`Intl.supportedValuesOf(${q(bad)})`);
}
add(`Intl.supportedValuesOf()`);
add(`Intl.supportedValuesOf(undefined)`);
add(`Intl.supportedValuesOf(null)`);
add(`Intl.supportedValuesOf(5)`);
add(`Intl.supportedValuesOf({ toString() { return "unit"; } }).length`);
add(`Intl.supportedValuesOf.length`);
add(`Intl.supportedValuesOf.name`);

// 9. Completa até 1500 com minimize/maximize em tags geradas por língua x script x região, sem repetição.
const LANGS = ["en", "pt", "es", "fr", "de", "it", "ja", "ko", "zh", "ru", "ar", "hi", "tr", "sr", "az", "uz", "pa", "ha", "ku", "mn"];
const SCRIPTS = ["Latn", "Cyrl", "Arab", "Hans", "Hant", "Deva", "Jpan", "Kore"];
const REGIONS = ["US", "GB", "BR", "PT", "ES", "MX", "FR", "DE", "JP", "KR", "CN", "TW", "RU", "RS", "ME", "IN", "PK", "TR", "EG", "AZ"];
const seen = new Set(programs);
const fill = (expr) => {
  if (programs.length < 1500 && !seen.has(expr)) {
    seen.add(expr);
    programs.push(expr);
  }
};
for (const l of LANGS) {
  for (const s of SCRIPTS) fill(`new Intl.Locale(${q(`${l}-${s}`)}).maximize().toString()`);
}
for (const l of LANGS) {
  for (const r of REGIONS) {
    fill(`new Intl.Locale(${q(`${l}-${r}`)}).maximize().toString()`);
    fill(`new Intl.Locale(${q(`${l}-${r}`)}).minimize().toString()`);
  }
}
for (const l of LANGS) {
  for (const s of SCRIPTS) {
    for (const r of REGIONS) fill(`new Intl.Locale(${q(`${l}-${s}-${r}`)}).minimize().toString()`);
  }
}
if (programs.length < 1500) throw new Error(`só ${programs.length} programas`);
programs.length = Math.min(programs.length, 1500);

const lines = programs.map((expr) => {
  const source = wrap(expr);
  if (/[\t\n\r]/.test(source)) throw new Error(`programa com tabulação ou quebra: ${expr}`);
  let result;
  try {
    result = String(eval(source));
  } catch (e) {
    result = `ERR ${e.name}: ${e.message}`;
  }
  if (/[\t\n\r]/.test(result)) result = JSON.stringify(result);
  return `${expr}\t${result}\n`;
});
writeFileSync(join(ROOT, "tests/golden/locale_more_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("")));
console.log(`${lines.length} programas`);
