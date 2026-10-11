// Gera tests/golden/intl_extra_bun.tsv: Intl.Segmenter (grapheme, word, sentence, emoji ZWJ, CJK, tailandês, bandeiras,
// isWordLike, containing, iteração), Intl.ListFormat (type x style x locales), Intl.PluralRules (cardinal, ordinal,
// selectRange, resolvedOptions, ar, ru, pl, cy, ja), Intl.Locale (maximize, minimize, getters, getWeekInfo, getTextInfo,
// getHourCycles, getCalendars, getNumberingSystems, getCollations, getTimeZones) e Intl.DurationFormat, medidos no bun.
// Complementa (sem repetir) gen-intl-golden.js, gen-intl-collator-golden.js, gen-intl-misc-golden.js e gen-intl-more-golden.js.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-intl-extra-golden.js > tests/golden/intl_extra_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const q = JSON.stringify;
// Programa que grava em R o texto da expressão, ou `Nome: mensagem` quando lança.
const P = expr => programs.push(`try { R = String(${expr}) } catch (e) { R = e.name + ': ' + e.message }`);
// Variante que serializa o valor em JSON.
const J = expr => programs.push(`try { R = JSON.stringify(${expr}) } catch (e) { R = e.name + ': ' + e.message }`);

// ---- Intl.Segmenter.
const texts = {
  family: "a\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}b",
  skin: "\u{1F44D}\u{1F3FD}x\u{1F44B}",
  flags: "\u{1F1E7}\u{1F1F7}\u{1F1FA}\u{1F1F8}\u{1F1EF}\u{1F1F5}\u{1F1E9}",
  keycap: "1️⃣a#️⃣",
  combining: "éạ̀o",
  hangul: "한가가",
  crlf: "a\r\nb\n\rc",
  cjk: "今日はいい天気ですね。明日も晴れ。",
  katakana: "カタカナとひらがな",
  thai: "สวัสดีครับทุกคน",
  thai2: "ผมรักคุณ hello นะ",
  words: "Hello, world! It's 3.14 o'clock; e-mail: a@b.com.",
  numbers: "1,234.56 and 7:30 or 1_000",
  sentences: "Hello world. How are you? Fine! Mr. Smith went to Washington. Dr. J. went too.",
  sentences2: "Ele disse: “Olá.” Depois saiu. E voltou?! Sim...",
  quote: "\"Stop!\" he said. Next one.",
  empty: "",
  spaces: "  a  b  ",
  surrogate: "\ud83d",
  lone: "a\ude00b",
  emoji: "❤️\u{1F9D1}‍\u{1F4BB}\u{1F600}",
  arabic: "مرحبا بالعالم.",
  latin: "café naïve über",
  hindi: "नमस्ते दुनिया",
  mixed: "abc中文defこんにちは123",
};
const granularities = ["grapheme", "word", "sentence"];
const seg = (locale, g) => `new Intl.Segmenter(${q(locale)}, { granularity: ${q(g)} })`;
for (const [name, text] of Object.entries(texts)) {
  const t = q(text);
  for (const g of granularities) {
    const locales = name === "cjk" || name === "katakana" ? ["en", "ja", "zh"] : name.startsWith("thai") ? ["en", "th"] : ["en"];
    for (const locale of locales) {
      P(`Array.from(${seg(locale, g)}.segment(${t}), s => s.segment).join('|')`);
      P(`Array.from(${seg(locale, g)}.segment(${t}), s => s.index).join(',')`);
    }
    J(`Array.from(${seg("en", g)}.segment(${t}), s => s.isWordLike)`);
    P(`Array.from(${seg("en", g)}.segment(${t})).length`);
  }
}
for (const name of ["words", "numbers", "mixed", "cjk", "thai", "family"]) {
  J(`Array.from(${seg("en", "word")}.segment(${q(texts[name])})).map(s => [s.segment, s.index, s.isWordLike, s.input === ${q(texts[name])}])`);
}
// containing
for (const name of ["family", "words", "cjk", "sentences", "flags"]) {
  const t = q(texts[name]);
  const len = texts[name].length;
  for (const g of granularities) {
    for (const i of [0, 1, 2, 3, 5, len - 1, len, len + 1, -1]) {
      J(`${seg("en", g)}.segment(${t}).containing(${i})`);
    }
  }
}
for (const arg of ["undefined", "NaN", "1.7", "'2'", "null", "Infinity", "-Infinity", "{}", "true"]) {
  J(`new Intl.Segmenter('en').segment('abcdef').containing(${arg})`);
}
// iteração e protocolo
P(`Object.prototype.toString.call(new Intl.Segmenter().segment('a'))`);
P(`Object.getPrototypeOf(new Intl.Segmenter().segment('a'))[Symbol.iterator].name`);
P(`typeof new Intl.Segmenter().segment('a')[Symbol.iterator]`);
P(`Object.prototype.toString.call(new Intl.Segmenter().segment('abc')[Symbol.iterator]())`);
P(`(() => { const it = new Intl.Segmenter().segment('ab')[Symbol.iterator](); return JSON.stringify([it.next(), it.next(), it.next(), it.next()]) })()`);
P(`(() => { const it = new Intl.Segmenter().segment('ab')[Symbol.iterator](); return it[Symbol.iterator]() === it })()`);
P(`Object.keys(new Intl.Segmenter().segment('a'))`);
P(`Object.getOwnPropertyNames(Object.getPrototypeOf(new Intl.Segmenter().segment('a'))).join()`);
P(`Object.keys(Array.from(new Intl.Segmenter('en', {granularity:'word'}).segment('a b'))[0]).join()`);
P(`Object.keys(Array.from(new Intl.Segmenter('en').segment('a b'))[0]).join()`);
P(`Array.from(new Intl.Segmenter().segment(12345), s => s.segment).join('|')`);
P(`Array.from(new Intl.Segmenter().segment(undefined), s => s.segment).join('|')`);
P(`Array.from(new Intl.Segmenter().segment(null), s => s.segment).join('|')`);
P(`new Intl.Segmenter().segment(Symbol())`);
P(`Array.from(new Intl.Segmenter().segment({ toString() { return 'xy' } }), s => s.segment).join('|')`);
P(`[...new Intl.Segmenter('en', {granularity:'word'}).segment('a b')].length`);
P(`(() => { let n = 0; for (const s of new Intl.Segmenter('en', {granularity:'word'}).segment('one two three')) if (s.isWordLike) n++; return n })()`);
P(`Intl.Segmenter.prototype.segment.call({}, 'a')`);
P(`Intl.Segmenter.prototype.resolvedOptions.call({})`);
P(`Intl.Segmenter()`);
P(`new Intl.Segmenter('en', { granularity: 'line' })`);
P(`new Intl.Segmenter('en', { granularity: undefined }).resolvedOptions().granularity`);
P(`new Intl.Segmenter('en', null)`);
P(`JSON.stringify(new Intl.Segmenter('pt-BR', { granularity: 'word' }).resolvedOptions())`);
P(`JSON.stringify(new Intl.Segmenter(['xx', 'th-TH']).resolvedOptions())`);
P(`JSON.stringify(Intl.Segmenter.supportedLocalesOf(['en', 'th', 'xx-invalid-', 'ja-JP']))`);
P(`Intl.Segmenter.length + ',' + Intl.Segmenter.name + ',' + Intl.Segmenter.prototype.segment.length`);
P(`Intl.Segmenter.prototype[Symbol.toStringTag]`);
P(`Array.from(new Intl.Segmenter('en', {granularity:'sentence'}).segment('A. B. C.'), s => s.segment).join('|')`);
for (const s of ["Hello world", "a.b", "U.S.A. is big", "Wait... what?", "x\ny", "x\n\ny", "a b", "こんにちは。さようなら！", "1. one 2. two", "e.g. this", "a;b:c", "A? b. C"]) {
  P(`Array.from(${seg("en", "sentence")}.segment(${q(s)}), s => s.segment).join('|')`);
  P(`Array.from(${seg("en", "word")}.segment(${q(s)}), s => s.segment + (s.isWordLike ? '+' : '-')).join('|')`);
}
for (const loc of ["ja", "zh", "ko", "th", "de", "fr", "ar", "ru"]) {
  P(`JSON.stringify(Array.from(new Intl.Segmenter(${q(loc)}, {granularity:'word'}).segment('abc 今日 สวัสดี привет'), s => s.segment))`);
}

