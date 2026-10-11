// Gera tests/golden/intl_text_bun.tsv: Intl.Collator, Intl.PluralRules, Intl.ListFormat, Intl.RelativeTimeFormat,
// Intl.Segmenter, Intl.DisplayNames, Intl.getCanonicalLocales e Intl.supportedValuesOf, medido no bun 1.4.2.
// Collator: sensitivity, numeric, caseFirst, ignorePunctuation, usage search e extensões -u-co/-kn/-kf/-ks.
// PluralRules: select e selectRange, cardinal e ordinal em 15 locales, notation compact e minimumFractionDigits.
// ListFormat: type e style. RelativeTimeFormat: numeric auto, unidades e formatToParts. Segmenter: grapheme, word e
// sentence com emoji, ZWJ e CJK, e containing(). DisplayNames: language, region, script, currency, calendar e dateTimeField.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host.
// Uso: bun scripts/gen-intl-text-golden.js > tests/golden/intl_text_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, stepSampler, sampleByHash } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

// Os candidatos entram em `pool`; os que só entram em parte levam a densidade (`thin(passo, ...)`, 1 em `passo`) e a escolha
// é por hash do texto (`sampleByHash` dentro do `stepSampler`), nunca pela posição nem pelo tamanho do locale.
const pool = stepSampler();
const add = (...list) => pool.push(1, ...list);
const thin = (step, ...list) => pool.push(step, ...list);
const q = JSON.stringify;
const T = body => (/^var /.test(body) ? `T(()=>{${body.replace(/;([^;]*)$/, ";return $1")}})` : `T(()=>${body})`);

