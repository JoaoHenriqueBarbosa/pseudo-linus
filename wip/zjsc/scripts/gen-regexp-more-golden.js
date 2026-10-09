// Gera tests/golden/regexp_more_bun.tsv: 2000 programas de uma linha que ampliam a cobertura de regexp
// além de regexp_bun.tsv e regexp_opt_bun.tsv: flag v (classes aninhadas, interseção, subtração, \q{...},
// propriedades de string), flag d (indices e groups), grupos nomeados duplicados, modificadores
// `(?i:...)`, lookbehind variável, backreferences nomeadas, \p{Script=}, \p{Script_Extensions=} e
// \p{General_Category=} de 100 valores com 5 caracteres cada, case folding unicode, sticky/lastIndex,
// matchAll/replace com função, split com captura, Symbol.replace/Symbol.split customizados,
// toString/source/flags, RegExp.escape e a mensagem exata de cada erro de sintaxe do Yarr.
// Colunas e serialização iguais às de gen-regexp-golden.js (harness tests/golden/e2e_values_harness.js).
// Uso: timeout 300 bun scripts/gen-regexp-more-golden.js > tests/golden/regexp_more_bun.tsv
const fs = require("fs");
const { emitRow, sampleByHash, stepSampler } = require("./golden-prelude.js");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

/** Literal de string JS em ASCII puro (JSON.stringify com o que passa de 0x7e escapado). */
function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
const R = (pattern, flags = "") => `new RegExp(${q(pattern)}, ${q(flags)})`;

// Os candidatos entram em `pool`; os que só entram em parte levam a densidade (`thin(passo, programa)`, 1 em `passo`) e a
// escolha é por hash do texto (`sampleByHash` dentro do `stepSampler`), nunca pela posição no laço nem por contador.
const pool = stepSampler();
const add = (source) => pool.push(1, source);
const thin = (step, source) => pool.push(step, source);
const exec = (pattern, flags, input) => add(`${R(pattern, flags)}.exec(${q(input)})`);
const test = (pattern, flags, input) => add(`${R(pattern, flags)}.test(${q(input)})`);
const compile = (pattern, flags) => add(`${R(pattern, flags)}`);
const replace = (pattern, flags, input, replacement) => add(`${q(input)}.replace(${R(pattern, flags)}, ${q(replacement)})`);
const matchAllIdx = (pattern, flags, input) =>
  add(`[...${q(input)}.matchAll(${R(pattern, flags)})].map(function (m) { return m[0] + "@" + m.index })`);

const cp = (n) => String.fromCodePoint(n);

// ---------------------------------------------------------------------------------------------
// Flag v: classes aninhadas, interseção, subtração, \q{...}.
const vInputs = ["a", "\u{1f600}", "A1b2"];
const vPatterns = [
  "[a-z]", "[[a-c][x-z]]", "[a-z&&[^aeiou]]", "[[a-z]--[aeiou]]", "[\\w--\\d]", "[\\w&&\\d]", "[\\w--[a-c]]",
  "[[a-z]&&[b-d]]", "[[a-z]--[b-d]--[x]]", "[\\p{L}--\\p{Lu}]", "[\\p{L}&&\\p{ASCII}]", "[^[a-z]--[aeiou]]",
  "[^\\w&&\\d]", "[[^a-c]&&[a-f]]", "[\\q{abc}]", "[\\q{abc|ab|a}]", "[\\q{abc|ab|a}b]", "[\\q{}]", "[\\q{a|}]",
  "[\\q{abc}--\\q{abc}]", "[\\q{abc|x}--\\q{x}]", "[\\q{ab|cd}&&\\q{cd|ef}]", "[[\\q{ab}]a]", "[a-z\\q{xyz}]+",
  "^[\\q{abc|ab|a}]$", "[\\q{a|bc}]+", "[\\p{ASCII}--[a-z]]+", "[\\d[a-c]]+", "[\\&]", "[\\-]", "[a\\-z]", "[\\(\\)\\[\\]\\{\\}\\/\\-\\\\\\|]",
  "[[a-z]&&[^m]]", "[[[a-c]]]", "[a-c&&[b-d]]", "[\\u{1f600}-\\u{1f64f}]", "\\p{ASCII_Hex_Digit}", "[\\p{Lu}\\p{Nd}]",
  "[[\\p{L}--[a-z]]&&\\p{ASCII}]", "\\P{L}", "[\\P{L}]", "[^\\P{L}]", "(?i)[a-z]", "[\\p{Lowercase}&&\\p{ASCII}]",
  "[\\p{Lu}--\\p{ASCII}]", "[^\\q{a}]", "[\\q{\\u{1f600}x}]", "[\\q{\\x41}]", "[\\q{\\n}]",
];
for (const p of vPatterns) for (const input of vInputs) exec(p, "v", input);
for (const p of ["[a-z]", "[[a-z]--[aeiou]]", "[\\q{abc|ab|a}]", "[\\w&&\\d]"]) {
  for (const flags of ["vi", "vg", "vs"]) exec(p, flags, "ABCabc");
}
for (const [p, input] of [
  ["\\p{RGI_Emoji}", "x\u{1f600}"], ["\\p{RGI_Emoji}", "\u{1f468}\u200d\u{1f469}\u200d\u{1f467}"],
  ["\\p{RGI_Emoji}", "\u{1f1e7}\u{1f1f7}"], ["\\p{RGI_Emoji}", "\u{1f44d}\u{1f3fd}"], ["\\p{RGI_Emoji}", "a"],
  ["\\p{Basic_Emoji}", "\u{1f600}"], ["\\p{Basic_Emoji}", "\u2764\ufe0f"], ["\\p{Emoji_Keycap_Sequence}", "1\ufe0f\u20e3"],
  ["\\p{Emoji_Keycap_Sequence}", "#\ufe0f\u20e3"], ["\\p{RGI_Emoji_Flag_Sequence}", "\u{1f1e7}\u{1f1f7}"],
  ["\\p{RGI_Emoji_Flag_Sequence}", "\u{1f1e7}"], ["\\p{RGI_Emoji_Modifier_Sequence}", "\u{1f44d}\u{1f3fb}"],
  ["\\p{RGI_Emoji_Tag_Sequence}", "\u{1f3f4}\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}"],
  ["\\p{RGI_Emoji_ZWJ_Sequence}", "\u{1f468}\u200d\u{1f469}\u200d\u{1f467}\u200d\u{1f466}"],
  ["\\p{RGI_Emoji_ZWJ_Sequence}", "\u{1f468}"], ["^\\p{RGI_Emoji}$", "\u{1f468}\u200d\u{1f469}\u200d\u{1f467}"],
  ["^\\p{RGI_Emoji}+$", "\u{1f600}\u{1f1e7}\u{1f1f7}\u{1f44d}\u{1f3fd}"], ["[\\p{RGI_Emoji}--\\q{\u{1f600}}]", "\u{1f600}"],
  ["[\\p{RGI_Emoji}--\\q{\u{1f600}}]", "\u{1f601}"], ["[\\p{RGI_Emoji}&&\\p{Emoji_Keycap_Sequence}]", "1\ufe0f\u20e3"],
  ["[\\p{Emoji_Keycap_Sequence}\\q{abc}]", "abc"], ["\\P{RGI_Emoji}", "a"], ["[^\\p{RGI_Emoji}]", "a"],
  ["[^\\q{ab}]", "a"], ["\\p{Emoji}", "5"], ["\\p{Emoji_Presentation}", "\u{1f600}"], ["\\p{Extended_Pictographic}", "\u2764"],
  ["\\p{Emoji_Modifier}", "\u{1f3fb}"], ["\\p{Emoji_Modifier_Base}", "\u{1f44d}"], ["\\p{Emoji_Component}", "#"],
  ["\\p{RGI_Emoji}", "\u{1f600}"], ["\\p{RGI_Emoji}", "\u2764\ufe0f"], ["\\p{RGI_Emoji}", "\u{1f9d1}\u200d\u{1f4bb}"],
]) {
  exec(p, "v", input);
}
// Erros específicos da flag v.
for (const p of [
  "[a-z&&]", "[&&a]", "[a&&&b]", "[a&&b--c]", "[a--b&&c]", "[a--]", "[--a]", "[a-z&&b]", "[a&&b]", "[(]", "[)]",
  "[{]", "[}]", "[/]", "[-]", "[|]", "[a-z--]", "[\\q{abc]", "[\\q]", "[\\q{a}", "[[a]", "[a]]", "[^\\q{ab}]", "[^\\p{RGI_Emoji}]",
  "\\P{RGI_Emoji}", "[\\P{RGI_Emoji}]", "[^\\P{Emoji_Keycap_Sequence}]", "[!!]", "[##]", "[$$]", "[%%]", "[**]", "[++]",
  "[,,]", "[..]", "[::]", "[;;]", "[<<]", "[==]", "[>>]", "[??]", "[@@]", "[^^]", "[``]", "[~~]", "[a-\\d]", "[\\d-a]", "[\\w-z]",
]) {
  compile(p, "v");
}
for (const p of ["a", "[a]", "\\p{L}", "[^a]"]) {
  for (const flags of ["uv", "vu", "vv"]) compile(p, flags);
}