// ---- Intl.ListFormat.
const listLocales = ["en", "en-GB", "pt", "pt-PT", "es", "fr", "de", "it", "ja", "zh", "zh-Hant", "ko", "ar", "ru", "pl", "tr", "he", "hi", "th", "nl", "sv", "fi", "cs", "uk", "cy", "id", "vi", "el"];
const lists = [[], ["A"], ["A", "B"], ["A", "B", "C"], ["A", "B", "C", "D", "E"]];
for (const loc of listLocales) {
  for (const type of ["conjunction", "disjunction", "unit"]) {
    for (const style of ["long", "short", "narrow"]) {
      const lf = `new Intl.ListFormat(${q(loc)}, { type: ${q(type)}, style: ${q(style)} })`;
      for (const n of [2, 3]) P(`${lf}.format(${q(lists[n])})`);
    }
  }
  J(`new Intl.ListFormat(${q(loc)}).formatToParts(['x', 'y', 'z'])`);
  J(`new Intl.ListFormat(${q(loc)}, { type: 'unit', style: 'narrow' }).formatToParts(['1', '2'])`);
}
for (const l of lists) P(`new Intl.ListFormat('en').format(${q(l)})`);
for (const l of lists) J(`new Intl.ListFormat('en', {type:'disjunction'}).formatToParts(${q(l)})`);
P(`new Intl.ListFormat('en').format(new Set(['a', 'b', 'c']))`);
P(`new Intl.ListFormat('en').format('abc')`);
P(`new Intl.ListFormat('en').format([1, 2])`);
P(`new Intl.ListFormat('en').format(['a', undefined])`);
P(`new Intl.ListFormat('en').format(undefined)`);
P(`new Intl.ListFormat('en').format(null)`);
P(`new Intl.ListFormat('en').format(5)`);
P(`new Intl.ListFormat('en').format({ length: 2, 0: 'a', 1: 'b' })`);
P(`new Intl.ListFormat('en').format((function* () { yield 'x'; yield 'y' })())`);
P(`new Intl.ListFormat('en', { type: 'bad' })`);
P(`new Intl.ListFormat('en', { style: 'bad' })`);
P(`ListFormat`);
P(`Intl.ListFormat('en')`);
P(`JSON.stringify(new Intl.ListFormat('de', { type: 'unit', style: 'short' }).resolvedOptions())`);
P(`JSON.stringify(new Intl.ListFormat().resolvedOptions().locale.length > 0)`);
P(`JSON.stringify(new Intl.ListFormat('xx').resolvedOptions())`);
P(`JSON.stringify(Intl.ListFormat.supportedLocalesOf(['en', 'pt-BR', 'zz']))`);
P(`Object.prototype.toString.call(new Intl.ListFormat())`);
P(`Intl.ListFormat.prototype.format.call({}, [])`);
P(`typeof Object.getOwnPropertyDescriptor(Intl.ListFormat.prototype, 'format')`);
P(`Intl.ListFormat.length + Intl.ListFormat.prototype.format.length + Intl.ListFormat.prototype.formatToParts.length`);
P(`new Intl.ListFormat('es', {type:'disjunction'}).format(['uno','otro'])`);
P(`new Intl.ListFormat('es').format(['hijo', 'inglés'])`);
P(`new Intl.ListFormat('es', {type:'disjunction'}).format(['siete', 'ocho'])`);
P(`new Intl.ListFormat('es').format(['Pedro', 'Iván'])`);
P(`new Intl.ListFormat('he').format(['a', 'b', 'c'])`);