// ---- 1. Intl.Collator.
const collLocales = ["en", "de", "sv", "es", "fr", "ja", "zh", "ru", "tr", "pl", "da", "cs", "el", "fi", "hu", "pt", "nb", "ko", "it"];
const collWords = [
  ["a", "A", "á", "Á", "b", "B"],
  ["z", "ä", "a", "å", "ö", "o", "ß", "ss"],
  ["résumé", "resume", "Resume", "résume"],
  ["item10", "item2", "item1", "Item3", "item02"],
  ["ñ", "n", "o", "ó", "ç", "c", "d"],
  ["ı", "i", "İ", "I", "j"],
  ["ch", "c", "h", "ci", "d", "cz"],
  ["あ", "ア", "か", "カ", "ｱ", "ー"],
];
for (const loc of collLocales) {
  for (let w = 0; w < collWords.length; w++) {
    thin(2, T(`new Intl.Collator(${q(loc)}).resolvedOptions().sensitivity+","+${q(collWords[w])}.sort(new Intl.Collator(${q(loc)}).compare).join("|")`));
  }
}
const sens = ["base", "accent", "case", "variant"];
const sensPairs = [["a", "A"], ["a", "á"], ["a", "Á"], ["a", "b"], ["ä", "a"], ["ae", "æ"], ["ß", "ss"], ["ç", "c"], ["a", "a "], ["", " "], ["ø", "o"], ["ñ", "n"], ["ı", "i"]];
for (const loc of ["en", "de", "sv", "tr", "da", "fr"]) for (const s of sens) for (const [x, y] of sensPairs) {
  add(T(`new Intl.Collator(${q(loc)},{sensitivity:${q(s)}}).compare(${q(x)},${q(y)})`));
}
const numPairs = [["1", "2"], ["2", "10"], ["10", "9"], ["a10", "a9"], ["a01", "a1"], ["1.5", "1.10"], ["x-2", "x-10"], ["007", "7"], ["a1b2", "a1b10"], ["١", "٢"], ["12abc", "12ABC"]];
for (const loc of ["en", "de", "sv", "ja", "ru", "ar"]) for (const [x, y] of numPairs) {
  add(T(`new Intl.Collator(${q(loc)},{numeric:true}).compare(${q(x)},${q(y)})+","+new Intl.Collator(${q(loc)},{numeric:false}).compare(${q(x)},${q(y)})`));
  add(T(`new Intl.Collator(${q(loc + "-u-kn")}).compare(${q(x)},${q(y)})+new Intl.Collator(${q(loc + "-u-kn-false")}).compare(${q(x)},${q(y)})`));
}
for (const loc of ["en", "de", "sv", "da", "tr", "fr"]) for (const cf of ["upper", "lower", "false"]) for (const [x, y] of [["a", "A"], ["A", "a"], ["b", "A"], ["ä", "Ä"], ["a", "B"], ["Z", "z"]]) {
  add(T(`new Intl.Collator(${q(loc)},{caseFirst:${q(cf)}}).compare(${q(x)},${q(y)})`));
}
for (const loc of ["en", "de", "sv"]) for (const cf of ["upper", "lower", "false"]) {
  add(T(`new Intl.Collator(${q(loc + "-u-kf-" + cf)}).resolvedOptions().caseFirst+","+["a","A","b","B","á","Á"].sort(new Intl.Collator(${q(loc + "-u-kf-" + cf)}).compare).join("")`));
}
const punct = [["a b", "ab"], ["a-b", "ab"], ["a.b", "a,b"], ["a_b", "ab"], ["(a)", "a"], ["a!", "a"], ["co-op", "coop"], ["e-mail", "email"], [" a", "a"], ["a  b", "a b"], ["$a", "a"]];
for (const loc of ["en", "de", "sv", "fr", "ja"]) for (const [x, y] of punct) {
  add(T(`new Intl.Collator(${q(loc)},{ignorePunctuation:true}).compare(${q(x)},${q(y)})+","+new Intl.Collator(${q(loc)},{ignorePunctuation:false}).compare(${q(x)},${q(y)})`));
}
for (const loc of ["en", "de", "sv", "tr", "fr", "es", "ja"]) for (const [x, y] of sensPairs) {
  add(T(`new Intl.Collator(${q(loc)},{usage:"search"}).compare(${q(x)},${q(y)})+","+new Intl.Collator(${q(loc)},{usage:"sort"}).compare(${q(x)},${q(y)})`));
}
const coWords = { phonebk: ["Ärger", "Arg", "Aal", "Äpfel", "Zebra", "Öl", "Ofen"], trad: ["llama", "luz", "chico", "cosa", "dama", "cu"], pinyin: ["中", "文", "字", "汉", "语", "学"], stroke: ["中", "文", "字", "汉", "语", "学"], eor: ["z", "a", "ä", "Z", "5", "α", "я"], emoji: ["😀", "a", "👍", "❤", "🙂"], standard: ["a", "B", "ä", "Z"], search: ["a", "B", "ä", "Z"], big5han: ["中", "文", "一", "二"], unihan: ["中", "文", "一", "二"], zhuyin: ["中", "文", "一", "二"], gb2312: ["中", "文", "一", "二"], dict: ["a", "b", "ä"], compat: ["a", "b", "ä"], reformed: ["a", "b", "ä"], ducet: ["a", "B", "ä"] };
for (const loc of ["de", "es", "zh", "sv", "en", "ja", "da"]) for (const [co, words] of Object.entries(coWords)) {
  add(T(`new Intl.Collator(${q(loc + "-u-co-" + co)}).resolvedOptions().collation+","+${q(words)}.sort(new Intl.Collator(${q(loc + "-u-co-" + co)}).compare).join("|")`));
}
add(
  T(`new Intl.Collator("en-u-ks-level1").compare("a","A")`), T(`new Intl.Collator("en-u-ks-level2").compare("a","á")`), T(`new Intl.Collator("en-u-ks-level3").compare("a","A")`),
  T(`new Intl.Collator("en-u-ks-level1").resolvedOptions().sensitivity`), T(`new Intl.Collator("en-u-ks-level2").resolvedOptions().sensitivity`),
  T(`new Intl.Collator("en-u-ks-level3").resolvedOptions().sensitivity`), T(`new Intl.Collator("en-u-ks-level4").resolvedOptions().sensitivity`),
  T(`new Intl.Collator("en-u-ks-identic").resolvedOptions().sensitivity`), T(`new Intl.Collator("en-u-kn-true-kf-upper-co-emoji").resolvedOptions()`),
  T(`new Intl.Collator("de-u-co-phonebk-kn").resolvedOptions()`), T(`new Intl.Collator("en",{sensitivity:"x"})`), T(`new Intl.Collator("en",{caseFirst:"x"})`),
  T(`new Intl.Collator("en",{usage:"x"})`), T(`new Intl.Collator("en",{collation:"phonebk"}).resolvedOptions().collation`), T(`new Intl.Collator("de",{collation:"phonebk"}).resolvedOptions().collation`),
  T(`new Intl.Collator("de",{collation:"zz"}).resolvedOptions().collation`), T(`new Intl.Collator("de",{collation:"x y"})`), T(`new Intl.Collator("en",{numeric:"false"}).resolvedOptions().numeric`),
  T(`new Intl.Collator("en",{numeric:0}).resolvedOptions().numeric`), T(`new Intl.Collator("en",{ignorePunctuation:1}).resolvedOptions().ignorePunctuation`),
  T(`new Intl.Collator("th").resolvedOptions().ignorePunctuation`), T(`new Intl.Collator("th",{ignorePunctuation:false}).resolvedOptions().ignorePunctuation`),
  T(`Intl.Collator.supportedLocalesOf(["en","de","xx","zz-ZZ","sv"]).join()`), T(`Intl.Collator.supportedLocalesOf("de-u-co-phonebk").join()`),
  T(`Intl.Collator.supportedLocalesOf(["en"],{localeMatcher:"best fit"}).join()`), T(`Intl.Collator.supportedLocalesOf(["en"],{localeMatcher:"x"})`),
  T(`Object.getOwnPropertyNames(Intl.Collator.prototype).join()`), T(`Intl.Collator.prototype[Symbol.toStringTag]`), T(`typeof Intl.Collator("en").compare`),
  T(`Intl.Collator("en").compare.name+Intl.Collator("en").compare.length`), T(`var c=new Intl.Collator("en");c.compare===c.compare`), T(`var c=new Intl.Collator("en");var f=c.compare;[2,1,3].sort(f).join()`),
  T(`new Intl.Collator("en").compare()`), T(`new Intl.Collator("en").compare("undefined")`), T(`new Intl.Collator("en").compare(undefined,"undefined")`), T(`new Intl.Collator("en").compare(1,2)`),
  T(`new Intl.Collator("en").compare(null,"null")`), T(`new Intl.Collator("en").compare({toString(){return "a"}},"a")`), T(`new Intl.Collator("en").compare(Symbol(),"a")`),
  T(`Intl.Collator.prototype.compare`), T(`Object.getOwnPropertyDescriptor(Intl.Collator.prototype,"compare").get.name`), T(`new Intl.Collator("en").compare("\\ud800","a")`),
  T(`new Intl.Collator("en").compare("a\\u0301","\\u00e1")`), T(`new Intl.Collator("en").compare("\\u212b","\\u00c5")`), T(`new Intl.Collator("en").compare("\\ufb01","fi")`), T(`new Intl.Collator("en",{sensitivity:"base"}).compare("\\ufb01","fi")`),
  T(`new Intl.Collator("en").compare("a","a\\u0000")`), T(`new Intl.Collator("en").compare("\\u0000","")`), T(`new Intl.Collator("en").compare("a\\u200bb","ab")`), T(`new Intl.Collator("en",{sensitivity:"base"}).compare("a\\u200bb","ab")`),
);

