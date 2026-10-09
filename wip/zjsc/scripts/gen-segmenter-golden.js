const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/segmenter_bun.tsv: segmentação do Intl.Segmenter medida no bun.
// Colunas: granularidade, locale, texto (literal JS em ASCII, o resto vira \uXXXX) e o resultado
// (JSON ASCII com os segmentos [segment, index, isWordLike] e o índice de `containing(i)` para
// i de 0 a length, -1 quando devolve undefined). tests/segmenter_bun_golden.rs roda o mesmo programa
// na engine e compara com a quarta coluna.
// Uso: bun scripts/gen-segmenter-golden.js > tests/golden/segmenter_bun.tsv
function ascii(text) {
  return text.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function q(text) {
  return ascii(JSON.stringify(text));
}

const texts = [
  // Emoji, ZWJ, modificadores de pele, bandeiras, teclas
  "\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}",
  "\u{1F44D}\u{1F3FD} ok",
  "\u{1F1E7}\u{1F1F7}\u{1F1FA}\u{1F1F8}\u{1F1EF}\u{1F1F5}",
  "\u{1F1E7}\u{1F1F7}\u{1F1FA}",
  "1️⃣ 2️⃣",
  "❤️‍\u{1F525}",
  "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
  "\u{1F600}\u{1F601}\u{1F602}",
  "\u{1F9D1}‍\u{1F4BB} codes",
  "☺️ ☺︎",
  // Combinantes
  "éàô",
  "á̂̃b",
  "éè café",
  "ñü",
  "Å̈",
  // Hangul
  "한",
  "한국어",
  "한글 단어 문장입니다.",
  "각가",
  // Devanagari
  "नमस्ते",
  "क्षि",
  "हिन्दी भाषा",
  "काम करो। अब जाओ।",
  "กํา",
  // Tailandês
  "สวัสดีครับ",
  "ผมรักคุณ",
  "ประเทศไทย",
  "วันนี้อากาศดี เราไปเที่ยวกัน",
  "สวัสดี hello 123",
  "เด็กๆ เล่น",
  // Japonês
  "今日は東京で天気がいい",
  "私は学生です。あなたは先生ですか。",
  "コンピューターとプログラミング",
  "こんにちは世界",
  "東京都渋谷区",
  "カタカナとひらがなと漢字",
  "「こんにちは」と言った。",
  "日本語の文章を分割します",
  "食べていました",
  "iPhoneを買った 3個",
  // Chinês
  "我爱北京天安门",
  "中华人民共和国",
  "今天天气很好，我们去公园玩。",
  "这是一个测试。它有两句话。",
  "我喜欢编程和机器学习",
  "自然语言处理",
  "繁體中文測試句子",
  "他说：“你好！”",
  "中文English混合123",
  "一二三四五六七八九十",
  // Inglês: apóstrofo, hífen, números
  "Hello, world!",
  "don't can't won't it's",
  "rock-n-roll well-known",
  "O'Neil's e Mary's",
  "3.14 1,000,000 1.2.3",
  "$1,234.56 and 50% off",
  "user_name foo_bar2 x1y2",
  "a.b.c.d e.g. i.e.",
  "U.S.A. is big. U.S. too.",
  "https://example.com/a?b=c#d",
  "mail@example.com, a.b@c.org",
  "The quick brown fox.",
  "It's 5 o'clock; isn't it?",
  "e-mail e‑mail",
  "a’b l’homme",
  "1st 2nd 3rd 4th",
  "12:30:45 2024-01-02",
  "word  two   spaces",
  "tab\there\tnow",
  "café naïve résumé",
  "Ångström Über straße",
  "Привет, мир!",
  "مرحبا بالعالم",
  "שלום עולם",
  "γειά σου κόσμε",
  "๒๓๔ ١٢٣ १२",
  "لاـلا",
  "؀١٢",
  // Sentenças
  "Hello world. How are you? I'm fine! Thanks.",
  "Mr. Smith went to Washington. He arrived at 3 p.m. and left.",
  "He said \"Hi.\" Then he left.",
  "What?! No way... Really?",
  "First line.\nSecond line.\r\nThird line.\rFourth.",
  "Paragraph one. Paragraph two. Line sep.",
  "see fig. 3. it's lower case. Next One.",
  "Is it 3.5 or 4.5? I think 3.5.",
  "(He left.) She stayed. [Really.] OK.",
  "End with quote.” Next sentence.",
  "a. b. c. d.",
  "¿Cómo estás? ¡Muy bien! Gracias.",
  "Olá, tudo bem? Sim. Obrigado.",
  "etc. and so on. The end.",
  "No terminator at the end",
  "Trailing spaces.   \n  Next.",
  "“Quoted sentence.” And another.",
  "Dr. Who? Yes. Dr. who is here.",
  "1. First 2. Second 3. Third",
  "Hi… there. Wait… what?",
  "こんにちは。元気ですか？はい！",
  "你好。再见！你好吗？",
  "ประโยคแรก ประโยคสอง",
  "한국어 문장. 두 번째 문장입니다!",
  // Controles, quebras, bordas
  "\r\n",
  "\n\r",
  "a\r\nb",
  "a‍b",
  "‍",
  "​‌",
  "\u0001\u0002\u007F",
  "­ soft­hyphen",
  "﻿bom text",
  " nbsp text",
  " em space",
  "",
  " ",
  "a",
  ".",
  "...",
  "a.b",
  "'quoted'",
  "\"double\"",
  "\u{1D400}\u{1D401}",
  "\u{20BB7}\u{20BB7}",
  "\uD800",
  "a\uD800b",
  "\uDC00\uD800",
  "\u{1F600}\uD83D",
  "x\u{10FFFF}y",
  "\u{E0100}",
  "a\u{E0100}b",
  // Mistos
  "Tokyo 東京 Beijing 北京 Bangkok กรุงเทพ",
  "café☕ and \u{1F355}!",
  "テストtestテスト",
  "あいうえお",
  "アイウエオ",
  "ーーー",
  "々々人々",
  "　全角スペース　",
  "ＡＢＣ１２３",
  "ｶﾅｶﾅ",
  "໐໑ການ",
  "ការប្រៃ",
  "ကျမန်မာ",
  "க்ஷ",
  "ਕ੍ਰ",
  "ക്ഷ",
  "مَرْحَبًا",
  "ཀྱི",
  "ẞß",
  "①② ⅠⅡ",
  "² ³ ½",
  "x̀́̂ y",
  "अंक",
];

// A lista tem 153 textos; os 3 últimos (cada um já coberto por outro de escrita parecida) ficam de fora.
texts.length = 150;
if (new Set(texts).size !== texts.length) throw new Error("textos repetidos");

const locales = ["en", "ja", "zh", "th"];
const granularities = ["grapheme", "word", "sentence"];
for (const text of texts) {
  for (const granularity of granularities) {
    for (const locale of locales) {
      const segmenter = new Intl.Segmenter(locale, { granularity });
      const segments = segmenter.segment(text);
      const parts = [...segments].map((s) => [s.segment, s.index, s.isWordLike]);
      const containing = [];
      for (let i = 0; i <= text.length; i++) {
        const found = segments.containing(i);
        containing.push(found === undefined ? -1 : found.index);
      }
      const result = ascii(JSON.stringify({ parts, containing }));
      emitRow([granularity, locale, q(text), result].join("\t"));
    }
  }
}