// ---- Intl.PluralRules.
const pluralLocales = ["en", "ar", "ru", "pl", "cy", "ja", "fr", "pt", "es", "de", "he", "uk", "cs", "lt", "lv", "ga", "gd", "sl", "mt", "ro", "ko", "zh", "hi", "tr", "it", "ca", "br", "kw", "mk", "sk", "ksh", "da", "sv", "is", "fil"];
const nums = [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 20, 21, 22, 25, 100, 101, 102, 111, 112, 1000, 0.5, 1.5, 2.5, -1, -2, 1e6, "1.0", "1.00"];
for (const loc of pluralLocales) {
  P(`JSON.stringify(new Intl.PluralRules(${q(loc)}).resolvedOptions().pluralCategories)`);
  P(`JSON.stringify(new Intl.PluralRules(${q(loc)}, {type:'ordinal'}).resolvedOptions().pluralCategories)`);
  const list = ["ar", "ru", "pl", "cy", "ja", "en", "fr", "ga"].includes(loc) ? nums : nums.slice(0, 20);
  P(`${q(list)}.map(n => new Intl.PluralRules(${q(loc)}).select(n)).join()`);
  P(`${q(list)}.map(n => new Intl.PluralRules(${q(loc)}, {type:'ordinal'}).select(n)).join()`);
}
for (const loc of ["en", "ar", "ru", "pl", "cy", "ja"]) {
  for (const [a, b] of [[0, 1], [1, 2], [1, 5], [2, 11], [0, 0], [1, 1], [3, 21], [1, 100], [0.5, 1.5], [5, 6], [10, 20], [21, 22]]) {
    P(`new Intl.PluralRules(${q(loc)}).selectRange(${a}, ${b})`);
    P(`new Intl.PluralRules(${q(loc)}, {type:'ordinal'}).selectRange(${a}, ${b})`);
  }
}
P(`new Intl.PluralRules('en').selectRange(1)`);
P(`new Intl.PluralRules('en').selectRange(undefined, 1)`);
P(`new Intl.PluralRules('en').selectRange(1, undefined)`);
P(`new Intl.PluralRules('en').selectRange(NaN, 1)`);
P(`new Intl.PluralRules('en').selectRange('1', '2')`);
P(`new Intl.PluralRules('en').selectRange(2, 1)`);
P(`new Intl.PluralRules('en').select()`);
P(`new Intl.PluralRules('en').select(NaN)`);
P(`new Intl.PluralRules('en').select(Infinity)`);
P(`new Intl.PluralRules('en').select('abc')`);
P(`new Intl.PluralRules('en').select(10n)`);
P(`new Intl.PluralRules('en').select(Symbol())`);
for (const opts of [
  "{ minimumFractionDigits: 2 }", "{ maximumFractionDigits: 0 }", "{ minimumIntegerDigits: 3 }", "{ minimumSignificantDigits: 3 }",
  "{ maximumSignificantDigits: 1 }", "{ type: 'ordinal', minimumFractionDigits: 1 }", "{ roundingMode: 'floor', maximumFractionDigits: 0 }",
  "{ roundingIncrement: 5, maximumFractionDigits: 2, minimumFractionDigits: 2 }", "{ trailingZeroDisplay: 'stripIfInteger', minimumFractionDigits: 1 }",
  "{ roundingPriority: 'morePrecision', maximumSignificantDigits: 2, maximumFractionDigits: 1 }",
]) {
  P(`JSON.stringify(new Intl.PluralRules('en', ${opts}).resolvedOptions())`);
  P(`JSON.stringify(new Intl.PluralRules('ar', ${opts}).resolvedOptions())`);
  P(`[0, 1, 1.5, 2, 21, 100.25].map(n => new Intl.PluralRules('en', ${opts}).select(n)).join()`);
  P(`[0, 1, 1.5, 2, 3, 11, 100.25].map(n => new Intl.PluralRules('ru', ${opts}).select(n)).join()`);
}
P(`new Intl.PluralRules('en', { type: 'bad' })`);
P(`new Intl.PluralRules('en', { minimumFractionDigits: 11 })`);
P(`new Intl.PluralRules('en', { maximumFractionDigits: 1, minimumFractionDigits: 3 })`);
P(`Intl.PluralRules('en')`);
P(`JSON.stringify(new Intl.PluralRules('pt-BR').resolvedOptions())`);
P(`JSON.stringify(new Intl.PluralRules('xx').resolvedOptions())`);
P(`JSON.stringify(Intl.PluralRules.supportedLocalesOf(['ar', 'ru-RU', 'cy', 'zz']))`);
P(`Intl.PluralRules.length + ',' + Intl.PluralRules.prototype.select.length + ',' + Intl.PluralRules.prototype.selectRange.length`);
P(`Object.getOwnPropertyNames(Intl.PluralRules.prototype).sort().join()`);
P(`Intl.PluralRules.prototype.select.call({}, 1)`);
P(`Intl.PluralRules.prototype[Symbol.toStringTag]`);