// ---- 2. Intl.PluralRules.
const prLocales = ["en", "ru", "ar", "pl", "cs", "fr", "he", "lt", "lv", "ro", "sl", "cy", "ga", "es", "pt"];
const prNums = [0, 1, 2, 3, 4, 5, 6, 10, 11, 12, 21, 22, 100, 101, 111, 1.5, 0.5, 2.5, 1e6, -1, -2, 1e21, 0.1, 21.5, 1000000, 1e7];
for (const loc of prLocales) for (const type of ["cardinal", "ordinal"]) for (const n of prNums) {
  if ((n === 1e21 || n === 1e7 || n === 0.1) && type === "ordinal") continue;
  add(T(`new Intl.PluralRules(${q(loc)},{type:${q(type)}}).select(${n})`));
}
for (const loc of prLocales) for (const type of ["cardinal", "ordinal"]) {
  add(T(`new Intl.PluralRules(${q(loc)},{type:${q(type)}}).resolvedOptions().pluralCategories.join()`));
}
const ranges = [[0, 1], [1, 2], [1, 5], [0, 0], [1, 1], [2, 2], [5, 5], [0, 5], [1, 21], [11, 21], [101, 102], [0.5, 1], [1.5, 2.5], [3, 100], [21, 22]];
for (const loc of ["en", "ru", "ar", "pl", "fr", "he", "lt", "cy", "ga", "es", "sl", "ro"]) for (const [a, b] of ranges) {
  add(T(`new Intl.PluralRules(${q(loc)}).selectRange(${a},${b})`));
}
for (const loc of ["en", "ru", "ar", "es", "fr", "pl", "pt", "de", "it", "he"]) for (const [mfd, mxfd] of [[0, 3], [1, 3], [2, 2], [3, 3]]) for (const n of [1, 2, 1.5, 0.5, 1.0]) {
  add(T(`new Intl.PluralRules(${q(loc)},{minimumFractionDigits:${mfd},maximumFractionDigits:${mxfd}}).select(${n})`));
}
for (const loc of ["en", "fr", "es", "pt", "it", "ca", "ru", "pl", "de", "ar", "he"]) for (const n of [1000000, 2000000, 1e9, 1500000, 999999, 1000, 1e6 + 1, 1e12, 5e5]) {
  add(T(`new Intl.PluralRules(${q(loc)},{notation:"compact"}).select(${n})`));
  add(T(`new Intl.PluralRules(${q(loc)},{notation:"compact",compactDisplay:"long"}).select(${n})`));
}
for (const loc of ["en", "fr", "es", "ru"]) for (const o of ['{maximumSignificantDigits:1}', '{minimumSignificantDigits:3}', '{minimumIntegerDigits:3}', '{roundingMode:"floor",maximumFractionDigits:0}', '{notation:"scientific"}', '{notation:"engineering"}', '{roundingIncrement:5,maximumFractionDigits:2,minimumFractionDigits:2}']) for (const n of [1, 1.9, 2, 0.99, 1.004]) {
  add(T(`new Intl.PluralRules(${q(loc)},${o}).select(${n})`));
}
add(
  T(`new Intl.PluralRules("en").select("1")`), T(`new Intl.PluralRules("en").select("1.0")`), T(`new Intl.PluralRules("en").select("abc")`), T(`new Intl.PluralRules("en").select()`), T(`new Intl.PluralRules("en").select(null)`),
  T(`new Intl.PluralRules("en").select(NaN)`), T(`new Intl.PluralRules("en").select(Infinity)`), T(`new Intl.PluralRules("en").select(-0)`), T(`new Intl.PluralRules("en").select(1n)`), T(`new Intl.PluralRules("en").select(Symbol())`),
  T(`new Intl.PluralRules("en").selectRange(1)`), T(`new Intl.PluralRules("en").selectRange(NaN,1)`), T(`new Intl.PluralRules("en").selectRange(1,NaN)`), T(`new Intl.PluralRules("en").selectRange(undefined,1)`), T(`new Intl.PluralRules("en").selectRange(1,undefined)`),
  T(`new Intl.PluralRules("en").selectRange("1","2")`), T(`new Intl.PluralRules("en").selectRange(2,1)`),
  T(`new Intl.PluralRules("en",{type:"x"})`), T(`new Intl.PluralRules("en").resolvedOptions()`), T(`new Intl.PluralRules("ar",{type:"ordinal"}).resolvedOptions()`), T(`new Intl.PluralRules("ru",{maximumFractionDigits:0}).resolvedOptions()`),
  T(`new Intl.PluralRules("en",{minimumFractionDigits:2}).resolvedOptions()`), T(`new Intl.PluralRules("en",{maximumSignificantDigits:3}).resolvedOptions()`), T(`new Intl.PluralRules("en",{notation:"compact"}).resolvedOptions()`),
  T(`new Intl.PluralRules("en",{roundingMode:"ceil"}).resolvedOptions().roundingMode`), T(`new Intl.PluralRules("en",{trailingZeroDisplay:"stripIfInteger"}).resolvedOptions().trailingZeroDisplay`),
  T(`Intl.PluralRules.supportedLocalesOf(["en","ru","xx"]).join()`), T(`Object.getOwnPropertyNames(Intl.PluralRules.prototype).join()`), T(`Intl.PluralRules.prototype[Symbol.toStringTag]`), T(`Intl.PluralRules()`),
  T(`new Intl.PluralRules("en",{minimumFractionDigits:5,maximumFractionDigits:2})`), T(`new Intl.PluralRules("en",{minimumFractionDigits:101})`), T(`new Intl.PluralRules("en",{maximumFractionDigits:-1})`),
  T(`new Intl.PluralRules("en-u-nu-arab").select(1)`), T(`new Intl.PluralRules("ar-u-nu-latn").resolvedOptions().locale`), T(`new Intl.PluralRules("en-US-u-va-posix").resolvedOptions().locale`),
  T(`new Intl.PluralRules("pt-PT").select(1)`), T(`new Intl.PluralRules("pt-PT").select(1.5)`), T(`new Intl.PluralRules("pt-PT").select(0)`), T(`new Intl.PluralRules("pt-PT").resolvedOptions().pluralCategories.join()`),
  T(`new Intl.PluralRules("pt-BR").select(0)`), T(`new Intl.PluralRules("pt-BR").select(1000000)`), T(`new Intl.PluralRules("fr").select(1000000)`), T(`new Intl.PluralRules("fr",{notation:"compact"}).select(1000000)`),
);

