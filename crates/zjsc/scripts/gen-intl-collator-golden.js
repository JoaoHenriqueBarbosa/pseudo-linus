// Gera tests/golden/intl_collator_bun.tsv a partir do bun (JavaScriptCore real): Intl.Collator (sensitivity, numeric,
// caseFirst, ignorePunctuation, usage search, collation por locale, resolvedOptions, sort de listas), Intl.Segmenter
// (grapheme, word com isWordLike, sentence), Intl.getCanonicalLocales, supportedValuesOf e Intl.Locale (maximize,
// minimize, getters de calendário e hourCycle). Cada linha é `expressão<TAB>resultado`; as expressões que já existem
// em outros goldens de Intl são descartadas.
//
// Como regenerar (da raiz de wip/zjsc):  bun scripts/gen-intl-collator-golden.js
const fs = require("fs");
const { knownPrograms } = require("./golden-prelude.js");
const path = require("path");

const out = [];
const seen = new Set();
const q = (value) => JSON.stringify(value);
const goldenDir = path.resolve(__dirname, "..", "tests", "golden");
for (const program of knownPrograms("intl_collator_bun.tsv", (name) => !(!/intl|collator|segmenter|locale/.test(name) || name === "intl_collator_bun.tsv"))) seen.add(JSON.stringify(program));

function emit(expression) {
  if (seen.has(expression)) return;
  seen.add(expression);
  let result;
  try {
    result = String(new Function("return " + expression)());
  } catch (error) {
    result = "throw";
  }
  if (/[\t\n\r]/.test(result) || result.includes(String.fromCharCode(0x2028)) || result.includes(String.fromCharCode(0x2029))) result = q(result);
  out.push(expression + "\t" + result);
}

// ---- Collator: comparações por sensibilidade.
const PAIRS = [["a", "A"], ["a", "á"], ["a", "b"], ["á", "A"], ["ä", "a"], ["ss", "ß"], ["e", "é"], ["ç", "c"], ["I", "ı"], ["i", "İ"], ["ñ", "n"], ["å", "z"]];
for (const sensitivity of ["base", "accent", "case", "variant"]) {
  for (const [a, b] of PAIRS) emit(`new Intl.Collator("en", { sensitivity: ${q(sensitivity)} }).compare(${q(a)}, ${q(b)})`);
}
for (const locale of ["sv", "de", "tr"]) {
  for (const [a, b] of [["ä", "z"], ["ö", "o"], ["å", "a"], ["i", "ı"], ["I", "ı"], ["ü", "u"]]) emit(`new Intl.Collator(${q(locale)}).compare(${q(a)}, ${q(b)})`);
}

// ---- numeric.
const NUMERIC = [["a2", "a10"], ["a02", "a2"], ["a1b", "a01b"], ["10", "9"], ["1.5", "1.10"], ["a-2", "a-10"], ["x1y2", "x1y10"], ["007", "7"], ["a", "1"], ["١٢", "٣"]];
for (const [a, b] of NUMERIC) {
  emit(`new Intl.Collator("en", { numeric: true }).compare(${q(a)}, ${q(b)})`);
  emit(`new Intl.Collator("en", { numeric: false }).compare(${q(a)}, ${q(b)})`);
  emit(`new Intl.Collator("en-u-kn").compare(${q(a)}, ${q(b)})`);
}

// ---- caseFirst.
for (const caseFirst of ["upper", "lower", "false"]) {
  for (const [a, b] of [["a", "A"], ["A", "a"], ["b", "A"], ["aB", "Ab"], ["é", "É"], ["ab", "AB"]]) {
    emit(`new Intl.Collator("en", { caseFirst: ${q(caseFirst)} }).compare(${q(a)}, ${q(b)})`);
  }
  emit(`new Intl.Collator("en-u-kf-${caseFirst}").resolvedOptions().caseFirst`);
}