// ---- Intl.Locale.
const tags = ["en", "pt", "zh", "sr", "und", "und-Latn", "und-Cyrl", "und-Hant", "und-JP", "en-US", "en-GB", "pt-BR", "pt-PT", "zh-TW", "zh-HK", "zh-Hans-CN", "ja", "ar", "ar-EG", "he", "hi", "ru", "uk", "sr-Latn", "sr-ME", "az", "uz", "ku", "mn", "pa", "ks", "ug", "es", "es-419", "fr-CA", "de-CH", "yue", "nb", "no", "tl", "fil", "iw", "in", "sh", "und-AQ", "xx", "en-Latn-US-u-ca-gregory", "th-TH-u-nu-thai", "en-u-hc-h23", "ja-JP-u-ca-japanese", "de-u-co-phonebk", "zh-u-co-pinyin", "ar-u-nu-latn", "fa-AF", "ps", "ti", "am", "ko-KR", "vi", "tr", "id", "ms", "el", "bn", "ta", "te", "sw", "zu"];
for (const tag of tags) {
  const L = `new Intl.Locale(${q(tag)})`;
  P(`${L}.maximize().toString()`);
  P(`${L}.minimize().toString()`);
  P(`${L}.maximize().minimize().toString()`);
  J(`(l => [l.language, l.script, l.region, l.baseName, l.calendar, l.collation, l.hourCycle, l.caseFirst, l.numeric, l.numberingSystem])(${L})`);
  P(`JSON.stringify(${L}.getWeekInfo())`);
  P(`JSON.stringify(${L}.getTextInfo())`);
  P(`JSON.stringify(${L}.getHourCycles())`);
  P(`JSON.stringify(${L}.getCalendars())`);
  P(`JSON.stringify(${L}.getNumberingSystems())`);
  P(`JSON.stringify(${L}.getCollations())`);
  P(`JSON.stringify(${L}.getTimeZones())`);
}
P(`typeof new Intl.Locale('en').weekInfo + typeof new Intl.Locale('en').textInfo + typeof new Intl.Locale('en').hourCycles`);
P(`JSON.stringify(new Intl.Locale('en').weekInfo)`);
P(`JSON.stringify(new Intl.Locale('ar').textInfo)`);
P(`JSON.stringify(new Intl.Locale('en').calendars)`);
P(`JSON.stringify(new Intl.Locale('en').timeZones)`);
for (const [loc, opts] of [
  ["en", "{ calendar: 'buddhist' }"], ["en", "{ hourCycle: 'h11' }"], ["en", "{ caseFirst: 'upper' }"], ["en", "{ numeric: true }"],
  ["en", "{ numberingSystem: 'arab' }"], ["en", "{ collation: 'emoji' }"], ["en", "{ language: 'fr', region: 'CA' }"], ["en", "{ script: 'Latn' }"],
  ["en", "{ region: 'br' }"], ["en-US", "{ hourCycle: 'h23', calendar: 'islamic', numberingSystem: 'thai' }"], ["pt", "{ region: 'PT', script: 'Latn' }"],
  ["en", "{ calendar: 'x' }"], ["en", "{ calendar: 'gregory', firstDayOfWeek: 'mon' }"], ["en", "{ firstDayOfWeek: 1 }"], ["en", "{ firstDayOfWeek: 'sun' }"],
  ["en", "{ language: 'zz9' }"], ["en", "{ script: 'Latin' }"], ["en", "{ region: 'USA' }"], ["en", "{ hourCycle: 'h99' }"], ["en", "{ numeric: 'false' }"],
  ["zh", "{ script: 'Hant', region: 'TW' }"], ["en-u-ca-gregory", "{ calendar: 'japanese' }"], ["en-u-hc-h12", "{ hourCycle: 'h23' }"],
]) {
  P(`new Intl.Locale(${q(loc)}, ${opts}).toString()`);
  P(`new Intl.Locale(${q(loc)}, ${opts}).maximize().toString()`);
}
for (const arg of ["''", "'e'", "'en_US'", "'en-'", "'-en'", "'en-US-US'", "'EN-us'", "'zh-hant-tw'", "'en-u'", "'en-u-ca'", "'en-x-private'", "'x-private'", "'i-klingon'", "'art-lojban'", "'en-a-bbb'", "'en-US-u-ca-gregory-ca-buddhist'", "undefined", "null", "5", "{}", "[]", "['en']", "new Intl.Locale('fr-CA')", "{ toString() { return 'de-AT' } }", "'en-1996'", "'sl-rozaj-biske'", "'de-DE-1901-1996'", "'und-u-hc-h23-ca-hebrew-nu-latn'", "'en-t-m0-ungegn'", "'en-U-CA-GREGORY'", "'ja-Jpan-JP-u-ca-japanese-hc-h12'", "'zh-cmn-Hans-CN'", "'sgn-BR'", "'en-aaaaaaaaa'"]) {
  P(`new Intl.Locale(${arg}).toString()`);
  P(`JSON.stringify(new Intl.Locale(${arg}).maximize().minimize().baseName)`);
}
P(`Intl.Locale('en')`);
P(`new Intl.Locale()`);
P(`Intl.Locale.length + ',' + Intl.Locale.prototype.maximize.length + ',' + Intl.Locale.name`);
P(`Object.getOwnPropertyNames(Intl.Locale.prototype).sort().join()`);
P(`Intl.Locale.prototype[Symbol.toStringTag]`);
P(`Object.getOwnPropertyDescriptor(Intl.Locale.prototype, 'language').get.name`);
P(`Intl.Locale.prototype.toString.call({})`);
P(`Intl.Locale.prototype.language`);
P(`new Intl.Locale('en') + ''`);
P(`JSON.stringify(new Intl.Locale('en-US'))`);
P(`new Intl.Locale('en').minimize() === new Intl.Locale('en')`);
P(`new Intl.Locale('en-Latn-US').minimize().toString()`);
P(`new Intl.Locale('en-Latn-GB').minimize().toString()`);
P(`new Intl.Locale('zh-Hant-TW').minimize().toString()`);
P(`new Intl.Locale('zh-Hans-CN').minimize().toString()`);
P(`new Intl.Locale('sr-Cyrl-RS').minimize().toString()`);
P(`new Intl.Locale('sr-Latn-RS').minimize().toString()`);
P(`new Intl.Locale('en-US-u-ca-gregory').minimize().toString()`);
P(`new Intl.Locale('und-u-ca-gregory').maximize().toString()`);