// ---- 3. Intl.ListFormat.
const lfLocales = ["en", "es", "fr", "de", "ja", "zh", "ru", "ar", "pt", "it", "ko", "hi", "tr", "pl", "nl", "sv", "he"];
const lfLists = [["A", "B"], ["A", "B", "C"], ["A", "B", "C", "D"]];
for (const loc of lfLocales) for (const type of ["conjunction", "disjunction", "unit"]) for (const style of ["long", "short", "narrow"]) {
  // Cada combinação dava 3 listas num terço dos casos e 1 nos outros: 5/3 por combinação, de 3 possíveis (densidade 5/9).
  for (const l of lfLists) thin(9 / 5, T(`new Intl.ListFormat(${q(loc)},{type:${q(type)},style:${q(style)}}).format(${q(l)})`));
}
for (const loc of ["en", "es", "fr", "ja", "ar", "ru", "hi", "de"]) for (const type of ["conjunction", "disjunction", "unit"]) {
  add(T(`new Intl.ListFormat(${q(loc)},{type:${q(type)}}).formatToParts(["x","y","z"])`));
  add(T(`new Intl.ListFormat(${q(loc)},{type:${q(type)},style:"short"}).formatToParts(["x","y"])`));
}
for (const loc of ["en", "es", "ja", "ru"]) for (const l of [[], ["one"], ["a", "b", "c", "d", "e"], ["", ""], ["a", ""]]) {
  add(T(`new Intl.ListFormat(${q(loc)}).format(${q(l)})`));
  add(T(`new Intl.ListFormat(${q(loc)}).formatToParts(${q(l)})`));
}
add(
  T(`new Intl.ListFormat("es").format(["a","i"])`), T(`new Intl.ListFormat("es").format(["a","hi"])`), T(`new Intl.ListFormat("es").format(["a","y"])`), T(`new Intl.ListFormat("es").format(["a","o"])`),
  T(`new Intl.ListFormat("es",{type:"disjunction"}).format(["a","o"])`), T(`new Intl.ListFormat("es",{type:"disjunction"}).format(["a","8"])`), T(`new Intl.ListFormat("es",{type:"disjunction"}).format(["a","ocho"])`),
  T(`new Intl.ListFormat("es").format(["a","ib"])`), T(`new Intl.ListFormat("es").format(["a","Iván"])`), T(`new Intl.ListFormat("es").format(["a","hielo"])`), T(`new Intl.ListFormat("es").format(["a","hierro"])`),
  T(`new Intl.ListFormat("en").format("abc")`), T(`new Intl.ListFormat("en").format(new Set(["a","b"]))`), T(`new Intl.ListFormat("en").format([1,2])`), T(`new Intl.ListFormat("en").format(["a",undefined])`),
  T(`new Intl.ListFormat("en").format([{}])`), T(`new Intl.ListFormat("en").format(1)`), T(`new Intl.ListFormat("en").format()`), T(`new Intl.ListFormat("en").format(null)`), T(`new Intl.ListFormat("en").format({length:2,0:"a",1:"b"})`),
  T(`new Intl.ListFormat("en",{type:"x"})`), T(`new Intl.ListFormat("en",{style:"x"})`), T(`new Intl.ListFormat("en",{type:"unit",style:"narrow"}).resolvedOptions()`), T(`new Intl.ListFormat("en").resolvedOptions()`),
  T(`new Intl.ListFormat("ja-u-nu-hanidec").resolvedOptions().locale`), T(`Intl.ListFormat.supportedLocalesOf(["en","xx","es"]).join()`), T(`Object.getOwnPropertyNames(Intl.ListFormat.prototype).join()`),
  T(`Intl.ListFormat()`), T(`Intl.ListFormat.prototype[Symbol.toStringTag]`), T(`Intl.ListFormat.length+Intl.ListFormat.name`), T(`new Intl.ListFormat("en-GB").format(["a","b","c"])`), T(`new Intl.ListFormat("en-AU").format(["a","b","c"])`),
  T(`new Intl.ListFormat("en-IN").format(["a","b","c"])`), T(`new Intl.ListFormat("pt-PT").format(["a","b","c"])`), T(`new Intl.ListFormat("zh-Hant").format(["a","b","c"])`), T(`new Intl.ListFormat("zh-HK",{type:"disjunction"}).format(["a","b","c"])`),
  T(`new Intl.ListFormat("de-CH").format(["a","b","c"])`), T(`new Intl.ListFormat("fr-CA").format(["a","b"],{})`), T(`new Intl.ListFormat("es-419").format(["a","b","c"])`), T(`new Intl.ListFormat("es-MX",{type:"disjunction"}).format(["a","b","c"])`),
);

