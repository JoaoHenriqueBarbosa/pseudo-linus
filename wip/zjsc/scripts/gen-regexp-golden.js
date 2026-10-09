// Gera tests/golden/regexp_v_bun.tsv: RegExp além do que regexp_more_bun.tsv cobre, medido no bun 1.4.2.
// Flags v (unicode sets), d (indices), s, y, g com lastIndex, lookbehind, grupos nomeados duplicados, modificadores
// `(?i:...)`, propriedades Unicode raras, case folding, quantificadores enormes, métodos Symbol.* com subclasses,
// replaceAll/matchAll, RegExp.escape, source/flags/toString e 200 SyntaxError com mensagem exata.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Uso: bun scripts/gen-regexp-golden.js > tests/golden/regexp_v_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram, RESULT_PRELOAD } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const j = JSON.stringify;

// Prelude: M descreve um resultado de match de forma determinística; E captura exceção como "Nome: mensagem".
const PRELUDE =
  "function M(m) { if (m === null) return 'null'; if (m === undefined) return 'undefined'; if (typeof m !== 'object') return String(m);" +
  " var o = { a: Array.from(m), i: m.index, g: m.groups === undefined ? 'u' : Object.entries(m.groups) };" +
  " if (m.indices) o.d = Array.from(m.indices); if (m.indices && m.indices.groups) o.dg = Object.entries(m.indices.groups);" +
  " return JSON.stringify(o) }\n" +
  "function E(f) { try { return f() } catch (e) { return e.name + ': ' + e.message } }\n";
const add = body => programs.push(PRELUDE + body);

const ex = (p, f, s) => add(`R = E(function () { var r = new RegExp(${j(p)}, ${j(f)}); var m = r.exec(${j(s)}); return M(m) + '|' + r.lastIndex })`);
const all = (p, f, s) => add(`R = E(function () { return JSON.stringify(Array.from(${j(s)}.matchAll(new RegExp(${j(p)}, ${j(f)} + 'g')), function (m) { return [m[0], m.index] })) })`);

// ---- Flag v: classes aninhadas, interseção, subtração, \q{}, propriedades de strings.
const vPatterns = [
  "[[a-z]--[aeiou]]", "[[a-z]&&[aeiou]]", "[\\w--\\d]", "[\\w&&\\d]", "[\\p{L}--\\p{Lu}]", "[\\p{L}&&\\p{ASCII}]",
  "[\\q{abc|d|ef}]", "[\\q{abc|d|ef}x]", "[\\q{}]", "[\\q{a}]", "[\\q{ab}--\\q{ab}]", "[\\q{abc|d}&&\\q{abc}]", "[^\\q{a}]",
  "[^[a-c]]", "[^[a-c]--[b]]", "[[a-c][x-z]]", "[[[a-b]--[b]][d-e]]", "[a-c&&b-d]", "[\\p{ASCII}--[a-z]--[A-Z]]",
  "[\\p{Lu}&&[A-F]&&[C-Z]]", "[(\\)]", "[\\(\\)\\[\\]\\{\\}\\/\\-\\\\\\|]", "[a&&&&b]".replace("&&&&", "&&"),
  "\\p{RGI_Emoji}", "\\p{Emoji_Keycap_Sequence}", "\\p{RGI_Emoji_Flag_Sequence}", "\\p{RGI_Emoji_Modifier_Sequence}",
  "\\p{Basic_Emoji}", "\\p{RGI_Emoji_Tag_Sequence}", "\\p{RGI_Emoji_ZWJ_Sequence}", "[\\p{RGI_Emoji}--\\q{\\u{1F600}}]",
  "[\\p{Emoji_Keycap_Sequence}\\q{x}]", "^\\p{RGI_Emoji}+$", "\\P{Lu}", "[\\P{Lu}]", "[^\\P{Lu}]", "\\P{ASCII}", "[\\u{1F600}-\\u{1F64F}]",
  "[\\u{10000}-\\u{10FFFF}]", ".", "[.]", "[\\-]", "[a-z&&\\w]", "[\\d--[5]]", "\\p{Any}", "\\p{Assigned}", "[^\\p{Any}]",
  "[\\q{a|b|c}--\\q{b}]", "[\\q{aa|a}]", "[\\q{ab|abc}]", "[\\q{abc|ab}]", "(?:[\\q{ab|a}])c", "[\\q{ab|a}]+",
];
const vInputs = ["a", "abc", "ef", "d", "b", "5", "A", "\u{1F600}", "\u{1F1E7}\u{1F1F7}", "1\uFE0F\u20E3", "\u{1F468}\u200D\u{1F469}\u200D\u{1F467}",
  "\u{1F44D}\u{1F3FD}", "\u00e9", "aab", "()"];
for (const p of vPatterns) for (const s of vInputs) ex(p, "v", s);
for (const p of ["[\\p{RGI_Emoji}]", "\\p{RGI_Emoji}", "[\\q{abc|d}]"]) {
  all(p, "v", "xabcd\u{1F600}\u{1F1E7}\u{1F1F7}d1\uFE0F\u20E3");
  all(p, "vi", "xABCDabc");
}

// ---- Flag d: indices e groups.
const dPatterns = ["(a)(b)?", "(?<x>a)(?<y>b)?", "(?<x>a)|(?<y>b)", "a(?=b)", "(?<=a)b", "(a)|(b)", "(?:(a)|b)+", "(a*)*", "(?<n>\\d+)-(?<m>\\d+)",
  "(?<a>.)\\k<a>", "((a)|(b))+", "(?<x>(?<y>a)b)", "\\u{1F600}", "()", "(a)\\1", "(?<é>a)", "(?<$>a)(?<_>b)"];
const dInputs = ["ab", "b", "xaab", "12-34", "aa", "\u{1F600}", "a", "bbab"];
for (const p of dPatterns) for (const s of dInputs) { ex(p, "d", s); ex(p, "du", s); ex(p, "dg", s); }

