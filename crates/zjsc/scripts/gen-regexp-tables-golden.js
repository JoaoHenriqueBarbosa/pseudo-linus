// Gera tests/golden/regexp_tables_bun.tsv: amostragem das tabelas Unicode do yarr (case folding de /iu e /iv e
// fronteiras de intervalo de \p{...}) medida no bun 1.4.2. Cada linha é um programa (JSON) e o valor da variável
// global `R` (JSON), igual a gen-function-error-golden.js, e o teste roda o mesmo programa no zjsc.
// Uso: bun scripts/gen-regexp-tables-golden.js > tests/golden/regexp_tables_bun.tsv
const rows = [];

function emit(program) {
  delete globalThis.R;
  (0, eval)(program);
  rows.push(JSON.stringify(program) + "\t" + JSON.stringify(String(globalThis.R)));
}

// ---- Case folding: para cada X amostrado, quais candidatos casam com /\u{X}/iu e /\u{X}/iv.
const cp = (c) => String.fromCodePoint(c);
const limit = 0x30000;
const special = [0x130, 0x131, 0x17f, 0x1e9e, 0x212a, 0x2126, 0x3c2, 0x1c5, 0x345, 0x1fbe, 0xdf, 0x49, 0x69, 0x4b, 0x6b, 0x53, 0x73];
const cased = new Set(special);
for (let c = 0; c < limit; c++) {
  if (c >= 0xd800 && c <= 0xdfff) continue;
  const s = cp(c);
  if (s.toLowerCase() !== s || s.toUpperCase() !== s) cased.add(c);
}
const casedList = [...cased].sort((a, b) => a - b);
const casedStrings = casedList.map(cp);

const samples = new Set(casedList);
for (let c = 0; c < limit; c += 61) samples.add(c);
for (let k = 0; k < limit; k += 0x100) {
  samples.add(k);
  if (k > 0) samples.add(k - 1);
  samples.add(k + 1);
}
for (const c of [0, 0x7f, 0x80, 0xd7ff, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xe000, 0xffff, 0x10000, 0x1ffff, 0x2ffff]) samples.add(c);

function classOf(x) {
  if (!cased.has(x)) {
    const near = [x, x - 1, x + 1].filter((c) => c >= 0);
    return near;
  }
  const found = new Set([x, x - 1, x + 1]);
  for (const flags of ["iu", "iv"]) {
    const re = new RegExp("\\u{" + x.toString(16) + "}", flags);
    for (let i = 0; i < casedList.length; i++) if (re.test(casedStrings[i])) found.add(casedList[i]);
  }
  return [...found].filter((c) => c >= 0).sort((a, b) => a - b);
}

const sampleList = [...samples].sort((a, b) => a - b);
const chunk = 25;
for (let i = 0; i < sampleList.length; i += chunk) {
  const items = sampleList.slice(i, i + chunk).map((x) => "[" + x + ",[" + classOf(x).join(",") + "]]");
  emit(
    '"use strict";\nvar items = [' + items.join(",") + '];\nvar out = [];\n' +
      'for (var i = 0; i < items.length; i++) {\n' +
      '  for (var f = 0; f < 2; f++) {\n' +
      '    var re = new RegExp("\\\\u{" + items[i][0].toString(16) + "}", f ? "iv" : "iu");\n' +
      '    var row = "";\n' +
      '    for (var j = 0; j < items[i][1].length; j++) row += re.test(String.fromCodePoint(items[i][1][j])) ? "1" : "0";\n' +
      '    out.push(row);\n  }\n}\nglobalThis.R = out.join("|")',
  );
}

// ---- Propriedades: as fronteiras de cada intervalo (primeiro, último e vizinhos) de \p{...}.
// O bun mede os intervalos varrendo o espaço Unicode inteiro; o programa só testa as fronteiras.
const bmpLow = Array.from({ length: 0xd800 }, (_, c) => cp(c)).join("");
const bmpHigh = Array.from({ length: 0x10000 - 0xe000 }, (_, c) => cp(c + 0xe000)).join("");
let astral = "";
for (let c = 0x10000; c <= 0x10ffff; c++) astral += cp(c);

function rangesOf(expression) {
  const re = new RegExp("(?:" + expression + ")+", "gu");
  const out = [];
  const scan = (text, toCp) => {
    re.lastIndex = 0;
    let m;
    while ((m = re.exec(text))) {
      out.push([toCp(m.index), toCp(m.index + m[0].length - (m[0].length > 0 && toCp === astralCp ? 2 : 1))]);
    }
  };
  const astralCp = (index) => 0x10000 + index / 2;
  scan(bmpLow, (i) => i);
  scan(bmpHigh, (i) => i + 0xe000);
  scan(astral, astralCp);
  for (let c = 0xd800; c <= 0xdfff; c += 1) {
    if (new RegExp(expression, "u").test(String.fromCharCode(c))) out.push([c, c]);
  }
  out.sort((a, b) => a[0] - b[0]);
  const merged = [];
  for (const r of out) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1] + 1) last[1] = Math.max(last[1], r[1]);
    else merged.push([r[0], r[1]]);
  }
  return merged;
}

function boundaryPoints(ranges) {
  let picked = ranges;
  if (ranges.length > 40) {
    const keep = new Set();
    for (let i = 0; i < 12; i++) keep.add(i), keep.add(ranges.length - 1 - i);
    for (let i = 0; i < 16; i++) keep.add(Math.floor((i * ranges.length) / 16));
    picked = [...keep].filter((i) => i >= 0 && i < ranges.length).sort((a, b) => a - b).map((i) => ranges[i]);
  }
  const points = new Set([0, 0x10ffff]);
  for (const [a, b] of picked) {
    for (const c of [a - 1, a, a + 1, b - 1, b, b + 1]) if (c >= 0 && c <= 0x10ffff) points.add(c);
  }
  return [...points].sort((a, b) => a - b);
}