// ---- 4. Intl.RelativeTimeFormat.
const rtLocales = ["en", "es", "fr", "de", "ja", "zh", "ru", "ar", "pt", "it", "ko", "pl", "tr", "hi", "nl"];
const units = ["second", "minute", "hour", "day", "week", "month", "quarter", "year"];
for (const loc of rtLocales) for (const u of units) for (const v of [-2, -1, 0, 1, 2]) {
  add(T(`new Intl.RelativeTimeFormat(${q(loc)},{numeric:"auto"}).format(${v},${q(u)})`));
}
for (const loc of rtLocales) for (const u of units) for (const v of [-1, 1, 1.5, 0, -0, 1000]) {
  (Object.is(v, -0) ? add : thin.bind(null, 2))(T(`new Intl.RelativeTimeFormat(${q(loc)}).format(${Object.is(v, -0) ? "-0" : v},${q(u + "s")})`));
}
for (const loc of ["en", "es", "fr", "ja", "ru", "de", "ar", "pt", "ko"]) for (const style of ["short", "narrow"]) for (const u of units) for (const v of [-1, 3]) {
  add(T(`new Intl.RelativeTimeFormat(${q(loc)},{style:${q(style)},numeric:"auto"}).format(${v},${q(u)})`));
}
for (const loc of ["en", "es", "ja", "ru", "ar", "de", "fr"]) for (const u of ["second", "day", "month", "year"]) for (const v of [-1, 0, 1, 12345.678]) {
  add(T(`new Intl.RelativeTimeFormat(${q(loc)},{numeric:"auto"}).formatToParts(${v},${q(u)})`));
  add(T(`new Intl.RelativeTimeFormat(${q(loc)}).formatToParts(${v},${q(u)})`));
}
add(
  T(`new Intl.RelativeTimeFormat("en").format()`), T(`new Intl.RelativeTimeFormat("en").format(1)`), T(`new Intl.RelativeTimeFormat("en").format(1,"x")`), T(`new Intl.RelativeTimeFormat("en").format(NaN,"day")`),
  T(`new Intl.RelativeTimeFormat("en").format(Infinity,"day")`), T(`new Intl.RelativeTimeFormat("en").format("3","day")`), T(`new Intl.RelativeTimeFormat("en").format(1n,"day")`), T(`new Intl.RelativeTimeFormat("en").format(null,"day")`),
  T(`new Intl.RelativeTimeFormat("en").format(1,"DAY")`), T(`new Intl.RelativeTimeFormat("en").format(1,"milliseconds")`), T(`new Intl.RelativeTimeFormat("en").format(1,undefined)`), T(`new Intl.RelativeTimeFormat("en").format(1e21,"day")`),
  T(`new Intl.RelativeTimeFormat("en").format(-1e-7,"day")`), T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(-0,"day")`), T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(0.5,"day")`),
  T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(1,"hour")`), T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(0,"hour")`), T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(-1,"minute")`),
  T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(0,"second")`), T(`new Intl.RelativeTimeFormat("en",{numeric:"auto"}).format(0,"week")`), T(`new Intl.RelativeTimeFormat("en",{numeric:"x"})`), T(`new Intl.RelativeTimeFormat("en",{style:"x"})`),
  T(`new Intl.RelativeTimeFormat("en").resolvedOptions()`), T(`new Intl.RelativeTimeFormat("ar-u-nu-latn",{style:"narrow",numeric:"auto"}).resolvedOptions()`), T(`new Intl.RelativeTimeFormat("ar").resolvedOptions().numberingSystem`),
  T(`new Intl.RelativeTimeFormat("en-u-nu-arab").format(123,"day")`), T(`new Intl.RelativeTimeFormat("hi-u-nu-deva").format(123,"day")`), T(`new Intl.RelativeTimeFormat("th-u-nu-thai").format(5,"day")`),
  T(`Intl.RelativeTimeFormat.supportedLocalesOf(["en","xx","ja"]).join()`), T(`Object.getOwnPropertyNames(Intl.RelativeTimeFormat.prototype).join()`), T(`Intl.RelativeTimeFormat.prototype[Symbol.toStringTag]`), T(`Intl.RelativeTimeFormat()`),
  T(`new Intl.RelativeTimeFormat("en").formatToParts(1)`), T(`new Intl.RelativeTimeFormat("en").formatToParts(1,"x")`), T(`new Intl.RelativeTimeFormat("en").formatToParts(NaN,"day")`),
  T(`new Intl.RelativeTimeFormat("en-GB",{style:"short"}).format(-3,"month")`), T(`new Intl.RelativeTimeFormat("en-AU",{style:"narrow"}).format(-3,"month")`), T(`new Intl.RelativeTimeFormat("es-419",{numeric:"auto"}).format(1,"day")`),
  T(`new Intl.RelativeTimeFormat("es-MX",{numeric:"auto"}).format(-2,"day")`), T(`new Intl.RelativeTimeFormat("pt-PT",{numeric:"auto"}).format(-2,"day")`), T(`new Intl.RelativeTimeFormat("zh-TW",{numeric:"auto"}).format(2,"day")`),
);