// ---- Flags s, y, g com lastIndex.
const sticky = [["a", "aaa"], ["a", "baa"], ["a*", "aab"], ["^a", "ba"], ["$", "ab"], ["(?:)", "xyz"], ["\\bb", "ab b"], ["a|b", "xb"], ["(?<=a)b", "ab"], [".", "\u{1F600}"]];
for (const [p, s] of sticky) for (const f of ["y", "g", "gy", "yu", "gu", "yd", "ys", "my"]) for (const li of [0, 1, 2, 3, 5, -1]) {
  add(`R = E(function () { var r = new RegExp(${j(p)}, ${j(f)}); r.lastIndex = ${li}; var a = M(r.exec(${j(s)})); return a + '|' + r.lastIndex + '|' + r.test(${j(s)}) + '|' + r.lastIndex })`);
}
for (const [p, s] of [[".", "a\nb"], [".+", "a\nb\r\nc\u2028d"], ["a.b", "a\nb"], ["a.b", "a\u2029b"], ["[^]", "\n"], ["^b", "a\nb"], ["a$", "a\nb"]]) for (const f of ["", "s", "m", "ms", "su", "sv"]) ex(p, f, s);
add("R = E(function () { var r = /a/g; r.lastIndex = 5; var o = [r.test('aaa'), r.lastIndex]; r.lastIndex = 1; o.push(r.test('aaa'), r.lastIndex); return JSON.stringify(o) })");
add("R = E(function () { var r = /a/y; var o = []; for (var i = 0; i < 4; i++) o.push(r.test('aab'), r.lastIndex); return JSON.stringify(o) })");
add("R = E(function () { var r = /a/g; Object.defineProperty(r, 'lastIndex', { writable: false, value: 0 }); return 'x' + r.test('a') })");
add("R = E(function () { var r = /a/; Object.defineProperty(r, 'lastIndex', { writable: false, value: 0 }); return 'x' + r.test('a') })");
add("R = E(function () { var r = /a/g; r.lastIndex = { valueOf: function () { return 2 } }; r.test('aaa'); return r.lastIndex })");
add("R = E(function () { var r = /a/g; r.lastIndex = 2 ** 53; return String(r.test('a')) + r.lastIndex })");

// ---- Lookbehind (inclusive variável).
const lb = ["(?<=a+)b", "(?<=a*)b", "(?<!a+)b", "(?<=(a+))b", "(?<=(\\d+)(\\d+))$", "(?<=\\1(a))b", "(?<=(a)\\1)b", "(?<=a|bc)d", "(?<=bc|a)d",
  "(?<=^a)b", "(?<=a$)b", "(?<=\\b)a", "(?<=(?<x>a)b)c", "(?<=a{2,3})b", "(?<!a{2,3})b", "(?<=[a-c]+)d", "(?<=\\w+@)\\w+", "(?<=ab|a)c",
  "(?<=(?=a)a)b", "(?<=(?<!x)a)b", "(?<=.)b", "(?<=\\u{1F600})x", "(?<=(?:a|ab)+)c", "(?<=(?<a>.)\\k<a>)x"];
for (const p of lb) for (const s of ["ab", "aab", "abcd", "bcd", "12345", "xx", "aabc", "abbc", "x\u{1F600}x"]) { ex(p, "", s); ex(p, "u", s); }

// ---- Grupos nomeados duplicados.
const dup = ["(?<a>x)|(?<a>y)", "(?:(?<a>x)|(?<a>y))\\k<a>", "(?<a>x)(?<a>y)", "(?<a>a)|b|(?<a>c)", "(?:(?<a>a)|(?<a>b))+",
  "(?<a>a)|(?<b>b)|(?<a>c)", "(?:(?<a>a)|b)(?<a>c)", "(?<a>a)(?:|(?<a>b))", "(?:(?<a>.)|(?<b>.))\\k<a>\\k<b>"];
for (const p of dup) for (const s of ["x", "y", "xy", "xx", "yy", "a", "b", "c", "ab", "abab"]) { ex(p, "", s); ex(p, "d", s); }
add("R = E(function () { var m = /(?<a>x)|(?<a>y)/.exec('y'); return JSON.stringify([m.groups.a, Object.keys(m.groups)]) })");
add("R = E(function () { return 'yx'.replace(/(?<a>x)|(?<a>y)/g, '[$<a>]') })");

// ---- Modificadores (?i:...), (?-i:...), (?ims-ims:...).
const mods = ["(?i:a)b", "(?i:a)B", "a(?i:b)", "(?i:a(?-i:b))", "(?-i:a)b", "(?s:.)", "(?-s:.)", "(?m:^b)", "(?-m:^b)", "(?ims:a.^b)", "(?i-s:a.)",
  "(?i:[a-z])", "(?i:\\u212a)", "(?i-:a)", "(?i:(a))\\1", "(?i:\\p{Lu})", "(?i:\\P{Lu})", "(?i:[^a])", "(?i:\\w)", "(?i:\\k<x>)(?<x>a)", "(?:(?i:a)|b)c",
  "(?i:a|b)c", "(?-i:a)", "(?m-m:a$)", "(?s-s:a.)"];
for (const p of mods) for (const [f, s] of [["", "aB"], ["i", "Ab"], ["", "A\nb"], ["u", "\u212a"], ["v", "K"], ["s", "a\nb"], ["m", "a\nb"]]) ex(p, f, s);

// ---- Backreferences nomeadas.
const nb = ["(?<a>.)\\k<a>", "\\k<a>(?<a>.)", "(?<a>.)\\k<b>", "\\k<a>", "(?<a>a)\\k", "(?<a>a)\\k<", "(?<a>a)\\k<a", "(?<a>a)\\k<>", "(?<a>a)\\1", "(?<a>a)(?<b>b)\\k<b>\\k<a>",
  "(?<a>a|b)\\k<a>", "(?<a>a)|\\k<a>", "(?:(?<a>a)|b)\\k<a>", "\\k<a>(?<a>a)?", "(?<\\u0061>.)\\k<a>", "(?<\\u{61}>.)\\k<a>", "(?<a\\u{1F600}>.)", "(?<𝒜>.)\\k<𝒜>"];
for (const p of nb) for (const s of ["aa", "bb", "ab", "abba", ""]) { ex(p, "", s); ex(p, "u", s); }

