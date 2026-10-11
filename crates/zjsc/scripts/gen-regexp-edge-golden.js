// Gera tests/golden/regexp_edge_bun.tsv: RegExp de borda, medido no bun 1.4.2.
// Named groups duplicados, lookbehind, propriedades Unicode (\p{...}), flag v (conjuntos e strings), flag d (indices),
// backreferences nomeadas, quantificadores lazy aninhados, padrões catastróficos seguros, sticky e lastIndex,
// Symbol.replace/split/matchAll personalizados, RegExp.escape, modifiers inline `(?i:...)` e mensagens de SyntaxError.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-json-more-golden.js.
// Uso: bun scripts/gen-regexp-edge-golden.js > tests/golden/regexp_edge_bun.tsv
const { emitFactored } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const path = require("path");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function M(m){if(m===null)return "null";var o={a:Array.from(m),i:m.index,g:m.groups===undefined?"u":Object.entries(m.groups)};' +
  'if(m.indices){o.d=Array.from(m.indices);o.dg=m.indices.groups===undefined?"u":Object.entries(m.indices.groups)}return JSON.stringify(o)}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const re = (p, f) => `new RegExp(${q(p)},${q(f || "")})`;
const exec = (p, f, s) => add(`T(()=>M(${re(p, f)}.exec(${q(s)})))`);
const ctor = (p, f) => add(`T(()=>String(${re(p, f)}))`);

// ---- 1. Named groups duplicados (alternativas) e referências nomeadas.
for (const [p, s] of [
  ["(?<a>x)|(?<a>y)", "y"], ["(?<a>x)|(?<a>y)", "x"], ["(?<a>x)|(?<a>y)", "z"], ["(?:(?<a>x)|(?<a>y))\\k<a>", "yy"],
  ["(?:(?<a>x)|(?<a>y))\\k<a>", "xy"], ["(?:(?<a>x)|(?<a>y))\\k<a>", "xx"], ["(?<a>a)|(?<b>b)|(?<a>c)", "c"], ["(?<a>a)|(?<b>b)|(?<a>c)", "b"],
  ["(?:(?<y>\\d{4})-(?<m>\\d\\d)|(?<m>\\d\\d)\\/(?<y>\\d{4}))", "2024-05"], ["(?:(?<y>\\d{4})-(?<m>\\d\\d)|(?<m>\\d\\d)\\/(?<y>\\d{4}))", "05/2024"],
  ["(?<a>x)?(?<b>y)?", ""], ["(?<a>x)(?<a>y)", "xy"], ["(?<a>x)|(?<a>y)|(?<a>z)", "z"], ["(?:(?<a>1)|(?<a>2))+", "12"],
  ["(?:(?<a>1)|(?<a>2))+", "21"], ["(?:(?<a>1)|(?<a>2)){2}", "11"], ["(?<a>.)\\k<a>", "aa"], ["(?<a>.)\\k<a>", "ab"], ["\\k<a>(?<a>x)", "x"],
  ["(?<a>x)\\k<b>", "x"], ["\\k<a>", "k<a>"], ["(?<a>x)\\k<a", "x"], ["(?<a>x)\\k", "x"], ["(?<$>x)(?<_>y)", "xy"], ["(?<\\u0061>x)", "x"],
  ["(?<\\u{61}>x)", "x"], ["(?<é>x)", "x"], ["(?<π>x)", "x"], ["(?<𝒜>x)", "x"], ["(?<a1>x)(?<a2>y)", "xy"], ["(?<1a>x)", "x"], ["(?<a-b>x)", "x"],
  ["(?<>x)", "x"], ["(?<a x)", "x"], ["(?<a>x", "x"], ["(?<a>x)(?<a>y)", "xy"],
]) for (const f of ["", "u", "d", "v", "dg"]) exec(p, f, s);

// ---- 2. Lookbehind e lookahead.
for (const [p, s] of [
  ["(?<=\\$)\\d+", "cost $42"], ["(?<!\\$)\\b\\d+", "cost $42 and 7"], ["(?<=a)b", "ab"], ["(?<=a)b", "cb"], ["(?<!a)b", "cb"], ["(?<!a)b", "ab"],
  ["(?<=(\\d)(\\d))x", "12x"], ["(?<=\\1(a))b", "aab"], ["(?<=(a)\\1)b", "aab"], ["(?<=(?<n>a)\\k<n>)b", "aab"], ["(?<=a*)b", "aaab"],
  ["(?<=a+?)b", "aaab"], ["(?<=^|,)\\w+", "a,b"], ["(?<=ab|abc)d", "abcd"], ["(?<=(ab|abc))d", "abcd"], ["(?<=(?:a|ab)(?:c|bc))d", "abcd"],
  ["(?<=[a-z]{2})\\d", "ab1"], ["(?<=\\b)x", "ax x"], ["(?<!\\b)x", "ax x"], ["(?<=(?<!a)b)c", "bc"], ["(?<=(?<!a)b)c", "abc"],
  ["(?<=.)", "ab"], ["(?<=^)", "ab"], ["(?<=$)", "ab"], ["(?<=\\s)\\S+(?=\\s)", "a bc d"], ["(?=(a))\\1", "a"], ["(?!(a))\\1b", "b"],
  ["(?=a)*", "a"], ["(?=a){2}", "a"], ["(?<=a)*", "a"], ["(?<=a){2}", "a"], ["(?!a)+", "b"], ["(?=a)+b", "b"], ["(?<=(a))\\1", "aa"],
  ["(?=(a)|b)\\1", "b"], ["(?<=(a)|b)\\1", "ab"], ["((?<=a))b", "ab"], ["(?<=a|b)+c", "bc"], ["x(?<=x)", "x"], ["(?<=\\k<n>(?<n>a))b", "ab"],
  ["(?<=\\u{1F600})x", "\u{1F600}x"], ["(?<=.)x", "\u{1F600}x"], ["(?<=\\ud83d)x", "\u{1F600}x"],
]) for (const f of ["", "u", "d"]) exec(p, f, s);

// ---- 3. Propriedades Unicode.
const props = [
  "L", "Lu", "Ll", "Lt", "Lm", "Lo", "LC", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "S", "Sm", "Sc", "Sk", "So",
  "Z", "Zs", "Zl", "Zp", "C", "Cc", "Cf", "Cn", "Co", "Cs", "Letter", "Uppercase_Letter", "Decimal_Number", "General_Category=Lu", "gc=Nd", "gc=Letter",
  "Script=Greek", "sc=Grek", "Script=Latin", "sc=Cyrl", "Script=Han", "scx=Han", "Script_Extensions=Hira", "scx=Kana", "Script=Hiragana", "Script=Arabic",
  "scx=Deva", "Script=Common", "Script=Inherited", "sc=Zyyy", "sc=Zinh", "Script=Thai", "Script=Hebrew", "scx=Latn", "Script=Hangul", "Script=Unknown",
  "Alphabetic", "Alpha", "Uppercase", "Lowercase", "White_Space", "space", "ASCII", "Any", "Assigned", "ASCII_Hex_Digit", "AHex", "Hex_Digit", "ID_Start", "IDS",
  "ID_Continue", "IDC", "XID_Start", "XID_Continue", "Emoji", "Emoji_Presentation", "Emoji_Modifier", "Emoji_Modifier_Base", "Emoji_Component", "Extended_Pictographic",
  "Ideographic", "Math", "Dash", "Diacritic", "Cased", "Case_Ignorable", "Changes_When_Lowercased", "Changes_When_NFKC_Casefolded", "Default_Ignorable_Code_Point",
  "Regional_Indicator", "RI", "Variation_Selector", "Grapheme_Base", "Grapheme_Extend", "Noncharacter_Code_Point", "Pattern_Syntax", "Pattern_White_Space",
  "Quotation_Mark", "Radical", "Sentence_Terminal", "Soft_Dotted", "Terminal_Punctuation", "Unified_Ideograph", "Bidi_Mirrored", "Join_Control", "Logical_Order_Exception",
];
const samples = ["a", "A", "ǅ", "ʰ", "ª", "\u0301", "\u0903", "5", "Ⅷ", "²", "_", "-", "(", "«", "!", "+", "$", "^", "©", " ", "\u2028", "\u0000", "\u00AD", "\uE000",
  "α", "я", "漢", "ひ", "カ", "ア", "ب", "ह", "ก", "א", "한", "\u{1F600}", "\u{1F1E7}", "\u{1F3FD}", "\u30FC", "\u3099", "\uFE0F", "\u200D", "\u{10FFFF}", "\u0378", "٣", "·", "∑"];
for (const p of props) {
  add(`T(()=>${re("\\p{" + p + "}", "u")}.test("a"))`, `T(()=>${re("\\P{" + p + "}", "u")}.test("a"))`);
  add(`T(()=>${q(samples.join("")).replace(/^/, "")}.match(${re("\\p{" + p + "}", "gu")}))`);
}
for (const bad of ["\\p", "\\p{", "\\p{}", "\\p{Foo}", "\\p{L", "\\p{lu}", "\\p{Script}", "\\p{Script=}", "\\p{Script=Foo}", "\\p{gc=Foo}", "\\p{General_Category}", "\\p{Lu=Lu}",
  "\\p{ASCII=Yes}", "\\p{Script=Latin=x}", "\\p{ Lu}", "\\p{Lu }", "\\p{L u}", "\\p{IsLu}", "\\p{Block=Basic_Latin}", "\\p{InGreek}", "\\p{Is_Alphabetic}", "[\\p{Lu}-\\p{Ll}]",
  "[\\p{Lu}-z]", "[a-\\p{Lu}]", "\\P{Any}", "\\p{Basic_Emoji}", "\\p{RGI_Emoji}", "\\p{Emoji_Keycap_Sequence}", "\\P{RGI_Emoji}", "[^\\p{RGI_Emoji}]", "\\p{Lowercase_Letter}"]) {
  for (const f of ["u", "", "v"]) add(`T(()=>String(${re(bad, f)}))`);
}
add(`T(()=>/\\p{L}/.test("p{L}"))`, `T(()=>/\\P{L}/.test("P{L}"))`, `T(()=>/\\p/.test("p"))`);
add(`T(()=>"aÀ𝒜".replace(/\\p{Lu}/gu, "[$&]"))`, `T(()=>"aÀ𝒜".replace(/\\p{Lu}/gi, "[$&]"))`, `T(()=>"aÀ𝒜".replace(/\\p{Lu}/giu, "[$&]"))`, `T(()=>"aÀ𝒜".replace(/\\p{Lu}/giv, "[$&]"))`,
  `T(()=>"aÀ𝒜".replace(/\\P{Lu}/giu, "[$&]"))`, `T(()=>"aÀ𝒜".replace(/[^\\p{Lu}]/giu, "[$&]"))`, `T(()=>"aÀ𝒜".replace(/[^\\p{Lu}]/giv, "[$&]"))`,
  `T(()=>/\\p{Lu}/iu.test("a"))`, `T(()=>/\\P{Lu}/iu.test("A"))`, `T(()=>/[\\p{Lu}]/iu.test("a"))`, `T(()=>/\\p{Ll}/iu.test("A"))`, `T(()=>/[^\\P{Lu}]/iu.test("a"))`,
  `T(()=>/\\P{Ll}/iv.test("a"))`, `T(()=>/[^\\P{Ll}]/iv.test("A"))`, `T(()=>/\\w/iu.test("\\u017f"))`, `T(()=>/\\w/iu.test("\\u212a"))`, `T(()=>/[\\w-a]/u.test("-"))`,
  `T(()=>/\\W/iu.test("S"))`, `T(()=>/[^\\W]/iu.test("\\u017f"))`, `T(()=>/\\b/iu.test("\\u017f"))`);

// ---- 4. Flag v: conjuntos, strings e escapes.
for (const [p, s] of [
  ["[\\p{L}--[a-z]]", "aBc"], ["[\\p{L}&&\\p{ASCII}]", "aéB"], ["[[a-z]--[aeiou]]+", "hello"], ["[[a-z]&&[aeiou]]+", "hello"], ["[\\w--\\d]+", "ab12cd"], ["[\\w&&\\d]+", "ab12cd"],
  ["[a-z--b]", "b"], ["[a-z--b]", "c"], ["[[a-c][x-z]]+", "azbycx"], ["[\\q{abc|d|}]", "abc"], ["[\\q{abc|d|}]", "d"], ["[\\q{abc|d|}]x", "x"], ["[\\q{abc|ab|a}]+", "abcaba"],
  ["[\\q{ab}--\\q{ab}]", "ab"], ["[[a-z]--\\q{ab}]", "ab"], ["[\\q{ab|c}&&\\q{ab}]", "ab"], ["[\\q{ab|c}&&\\q{c}]", "c"], ["[^\\q{a}]", "b"], ["[^\\q{ab}]", "b"], ["[^\\q{}]", "b"],
  ["\\p{Emoji_Keycap_Sequence}", "1\uFE0F\u20E3"], ["\\p{RGI_Emoji_Flag_Sequence}", "\u{1F1E7}\u{1F1F7}"], ["\\p{Basic_Emoji}", "\u{1F600}"], ["\\p{RGI_Emoji}", "\u{1F468}\u200D\u{1F469}\u200D\u{1F467}"],
  ["\\p{RGI_Emoji_ZWJ_Sequence}", "\u{1F468}\u200D\u{1F469}\u200D\u{1F467}"], ["\\p{RGI_Emoji_Modifier_Sequence}", "\u{1F44B}\u{1F3FD}"], ["\\p{RGI_Emoji_Tag_Sequence}", "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}"],
  ["^\\p{RGI_Emoji}$", "\u{1F600}"], ["[\\p{Emoji_Keycap_Sequence}a]", "a"], ["[\\p{RGI_Emoji}--\\q{\u{1F600}}]", "\u{1F600}"], ["\\p{Lu}", "A"], ["[\\p{Lu}\\q{ab}]", "ab"],
  ["[(]", "("], ["[\\(]", "("], ["[a&&&b]", "a"], ["[a&&b]", "a"], ["[a--]", "a"], ["[--a]", "a"], ["[a-z&&b]", "b"], ["[a&&b&&c]", "a"], ["[a--b--c]", "a"], ["[a--b&&c]", "a"],
  ["[a&&b--c]", "a"], ["[&&]", "&"], ["[a&b]", "&"], ["[a!!b]", "!"], ["[(]", "("], ["[)]", ")"], ["[{]", "{"], ["[}]", "}"], ["[/]", "/"], ["[-]", "-"], ["[|]", "|"], ["[a-]", "-"],
  ["[\\-]", "-"], ["[\\&]", "&"], ["[\\!]", "!"], ["[\\#]", "#"], ["[\\%]", "%"], ["[\\,]", ","], ["[\\:]", ":"], ["[\\;]", ";"], ["[\\<]", "<"], ["[\\=]", "="], ["[\\>]", ">"], ["[\\@]", "@"],
  ["[\\`]", "`"], ["[\\~]", "~"], ["[\\a]", "a"], ["[\\q]", "q"], ["\\q{a}", "q{a}"], ["[\\q{a]", "a"], ["[\\q{a}", "a"], ["[\\q{\\u{1F600}}]", "\u{1F600}"], ["[\\q{a\\|b}]", "a|b"],
  ["[\\q{a|b}--a]", "b"], ["[[^a]&&[^b]]", "c"], ["[^[a-z]]", "A"], ["[^[a-z]--b]", "A"], ["[[^a]--b]", "c"], ["[^a-z&&b]", "A"], ["[\\p{L}&&\\P{ASCII}]", "é"], ["[^\\p{Lu}&&\\p{ASCII}]", "a"],
  ["[\\d--[1-3]]+", "0123456"], ["[\\s\\S]", "x"], ["[\\b]", "\b"], ["[\\B]", "B"], ["[\\cJ]", "\n"], ["[\\-a]", "-"], ["[a\\-z]", "-"], ["[[a]-z]", "-"], ["[a-[z]]", "-"],
]) for (const f of ["v", "vi", "vg", "vd", "u"]) exec(p, f, s);
add(`T(()=>/[\\q{abc}]/v.unicodeSets)`, `T(()=>/a/v.flags)`, `T(()=>/a/dgimsvy.flags)`, `T(()=>new RegExp("a","uv"))`, `T(()=>new RegExp("a","vu"))`, `T(()=>new RegExp("a","vv"))`,
  `T(()=>/a/u.unicodeSets)`, `T(()=>/a/v.unicode)`, `T(()=>RegExp.prototype.unicodeSets)`, `T(()=>RegExp.prototype.flags)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"unicodeSets").get.name)`,
  `T(()=>"abcabc".match(/[\\q{abc}]/gv))`, `T(()=>"ABC".match(/[\\q{abc}]/giv))`, `T(()=>"aBc".replace(/[\\q{abc}]/giv,"x"))`, `T(()=>"\u{1F600}".match(/./gv))`,
  `T(()=>"\u{1F600}".match(/^[^x]$/v))`, `T(()=>"\u{1F600}".match(/^[^x]$/u))`, `T(()=>"\u{1F600}".match(/^[^x]$/))`, `T(()=>/^[^\\q{ab}]$/v.test("a"))`);

// ---- 5. Flag d (indices).
for (const [p, f, s] of [
  ["a(b)(c)?", "d", "xab"], ["(?<x>a)(?<y>b)?", "d", "xa"], ["(?<x>a)|(?<x>b)", "d", "b"], ["(a)|(b)", "d", "b"], ["(?:(a)|b)+", "d", "ab"], ["(a)*", "d", "aaa"],
  ["(?<=(a))b", "d", "ab"], ["(?=(a))", "d", "a"], ["", "d", "abc"], ["$", "d", "abc"], ["(?:)", "dg", "ab"], ["\\u{1F600}(x)", "du", "\u{1F600}x"], ["(.)(.)", "du", "\u{1F600}\u{1F601}"],
  ["(.)(.)", "d", "\u{1F600}"], ["a", "dy", "ba"], ["b", "dy", "ba"], ["(b)", "dgy", "bb"], ["(?<n>.)", "dsv", "\n"], ["(a)(?<n>b)", "di", "AB"], ["((a)(b))", "d", "ab"],
  ["(?<a>(?<b>x))", "d", "x"], ["(\\d+)-(\\d+)", "d", "10-20"], ["(?<h>\\d\\d):(?<m>\\d\\d)", "dg", "12:34"], ["(x)\\1", "d", "xx"], ["(?<q>x)\\k<q>", "d", "xx"], ["[a-z]+", "dv", "abc"],
]) exec(p, f, s);
add(`T(()=>/a/d.hasIndices)`, `T(()=>/a/.hasIndices)`, `T(()=>/a/d.flags)`, `T(()=>/a/d.exec("a").indices.length)`, `T(()=>/a/.exec("a").indices)`, `T(()=>Object.keys(/a(b)/d.exec("ab")))`,
  `T(()=>Object.getOwnPropertyNames(/a(?<n>b)/d.exec("ab")))`, `T(()=>Object.getPrototypeOf(/a(?<n>b)/d.exec("ab").indices.groups))`, `T(()=>Object.getPrototypeOf(/a(?<n>b)/.exec("ab").groups))`,
  `T(()=>/a(?<n>b)/d.exec("ab").indices.groups.n)`, `T(()=>[..."aXbX".matchAll(/X/dg)].map(m=>m.indices[0]))`, `T(()=>"ab".replace(/(?<n>b)/d,(...a)=>JSON.stringify(a)))`);

// ---- 6. Quantificadores lazy aninhados e catastrophic seguro.
for (const [p, s] of [
  ["(a+?)+?b", "aaab"], ["(a*?)*?b", "aab"], ["(a??)*?b", "ab"], ["(?:a+?b+?)+?c", "aabbabc"], ["(a|aa)+?$", "aaaa"], ["(a|aa)+$", "aaaa"], ["(a*)*", "b"], ["(a*)+", "b"], ["(a*?)+", "b"],
  ["(?:a*)*b", "aaaa"], ["(a+)+b", "aaaaaaaaaaaac"], ["(a|a)+b", "aaaaaaaaaaaac"], ["(.*a){6}", "aaaaaaaaaaaaaac"],
  ["^(a?){12}a{12}$", "aaaaaaaaaaaa"], ["(x+x+)+y", "xxxxxxxxxxxx"], ["(?:(?:a?){10}){10}b", "aaaaaaaaaa"], ["(a{1,3}?){2,4}?b", "aaaab"], ["a{2,}?", "aaaa"], ["a{2,3}?", "aaaa"],
  ["a{0}", "a"], ["a{0,0}?", "a"], ["a{,3}", "a{,3}"], ["a{1", "a{1"], ["a{1,", "a{1,"], ["a{x}", "a{x}"], ["a{1}{2}", "aa"], ["a{2}{3}", "aaaaaa"], ["a**", "a"], ["a+*", "a"], ["a?+", "a"], ["a??", "a"],
  ["(?:a?)??", "a"], ["(a?){3}", "a"], ["(?:(a)|b)*?c", "abc"], ["(a)|b*?", "b"], ["(?:(a)|(b))*", "ab"], ["(?:(a)|(b)){2}", "ab"], ["(?:(a)|(b)){2}?", "ab"], ["(z)((a+)?(b+)?(c))*", "zaacbbbcac"],
  ["(?:^a)*", "aa"], ["(?:$)*", "a"], ["(?:\\b)+", "a"], ["(\\b)*", "a"], ["(?:a|ab)(?:c|bcd)(?:d*)", "abcd"], ["(a*)b\\1+", "baaaac"], ["(?=(a+))a*b\\1", "baaabac"], ["(.*?)a(?!(a+)b\\2c)\\2(.*)", "baaabaac"],
  ["a{4294967295}", "a"], ["a{4294967296}", "a"], ["a{99999999999999999999}", "a"], ["a{2,1}", "a"], ["a{1,4294967296}", "a"], ["a{4294967295,}", "a"],
]) for (const f of ["", "u"]) exec(p, f, s);

// ---- 7. Sticky e lastIndex.
const sticky = [
  `var r=/a/y;r.lastIndex=1;[r.test("ba"),r.lastIndex,r.test("ba"),r.lastIndex]`, `var r=/a/y;r.lastIndex=0;[r.test("ba"),r.lastIndex]`, `var r=/a/gy;[r.test("aab"),r.lastIndex,r.test("aab"),r.lastIndex,r.test("aab"),r.lastIndex]`,
  `var r=/a/gy;"aaba".match(r)`, `var r=/a/y;"aaba".match(r)`, `var r=/a/y;"baa".search(r)`, `var r=/a/y;r.lastIndex=1;"baa".search(r)+","+r.lastIndex`, `var r=/^a/y;r.lastIndex=1;[r.test("ba"),r.lastIndex]`,
  `var r=/^a/my;r.lastIndex=2;[r.test("b\\na"),r.lastIndex]`, `var r=/a|b/y;r.lastIndex=1;M(r.exec("ab"))`, `var r=/(?<=a)b/y;r.lastIndex=1;[r.test("ab"),r.lastIndex]`, `var r=/\\bb/y;r.lastIndex=1;[r.test("ab"),r.lastIndex]`,
  `var r=/a/y;r.lastIndex=-1;[r.test("a"),r.lastIndex]`, `var r=/a/y;r.lastIndex=5;[r.test("a"),r.lastIndex]`, `var r=/a/g;r.lastIndex="1";[r.test("aa"),r.lastIndex]`, `var r=/a/g;r.lastIndex=1.9;[r.test("aa"),r.lastIndex]`,
  `var r=/a/g;r.lastIndex=NaN;[r.test("a"),r.lastIndex]`, `var r=/a/g;r.lastIndex=-0;[r.test("a"),Object.is(r.lastIndex,-0),r.lastIndex]`, `var r=/a/g;r.lastIndex=2**53;[r.test("a"),r.lastIndex]`,
  `var r=/a/;r.lastIndex=5;[r.test("a"),r.lastIndex]`, `var r=/a/;r.lastIndex=5;[r.test("b"),r.lastIndex]`, `var r=/a/g;r.lastIndex=5;[r.test("b"),r.lastIndex]`, `var r=/a/y;r.lastIndex=5;[r.test("b"),r.lastIndex]`,
  `var r=/a/g;r.lastIndex={valueOf(){return 1}};[r.test("aa"),r.lastIndex]`, `var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false});r.test("b")`, `var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false,value:0});r.test("a")`,
  `var r=/a/;Object.defineProperty(r,"lastIndex",{writable:false,value:0});[r.test("a"),r.lastIndex]`, `var r=/a/;Object.freeze(r);r.test("a")`, `var r=/a/g;Object.freeze(r);r.test("b")`,
  `var r=/\\u{1F600}/uy;r.lastIndex=1;[r.test("\\u{1F600}"),r.lastIndex]`, `var r=/./gu;r.lastIndex=1;[r.exec("\\u{1F600}")[0].length,r.lastIndex]`, `var r=/./gv;r.lastIndex=1;[r.exec("\\u{1F600}")[0].length,r.lastIndex]`,
  `var r=/./g;r.lastIndex=1;[r.exec("\\u{1F600}")[0].length,r.lastIndex]`, `var r=/(?:)/gu;r.lastIndex=1;[r.exec("\\u{1F600}").index,r.lastIndex]`, `"\\u{1F600}".replace(/(?:)/gu,"-")`, `"\\u{1F600}".replace(/(?:)/g,"-").length`,
  `"abc".replace(/(?:)/g,"-")`, `"abc".split(/(?:)/u)`, `"\\u{1F600}a".split(/(?:)/u).length`, `"\\u{1F600}a".split(/(?:)/).length`, `[..."a\\u{1F600}".matchAll(/(?:)/gu)].map(m=>m.index)`, `[..."a\\u{1F600}".matchAll(/(?:)/g)].map(m=>m.index)`,
  `var r=/a/y;"abab".split(r)`, `"ba".split(/a/y)`, `"aXbXc".split(/X/y)`, `var r=/a/gy;"baa".replace(r,"x")`, `var r=/a/gy;"aab".replace(r,"x")`, `var r=/a/y;"aab".replace(r,"x")`, `var r=/a/y;r.lastIndex=1;"aab".replace(r,"x")+r.lastIndex`,
  `var r=/a/y;r.lastIndex=1;"aab".replaceAll?.call("aab",/a/gy,"x")`, `var r=/a/y;r.lastIndex=1;[..."aab".matchAll(/a/gy)].length`, `[..."aab".matchAll(/a/y)].length`, `RegExp.prototype[Symbol.matchAll].call(/a/y,"aab").next().value.index`,
  `var r=/a/y;r.lastIndex=1;var it=r[Symbol.matchAll]("aaab");[it.next().value.index,r.lastIndex]`, `var r=/a/gy;r.lastIndex=1;var it=r[Symbol.matchAll]("aaab");[...it].length`,
  `var r=/a/g;r.lastIndex=3;"aaa".replace(r,"x")+r.lastIndex`, `var r=/a/g;r.lastIndex=3;"aaa".match(r).length+","+r.lastIndex`, `var r=/a/g;r.lastIndex=3;"aaa".search(r)+","+r.lastIndex`,
  `var r=/a/g;r.lastIndex=3;"aaa".split(r).length+","+r.lastIndex`, `var r=/(a)/g;r.lastIndex=2;M(r.exec("aaa"))+r.lastIndex`, `var r=/b/y;r.lastIndex=2;[r.test("aab"),r.lastIndex]`,
];
add(...sticky.map(s => `T(()=>{${s.includes("return") ? s : s.replace(/;([^;]*)$/, ";return $1")}})`.replace("T(()=>{var", "T(()=>{var").replace(/^T\(\(\)=>\{(?!var)/, "T(()=>{return ")));

// ---- 8. Symbol.replace / split / match / matchAll / search personalizados.
add(
  `T(()=>"abc".replace({[Symbol.replace](s,r){return "R:"+s+":"+r}},"x"))`, `T(()=>"abc".replaceAll({[Symbol.replace](s,r){return "RA:"+s+":"+r},flags:"g"},"x"))`,
  `T(()=>"abc".replaceAll({[Symbol.replace](s,r){return "RA"},flags:"i"},"x"))`, `T(()=>"abc".replaceAll({[Symbol.replace](s,r){return "RA"},flags:undefined},"x"))`, `T(()=>"abc".replaceAll({[Symbol.replace](s,r){return "RA"},[Symbol.match]:true},"x"))`,
  `T(()=>"abc".split({[Symbol.split](s,l){return [s,l]}},3))`, `T(()=>"abc".split({[Symbol.split](s,l){return [s,l]}}))`, `T(()=>"abc".match({[Symbol.match](s){return "M"+s}}))`, `T(()=>"abc".matchAll({[Symbol.matchAll](s){return "MA"+s},flags:"g"}))`,
  `T(()=>"abc".search({[Symbol.search](s){return 42}}))`, `T(()=>"abc".replace({[Symbol.replace]:null,toString(){return "b"}},"x"))`, `T(()=>"abc".replace({[Symbol.replace]:undefined,toString(){return "b"}},"x"))`,
  `T(()=>"abc".replace({[Symbol.replace]:1},"x"))`, `T(()=>"abc".split({[Symbol.split]:"x"},1))`, `T(()=>"abc".matchAll({flags:"i"}))`, `T(()=>"abc".matchAll({flags:"g",[Symbol.matchAll]:undefined,toString(){return "b"}}))`,
  `T(()=>{var r=/b/g;r.exec=function(s){this.lastIndex=0;return null};return "abc".replace(r,"x")})`, `T(()=>{var r=/b/g;var n=0;r.exec=function(s){return n++?null:{index:1,0:"b",length:1}};return "abc".replace(r,"[$&]")})`,
  `T(()=>{var r=/b/;r.exec=function(s){return {index:1,0:"b",length:1,groups:{k:"v"}}};return "abc".replace(r,"[$<k>]")})`, `T(()=>{var r=/b/;r.exec=function(s){return {index:5,0:"b",length:1}};return "abc".replace(r,"[$&]")})`,
  `T(()=>{var r=/b/;r.exec=function(s){return {index:-3,0:"b",length:1}};return "abc".replace(r,"[$&]")})`, `T(()=>{var r=/b/;r.exec=function(s){return 1};return "abc".replace(r,"x")})`, `T(()=>{var r=/b/;r.exec=function(s){return {index:1,length:0}};return "abc".replace(r,"x")})`,
  `T(()=>{var r=/b/;r.exec=function(s){return {index:1,length:1,0:{toString(){return "bb"}}}};return "abc".replace(r,"[$&]")})`, `T(()=>{var r=/b/;r.exec=()=>null;return [r.test("b"),"b".match(r),"b".search(r)]})`,
  `T(()=>{var r=/b/;r.exec=()=>({});return [r.test("b"),"b".search(r)]})`, `T(()=>{var r=/b/;r.exec=()=>undefined;return r.test("b")})`, `T(()=>{var r=/b/;r.exec=()=>1;return r.test("b")})`,
  `T(()=>{var r=/b/g;var log=[];var o=r.exec;r.exec=function(s){log.push(this.lastIndex);return o.call(this,s)};"abcb".replace(r,"x");return log})`,
  `T(()=>{var log=[];var r=new Proxy(/b/g,{get(t,k,rc){log.push(String(k));var v=Reflect.get(t,k,t);return typeof v==="function"?v.bind(t):v}});RegExp.prototype[Symbol.replace].call(r,"abcb","x");return log})`,
  `T(()=>{var log=[];var r=new Proxy(/b/y,{get(t,k,rc){log.push(String(k));var v=Reflect.get(t,k,t);return typeof v==="function"?v.bind(t):v}});RegExp.prototype[Symbol.split].call(r,"abcb");return log})`,
  `T(()=>{var log=[];var r=new Proxy(/b/,{get(t,k,rc){log.push(String(k));var v=Reflect.get(t,k,t);return typeof v==="function"?v.bind(t):v}});RegExp.prototype[Symbol.match].call(r,"abcb");return log})`,
  `T(()=>{var log=[];var r=new Proxy(/b/,{get(t,k,rc){log.push(String(k));var v=Reflect.get(t,k,t);return typeof v==="function"?v.bind(t):v}});RegExp.prototype[Symbol.search].call(r,"abcb");return log})`,
  `T(()=>{var log=[];var r=new Proxy(/b/g,{get(t,k,rc){log.push(String(k));var v=Reflect.get(t,k,t);return typeof v==="function"?v.bind(t):v}});RegExp.prototype[Symbol.matchAll].call(r,"abcb");return log})`,
  `T(()=>{class R extends RegExp{exec(s){var m=super.exec(s);if(m)m[0]=m[0].toUpperCase();return m}};return ["abc".replace(new R("b"),"[$&]"),new R("b").test("abc"),"abc".match(new R("b"))[0]]})`,
  `T(()=>{class R extends RegExp{static get [Symbol.species](){return RegExp}};return "a,b".split(new R(","))})`, `T(()=>{class R extends RegExp{constructor(p,f){super(p,f);this.made=1}};var r=new R(",");return "a,b".split(r).length+String(r.made)})`,
  `T(()=>{var n=0;class R extends RegExp{constructor(p,f){super(p,f);n++}};"a,b,c".split(new R(","));return n})`, `T(()=>{var seen;class R extends RegExp{constructor(p,f){super(p,f);seen=f}};"a,b".split(new R(","));return seen})`,
  `T(()=>{var seen;class R extends RegExp{constructor(p,f){super(p,f);seen=f}};"a,b".split(new R(",","u"));return seen})`, `T(()=>{var r=/b/;r.constructor={[Symbol.species]:function(p,f){return /c/y}};return "abcb".split(r)})`,
  `T(()=>{var r=/b/;r.constructor={[Symbol.species]:null};return "abcb".split(r)})`, `T(()=>{var r=/b/;r.constructor=undefined;return "abcb".split(r)})`, `T(()=>{var r=/b/;r.constructor=1;return "abcb".split(r)})`,
  `T(()=>{var r=/b/g;r.constructor={[Symbol.species]:function(p,f){return /b/g}};return [..."abcb".matchAll(r)].length})`, `T(()=>{var r=/b/g;r.constructor={[Symbol.species]:function(p,f){return {exec(){return null},lastIndex:0}}};return [..."abcb".matchAll(r)].length})`,
  `T(()=>{var r=/b/g;Object.defineProperty(r,"flags",{value:"g"});return "abcb".replace(r,"x")})`, `T(()=>{var r=/b/g;Object.defineProperty(r,"flags",{value:""});return "abcb".replace(r,"x")})`,
  `T(()=>{var r=/b/gy;Object.defineProperty(r,"flags",{value:"gy"});return "abcb".replace(r,"x")})`, `T(()=>{var r=/b/g;Object.defineProperty(r,"global",{value:false});return "abcb".replace(r,"x")})`,
  `T(()=>{var r=/b/g;Object.defineProperty(r,"global",{value:true});return "abcb".replace(r,"x")})`, `T(()=>{var r=/b/g;Object.defineProperty(r,"unicode",{value:true});return "\\u{1F600}b".replace(r,"x")})`,
  `T(()=>{var r=/b/;r.flags=undefined;return "abcb".replaceAll(r,"x")})`, `T(()=>{var r=/b/g;Object.defineProperty(r,"flags",{value:undefined});return "abcb".replaceAll(r,"x")})`,
  `T(()=>"abcb".replaceAll(/b/,"x"))`, `T(()=>"abcb".matchAll(/b/))`, `T(()=>RegExp.prototype[Symbol.matchAll].call(1,"a"))`, `T(()=>RegExp.prototype[Symbol.replace].call(1,"a","b"))`,
  `T(()=>RegExp.prototype[Symbol.split].call({},"a"))`, `T(()=>RegExp.prototype[Symbol.match].call({exec(){return null},flags:"",toString(){return "x"}},"a"))`,
  `T(()=>RegExp.prototype[Symbol.match].call({exec(){return null},flags:"g",lastIndex:0},"a"))`, `T(()=>RegExp.prototype[Symbol.search].call({lastIndex:3,exec(){return {index:7}}},"a"))`,
  `T(()=>{var o={lastIndex:3,exec(){this.lastIndex=9;return null}};var r=RegExp.prototype[Symbol.search].call(o,"a");return [r,o.lastIndex]})`, `T(()=>RegExp.prototype.test.call({exec(){return {}}},"a"))`,
  `T(()=>RegExp.prototype.test.call({exec(){return null}},"a"))`, `T(()=>RegExp.prototype.test.call({exec:1},"a"))`, `T(()=>RegExp.prototype.exec.call({},"a"))`, `T(()=>RegExp.prototype.test.call(1,"a"))`,
  `T(()=>RegExp.prototype.toString.call({source:"a",flags:"b"}))`, `T(()=>RegExp.prototype.toString.call({}))`, `T(()=>RegExp.prototype.toString.call(1))`, `T(()=>RegExp.prototype.compile.call({},"a"))`,
  `T(()=>{var r=/a/g;r.lastIndex=3;r.compile("b","i");return [String(r),r.lastIndex]})`, `T(()=>{var r=/a/;return r.compile(/b/i)===r&&String(r)})`, `T(()=>/a/.compile(/b/,"i"))`, `T(()=>/a/.compile(undefined))`, `T(()=>String(/a/.compile(undefined,undefined)))`,
  `T(()=>String(/a/.compile("(",)))`, `T(()=>{class R extends RegExp{};return new R("a").compile("b")})`, `T(()=>RegExp.prototype.compile.call(/a/,/b/,"i"))`,
);

// ---- 9. RegExp.escape.
for (const s of ["", "a", "abc", "a.b", "1abc", "9", "a1", "_", "-", " ", "\n", "\t", "\u2028", "\ufeff", "\u00a0", "^$\\.*+?()[]{}|/", "a,b", "a=b", "a<b>c", "a:b", "a!b", "a#b", "a%b", "a&b", "a'b", 'a"b', "a;b", "a@b", "a`b", "a~b", "é", "\u{1F600}", "\ud800", "\udc00", "\ud800a", "a\udc00", "\u0000", "\u007f", "Z", "z", "0", "az09_AZ", "a b"]) {
  add(`T(()=>RegExp.escape(${q(s)}))`, `T(()=>new RegExp(RegExp.escape(${q(s)})).test(${q(s)}))`, `T(()=>new RegExp(RegExp.escape(${q(s)}),"u").test(${q(s)}))`);
}
add(`T(()=>RegExp.escape(1))`, `T(()=>RegExp.escape(undefined))`, `T(()=>RegExp.escape(null))`, `T(()=>RegExp.escape({}))`, `T(()=>RegExp.escape(Symbol()))`, `T(()=>RegExp.escape(["a"]))`, `T(()=>RegExp.escape.length)`,
  `T(()=>RegExp.escape.name)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp,"escape").enumerable)`, `T(()=>RegExp.escape(new String("a.b")))`, `T(()=>RegExp.escape())`, `T(()=>new RegExp(RegExp.escape("a-b"),"v").test("a-b"))`,
  `T(()=>"a.b".replace(new RegExp(RegExp.escape("."),"g"),"X"))`, `T(()=>RegExp.escape("\\ud83d\\ude00"))`);

// ---- 10. Modifiers inline.
for (const [p, s] of [
  ["(?i:a)b", "Ab"], ["(?i:a)b", "AB"], ["(?i:a)b", "aB"], ["(?-i:a)b", "ab"], ["a(?i:b)c", "aBc"], ["a(?i:b)c", "aBC"], ["(?i-s:.)", "A"], ["(?s:.)", "\n"], ["(?-s:.)", "\n"], ["(?m:^b)", "a\nb"],
  ["(?-m:^b)", "a\nb"], ["(?m-i:^b$)", "a\nB"], ["(?i)a", "a"], ["(?:(?i)a)", "a"], ["(?ii:a)", "a"], ["(?i-i:a)", "a"], ["(?-:a)", "a"], ["(?:a)", "a"], ["(?i-:a)", "a"], ["(?-i-s:a)", "a"], ["(?g:a)", "a"],
  ["(?y:a)", "a"], ["(?u:a)", "a"], ["(?v:a)", "a"], ["(?d:a)", "a"], ["(?I:a)", "a"], ["(?is:A.)", "a\n"], ["(?is-m:A.)", "a\n"], ["(?i:\\w)", "\u017f"], ["(?i:[a-c])", "B"], ["(?i:[^a])", "A"],
  ["(?i:(a)\\1)", "aA"], ["(?i:(?<n>a)\\k<n>)", "aA"], ["(?i:a)|b", "A"], ["(?i:a|b)c", "Bc"], ["(?i:a|b)c", "BC"], ["(?-i:a)", "A"], ["(?-i:a)", "a"], ["(?i:(?-i:a))", "A"], ["(?i:(?-i:a)b)", "aB"],
  ["(?i:(?-i:a)b)", "AB"], ["(?i:\\u{41})", "a"], ["(?i:\\p{Lu})", "a"], ["(?i:\\P{Lu})", "A"], ["(?i:[\\p{Lu}])", "a"], ["(?i:[\\q{a}])", "A"], ["(?i:ß)", "SS"], ["(?i:k)", "\u212a"], ["(?i:k)", "K"],
  ["(?i:\\u212a)", "k"], ["(?s:a.b)", "a\nb"], ["x(?s:.)y", "x\ny"], ["x(?s:.)y", "x\ry"], ["(?s:.)*", "\n\n"], ["(?m:$)", "a\nb"], ["(?m:$)\\n", "a\nb"], ["(?:(?i:a)b)+", "AbAb"], ["(?i:a(?-i:b))", "Ab"], ["(?i:a(?-i:b))", "AB"],
  ["(?i:(?:a))", "A"], ["(?i:(a))", "A"], ["(?i:(?=a))", "A"], ["(?i:(?<=a))b", "ab"], ["(?i:(?=a))A", "A"], ["(?i:(?!a))A", "A"], ["(?i:a{2})", "aA"], ["(?i:a)+", "aA"], ["(?i:a)*?", "aA"], ["(?i:)", ""], ["(?i-s", "a"],
  ["(?i", "a"], ["(?", "a"], ["(?i:", "a"], ["(?i:a", "a"], ["(?i-:", "a"], ["(?-", "a"], ["(?i-s:a)", "a"], ["(?i-i-s:a)", "a"], ["(?s-s:a)", "a"], ["(?ims:a)", "a"], ["(?ims-ims:a)", "a"], ["(?imsi:a)", "a"],
  ["(?-ims:a)", "a"], ["(?-imsi:a)", "a"], ["(?iv:a)", "a"], ["(? i:a)", "a"], ["(?i :a)", "a"], ["(?i: a)", " a"],
]) for (const f of ["", "u", "v"]) exec(p, f, s);
add(`T(()=>/(?i:a)/.flags)`, `T(()=>/(?i:a)/.source)`, `T(()=>String(/(?i:a)/))`, `T(()=>/(?i:a)/i.test("A"))`, `T(()=>/(?-i:a)/i.test("A"))`, `T(()=>/(?-i:a)/i.test("a"))`, `T(()=>/(?s:.)/.test("\\n"))`, `T(()=>/(?-s:.)/s.test("\\n"))`);

// ---- 11. Mensagens de SyntaxError de padrão e flags inválidos.
const badPatterns = [
  "(", ")", "(a", "a)", "[", "[a", "[z-a]", "[\\d-z]", "[a-\\d]", "\\", "a\\", "*", "+", "?", "a**", "a{2,1}", "{", "}", "a{", "(?", "(?:", "(?=", "(?!", "(?<", "(?<=", "(?<!", "(?<n", "(?<n>", "(?<n>a", "(?<n>a)(?<n>b)",
  "(?x)", "(?P<n>a)", "(?#c)", "\\k<n>(?<m>a)", "(?<n>a)\\k<m>", "\\1(a)", "(a)\\2", "\\c", "\\cA", "\\c1", "\\u", "\\u{", "\\u{}", "\\u{110000}", "\\u{1F600", "\\x", "\\xg", "\\x4", "\\0", "\\00", "\\01", "\\8", "\\9",
  "\\-", "\\a", "\\e", "\\_", "\\ ", "[\\-]", "[\\c]", "[\\cA]", "[\\c1]", "[\\_]", "[\\1]", "[\\8]", "[\\k]", "[\\k<a>]", "\\k", "\\k<", "\\k<a", "\\k<>", "\\k<1>", "(?<a>.)\\k<a", "(?<a>.)\\k<>", "^*", "$*", "\\b*", "\\B+", "(?=a)?", "(?!a){1}", "(?<=a)?", "(?<!a)*",
  "\\p{L}", "\\P{L}", "\\u{1F600}", "[\\u{1F600}]", "\\-", "\\/", "\\!", "\\$", "\\^", "\\ud83d\\ude00", "\\ud83d", "[\\ud83d\\ude00]", "a|*", "|*", "(|*)", "(?:*)", "(?:+)", "x{1}{2}", "x{1}*", "x{1}?", "x{1}??",
  "(?<a>)(?<a>)", "(?<a>a)|(?<a>b)", "(?<a>a)(?:(?<a>b))", "(?:(?<a>a)|(?<b>b))(?<a>c)", "(?:(?<a>a)(?<a>b)|c)", "(?:(?<a>a)|b)(?<a>c)", "(?:(?<a>a)|(?<b>b))|(?<a>c)", "((?<a>a)|(?<a>b))",
  "[[a]", "[a&&]", "[a&&&&b]", "[a--]", "[(]", "[a[]]", "[\\q{a}]", "\\q", "[\\p{L}--]", "[^\\q{ab}]",
];
for (const p of badPatterns) for (const f of ["", "u", "v"]) ctor(p, f);
for (const f of ["gg", "ii", "mm", "ss", "uu", "yy", "dd", "vv", "x", "G", "gimsuyd!", "gimsuyvd", "uv", "vu", "a", " g", "g ", "g\u0000", "\u0067", "\u{1F600}", "undefined", "null", "1", "", "dgimsuy", "ydgimsu", "gimsvy"]) {
  add(`T(()=>String(new RegExp("a",${q(f)})))`, `T(()=>new RegExp("a",${q(f)}).flags)`);
}
add(`T(()=>new RegExp("a",undefined).flags)`, `T(()=>new RegExp("a",null).flags)`, `T(()=>new RegExp("a",1).flags)`, `T(()=>new RegExp("a",{toString(){return "g"}}).flags)`, `T(()=>new RegExp("a",Symbol()))`,
  `T(()=>new RegExp(Symbol()))`, `T(()=>new RegExp(undefined)+"")`, `T(()=>new RegExp(null)+"")`, `T(()=>new RegExp("")+"")`, `T(()=>new RegExp("\\n")+"")`, `T(()=>new RegExp("/")+"")`, `T(()=>new RegExp("\\\\/")+"")`, `T(()=>new RegExp("[/]")+"")`,
  `T(()=>new RegExp("\\u2028")+"")`, `T(()=>new RegExp("\\r\\n")+"")`, `T(()=>new RegExp("\\\\\\n")+"")`, `T(()=>new RegExp("\\\\\\n").source)`, `T(()=>new RegExp("\\n").source)`, `T(()=>RegExp("a")===RegExp("a"))`,
  `T(()=>{var r=/a/;return RegExp(r)===r})`, `T(()=>{var r=/a/;return RegExp(r,"g")===r})`, `T(()=>{var r=/a/;return new RegExp(r)===r})`, `T(()=>{var r=/a/g;return new RegExp(r,"i").flags})`, `T(()=>{var r=/a/g;return new RegExp(r).flags})`,
  `T(()=>{var r=/a/g;r.constructor=Object;return RegExp(r)===r})`, `T(()=>{var r={[Symbol.match]:true,source:"b",flags:"i",constructor:RegExp};return RegExp(r)===r})`, `T(()=>{var r={[Symbol.match]:true,source:"b",flags:"i"};return String(new RegExp(r))})`,
  `T(()=>{var r={[Symbol.match]:true,source:"b",flags:"i"};return String(new RegExp(r,"g"))})`, `T(()=>{var r={[Symbol.match]:false,toString(){return "z"}};return String(new RegExp(r))})`, `T(()=>RegExp.prototype.source)`, `T(()=>RegExp.prototype.toString())`,
  `T(()=>RegExp.prototype.global)`, `T(()=>RegExp.prototype.hasIndices)`, `T(()=>RegExp.prototype.sticky)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"global").get.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"source").get.call({}))`,
  `T(()=>RegExp.prototype.exec.call(RegExp.prototype,"a"))`, `T(()=>RegExp.prototype.test.call(RegExp.prototype,"a"))`, `T(()=>RegExp.prototype.test.call(Object.create(/a/),"a"))`);

// ---- 12. Escapes, classes e bordas variadas.
for (const [p, f, s] of [
  ["\\cJ", "", "\n"], ["[\\cJ]", "", "\n"], ["\\c", "", "\\c"], ["[\\c]", "", "c"], ["[\\c_]", "", "\u001f"], ["[\\c1]", "", "\u0011"], ["\\c1", "", "\\c1"], ["\\8", "", "8"], ["\\1", "", "\u0001"], ["(a)\\1", "", "aa"],
  ["\\10", "", "\u0008"], ["(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\\10", "", "abcdefghijj"], ["\\u{61}", "", "u".repeat(61)], ["\\u{61}", "u", "a"], ["\\x6", "", "x6"], ["\\u006", "", "u006"], ["[\\b]", "", "\b"],
  ["a|", "", "b"], ["|a", "", "a"], ["()", "", "x"], ["(|)", "", "x"], ["[]", "", "a"], ["[^]", "", "\n"], ["[]a", "", "a"], ["[^]a", "", "ba"], ["[]", "v", "a"], ["[^]", "v", "\n"], ["\\0", "", "\u0000"], ["\\00", "", "\u0000"],
  ["\\07", "", "\u0007"], ["\\08", "", "\u00008"], ["[\\0]", "", "\u0000"], ["\\377", "", "\u00ff"], ["\\400", "", "\u00200"], ["^$", "", ""], ["^$", "m", "\n"], ["^", "gm", "a\nb"], ["$", "gm", "a\nb"], ["^", "gm", "a\r\nb"],
  ["^b", "m", "a\u2028b"], ["^b", "m", "a\u2029b"], ["a$", "m", "a\u2028"], [".", "", "\u2028"], [".", "", "\u0085"], [".", "s", "\u2028"], ["\\s", "", "\ufeff"], ["\\s", "", "\u180e"], ["\\s", "", "\u2028"], ["\\s", "", "\u200b"],
  ["\\S+", "", "a\u00a0b"], ["\\w+", "", "aé_1"], ["\\w+", "i", "a\u017f\u212a"], ["\\w+", "iu", "a\u017f\u212a"], ["\\W", "iu", "\u017f"], ["\\d+", "u", "٣1"], ["\\b.", "g", "ab cd"], ["\\B.", "g", "ab cd"],
  ["\\bé", "", "é"], ["\\Bé", "", "é"], ["[\\w-]+", "", "a-b"], ["[a-]+", "", "a-"], ["[-a]+", "", "-a"], ["[\\d-x]+", "", "1-x"], ["[\\d-x]+", "u", "1-x"], ["[a-a]", "", "a"], ["[\\u{61}-\\u{63}]", "u", "b"],
  ["[\\ud83d\\ude00]", "", "\ude00"], ["[\\ud83d\\ude00]", "u", "\u{1F600}"], ["[\\ud83d\\ude00]", "u", "\ude00"], ["\\ud83d\\ude00", "u", "\u{1F600}"], ["\\ud83d", "u", "\u{1F600}"], ["\\ud83d", "", "\u{1F600}"], ["^.$", "u", "\u{1F600}"],
  ["^.$", "", "\u{1F600}"], ["^..$", "", "\u{1F600}"], ["\u{1F600}+", "u", "\u{1F600}\u{1F600}"], ["\u{1F600}+", "", "\u{1F600}\u{1F600}"], ["[\u{1F600}-\u{1F64F}]", "u", "\u{1F601}"], ["[^\u{1F600}]", "u", "\u{1F601}"],
  ["\\u{1F600}", "u", "\u{1F600}"], ["\\u{10FFFF}", "u", "\u{10FFFF}"], ["\\u{0000000061}", "u", "a"], ["[\\u{1F600}-\\u{1F5FF}]", "u", "a"], ["\ud83d", "u", "\ud83d"], ["\ud83d", "u", "\u{1F600}"], ["\ude00", "u", "\u{1F600}"],
  ["^\ude00", "u", "\ude00"], ["(?<=\ud83d)\ude00", "", "\u{1F600}"], ["(?<=\ud83d)\ude00", "u", "\u{1F600}"], ["\\k<a>", "u", "k<a>"], ["\\-", "u", "-"], ["[\\-]", "u", "-"], ["\\/", "u", "/"], ["\\p{L}", "", "p{L}"],
  ["a{1,2}", "", "aaa"], ["a{,2}", "", "a{,2}"], ["a{ 1}", "", "a{ 1}"], ["x{1}", "u", "x"], ["]", "", "]"], ["}", "", "}"], ["{", "", "{"], ["a{", "", "a{"], ["{1}", "", "{1}"], ["x{", "", "x{"], ["\\{", "u", "{"],
  ["(?:)", "g", "ab"], ["(?:)*", "", "a"], ["(?:a?)*?b", "", "b"], ["(a)|b", "", "b"], ["(?:(a)|b)\\1", "", "b"], ["(?:(a)|b)\\1", "", "ba"], ["\\1(a)", "", "a"], ["(\\1a)", "", "a"], ["(a\\1)", "", "a"], ["(?<n>\\k<n>a)", "", "a"],
  ["^(?:(a)|b)*$", "", "ab"], ["^(?:(a)|b)*$", "", "ba"], ["(?:(a)|b)*", "", "ab"], ["(a)?\\1b", "", "b"], ["(?<n>a)?\\k<n>b", "", "b"], ["(a)?(?:\\1)b", "", "b"],
]) exec(p, f, s);

// ---- 13. matchAll, replace e split com padrões de borda.
add(
  `T(()=>[..."a1b22c333".matchAll(/\\d+/g)].map(m=>m[0]+"@"+m.index))`, `T(()=>[..."aaa".matchAll(/a*?/g)].map(m=>m.index))`, `T(()=>[..."aaa".matchAll(/(?<=a)/g)].map(m=>m.index))`,
  `T(()=>"2024-05-06".replace(/(?<y>\\d+)-(?<m>\\d+)-(?<d>\\d+)/,"$<d>/$<m>/$<y>"))`, `T(()=>"ab".replace(/(?<x>a)/,"$<y>|$<x>|$<"))`, `T(()=>"ab".replace(/(a)/,"$<x>"))`, `T(()=>"ab".replace(/(?<x>a)/,"$<x"))`,
  `T(()=>"ab".replace(/(?<x>a)/,"$<>"))`, `T(()=>"ab".replace(/(a)(b)?/,"$2|$1|$3|$01|$10|$00|$0"))`, `T(()=>"abcdefghijk".replace(/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)/,"$11|$10|$1|$011"))`,
  `T(()=>"abc".replace(/b/,"$\`|$'|$&|$$|$"))`, `T(()=>"abc".replace(/(?<n>b)/,(m,p1,off,str,g)=>[m,p1,off,str,JSON.stringify(g)].join("/")))`, `T(()=>"abc".replace(/(b)/,(...a)=>a.length))`,
  `T(()=>"abc".replace(/(?<n>b)/,(...a)=>a.length))`, `T(()=>"aaa".replace(/a/g,(m,i)=>i))`, `T(()=>"aaa".replace(/a/y,"b"))`, `T(()=>"abc".replace(/(?:)/g,"-"))`, `T(()=>"abc".replace(/$/g,"-"))`, `T(()=>"abc".replace(/^/g,"-"))`,
  `T(()=>"a\\nb".replace(/^/gm,"> "))`, `T(()=>"a\\nb".replace(/$/gm,";"))`, `T(()=>"abc".split(/(b)/))`, `T(()=>"abc".split(/(?:b)/))`, `T(()=>"abc".split(/(x)?b/))`, `T(()=>"abc".split(/b/,0))`, `T(()=>"abc".split(/b/,1))`,
  `T(()=>"abc".split(/b/,-1))`, `T(()=>"abc".split(/b/,2**32+1))`, `T(()=>"abc".split(/b/,undefined))`, `T(()=>"".split(/b/))`, `T(()=>"".split(/(?:)/))`, `T(()=>"".split(/^/))`, `T(()=>"abc".split(/$/))`, `T(()=>"abc".split(/^/))`,
  `T(()=>"a\\nb".split(/^/m))`, `T(()=>"ab".split(/(?=b)/))`, `T(()=>"ab".split(/(?<=a)/))`, `T(()=>"test".split(/(?:)/u))`, `T(()=>"A<B>bold</B>".split(/<(\\/)?([^<>]+)>/))`, `T(()=>"abc".split(/(?<n>b)/))`,
  `T(()=>"ab".split(/a*?/))`, `T(()=>"ab".split(/a*/))`, `T(()=>"ab".split(/a*?/,1))`, `T(()=>"\\u{1F600}".split(/(?:)/v).length)`, `T(()=>"abc".split({[Symbol.split]:undefined}))`,
  `T(()=>"abc".search(/c/))`, `T(()=>"abc".search(/x/))`, `T(()=>"abc".search())`, `T(()=>"a.c".search("."))`, `T(()=>"abc".match(/(?<n>b)/).groups.n)`, `T(()=>"abc".match(/b/g))`, `T(()=>"abc".match(/x/g))`, `T(()=>"abc".match())`,
  `T(()=>M("abc".match(/(?:)/)))`, `T(()=>"aaa".lastIndexOf("a",-1))`, `T(()=>/a/[Symbol.replace]("aaa","b"))`, `T(()=>/a/g[Symbol.replace]("aaa","b"))`, `T(()=>/a/g[Symbol.match]("aaa"))`, `T(()=>/a/[Symbol.split]("bab"))`,
  `T(()=>Object.getOwnPropertyNames(RegExp.prototype).sort())`, `T(()=>Object.getOwnPropertySymbols(RegExp.prototype).map(String))`, `T(()=>RegExp.prototype[Symbol.matchAll].name)`, `T(()=>RegExp.prototype[Symbol.replace].length)`,
  `T(()=>RegExp[Symbol.species]===RegExp)`, `T(()=>Object.prototype.toString.call(/a/[Symbol.matchAll]("a")))`, `T(()=>Object.prototype.toString.call(/a/g[Symbol.matchAll]("a")))`, `T(()=>/a/g[Symbol.matchAll]("a").next.name)`,
  `T(()=>Object.getPrototypeOf(/a/g[Symbol.matchAll]("a"))[Symbol.toStringTag])`, `T(()=>{var it=/a/g[Symbol.matchAll]("aa");it.next();it.next();return JSON.stringify(it.next())})`,
  `T(()=>{var it=/a/g[Symbol.matchAll]("aa");return it[Symbol.iterator]()===it})`, `T(()=>{var it=/a/g[Symbol.matchAll]("aa");return it.next.call({})})`,  `T(()=>{/(a)(b)/.exec("xab");return [RegExp.$1,RegExp.$2,RegExp.$3,RegExp.lastMatch,RegExp["$&"],RegExp.leftContext,RegExp.rightContext,RegExp.lastParen,RegExp["$+"]]})`,
);

// ---- Saída: descarta duplicados, executa no bun e grava fonte e resultado.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const baseFile = path.join(__dirname, "..", "tests", "golden", "regexp_edge_bun.tsv");
void fs; void baseFile;
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + (/^T\(/.test(expr) ? `globalThis.R = ${expr}` : `globalThis.R = T(()=>{return ${expr}})`);
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) {
    dropped++;
    process.stderr.write("caminho no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
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
process.stdout.write(emitFactored("regexp_edge", rows));