// ---- 5. Intl.Segmenter.
const segTexts = [
  "Hello, world! How are you? Fine.", "👨‍👩‍👧‍👦 family", "🇧🇷🇺🇸🇯🇵", "👍🏽 ok", "éà", "한국어 텍스트", "日本語のテキスト。これは文です。", "中文文本。第二句！",
  "can't stop; it's 3.14, ok? U.S.A. Mr. Smith", "a\r\nb\nc", "नमस्ते दुनिया", "สวัสดีชาวโลก", "😀😁😂", "x‍y", "1,234.56 and 7-8", "Привет, мир! Как дела?", "", " ", "A.B.C. D", "ﬁne",
  "😀‍🔥", "🏳️‍🌈 flag", "क्ष", "ab̀́c", "Dr. No said \"Hi.\" Then left.", "e-mail: a@b.co, www.x.org", "a_b c-d", "각", "𠮷野家", "ａｂｃ ＡＢＣ １２３",
];
for (const g of ["grapheme", "word", "sentence"]) for (const txt of segTexts) {
  const body = g === "word"
    ? `Array.from(new Intl.Segmenter("en",{granularity:${q(g)}}).segment(${q(txt)}),s=>[s.segment,s.index,s.isWordLike])`
    : `Array.from(new Intl.Segmenter("en",{granularity:${q(g)}}).segment(${q(txt)}),s=>[s.segment,s.index])`;
  add(T(body));
}
for (const loc of ["en", "ja", "zh", "th", "ko", "de", "ar", "hi", "ru", "sv", "fr"]) for (const g of ["word", "sentence"]) for (const txt of ["日本語のテキスト。これは文です。", "สวัสดีชาวโลก ฉันรักเธอ", "Hello world. Bye!", "中文文本。第二句！", "Привет, мир! Как дела?"]) {
  add(T(`Array.from(new Intl.Segmenter(${q(loc)},{granularity:${q(g)}}).segment(${q(txt)}),s=>s.segment).join("|")`));
}
for (const g of ["grapheme", "word", "sentence"]) for (const txt of ["Hello, world! Bye.", "👨‍👩‍👧‍👦a🇧🇷b", "日本語のテキスト。これは文です。"]) for (const i of [-1, 0, 1, 2, 3, 5, 7, 8, 10, 12, 14, 100, 1.5, "2", NaN, undefined]) {
  add(T(`new Intl.Segmenter("en",{granularity:${q(g)}}).segment(${q(txt)}).containing(${typeof i === "string" ? q(i) : i === undefined ? "" : i})`));
}
add(
  T(`new Intl.Segmenter().resolvedOptions()`), T(`new Intl.Segmenter("en",{granularity:"word"}).resolvedOptions()`), T(`new Intl.Segmenter("en",{granularity:"x"})`), T(`Intl.Segmenter()`),
  T(`Intl.Segmenter.supportedLocalesOf(["en","xx","ja"]).join()`), T(`Object.getOwnPropertyNames(Intl.Segmenter.prototype).join()`), T(`Intl.Segmenter.prototype[Symbol.toStringTag]`),
  T(`new Intl.Segmenter().segment()`), T(`Array.from(new Intl.Segmenter().segment()).length`), T(`Array.from(new Intl.Segmenter().segment(undefined)).length`), T(`Array.from(new Intl.Segmenter().segment(123),s=>s.segment).join()`),
  T(`Array.from(new Intl.Segmenter().segment(null),s=>s.segment).join()`), T(`Array.from(new Intl.Segmenter().segment({toString(){return "ab"}}),s=>s.segment).join()`), T(`new Intl.Segmenter().segment(Symbol())`),
  T(`var s=new Intl.Segmenter().segment("ab");var it=s[Symbol.iterator]();Object.prototype.toString.call(it)+Object.prototype.toString.call(s)`),
  T(`var s=new Intl.Segmenter().segment("ab");Object.getOwnPropertyNames(Object.getPrototypeOf(s)).join()`), T(`var s=new Intl.Segmenter().segment("ab");var it=s[Symbol.iterator]();Object.getOwnPropertyNames(Object.getPrototypeOf(it)).join()`),
  T(`var s=new Intl.Segmenter().segment("ab");var it=s[Symbol.iterator]();[it.next(),it.next(),it.next(),it.next()].map(r=>r.done+":"+(r.value&&r.value.segment))`),
  T(`var s=new Intl.Segmenter().segment("ab");var r=s.containing(0);Object.keys(r).join()+","+r.input`), T(`var s=new Intl.Segmenter("en",{granularity:"word"}).segment("ab cd");var r=s.containing(1);Object.keys(r).join()+","+r.isWordLike`),
  T(`var s=new Intl.Segmenter("en",{granularity:"word"}).segment("ab cd");s.containing(2).isWordLike`), T(`var s=new Intl.Segmenter("en",{granularity:"word"}).segment("ab 12 cd");s.containing(3).isWordLike`),
  T(`Array.from(new Intl.Segmenter("en",{granularity:"word"}).segment("a1 b_2 3.5 x'y"),s=>s.isWordLike).join()`),
  T(`Array.from(new Intl.Segmenter("en",{granularity:"grapheme"}).segment("a\\ud800b"),s=>s.segment.length).join()`), T(`Array.from(new Intl.Segmenter("en",{granularity:"word"}).segment("\\udc00\\ud800"),s=>s.segment.length).join()`),
  T(`Array.from(new Intl.Segmenter("en",{granularity:"sentence"}).segment("a. b"),s=>s.segment).join("|")`), T(`Array.from(new Intl.Segmenter("en",{granularity:"sentence"}).segment("a.b"),s=>s.segment).join("|")`),
  T(`Array.from(new Intl.Segmenter("en",{granularity:"sentence"}).segment("a?! b"),s=>s.segment).join("|")`), T(`Array.from(new Intl.Segmenter("en",{granularity:"sentence"}).segment("a.\\nb"),s=>s.segment).join("|")`),
  T(`Array.from(new Intl.Segmenter("en",{granularity:"sentence"}).segment("a.\\u2029b"),s=>s.segment).join("|")`), T(`Array.from(new Intl.Segmenter("en",{granularity:"sentence"}).segment("e.g. x"),s=>s.segment).join("|")`),
);