// ---- ignorePunctuation.
for (const [a, b] of [["a-b", "ab"], ["a b", "ab"], ["a.b", "ab"], ["a_b", "ab"], ["a,b", "a;b"], ["(a)", "a"], ["a!", "a"], ["a-b", "ac"]]) {
  emit(`new Intl.Collator("en", { ignorePunctuation: true }).compare(${q(a)}, ${q(b)})`);
  emit(`new Intl.Collator("en", { ignorePunctuation: false }).compare(${q(a)}, ${q(b)})`);
  emit(`new Intl.Collator("th", { ignorePunctuation: true }).compare(${q(a)}, ${q(b)})`);
}

// ---- usage search.
for (const [a, b] of [["a", "á"], ["a", "A"], ["ss", "ß"], ["e", "é"], ["a-b", "ab"]]) {
  emit(`new Intl.Collator("en", { usage: "search" }).compare(${q(a)}, ${q(b)})`);
  emit(`new Intl.Collator("en", { usage: "search", sensitivity: "base" }).compare(${q(a)}, ${q(b)})`);
  emit(`new Intl.Collator("de", { usage: "search" }).compare(${q(a)}, ${q(b)})`);
}

// ---- collation por locale e extensão -u-co-.
const COLLATIONS = [["de", "phonebk"], ["de", "standard"], ["es", "trad"], ["zh", "pinyin"], ["zh", "stroke"], ["zh", "zhuyin"], ["ja", "unihan"],
  ["sv", "reformed"], ["en", "emoji"], ["en", "eor"], ["ko", "searchjl"], ["en", "search"], ["en", "standard"], ["en", "bogus"], ["fr", "standard"]];
for (const [locale, collation] of COLLATIONS) {
  emit(`new Intl.Collator("${locale}-u-co-${collation}").resolvedOptions().collation`);
  emit(`new Intl.Collator("${locale}-u-co-${collation}").resolvedOptions().locale`);
  emit(`new Intl.Collator("${locale}", { collation: ${q(collation)} }).resolvedOptions().collation`);
}
const LOCALE_SORTS = {
  de: ["ä", "a", "z", "o", "ö", "ß", "ss", "u", "ü"],
  sv: ["ä", "a", "z", "ö", "å", "o"],
  es: ["ñ", "n", "o", "ch", "c", "d", "ll", "l", "m"],
  fr: ["côte", "cote", "côté", "coté", "Cote"],
  da: ["aa", "å", "z", "ø", "æ", "a", "ä"],
  nb: ["å", "ø", "æ", "z", "a"],
  fi: ["w", "v", "å", "ä", "ö", "z"],
  pl: ["ł", "l", "m", "ń", "n", "ó", "o", "ż", "z", "ź"],
  cs: ["ch", "c", "h", "i", "č", "d", "ř", "r", "s"],
  hu: ["cs", "c", "d", "dz", "dzs", "e", "ö", "o", "ő"],
  tr: ["ç", "c", "ı", "i", "İ", "I", "ş", "s", "ğ", "g"],
  ru: ["я", "а", "ё", "е", "ж", "Я", "А"],
  el: ["ω", "α", "ά", "Α", "β"],
  ar: ["ب", "ا", "أ", "ت", "ة", "ث"],
  he: ["ב", "א", "ת", "ג"],
  ja: ["あ", "ア", "か", "カ", "ん", "ー", "漢"],
  zh: ["中", "文", "阿", "波", "啊", "字"],
  ko: ["가", "나", "ㄱ", "한", "ㅎ"],
  hi: ["क", "ख", "अ", "आ", "ह"],
  th: ["ก", "ข", "ฮ", "เ", "ไ"],
  pt: ["á", "a", "b", "ç", "c", "A"],
  it: ["è", "e", "é", "à", "a"],
  nl: ["ij", "IJ", "i", "j", "y"],
  vi: ["â", "ă", "a", "đ", "d", "ô", "ơ", "o"],
  uk: ["ї", "і", "и", "й", "г", "ґ"],
};
for (const [locale, list] of Object.entries(LOCALE_SORTS)) {
  const l = q(locale);
  emit(`${q(list)}.slice().sort(new Intl.Collator(${l}).compare).join(",")`);
  emit(`${q(list)}.slice().sort(new Intl.Collator(${l}, { sensitivity: "base" }).compare).join(",")`);
  emit(`${q(list)}.slice().sort(new Intl.Collator(${l}, { caseFirst: "upper" }).compare).join(",")`);
  emit(`${q(list)}.slice().sort(new Intl.Collator(${l}, { numeric: true }).compare).join(",")`);
  emit(`${q(list)}.slice().sort((a, b) => a.localeCompare(b, ${l})).join(",")`);
  emit(`${q(list)}.slice().sort(new Intl.Collator(${l}).compare).reverse().join(",")`);
  emit(`new Intl.Collator(${l}).resolvedOptions().locale`);
}