// ---------------------------------------------------------------------------------------------
// Flag d: indices e groups.
for (const [p, flags, input] of [
  ["a(b)(c)?", "d", "xab"], ["(?<x>a)(?<y>b)?", "d", "xa"], ["(?<x>a)(?<y>b)", "d", "xab"], ["(a)|(b)", "d", "b"],
  ["(?<a>x)|(?<a>y)", "d", "y"], ["(?<a>x)|(?<a>y)", "d", "x"], ["", "d", "abc"], ["b*", "dg", "abbc"],
  ["(?<=a)(b)", "d", "ab"], ["(?<!a)(b)", "d", "bb"], ["\\u{1f600}(.)", "du", "\u{1f600}\u{1f601}"],
  ["(.)(?<emoji>\u{1f600})", "du", "a\u{1f600}"], ["(\\d+)-(\\d+)", "dy", "12-34"], ["(\\d+)-(\\d+)", "dyg", "12-3412-34"],
  ["((a)(b))", "d", "ab"], ["(?:(a)|b)+", "d", "ab"], ["(a)\\1", "d", "aa"], ["(?<n>a)\\k<n>", "d", "aa"],
  ["(?<n>.)(?<m>.)", "ds", "\n\n"], ["(?<ab>a)(?<cd>b)", "dgi", "ABab"],
]) {
  add(`(function () { var m = ${R(p, flags)}.exec(${q(input)}); return m && { i: m.indices, g: m.indices && m.indices.groups, k: Object.keys(m.indices || {}) } })()`);
}
add(`${R("a", "d")}.hasIndices`);
add(`${R("a", "")}.hasIndices`);
add(`Object.getOwnPropertyDescriptor(RegExp.prototype, "hasIndices").get.call(/a/d)`);
add(`${R("(?<a>x)(?<b>y)", "d")}.exec("xy").indices.groups.b`);
add(`Object.getPrototypeOf(${R("(?<a>x)", "d")}.exec("x").indices.groups)`);
add(`Object.getPrototypeOf(${R("(?<a>x)", "")}.exec("x").groups)`);
add(`"xy".replace(${R("(?<a>x)(?<b>y)", "d")}, "$<b>$<a>")`);
add(`"xy".replace(${R("(?<a>x)(?<b>y)", "")}, "$<c>[$<a>]")`);
add(`"xy".replace(${R("(x)(y)", "")}, "$<a>")`);
add(`"xy".replace(${R("(?<a>x)(y)", "")}, "$<a")`);
add(`"xy".replace(${R("(?<a>x)(y)", "")}, "$<>")`);

// ---------------------------------------------------------------------------------------------
// Grupos nomeados (inclusive duplicados em alternativas) e backreferences nomeadas.
for (const [p, flags, input] of [
  ["(?<a>x)|(?<a>y)", "", "x"], ["(?<a>x)|(?<a>y)", "", "y"], ["(?<a>x)|(?<a>y)", "", "z"],
  ["(?:(?<a>x)|(?<a>y))\\k<a>", "", "xx"], ["(?:(?<a>x)|(?<a>y))\\k<a>", "", "yy"], ["(?:(?<a>x)|(?<a>y))\\k<a>", "", "xy"],
  ["(?<a>x)\\k<a>|(?<a>y)\\k<a>", "", "yy"], ["(?:(?<a>x)|(?<a>y)){2}", "", "xy"], ["(?:(?<a>x)|(?<a>y))+", "", "yx"],
  ["(?<a>a)|(?<b>b)|(?<a>c)", "", "c"], ["(?<a>a)(?:(?<b>b)|(?<b>c))", "", "ac"], ["(?:(?<a>a)|b)(?:(?<a>c)|d)", "", "bc"],
  ["\\k<a>(?<a>x)", "", "x"], ["\\k<a>(?<a>x)", "u", "x"], ["(?<a>x)\\k<a>", "", "xx"], ["(?<a>x)\\k<b>", "", "xk<b>"],
  ["\\k<a>", "", "k<a>"], ["\\k<a>", "u", "k<a>"], ["\\k", "", "k"], ["\\k<", "", "k<"], ["(?<a>x)\\k", "", "xk"],
  ["(?<\u03c0>x)", "", "x"], ["(?<\\u03c0>x)", "", "x"], ["(?<\\u{1d7d8}>x)", "", "x"], ["(?<$>x)", "", "x"], ["(?<_a1>x)", "", "x"],
  ["(?<a\\u{1f600}>x)", "", "x"], ["(?<\u{1d7d8}>x)", "", "x"], ["(?<a>.)(?<b>.)\\k<b>\\k<a>", "", "abba"],
  ["(?<a>.)\\k<a>+", "", "aaa"], ["(?<a>.)\\k<a>{2}", "", "aaa"], ["(?<a>a)|\\k<a>b", "", "b"], ["(?:(?<a>a)|\\k<a>b)+", "", "ab"],
  ["(?<A>a)(?<a>b)", "", "ab"], ["(?<a>a)\\k<a>", "i", "aA"], ["(?<n>\\d)\\k<n>", "", "112"],
  ["(?<year>\\d{4})-(?<month>\\d{2})-(?<day>\\d{2})", "", "on 2024-03-09."],
  ["(?<ab>a)(?<ba>b)", "", "xab"], ["(?<proto>a)", "", "a"], ["(?<__proto__>a)", "", "a"], ["(?<constructor>a)", "", "a"],
  ["(?<length>a)", "", "a"], ["(?<index>a)", "", "a"], ["(?<input>a)", "", "a"], ["(?<groups>a)", "", "a"],
]) {
  exec(p, flags, input);
  add(`(function () { var m = ${R(p, flags)}.exec(${q(input)}); return m && m.groups && Object.keys(m.groups).map(function (k) { return k + "=" + m.groups[k] }) })()`);
}
for (const p of [
  "(?<a>x)(?<a>y)", "(?<a>x)|(?<a>y)(?<a>z)", "(?<a>x)(?:|(?<a>y))", "(?<a>x)(?:(?<a>y))", "(?:(?<a>x))(?:(?<a>y))",
  "(?<a>x)(?=(?<a>y))", "(?<a>x)(?:(?<a>y)|z)", "(?:(?<a>x)|(?<a>y))(?<a>z)", "(?<a", "(?<a>", "(?<>x)", "(?<1a>x)", "(?<a-b>x)",
  "(?<a b>x)", "(?<\\u{110000}>x)", "(?<\\ud83d>x)", "(?<a\\u>x)", "(?<a\\u{>x)", "(?<a\\u{1>x)", "(?<a\\x41>x)",
  "\\k<a", "\\k<>", "\\k<1>", "(?<a>x)\\k<a", "(?<a>x)\\k<b>", "(?<a>x)\\k", "(?<a>x)\\k<", "(?<a>x)\\k<>",
]) {
  for (const flags of ["", "u"]) compile(p, flags);
}