// ---- Intl.DurationFormat.
const durations = [
  "{}", "{ hours: 1 }", "{ hours: 1, minutes: 2, seconds: 3 }", "{ years: 1, months: 2, weeks: 3, days: 4, hours: 5, minutes: 6, seconds: 7, milliseconds: 8, microseconds: 9, nanoseconds: 10 }",
  "{ minutes: 5, seconds: 30 }", "{ seconds: 1, milliseconds: 500 }", "{ hours: 0, minutes: 0, seconds: 0 }", "{ hours: -1, minutes: -2 }", "{ days: 2, hours: 12 }",
  "{ hours: 25, minutes: 61, seconds: 61 }", "{ milliseconds: 1500 }", "{ nanoseconds: 123456789 }", "{ seconds: 3, milliseconds: 5, microseconds: 7, nanoseconds: 9 }",
  "{ hours: 1.5 }", "{ weeks: 1 }",
];
const dfLocales = ["en", "pt", "es", "fr", "de", "ja", "ru", "ar", "zh", "pl"];
for (const d of durations) {
  for (const loc of dfLocales.slice(0, d.length > 40 ? 10 : 4)) {
    for (const style of ["long", "short", "narrow", "digital"]) {
      P(`new Intl.DurationFormat(${q(loc)}, { style: ${q(style)} }).format(${d})`);
    }
  }
  J(`new Intl.DurationFormat('en').formatToParts(${d})`);
  P(`new Intl.DurationFormat('en', { style: 'digital', fractionalDigits: 3 }).format(${d})`);
  P(`new Intl.DurationFormat('en', { style: 'short', hours: 'numeric', minutes: '2-digit', seconds: '2-digit' }).format(${d})`);
  P(`new Intl.DurationFormat('en', { style: 'long', hoursDisplay: 'always', minutesDisplay: 'always' }).format(${d})`);
}
for (const opts of ["{ localeMatcher: 'lookup' }", "{ numberingSystem: 'arab' }", "{ style: 'bad' }", "{ hours: 'bad' }", "{ fractionalDigits: 10 }", "{ fractionalDigits: -1 }", "{ fractionalDigits: 5 }", "{ years: 'numeric' }", "{ hours: '2-digit', minutes: 'long' }", "{ secondsDisplay: 'auto', style: 'digital' }", "{ milliseconds: 'numeric', seconds: 'numeric' }", "{ style: 'digital', hours: 'long' }", "{ minutes: 'numeric', seconds: 'long' }"]) {
  P(`JSON.stringify(new Intl.DurationFormat('en', ${opts}).resolvedOptions())`);
  P(`new Intl.DurationFormat('pt', ${opts}).format({ hours: 1, minutes: 5, seconds: 9, milliseconds: 120 })`);
}
for (const arg of ["undefined", "null", "'PT1H'", "1", "{ hours: 1, foo: 2 }", "{ hours: Infinity }", "{ hours: NaN }", "{ hours: 1, minutes: -1 }", "{ hours: 1e21 }", "{ hours: '2' }", "{ hours: 1.5, minutes: 1 }", "{ hours: undefined }", "[]"]) {
  P(`new Intl.DurationFormat('en').format(${arg})`);
}
P(`JSON.stringify(new Intl.DurationFormat('pt-BR').resolvedOptions())`);
P(`JSON.stringify(new Intl.DurationFormat('ja', { style: 'narrow' }).resolvedOptions())`);
P(`JSON.stringify(new Intl.DurationFormat('ar', { style: 'digital' }).resolvedOptions())`);
P(`JSON.stringify(Intl.DurationFormat.supportedLocalesOf(['en', 'xx', 'pt-BR']))`);
P(`Intl.DurationFormat('en')`);
P(`Intl.DurationFormat.length + ',' + Intl.DurationFormat.prototype.format.length + ',' + Intl.DurationFormat.prototype.formatToParts.length`);
P(`Intl.DurationFormat.prototype[Symbol.toStringTag]`);
P(`Object.getOwnPropertyNames(Intl.DurationFormat.prototype).sort().join()`);
P(`Intl.DurationFormat.prototype.format.call({}, {})`);

// ---- Execução, igual a gen-scope-golden.js: `vm.runInThisContext` roda como ProgramExecutable do JSC puro.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "intl-extra-golden-"));
const source_file = path.join(dir, "intl_source.js");
const file = path.join(dir, "intl_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