const scripts = [
  "Latin", "Greek", "Cyrillic", "Han", "Hiragana", "Katakana", "Arabic", "Hebrew", "Thai", "Devanagari", "Bengali",
  "Tamil", "Telugu", "Georgian", "Armenian", "Hangul", "Ethiopic", "Khmer", "Mongolian", "Tibetan", "Common",
  "Inherited", "Cherokee", "Coptic", "Gothic", "Runic", "Ogham", "Braille", "Lao", "Myanmar", "Sinhala", "Gujarati",
  "Gurmukhi", "Kannada", "Malayalam", "Oriya", "Syriac", "Thaana", "Tifinagh", "Yi", "Bopomofo", "Canadian_Aboriginal",
  "Cuneiform", "Egyptian_Hieroglyphs", "Linear_B", "Phoenician", "Glagolitic", "Adlam", "Vithkuqi", "Toto",
  "Nag_Mundari", "Kawi", "Cypro_Minoan", "Old_Uyghur", "Tangsa", "Tai_Tham", "Brahmi", "Balinese", "Javanese",
  "Sundanese", "Latn", "Grek", "Cyrl", "Hani", "Zyyy", "Zinh", "Qaai", "Unknown", "Zzzz",
];
const extensions = [
  "Latin", "Greek", "Cyrillic", "Han", "Hiragana", "Katakana", "Arabic", "Devanagari", "Common", "Inherited",
  "Bengali", "Tamil", "Thaana", "Syriac", "Coptic", "Georgian", "Hangul", "Bopomofo", "Yi", "Mongolian", "Telugu",
  "Kannada", "Gujarati", "Gurmukhi", "Limbu", "Javanese", "Latn", "Arab", "Deva", "Zyyy", "Zinh",
];
const categories = [
  "Lu", "Ll", "Lt", "Lm", "Lo", "L", "LC", "Mn", "Mc", "Me", "M", "Nd", "Nl", "No", "N", "Pc", "Pd", "Ps", "Pe",
  "Pi", "Pf", "Po", "P", "Sm", "Sc", "Sk", "So", "S", "Zs", "Zl", "Zp", "Z", "Cc", "Cf", "Cs", "Co", "Cn", "C",
  "Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter", "Letter", "Cased_Letter", "Nonspacing_Mark",
  "Decimal_Number", "Number", "Punctuation", "Symbol", "Separator", "Control", "Format", "Surrogate", "Private_Use",
  "Unassigned", "Other", "digit", "punct", "Combining_Mark",
];
const binary = [
  "ASCII", "ASCII_Hex_Digit", "AHex", "Alphabetic", "Alpha", "Any", "Assigned", "Bidi_Control", "Bidi_Mirrored",
  "Case_Ignorable", "Cased", "Changes_When_Casefolded", "Changes_When_Casemapped", "Changes_When_Lowercased",
  "Changes_When_NFKC_Casefolded", "Changes_When_Titlecased", "Changes_When_Uppercased", "Dash",
  "Default_Ignorable_Code_Point", "Deprecated", "Diacritic", "Emoji", "Emoji_Component", "Emoji_Modifier",
  "Emoji_Modifier_Base", "Emoji_Presentation", "Extended_Pictographic", "Extender", "Grapheme_Base",
  "Grapheme_Extend", "Hex_Digit", "IDS_Binary_Operator", "IDS_Trinary_Operator", "ID_Continue", "ID_Start",
  "Ideographic", "Join_Control", "Logical_Order_Exception", "Lowercase", "Math", "Noncharacter_Code_Point",
  "Pattern_Syntax", "Pattern_White_Space", "Quotation_Mark", "Radical", "Regional_Indicator", "Sentence_Terminal",
  "Soft_Dotted", "Terminal_Punctuation", "Unified_Ideograph", "Uppercase", "Variation_Selector", "White_Space",
  "XID_Continue", "XID_Start", "IDC", "IDS", "Upper", "Lower", "space", "VS", "RI", "Ideo", "EPres", "EBase",
];

const props = [
  ...scripts.flatMap((v) => ["Script=" + v, "sc=" + v]).filter((_, i) => i % 2 === 0 || i < 40),
  ...extensions.map((v) => "Script_Extensions=" + v),
  ...extensions.slice(0, 12).map((v) => "scx=" + v),
  ...categories.map((v) => "General_Category=" + v),
  ...categories.slice(0, 20).map((v) => "gc=" + v),
  ...binary,
];

let count = 0;
for (const prop of props) {
  const expression = "\\p{" + prop + "}";
  try {
    new RegExp(expression, "u");
  } catch (error) {
    continue;
  }
  const points = boundaryPoints(rangesOf(expression));
  for (const flags of ["u", "v"]) {
    emit(
      '"use strict";\nvar re = new RegExp(' + JSON.stringify(expression) + ', "' + flags + '");\n' +
        'var ne = new RegExp(' + JSON.stringify("\\P{" + prop + "}") + ', "' + flags + '");\n' +
        'var a = [' + points.join(",") + '];\nvar o = "";\n' +
        'for (var i = 0; i < a.length; i++) {\n  var s = String.fromCodePoint(a[i]);\n' +
        '  o += (re.test(s) ? "1" : "0") + (ne.test(s) ? "1" : "0");\n}\nglobalThis.R = o',
    );
  }
  count++;
}
console.error("propriedades: " + count + ", linhas: " + rows.length);
process.stdout.write(require("./golden-prelude.js").assertPublicResult(rows.join("\n") + "\n"));