// ---------------------------------------------------------------------------------------------
// Modificadores (?ims-ims:...).
for (const [p, input] of [
  ["(?i:a)b", "Ab"], ["(?i:a)b", "AB"], ["(?i:a)b", "ab"], ["a(?i:b)c", "aBc"], ["a(?i:b)c", "aBC"], ["(?-i:a)b", "Ab"],
  ["(?-i:a)b", "ab"], ["(?s:.)", "\n"], ["(?-s:.)", "\n"], ["(?m:^a$)", "x\na\ny"], ["(?-m:^a$)", "x\na\ny"],
  ["(?im:^a$)", "x\nA\ny"], ["(?i-s:a.)", "A\n"], ["(?s-i:a.)", "a\n"], ["(?i:[a-c])", "B"], ["(?i:\\w)", "\u017f"],
  ["(?i:a|b)c", "Bc"], ["(?i:(a)b)", "AB"], ["(?i:a(?-i:b))", "Ab"], ["(?i:a(?-i:b))", "AB"], ["(?i:\\u00e9)", "\u00c9"],
  ["(?i:\\1(a))", "aA"], ["(a)(?i:\\1)", "aA"], ["(?i:ss)", "\u00df"], ["(?i:k)", "\u212a"], ["(?i:s)", "\u017f"],
  ["(?i:\\p{Lu})", "a"], ["(?s:a.b)(?-s:c.d)", "a\nbc\nd"], ["(?ims:a)", "A"], ["(?i:)", "x"], ["(?-:a)", "a"],
  ["(?i-i:a)", "A"], ["(?i:(?-i:a)|b)", "A"], ["(?i:(?-i:a)|b)", "B"], ["(?i:a)+", "aAaA"], ["(?i:a){2}", "aA"],
  ["(?i:(?<n>a))\\k<n>", "Aa"], ["(?i:\\k<n>)(?<n>a)", "Aa"], ["(?<=(?i:a))b", "Ab"], ["(?=(?i:a))A", "A"],
]) {
  for (const flags of ["", "i"]) exec(p, flags, input);
}
for (const p of [
  "(?x:a)", "(?ii:a)", "(?i-i:a)", "(?-ii:a)", "(?i", "(?i:", "(?i-", "(?-", "(?--i:a)", "(?i-:a)", "(?i-m", "(?I:a)", "(?g:a)",
  "(?u:a)", "(?v:a)", "(?d:a)", "(?y:a)", "(?i:a", "(?ims-ims:a)", "(?imsi:a)", "(?ims-:a)", "(?:?i:a)", "(?i)a", "(?i)",
]) {
  for (const flags of ["", "u"]) compile(p, flags);
}

// ---------------------------------------------------------------------------------------------
// Lookbehind (inclusive de tamanho variável) e lookahead.
for (const [p, flags, input] of [
  ["(?<=a+)b", "", "aaab"], ["(?<=a*)b", "", "b"], ["(?<=a{2,3})b", "", "aab"], ["(?<=a{2,3})b", "", "ab"], ["(?<=\\d+)x", "", "12x"],
  ["(?<=(\\d+))x", "", "12x"], ["(?<=(\\d+)(\\d+))x", "", "1234x"], ["(?<=\\1(a))b", "", "aab"], ["(?<=(a)\\1)b", "", "aab"],
  ["(?<=ab|c)d", "", "abd"], ["(?<=ab|c)d", "", "cd"], ["(?<=(?<a>x)|(?<a>y))z", "", "yz"], ["(?<=a(?=b))b", "", "ab"],
  ["(?<!a+)b", "", "aab"], ["(?<!a+)b", "", "cb"], ["(?<=\\b)a", "", " a"], ["(?<=^|,)\\w+", "g", "a,b,c"], ["(?<=\\$)\\d+(\\.\\d*)?", "", "cost $10.50"],
  ["(?<![a-z])\\d", "", "a1 2"], ["(?<=[a-c]{2})d", "i", "ABd"], ["(?<=.)", "g", "abc"], ["(?<=^)", "", "abc"], ["(?<=$)", "", "abc"],
  ["(?<=\\u{1f600})a", "u", "\u{1f600}a"], ["(?<=.)a", "u", "\u{1f600}a"], ["(?<=^.)a", "u", "\u{1f600}a"], ["(?<=^.)a", "", "\u{1f600}a"],
  ["(?<=(?<=a)b)c", "", "abc"], ["(?<=(?<!a)b)c", "", "bc"], ["(?<=a?)b", "", "ab"], ["(?<=(a|ab)c)d", "", "abcd"],
  ["(?<=\\k<n>(?<n>a))b", "", "aab"], ["(?<=(?<n>a)\\k<n>)b", "", "aab"], ["(?<=a|bc*)d", "", "bcccd"], ["(?<=(?:a|b)+)c", "", "abac"],
  ["(?=(a))\\1b", "", "ab"], ["(?!(a))b", "", "b"], ["(?=a)*", "", "a"], ["(?=a){2}", "", "a"], ["(?:(?=a))?a", "", "a"], ["(?!a)*", "", "b"],
  ["(?<=a)*", "", "a"], ["(?<=a){2}", "", "aa"], ["(?<=a)?b", "", "b"], ["(?<!a)*b", "", "b"],
]) {
  exec(p, flags, input);
}
for (const p of ["(?<=a", "(?<", "(?<!", "(?<=)", "(?<=a)*", "(?<=a)+", "(?<=a){1}", "(?=a)+", "(?=a)*", "(?!a)?", "(?!a){2}"]) {
  for (const flags of ["", "u"]) compile(p, flags);
}