// ---- 6. Intl.DisplayNames.
const dnLocales = ["en", "es", "fr", "de", "ja", "zh", "ru", "ar", "pt", "it", "ko", "hi", "tr"];
const dnCodes = {
  language: ["en", "pt", "pt-BR", "en-US", "en-GB", "zh-Hant", "zh-Hans-CN", "es-419", "de-AT", "fr-CA", "ja", "sr-Latn", "ar-EG", "und", "tlh", "haw", "gsw", "yue", "nds-NL", "en-Latn-US"],
  region: ["BR", "US", "JP", "DE", "FR", "CN", "IN", "ZA", "419", "001", "150", "UN", "EU", "XK", "AQ", "ZZ", "HK", "MO", "TW", "GB"],
  script: ["Latn", "Cyrl", "Arab", "Hans", "Hant", "Jpan", "Kore", "Deva", "Thai", "Grek", "Hebr", "Zzzz", "Brai", "Zyyy"],
  currency: ["USD", "EUR", "BRL", "JPY", "GBP", "CNY", "INR", "RUB", "KRW", "CHF", "XAU", "XXX", "BTC", "usd", "MXN", "CAD"],
  calendar: ["gregory", "buddhist", "chinese", "hebrew", "islamic", "japanese", "persian", "roc", "indian", "coptic", "ethiopic", "iso8601", "islamic-umalqura"],
  dateTimeField: ["era", "year", "quarter", "month", "weekOfYear", "weekday", "day", "dayPeriod", "hour", "minute", "second", "timeZoneName"],
};
for (const [type, codes] of Object.entries(dnCodes)) for (const loc of dnLocales) for (const code of codes) {
  thin(3, T(`new Intl.DisplayNames(${q(loc)},{type:${q(type)}}).of(${q(code)})`));
}
for (const loc of ["en", "es", "ja", "ru", "de", "fr"]) for (const style of ["long", "short", "narrow"]) for (const [type, code] of [["language", "en-US"], ["region", "US"], ["currency", "USD"], ["dateTimeField", "weekday"], ["script", "Latn"], ["calendar", "gregory"], ["language", "zh-Hant"], ["region", "GB"]]) {
  add(T(`new Intl.DisplayNames(${q(loc)},{type:${q(type)},style:${q(style)}}).of(${q(code)})`));
}
for (const fb of ["code", "none"]) for (const [type, code] of [["language", "xx"], ["region", "QQ"], ["script", "Qaaa"], ["currency", "ZZZ"], ["calendar", "foo"], ["dateTimeField", "era"], ["language", "en-XX"], ["region", "XA"]]) {
  for (const loc of ["en", "de", "ja"]) add(T(`new Intl.DisplayNames(${q(loc)},{type:${q(type)},fallback:${q(fb)}}).of(${q(code)})`));
}
for (const ld of ["dialect", "standard"]) for (const loc of ["en", "es", "fr", "de", "pt", "ja", "ar", "zh"]) for (const code of ["en-US", "en-GB", "pt-BR", "pt-PT", "es-419", "es-MX", "zh-Hant", "fr-CA", "nl-BE", "de-CH", "ar-001", "en-AU"]) {
  thin(2, T(`new Intl.DisplayNames(${q(loc)},{type:"language",languageDisplay:${q(ld)}}).of(${q(code)})`));
}
add(
  T(`new Intl.DisplayNames("en")`), T(`new Intl.DisplayNames("en",{})`), T(`new Intl.DisplayNames("en",{type:"x"})`), T(`new Intl.DisplayNames("en",{type:"region"}).resolvedOptions()`),
  T(`new Intl.DisplayNames("en",{type:"language",languageDisplay:"standard",style:"short",fallback:"none"}).resolvedOptions()`), T(`new Intl.DisplayNames("en",{type:"region"}).of()`), T(`new Intl.DisplayNames("en",{type:"region"}).of(undefined)`),
  T(`new Intl.DisplayNames("en",{type:"region"}).of(1)`), T(`new Intl.DisplayNames("en",{type:"region"}).of("")`), T(`new Intl.DisplayNames("en",{type:"region"}).of("us")`), T(`new Intl.DisplayNames("en",{type:"region"}).of("USA")`),
  T(`new Intl.DisplayNames("en",{type:"region"}).of("U1")`), T(`new Intl.DisplayNames("en",{type:"region"}).of("0001")`), T(`new Intl.DisplayNames("en",{type:"script"}).of("latn")`), T(`new Intl.DisplayNames("en",{type:"script"}).of("Latin")`),
  T(`new Intl.DisplayNames("en",{type:"currency"}).of("US")`), T(`new Intl.DisplayNames("en",{type:"currency"}).of("USDX")`), T(`new Intl.DisplayNames("en",{type:"currency"}).of("us1")`), T(`new Intl.DisplayNames("en",{type:"language"}).of("e")`),
  T(`new Intl.DisplayNames("en",{type:"language"}).of("en_US")`), T(`new Intl.DisplayNames("en",{type:"language"}).of("en-")`), T(`new Intl.DisplayNames("en",{type:"language"}).of("root")`), T(`new Intl.DisplayNames("en",{type:"language"}).of("en-u-ca-gregory")`),
  T(`new Intl.DisplayNames("en",{type:"language"}).of("EN-us")`), T(`new Intl.DisplayNames("en",{type:"language"}).of("i-klingon")`), T(`new Intl.DisplayNames("en",{type:"calendar"}).of("Gregory")`), T(`new Intl.DisplayNames("en",{type:"calendar"}).of("x")`),
  T(`new Intl.DisplayNames("en",{type:"dateTimeField"}).of("Era")`), T(`new Intl.DisplayNames("en",{type:"dateTimeField"}).of("week")`), T(`new Intl.DisplayNames("en",{type:"dateTimeField"}).of("weekOfYear")`),
  T(`Intl.DisplayNames.supportedLocalesOf(["en","xx","ja"]).join()`), T(`Object.getOwnPropertyNames(Intl.DisplayNames.prototype).join()`), T(`Intl.DisplayNames.prototype[Symbol.toStringTag]`), T(`Intl.DisplayNames("en",{type:"region"})`),
  T(`new Intl.DisplayNames(undefined,{type:"region"}).resolvedOptions().locale`), T(`new Intl.DisplayNames("en-u-nu-arab",{type:"region"}).resolvedOptions().locale`), T(`new Intl.DisplayNames("xx-ZZ",{type:"region"}).resolvedOptions().locale`),
  T(`new Intl.DisplayNames(["xx","de"],{type:"region"}).of("BR")`), T(`new Intl.DisplayNames("en",{type:"region",fallback:"x"})`), T(`new Intl.DisplayNames("en",{type:"language",languageDisplay:"x"})`), T(`new Intl.DisplayNames("en",{type:"region",style:"x"})`),
);

