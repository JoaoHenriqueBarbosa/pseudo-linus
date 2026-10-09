const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/text_locale_bun.tsv: texto sensível a locale medido no bun (JavaScriptCore real).
// Cobre Intl.Segmenter (grapheme, word, sentence, isWordLike, containing, formato do resultado),
// localeCompare e Intl.Collator.compare (sensitivity, numeric, caseFirst, ignorePunctuation),
// toLocaleUpperCase/toLocaleLowerCase, normalize (NFC, NFD, NFKC, NFKD), resolvedOptions e supportedLocalesOf.
// Colunas: programa de uma linha (ASCII, o resto vira \uXXXX) e o resultado (JSON ASCII, "throw" se lançar).
// tests/text_locale_bun_golden.rs roda cada programa na engine e compara com a segunda coluna.
// Uso: bun scripts/gen-text-locale-golden.js > tests/golden/text_locale_bun.tsv
function ascii(text) {
  return text.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function q(text) {
  return ascii(JSON.stringify(text));
}

const texts = [
  "\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}", "\u{1F44D}\u{1F3FD} ok", "\u{1F1E7}\u{1F1F7}\u{1F1FA}\u{1F1F8}\u{1F1EF}\u{1F1F5}",
  "\u{1F1E7}\u{1F1F7}\u{1F1FA}", "1️⃣ 2️⃣", "❤️‍\u{1F525}", "\u{1F600}\u{1F601}\u{1F602}",
  "\u{1F9D1}‍\u{1F4BB} codes", "éàô", "ẫb", "café è", "ñü",
  "한", "한국어", "한글 단어 문장입니다.", "각가",
  "สวัสดีครับ", "ประเทศไทย",
  "วันนี้อากาศดี เราไป", "สวัสดี hello 123",
  "今日は東京で天気がいい", "私は学生です。あなたは先生ですか。",
  "コンピューターとプログラミング", "「こんにちは」と言った。",
  "iPhoneを買った 3個", "我爱北京天安门", "今天天气很好，我们去公园。你来吗？",
  "中华人民共和国", "مرحبا بالعالم", "السلام عليكم. كيف حالك؟",
  "عَرَبِي", "नमस्ते", "क्षि", "हिन्दी भाषा",
  "काम करो। अब जाओ।", "שלום עולם",
  "Привет, мир! Как дела?", "γεια σου κόσμε",
  "Hello, world! How are you?", "It's a dog-eat-dog world.", "don’t stop", "3.14 e 1,234,567.89 e 1.234,56",
  "R$ 1.234,56 e US$ 9.99", "10,000 and 5,5", "a.b.c and x_y_z", "https://example.com/path?q=1&b=2#frag",
  "user@example.com, mail me", "www.example.co.uk/a.b", "Dr. Smith went to Washington D.C. on Jan. 5. He left.",
  "Mr. Jones said “Hello.” Then he left. Next!", "\"Hi,\" she said. «Bonjour». Done.", "e.g. this, i.e. that. Then more.",
  "U.S.A. is big. So is Brasil.", "Line one.\nLine two.\r\nLine three.", "A  B\tC D", "foo_bar baz-qux 12ab34",
  "École’s café \u2014 fine", "ab‍cd ​ zero", "Türkiye'de İstanbul ısı", "ßéß", "\u{10400}\u{10428} deseret",
  "ก๊า", "ok?! yes... maybe?", "1. First. 2. Second. 3. Third.", "", " ", "x",
];

const segmentLocales = ["en", "pt-BR", "de", "fr", "es", "ja", "zh", "ko", "th", "ar", "hi", "ru", "tr", "he", "sv"];
const granularities = ["grapheme", "word", "sentence"];

const collatorLocales = [
  "en", "sv", "de", "de-u-co-phonebk", "es", "es-u-co-trad", "tr", "ja", "zh-u-co-pinyin", "zh-u-co-stroke", "ko", "ru", "el",
  "he", "ar", "hi", "th", "cs", "pl", "da", "fi", "fr", "pt", "it", "nb",
];
const pairs = [
  ["a", "b"], ["a", "A"], ["A", "a"], ["a", "á"], ["e", "é"], ["é", "f"], ["z", "å"], ["z", "ä"], ["ä", "ö"],
  ["å", "ä"], ["o", "ö"], ["u", "ü"], ["ae", "ä"], ["ss", "ß"], ["s", "ß"], ["n", "ñ"], ["ñ", "o"],
  ["ch", "d"], ["ch", "i"], ["c", "č"], ["l", "ł"], ["ł", "m"], ["i", "ı"], ["ı", "j"], ["i", "İ"], ["I", "ı"],
  ["ll", "lz"], ["a", "aa"], ["aa", "å"], ["æ", "z"], ["ø", "z"], ["file2", "file10"], ["file02", "file2"], ["x9y", "x10y"],
  ["a b", "ab"], ["a-b", "ab"], ["a.b", "a b"], ["_a", "a"], ["1", "a"], ["Z", "a"], ["", "a"], ["a", ""],
  ["а", "б"], ["ё", "е"], ["ё", "ж"], ["α", "ά"], ["σ", "ς"], ["α", "β"],
  ["א", "ב"], ["ا", "ب"], ["क", "ख"], ["ก", "ข"], ["あ", "ア"], ["あ", "い"],
  ["中", "文"], ["一", "二"], ["가", "나"], ["田", "中"], ["café", "cafe"], ["resume", "résumé"],
  ["coöp", "coop"], ["Å", "A"],
];
const collatorOptions = [
  "", '{sensitivity:"base"}', '{sensitivity:"accent"}', '{sensitivity:"case"}', '{numeric:true}', '{caseFirst:"upper"}',
  '{caseFirst:"lower"}', "{ignorePunctuation:true}", '{numeric:true,sensitivity:"base"}', '{caseFirst:"upper",sensitivity:"variant"}',
];

const programs = [];
const add = (source) => programs.push(source);

// Segmenter: cada par texto/locale pega uma granularidade em rodízio, e o formato do resultado vai junto.
texts.forEach((text, ti) => {
  segmentLocales.forEach((locale, li) => {
    const granularity = granularities[(ti + li) % 3];
    add(
      `(function(){var text=${q(text)};var segments=new Intl.Segmenter(${q(locale)},{granularity:${q(granularity)}}).segment(text);` +
        `var parts=[];for(var s of segments)parts.push([s.segment,s.index,s.isWordLike]);var containing=[];` +
        `for(var i=-1;i<=text.length+1;i++){var f=segments.containing(i);containing.push(f===undefined?-1:f.index)}` +
        `return{parts:parts,containing:containing}})()`
    );
  });
  const granularity = granularities[ti % 3];
  add(
    `(function(){var text=${q(text)};var segments=new Intl.Segmenter("en",{granularity:${q(granularity)}}).segment(text);` +
      `var first=segments.containing(0);var it=segments[Symbol.iterator]();var r=it.next();` +
      `return{keys:first===undefined?null:Object.keys(first),input:first===undefined?null:first.input===text,` +
      `wordLike:first===undefined?null:typeof first.isWordLike,tag:Object.prototype.toString.call(it),done:r.done,` +
      `count:Array.from(segments).length,resolved:new Intl.Segmenter("en",{granularity:${q(granularity)}}).resolvedOptions()}})()`
  );
});

// Comparação: 60 pares em 25 locales, com opções em rodízio.
pairs.forEach(([x, y], pi) => {
  collatorLocales.forEach((locale, li) => {
    const options = collatorOptions[(pi + li) % collatorOptions.length];
    const optionText = options ? "," + options : "";
    add(`[${q(x)}.localeCompare(${q(y)},${q(locale)}${optionText}),new Intl.Collator(${q(locale)}${options ? "," + options : ""}).compare(${q(y)},${q(x)})]`);
  });
});

// Maiúsculas e minúsculas por locale.
const caseWords = [
  "i", "I", "İ", "ı", "ij", "IJ", "ĳ", "Ĳ", "ijsselmeer", "IJSSELMEER", "ß", "straße", "STRASSE", "ẞ",
  "Σ", "οδός", "ΟΔΟΣ", "ΣΣ", "AΣ", "AΣ B", "άέ", "αι",
  "ǆ", "ǅ", "Ǆ", "ǉ", "ǌ", "ﬁ", "ﬃ", "ŉ", "ẚ", "i̇", "İ", "Ì", "J́",
  "Į́", "Į́", "ì", "í", "ĩ", "İstanbul", "ISTANBUL", "bağcı", "TITLE", "ı̇",
  "Ç", "ő", "Ω", "ΐ", "ΰ", "ᾀ", "ᾳ",
];
const caseLocales = ["tr", "az", "lt", "el", "nl", "de", "en", "und", "tr-TR", "lt-LT", "nl-BE", "de-CH", "pt-BR"];
for (const locale of caseLocales) {
  for (const word of caseWords) {
    add(`[${q(word)}.toLocaleUpperCase(${q(locale)}),${q(word)}.toLocaleLowerCase(${q(locale)})]`);
  }
}
add('["I".toLocaleLowerCase(["tr","en"]),"i".toLocaleUpperCase(["xx","az"]),"I".toLocaleLowerCase([]),"i".toLocaleUpperCase(undefined)]');
add('(function(){try{"a".toLocaleUpperCase("x")}catch(e){return e.name}})()');
add('(function(){try{return "a".toLocaleLowerCase(1)}catch(e){return e.name}})()');

// Normalização.
const normalizeStrings = [
  "é", "é", "Å", "Å", "Å", "ẛ̣", "ṩ", "ṩ", "ṩ", "ﬁ", "ﬃ", "Ω", "Ω",
  "½", "①", "…", " ", "　", "ＡＢＣ", "ｱｲ", "ガ", "ガ", "ｶﾞ", "㎒",
  "각", "각", "한글", "한", "क़", "क़", "ऩ", "ऩ", "اً", "آ",
  "ﺀ", "ﻻ", "لا", "Ⅰ", "Ⅳ", "²", "µ", "ª", "™", "⅐", "⑴", " ", " ", "ß", "ẞ",
  "Ǆ", "ǅ", "Ĳ", "ĳ", "ḍ̇", "ḍ̇", "q̣̇", "ậ", "ậ", "ậ", "ậ",
  "ᾀ", "ᾳ", "̈́", "΅", "΅", "̀", "ཱི", "ཱུ", "ཱྀ", "⾀0", "\u{2F800}", "塚", "豈", "\u{1D15E}",
  "\u{1D160}", "퟿", "각", "가", "à̖", "à̖", "à́̂", "ﷺ", "⑴⑵",
];
for (const text of normalizeStrings.slice(0, 80)) {
  add(`["NFC","NFD","NFKC","NFKD"].map(function(f){return ${q(text)}.normalize(f)})`);
}
add('["a".normalize(),"a".normalize(undefined)]');
add('(function(){try{"a".normalize("nfc")}catch(e){return e.name+": "+e.message}})()');
add('(function(){try{"a".normalize(null)}catch(e){return e.name}})()');

// resolvedOptions e supportedLocalesOf.
const tags = [
  "en", "en-US", "en-GB", "pt-BR", "pt-PT", "de", "de-AT", "de-u-co-phonebk", "de-u-co-phonebk-kn", "es", "es-u-co-trad", "es-MX", "sv", "sv-FI", "tr", "ja",
  "zh", "zh-u-co-pinyin", "zh-u-co-stroke", "zh-Hant-TW", "ko", "ru", "el", "he", "ar", "hi", "th", "cs", "pl", "da", "fi", "nb", "no", "fr-CA", "it", "uk",
  "en-u-kn", "en-u-kn-false", "en-u-kf-upper", "en-u-kf-lower", "sv-u-kf-false", "en-u-co-emoji", "en-u-co-standard", "en-u-co-search", "und", "xx", "en-ZZ",
  "de-u-co-eor", "ja-u-co-unihan", "zh-u-co-zhuyin", "ar-u-co-compat", "th-u-nu-thai",
];
for (const tag of tags) {
  add(`new Intl.Collator(${q(tag)}).resolvedOptions()`);
  add(`Intl.Collator.supportedLocalesOf([${q(tag)},"en"])`);
  add(`Intl.Collator.supportedLocalesOf(${q(tag)},{localeMatcher:"best fit"})`);
}
for (const options of collatorOptions.slice(1).concat(['{usage:"search"}', '{usage:"search",sensitivity:"base"}', '{numeric:false,caseFirst:"false"}', '{collation:"phonebk"}'])) {
  for (const locale of ["en", "de", "sv", "tr", "ja"]) {
    add(`new Intl.Collator(${q(locale)},${options}).resolvedOptions()`);
  }
}
add('Intl.Collator.supportedLocalesOf(["en","xx","de-DE","zz-ZZ","sv","tlh"])');
add('Intl.Collator.supportedLocalesOf(["en-u-co-phonebk","de-u-kn-true"],{localeMatcher:"lookup"})');
add('(function(){try{new Intl.Collator("en",{sensitivity:"x"})}catch(e){return e.name}})()');
add('(function(){try{new Intl.Collator("en",{caseFirst:"x"})}catch(e){return e.name}})()');
add('(function(){try{new Intl.Collator("e")}catch(e){return e.name}})()');
add('typeof new Intl.Collator().compare+new Intl.Collator().compare.length+new Intl.Collator().compare.name');
add('Object.prototype.toString.call(new Intl.Collator())');

// Fatia de caixa por locale (acrescentada no fim para não mexer nas linhas anteriores):
// sigma final em posições variadas, SpecialCasing completo, formas do argumento locales e o lituano com marcas.
const sigmaWords = [
  "ΑΣ", "ΑΣΑ", "Σ", "ΑΣ.", "Α.Σ", "ΑΣ́", "Σ΄Α", "ΑΣ Α", "ΑΣ'", "Α­Σ", "ΑΣΣ", "ΑΣΣΑ", "Α1Σ", "ΣΑ", "ΑΣ​", "aΣ",
];
for (const word of sigmaWords) {
  for (const locale of ["undefined", '"el"', '"tr"', '"lt"', '"en"']) {
    add(`[${q(word)}.toLowerCase(),${q(word)}.toLocaleLowerCase(${locale})]`);
  }
}
const specialWords = ["ß", "ŉ", "ǰ", "ﬃ", "ᾳ", "ΐ", "ΰ", "ﬓ", "ᾼ", "ῼ", "ῷ", "ﬗ", "İ", "ﬆ"];
for (const word of specialWords) {
  add(
    `[${q(word)}.toUpperCase(),${q(word)}.toLowerCase(),${q(word)}.toLocaleUpperCase("tr"),${q(word)}.toLocaleUpperCase("el"),` +
      `${q(word)}.toLocaleUpperCase("lt"),${q(word)}.toLocaleLowerCase("lt")]`
  );
}
const localeForms = [
  '["tr"]', '["xx","tr"]', '["tr","en"]', '"TR"', '"tr-TR-u-co-x"', '"tr-u-ca-x"', '"tr_TR"', '""', "null", '"und"', '"und-TR"',
  '"tr-Latn"', '"az-Cyrl"', '"az-Arab"', '"lt-u-x"', '"el-polyton"', '"Tr"', '"i-tr"', '"sr-Latn"', '"zh"', 'new String("tr")',
  '{length:1,0:"tr"}', '["tr","tr"]', "[undefined]", "[null]", "[1]", '"tr-x-private"', '"x-tr"', '"root"', '"en-u-tr"', '"TR-u-nu-latn"',
  '"lt-1994"', '"az-AZ"', '"tr-CY"', '"tur"', '"tur-TR"', '"aze"', '["en","tr"]', '["en-x-a","tr"]', '[]',
];
for (const locale of localeForms) {
  add(`(function(){try{return ["Iİiı".toLocaleLowerCase(${locale}),"Iİiı".toLocaleUpperCase(${locale})]}catch(e){return e.name+": "+e.message.replace(/ \\(evaluating.*$/,"")}})()`);
}
const ltWords = ["Jı̇", "J̇", "Ị́", "Í", "Í̛", "Į́", "Ì̠", "i̇́", "ji̇", "Ì́", "Ĩ", "ÌÍ"];
for (const word of ltWords) {
  add(`[${q(word)}.toLocaleLowerCase("lt"),${q(word)}.toLocaleUpperCase("lt"),${q(word)}.toLocaleLowerCase("lt-LT"),${q(word)}.toLowerCase()]`);
}

for (const source of programs) {
  let result;
  try {
    result = JSON.stringify((0, eval)(source));
    if (result === undefined) result = "undefined";
  } catch (error) {
    result = "throw";
  }
  emitRow(source + "\t" + ascii(result));
}