// ---------------------------------------------------------------------------------------------
// Propriedades Unicode: 100 valores (Script, Script_Extensions, General_Category) com 5 caracteres cada.
// A mesma varredura de escolha dos 5 caracteres (3 membros, 2 não membros) roda aqui, no bun.
const scripts = [
  "Latin", "Greek", "Cyrillic", "Armenian", "Hebrew", "Arabic", "Syriac", "Thaana", "Devanagari", "Bengali", "Gurmukhi", "Gujarati", "Oriya",
  "Tamil", "Telugu", "Kannada", "Malayalam", "Sinhala", "Thai", "Lao", "Tibetan", "Myanmar", "Georgian", "Hangul", "Ethiopic", "Cherokee",
  "Canadian_Aboriginal", "Ogham", "Runic", "Khmer", "Mongolian", "Hiragana", "Katakana", "Bopomofo", "Han", "Yi", "Old_Italic", "Gothic",
  "Deseret", "Braille", "Coptic", "Glagolitic", "Tifinagh", "Phoenician", "Cuneiform", "Egyptian_Hieroglyphs", "Linear_B", "Ugaritic",
  "Brahmi", "Adlam", "Common", "Inherited", "Unknown", "Zyyy", "Grek", "Cyrl", "Hani", "Kana",
];
const categories = [
  "Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Mc", "Me", "Nd", "Nl", "No", "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "Sm", "Sc", "Sk", "So",
  "Zs", "Zl", "Zp", "Cc", "Cf", "Cs", "Co", "Cn", "Letter", "Cased_Letter", "Mark", "Number", "Punctuation", "Symbol", "Separator",
  "Other", "Uppercase_Letter", "Decimal_Number", "L&", "LC", "Combining_Mark", "digit", "punct", "Control", "Format",
];
function samples(testRe) {
  const members = [], nonMembers = [];
  for (let c = 0; c < 0x30000 && (members.length < 3 || nonMembers.length < 2); c += 1) {
    if (c >= 0xd800 && c <= 0xdfff && !testRe.unicode) continue;
    const hit = testRe.test(String.fromCodePoint(c));
    if (hit && members.length < 3 && (members.length === 0 || c > members[members.length - 1] + 40)) members.push(c);
    else if (!hit && nonMembers.length < 2 && c > 0x40 + nonMembers.length * 0x2000) nonMembers.push(c);
  }
  while (members.length < 3) members.push(0x61 + members.length);
  while (nonMembers.length < 2) nonMembers.push(0x30000 + nonMembers.length);
  return [...members, ...nonMembers];
}
const propertyForms = [];
for (const s of scripts) {
  propertyForms.push(`Script=${s}`, `scx=${s}`);
}
for (const g of categories) propertyForms.push(`gc=${g}`, g);
for (const form of propertyForms) {
  let re;
  try { re = new RegExp(`\\p{${form}}`, "u"); } catch (e) { re = null; }
  if (!re) {
    compile(`\\p{${form}}`, "u");
    continue;
  }
  const chars = samples(re);
  add(`[${chars.join(",")}].map(function (c) { return ${R(`\\p{${form}}`, "u")}.test(String.fromCodePoint(c)) })`);
  thin(3, `[${chars.join(",")}].map(function (c) { return ${R(`[\\p{${form}}]`, "v")}.test(String.fromCodePoint(c)) })`);
}
for (const bad of [
  "\\p{Script=}", "\\p{Script=Foo}", "\\p{Script}", "\\p{Latin}", "\\p{sc=latin}", "\\p{Script_Extensions}", "\\p{General_Category}",
  "\\p{General_Category=Foo}", "\\p{gc=lu}", "\\p{Lu=Lu}", "\\p{=Lu}", "\\p{}", "\\p{", "\\p", "\\pL", "\\p{ Lu}", "\\p{Lu }", "\\p{Script=Latin=x}",
  "\\p{ASCII=Yes}", "\\p{Any}", "\\p{Assigned}", "\\p{Alphabetic}", "\\p{alphabetic}", "\\p{Block=Basic_Latin}", "\\p{InBasicLatin}",
  "\\p{Line_Break=AL}", "\\p{Age=1.1}", "\\p{Script=Hira}", "\\p{Script=Hiragana}", "\\p{scx=Hira}", "\\p{Script=Zzzz}", "\\p{Script=Qaai}",
  "\\p{Script=Kawi}", "\\p{Script=Nag_Mundari}", "\\p{Script=Garay}", "\\p{Script=Vithkuqi}", "\\p{Script=Toto}", "\\p{Script=Cypro_Minoan}",
  "\\p{ID_Start}", "\\p{ID_Continue}", "\\p{XID_Start}", "\\p{Emoji}", "\\p{Math}", "\\p{White_Space}", "\\p{Uppercase}", "\\p{Lowercase}", "\\p{Cased}",
  "\\p{Case_Ignorable}", "\\p{Changes_When_Lowercased}", "\\p{Default_Ignorable_Code_Point}", "\\p{Hex_Digit}", "\\p{Regional_Indicator}",
  "\\p{Variation_Selector}", "\\p{Join_Control}", "\\p{Pattern_Syntax}", "\\p{Noncharacter_Code_Point}", "\\p{Quotation_Mark}",
]) {
  compile(bad, "u");
}
for (const [p, input] of [
  ["\\p{Lu}", "a"], ["\\p{Lu}", "A"], ["\\p{Lu}+", "ABCd"], ["\\p{Script=Greek}+", "abc\u03b1\u03b2\u03b3"], ["\\p{Script_Extensions=Han}+", "\u3001\u4e2d\u6587"],
  ["\\p{Script=Han}+", "\u3001\u4e2d\u6587"], ["\\p{Script=Common}", "\u3001"], ["\\p{Script_Extensions=Hiragana}", "\u30fc"], ["\\p{Script=Hiragana}", "\u30fc"],
  ["\\P{Script=Latin}+", "abc123"], ["[^\\p{L}]+", "ab12cd"], ["[\\p{L}\\p{N}]+", "ab12-"], ["\\p{Nd}+", "\u0663\u0664x"], ["\\p{Any}", "\u{10ffff}"],
  ["\\p{Assigned}", "\u0378"], ["\\P{Assigned}", "\u0378"], ["\\p{ASCII}+", "ab\u00e9"], ["\\p{Alphabetic}+", "ab\u00e91"], ["\\p{Lowercase}", "\u00aa"],
  ["\\p{Cased}", "\u01c5"], ["\\p{Lt}", "\u01c5"], ["\\p{L}", "\u{20000}"], ["\\p{Cn}", "\u0378"], ["\\p{Cs}", "\ud800"], ["\\p{Cs}", "\ud800"],
  ["\\p{Co}", "\ue000"], ["\\p{Zs}", "\u3000"], ["\\p{Sc}", "$"], ["\\p{Sm}", "+"], ["\\p{Pd}", "-"], ["\\p{Sk}", "^"], ["\\p{Pc}", "_"],
  ["\\p{ID_Start}", "\u2118"], ["\\p{ID_Continue}", "\u00b7"], ["\\p{XID_Continue}", "\u00b7"], ["\\p{Hex_Digit}", "\uff21"], ["\\p{ASCII_Hex_Digit}", "\uff21"],
  ["\\p{White_Space}", "\u0085"], ["\\p{Changes_When_Casefolded}", "A"], ["\\p{Changes_When_NFKC_Casefolded}", "\u00a0"],
  ["\\p{Default_Ignorable_Code_Point}", "\u00ad"], ["\\p{Dash}", "\u2014"], ["\\p{Diacritic}", "^"], ["\\p{Extender}", "\u00b7"],
  ["\\p{Grapheme_Base}", "a"], ["\\p{Grapheme_Extend}", "\u0301"], ["\\p{IDS_Binary_Operator}", "\u2ff0"], ["\\p{Ideographic}", "\u4e00"],
  ["\\p{Join_Control}", "\u200d"], ["\\p{Logical_Order_Exception}", "\u0e40"], ["\\p{Noncharacter_Code_Point}", "\ufffe"], ["\\p{Pattern_White_Space}", "\u200e"],
  ["\\p{Quotation_Mark}", "\u00ab"], ["\\p{Radical}", "\u2e80"], ["\\p{Regional_Indicator}", "\u{1f1e7}"], ["\\p{Sentence_Terminal}", "."], ["\\p{Soft_Dotted}", "i"],
  ["\\p{Terminal_Punctuation}", ","], ["\\p{Unified_Ideograph}", "\u4e00"], ["\\p{Variation_Selector}", "\ufe0f"], ["\\p{Bidi_Control}", "\u200e"], ["\\p{Bidi_Mirrored}", "("],
  ["\\p{Deprecated}", "\u0149"], ["\\p{Math}", "+"], ["\\p{Lowercase}", "a"], ["\\p{Uppercase}", "a"], ["\\p{Case_Ignorable}", "'"],
]) {
  exec(p, "u", input);
}