// ---- 7. Intl.getCanonicalLocales e Intl.supportedValuesOf.
const canon = [
  "en-us", "EN-US", "en_US", "zh-hans-cn", "ZH-hant-tw", "sr-latn-rs", "iw", "in", "ji", "jw", "mo", "tl", "fil", "sh", "cmn", "cmn-Hans", "zh-cmn", "i-klingon", "art-lojban", "en-gb-oed", "sgn-be-fr", "no-bok", "no-nyn", "ar-ar", "he-il", "az-az", "uz-uz",
  "en-u-ca-gregory", "en-u-ca-gregorian", "en-u-ca-islamicc", "en-u-nu-arab", "en-u-co-phonebook", "en-u-ks-primary", "en-u-ks-tertiary", "en-u-ms-imperial", "en-u-tz-aqams", "en-u-tz-cnckg", "en-u-kb-yes", "en-u-kb-true", "en-u-kn-true", "en-u-kn-false", "en-u-hc-h12", "en-u-attr-ca-gregory", "en-u-ca", "en-u", "en-u-", "en-t-ja", "en-t-m0-ungegn", "en-t-hi-latn-m0-ungegn",
  "en-x-private", "x-private", "en-a-bbb-x-a-ccc", "en-b-ccc-a-bbb", "en-z-aa-a-bb", "en-US-u-va-posix", "de-DE-u-co-phonebk", "de-DE-1996", "sl-rozaj-biske", "sl-biske-rozaj", "en-latn-us", "EN-LATN-US", "ca-valencia", "ca-ES-valencia", "es-419", "es-latn-419",
  "en-gb-u-ca-gregory-nu-latn", "en-u-nu-latn-ca-gregory", "en-u-ca-gregory-ca-buddhist", "en-u-nu-latn-ca-buddhist-hc-h24", "ja-u-ca-japanese-nu-jpanfin", "th-u-nu-thai-ca-buddhist", "en-US-posix", "en-US-POSIX",
  "", "e", "en-", "-en", "en--us", "en-u-u-ca", "123", "en-12", "en-1234", "en-us-us", "en-latn-latn", "en-a", "en-abcdefghi", "abcdefghi", "a-b", "und", "und-Latn", "und-u-ca-gregory", "root", "en-Latn-US-u-ca-gregory-x-priv", "EN-X-PRIV", "ZH-HANT-HK", "de-CH-1901", "de-1901-1996", "de-1996-1901",
  "en-t", "en-t-", "en-t-en-t-fr", "en-u-ca-gregory-u-nu-latn", "en-a-xx-a-yy", "en-u-ca-gregory-a-xx", "en-US-a-xx-u-ca-gregory", "tlh", "zzz", "qaa", "en-qaa", "ar-DZ-u-nu-arab", "pa-Arab-PK", "ku-Latn-TR", "en-001", "en-150", "en-ZZ",
];
for (const c of canon) {
  add(T(`Intl.getCanonicalLocales(${q(c)})`));
  add(T(`new Intl.Locale(${q(c)}).toString()`));
}
add(
  T(`Intl.getCanonicalLocales()`), T(`Intl.getCanonicalLocales(undefined)`), T(`Intl.getCanonicalLocales(null)`), T(`Intl.getCanonicalLocales([])`), T(`Intl.getCanonicalLocales(["en-us","EN-US","en-US"])`), T(`Intl.getCanonicalLocales(["pt-br","en","PT-BR","EN"])`),
  T(`Intl.getCanonicalLocales(["en",1])`), T(`Intl.getCanonicalLocales([{toString(){return "fr-ca"}}])`), T(`Intl.getCanonicalLocales(new Intl.Locale("de-de"))`), T(`Intl.getCanonicalLocales([new Intl.Locale("de-de-u-co-phonebk")])`),
  T(`Intl.getCanonicalLocales({length:2,0:"a-b",1:"cs"})`), T(`Intl.getCanonicalLocales({length:1,0:"de"})`), T(`Intl.getCanonicalLocales(5)`), T(`Intl.getCanonicalLocales(true)`), T(`Intl.getCanonicalLocales(Symbol())`), T(`Intl.getCanonicalLocales("en","fr")`),
  T(`Intl.getCanonicalLocales.length+Intl.getCanonicalLocales.name`), T(`Object.getOwnPropertyNames(Intl).join()`), T(`Intl[Symbol.toStringTag]`), T(`Object.prototype.toString.call(Intl)`), T(`typeof Intl.supportedValuesOf`), T(`Intl.supportedValuesOf.length+Intl.supportedValuesOf.name`),
);
for (const k of ["calendar", "collation", "currency", "numberingSystem", "timeZone", "unit", "x", "", "Calendar", "numbering", "region"]) {
  add(T(`Intl.supportedValuesOf(${q(k)})`));
  add(T(`Intl.supportedValuesOf(${q(k)}).length`));
}
for (const k of ["calendar", "collation", "currency", "numberingSystem", "unit"]) {
  add(T(`Intl.supportedValuesOf(${q(k)}).slice().sort().join()===Intl.supportedValuesOf(${q(k)}).join()`));
  add(T(`Intl.supportedValuesOf(${q(k)}).includes("gregory")+","+Intl.supportedValuesOf(${q(k)}).includes("usd")+","+Intl.supportedValuesOf(${q(k)}).includes("latn")`));
  add(T(`Array.isArray(Intl.supportedValuesOf(${q(k)}))+","+(Intl.supportedValuesOf(${q(k)})!==Intl.supportedValuesOf(${q(k)}))`));
}
add(
  T(`Intl.supportedValuesOf()`), T(`Intl.supportedValuesOf(undefined)`), T(`Intl.supportedValuesOf(null)`), T(`Intl.supportedValuesOf({toString(){return "unit"}}).length`), T(`Intl.supportedValuesOf(1)`),
  T(`Intl.supportedValuesOf("timeZone").includes("America/Sao_Paulo")+","+Intl.supportedValuesOf("timeZone").includes("UTC")+","+Intl.supportedValuesOf("timeZone").includes("Asia/Calcutta")+","+Intl.supportedValuesOf("timeZone").includes("Asia/Kolkata")`),
  T(`Intl.supportedValuesOf("calendar").includes("islamic-civil")+","+Intl.supportedValuesOf("calendar").includes("islamicc")+","+Intl.supportedValuesOf("calendar").includes("ethioaa")`),
  T(`Intl.supportedValuesOf("collation").includes("standard")+","+Intl.supportedValuesOf("collation").includes("search")+","+Intl.supportedValuesOf("collation").includes("phonebk")`),
  T(`Intl.supportedValuesOf("unit").includes("kilometer-per-hour")+","+Intl.supportedValuesOf("unit").includes("percent")+","+Intl.supportedValuesOf("unit").includes("meter-per-second")`),
  T(`Intl.supportedValuesOf("currency").includes("BRL")+","+Intl.supportedValuesOf("currency").includes("XXX")+","+Intl.supportedValuesOf("currency").includes("XAU")`),
  T(`Intl.supportedValuesOf("numberingSystem").includes("arab")+","+Intl.supportedValuesOf("numberingSystem").includes("hanidec")+","+Intl.supportedValuesOf("numberingSystem").includes("roman")`),
);

// ---- Execução.
const baseSources = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
baseSources.push(...knownPrograms("intl_text_bun.tsv", (file) => file.endsWith("_bun.tsv") && file !== "intl_text_bun.tsv" && /intl|plural|reltime|segmenter|display|collator|locale|list/.test(file)));
const baseSet = new Set(baseSources);
const seen = new Set();
const unique = pool.resolve().filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  if (baseSet.has(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    const child = spawnSync(process.execPath, [__filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, env: { ...process.env, TZ: "UTC" } });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = child.stdout;
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("intl_text", rows));