// ---- resolvedOptions e supportedLocalesOf.
const OPTION_SETS = [
  "{}", `{ sensitivity: "base" }`, `{ sensitivity: "accent" }`, `{ sensitivity: "case" }`, `{ numeric: true }`, `{ caseFirst: "upper" }`,
  `{ caseFirst: "lower" }`, `{ ignorePunctuation: true }`, `{ usage: "search" }`, `{ usage: "sort" }`, `{ sensitivity: "bogus" }`, `{ usage: "bogus" }`,
  `{ caseFirst: "bogus" }`, `{ numeric: "yes" }`, `{ collation: "phonebk" }`, `{ localeMatcher: "lookup" }`, `{ localeMatcher: "bogus" }`,
];
for (const options of OPTION_SETS) {
  for (const locale of ["en", "de", "sv", "th", "tr", "zh-u-co-pinyin"]) {
    emit(`JSON.stringify(new Intl.Collator(${q(locale)}, ${options}).resolvedOptions())`);
  }
}
emit(`JSON.stringify(Object.getOwnPropertyNames(new Intl.Collator().resolvedOptions()))`);
emit(`Object.prototype.toString.call(new Intl.Collator())`);
emit(`typeof new Intl.Collator().compare`);
emit(`new Intl.Collator().compare === new Intl.Collator().compare`);
emit(`(function () { const c = new Intl.Collator(); return c.compare === c.compare })()`);
emit(`new Intl.Collator().compare.name === ""`);
emit(`new Intl.Collator().compare.length`);
emit(`JSON.stringify(Intl.Collator.supportedLocalesOf(["en", "de-DE", "xx", "zh-Hant", "sv-u-co-reformed"]))`);
emit(`JSON.stringify(Intl.Collator.supportedLocalesOf("fr", { localeMatcher: "lookup" }))`);
emit(`Intl.Collator.length`);
emit(`Intl.Collator().compare("a", "b")`);

// ---- Segmenter: grapheme.
const GRAPHEMES = {
  zwj: "👨‍👩‍👧‍👦", flags: "🇧🇷🇺🇸🇯🇵", flagOdd: "🇧🇷🇺", skin: "👍🏽", keycap: "1️⃣", combining: "é́", hangul: "한국어", hangulJamo: "ᄒ​ᅡᆫ",
  crlf: "a\r\nb", indic: "क्षि", thai: "สวัสดี", tagFlag: "🏴󠁧󠁢󠁥󠁮󠁧󠁿", emojiMix: "a👩🏽‍💻b", virama: "नमस्ते", lone: "́a", empty: "", sp: "a b", arabic: "مرحبا",
};
for (const [name, text] of Object.entries(GRAPHEMES)) {
  const t = q(text);
  emit(`Array.from(new Intl.Segmenter("en").segment(${t}), s => s.segment).join("|")`);
  emit(`Array.from(new Intl.Segmenter("en", { granularity: "grapheme" }).segment(${t}), s => s.index).join(",")`);
  emit(`Array.from(new Intl.Segmenter("en").segment(${t})).length`);
  emit(`Array.from(new Intl.Segmenter("ja").segment(${t}), s => s.segment.length).join(",")`);
}