// ---------------------------------------------------------------------------------------------
// Case folding unicode com i, u e v.
const foldChars = ["s", "S", "\u017f", "k", "K", "\u212a", "\u00df", "\u1e9e", "\u0130", "i", "I", "\u0131", "\u03c3", "\u03c2", "\u03a3", "\u00b5", "\u03bc",
  "\u039c", "\u01c4", "\u01c5", "\u01c6", "\u1e61", "\u1e60", "\u1e9b", "\u2126", "\u03c9", "\u03a9", "\u00e5", "\u212b", "\u00c5", "\ufb00", "\ufb05", "\ufb06",
  "\u{10400}", "\u{10428}", "\u0345", "\u03b9", "\u1fbe", "\u0399", "\u04c0", "\u04cf", "\u2c2f", "\u2c5f", "\ua64a", "\u1c88", "\u0131", "\u1e9e", "\u03f4", "\u03b8", "\u03d1"];
// Os pares de caracteres entram com densidade 1/180 (por hash do programa) para caber no total de 2000.
foldChars.forEach((a, i) => {
  foldChars.forEach((b, j) => {
    if (a === b) return;
    for (const flags of ["i", "iu", "iv"]) thin(180, `${R(a, flags)}.test(${q(b)})`);
  });
});
for (const [p, input] of [
  ["[a-z]", "\u017f"], ["[a-z]", "\u212a"], ["\\w", "\u017f"], ["\\w", "\u212a"], ["\\W", "\u017f"], ["\\W", "\u212a"], ["[\\w]", "\u017f"], ["[^\\W]", "\u017f"],
  ["\\b", "\u017f"], ["\\B", "\u017f"], ["[^\\W]", "\u212a"], ["[\\W]", "\u212a"], ["\\p{Lu}", "a"], ["\\p{Ll}", "A"], ["\\P{Lu}", "A"], ["\\P{Ll}", "a"],
  ["[^\\p{Lu}]", "A"], ["[\\p{Lu}--[A-Z]]", "a"], ["[\\p{Lu}&&[A-Z]]", "a"], ["[^\\p{Lu}&&[A-Z]]", "a"], ["[[A-Z]--\\q{A}]", "a"], ["\\q{a}", "A"],
  ["[\\q{ss}]", "\u00df"], ["[\\q{\u00df}]", "SS"], ["ss", "\u00df"], ["\u00df", "ss"], ["\u1e9e", "ss"], ["[\u00df]", "\u1e9e"], ["\u0130", "i"], ["\u0130", "i\u0307"],
  ["i", "\u0130"], ["\u0131", "I"], ["[\u0130]", "\u0131"], ["\u03c3\u03c2", "\u03a3\u03a3"], ["\u03a3", "\u03c2"], ["\u212a+", "kK"], ["\u017f+", "sS"],
  ["(?:k)\\1", "kK"], ["(k)\\1", "kK"], ["(\u212a)\\1", "kK"], ["(?<a>\u017f)\\k<a>", "sS"], ["\\u{10400}", "\u{10428}"], ["[\\u{10400}-\\u{10410}]", "\u{10428}"],
  ["[\ud801\udc00]", "\ud801\udc28"], ["\ud801\udc00", "\ud801\udc28"], ["\\ud801", "\ud801"], ["[\\ud801\\udc00]", "\ud801\udc28"], ["\\u0345", "\u03b9"],
  ["[\\u0345]", "\u1fbe"], ["\\u1fbe", "\u0399"], ["\\u2126", "\u03c9"], ["\\u212b", "\u00e5"], ["[a-z]+", "KkSs\u212a\u017f"],
]) {
  for (const flags of ["i", "iu", "iv", "u", "v"]) exec(p, flags, input);
}

// ---------------------------------------------------------------------------------------------
// Sticky, lastIndex, global.
for (const [p, flags, input, start] of [
  ["a", "y", "ba", 1], ["a", "y", "ba", 0], ["a", "y", "ba", 2], ["a", "y", "ba", 5], ["a", "y", "ba", -1], ["a", "g", "ba", 1], ["a", "g", "ba", 2],
  ["a*", "y", "baa", 1], ["a*", "gy", "baa", 1], ["^a", "y", "ba", 1], ["^a", "ym", "b\na", 2], ["^a", "y", "ba", 0], ["\\ba", "y", "ba", 1],
  ["(?<=b)a", "y", "ba", 1], ["a", "yu", "\u{1f600}a", 1], ["a", "yu", "\u{1f600}a", 2], ["a", "gu", "\u{1f600}a", 1], [".", "yu", "\u{1f600}", 1],
  [".", "gu", "\u{1f600}", 1], [".", "y", "\u{1f600}", 1], ["", "y", "abc", 3], ["", "y", "abc", 4], ["", "g", "abc", 3], ["", "gu", "\u{1f600}", 1],
  ["\\udf06", "gu", "\ud834\udf06", 1], ["\\udf06", "g", "\ud834\udf06", 1], ["[\\udf06]", "gu", "\ud834\udf06", 1], ["a|ab", "y", "xab", 1],
  ["(a)|b", "gy", "ab", 0], ["(a)|b", "gy", "ab", 1], ["a", "gy", "aa", 1], ["a", "dgy", "aa", 1], ["a", "gv", "\u{1f600}a", 1],
]) {
  add(`(function () { var re = ${R(p, flags)}; re.lastIndex = ${start}; var m = re.exec(${q(input)}); return [m && m.index, m && m[0], re.lastIndex] })()`);
}
for (const [p, flags, input] of [
  ["a", "g", "aaa"], ["a", "y", "aaa"], ["a", "gy", "aab"], ["", "g", "ab"], ["a*", "g", "baab"], ["\\b", "g", "ab cd"], ["^", "gm", "a\nb"], ["$", "gm", "a\nb"],
  [".", "gu", "a\u{1f600}b"], [".", "g", "a\u{1f600}b"], ["", "gu", "\u{1f600}"], ["", "g", "\u{1f600}"], ["(?:)", "gv", "\u{1f600}x"],
]) {
  add(`(function () { var re = ${R(p, flags)}, out = [], m, guard = 0; while ((m = re.exec(${q(input)})) !== null && guard++ < 12) out.push([m.index, m[0], re.lastIndex]); return [out, re.lastIndex] })()`);
  add(`(function () { var re = ${R(p, flags)}, out = [], guard = 0; while (re.test(${q(input)}) && guard++ < 12) out.push(re.lastIndex); return [out, re.lastIndex] })()`);
  add(`(function () { var re = ${R(p, flags)}; return [${q(input)}.search(re), re.lastIndex] })()`);
  add(`(function () { var re = ${R(p, flags)}; re.lastIndex = 1; return [${q(input)}.match(re), re.lastIndex] })()`);
  add(`(function () { var re = ${R(p, flags)}; re.lastIndex = 1; return [${q(input)}.replace(re, "[$&]"), re.lastIndex] })()`);
}
add(`(function () { var re = /a/g; re.lastIndex = "1"; re.exec("aa"); return re.lastIndex })()`);
add(`(function () { var re = /a/; re.lastIndex = 9; re.exec("a"); return re.lastIndex })()`);
add(`(function () { var re = /a/g; Object.defineProperty(re, "lastIndex", { writable: false }); try { re.exec("a"); return "ok" } catch (e) { return e.name } })()`);
add(`(function () { var re = /a/; Object.defineProperty(re, "lastIndex", { writable: false }); return re.exec("a") && "ok" })()`);
add(`(function () { var re = /b/; Object.defineProperty(re, "lastIndex", { writable: false }); return re.exec("a") })()`);
add(`(function () { var re = /b/g; Object.defineProperty(re, "lastIndex", { writable: false, value: 0 }); try { return re.exec("a") } catch (e) { return e.name } })()`);
add(`(function () { var re = /a/g; re.lastIndex = { valueOf: function () { return 1 } }; return [re.exec("aa").index, re.lastIndex] })()`);
add(`(function () { var re = /a/g; re.lastIndex = -5; return [re.exec("aa").index, re.lastIndex] })()`);
add(`(function () { var re = /a/g; re.lastIndex = 2 ** 53; return [re.exec("aa"), re.lastIndex] })()`);