// ---- Propriedades Unicode: conta de pontos de código em 0..0x2FFFF (e, para Script, as primeiras ocorrências).
const props = [
  "Script=Adlam", "Script=Ahom", "Script=Anatolian_Hieroglyphs", "Script=Avestan", "Script=Balinese", "Script=Bamum", "Script=Bassa_Vah",
  "Script=Batak", "Script=Bhaiksuki", "Script=Brahmi", "Script=Buginese", "Script=Buhid", "Script=Carian", "Script=Caucasian_Albanian", "Script=Chakma",
  "Script=Chorasmian", "Script=Coptic", "Script=Cypriot", "Script=Cypro_Minoan", "Script=Dives_Akuru", "Script=Dogra", "Script=Duployan",
  "Script=Egyptian_Hieroglyphs", "Script=Elbasan", "Script=Elymaic", "Script=Garay", "Script=Grantha", "Script=Gunjala_Gondi", "Script=Hanifi_Rohingya",
  "Script=Hatran", "Script=Imperial_Aramaic", "Script=Inscriptional_Pahlavi", "Script=Kaithi", "Script=Kawi", "Script=Khitan_Small_Script",
  "Script=Khojki", "Script=Khudawadi", "Script=Kirat_Rai", "Script=Linear_A", "Script=Linear_B", "Script=Lycian", "Script=Lydian", "Script=Mahajani",
  "Script=Makasar", "Script=Mandaic", "Script=Manichaean", "Script=Marchen", "Script=Masaram_Gondi", "Script=Medefaidrin", "Script=Meetei_Mayek",
  "Script=Mende_Kikakui", "Script=Meroitic_Cursive", "Script=Meroitic_Hieroglyphs", "Script=Modi", "Script=Mro", "Script=Multani", "Script=Nabataean",
  "Script=Nag_Mundari", "Script=Nandinagari", "Script=Newa", "Script=Nushu", "Script=Nyiakeng_Puachue_Hmong", "Script=Old_Hungarian", "Script=Old_Italic",
  "Script=Old_North_Arabian", "Script=Old_Permic", "Script=Old_Persian", "Script=Old_Sogdian", "Script=Old_South_Arabian", "Script=Old_Turkic",
  "Script=Old_Uyghur", "Script=Osage", "Script=Pahawh_Hmong", "Script=Palmyrene", "Script=Pau_Cin_Hau", "Script=Phags_Pa", "Script=Psalter_Pahlavi",
  "Script=Sharada", "Script=Siddham", "Script=SignWriting", "Script=Sogdian", "Script=Sora_Sompeng", "Script=Soyombo", "Script=Sunuwar",
  "Script=Tangsa", "Script=Tangut", "Script=Tirhuta", "Script=Todhri", "Script=Toto", "Script=Tulu_Tigalari", "Script=Vithkuqi", "Script=Wancho",
  "Script=Warang_Citi", "Script=Yezidi", "Script=Zanabazar_Square", "Script=Latn", "Script=Latin", "Script=Grek", "Script=Zyyy", "Script=Zinh",
  "Script=Zzzz", "Script=Qaai", "Script=Qaac", "Script=Common", "Script=Inherited", "Script=Unknown", "Script=Hira", "Script=Kana", "Script=Hani",
  "scx=Latn", "scx=Grek", "scx=Deva", "scx=Beng", "scx=Hira", "scx=Kana", "scx=Hani", "scx=Arab", "scx=Syrc", "scx=Cyrl", "scx=Zyyy", "scx=Zinh",
  "Script_Extensions=Common", "Script_Extensions=Inherited", "Script_Extensions=Copt", "Script_Extensions=Qaac", "Script_Extensions=Mong",
  "Script_Extensions=Thaa", "Script_Extensions=Yiii", "Script_Extensions=Bopo", "Script_Extensions=Hang", "Script_Extensions=Gujr", "Script_Extensions=Guru",
  "General_Category=Lu", "General_Category=Cased_Letter", "General_Category=LC", "gc=Lm", "gc=Lo", "gc=Mn", "gc=Mc", "gc=Me", "gc=Nd", "gc=Nl", "gc=No",
  "gc=Pc", "gc=Pd", "gc=Ps", "gc=Pe", "gc=Pi", "gc=Pf", "gc=Po", "gc=Sm", "gc=Sc", "gc=Sk", "gc=So", "gc=Zs", "gc=Zl", "gc=Zp", "gc=Cc", "gc=Cf",
  "gc=Cs", "gc=Co", "gc=Cn", "gc=Unassigned", "gc=Decimal_Number", "gc=digit", "gc=punct", "gc=Punctuation", "gc=Combining_Mark", "gc=Mark",
  "gc=Separator", "gc=Symbol", "gc=Other", "gc=Control", "gc=cntrl", "gc=Private_Use", "gc=Surrogate", "gc=Format", "gc=Letter_Number",
  "gc=Titlecase_Letter", "gc=Lt", "gc=Ll", "gc=Dash_Punctuation", "gc=Initial_Punctuation", "gc=Final_Punctuation", "gc=Space_Separator",
  "L", "Lu", "LC", "M", "N", "P", "S", "Z", "C", "Cn", "Letter", "Mark", "Number",
  "ASCII", "ASCII_Hex_Digit", "AHex", "Alphabetic", "Alpha", "Any", "Assigned", "Bidi_Control", "Bidi_Mirrored", "Case_Ignorable", "Cased",
  "Changes_When_Casefolded", "Changes_When_Casemapped", "Changes_When_Lowercased", "Changes_When_NFKC_Casefolded", "Changes_When_Titlecased",
  "Changes_When_Uppercased", "Dash", "Default_Ignorable_Code_Point", "Deprecated", "Diacritic", "Emoji", "Emoji_Component", "Emoji_Modifier",
  "Emoji_Modifier_Base", "Emoji_Presentation", "Extended_Pictographic", "Extender", "Grapheme_Base", "Grapheme_Extend", "Hex_Digit",
  "IDS_Binary_Operator", "IDS_Trinary_Operator", "IDS_Unary_Operator", "ID_Continue", "ID_Start", "Ideographic", "Join_Control", "Logical_Order_Exception",
  "Lowercase", "Math", "Noncharacter_Code_Point", "Pattern_Syntax", "Pattern_White_Space", "Quotation_Mark", "Radical", "Regional_Indicator",
  "Sentence_Terminal", "Soft_Dotted", "Terminal_Punctuation", "Unified_Ideograph", "Uppercase", "Variation_Selector", "White_Space", "XID_Continue",
  "XID_Start", "Lowercase_Letter", "Uppercase_Letter", "Cased_Letter",
];
for (const p of props) {
  add(`R = E(function () { var r = new RegExp(${j("\\p{" + p + "}")}, 'u'); var n = 0, first = -1, last = -1; for (var c = 0; c < 0x30000; c++) if (r.test(String.fromCodePoint(c))) { n++; if (first < 0) first = c; last = c } return n + ',' + first + ',' + last })`);
}
for (const p of ["Script=Latn", "gc=Lu", "Alphabetic", "Emoji", "Script_Extensions=Deva", "Lowercase", "Uppercase"]) {
  add(`R = E(function () { var r = new RegExp(${j("\\p{" + p + "}")}, 'iu'); var n = 0; for (var c = 0; c < 0x20000; c++) if (r.test(String.fromCodePoint(c))) n++; return n })`);
  add(`R = E(function () { var r = new RegExp(${j("\\p{" + p + "}")}, 'iv'); var n = 0; for (var c = 0; c < 0x20000; c++) if (r.test(String.fromCodePoint(c))) n++; return n })`);
  add(`R = E(function () { var r = new RegExp(${j("\\P{" + p + "}")}, 'iu'); var n = 0; for (var c = 0; c < 0x20000; c++) if (r.test(String.fromCodePoint(c))) n++; return n })`);
  add(`R = E(function () { var r = new RegExp(${j("\\P{" + p + "}")}, 'iv'); var n = 0; for (var c = 0; c < 0x20000; c++) if (r.test(String.fromCodePoint(c))) n++; return n })`);
}