// ---- Segmenter: word.
const WORDS = [
  "Hello, world! It's 3.14 o'clock.", "foo_bar baz-qux", "e-mail user@example.com", "日本語のテキストです", "你好，世界！", "สวัสดีครับ ผมชื่อ", "Привет, мир!", "안녕하세요 세계",
  "مرحبا بالعالم", "שלום עולם", "नमस्ते दुनिया", "1,234.56 and 7:30", "can't won’t", "a.b.c", "🙂 smile 😀", "naïve café", "x y", "tab\tsep", "ひらがなカタカナ", "ＡＢＣ１２３",
  "hello\nworld", "  lead", "trail  ", "snake_case_id", "3rd 4th", "version 1.2.3-beta", "O'Brien", "ç‿d",
];
for (const text of WORDS) {
  const t = q(text);
  for (const locale of ["en", "ja", "th"]) {
    emit(`Array.from(new Intl.Segmenter(${q(locale)}, { granularity: "word" }).segment(${t}), s => s.segment).join("|")`);
    emit(`Array.from(new Intl.Segmenter(${q(locale)}, { granularity: "word" }).segment(${t}), s => s.isWordLike ? 1 : 0).join("")`);
  }
}
emit(`Array.from(new Intl.Segmenter("en", { granularity: "word" }).segment("ab cd"), s => s.index).join(",")`);
emit(`Array.from(new Intl.Segmenter("en", { granularity: "word" }).segment("ab cd"), s => s.input).join(",")`);
emit(`JSON.stringify(new Intl.Segmenter("en", { granularity: "word" }).segment("ab cd").containing(3))`);
emit(`JSON.stringify(new Intl.Segmenter("en", { granularity: "word" }).segment("ab cd").containing(2))`);
emit(`JSON.stringify(new Intl.Segmenter("en", { granularity: "word" }).segment("ab cd").containing(99))`);
emit(`JSON.stringify(new Intl.Segmenter("en", { granularity: "word" }).segment("ab cd").containing(-1))`);
emit(`JSON.stringify(new Intl.Segmenter("en").segment("🇧🇷x").containing(1))`);
emit(`JSON.stringify(new Intl.Segmenter("en").segment("abc").containing())`);

// ---- Segmenter: sentence.
const SENTENCES = [
  "Hello world. How are you? I am fine!", "Dr. Smith went home. He slept.", "Mr. and Mrs. Jones arrived at 5 p.m. today.", "Wait... what? No!", "One.\nTwo.\n\nThree.",
  "He said \"Stop.\" Then left.", "3.14 is pi. 2.71 is e.", "日本語です。これは文です。", "你好。世界！再见？", "Привет. Как дела? Хорошо!", "a.b.c. Next one.", "Is it U.S.A. or USA? Both.",
  "no terminator", "", "...", "Hi!!! Really?! Yes.", "Line one Line two Line three", "e.g. this one. And that.", "ΠΡΟΣΟΧΉ; Επόμενη πρόταση.", "सुनो। आगे चलो।",
];
for (const text of SENTENCES) {
  const t = q(text);
  for (const locale of ["en", "ja", "de"]) {
    emit(`Array.from(new Intl.Segmenter(${q(locale)}, { granularity: "sentence" }).segment(${t}), s => s.segment).join("|")`);
  }
  emit(`Array.from(new Intl.Segmenter("en", { granularity: "sentence" }).segment(${t}), s => s.index).join(",")`);
}
for (const options of [`{}`, `{ granularity: "word" }`, `{ granularity: "sentence" }`, `{ granularity: "bogus" }`, `{ localeMatcher: "lookup" }`, `{ localeMatcher: "bogus" }`]) {
  for (const locale of ["en", "ja-JP", "th", "zz"]) emit(`JSON.stringify(new Intl.Segmenter(${q(locale)}, ${options}).resolvedOptions())`);
}
emit(`Object.prototype.toString.call(new Intl.Segmenter())`);
emit(`Object.prototype.toString.call(new Intl.Segmenter().segment("a"))`);
emit(`Object.prototype.toString.call(new Intl.Segmenter().segment("a")[Symbol.iterator]())`);
emit(`JSON.stringify(Intl.Segmenter.supportedLocalesOf(["en", "ja", "xx", "th-TH"]))`);
emit(`typeof new Intl.Segmenter().segment("x")[Symbol.iterator]`);
emit(`Intl.Segmenter.length`);
emit(`Intl.Segmenter("en")`);
emit(`new Intl.Segmenter("en").segment(Symbol())`);
emit(`Array.from(new Intl.Segmenter("en").segment(12345), s => s.segment).join("|")`);
emit(`Array.from(new Intl.Segmenter("en").segment(undefined), s => s.segment).join("|")`);
emit(`JSON.stringify(Array.from(new Intl.Segmenter("en", { granularity: "word" }).segment("hi there"))[0])`);
emit(`JSON.stringify(Array.from(new Intl.Segmenter("en", { granularity: "sentence" }).segment("hi there"))[0])`);
emit(`Object.keys(Array.from(new Intl.Segmenter("en", { granularity: "word" }).segment("hi"))[0]).join()`);
emit(`Object.keys(Array.from(new Intl.Segmenter("en").segment("hi"))[0]).join()`);