// ---------------------------------------------------------------------------------------------
// matchAll, replace com função e grupos nomeados, split com captura.
for (const [p, flags, input] of [
  ["(?<d>\\d)", "g", "a1b2c3"], ["(\\d)(\\w)?", "g", "1a2"], ["", "g", "ab"], ["a|b", "gi", "AaBb"], ["(?<x>.)", "gu", "a\u{1f600}"], ["(?<x>.)", "g", "a\u{1f600}"],
  ["\\p{L}+", "gu", "ab cd\u00e9 1"], ["[\\p{L}--[a-c]]+", "gv", "abcdef"], ["\\b\\w", "g", "ab cd ef"], ["(?:(?<k>a)|(?<k>b))", "g", "ab"],
]) {
  add(`[...${q(input)}.matchAll(${R(p, flags)})].map(function (m) { return [m[0], m.index, m.groups && JSON.stringify(m.groups), m.length] })`);
  add(`${q(input)}.replace(${R(p, flags)}, function () { var a = [].slice.call(arguments); return "<" + a.length + ":" + a.slice(0, -1).map(String).join("|") + ">" })`);
  add(`${q(input)}.replace(${R(p, flags)}, function () { var a = arguments; var g = a[a.length - 1]; return typeof g === "object" ? "<" + JSON.stringify(g) + ">" : "<" + (typeof g) + ">" })`);
  add(`${q(input)}.split(${R(p, flags)})`);
}
for (const [p, flags, input, limit] of [
  ["(-)", "", "a-b-c", undefined], ["(-)|(\\+)", "", "a-b+c", undefined], ["(?<s>-)", "", "a-b-c", undefined], ["(-)", "", "a-b-c", 3], ["(-)", "", "a-b-c", 0],
  ["(-)", "", "-a-", undefined], ["(a)?b", "", "xbyabz", undefined], ["", "", "abc", undefined], ["(?:)", "u", "\u{1f600}a", undefined], ["", "", "\u{1f600}", undefined],
  ["(?:)", "v", "\u{1f600}a", undefined], ["x*", "", "axxb", undefined], ["x*", "y", "axxb", undefined], ["a", "y", "bab", undefined], ["(a)|(b)", "", "xaybz", undefined],
  ["\\s*,\\s*", "", "a , b,c ,d", undefined], ["(\\d)", "", "a1b2c", -1], ["(\\d)", "", "a1b2c", 4294967297], ["(\\d)", "", "a1b2c", 1.9], ["(\\d)", "", "a1b2c", "2"],
]) {
  add(`${q(input)}.split(${R(p, flags)}${limit === undefined ? "" : ", " + JSON.stringify(limit)})`);
}
for (const [replacement, input] of [
  ["$1", "ab"], ["$01", "ab"], ["$10", "ab"], ["$001", "ab"], ["$2", "ab"], ["$3", "ab"], ["$&$&", "ab"], ["$`", "ab"], ["$'", "ab"], ["$$", "ab"], ["$", "ab"],
  ["$<n>", "ab"], ["$<m>", "ab"], ["$<n", "ab"], ["$0", "ab"], ["$00", "ab"], ["$1$2$3", "ab"], ["$11", "ab"], ["\\$1", "ab"], ["$$1", "ab"], ["$<n>$1", "ab"],
]) {
  add(`${q(input)}.replace(${R("(?<n>a)(b)", "")}, ${q(replacement)})`);
  add(`${q(input)}.replace(${R("(a)(b)", "")}, ${q(replacement)})`);
  add(`${q(input)}.replace(${R("(?<n>a)", "g")}, ${q(replacement)})`);
}

// ---------------------------------------------------------------------------------------------
// Symbol.replace, Symbol.split, Symbol.match, Symbol.matchAll, Symbol.search customizados.
const symbolCases = [
  `"abc".replace({ [Symbol.replace]: function (s, r) { return [s, r] } }, "x")`,
  `"abc".replaceAll({ [Symbol.replace]: function (s, r) { return [s, r] }, flags: "g", [Symbol.match]: true }, "x")`,
  `"abc".split({ [Symbol.split]: function (s, l) { return [s, l] } }, 3)`,
  `"abc".match({ [Symbol.match]: function (s) { return "m:" + s } })`,
  `"abc".matchAll({ [Symbol.matchAll]: function (s) { return "ma:" + s } })`,
  `"abc".search({ [Symbol.search]: function (s) { return "s:" + s } })`,
  `(function () { var re = /b/; re[Symbol.replace] = function (s, r) { return "custom:" + s + r }; return "abc".replace(re, "X") })()`,
  `(function () { var re = /b/; re[Symbol.split] = function (s, l) { return "custom:" + s + l }; return "abc".split(re, 2) })()`,
  `(function () { var re = /b/; re[Symbol.replace] = undefined; return "abc".replace(re, "X") })()`,
  `(function () { var re = /b/; re[Symbol.split] = null; return "abcbd".split(re) })()`,
  `(function () { var re = /b/; re[Symbol.match] = false; return "a/b/".startsWith(re) })()`,
  `(function () { var re = /b/; re[Symbol.match] = false; return "/b/".startsWith(re) })()`,
  `(function () { try { return "a".startsWith(/a/) } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return "a".includes(/a/) } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return "a".endsWith(/a/) } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return "a".replaceAll(/a/, "b") } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return "a".matchAll(/a/) } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { class R extends RegExp { exec(s) { return null } } return "abc".replace(new R("b"), "X") })()`,
  `(function () { var calls = []; class R extends RegExp { exec(s) { calls.push(this.lastIndex); return super.exec(s) } } "abab".replace(new R("b", "g"), "X"); return calls })()`,
  `(function () { class R extends RegExp { exec(s) { return { index: 1, 0: "b", length: 1 } } } return "abc".replace(new R("b"), "[$&]") })()`,
  `(function () { class R extends RegExp { exec(s) { return { index: 1, 0: "b", length: 1, groups: { n: "q" } } } } return "abc".replace(new R("b"), "[$<n>]") })()`,
  `(function () { class R extends RegExp { exec(s) { return 1 } } try { return new R("b").test("b") } catch (e) { return e.name } })()`,
  `(function () { class R extends RegExp { static get [Symbol.species]() { return RegExp } } return "a-b".split(new R("-")) })()`,
  `(function () { var calls = []; class R extends RegExp { constructor(p, f) { calls.push(f); super(p, f) } } "a-b-c".split(new R("-", "g")); return calls })()`,
  `(function () { var calls = []; class R extends RegExp { constructor(p, f) { calls.push(f); super(p, f) } } "a-b-c".split(new R("-", "iu")); return calls })()`,
  `(function () { var calls = []; class R extends RegExp { constructor(p, f) { calls.push(f); super(p, f) } } [..."a1b2".matchAll(new R("\\\\d", "gi"))]; return calls })()`,
  `(function () { var re = /a/g; var it = "aa".matchAll(re); re.lastIndex = 1; return [...it].length })()`,
  `(function () { var re = /a/g; re.lastIndex = 1; var it = "aaa".matchAll(re); return [[...it].length, re.lastIndex] })()`,
  `(function () { var log = []; var re = /a/g; var p = new Proxy(re, { get: function (t, k) { log.push(String(k)); var v = t[k]; return typeof v === "function" ? v.bind(t) : v } }); RegExp.prototype[Symbol.replace].call(p, "aa", "b"); return log })()`,
  `(function () { var log = []; var re = /a/y; var p = new Proxy(re, { get: function (t, k) { log.push(String(k)); var v = t[k]; return typeof v === "function" ? v.bind(t) : v } }); RegExp.prototype[Symbol.split].call(p, "aa"); return log })()`,
  `(function () { var log = []; var re = /(?<x>a)/gu; var p = new Proxy(re, { get: function (t, k) { log.push(String(k)); var v = t[k]; return typeof v === "function" ? v.bind(t) : v } }); RegExp.prototype[Symbol.matchAll].call(p, "aa"); return log })()`,
  `(function () { try { return RegExp.prototype[Symbol.replace].call(1, "a", "b") } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return RegExp.prototype[Symbol.split].call(undefined, "a") } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return RegExp.prototype.exec.call({}, "a") } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return RegExp.prototype.test.call(1, "a") } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { try { return RegExp.prototype.toString.call(1) } catch (e) { return e.name + ":" + e.message } })()`,
  `RegExp.prototype.toString.call({ source: "a", flags: "b" })`,
  `RegExp.prototype.toString.call({})`,
  `RegExp.prototype.flags`,
  `RegExp.prototype.source`,
  `RegExp.prototype.global`,
  `RegExp.prototype.toString()`,
  `Object.getOwnPropertyNames(RegExp.prototype).sort()`,
  `Object.getOwnPropertyNames(RegExp).sort()`,
  `Object.getOwnPropertySymbols(RegExp.prototype).map(String)`,
  `RegExp.prototype[Symbol.replace].name + "/" + RegExp.prototype[Symbol.replace].length`,
  `RegExp.prototype[Symbol.split].name + "/" + RegExp.prototype[Symbol.split].length`,
  `RegExp.prototype[Symbol.matchAll].name + "/" + RegExp.prototype[Symbol.matchAll].length`,
  `Object.getOwnPropertyDescriptor(RegExp.prototype, "flags").get.name`,
  `Object.getOwnPropertyDescriptor(RegExp.prototype, "unicodeSets").get.name`,
  `Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get.name`,
];
for (const s of symbolCases) add(s);