// ---- Case folding: u vs v vs i.
const chars = ["a", "A", "k", "K", "\u212a", "s", "S", "\u017f", "\u00df", "\u1e9e", "\u0130", "\u0131", "i", "I", "\u03c3", "\u03c2", "\u03a3", "\u03b8", "\u03d1",
  "\u03f4", "\u01c4", "\u01c5", "\u01c6", "\u1c80", "\u0432", "\u0412", "\u00b5", "\u03bc", "\u039c", "\u2126", "\u03c9", "\u03a9", "\u00e5", "\u212b",
  "\u00c5", "\ua64a", "\ua64b", "\u1c88", "\u{10400}", "\u{10428}", "\u{1E900}", "\u{1E922}", "\u0345", "\u03b9", "\u1fbe", "\u0399", "\ufb05", "\ufb06"];
for (const c of chars) {
  for (const f of ["i", "iu", "iv", "u", "v"]) {
    add(`R = E(function () { var r = new RegExp(${j(c)}, ${j(f)}); var o = []; var cs = ${j(chars)}; for (var i = 0; i < cs.length; i++) o.push(r.test(cs[i]) ? 1 : 0); return o.join('') })`);
  }
  add(`R = E(function () { var r = new RegExp('[' + ${j(c)} + ']', 'iu'); var q = new RegExp('[^' + ${j(c)} + ']', 'iu'); var q2 = new RegExp('[^' + ${j(c)} + ']', 'iv'); var cs = ${j(chars)}; return cs.map(function (x) { return (r.test(x) ? 1 : 0) + '' + (q.test(x) ? 1 : 0) + (q2.test(x) ? 1 : 0) }).join(',') })`);
}
for (const [p, f, s] of [["\\W", "iu", "\u017f"], ["\\W", "iv", "\u017f"], ["\\W", "i", "\u017f"], ["[\\W]", "iu", "S"], ["[^\\w]", "iu", "\u212a"], ["[^\\w]", "iv", "\u212a"],
  ["\\w", "iu", "\u212a"], ["\\w", "i", "\u212a"], ["\\b", "iu", "\u017f"], ["\\B", "iu", "\u017f"], ["[\\W&&\\w]", "iv", "s"], ["[^\\W]", "iv", "\u017f"],
  ["[\\w--[a-z]]", "iv", "a"], ["[\\p{Lu}--[A-Z]]", "iv", "a"], ["\\P{Lu}", "iv", "a"], ["\\P{Lu}", "iu", "a"], ["[^\\P{Lu}]", "iv", "a"], ["[^\\P{Lu}]", "iu", "a"],
  ["[\\p{Ll}]", "iu", "A"], ["\\p{Ll}", "iv", "A"], ["[a-z]", "iu", "\u212a"], ["[a-z]", "i", "\u212a"], ["[a-z]", "iv", "\u017f"], ["\\u{e5}", "iu", "\u212b"],
  ["\\\u00df", "iu", "\u1e9e"], ["ss", "iu", "\u00df"], ["\\u03c3", "iu", "\u03c2"], ["\u01c5", "i", "\u01c4"], ["[\\u01c5]", "i", "\u01c6"]])
  ex(p, f, s);

// ---- Quantificadores com limites enormes.
const big = ["a{0,4294967295}", "a{4294967295}", "a{4294967296}", "a{2147483647,}", "a{0,2147483648}", "a{1,4294967295}b", "a{99999999999999999999}",
  "a{99999999999999999999,}", "a{1,99999999999999999999}", "a{2,1}", "a{2,1}?", "(?:a{0,4294967295}){0,2}", "a{0}", "a{0,0}", "a{0}b", "(?:){4294967295}", "(a{1,10}){1,10}",
  "(?:a{1000}){1000}", "a{65535}", "a{65536}", "a{4294967294,4294967295}", "(?=a){0,4294967295}", "(?=a){4294967296}", "[a-c]{3,}?", "\\d{1,3}?", "a{,5}", "a{5,}", "{1}", "a{1", "a{1,", "a{1,2"];
for (const p of big) for (const s of ["", "a", "aaa", "aaab", "{1}", "a{,5}", "a{1"]) { ex(p, "", s); ex(p, "u", s); }
add("R = E(function () { return /^a{0,4294967295}$/.test('a'.repeat(100000)) })");
add("R = E(function () { return /(?:a|b){5000}/.test('ab'.repeat(2500)) })");
add("R = E(function () { return new RegExp('a{' + '9'.repeat(400) + '}').test('a') })");

