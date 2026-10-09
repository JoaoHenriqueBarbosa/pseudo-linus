// Gera tests/golden/segmenter_locales_bun.tsv: o Intl.Segmenter medido no bun nos locales que
// tests/golden/segmenter_bun.tsv não cobre (pt, de, ko, hi). Mesmas colunas e mesmo programa do
// gen-segmenter-golden.js: granularidade, locale, texto (literal JS em ASCII) e o resultado em JSON ASCII
// com os segmentos [segment, index, isWordLike] e o índice de `containing(i)` de 0 a length (-1 para undefined).
// Como regenerar (da raiz de wip/zjsc):  bun scripts/gen-segmenter-locales-golden.js
const fs = require("fs");
const path = require("path");

function ascii(text) {
  return text.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
function q(text) {
  return ascii(JSON.stringify(text));
}

const texts = [
  // Português
  "Olá, mundo! Tudo bem?",
  "O Dr. Silva chegou às 10h30. Depois saiu.",
  "São Paulo, 25 de janeiro de 2024.",
  "pré-história e guarda-chuva",
  "R$ 1.234,56 e 3,14%",
  "Ação, coração, não e à toa",
  "Ele disse: \"vamos embora\". Ela riu.",
  "e-mail: joao@exemplo.com.br; site www.exemplo.com.br",
  "Isto é um teste... de reticências!",
  "1) primeiro; 2) segundo",
  // Alemão
  "Straßenbahn und Fußball",
  "Das ist z. B. ein Test. Und noch einer!",
  "Müller, Köln, Österreich",
  "Donaudampfschifffahrtsgesellschaft",
  "3,5 Mio. € und 1.000.000 Stück",
  "„Hallo“, sagte er. „Wie geht’s?“",
  "Dr. Meier kam um 9:30 Uhr an.",
  "Zwei-Euro-Münze",
  // Coreano
  "한국어",
  "안녕하세요 세계",
  "저는 학생입니다. 당신은 선생님입니까?",
  "한글은 훌륭한 문자입니다",
  "각가나다라",
  "서울특별시 강남구 123번지",
  "ㅎㅏㄴ",
  "가ᅠ각",
  // Hindi
  "नमस्ते दुनिया",
  "मैं हिन्दी बोलता हूँ। आप कैसे हैं?",
  "क्षत्रिय ज्ञान श्रीमान्",
  "भारत 1947 में आज़ाद हुआ।",
  "कृपया ध्यान दें: यह परीक्षण है!",
  "पुस्तकालय, विद्यालय, और महाविद्यालय",
  // Emoji, bandeiras e controles
  "\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}",
  "\u{1F44D}\u{1F3FD} ok",
  "\u{1F1E7}\u{1F1F7}\u{1F1E9}\u{1F1EA}\u{1F1F0}\u{1F1F7}",
  "1️⃣ 2️⃣",
  "a\r\nb\nc\rd",
  "tab\tseparado e  espaços",
  "",
  " ",
  // Misturas
  "Olá 世界 Hallo 안녕 नमस्ते",
  "COVID-19, Wi-Fi e iPhone15",
  "3.14 e 1,000.50 e 2:30:15",
  "don't stop\u2014believing",
  "O'Neil's e l'été",
  "ÀÉÎÕÜ àéîõü ǅ ß",
  "é à̂ ö",
  "Fim. Início! Meio? Sim...",
  "Uma frase.\n\nOutra frase após quebra.",
  "Ponto no meio.3 e v1.2.3 e a.b.c",
  "(parênteses) [colchetes] {chaves} <ângulos>",
];

if (new Set(texts).size !== texts.length) throw new Error("textos repetidos");

const locales = ["pt", "de", "ko", "hi"];
const granularities = ["grapheme", "word", "sentence"];
const lines = [];
for (const text of texts) {
  for (const granularity of granularities) {
    for (const locale of locales) {
      const segments = new Intl.Segmenter(locale, { granularity }).segment(text);
      const parts = [...segments].map((s) => [s.segment, s.index, s.isWordLike]);
      const containing = [];
      for (let i = 0; i <= text.length; i++) {
        const found = segments.containing(i);
        containing.push(found === undefined ? -1 : found.index);
      }
      lines.push([granularity, locale, q(text), ascii(JSON.stringify({ parts, containing }))].join("\t"));
    }
  }
}
fs.writeFileSync(path.join(__dirname, "..", "tests", "golden", "segmenter_locales_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.log(lines.length + " linhas");