// ---------------------------------------------------------------------------------------------
// toString, source, flags e escapes.
for (const [p, flags] of [
  ["/", ""], ["\\/", ""], ["[/]", ""], ["\n", ""], ["\\n", ""], ["\r", ""], ["\u2028", ""], ["\u2029", ""], ["\\\n", ""], ["", ""], ["(?:)", ""], ["a/b", "g"],
  ["a\\/b", "g"], ["[\\/]", ""], ["\\\\/", ""], ["\\\\\\/", ""], ["[^/]", ""], ["\\u2028", ""], ["\\\u2028", ""], ["a\\\nb", ""], ["\t", ""], ["\\t", ""],
  ["\0", ""], ["\\0", ""], ["\u00e9", ""], ["\u{1f600}", "u"], ["/[/]/", ""], ["[", ""], ["]", ""], ["{", ""], ["}", ""], ["a{", ""], ["\\", ""], ["a\\", ""],
  ["a", "gimsuyd"], ["a", "dgimsuy"], ["a", "yusmigd"], ["a", "gv"], ["a", "dgimsvy"], ["a", "vgimsdy"], ["a", "iiv"], ["a", "gg"], ["a", "x"], ["a", "G"], ["a", "uv"],
  ["a", "gimsuyvd"], ["a", " g"], ["a", "g "], ["a", "\u0067"], ["a", "\u212a"], ["a", "g\u0000"], ["a", "l"], ["a", "t"], ["a", "n"],
]) {
  add(`(function () { try { var re = ${R(p, flags)}; return [re.source, re.flags, String(re), re.global, re.ignoreCase, re.multiline, re.dotAll, re.unicode, re.unicodeSets, re.sticky, re.hasIndices] } catch (e) { return e.name + ":" + e.message } })()`);
}
for (const s of [
  `new RegExp("/").source`, `new RegExp("\\n").source`, `new RegExp("\\n").source`, `new RegExp("\\\\n").source`, `new RegExp("").source`, `new RegExp("/").toString()`,
  `new RegExp("\\n").toString()`, `new RegExp("[/]").toString()`, `new RegExp("\\/").toString()`, `String(new RegExp("\\\\/"))`, `new RegExp("\\u2028").source`,
  `new RegExp("a", "gi").toString()`, `RegExp("a", "g").flags`, `RegExp(/a/g).flags`, `RegExp(/a/g, "i").flags`, `new RegExp(/a/g, "").flags`, `new RegExp(/a/g, undefined).flags`,
  `new RegExp(/a/g).source`, `new RegExp(undefined).source`, `new RegExp(null).source`, `new RegExp(undefined, undefined).toString()`, `new RegExp(1).source`,
  `new RegExp([1, 2]).source`, `new RegExp({}).source`, `new RegExp("a", undefined).flags`, `new RegExp("a", null).flags`, `RegExp.prototype.flags`,
  `RegExp(/a/) === RegExp(/a/)`, `(function () { var r = /a/; return RegExp(r) === r })()`, `(function () { var r = /a/; return new RegExp(r) === r })()`,
  `(function () { var r = /a/; r.constructor = Object; return RegExp(r) === r })()`, `(function () { var r = { constructor: RegExp, [Symbol.match]: true, source: "q", flags: "g" }; return RegExp(r) === r })()`,
  `(function () { var r = { [Symbol.match]: true, source: "q", flags: "gi" }; return String(new RegExp(r)) })()`, `(function () { var r = { [Symbol.match]: true, source: "q" }; return String(new RegExp(r)) })()`,
  `(function () { var r = /a/g; r.compile("b", "i"); return String(r) })()`, `(function () { var r = /a/g; r.compile(/b/i); return String(r) })()`,
  `(function () { var r = /a/g; r.lastIndex = 3; r.compile("b"); return r.lastIndex })()`, `(function () { var r = /a/g; try { r.compile(/b/i, "g") } catch (e) { return e.name + ":" + e.message } })()`,
  `(function () { var r = /a/g; r.compile(); return String(r) })()`, `(function () { var r = /a/g; r.compile(undefined, undefined); return String(r) })()`, `/a/.compile.length`,
  `/a/g.compile("(", "")`, `RegExp.prototype.compile.name`,
  `typeof RegExp.escape`, `RegExp.escape.length`, `RegExp.escape.name`, `Object.getOwnPropertyDescriptor(RegExp, "escape").enumerable`,
]) {
  add(`(function () { try { return ${s} } catch (e) { return e.name + ":" + e.message } })()`);
}
// RegExp.escape (se existir).
for (const s of [
  "", "a", "1", "_", "a.b", "a*b", "$^", "(a)", "[a]", "{a}", "a|b", "\\", "/", "-", ",", "=", "<>", "#", "&", "!", "%", "'", "\"", "`", ":", ";", "@", "~", " ", "\t", "\n",
  "\r", "\v", "\f", "\u00a0", "\u2028", "\u2029", "\ufeff", "\u00e9", "\u{1f600}", "\ud800", "\udc00", "1a", "a1", "0", "9z", "ab cd", "x-y", "a\nb", "\u0085", "\u1680",
  "\u2000", "\u3000", "Z", "z", "A", "\x7f", "\x00", "\x01", "\u00ff", "\u0100", "\ufffd", "\u200b", "\u200e",
]) {
  add(`(function () { try { return RegExp.escape(${q(s)}) } catch (e) { return e.name + ":" + e.message } })()`);
}
for (const s of ["1", "null", "undefined", "{}", "[]", "Symbol()", "1n", "true"]) {
  add(`(function () { try { return RegExp.escape(${s}) } catch (e) { return e.name + ":" + e.message } })()`);
}