// ---- Backtracking limitado: só os que terminam rápido.
const bt = [["(a+)+b", "aaaaaaaaaaaaaaaaaaaaab"], ["(a+)+b", "aaaaaaaaaaaaaaaa"], ["(a|aa)+b", "aaaaaaaaaaaaaaaaaab"], ["(a*)*b", "aaaaaaaaaaaaaaaaaaaaab"],
  ["(x+x+)+y", "xxxxxxxxxxxxxxxxxy"], ["^(a?){25}a{25}$", "a".repeat(25)], ["^(a?){20}a{20}$", "a".repeat(20)], ["(?:a*)*$", "aaaaaaaaaaaaaaaaaaaaaaaaaaaab"],
  ["(.*)*x", "abcdefghijklmnop"], ["(\\w+\\s?)*$", "word word word word word!"], ["^(\\d+)*$", "1111111111111111111x"], ["(a|b|ab)*c", "ababababababababc"],
  ["((a)|b)*c", "ababababababababc"], ["(a{1,3}){1,5}b", "aaaaaaaaaaaaaaaaaaaaaaaaaaaab"], ["(?:(a)|b)*", "ab".repeat(50)], ["(a*?)*?b", "aaaaaaaaaaaaaab"],
  ["(?=(a+))a*b\\1", "baaabac"], ["(?:a?){10}a{10}", "aaaaaaaaaa"], ["^(?:a|aa|aaa){5,}$", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
  ["(a|a)+$", "aaaaaaaaaaaaaaaaaaaaaaaaaaa"], ["[a-z]*[a-z]*[a-z]*[a-z]*1", "abcdefghijklmnopq"]];
for (const [p, s] of bt) { ex(p, "", s); ex(p, "u", s); ex(p, "v", s); }

// ---- Métodos Symbol.* com subclasses e exec customizado.
const subs = [
  "class R extends RegExp {}; var r = new R('a', 'g'); R_ = [r instanceof R, r.constructor.name, 'banana'.replace(r, 'X'), 'banana'.split(r).join('|'), 'banana'.match(r).join(), 'banana'.search(r)]",
  "class R extends RegExp { exec(s) { this.n = (this.n || 0) + 1; return super.exec(s) } }; var r = new R('a', 'g'); 'banana'.replace(r, 'X'); R_ = r.n",
  "class R extends RegExp { exec(s) { this.n = (this.n || 0) + 1; return super.exec(s) } }; var r = new R('a'); 'banana'.split(r); R_ = r.n",
  "class R extends RegExp { exec(s) { this.n = (this.n || 0) + 1; return super.exec(s) } }; var r = new R('a', 'g'); 'banana'.match(r); R_ = r.n",
  "class R extends RegExp { exec(s) { this.n = (this.n || 0) + 1; return super.exec(s) } }; var r = new R('a'); 'banana'.search(r); R_ = r.n",
  "class R extends RegExp { exec(s) { this.n = (this.n || 0) + 1; return super.exec(s) } }; var r = new R('a', 'g'); R_ = Array.from('banana'.matchAll(r)).length + ',' + r.n",
  "class R extends RegExp { exec(s) { return null } }; R_ = ['abc'.replace(new R('a'), 'X'), new R('a').test('a'), 'abc'.split(new R('b')).join('|')]",
  "class R extends RegExp { exec(s) { return { index: 1, 0: 'zz', length: 1 } } }; R_ = [new R('a').test('x'), 'abc'.replace(new R('a'), '[$&]'), new R('a').exec('x') && 1]",
  "class R extends RegExp { exec(s) { return 5 } }; R_ = E(function () { return new R('a').test('a') })",
  "class R extends RegExp { exec(s) { return 'str' } }; R_ = E(function () { return new R('a').test('a') })",
  "class R extends RegExp { exec(s) { return undefined } }; R_ = E(function () { return new R('a').test('a') })",
  "class R extends RegExp { static get [Symbol.species]() { return RegExp } }; var r = new R('a', 'g'); R_ = 'banana'.split(r).join('|') + (r.constructor === R)",
  "class R extends RegExp { constructor(p, f) { super(p, f + 'y'); } }; R_ = 'banana'.split(new R('a', '')).join('|')",
  // Os quatro próximos não terminam no bun (o `flags` diz global mas o `exec` interno não avança `lastIndex`): um
  // contador no `exec` interrompe o laço com RangeError depois de 50 chamadas, e a mensagem entra no resultado.
  "var n = 0; class R extends RegExp { get flags() { return 'g' } exec(s) { if (++n > 50) throw new RangeError('loop'); return super.exec(s) } }; R_ = E(function () { return 'banana'.replace(new R('a'), 'X') }) + n",
  "var n = 0; class R extends RegExp { get global() { return true } exec(s) { if (++n > 50) throw new RangeError('loop'); return super.exec(s) } }; R_ = E(function () { return 'banana'.replace(new R('a'), 'X') }) + n",
  "var r = /a/g; r.exec = function (s) { return null }; R_ = 'banana'.replace(r, 'X') + 'banana'.match(r)",
  "var r = /a/; r.exec = function (s) { return { 0: 'zz', index: 2, length: 1 } }; R_ = 'banana'.replace(r, '<$&>') ",
  "var r = /a/g; var n = 0; r.exec = function (s) { return n++ < 2 ? { 0: 'a', index: n, length: 1 } : null }; R_ = 'banana'.replace(r, '<$&>') + n",
  "var r = /a/g; var n = 0; r.exec = function (s) { if (++n > 50) throw new RangeError('loop'); return { 0: 'a', get index() { throw new RangeError('idx') }, length: 1 } }; R_ = E(function () { return 'b'.replace(r, 'x') }) + n",
  "var r = /a/; var n = 0; r.exec = function (s) { if (++n > 50) throw new RangeError('loop'); return RegExp.prototype.exec.call(this, s) }; Object.defineProperty(r, 'flags', { value: 'gi' }); R_ = E(function () { return 'AaA'.replace(r, 'x') }) + n",
  "var r = /a/g; Object.defineProperty(r, 'flags', { value: 'g' }); R_ = Array.from('aXa'.matchAll(r)).length",
  "var o = { [Symbol.replace](s, r) { return 'rep:' + s + ':' + r }, [Symbol.split](s, l) { return ['sp', s, l] }, [Symbol.match](s) { return 'm' + s }, [Symbol.search](s) { return 's' + s }, [Symbol.matchAll](s) { return 'ma' + s } }; R_ = ['abc'.replace(o, 'Z'), 'abc'.split(o, 3), 'abc'.match(o), 'abc'.search(o), 'abc'.matchAll(o), 'abc'.replaceAll(o, 'Q')]",
  "R_ = E(function () { return RegExp.prototype[Symbol.replace].call(1, 'a', 'b') })",
  "R_ = E(function () { return RegExp.prototype[Symbol.split].call({}, 'a') })",
  "R_ = E(function () { return RegExp.prototype[Symbol.match].call({ exec() { return null }, get flags() { return '' } }, 'a') })",
  "R_ = E(function () { return RegExp.prototype[Symbol.search].call({ lastIndex: 3, exec() { this.lastIndex = 9; return { index: 4 } } }, 'a') })",
  "R_ = E(function () { return RegExp.prototype[Symbol.matchAll].call(/a/g, 'aa').toString() })",
  "R_ = E(function () { return Object.prototype.toString.call(/a/g[Symbol.matchAll]('aa')) })",
  "R_ = E(function () { var it = /a/g[Symbol.matchAll]('aa'); return [typeof it.next, it[Symbol.toStringTag], Object.getPrototypeOf(it) === Object.getPrototypeOf(/b/g[Symbol.matchAll]('')), it.next().value[0]] })",
  "R_ = E(function () { return 'abc'.matchAll(/b/).next })",
  "R_ = E(function () { return 'abc'.replaceAll(/b/, 'x') })",
  "R_ = E(function () { return 'abcb'.replaceAll(/b/g, 'x') })",
  "R_ = E(function () { return 'abcb'.replaceAll('b', '$&$&') })",
  "R_ = E(function () { return 'abcb'.replaceAll('', '-') })",
  "R_ = E(function () { return 'abcb'.replaceAll('b', function (m, o, s) { return [m, o, s].join('/') }) })",
  "R_ = E(function () { return 'abcb'.replaceAll({ [Symbol.match]: true, flags: 'g', [Symbol.replace]() { return 'custom' } }, 'x') })",
  "R_ = E(function () { return 'abcb'.replaceAll({ [Symbol.match]: true, flags: 'i', [Symbol.replace]() { return 'custom' } }, 'x') })",
  "R_ = E(function () { return 'abcb'.replaceAll({ [Symbol.match]: true, flags: undefined, [Symbol.replace]() { return 'custom' } }, 'x') })",
  "R_ = E(function () { return 'abcb'.replaceAll({ [Symbol.match]: true, flags: null, [Symbol.replace]() { return 'custom' } }, 'x') })",
  "R_ = E(function () { return 'abcb'.matchAll({ [Symbol.match]: true, flags: 'i', [Symbol.matchAll]() { return 'c' } }) })",
  "R_ = E(function () { return 'abcb'.matchAll({ [Symbol.match]: true, flags: 'g', [Symbol.matchAll]() { return 'c' } }) })",
  "R_ = E(function () { return String.prototype.matchAll.call(null, /a/g) })",
  "R_ = E(function () { return String.prototype.replaceAll.call(undefined, 'a', 'b') })",
  "R_ = E(function () { return 'aXbX'.matchAll('X').next().value.index })",
  "R_ = E(function () { return Array.from('a.b.c'.matchAll('.')).length })",
  "R_ = E(function () { return Array.from('abc'.matchAll()).length })",
  "R_ = E(function () { return Array.from('abc'.matchAll(undefined)).length })",
  "R_ = E(function () { return 'abc'.replaceAll() })",
  "R_ = E(function () { return 'aaa'.replaceAll('a', undefined) })",
  "R_ = E(function () { return 'aaa'.replace(/(?<x>a)/g, '$<x>$<y>$<x') })",
  "R_ = E(function () { return 'aaa'.replace(/(a)/g, '$1$2$01$10$00$$$`$\\'') })",
  "R_ = E(function () { return 'abc'.replace(/(?<x>b)/, function () { return JSON.stringify(Array.from(arguments)) }) })",
  "R_ = E(function () { return 'abc'.replace(/b/, function () { return arguments.length }) })",
  "R_ = E(function () { return 'a-b'.split(/(-)/, 2).join('|') + ',' + 'abc'.split(/(?:)/u).join('|') + ',' + '\\ud83d\\ude00'.split(/(?:)/u).length + ',' + '\\ud83d\\ude00'.split(/(?:)/).length })",
  "R_ = E(function () { return ['ab'.split(/a*?/).join('|'), 'ab'.split(/a*/).join('|'), ''.split(/a/).length, ''.split(/(?:)/).length, 'abc'.split(/b/, 0).length, 'abc'.split(/b/, -1).length, 'abc'.split(/b/, 4294967297).length].join(' ') })",
  "R_ = E(function () { return 'test'.search(/s/g) + ',' + 'test'.search(/x/y) + ',' + 'test'.search(/t/y) })",
  "R_ = E(function () { var r = /t/g; r.lastIndex = 2; var a = 'test'.search(r); return a + ',' + r.lastIndex })",
  "R_ = E(function () { var r = /t/g; r.lastIndex = 2; var a = 'test'.match(r); return a + ',' + r.lastIndex })",
  "R_ = E(function () { var r = /t/y; r.lastIndex = 3; var a = 'test'.match(r); return a + ',' + r.lastIndex })",
  "R_ = E(function () { var r = /(?:)/gu; return 'a\\ud83d\\ude00'.match(r).length + ',' + 'a\\ud83d\\ude00'.replace(r, '-') })",
  "R_ = E(function () { var r = /(?:)/g; return 'a\\ud83d\\ude00'.match(r).length + ',' + 'a\\ud83d\\ude00'.replace(r, '-').length })",
  "R_ = E(function () { return [...'a\\ud83d\\ude00'.matchAll(/(?:)/gu)].map(function (m) { return m.index }).join() })",
  "R_ = E(function () { return [...'a\\ud83d\\ude00'.matchAll(/(?:)/g)].map(function (m) { return m.index }).join() })",
  "R_ = E(function () { var r = /a/g; r.lastIndex = 1; return [...'aaa'.matchAll(r)].map(function (m) { return m.index }).join() + ',' + r.lastIndex })",
  "R_ = E(function () { return RegExp.prototype.exec.call({}, 'a') })",
  "R_ = E(function () { return RegExp.prototype.test.call({ exec() { return {} } }, 'a') })",
  "R_ = E(function () { return RegExp.prototype.test.call(1, 'a') })",
  "R_ = E(function () { return RegExp.prototype.toString.call({ source: 'a', flags: 'b' }) })",
  "R_ = E(function () { return RegExp.prototype.toString.call(1) })",
  "R_ = E(function () { return RegExp.prototype.compile.call({}) })",
  "R_ = E(function () { var r = /a/g; r.compile('b', 'i'); return [r.source, r.flags, r.lastIndex].join() })",
  "R_ = E(function () { var r = /a/g; r.lastIndex = 3; r.compile(/b/i); return [r.source, r.flags, r.lastIndex].join() })",
  "R_ = E(function () { var r = /a/g; return r.compile(/b/i, 'g') })",
  "R_ = E(function () { class R extends RegExp {}; return new R('a').compile('b') })",
];
for (const body of subs) add("var R_;\n" + body + ";\nR = typeof R_ === 'string' ? R_ : (Array.isArray(R_) ? R_.map(String).join(' ; ') : String(R_))");

// ---- Construtor, Symbol.species, RegExp(pattern) com regex e flags.
const ctor = [
  "RegExp(/a/g).flags", "RegExp(/a/g, 'i').flags", "new RegExp(/a/g).flags", "new RegExp(/a/g, undefined).flags", "RegExp(/a/g) === RegExp(/a/g)",
  "(function () { var r = /a/; return RegExp(r) === r })()", "(function () { var r = /a/; return new RegExp(r) === r })()",
  "(function () { var r = /a/; r.constructor = null; return RegExp(r) === r })()", "(function () { var r = /a/; r.constructor = RegExp; return RegExp(r) === r })()",
  "(function () { var o = { [Symbol.match]: true, source: 'x', flags: 'g', constructor: RegExp }; return RegExp(o) === o })()",
  "(function () { var o = { [Symbol.match]: true, source: 'x', flags: 'g' }; return RegExp(o).source + RegExp(o).flags })()",
  "(function () { var o = { [Symbol.match]: false, toString() { return 'y' } }; return RegExp(o).source })()",
  "RegExp(undefined).source", "RegExp(null).source", "RegExp('').source", "RegExp().source", "new RegExp('a', '').flags", "RegExp(1).source", "RegExp({}).source",
  "RegExp('a', 'gimsuyd').flags", "RegExp('a', 'dgimsvy').flags", "RegExp('a', 'yvsmigd').flags", "RegExp('a', 'uv')", "RegExp('a', 'gg')", "RegExp('a', 'x')", "RegExp('a', ' ')",
  "RegExp('a', null)", "RegExp('a', 1)", "RegExp('a', {})", "RegExp('a', 'i\\0')", "RegExp('a', 'I')", "RegExp('a', 'ü')", "RegExp('a', 't')",
  "RegExp.length", "RegExp.name", "RegExp.prototype.constructor === RegExp", "Object.getOwnPropertyNames(RegExp).sort().join()", "Object.getOwnPropertyNames(RegExp.prototype).sort().join()",
  "Object.getOwnPropertyNames(/a/).join()", "typeof RegExp[Symbol.species]", "RegExp[Symbol.species] === RegExp", "Object.getOwnPropertyDescriptor(RegExp, Symbol.species).set",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.name", "Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get.call(RegExp.prototype)",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'source').get.call(RegExp.prototype)", "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.call({})",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.call({ global: 1, hasIndices: 1, unicodeSets: 1, sticky: 1, dotAll: 1, ignoreCase: 1, multiline: 1, unicode: 1 })",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get.call({})", "Object.getOwnPropertyDescriptor(RegExp.prototype, 'unicodeSets').get.call(/a/v)",
  "Object.getOwnPropertyDescriptor(RegExp.prototype, 'hasIndices').get.call(/a/d)", "RegExp.prototype.source", "RegExp.prototype.flags", "RegExp.prototype.global",
  "RegExp.prototype.toString()", "String(RegExp.prototype)", "/a/v.unicodeSets", "/a/v.unicode", "/a/u.unicodeSets", "/a/d.hasIndices",
  "Object.keys(RegExp.prototype).length", "RegExp.prototype[Symbol.replace].name", "RegExp.prototype[Symbol.matchAll].name", "RegExp.prototype[Symbol.split].length",
  "RegExp.prototype[Symbol.replace].length", "RegExp.prototype.exec.length", "RegExp.prototype.compile.length", "typeof RegExp.$1", "typeof RegExp.lastMatch",
  "(function () { /(a)(b)/.exec('ab'); return [RegExp.$1, RegExp.$2, RegExp.lastMatch, RegExp['$&'], RegExp.input, RegExp.$_, RegExp.leftContext, RegExp.rightContext, RegExp.lastParen, RegExp['$+']].join() })()",
  "(function () { /(?<n>a)(b)/.exec('xaby'); return [RegExp.$1, RegExp.$2, RegExp.$3, RegExp['`'], RegExp[\"'\"]].join() })()",
];
for (const e of ctor) add(`R = E(function () { var v = ${e}; return typeof v === 'object' && v !== null && !Array.isArray(v) ? Object.prototype.toString.call(v) + String(v) : String(v) })`);

// ---- RegExp.escape (se existir).
add("R = typeof RegExp.escape");
for (const s of ["a", "a.b", "^$\\.*+?()[]{}|/", "-", "a-b", "1abc", "abc1", " ", "\t\n\r\v\f", "\u00a0\u2028\u2029\ufeff", "\u{1F600}", "\ud800", "\udc00", "_a", "a_", "ß", ",=<>#&!%:;@~'`\"", "\0", "\u007f",
  "ab cd", "\u3000", "0", "A", "z", "Z", "9", "é", "\\", "/", "$", "{}", "-0", "a\u0300"]) add(`R = E(function () { return RegExp.escape(${j(s)}) })`);
for (const a of ["1", "null", "undefined", "{}", "[]", "Symbol()", "new String('a.b')", "true"]) add(`R = E(function () { return RegExp.escape(${a}) })`);
add("R = E(function () { return [RegExp.escape.length, RegExp.escape.name, Object.getOwnPropertyDescriptor(RegExp, 'escape') && Object.getOwnPropertyDescriptor(RegExp, 'escape').enumerable].join() })");
add("R = E(function () { var s = '^$.*+?()[]{}|\\\\/ -,\\n'; return new RegExp(RegExp.escape(s)).test(s) })");

// ---- source / flags / toString com barras e quebras de linha.
const srcs = ["a/b", "a\\/b", "\n", "\r", "\u2028", "\u2029", "\\n", "[/]", "[\\/]", "/", "//", "\\\\", "\\\\/", "", "(?:)", "a\nb", "[\n]", "\\\n", "\u2028\u2029\n\r",
  "\\u2028", "a\\", "[", "(", "a/", "/a", "[/\\]]", "\\/[/]/", "x\\\ny", "\ud83d\ude00", "\ud800", "\u0000", "\\0", "a\tb", "\\cJ", "\\x0a", "\\/\\/"];
for (const s of srcs) {
  add(`R = E(function () { var r = new RegExp(${j(s)}); return JSON.stringify([r.source, String(r), r.flags]) })`);
  add(`R = E(function () { var r = new RegExp(${j(s)}, 'gimsuyd'); return JSON.stringify([r.source, String(r), r.flags]) })`);
  add(`R = E(function () { var r = new RegExp(${j(s)}, 'v'); return JSON.stringify([r.source, String(r), r.flags]) })`);
  add(`R = E(function () { var r = new RegExp(${j(s)}); var q = new RegExp(r.source, r.flags); return JSON.stringify([q.source === r.source, String(q) === String(r), eval(String(r)).source === r.source]) })`);
}
for (const e of ["/a/gimsuyd.flags", "/a/dgimsvy.flags", "/\\//.source", "/[/]/.source", "/\\\\/.source", "/a/.toString()", "RegExp('/').toString()", "RegExp('\\n').toString()",
  "RegExp('\\\\n').toString()", "RegExp('a', 'm').toString()", "String(new RegExp('', ''))", "String(new RegExp('(?:)', 'g'))", "new RegExp('[/]').source", "new RegExp('\\\\/').source",
  "new RegExp('/[/]/').source", "new RegExp('\\\\\\n').source"]) add(`R = E(function () { return String(${e}) })`);

// ---- SyntaxError: 200+ padrões inválidos, mensagem exata.
const bad = [
  ["(", ""], [")", ""], ["(?", ""], ["(?:", ""], ["(?<", ""], ["(?<a", ""], ["(?<a>", ""], ["(?<>a)", ""], ["(?<1a>a)", ""], ["(?<a-b>a)", ""], ["(?<a b>a)", ""],
  ["(?=", ""], ["(?!", ""], ["(?<=", ""], ["(?<!", ""], ["[", ""], ["[a", ""], ["[a-", ""], ["[z-a]", ""], ["[z-a]", "u"], ["[\\d-a]", "u"], ["[a-\\d]", "u"], ["[\\d-\\w]", "u"],
  ["*", ""], ["+", ""], ["?", ""], ["a**", ""], ["a++", ""], ["a???", ""], ["a{2,1}", ""], ["a{2,1}", "u"], ["{", "u"], ["}", "u"], ["a{", "u"], ["a{1", "u"], ["a{1,", "u"],
  ["]", "u"], ["]", "v"], ["\\", ""], ["\\", "u"], ["\\c", "u"], ["\\c1", "u"], ["\\x", "u"], ["\\x1", "u"], ["\\u", "u"], ["\\u12", "u"], ["\\u{", "u"], ["\\u{}", "u"],
  ["\\u{110000}", "u"], ["\\u{FFFFFFFFF}", "u"], ["\\u{1", "u"], ["\\1", "u"], ["\\2(a)", "u"], ["\\k", "u"], ["\\k<a>", "u"], ["(?<a>.)\\k<b>", ""], ["(?<a>.)\\k", ""],
  ["\\k<a", "u"], ["\\-", "u"], ["\\a", "u"], ["\\e", "u"], ["\\_", "u"], ["\\p", "u"], ["\\p{", "u"], ["\\p{}", "u"], ["\\p{L", "u"], ["\\p{Foo}", "u"], ["\\p{Script=Foo}", "u"],
  ["\\p{Script}", "u"], ["\\p{Script=}", "u"], ["\\p{gc=Foo}", "u"], ["\\p{General_Category}", "u"], ["\\p{lu}", "u"], ["\\p{ASCII=Yes}", "u"], ["\\p{Lu=Lu}", "u"],
  ["\\p{ L}", "u"], ["\\p{L }", "u"], ["\\p{Script = Latin}", "u"], ["\\p{RGI_Emoji}", "u"], ["\\P{RGI_Emoji}", "v"], ["[^\\p{RGI_Emoji}]", "v"], ["[\\P{RGI_Emoji}]", "v"],
  ["\\p{Basic_Emoji}", ""], ["[\\p{RGI_Emoji}]", "u"], ["\\p{Script_Extensions=}", "u"], ["\\p{scx=Foo}", "u"], ["\\p{Block=Basic_Latin}", "u"], ["\\p{InBasicLatin}", "u"],
  ["\\p{IsLatin}", "u"], ["\\p{Latin}", "u"], ["\\p{Any=1}", "u"], ["\\p{Alphabetic=Yes}", "u"], ["\\p{Emoji_Keycap_Sequence}", "u"],
  ["(?<a>.)(?<a>.)", ""], ["(?<a>.)(?<a>.)", "u"], ["(?<a>.)|(?<a>.)(?<a>.)", ""], ["(?<a>.)(?:|(?<a>.))", ""], ["(?<\\u{110000}>a)", ""], ["(?<\\ud800>a)", ""], ["(?<\\u0>a)", ""],
  ["(?i:", ""], ["(?i-i:a)", ""], ["(?ii:a)", ""], ["(?-:a)", ""], ["(?x:a)", ""], ["(?i", ""], ["(?-", ""], ["(?i-", ""], ["(?u:a)", ""], ["(?g:a)", ""], ["(?I:a)", ""], ["(?-i-s:a)", ""],
  ["(?<a>", ""], ["(?P<a>a)", ""], ["(?#c)", ""], ["(?'a'a)", ""], ["(?|a)", ""], ["(?>a)", ""], ["(?R)", ""], ["(?1)", ""], ["(*)", ""], ["(?=a)*", "u"], ["(?<=a)*", ""], ["(?!a)+", "u"],
  ["(?=a){1}", "u"], ["\\b+", "u"], ["^*", ""], ["$+", ""], ["^*", "u"], ["(?<!a)?", ""], ["a|*", ""], ["|*", ""], ["(*a)", ""], ["(+a)", ""], ["(?:*)", ""], ["a{1}{2}", ""], ["a{1}*", ""],
  ["[[a]", "v"], ["[a&&]", "v"], ["[&&a]", "v"], ["[a&&&b]", "v"], ["[a--]", "v"], ["[--a]", "v"], ["[a&&b--c]", "v"], ["[a--b&&c]", "v"], ["[ab&&c]", "v"], ["[a&&bc]", "v"], ["[a-z&&b]", "v"],
  ["[(]", "v"], ["[)]", "v"], ["[{]", "v"], ["[}]", "v"], ["[/]", "v"], ["[-]", "v"], ["[|]", "v"], ["[a|b]", "v"], ["[!!]", "v"], ["[##]", "v"], ["[$$]", "v"], ["[%%]", "v"],
  ["[**]", "v"], ["[++]", "v"], ["[,,]", "v"], ["[..]", "v"], ["[::]", "v"], ["[;;]", "v"], ["[<<]", "v"], ["[==]", "v"], ["[>>]", "v"], ["[??]", "v"], ["[@@]", "v"], ["[^^]", "v"],
  ["[``]", "v"], ["[~~]", "v"], ["[\\q]", "v"], ["[\\q{]", "v"], ["[\\q{a]", "v"], ["\\q{a}", "v"], ["[\\q{a}", "v"], ["[^\\q{ab}]", "v"], ["[^\\q{a|bc}]", "v"], ["[\\q{a|}]", "v"],
  ["[\\q{[a]}]", "v"], ["[\\q{a-b}]", "v"], ["[a-\\q{b}]", "v"], ["[\\q{a}-b]", "v"], ["[^[\\q{ab}]]", "v"], ["[^[\\p{RGI_Emoji}]]", "v"], ["[^[a]&&\\p{RGI_Emoji}]", "v"],
  ["\\p{RGI_Emoji", "v"], ["\\p{rgi_emoji}", "v"], ["\\p{RGI_Emoji=1}", "v"], ["[\\p{RGI_Emoji}--\\P{RGI_Emoji}]", "v"],
  ["a", "gg"], ["a", "uv"], ["a", "vu"], ["a", "z"], ["a", "G"], ["a", "dd"], ["a", "yy"], ["a", "ii"], ["a", "mm"], ["a", "ss"], ["a", "vv"], ["a", "g "], ["a", "ig\u0000"],
  ["(?<a>a)\\k<a", "u"], ["(?<=a)\\1", "u"], ["\\00", "u"], ["\\01", "u"], ["\\8", "u"], ["\\9", "u"], ["[\\1]", "u"], ["[\\b-a]", "u"], ["[\\cA-\\cB]", "u"], ["\\c\u0100", "u"],
  ["\\ud83d\\", "u"], ["(?:a", ""], ["(a", ""], ["a)", ""], ["(?<a>a", ""], ["((a)", ""], ["(a))", ""], ["(?=a", "u"], ["a{1,2", "u"], ["a{,1}", "u"], ["a{1}?{2}", ""], ["x{4294967296,1}", ""],
  ["[\\u{110000}]", "u"], ["[\\p{L}-z]", "u"], ["[a-\\p{L}]", "u"], ["\\p{L}-", "u"], ["[\\P{Lu}-z]", "u"], ["(?<a>.)\\k<a>(?<b>", ""], ["(?<\\u{61>a)", ""], ["(?<a\\>a)", ""],
];
const seenBad = new Set();
for (const [p, f] of bad) {
  const key = p + "\0" + f;
  if (seenBad.has(key)) continue;
  seenBad.add(key);
  add(`R = E(function () { return String(new RegExp(${j(p)}, ${j(f)}).source) })`);
}
// Os mesmos como literal em eval (mensagem de parse do literal) para um subconjunto.
for (const [p, f] of bad.slice(0, 60)) {
  if (/[\n\r/\\]/.test(p) && p.includes("/")) continue;
  add(`R = E(function () { return String(eval(${j("/" + p + "/" + f)})) })`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "regexp-golden-"));
const file = path.join(dir, "regexp_v_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(preload, RESULT_PRELOAD);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
const rows = [];
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = body.replace(/\bR = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC: grava-se o texto canônico e o bun executa `executableSource(original)`.
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 5000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRELUDE.length)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRELUDE.length)) + "\n");
    continue;
  }
  kept++;
  rows.push(JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("regexp_v", rows));
fs.rmSync(dir, { recursive: true, force: true });