// ---- Intl.getCanonicalLocales.
const TAGS = [
  "EN-us", "en_US", "en-us-u-ca-gregory", "en-u-ca-gregory-nu-latn", "en-u-nu-latn-ca-gregory", "iw", "in", "ji", "jw", "mo", "sh", "tl", "no-bok", "zh-cmn-Hans-CN", "sgn-BR", "i-klingon",
  "art-lojban", "en-GB-oed", "de-DE-1996", "de-1996-DE", "ca-ES-valencia", "sl-rozaj-biske", "en-a-bbb-x-a-ccc", "en-x-private", "x-whatever", "und", "und-Latn", "zh-hans", "zh-TW", "zh-Hant-HK",
  "sr-latn-rs", "en-t-hi-latn", "en-t-m0-ungegn-2007", "ja-JP-u-ca-japanese", "th-TH-u-ca-buddhist-nu-thai", "ar-u-nu-arab", "en-u-hc-h23", "en-u-ks-level1", "en-u-co-phonebk", "en-US-u-va-posix",
  "en-u-ca-islamicc", "en-u-ca-ethiopic-amete-alem", "en-u-tz-usnyc", "en-u-kn-true", "en-u-kn", "en-u-kf-upper", "pt-BR", "pt-br", "ES-419", "es-419", "en-001", "fr-CA", "FR-ca",
  "en-", "en--US", "-en", "e", "toolongtag-x", "en-us-", "123", "en-Latn-Latn", "en-US-US", "en-u", "en-u-", "en-u-a", "en-a", "", "en-u-ca-gregory-u-nu-latn", "en-a-bb-a-cc",
  "en-x-", "root", "EN", "eN-uS", "en-U-CA-GREGORY", "ZH-HANS-cn", "de-u-co-phonebk-kn", "iw-IL", "he-IL", "no-NO-nynorsk", "nn", "cmn", "zh-yue", "yue-HK", "pa-Arab-PK", "uz-Cyrl", "az-Latn-AZ",
];
for (const tag of TAGS) emit(`Intl.getCanonicalLocales(${q(tag)}).join(",")`);
emit(`Intl.getCanonicalLocales(["en-US", "EN-us", "pt-br", "en"]).join(",")`);
emit(`Intl.getCanonicalLocales(["b", "a", "b"]).join(",")`);
emit(`Intl.getCanonicalLocales().length`);
emit(`Intl.getCanonicalLocales(undefined).length`);
emit(`Intl.getCanonicalLocales(null).length`);
emit(`Intl.getCanonicalLocales([]).length`);
emit(`Intl.getCanonicalLocales([1]).length`);
emit(`Intl.getCanonicalLocales([undefined]).length`);
emit(`Intl.getCanonicalLocales(new Intl.Locale("en-us")).join(",")`);
emit(`Intl.getCanonicalLocales({ length: 2, 0: "en", 1: "pt-br" }).join(",")`);
emit(`Intl.getCanonicalLocales(42).length`);
emit(`Intl.getCanonicalLocales.length`);
emit(`Intl.getCanonicalLocales.name`);