// ---------------------------------------------------------------------------------------------
// Erros de sintaxe com a mensagem exata, um programa por padrão e modo (cada ErrorCode do Yarr).
const syntaxPatterns = [
  "a**", "a++", "a???", "+", "*", "?", "a|*", "(?", "(?<", "(?<a", "(?<a>", "(?<>", "(?=", "(?!", "(?<=", "(?<!", "(?:", "(", ")", "(a", "a)", "[", "[a", "[]", "[^",
  "\\", "a\\", "[\\", "[a-", "[z-a]", "[\\d-a]", "[a-\\d]", "a{2,1}", "a{1", "a{1,", "a{1,2", "{1}", "{", "}", "]", "a{2,1}?", "x{2,1}", "\\1", "(a)\\2", "\\k<a>",
  "(?<a>)\\k<b>", "(?<a>)(?<a>)", "(?<a-b>)", "\\u", "\\u1", "\\u12", "\\u123", "\\u{", "\\u{}", "\\u{110000}", "\\u{1", "\\ud800\\u", "\\x", "\\x1", "\\xg", "\\c", "\\c1",
  "\\p", "\\p{", "\\p{L", "\\p{}", "\\p{Foo}", "\\P{Foo}", "\\p{L}=", "\\pL", "\\q{a}", "\\a", "\\e", "\\-", "\\_", "\\@", "\\ ", "\\%", "\\!", "\\'", "\\\"", "\\<", "\\>", "\\=", "\\:",
  "\\,", "\\;", "\\~", "\\`", "\\#", "\\&", "\\k", "\\k<", "\\k<a", "\\8", "\\9", "\\08", "\\00", "\\01", "\\1(a)", "(?<n>a)\\k<n", "(?i", "(?i:", "(?-i:a)", "(?i-i:a)",
  "(?ii:a)", "(?z:a)", "^*", "$*", "\\b*", "\\B+", "(?=a)*", "(?<=a)+", "(?!a)?", "a{1}{2}", "a*{2}", "a{2}*", "a+*", "a?+", "[\\b-a]", "[a-\\b]", "[\\w-\\d]", "[\\d-\\d]",
  "[a-a]", "[\\cA-\\cZ]", "[\\c-a]", "(?<a>a)|(?<a>b)", "(?<a>a)(?<b>b)(?<a>c)", "[\\u{110000}]", "[\\p{Foo}]", "(?:a", "(?:)a)", "((a)", "(a))", "[(]", "[)]", "(?<a>a", "(?<=a",
  "(?<!a", "a|", "|a", "|", "||", "()", "(|)", "(?:|)", "[^]", "[]]", "[^]]", "{}", "x{}", "x{,}", "x{,1}", "x{1,,2}", "\\", "\\\\", "\\/", "/", "a/b",
];
for (const p of syntaxPatterns) {
  for (const flags of p.includes("[") || p.includes("\\q") ? ["", "v"] : [""]) {
    add(`(function () { try { return String(${R(p, flags)}) } catch (e) { return e.name + ":" + e.message } })()`);
  }
}
for (const flags of ["gg", "ii", "mm", "ss", "uu", "yy", "dd", "vv", "uv", "x", "G", "gix", "gimsuyd v", "\u0131", "1", "-", "u v", "gimsuydv", "gimsuyvd"]) {
  add(`(function () { try { return String(${R("a", flags)}) } catch (e) { return e.name + ":" + e.message } })()`);
}
for (const s of ["/(/", "/a/gg", "/[/", "/a/x"]) {
  add(`(function () { try { return (0, eval)(${q(s)}) } catch (e) { return e.name + ":" + e.message } })()`);
}
add(`(function () { try { return new Function("return /(/") } catch (e) { return e.name + ":" + e.message } })()`);
add(`(function () { try { return new RegExp("a".repeat(70000) + "{70000}") } catch (e) { return e.name + ":" + e.message } })()`);
add(`(function () { try { return new RegExp("(".repeat(70000) + ")".repeat(70000)).test("") } catch (e) { return e.name + ":" + e.message } })()`);
add(`(function () { try { return new RegExp("((((a{1000}){1000}){1000}){1000})").test("") } catch (e) { return e.name + ":" + e.message } })()`);
add(`(function () { try { return new RegExp("(?:a{1000000}){1000000}").test("b") } catch (e) { return e.name + ":" + e.message } })()`);

// ---------------------------------------------------------------------------------------------
// Preenchimento determinístico até 2000 programas: combinações de sintaxe v/u/i sobre entradas variadas.
const fillPatterns = [
  "(?<a>\\w)\\k<a>", "(\\w)\\1", "\\p{L}+", "[\\p{L}--[aeiou]]+", "[^\\p{L}]+", "(?:a|b)+?c", "a{2,}?", "(?<=\\w)\\d", "(?<!\\d)\\w", "\\b\\w+\\b", "^\\s*$", "[\\s\\S]*?x",
  "(a)|(b)|(c)", "(?:(a)|b)*", "\\u{1f600}+", "[\\u{1f600}-\\u{1f64f}]+", ".", ".+", "\\S+", "\\D+", "(\\d)(?=(\\d{3})+$)", "[a-z&&[^b]]+", "\\p{Lu}\\p{Ll}*", "(?i:a)b",
];
const fillInputs = ["", "a", "ab", "aab", "abc", "xx", "b1c", "\u{1f600}\u{1f600}", "A b", "1234567", "Hello World", "\u00e9a", "ab\ncd", "\u017fs", "Ab1", "zzz a1 b2"];
const fillFlags = ["", "u", "v", "i", "iu", "iv", "g", "gu", "gv", "s", "m", "d", "du", "dv", "y", "yu"];
const programs = pool.resolve();
const fillSeen = new Set(programs);
// Complemento até as 2000: todas as combinações entram como candidatas (as que ainda não existem) e as que faltam saem
// por hash do programa.
const fillCandidates = [];
for (const input of fillInputs) {
  for (const p of fillPatterns) {
    for (const flags of fillFlags) {
      const source = `${R(p, flags)}.exec(${q(input)})`;
      if (!fillSeen.has(source)) { fillSeen.add(source); fillCandidates.push(source); }
    }
  }
}
programs.push(...sampleByHash(fillCandidates, Math.max(0, 2000 - new Set(programs).size)));

// ---------------------------------------------------------------------------------------------
let emitted = 0;
for (const src of sampleByHash([...new Set(programs)], 2000)) {
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  const out = (0, eval)(`${harness}(${JSON.stringify(src)})`);
  if (typeof out !== "string") throw new Error(`${src}: o harness não devolveu string`);
  emitRow(`${src}\t${out}`);
  emitted += 1;
}
if (emitted < 2000) throw new Error(`só ${emitted} programas, faltam ${2000 - emitted}`);
