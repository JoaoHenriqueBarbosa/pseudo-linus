// Gera tests/golden/regexp_v_bun.tsv: RegExp avançado (flag v, propriedades Unicode, case folding unicode, named groups
// duplicados, modificadores, lookbehind, hasIndices, mensagens de SyntaxError, flags/source/toString, lastIndex em
// unicode), medido no bun 1.4.2. Complementa regexp_edge_bun.tsv sem repetir seus casos.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// O programa roda por `vm.runInThisContext` (ProgramExecutable do JSC puro) e `globalThis.R` é capturado.
// Uso: bun scripts/gen-regexp-v-golden.js > tests/golden/regexp_v_bun.tsv
const { emitFactored } = require("./golden-prelude.js");
const rows = [];
const vm = require("node:vm");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function M(m){if(m===null)return "null";var o={a:Array.from(m),i:m.index,g:m.groups===undefined?"u":Object.entries(m.groups)};' +
  'if(m.indices){o.d=Array.from(m.indices);o.dg=m.indices.groups===undefined?"u":Object.entries(m.indices.groups)}return JSON.stringify(o)}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const re = (p, f) => `new RegExp(${q(p)},${q(f || "")})`;
const exec = (p, f, s) => add(`T(()=>M(${re(p, f)}.exec(${q(s)})))`);
const ctor = (p, f) => add(`T(()=>String(${re(p, f)}))`);
const test = (p, f, s) => add(`T(()=>${re(p, f)}.test(${q(s)}))`);
const all = (p, f, s) => add(`T(()=>Array.from(${q(s)}.matchAll(${re(p, f)}),m=>m[0]+"@"+m.index))`);

// ---- 1. Flag v: classes aninhadas, interseção, subtração, \q{}.
const vSets = [
  ["[[a-z]--[aeiou]]", ["a", "b", "z", "e"]],
  ["[[a-z]&&[aeiou]]", ["a", "b", "e", "u"]],
  ["[\\w--\\d]", ["a", "5", "_"]],
  ["[\\w&&\\d]", ["a", "5", "_"]],
  ["[\\p{L}--[a-z]]", ["a", "A", "é", "1"]],
  ["[\\p{L}&&\\p{ASCII}]", ["a", "é", "Z", "1"]],
  ["[[a-z][0-9]]", ["a", "5", "A"]],
  ["[[a-z]--[a-c]--[x-z]]", ["a", "d", "x", "m"]],
  ["[[a-z]&&[a-m]&&[f-z]]", ["a", "f", "m", "n"]],
  ["[^[a-z]--[aeiou]]", ["a", "b", "1"]],
  ["[^\\d]", ["a", "1"]],
  ["[\\q{abc|d|}]", ["abc", "d", "", "x", "ab"]],
  ["[\\q{abc|d}a]", ["abc", "a", "d", "b"]],
  ["[\\q{ab|cd}--\\q{ab}]", ["ab", "cd"]],
  ["[\\q{ab|cd}&&\\q{cd|ef}]", ["ab", "cd", "ef"]],
  ["[\\q{a}]", ["a", "b"]],
  ["[\\q{}]", ["", "a"]],
  ["[\\q{ab|abc}]", ["abc", "ab"]],
  ["[\\q{abc|ab}]", ["abc", "ab"]],
  ["[[\\q{ab}c]--c]", ["ab", "c"]],
  ["[\\p{Lu}--\\q{A}]", ["A", "B", "a"]],
  ["[a-z--[b-y]]", ["a", "z", "m"]],
  ["[\\u{1F600}-\\u{1F64F}]", ["😀", "🙏", "a"]],
  ["[\\u{1F600}--\\u{1F601}]", ["😀", "😂"]],
  ["[[^a]&&[^b]]", ["a", "b", "c"]],
  ["[[^a]--[b]]", ["a", "b", "c"]],
  ["[\\s&&[^\\n]]", [" ", "\n", "\t"]],
  ["[\\S--[a-z]]", ["a", "A", " "]],
  ["[\\W&&[^\\s]]", ["!", " ", "a"]],
  ["[\\b]", ["\b", "b"]],
  ["[()\\[\\]{}\\/\\-\\|]", ["(", "-", "|", "a"]],
  ["[\\&\\!\\#\\%\\,\\:\\;\\<\\=\\>\\@\\`\\~]", ["&", "~", "a"]],
];
for (const [p, ss] of vSets) {
  ctor(p, "v");
  for (const s of ss) {
    exec(`^${p}$`, "v", s);
    exec(`^${p}$`, "vi", s);
  }
}
for (const p of ["[a-z]", "\\p{Lu}", "[\\q{ab}c]", "a+", "(?:ab|cd)+"]) for (const s of ["abab", "xAbz", "cdcd"]) all(p, "gv", s);
// Interseção e subtração em classe aninhada profunda.
for (const p of ["[[[a-z]--[a-f]]&&[[d-k]--[j]]]", "[[[a-c]&&[b-d]]--[c]]", "[[a-z]--[[aeiou]--[e]]]", "[[\\d\\p{L}]--[0-9]]"]) {
  for (const s of ["b", "e", "g", "j", "k", "5", "z"]) exec(`^${p}$`, "v", s);
}
// Case folding com v: classe negada e complemento.
for (const [p, s] of [["[^a]", "A"], ["[^a]", "b"], ["[\\P{Lu}]", "A"], ["[^\\P{Lu}]", "a"], ["\\P{Lu}", "A"], ["[\\p{Lu}]", "a"], ["\\p{Lu}", "a"], ["[^\\p{Lu}]", "A"]]) {
  exec(`^${p}$`, "vi", s);
  exec(`^${p}$`, "iu", s);
}
// Propriedades de strings.
const strProps = ["RGI_Emoji", "Basic_Emoji", "Emoji_Keycap_Sequence", "RGI_Emoji_Flag_Sequence", "RGI_Emoji_Modifier_Sequence", "RGI_Emoji_Tag_Sequence", "RGI_Emoji_ZWJ_Sequence"];
const emojiSamples = ["😀", "👍🏽", "1️⃣", "🇧🇷", "👨‍👩‍👧", "🏴󠁧󠁢󠁥󠁮󠁧󠁿", "a", "©", "❤️", "❤", "🇧", "#️⃣", "👩🏽‍💻", "🧑‍🤝‍🧑", "🫠"];
for (const prop of strProps) {
  ctor(`\\p{${prop}}`, "v");
  for (const s of emojiSamples) exec(`^\\p{${prop}}$`, "v", s);
  exec(`\\p{${prop}}`, "u", "x");
}
for (const s of emojiSamples) exec("^[\\p{RGI_Emoji}--\\q{😀}]$", "v", s);
for (const s of emojiSamples) exec("^[\\p{RGI_Emoji}&&\\p{Emoji_Keycap_Sequence}]$", "v", s);
for (const s of emojiSamples) exec("^\\p{Emoji}$", "v", s);
all("\\p{RGI_Emoji}", "gv", "a😀b👍🏽c1️⃣d🇧🇷e👨‍👩‍👧f");
all("\\p{RGI_Emoji}+", "gv", "😀😀x👍🏽");
add(`T(()=>"a😀b👨‍👩‍👧".replace(/\\p{RGI_Emoji}/gv,"<$&>"))`, `T(()=>"a😀b".split(/\\p{RGI_Emoji}/v))`);
// Erros da flag v.
for (const p of ["[a&&&b]", "[a--]", "[--a]", "[&&a]", "[a&&b--c]", "[a--b&&c]", "[(]", "[a-z&&b]", "[a&&b-c]", "[\\q{a]", "\\q{a}", "[\\q{a|}b", "[a||b]", "[a!!b]", "[a##b]", "[a$$b]", "[a%%b]", "[a**b]", "[a++b]", "[a,,b]", "[a..b]", "[a::b]", "[a;;b]", "[a<<b]", "[a==b]", "[a>>b]", "[a??b]", "[a@@b]", "[a^^b]", "[a``b]", "[a~~b]", "[[a]", "[a]]", "[^\\q{ab}]", "[^\\p{RGI_Emoji}]", "\\P{RGI_Emoji}", "[\\P{RGI_Emoji}]", "\\p{rgi_emoji}", "\\p{Basic_Emoji=Yes}", "[|]", "[{]", "[}]", "[/]", "[-]", "[a-]", "[\\p{L}-z]", "[a-\\p{L}]", "[z-a]", "[\\d-z]"]) ctor(p, "v");
for (const f of ["uv", "vu", "vv", "vg", "vgimsyd", "viu"]) ctor("a", f);
add(`T(()=>/a/v.unicodeSets+","+/a/v.unicode+","+/a/u.unicodeSets+","+/a/v.flags)`, `T(()=>new RegExp("a","vgimsdy").flags)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"unicodeSets").get.name)`, `T(()=>RegExp.prototype.unicodeSets)`);

// ---- 2. \p{...}: General_Category, Script, Script_Extensions, binárias.
const gc = ["L", "Lu", "Ll", "Lt", "LC", "Lm", "Lo", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "S", "Sm", "Sc", "Sk", "So", "Z", "Zs", "Zl", "Zp", "C", "Cc", "Cf", "Cs", "Co", "Cn", "Letter", "Uppercase_Letter", "Decimal_Number", "Punctuation", "Symbol", "Separator", "Other", "Control", "Format", "Unassigned", "Math_Symbol", "Currency_Symbol", "Space_Separator", "Mark", "Number", "digit", "punct", "Combining_Mark", "Cased_Letter"];
const gcSamples = ["a", "A", "ǅ", "ʰ", "ª", "\u0301", "\u0903", "5", "Ⅷ", "²", "_", "-", "(", ")", "«", "»", "!", "+", "$", "^", "©", " ", "\u2028", "\u2029", "\x00", "\u200b", "\ud83d\ude00", "\ue000", "\u0378", "中", "あ", "ß", "İ"];
for (const g of gc) {
  ctor(`\\p{${g}}`, "u");
  add(`T(()=>Array.from(${q(gcSamples.join(""))}.matchAll(/\\p{${g}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
  add(`T(()=>Array.from(${q(gcSamples.join(""))}.matchAll(/\\P{${g}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
  exec(`\\p{General_Category=${g}}+`, "u", gcSamples.join(""));
  exec(`\\p{gc=${g}}+`, "u", "aA5 -");
}
const scripts = ["Latin", "Latn", "Greek", "Grek", "Cyrillic", "Cyrl", "Han", "Hani", "Hiragana", "Hira", "Katakana", "Kana", "Arabic", "Arab", "Hebrew", "Hebr", "Thai", "Devanagari", "Deva", "Hangul", "Hang", "Common", "Zyyy", "Inherited", "Zinh", "Qaai", "Armenian", "Georgian", "Ethiopic", "Unknown", "Zzzz", "Braille", "Cherokee", "Coptic", "Adlam", "Vithkuqi", "Kawi", "Nag_Mundari", "Toto", "Tangsa"];
const scSamples = ["a", "α", "я", "中", "あ", "ア", "ع", "ש", "ก", "अ", "한", "1", "\u0301", "ա", "ა", "አ", "\u2800", "Ꭰ", "ⲁ", "𞤀", "𐖘", "ー", "、", "\u0378", "ー"];
for (const s of scripts) {
  ctor(`\\p{Script=${s}}`, "u");
  add(`T(()=>Array.from(${q(scSamples.join(""))}.matchAll(/\\p{Script=${s}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
  add(`T(()=>Array.from(${q(scSamples.join(""))}.matchAll(/\\p{sc=${s}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
  add(`T(()=>Array.from(${q(scSamples.join(""))}.matchAll(/\\p{scx=${s}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
  add(`T(()=>Array.from(${q(scSamples.join(""))}.matchAll(/\\p{Script_Extensions=${s}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
}
for (const [c, s] of [["ー", "Hira"], ["ー", "Kana"], ["ー", "Hiragana"], ["、", "Han"], ["、", "Hira"], ["\u0301", "Greek"], ["\u0301", "Latin"], ["\u0640", "Arab"], ["\u0640", "Syrc"], ["\u30fb", "Kana"], ["\u30fb", "Bopo"], ["々", "Han"], ["\u3003", "Hira"]]) {
  exec(`^\\p{Script=${s}}$`, "u", c);
  exec(`^\\p{Script_Extensions=${s}}$`, "u", c);
  exec(`^\\p{scx=${s}}$`, "v", c);
}
const bins = ["Alphabetic", "Alpha", "Emoji", "Emoji_Presentation", "Emoji_Modifier", "Emoji_Modifier_Base", "Emoji_Component", "Extended_Pictographic", "ID_Start", "IDS", "ID_Continue", "IDC", "XID_Start", "XID_Continue", "Uppercase", "Lowercase", "White_Space", "space", "ASCII", "ASCII_Hex_Digit", "AHex", "Hex_Digit", "Any", "Assigned", "Cased", "Case_Ignorable", "Changes_When_Lowercased", "Changes_When_Uppercased", "Changes_When_Casefolded", "Changes_When_Casemapped", "Changes_When_NFKC_Casefolded", "Dash", "Default_Ignorable_Code_Point", "Deprecated", "Diacritic", "Extender", "Grapheme_Base", "Grapheme_Extend", "IDS_Binary_Operator", "Ideographic", "Join_Control", "Logical_Order_Exception", "Math", "Noncharacter_Code_Point", "Pattern_Syntax", "Pattern_White_Space", "Quotation_Mark", "Radical", "Regional_Indicator", "RI", "Sentence_Terminal", "Soft_Dotted", "Terminal_Punctuation", "Unified_Ideograph", "Variation_Selector", "Bidi_Control", "Bidi_Mirrored", "IDS_Trinary_Operator", "IDS_Unary_Operator", "Lowercase_Letter_X"];
const binSamples = ["a", "A", "1", "_", " ", "😀", "\u200d", "\u0301", "中", "é", "\u00ad", "\ufdd0", "🇧", "\u0e01", "(", "-", "\u2028", "©", "#", "*", "\ufe0f", "\u202e", "ⅷ", "ǆ", "⿰", "\u2e80"];
for (const b of bins) {
  ctor(`\\p{${b}}`, "u");
  add(`T(()=>Array.from(${q(binSamples.join(""))}.matchAll(/\\p{${b}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
  add(`T(()=>Array.from(${q(binSamples.join(""))}.matchAll(/\\P{${b}}/gu),m=>m[0].codePointAt(0).toString(16)).join())`);
}
// Erros de \p.
for (const p of ["\\p", "\\p{", "\\p{}", "\\p{L", "\\p{Foo}", "\\p{Script}", "\\p{Script=}", "\\p{Script=Foo}", "\\p{gc=Foo}", "\\p{L=Lu}", "\\p{Alphabetic=Yes}", "\\p{Alphabetic=True}", "\\p{lu}", "\\p{LU}", "\\p{Latin}", "\\p{ Lu}", "\\p{Lu }", "\\p{script=Latin}", "\\p{Script=latin}", "\\p{Block=Basic_Latin}", "\\p{InBasicLatin}", "\\p{IsLatin}", "\\P", "\\P{", "\\p{Is}", "[\\p{L}-\\p{Lu}]", "\\p{Script_Extensions}", "\\p{General_Category}", "\\p{Any=Yes}", "\\p{Line_Break=Alphabetic}", "\\p{Age=6.0}", "\\p{Numeric_Type=Decimal}", "\\p{Bidi_Class=L}", "\\p{East_Asian_Width=Wide}"]) ctor(p, "u");
for (const p of ["\\p{L}", "\\P{L}", "\\p{Foo}", "\\p", "[\\p{L}-a]"]) exec(p, "", "pL{L}");

// ---- 3. Case-insensitive com unicode.
const ciPairs = [["ſ", "s"], ["ſ", "S"], ["s", "ſ"], ["S", "ſ"], ["K", "k"], ["K", "K"], ["k", "K"], ["\u212a", "\u212a"], ["ß", "ẞ"], ["ẞ", "ß"], ["ß", "ss"], ["İ", "i"], ["ı", "I"], ["ı", "i"], ["Σ", "ς"], ["ς", "σ"], ["σ", "Σ"], ["µ", "μ"], ["µ", "Μ"], ["Å", "å"], ["\u212b", "å"], ["Ω", "ω"], ["\u2126", "ω"], ["ǆ", "ǅ"], ["ǅ", "Ǆ"], ["ﬃ", "FFI"], ["ŉ", "ʼN"], ["ǰ", "J̌"], ["ᾳ", "ᾼ"], ["θ", "ϑ"], ["ϴ", "θ"], ["ẛ", "ṡ"], ["ſ", "ß"], ["ᲀ", "в"], ["Ꭰ", "ꭰ"], ["𐐀", "𐐨"], ["𐐨", "𐐀"], ["Ⴀ", "ⴀ"], ["\u1e9e", "\u00df"], ["ǈ", "ǉ"]];
for (const [a, b] of ciPairs) {
  for (const f of ["i", "iu", "iv"]) {
    exec(`^${a}$`, f, b);
    exec(`^[${a}]$`, f, b);
  }
  exec(`^[^${a}]$`, "iu", b);
  exec(`^\\${"u"}{${a.codePointAt(0).toString(16)}}$`, "iu", b);
}
for (const [p, s] of [["\\w", "ſ"], ["\\w", "\u212a"], ["\\W", "ſ"], ["\\W", "\u212a"], ["[\\w]", "ſ"], ["[^\\W]", "ſ"], ["\\b", "ſ"], ["\\B", "ſ"], ["[\\W]", "K"], ["[^\\w]", "K"], ["\\w", "S"], ["[^\\w]", "s"], ["[\\W]", "s"]]) {
  for (const f of ["", "i", "iu", "iv", "u", "v"]) exec(p, f, s);
}
for (const s of ["ſ", "s", "S", "K", "k", "\u212a"]) {
  for (const f of ["iu", "iv", "i"]) {
    exec("\\b", f, s);
    exec("(.)\\1", f, s + "s");
    exec("(.)\\1", f, s + "k");
    exec("[a-z]", f, s);
    exec("[^a-z]", f, s);
    exec("\\P{Ll}", f, s);
    exec("\\p{Lu}", f, s);
  }
}
for (const f of ["iu", "iv"]) {
  exec("[\\q{ſ}]", f.replace("iu", "iv"), "s");
  exec("[\\q{ſs|K}]", "iv", "SS");
  exec("[\\q{ſs|K}]", "iv", "ſs");
  exec("[[a-z]--[s]]", "iv", "ſ");
  exec("[[a-z]--[s]]", "iv", "S");
  exec("[[a-z]--[k]]", "iv", "\u212a");
  exec("[\\p{Lu}--[A]]", "iv", "a");
  exec("[^\\p{Lu}]", "iv", "a");
  exec("[^\\P{Lu}]", "iv", "a");
}
add(`T(()=>/[\\u{10400}-\\u{10427}]/iu.test("\\u{10428}"))`,`T(()=>/\\u{10400}/i.test("\\u{10400}"))`, `T(()=>/^.$/iu.test("\\u{10400}"))`, `T(()=>/^.$/i.test("\\u{10400}"))`);
add(`T(()=>/\\u212a/i.test("k"))`, `T(()=>/\\u212a/iu.test("k"))`, `T(()=>/[\\u212a]/i.test("K"))`, `T(()=>/[a-z]/i.test("\\u212a"))`, `T(()=>/[a-z]/iu.test("\\u212a"))`, `T(()=>/[a-z]/iu.test("\\u017f"))`, `T(()=>/[a-z]/i.test("\\u017f"))`, `T(()=>/\\u017f/i.test("S"))`, `T(()=>/\\u017f/iu.test("S"))`);

// ---- 4. Named groups duplicados (ES2025) e referências.
const dups = [
  ["(?<a>x)|(?<a>y)", ["x", "y", "z"]],
  ["(?:(?<a>x)|(?<a>y))\\k<a>", ["xx", "yy", "xy"]],
  ["(?:(?<a>x)|(?<a>y))+", ["xy", "yx", "xyx"]],
  ["(?:(?<a>x)|y)(?:(?<a>z)|w)", ["xz"]],
  ["(?:(?<a>a)|b)|(?<a>c)", ["a", "b", "c"]],
  ["(?<a>a)|(?:b|(?<a>c))", ["a", "b", "c"]],
  ["(?:(?<a>a)(?<b>b)|(?<b>c)(?<a>d))", ["ab", "cd"]],
  ["(?:(?<a>a)|(?<a>b))(?<a>c)", ["ac"]],
  ["(?<a>a)(?:(?<a>b)|c)", ["ab"]],
  ["(?:(?<a>a)|(?<a>b)|(?<a>c))\\k<a>\\k<a>", ["aaa", "bbb", "abc"]],
  ["(?:(?<a>.)|(?<a>..))\\k<a>", ["aa", "abab", "aab"]],
  ["\\k<a>(?:(?<a>x)|(?<a>y))", ["x", "y", "xx"]],
  ["(?:(?<a>a)\\k<a>|(?<a>b)\\k<a>)", ["aa", "bb", "ab"]],
  ["(?:(?<a>x)|(?<b>y)|(?<a>z))", ["z", "y"]],
  ["(?<a>x)|(?<a>x)|(?<a>x)", ["x"]],
  ["(?:(?<n>\\d+)px|(?<n>\\d+)em)", ["12px", "3em", "7"]],
  ["(?<a>a)?(?:(?<a>b)|c)", ["c", "b", "ac", "ab"]],
  ["(?:(?<a>a)|b)*", ["ab", "ba", "aba"]],
  ["(?:(?<a>a)|(?<a>b))*?c", ["abc", "bac"]],
  ["(?<=(?<a>a)|(?<a>b))c", ["ac", "bc"]],
  ["(?=(?<a>a)|(?<a>b))\\w", ["a", "b"]],
  ["(?!(?<a>a)|(?<a>b))\\w", ["a", "c"]],
];
for (const [p, ss] of dups) {
  for (const s of ss) {
    exec(p, "", s);
    exec(p, "d", s);
    exec(p, "u", s);
  }
  add(`T(()=>Object.keys(${re(p, "")}.exec(${q(ss[0])})?.groups||{}).join())`);
}
add(`T(()=>"xy".replace(/(?<a>x)|(?<a>y)/g,"[$<a>]"))`, `T(()=>"xy".replace(/(?<a>x)|(?<a>y)/g,(...m)=>JSON.stringify(m[m.length-1])))`, `T(()=>[..."xyx".matchAll(/(?<a>x)|(?<a>y)/g)].map(m=>m.groups.a).join())`, `T(()=>"yx".replace(/(?<a>x)|(?<a>y)/g,"$<a>$<a>"))`);
for (const p of ["(?<a>x)(?<a>y)", "(?<a>x)(?:(?<a>y))", "(?:(?<a>x))(?<a>y)", "(?<a>x)(?<b>y)(?<a>z)", "(?:(?<a>x)|(?<b>y))(?<a>z)", "(?<a>x)|(?<b>y)(?<b>z)", "(?:(?<a>x)(?<a>y)|z)", "(?<a>x)+(?<a>y)", "(?:(?<a>x)|(?<a>y))(?:(?<a>x)|(?<a>y))", "((?<a>x)|(?<a>y))(?<a>z)"]) {
  ctor(p, "");
  ctor(p, "u");
}

// ---- 5. Modificadores (?i:...), (?-i:...), (?s:...), (?m:...).
const mods = [
  ["(?i:a)b", ["Ab", "AB", "ab", "aB"]],
  ["(?i:a)b", ["Ab"]],
  ["a(?-i:b)", ["aB", "Ab", "AB", "ab"]],
  ["(?-i:a)b", ["AB", "aB"]],
  ["(?i:a|b)c", ["Ac", "BC", "bc"]],
  ["(?i:[a-c])d", ["Bd", "BD"]],
  ["(?i:\\u212a)", ["k", "K"]],
  ["(?i:a(?-i:b)c)", ["ABC", "AbC", "aBc"]],
  ["(?i:a(?-i:b(?i:c)))", ["aBC", "ABC", "abC"]],
  ["(?s:.)", ["\n", "a"]],
  ["(?-s:.)", ["\n", "a"]],
  ["(?m:^b)", ["a\nb"]],
  ["(?-m:^b)", ["a\nb"]],
  ["(?m:a$)", ["a\nb"]],
  ["(?ims:a.b$)", ["A\nB"]],
  ["(?i-m:a)", ["A"]],
  ["(?is-m:a.)", ["A\n"]],
  ["(?i:(a))\\1", ["Aa", "aA", "aa"]],
  ["(a)(?i:\\1)", ["aA", "aa", "Aa"]],
  ["(?i:a)+", ["aAaA"]],
  ["(?i:(?<n>a))\\k<n>", ["Aa"]],
  ["(?i:\\w)", ["ſ", "K"]],
  ["(?i:\\p{Lu})", ["a"]],
  ["(?i:\\P{Lu})", ["A"]],
  ["(?i:[^a])", ["A", "b"]],
  ["(?i:\\b)", ["a"]],
  ["(?-i:a)", ["A"]],
  ["(?i:)", [""]],
  ["(?:(?i:a)|b)", ["A", "B"]],
  ["(?i:a)(?=B)", ["aB", "ab"]],
  ["(?<=(?i:A))b", ["ab", "Ab", "aB"]],
  ["(?i:(?<=A))b", ["ab"]],
  ["(?i:x(?s:.)y)", ["X\nY", "x\nY", "xaY"]],
  ["(?i:\\u0041)", ["a"]],
  ["(?i:\\x41)", ["a"]],
  ["(?i:\\cJ)", ["\n"]],
  ["(?i:[\\q{a}])", ["A"]],
];
for (const [p, ss] of mods) {
  for (const s of ss) {
    exec(p, "", s);
    exec(p, "u", s);
    exec(p, "v", s);
  }
}
for (const p of ["(?i", "(?i:", "(?i)", "(?ii:a)", "(?i-i:a)", "(?-:a)", "(?:-i:a)", "(?x:a)", "(?I:a)", "(?g:a)", "(?u:a)", "(?v:a)", "(?y:a)", "(?d:a)", "(?i-:a)", "(?-ii:a)", "(?imsi:a)", "(?i-s-m:a)", "(?is-ms:a)", "(?--i:a)", "(?i:a", "(?i:a))", "(?-i-s:a)", "(?:i:a)", "(?i:)+", "(?i:a)*", "(?i:a)?", "(?i:a){2}", "(?i:a)+?", "(? i:a)", "(?i :a)", "(?i-s:a)", "(?-ims:a)", "(?ims-:a)", "(?\\u0069:a)"]) {
  ctor(p, "");
  ctor(p, "u");
}

// ---- 6. Lookbehind variável e backreferences.
const lbs = [
  ["(?<=a+)b", ["aab", "b", "ab"]],
  ["(?<=a*)b", ["aab", "b"]],
  ["(?<=\\d{2,3})x", ["1x", "12x", "1234x"]],
  ["(?<=(a|bc))x", ["ax", "bcx", "cx"]],
  ["(?<=(\\d+)(\\d+))$", ["1053"]],
  ["(?<=\\1(a))b", ["aab", "ab"]],
  ["(?<=(a)\\1)b", ["aab", "ab"]],
  ["(?<=(?<n>a)\\k<n>)b", ["aab", "ab"]],
  ["(?<=\\k<n>(?<n>a))b", ["aab", "ab"]],
  ["(?<!a+)b", ["aab", "b", "cb"]],
  ["(?<!\\d{2})x", ["1x", "12x", "x"]],
  ["(?<=^|,)\\w+", ["a,bb,ccc"]],
  ["(?<=\\$)\\d+(\\.\\d+)?", ["$10.50", "10"]],
  ["(?<=(?<!a)b)c", ["bc", "abc", "xbc"]],
  ["(?<=(?=a)a)b", ["ab"]],
  ["(?<=a(?=b))b", ["ab"]],
  ["(?<=[a-c]+?)d", ["abcd"]],
  ["(?<=(a+?))b", ["aab"]],
  ["(?<=(a+))b", ["aab"]],
  ["(?<=(\\w+)\\s)(\\w+)", ["hello world"]],
  ["(?<=\\b)\\w", ["ab cd"]],
  ["(?<=.)\\b", ["ab cd"]],
  ["(?<=\\p{Lu}+)\\p{Ll}", ["ABc"]],
  ["(?<=[\\q{ab}c])d", ["abd", "cd"]],
  ["(?<=(?:a|bb)+)c", ["abbc", "bc", "bbbc"]],
  ["(?<=\\u{1F600}+)x", ["😀😀x"]],
  ["(?<=^.)x", ["😀x"]],
  ["(?<=(.))\\1", ["aa", "ab"]],
  ["(?<=(a)|b)\\1", ["a", "b", "aa"]],
  ["(\\1a)", ["a"]],
  ["(?:(a)|b)\\1", ["b", "aa", "ba"]],
  ["\\2(a)(b)", ["ab"]],
  ["(a)\\2", ["a"]],
  ["\\k<a>(?<a>x)", ["x"]],
  ["(?<a>x)\\k<a>*", ["xxx"]],
  ["(?<a>x)\\k<a>{2}", ["xxx"]],
  ["(?<a>.)(?<b>.)\\k<b>\\k<a>", ["abba", "abab"]],
  ["(?<a>.)\\k<a>(?<b>.)\\k<b>", ["aabb"]],
  ["(?<a>(?<b>x)\\k<b>)\\k<a>", ["xxxx"]],
  ["(?<a>a)|\\k<a>b", ["b", "ab"]],
  ["(?:\\k<a>|(?<a>x))+", ["xx"]],
];
for (const [p, ss] of lbs) {
  for (const s of ss) {
    exec(p, "", s);
    exec(p, "u", s);
    exec(p, "d", s);
  }
}
for (const p of ["\\k<a>", "\\k", "\\k<>", "\\k<a", "(?<a>x)\\k<b>", "(?<a>x)\\k", "(?<a>x)\\k<", "(?<a>x)\\k<a", "(?<a>x)\\k<1>", "(?<1>x)", "(?<a-b>x)", "(?<a b>x)", "(?<>x)", "(?<a", "(?<a>", "(?<", "(?<=", "(?<!", "(?<a>x)(?<a>y)\\k<a>", "(?<\\u0061>x)\\k<a>", "(?<\\u{61}>x)\\k<a>", "(?<\\u{1d7d8}>x)", "(?<$>x)\\k<$>", "(?<_a>x)\\k<_a>", "(?<ä>x)\\k<ä>", "(?<a\\u200c>x)", "(?<\\ud835\\udc9c>x)", "(?<𝒜>x)\\k<𝒜>", "(?<a>x)\\k<\\u0061>", "(?<a>x)(?<A>y)", "(?<a>)\\1", "\\1(a)", "\\8", "\\9(a)", "(?<=a)*", "(?<=a)+", "(?<=a)?", "(?<=a){2}", "(?<!a)*", "(?=a)*", "(?=a)+", "(?!a){2}", "(?=a)?", "a**", "a{2}{3}", "a{3,2}", "a{,3}", "{", "}", "a{", "a{1", "a{1,", "x{1}{", "]", "(?", "(?x", "(?<=a", "(", ")", "[", "[a", "\\", "a\\", "(?:", "?", "*", "+", "a|*", "(*)", "(?=*)", "^*", "$+", "\\b*", "\\B+", "\\c", "\\c1", "[\\c1]", "\\u{", "\\u{110000}", "\\u{}", "\\xZ", "\\u12", "\\-", "[\\-]", "\\_", "\\a", "\\e", "\\z", "\\ ", "\\/", "[b-a]", "[\\d-a]", "[a-\\d]", "[--a]", "\\p{L}", "\\k<a>(?<a>x)", "a{2,1}", "a{99999999999}", "a{1,99999999999}"]) {
  ctor(p, "");
  ctor(p, "u");
  ctor(p, "v");
}

// ---- 7. hasIndices `d`.
for (const [p, s] of [["a(b)?", "a"], ["a(b)?", "ab"], ["(?<x>a)(?<y>b)?", "a"], ["(?<x>a)(?<y>b)?", "ab"], ["(?<x>\\d+)-(?<y>\\d+)", "10-200"], ["(a)|(b)", "b"], ["(?:(a)|(b))+", "ab"], ["(?:(a)|(b))+", "ba"], ["(?<=(a))b", "ab"], ["(?<x>.)\\k<x>", "😀😀"], ["(?<x>😀)", "a😀"], ["(?<x>.)", "😀"], ["(?<x>\\p{Emoji})", "a😀"], ["((a)|(b))*", "ab"], ["()", ""], ["(?<e>)", "abc"], ["(a)(?=(b))", "ab"], ["(?<n>a)(?<m>b)(?<n2>c)", "xabc"], ["(?:(?<a>x)|(?<a>y))z", "yz"], ["(?:(?<a>x)|(?<a>y))z", "xz"], ["(?<a>x)|(?<a>y)", "q"]]) {
  for (const f of ["d", "du", "dg", "dy", "dv"]) exec(p, f, s);
}
add(`T(()=>{var m=/(?<a>x)/d.exec("yx");return [m.indices.length,m.indices.groups.a,Object.getPrototypeOf(m.indices.groups)===null,Object.getPrototypeOf(m.groups)===null]})`);
add(`T(()=>{var m=/(x)/d.exec("yx");return [typeof m.indices,Array.isArray(m.indices),m.indices.groups]})`);
add(`T(()=>{var m=/(x)/.exec("yx");return [typeof m.indices,"indices" in m]})`);
add(`T(()=>Object.keys(/(x)/d.exec("yx")).join())`, `T(()=>Object.keys(/(?<a>x)/d.exec("yx")).join())`, `T(()=>JSON.stringify(Object.getOwnPropertyDescriptor(/(x)/d.exec("x"),"indices")))`);
add(`T(()=>/a/d.hasIndices+","+/a/.hasIndices+","+/a/d.flags)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"hasIndices").get.call(/a/d))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"hasIndices").get.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"hasIndices").get.call(RegExp.prototype))`);
add(`T(()=>[..."a😀b😀".matchAll(/😀/dgu)].map(m=>m.indices[0]))`, `T(()=>[..."a😀b😀".matchAll(/😀/dg)].map(m=>m.indices[0]))`);

// ---- 8. RegExp.prototype.flags/source/toString com escapes.
for (const p of ["", "/", "\\/", "[/]", "a/b", "\n", "\r", "\u2028", "\u2029", "\\n", "\\\n", "a\nb", "[\n]", "\\/\\/", "[\\/]", "\\\\/", "(?:)", "\\0", "\\x00", "\\u0000", "\\ud83d\\ude00", "😀", "\\u{1F600}", "[^]", "[]", "\\cA", "a|b", "\\", "\\\\", "\\\\\\/", "/[/]/", "[/\\]/]", "\u00e9", "\u0000", "\t", " ", "\\ ", "\\-", "[\\-]", "$", "^", ".", "?", "\\?", "(?<n>a)\\k<n>", "(?i:a)", "\\p{L}", "[\\q{a}]", "]", "}", "{", "a{1}", "\\{"]) {
  for (const f of ["", "u", "v", "gimsuyd"]) {
    add(`T(()=>{var r=new RegExp(${q(p)},${q(f)});return [r.source,String(r),r.flags,r.toString===RegExp.prototype.toString]})`);
  }
}
for (const [p, f] of [["a", "gimsuyd"], ["a", "dgimsvy"], ["a", "ydsmigu"], ["a", "yd"], ["a", "vd"]]) ctor(p, f);
for (const f of ["gg", "ii", "mm", "ss", "uu", "yy", "dd", "x", "G", "gx", "u v", "g,i", "gu\u0000", "ä", "😀", "uv", "v v"]) ctor("a", f);
add(
  `T(()=>RegExp.prototype.source)`, `T(()=>RegExp.prototype.flags)`, `T(()=>String(RegExp.prototype))`, `T(()=>RegExp.prototype.toString.call({source:"a",flags:"g"}))`,
  `T(()=>RegExp.prototype.toString.call({}))`, `T(()=>RegExp.prototype.toString.call(1))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"flags").get.call({global:1,hasIndices:1,unicodeSets:1,sticky:1,ignoreCase:1,multiline:1,dotAll:1,unicode:1}))`,
  `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"flags").get.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"flags").get.call(1))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"source").get.call({}))`,
  `T(()=>new RegExp(/a\\/b/g).source)`, `T(()=>new RegExp(/a\\/b/g,"i").flags)`, `T(()=>new RegExp(new RegExp("a/b")).source)`, `T(()=>RegExp("\\n").source)`, `T(()=>RegExp("\\\\n").source)`, `T(()=>RegExp("[\\n]").source)`,
  `T(()=>{var r=/a/g;r.lastIndex=5;var c=new RegExp(r);return [c.lastIndex,c.flags]})`, `T(()=>{var r=/a/g;r.flags;return Object.getOwnPropertyNames(r).join()})`, `T(()=>Object.getOwnPropertyNames(/a/).join())`,
  `T(()=>RegExp(/a/g)===RegExp(/a/g))`, `T(()=>{var r=/a/g;return RegExp(r)===r})`, `T(()=>{var r=/a/g;return RegExp(r,"i")===r})`, `T(()=>{var r=/a/g;r.constructor=null;return RegExp(r)===r})`,
  `T(()=>{var r=/a/g;r.constructor=1;return RegExp(r)===r})`, `T(()=>new RegExp({source:"x",flags:"g",[Symbol.match]:true}).flags)`, `T(()=>new RegExp({source:"x",flags:"g",[Symbol.match]:true}).source)`,
  `T(()=>new RegExp({[Symbol.match]:true,constructor:RegExp,source:"y",flags:"i"}).flags)`, `T(()=>new RegExp("a",undefined).flags)`, `T(()=>new RegExp(undefined).source)`, `T(()=>new RegExp(null).source)`, `T(()=>new RegExp("a",null).flags)`,
  `T(()=>RegExp.prototype.compile.call(/a/g,"b","i")+"")`, `T(()=>{var r=/a/g;r.compile("b","v");return [r.source,r.flags]})`, `T(()=>{var r=/a/g;r.compile(/c/y);return [r.source,r.flags]})`, `T(()=>{var r=/a/g;r.compile(/c/y,"g");return [r.source,r.flags]})`,
  `T(()=>/a/[Symbol.replace].call(1,"a","b"))`, `T(()=>String(/[/]/))`, `T(()=>/[/]/.source)`, `T(()=>/\\//.source)`, `T(()=>eval("/[\\\\u{1F600}-\\\\u{1F64F}]/v").source)`,
);

// ---- 9. lastIndex em unicode e sticky/global.
const lis = [
  ["😀", "ug", "a😀", [0, 1, 2, 3]],
  ["😀", "g", "a😀", [0, 1, 2, 3]],
  ["\\udE00", "ug", "😀", [0, 1, 2]],
  ["\\udE00", "g", "😀", [0, 1, 2]],
  ["^.", "ugm", "😀\n😀", [0, 1, 2, 3, 4]],
  [".", "ug", "😀😀", [0, 1, 2, 3, 4]],
  [".", "g", "😀😀", [0, 1, 2, 3, 4]],
  [".", "uy", "😀😀", [0, 1, 2, 3, 4]],
  [".", "y", "😀😀", [0, 1, 2, 3, 4]],
  ["", "ug", "😀", [0, 1, 2, 3]],
  ["", "g", "😀", [0, 1, 2, 3]],
  ["", "vg", "😀", [0, 1, 2, 3]],
  ["\\p{RGI_Emoji}", "vg", "👨‍👩‍👧", [0, 1, 2, 3, 8]],
  ["[^a]", "ug", "a😀", [0, 1, 2]],
  ["[^a]", "g", "a😀", [0, 1, 2]],
  ["\\udc00", "ug", "\ud800\udc00", [0, 1]],
  ["\\ud800", "ug", "\ud800\udc00", [0, 1]],
  ["\\ud800", "g", "\ud800\udc00", [0, 1]],
  ["\\b", "ug", "😀a", [0, 1, 2, 3]],
  ["$", "ug", "😀", [0, 1, 2]],
  ["a?", "ug", "😀", [0, 1, 2]],
];
for (const [p, f, s, starts] of lis) {
  for (const li of starts) add(`T(()=>{var r=new RegExp(${q(p)},${q(f)});r.lastIndex=${li};var m=r.exec(${q(s)});return [m&&m.index,m&&m[0].length,r.lastIndex]})`);
}
add(`T(()=>{var r=/./gu,o=[];var s="a😀b";while(r.test(s))o.push(r.lastIndex);return o})`, `T(()=>{var r=/./g,o=[];var s="a😀b";while(r.test(s))o.push(r.lastIndex);return o})`, `T(()=>{var r=/(?:)/gu;return "😀".replace(r,"-")})`, `T(()=>"😀".replace(/(?:)/g,"-").length)`, `T(()=>"a😀".split(/(?:)/u))`, `T(()=>"a😀".split(/(?:)/).length)`, `T(()=>"a😀".split(/(?:)/v))`);
add(`T(()=>[..."a😀b".matchAll(/(?:)/gu)].map(m=>m.index))`, `T(()=>[..."a😀b".matchAll(/(?:)/g)].map(m=>m.index))`, `T(()=>[..."a😀b".matchAll(/(?:)/gv)].map(m=>m.index))`);
add(`T(()=>{var r=/a/y;r.lastIndex=-1;return [r.test("a"),r.lastIndex]})`, `T(()=>{var r=/a/g;r.lastIndex=-1;return [r.test("a"),r.lastIndex]})`, `T(()=>{var r=/a/g;r.lastIndex=2**53;return [r.test("a"),r.lastIndex]})`, `T(()=>{var r=/a/g;r.lastIndex="1";r.test("aa");return typeof r.lastIndex+r.lastIndex})`, `T(()=>{var r=/a/g;r.lastIndex={valueOf(){return 1}};r.test("aa");return r.lastIndex})`, `T(()=>{var r=/a/;r.lastIndex=5;r.test("a");return r.lastIndex})`, `T(()=>{var r=/b/y;return [r.test("ab"),r.lastIndex]})`);
add(`T(()=>{var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false});return r.test("a")})`, `T(()=>{var r=/a/;Object.defineProperty(r,"lastIndex",{writable:false});return r.test("b")})`, `T(()=>{var r=/a/g;Object.freeze(r);return r.test("b")})`, `T(()=>Object.getOwnPropertyDescriptor(/a/,"lastIndex").writable+","+Object.getOwnPropertyDescriptor(/a/,"lastIndex").enumerable+","+Object.getOwnPropertyDescriptor(/a/,"lastIndex").configurable)`);

// ---- 10. Sintaxe de literais no código-fonte (early errors).
for (const lit of ["/[a&&b]/v", "/[a&&&b]/v", "/\\p{RGI_Emoji}/v", "/\\p{RGI_Emoji}/u", "/[^\\p{RGI_Emoji}]/v", "/(?<a>x)|(?<a>y)/", "/(?<a>x)(?<a>y)/", "/(?i:a)/", "/(?x:a)/", "/a/uv", "/a/vv", "/a/dd", "/a/x", "/(?<=a+)b/", "/\\k<a>/", "/\\k<a>/u", "/(?<a>.)\\k<b>/", "/\\p{Foo}/u", "/\\p{Foo}/", "/[\\q{a}]/v", "/[\\q{a}]/u", "/\\q{a}/v", "/[a--b]/v", "/[a--b]/u"]) {
  add(`T(()=>String(eval(${q(lit)})))`);
  add(`T(()=>{var f=new Function(${q("return " + lit)});return String(f())})`);
}

// ---- Saída.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  if (/\bsetTimeout\b|\bprocess\b|\brequire\b|\bBun\b|\bconsole\b/.test(expr)) continue;
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${/^T\(/.test(expr) ? expr : `T(()=>{return ${expr}})`}`;
  let result;
  try {
    globalThis.R = undefined;
    vm.runInThisContext(source);
    result = String(globalThis.R);
  } catch (e) {
    dropped++;
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  // Surrogate solto não cabe em String do Rust: o harness não consegue carregar a fonte nem o resultado.
  if (/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(source + result) || /\\ud[89ab][0-9a-f]{2}(?!\\ud[c-f])|(?<!\\ud[89ab][0-9a-f]{2})\\ud[c-f][0-9a-f]{2}/i.test(source + result)) {
    dropped++;
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("regexp_v", rows));