// ---- supportedValuesOf.
for (const key of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit"]) {
  emit(`Intl.supportedValuesOf(${q(key)}).length > 5`);
  emit(`Intl.supportedValuesOf(${q(key)}).slice(0, 8).join(",")`);
  emit(`Intl.supportedValuesOf(${q(key)}).slice(-5).join(",")`);
  emit(`(function () { const v = Intl.supportedValuesOf(${q(key)}); return v.every((x, i) => i === 0 || v[i - 1] < x) })()`);
  emit(`Array.isArray(Intl.supportedValuesOf(${q(key)}))`);
  emit(`Intl.supportedValuesOf(${q(key)}) === Intl.supportedValuesOf(${q(key)})`);
}
for (const bad of ["", "locale", "region", "Calendar", "timezone", "numbering", undefined, null, 1]) emit(`Intl.supportedValuesOf(${q(bad === undefined ? null : bad)})`);
emit(`Intl.supportedValuesOf()`);
emit(`Intl.supportedValuesOf.length`);
emit(`Intl.supportedValuesOf("calendar").includes("gregory")`);
emit(`Intl.supportedValuesOf("calendar").includes("islamic-umalqura")`);
emit(`Intl.supportedValuesOf("calendar").includes("islamicc")`);
emit(`Intl.supportedValuesOf("collation").includes("standard")`);
emit(`Intl.supportedValuesOf("collation").includes("search")`);
emit(`Intl.supportedValuesOf("currency").includes("BRL")`);
emit(`Intl.supportedValuesOf("numberingSystem").includes("latn")`);
emit(`Intl.supportedValuesOf("timeZone").includes("America/Sao_Paulo")`);
emit(`Intl.supportedValuesOf("timeZone").includes("UTC")`);
emit(`Intl.supportedValuesOf("unit").includes("kilometer-per-hour")`);

// ---- Intl.Locale: maximize e minimize.
const LOCALES = [
  "en", "pt", "zh", "zh-TW", "zh-Hant", "zh-HK", "sr", "sr-ME", "ja", "ko", "ar", "he", "ru", "uk", "hi", "bn", "th", "vi", "tr", "de", "fr", "es", "it", "nl", "sv", "pl", "cs", "el", "fa", "ur",
  "id", "ms", "fil", "sw", "am", "ha", "yo", "zu", "az", "uz", "kk", "mn", "ne", "si", "my", "km", "und", "und-BR", "und-Cyrl", "und-419", "en-AU", "en-001", "pt-PT", "es-MX", "fr-CH", "de-AT",
  "en-Latn-US", "zh-Hans-CN", "sr-Latn", "pa", "pa-PK", "ku", "ky", "tg", "ps", "sd", "yue", "iw", "in", "no", "nb", "nn", "ca-ES-valencia", "en-u-ca-gregory", "ja-JP-u-hc-h23", "xx", "tlh", "und-Hant",
];
for (const tag of LOCALES) {
  const t = q(tag);
  emit(`new Intl.Locale(${t}).maximize().toString()`);
  emit(`new Intl.Locale(${t}).minimize().toString()`);
  emit(`new Intl.Locale(${t}).maximize().minimize().toString()`);
  emit(`new Intl.Locale(${t}).minimize().maximize().toString()`);
}

// ---- Intl.Locale: getters de calendário e hourCycle.
const CAL_TAGS = [
  "en", "en-US", "ja-JP-u-ca-japanese", "th-TH-u-ca-buddhist", "ar-SA-u-ca-islamic", "fa-IR-u-ca-persian", "he-IL-u-ca-hebrew", "zh-CN-u-ca-chinese", "ko-KR-u-ca-dangi", "en-u-ca-iso8601",
  "en-u-ca-gregory", "en-u-ca-bogus", "en-u-ca-islamic-civil", "en-u-ca-roc", "en-u-ca-coptic", "en-u-ca-indian", "en-u-hc-h11", "en-u-hc-h12", "en-u-hc-h23", "en-u-hc-h24", "en-u-hc-bogus",
  "ja-u-hc-h11", "en-u-nu-arab", "en-u-nu-thai", "en-u-co-phonebk", "de-u-co-phonebk-kn-kf-upper", "en-u-kn-false", "en-u-kn-true", "en-u-kf-lower", "en-u-kf-false", "en-u-fw-mon", "en-u-fw-sun", "en-u-ca-gregory-hc-h12-nu-latn",
];
const GETTERS = ["calendar", "calendars", "collation", "collations", "hourCycle", "hourCycles", "numberingSystem", "numberingSystems", "caseFirst", "numeric", "language", "script", "region", "baseName", "firstDayOfWeek"];
for (const tag of CAL_TAGS) {
  for (const getter of GETTERS) emit(`String(new Intl.Locale(${q(tag)}).${getter})`);
}
const BASES = ["en", "ja", "ar", "de", "th"];
const CTOR_OPTIONS = [
  `{ calendar: "buddhist" }`, `{ calendar: "bogus" }`, `{ calendar: "islamic-civil" }`, `{ hourCycle: "h11" }`, `{ hourCycle: "h12" }`, `{ hourCycle: "h23" }`, `{ hourCycle: "h24" }`, `{ hourCycle: "x" }`,
  `{ collation: "phonebk" }`, `{ numberingSystem: "arab" }`, `{ numeric: true }`, `{ caseFirst: "upper" }`, `{ firstDayOfWeek: "mon" }`, `{ firstDayOfWeek: 7 }`,
];
for (const base of BASES) {
  for (const options of CTOR_OPTIONS) {
    emit(`new Intl.Locale(${q(base)}, ${options}).toString()`);
    emit(`new Intl.Locale(${q(base + "-u-ca-gregory-hc-h23")}, ${options}).toString()`);
  }
}
for (const tag of ["en", "en-US", "ar-EG", "ja", "de-DE", "pt-BR", "he", "fa", "th", "ru", "zh-TW", "hi", "tr"]) {
  const t = q(tag);
  emit(`JSON.stringify(new Intl.Locale(${t}).getCalendars())`);
  emit(`JSON.stringify(new Intl.Locale(${t}).getCollations())`);
  emit(`JSON.stringify(new Intl.Locale(${t}).getHourCycles())`);
  emit(`JSON.stringify(new Intl.Locale(${t}).getNumberingSystems())`);
  emit(`JSON.stringify(new Intl.Locale(${t}).getTextInfo())`);
  emit(`JSON.stringify(new Intl.Locale(${t}).getWeekInfo())`);
  emit(`JSON.stringify(new Intl.Locale(${t}).getTimeZones())`);
}
emit(`Object.prototype.toString.call(new Intl.Locale("en"))`);
emit(`Intl.Locale("en")`);
emit(`new Intl.Locale()`);
emit(`new Intl.Locale(5)`);
emit(`new Intl.Locale("")`);
emit(`new Intl.Locale(new Intl.Locale("pt-BR")).toString()`);
emit(`new Intl.Locale("en").toString === Intl.Locale.prototype.toString`);
emit(`Intl.Locale.length`);

const file = path.resolve(__dirname, "..", "tests", "golden", "intl_collator_bun.tsv");
fs.writeFileSync(file, out.join("\n") + "\n");
console.log(out.length + " linhas em tests/golden/intl_collator_bun.tsv");
